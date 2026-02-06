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
use super::types::{Lane, RendezvousId, SessionId};
use crate::{
    control::cap::CapsMask,
    eff,
    epf::host::HostSlots,
    global::const_dsl::ScopeId,
    observe::{TapRing, emit},
    runtime::config::Clock,
    transport::{Transport, TransportEvent, TransportEventKind, TransportMetrics},
};

// Drain hints defensively: transport hint queues may be sized to MAX_EFF_NODES.
const ROUTE_HINT_SLOTS: usize = eff::meta::MAX_EFF_NODES;

#[derive(Clone, Copy)]
struct RouteHintQueue {
    len: u16,
    buf: [u8; ROUTE_HINT_SLOTS],
}

impl RouteHintQueue {
    const fn new() -> Self {
        Self {
            len: 0,
            buf: [0; ROUTE_HINT_SLOTS],
        }
    }

    fn clear(&mut self) {
        self.len = 0;
    }

    fn push(&mut self, label: u8) {
        if label == 0 {
            return;
        }
        let cap = ROUTE_HINT_SLOTS;
        let len = self.len as usize;
        if len >= cap {
            self.buf.copy_within(1..cap, 0);
            self.buf[cap - 1] = label;
            return;
        }
        self.buf[len] = label;
        self.len += 1;
    }

    fn take_matching<F>(&mut self, mut matches: F) -> Option<u8>
    where
        F: FnMut(u8) -> bool,
    {
        // Hints are one-shot for the current offer; drop any leftovers.
        let len = self.len as usize;
        let mut found = None;
        for idx in 0..len {
            let label = self.buf[idx];
            if matches(label) {
                found = Some(label);
                break;
            }
        }
        self.clear();
        found
    }

    fn peek_matching<F>(&self, mut matches: F) -> Option<u8>
    where
        F: FnMut(u8) -> bool,
    {
        let len = self.len as usize;
        for idx in 0..len {
            let label = self.buf[idx];
            if matches(label) {
                return Some(label);
            }
        }
        None
    }

    fn drain_from_transport<'a, T: Transport>(&mut self, transport: &'a T, rx: &'a T::Rx<'a>) {
        let mut budget = ROUTE_HINT_SLOTS;
        while budget > 0 {
            let label = match transport.recv_label_hint(rx) {
                Some(label) => label,
                None => break,
            };
            self.push(label);
            budget -= 1;
        }
    }
}

/// Lightweight port describing how an endpoint reaches the transport.
///
/// The port is intentionally `!Send`/`!Sync` (via the hidden `PhantomData`) so
/// that transport handles remain affine. Endpoint methods keep the mutable
/// borrow for the duration of a single `.await`, but exclusivity is preserved
/// because owning endpoints themselves are affine.
pub struct Port<
    'r,
    T: Transport,
    E: crate::control::cap::EpochTable = crate::control::cap::EpochInit,
> {
    transport: &'r T,
    tx: UnsafeCell<T::Tx<'r>>,
    rx: UnsafeCell<T::Rx<'r>>,
    scratch: *mut [u8],
    scratch_marker: PhantomData<&'r mut [u8]>,
    host_slots: *const HostSlots<'static>,
    host_slots_marker: PhantomData<&'r HostSlots<'r>>,
    pub sid: SessionId,
    pub lane: Lane,
    pub role: u8,
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

impl<'r, T: Transport, E: crate::control::cap::EpochTable + 'r> Port<'r, T, E> {
    #[allow(clippy::too_many_arguments)]
    pub fn new<'tap>(
        transport: &'r T,
        tap: &'tap TapRing<'tap>,
        clock: &'tap dyn Clock,
        vm_caps: &'tap VmCapsTable,
        loops: &'tap LoopTable,
        routes: &'tap RouteTable,
        host_slots: &'tap HostSlots<'tap>,
        scratch: *mut [u8],
        sid: SessionId,
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
            sid,
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

    pub fn transport(&self) -> &'r T {
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
    pub(crate) fn ingest_route_hints(&self) {
        let hints = unsafe { &mut *self.route_hints.get() };
        let rx = unsafe { &*self.rx.get() };
        hints.drain_from_transport(self.transport, rx);
    }

    #[inline]
    pub(crate) fn take_route_hint<F>(&self, matches: F) -> Option<u8>
    where
        F: FnMut(u8) -> bool,
    {
        let hints = unsafe { &mut *self.route_hints.get() };
        hints.take_matching(matches)
    }

    #[inline]
    pub(crate) fn peek_route_hint<F>(&self, matches: F) -> Option<u8>
    where
        F: FnMut(u8) -> bool,
    {
        let hints = unsafe { &*self.route_hints.get() };
        hints.peek_matching(matches)
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
    pub fn now32(&self) -> u32 {
        self.clock.now32()
    }

    #[inline]
    pub(crate) fn vm_caps(&self) -> &VmCapsTable {
        unsafe { &*self.vm_caps }
    }

    #[inline]
    pub fn caps_mask(&self) -> CapsMask {
        self.vm_caps().get(self.lane)
    }

    #[inline]
    pub fn flush_transport_events(&self) -> Option<TransportEvent> {
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
            emit(tap, events::TransportMetrics::new(clock.now32(), arg0, arg1));
            if let Some((ext0, ext1)) = payload.extension {
                emit(tap, events::TransportMetricsExt::new(clock.now32(), ext0, ext1));
            }
        }

        last_loss
    }

    #[inline]
    pub fn sid(&self) -> SessionId {
        self.sid
    }

    #[inline]
    pub fn lane(&self) -> Lane {
        self.lane
    }

    #[inline]
    pub fn role(&self) -> u8 {
        self.role
    }

    #[inline]
    pub fn rv_id(&self) -> RendezvousId {
        self.rv_id
    }

    pub fn with_tx<R>(&mut self, f: impl FnOnce(&mut T::Tx<'r>) -> R) -> R {
        unsafe { f(&mut *self.tx.get()) }
    }

    pub fn with_rx<R>(&mut self, f: impl FnOnce(&mut T::Rx<'r>) -> R) -> R {
        unsafe { f(&mut *self.rx.get()) }
    }
}
