//! Port abstraction for transport layer access.
//!
//! Port provides lightweight access to transport Tx/Rx handles,
//! scratch buffers, tap rings, and state tables.

use core::{
    cell::UnsafeCell,
    marker::PhantomData,
    task::{Context, Poll},
};

use super::tables::{LoopDisposition, LoopTable, RouteTable, VmCapsTable};
use crate::{
    control::cap::mint::CapsMask,
    control::types::{Lane, RendezvousId},
    eff,
    epf::host::HostSlots,
    global::const_dsl::ScopeId,
    observe::core::{TapRing, emit},
    runtime::config::Clock,
    transport::{Transport, TransportEvent, TransportEventKind, TransportMetrics},
};

// Drain hints defensively: transport hint queues may be sized to MAX_EFF_NODES.
const ROUTE_HINT_SLOTS: usize = eff::meta::MAX_EFF_NODES;

#[derive(Clone, Copy)]
struct RouteHintQueue {
    len: u16,
    present_mask: u128,
    buf: [u8; ROUTE_HINT_SLOTS],
}

impl RouteHintQueue {
    const fn new() -> Self {
        Self {
            len: 0,
            present_mask: 0,
            buf: [0; ROUTE_HINT_SLOTS],
        }
    }

    #[inline]
    const fn label_bit(label: u8) -> u128 {
        if label < u128::BITS as u8 {
            1u128 << label
        } else {
            0
        }
    }

    fn push(&mut self, label: u8) -> bool {
        if label == 0 {
            return false;
        }
        let label_bit = Self::label_bit(label);
        if (self.present_mask & label_bit) != 0 {
            return false;
        }
        let len = self.len as usize;
        let cap = ROUTE_HINT_SLOTS;
        if len >= cap {
            let dropped = self.buf[0];
            self.present_mask &= !Self::label_bit(dropped);
            self.buf.copy_within(1..cap, 0);
            self.buf[cap - 1] = label;
            self.present_mask |= label_bit;
            return true;
        }
        self.buf[len] = label;
        self.len += 1;
        self.present_mask |= label_bit;
        true
    }

    fn take_matching<F>(&mut self, mut matches: F) -> Option<u8>
    where
        F: FnMut(u8) -> bool,
    {
        let len = self.len as usize;
        let mut idx = 0usize;
        while idx < len {
            let label = self.buf[idx];
            if matches(label) {
                if idx + 1 < len {
                    self.buf.copy_within((idx + 1)..len, idx);
                }
                self.len -= 1;
                self.present_mask &= !Self::label_bit(label);
                return Some(label);
            }
            idx += 1;
        }
        None
    }

    #[cfg(test)]
    fn has_matching<F>(&self, mut matches: F) -> bool
    where
        F: FnMut(u8) -> bool,
    {
        let len = self.len as usize;
        let mut idx = 0usize;
        while idx < len {
            if matches(self.buf[idx]) {
                return true;
            }
            idx += 1;
        }
        false
    }

    #[inline]
    fn has_any_label_in_mask(&self, label_mask: u128) -> bool {
        (self.present_mask & label_mask) != 0
    }

    fn take_from_label_mask(&mut self, label_mask: u128) -> Option<u8> {
        self.take_matching(|label| (Self::label_bit(label) & label_mask) != 0)
    }

    fn drain_from_transport<'a, T: Transport>(&mut self, transport: &'a T, rx: &'a T::Rx<'a>) {
        let mut budget = ROUTE_HINT_SLOTS;
        while budget > 0 {
            let label = match transport.recv_label_hint(rx) {
                Some(label) => label,
                None => break,
            };
            if !self.push(label) {
                break;
            }
            budget -= 1;
        }
    }

    #[inline]
    fn clear(&mut self) {
        self.len = 0;
        self.present_mask = 0;
    }
}

