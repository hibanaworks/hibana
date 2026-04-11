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
    control::types::{Lane, RendezvousId, SessionId},
    endpoint::kernel::FrontierScratchLayout,
    epf::{Action, PolicyMode, host::HostSlots, vm::Slot, vm::VmCtx},
    global::const_dsl::ScopeId,
    observe::core::{TapEvent, TapRing, emit},
    runtime::config::Clock,
    transport::{Transport, TransportEvent, TransportEventKind, TransportMetrics},
};

// Hint queues only need to cover the tracked label universe.
const ROUTE_HINT_SLOTS: usize = u128::BITS as usize;

#[derive(Clone, Copy)]
struct RouteHintQueue {
    present_mask: u128,
}

impl RouteHintQueue {
    #[cfg(test)]
    const fn new() -> Self {
        Self { present_mask: 0 }
    }

    const fn from_mask(present_mask: u128) -> Self {
        Self { present_mask }
    }

    fn push(&mut self, label: u8) -> bool {
        if label == 0 || label >= u128::BITS as u8 {
            return false;
        }
        let bit = 1u128 << label;
        if (self.present_mask & bit) != 0 {
            return false;
        }
        self.present_mask |= bit;
        true
    }

    fn take_matching<F>(&mut self, mut matches: F) -> Option<u8>
    where
        F: FnMut(u8) -> bool,
    {
        let mut remaining = self.present_mask;
        while remaining != 0 {
            let label = remaining.trailing_zeros() as u8;
            if matches(label) {
                self.present_mask &= !(1u128 << label);
                return Some(label);
            }
            remaining &= remaining - 1;
        }
        None
    }

    #[cfg(test)]
    fn has_matching<F>(&self, mut matches: F) -> bool
    where
        F: FnMut(u8) -> bool,
    {
        let mut remaining = self.present_mask;
        while remaining != 0 {
            let label = remaining.trailing_zeros() as u8;
            if matches(label) {
                return true;
            }
            remaining &= remaining - 1;
        }
        false
    }

    #[inline]
    fn has_any_label_in_mask(&self, label_mask: u128) -> bool {
        (self.present_mask & label_mask) != 0
    }

    fn take_from_label_mask(&mut self, label_mask: u128) -> Option<u8> {
        self.take_matching(|label| (label_mask & (1u128 << label)) != 0)
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
    slab: *mut [u8],
    image_frontier: *const u32,
    scratch_reserved_bytes: *const u32,
    endpoint_leases: *const super::core::EndpointLeaseSlot,
    endpoint_lease_capacity: super::core::EndpointLeaseId,
    scratch_marker: PhantomData<&'r mut [u8]>,
    #[cfg(test)]
    host_slots: *const HostSlots<'static>,
    #[cfg(test)]
    host_slots_marker: PhantomData<&'r HostSlots<'r>>,
    pub lane: Lane,
    role: u8,
    role_count: u8,
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
    _epoch: PhantomData<E>,
}

impl<'r, T: Transport, E: crate::control::cap::mint::EpochTable + 'r> Port<'r, T, E> {
    #[inline(always)]
    const fn align_up(value: usize, align: usize) -> usize {
        let mask = align.saturating_sub(1);
        (value + mask) & !mask
    }

    #[inline(always)]
    const fn frontier_scratch_align() -> usize {
        FrontierScratchLayout::new(0).total_align()
    }

    #[inline]
    fn sync_pending_route_hint_lane_masks(&self, before: u128, after: u128) {
        if before != after {
            self.route_table()
                .update_pending_hint_lane_masks(self.lane, before, after);
        }
    }