/// Lightweight port describing how an endpoint reaches the transport.
///
/// The port is intentionally `!Send`/`!Sync` (via the hidden `PhantomData`) so
/// that transport handles remain affine. Endpoint methods keep the mutable
/// borrow for the duration of a single `.await`, but exclusivity is preserved
/// because owning endpoints themselves are affine.
pub(crate) struct Port<
    'r,
    T: Transport,
    E: crate::control::cap::mint::EpochTable = crate::control::cap::mint::EpochTbl,
> {
    transport: &'r T,
    tx: UnsafeCell<T::Tx<'r>>,
    rx: UnsafeCell<T::Rx<'r>>,
    scratch: *mut [u8],
    scratch_marker: PhantomData<&'r mut [u8]>,
    host_slots: *const HostSlots<'static>,
    host_slots_marker: PhantomData<&'r HostSlots<'r>>,
    pub lane: Lane,
    role: u8,
    rv_id: RendezvousId,
    _no_send_sync: PhantomData<*mut ()>,
    tap: *const TapRing<'static>,
    tap_marker: PhantomData<&'r TapRing<'r>>,
    clock: &'r dyn Clock,
    loops: *const LoopTable,
    loops_marker: PhantomData<&'r LoopTable>,
    routes: *const RouteTable,
    routes_marker: PhantomData<&'r RouteTable>,
    vm_caps: *const VmCapsTable,
    vm_caps_marker: PhantomData<&'r VmCapsTable>,
    route_hints: UnsafeCell<RouteHintQueue>,
    _epoch: PhantomData<E>,
}

impl<'r, T: Transport, E: crate::control::cap::mint::EpochTable + 'r> Port<'r, T, E> {
    #[inline]
    fn sync_pending_route_hint_lane_masks(&self, before: u128, after: u128) {
        if before != after {
            self.route_table()
                .update_pending_hint_lane_masks(self.lane, before, after);
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new<'tap>(
        transport: &'r T,
        tap: &'tap TapRing<'tap>,
        clock: &'tap dyn Clock,
        vm_caps: &'tap VmCapsTable,
        loops: &'tap LoopTable,
        routes: &'tap RouteTable,
        host_slots: &'tap HostSlots<'tap>,
        scratch: *mut [u8],
        lane: Lane,
        role: u8,
        rv_id: RendezvousId,
        tx: T::Tx<'r>,
        rx: T::Rx<'r>,
    ) -> Self
    where
        'tap: 'r,
    {
        #[cfg(all(not(test), not(feature = "std")))]
        {
            let _ = tap;
            let _ = clock;
        }
        Self {
            transport,
            tx: UnsafeCell::new(tx),
            rx: UnsafeCell::new(rx),
            scratch,
            scratch_marker: PhantomData,
            host_slots: (host_slots as *const HostSlots<'tap>).cast::<HostSlots<'static>>(),
            host_slots_marker: PhantomData,
            lane,
            role,
            rv_id,
            _no_send_sync: PhantomData,
            tap: (tap as *const TapRing<'tap>).cast::<TapRing<'static>>(),
            tap_marker: PhantomData,
            clock,
            loops: loops as *const LoopTable,
            loops_marker: PhantomData,
            routes: routes as *const RouteTable,
            routes_marker: PhantomData,
            vm_caps: vm_caps as *const VmCapsTable,
            vm_caps_marker: PhantomData,
            route_hints: UnsafeCell::new(RouteHintQueue::new()),
            _epoch: PhantomData,
        }
    }

    pub(crate) fn transport(&self) -> &'r T {
        self.transport
    }

    #[inline]
    pub(crate) fn loop_table(&self) -> &LoopTable {
        unsafe { &*self.loops }
    }

    #[inline]
    pub(crate) fn route_table(&self) -> &RouteTable {
        unsafe { &*self.routes }
    }

    #[cfg(test)]
    #[inline]
    pub(crate) fn vm_caps_table(&self) -> &VmCapsTable {
        unsafe { &*self.vm_caps }
    }

    #[inline]
    pub(crate) fn record_loop_decision(&self, idx: u8, disposition: LoopDisposition) -> u16 {
        self.loop_table()
            .record(self.lane, self.role, idx, disposition)
    }

    #[inline]
    pub(crate) fn ack_loop_decision(&self, idx: u8, role: u8) {
        self.loop_table().acknowledge(self.lane, role, idx);
    }

    #[inline]
    pub(crate) fn record_route_decision(&self, scope: ScopeId, arm: u8) -> u16 {
        self.route_table().record(self.lane, self.role, scope, arm)
    }

    #[inline]
    pub(crate) fn poll_route_decision(
        &self,
        scope: ScopeId,
        role: u8,
        cx: &mut Context<'_>,
    ) -> Poll<u8> {
        self.route_table().poll(self.lane, role, scope, cx)
    }

    #[inline]
    pub(crate) fn ack_route_decision(&self, scope: ScopeId, role: u8) -> Option<u8> {
        self.route_table().acknowledge(self.lane, role, scope)
    }

    #[inline]
    pub(crate) fn peek_route_decision(&self, scope: ScopeId, role: u8) -> Option<u8> {
        self.route_table().peek(self.lane, role, scope)
    }

    #[inline]
    pub(crate) fn pending_route_decision_lane_mask(&self, scope: ScopeId, role: u8) -> u16 {
        self.route_table().pending_lane_mask(role, scope)
    }

    #[inline]
    pub(crate) fn route_change_epoch(&self) -> u32 {
        self.route_table().change_epoch()
    }

    #[cfg(test)]
    #[inline]
    pub(crate) fn has_route_hint_matching<F>(&self, matches: F) -> bool
    where
        F: FnMut(u8) -> bool,
    {
        let hints = unsafe { &mut *self.route_hints.get() };
        let before = hints.present_mask;
        let rx = unsafe { &*self.rx.get() };
        hints.drain_from_transport(self.transport, rx);
        self.sync_pending_route_hint_lane_masks(before, hints.present_mask);
        hints.has_matching(matches)
    }

    #[inline]
    pub(crate) fn has_route_hint_for_label_mask(&self, label_mask: u128) -> bool {
        let hints = unsafe { &mut *self.route_hints.get() };
        let before = hints.present_mask;
        let rx = unsafe { &*self.rx.get() };
        hints.drain_from_transport(self.transport, rx);
        self.sync_pending_route_hint_lane_masks(before, hints.present_mask);
        hints.has_any_label_in_mask(label_mask)
    }

    #[inline]
    pub(crate) fn pending_route_hint_lane_mask_for_label_mask(&self, label_mask: u128) -> u16 {
        let hints = unsafe { &mut *self.route_hints.get() };
        let before = hints.present_mask;
        let rx = unsafe { &*self.rx.get() };
        hints.drain_from_transport(self.transport, rx);
        self.sync_pending_route_hint_lane_masks(before, hints.present_mask);
        self.route_table()
            .pending_hint_lane_mask_for_labels(label_mask)
    }

    #[inline]
    pub(crate) fn take_route_hint_for_label_mask(&self, label_mask: u128) -> Option<u8> {
        let hints = unsafe { &mut *self.route_hints.get() };
        let before = hints.present_mask;
        let rx = unsafe { &*self.rx.get() };
        hints.drain_from_transport(self.transport, rx);
        let taken = hints.take_from_label_mask(label_mask);
        self.sync_pending_route_hint_lane_masks(before, hints.present_mask);
        taken
    }

    #[inline]
    pub(crate) fn clear_route_hints(&self) {
        let hints = unsafe { &mut *self.route_hints.get() };
        let before = hints.present_mask;
        hints.clear();
        self.sync_pending_route_hint_lane_masks(before, hints.present_mask);
    }

    #[inline]
    pub(crate) fn tx_ptr(&self) -> *mut T::Tx<'r> {
        self.tx.get()
    }

    #[inline]
    pub(crate) fn rx_ptr(&self) -> *mut T::Rx<'r> {
        self.rx.get()
    }

    #[inline]
    pub(crate) fn scratch_ptr(&self) -> *mut [u8] {
        self.scratch
    }

    #[inline]
    pub(crate) fn host_slots(&self) -> &HostSlots<'r> {
        unsafe { &*self.host_slots.cast::<HostSlots<'r>>() }
    }

    #[inline]
    pub(crate) fn tap(&self) -> &TapRing<'r> {
        unsafe { &*self.tap.cast::<TapRing<'r>>() }
    }

    #[inline]
    pub(crate) fn clock(&self) -> &dyn Clock {
        self.clock
    }

    #[inline]
    pub(crate) fn now32(&self) -> u32 {
        self.clock.now32()
    }

    #[inline]
    pub(crate) fn vm_caps(&self) -> &VmCapsTable {
        unsafe { &*self.vm_caps }
    }

    #[inline]
    pub(crate) fn caps_mask(&self) -> CapsMask {
        self.vm_caps().get(self.lane)
    }

    #[inline]
    pub(crate) fn flush_transport_events(&self) -> Option<TransportEvent> {
        use crate::observe::events;
        let tap = self.tap();
        let clock = self.clock();
        let mut last_loss = None;
        let mut emit_event = |event: TransportEvent| {
            let (arg0, arg1) = event.encode_tap_args();
            if matches!(event.kind, TransportEventKind::Loss) {
                last_loss = Some(event);
            }
            emit(tap, events::TransportEvent::new(clock.now32(), arg0, arg1));
        };
        self.transport.drain_events(&mut emit_event);
        let snapshot = self.transport.metrics().snapshot();
        if let Some(payload) = snapshot.encode_tap_metrics() {
            let (arg0, arg1) = payload.primary;
            emit(
                tap,
                events::TransportMetrics::new(clock.now32(), arg0, arg1),
            );
            if let Some((ext0, ext1)) = payload.extension {
                emit(
                    tap,
                    events::TransportMetricsExt::new(clock.now32(), ext0, ext1),
                );
            }
        }

        last_loss
    }

    #[inline]
    pub(crate) fn lane(&self) -> Lane {
        self.lane
    }

    #[inline]
    pub(crate) fn rv_id(&self) -> RendezvousId {
        self.rv_id
    }
}