    #[inline]
    fn route_hints_from_table(&self) -> RouteHintQueue {
        RouteHintQueue::from_mask(self.route_table().pending_hint_labels_for_lane(self.lane))
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
        slab: *mut [u8],
        image_frontier: *const u32,
        scratch_reserved_bytes: *const u32,
        endpoint_leases: *const super::core::EndpointLeaseSlot,
        endpoint_lease_capacity: super::core::EndpointLeaseId,
        lane: Lane,
        role: u8,
        role_count: u8,
        rv_id: RendezvousId,
        tx: T::Tx<'r>,
        rx: T::Rx<'r>,
    ) -> Self
    where
        'tap: 'r,
    {
        #[cfg(not(test))]
        let _ = host_slots;
        #[cfg(all(not(test), not(feature = "std")))]
        {
            let _ = tap;
            let _ = clock;
        }
        Self {
            transport,
            tx: UnsafeCell::new(tx),
            rx: UnsafeCell::new(rx),
            slab,
            image_frontier,
            scratch_reserved_bytes,
            endpoint_leases,
            endpoint_lease_capacity,
            scratch_marker: PhantomData,
            #[cfg(test)]
            host_slots: (host_slots as *const HostSlots<'tap>).cast::<HostSlots<'static>>(),
            #[cfg(test)]
            host_slots_marker: PhantomData,
            lane,
            role,
            role_count,
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
            _epoch: PhantomData,
        }
    }

    #[inline]
    fn slab_ptr_and_len(&self) -> (*mut u8, usize) {
        unsafe {
            let slab = &mut *self.slab;
            (slab.as_mut_ptr(), slab.len())
        }
    }

    #[inline]
    fn endpoint_storage_floor(&self) -> usize {
        let (_, slab_len) = self.slab_ptr_and_len();
        let mut floor = slab_len;
        let mut idx = 0usize;
        while idx < usize::from(self.endpoint_lease_capacity) {
            let slot = unsafe { &*self.endpoint_leases.add(idx) };
            if slot.occupied && slot.len != 0 && (slot.offset as usize) < floor {
                floor = slot.offset as usize;
            }
            idx += 1;
        }
        floor
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
        self.route_table()
            .record_with_role_count(self.lane, self.role_count, self.role, scope, arm)
    }

    #[inline]
    pub(crate) fn poll_route_decision(
        &self,
        scope: ScopeId,
        role: u8,
        cx: &mut Context<'_>,
    ) -> Poll<u8> {
        self.route_table()
            .poll_with_role_count(self.lane, self.role_count, role, scope, cx)
    }

    #[inline]
    pub(crate) fn ack_route_decision(&self, scope: ScopeId, role: u8) -> Option<u8> {
        self.route_table()
            .acknowledge_with_role_count(self.lane, self.role_count, role, scope)
    }

    #[inline]
    pub(crate) fn peek_route_decision(&self, scope: ScopeId, role: u8) -> Option<u8> {
        self.route_table()
            .peek_with_role_count(self.lane, self.role_count, role, scope)
    }

    #[inline]
    pub(crate) fn pending_route_decision_lane_mask(&self, scope: ScopeId, role: u8) -> u16 {
        self.route_table()
            .pending_lane_mask_with_role_count(self.role_count, role, scope)
    }

    #[inline]
    pub(crate) fn route_change_epoch(&self) -> u16 {
        self.route_table().change_epoch()
    }

    #[cfg(test)]
    #[inline]
    pub(crate) fn has_route_hint_matching<F>(&self, matches: F) -> bool
    where
        F: FnMut(u8) -> bool,
    {
        let mut hints = self.route_hints_from_table();
        let before = hints.present_mask;
        let rx = unsafe { &*self.rx.get() };
        hints.drain_from_transport(self.transport(), rx);
        self.sync_pending_route_hint_lane_masks(before, hints.present_mask);
        hints.has_matching(matches)
    }

    #[inline]
    pub(crate) fn has_route_hint_for_label_mask(&self, label_mask: u128) -> bool {
        let mut hints = self.route_hints_from_table();
        let before = hints.present_mask;
        let rx = unsafe { &*self.rx.get() };
        hints.drain_from_transport(self.transport(), rx);
        self.sync_pending_route_hint_lane_masks(before, hints.present_mask);
        hints.has_any_label_in_mask(label_mask)
    }

    #[inline]
    pub(crate) fn pending_route_hint_lane_mask_for_label_mask(&self, label_mask: u128) -> u16 {
        let mut hints = self.route_hints_from_table();
        let before = hints.present_mask;
        let rx = unsafe { &*self.rx.get() };
        hints.drain_from_transport(self.transport(), rx);
        self.sync_pending_route_hint_lane_masks(before, hints.present_mask);
        self.route_table()
            .pending_hint_lane_mask_for_labels(label_mask)
    }

    #[inline]
    pub(crate) fn take_route_hint_for_label_mask(&self, label_mask: u128) -> Option<u8> {
        let mut hints = self.route_hints_from_table();
        let before = hints.present_mask;
        let rx = unsafe { &*self.rx.get() };
        hints.drain_from_transport(self.transport(), rx);
        let taken = hints.take_from_label_mask(label_mask);
        self.sync_pending_route_hint_lane_masks(before, hints.present_mask);
        taken
    }

    #[inline]
    pub(crate) fn clear_route_hints(&self) {
        let mut hints = self.route_hints_from_table();
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
        let (ptr, _) = self.slab_ptr_and_len();
        let base = unsafe { *self.image_frontier } as usize;
        let reserved = unsafe { *self.scratch_reserved_bytes } as usize;
        let start = base.saturating_add(reserved);
        let end = self.endpoint_storage_floor();
        let len = end.saturating_sub(start);
        unsafe { core::ptr::slice_from_raw_parts_mut(ptr.add(start), len) }
    }

    #[inline]
    pub(crate) fn frontier_scratch_ptr(&self) -> *mut [u8] {
        let (ptr, _) = self.slab_ptr_and_len();
        let start = unsafe { *self.image_frontier } as usize;
        let reserved = unsafe { *self.scratch_reserved_bytes } as usize;
        let lease_floor = self.endpoint_storage_floor();
        let end = if reserved == 0 {
            lease_floor
        } else {
            core::cmp::min(start.saturating_add(reserved), lease_floor)
        };
        let scratch_start =
            core::cmp::min(Self::align_up(start, Self::frontier_scratch_align()), end);
        let len = end.saturating_sub(scratch_start);
        unsafe { core::ptr::slice_from_raw_parts_mut(ptr.add(scratch_start), len) }
    }

    #[cfg(test)]
    #[inline]
    pub(crate) fn scratch_ledger_parts(
        &self,
    ) -> (
        *mut [u8],
        *const u32,
        *const u32,
        *const super::core::EndpointLeaseSlot,
        super::core::EndpointLeaseId,
    ) {
        (
            self.slab,
            self.image_frontier,
            self.scratch_reserved_bytes,
            self.endpoint_leases,
            self.endpoint_lease_capacity,
        )
    }

    #[inline]
    #[cfg(test)]
    pub(crate) fn host_slots(&self) -> &HostSlots<'r> {
        unsafe { &*self.host_slots.cast::<HostSlots<'r>>() }
    }

    #[inline]
    pub(crate) fn policy_digest(&self, slot: Slot) -> u32 {
        #[cfg(test)]
        {
            self.host_slots().active_digest(slot)
        }
        #[cfg(not(test))]
        {
            let _ = slot;
            0
        }
    }

    #[inline]
    pub(crate) fn policy_mode(&self, slot: Slot) -> PolicyMode {
        #[cfg(test)]
        {
            self.host_slots().policy_mode(slot)
        }
        #[cfg(not(test))]
        {
            let _ = slot;
            PolicyMode::Enforce
        }
    }

    #[inline]
    pub(crate) fn last_policy_fuel_used(&self, slot: Slot) -> u16 {
        #[cfg(test)]
        {
            self.host_slots().last_fuel_used(slot)
        }
        #[cfg(not(test))]
        {
            let _ = slot;
            0
        }
    }

    #[inline]
    pub(crate) fn run_policy<F>(
        &self,
        slot: Slot,
        event: &TapEvent,
        caps: CapsMask,
        session: Option<SessionId>,
        lane: Option<Lane>,
        configure: F,
    ) -> Action
    where
        F: FnOnce(&mut VmCtx<'_>),
    {
        #[cfg(test)]
        {
            return crate::epf::run_with(
                self.host_slots(),
                slot,
                event,
                caps,
                session,
                lane,
                configure,
            );
        }
        #[cfg(not(test))]
        {
            let mut ctx = VmCtx::new(slot, event, caps);
            if let Some(session) = session {
                ctx.set_session(session);
            }
            if let Some(lane) = lane {
                ctx.set_lane(lane);
            }
            configure(&mut ctx);
            Action::Proceed
        }
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