#[cfg(test)]
mod tests {
    use super::RouteHintQueue;

    #[test]
    fn route_hint_unmatched_is_not_discarded_across_scope_selection() {
        let mut queue = RouteHintQueue::new();
        queue.push(41);
        queue.push(42);

        let first = queue.take_matching(|label| label == 99);
        assert_eq!(
            first, None,
            "non-matching take must not clear buffered hints"
        );

        let second = queue.take_matching(|label| label == 42);
        assert_eq!(
            second,
            Some(42),
            "later matching hint must remain available"
        );

        let third = queue.take_matching(|label| label == 41);
        assert_eq!(
            third,
            Some(41),
            "earlier unmatched hint must still be available after sibling selection"
        );
    }

    #[test]
    fn route_hint_queue_deduplicates_same_label() {
        let mut queue = RouteHintQueue::new();
        queue.push(25);
        queue.push(25);
        queue.push(25);

        let first = queue.take_matching(|label| label == 25);
        assert_eq!(first, Some(25));

        let second = queue.take_matching(|label| label == 25);
        assert_eq!(second, None, "duplicate labels must be coalesced in queue");
    }

    #[test]
    fn route_hint_has_matching_is_non_consuming() {
        let mut queue = RouteHintQueue::new();
        queue.push(25);
        queue.push(41);

        assert!(queue.has_matching(|label| label == 41));
        assert_eq!(queue.take_matching(|label| label == 41), Some(41));
        assert_eq!(queue.take_matching(|label| label == 25), Some(25));
    }
}
