//! Port abstraction for transport layer access.
//!
//! Port provides lightweight access to transport Tx/Rx handles,
//! scratch buffers, tap rings, and state tables.

use core::{
    cell::UnsafeCell,
    marker::PhantomData,
    task::{Context, Poll},
};

use super::tables::{LoopDisposition, LoopTable, RouteTable};
use crate::{
    control::types::{Lane, RendezvousId},
    endpoint::kernel::FrontierScratchLayout,
    global::const_dsl::ScopeId,
    observe::core::TapRing,
    policy_runtime::{self, PolicySlot},
    runtime::config::Clock,
    transport::{FrameLabelMask, Transport},
};

#[inline(always)]
const fn align_up(value: usize, align: usize) -> usize {
    let mask = align.saturating_sub(1);
    (value + mask) & !mask
}

#[inline(always)]
fn align_up_absolute_offset(base: usize, offset: usize, align: usize) -> usize {
    align_up(base.saturating_add(offset), align).saturating_sub(base)
}

mod recv_frame;
mod route_hints;

pub(crate) use self::recv_frame::ReceivedFrame;
use self::{recv_frame::RecvFrameReceiptState, route_hints::RouteHintQueue};

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
    frontier_workspace_bytes: *const u32,
    endpoint_leases: *const super::core::EndpointLeaseSlot,
    endpoint_lease_capacity: super::core::EndpointLeaseId,
    scratch_marker: PhantomData<&'r mut [u8]>,
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
    recv_frame_receipt: RecvFrameReceiptState,
    _epoch: PhantomData<E>,
}

pub(crate) struct PortInit<
    'r,
    'tap,
    T: Transport,
    E: crate::control::cap::mint::EpochTable = crate::control::cap::mint::EpochTbl,
> {
    pub transport: &'r T,
    pub tap: &'tap TapRing<'tap>,
    pub clock: &'tap dyn Clock,
    pub loops: &'tap LoopTable,
    pub routes: &'tap RouteTable,
    pub slab: *mut [u8],
    pub image_frontier: *const u32,
    pub frontier_workspace_bytes: *const u32,
    pub endpoint_leases: *const super::core::EndpointLeaseSlot,
    pub endpoint_lease_capacity: super::core::EndpointLeaseId,
    pub lane: Lane,
    pub role: u8,
    pub role_count: u8,
    pub rv_id: RendezvousId,
    pub tx: T::Tx<'r>,
    pub rx: T::Rx<'r>,
    pub _epoch: PhantomData<E>,
}

impl<'r, T: Transport, E: crate::control::cap::mint::EpochTable + 'r> Port<'r, T, E> {
    #[inline(always)]
    const fn frontier_scratch_align() -> usize {
        FrontierScratchLayout::new(0, 0, 0).total_align()
    }

    #[inline]
    fn sync_pending_route_frame_hint_lane_masks(
        &self,
        before: FrameLabelMask,
        after: FrameLabelMask,
    ) {
        if before != after {
            self.route_table()
                .update_pending_frame_hint_mask_for_lane(self.lane, before, after);
        }
    }

    #[inline]
    fn route_hints_from_table(&self) -> RouteHintQueue {
        RouteHintQueue::from_mask(
            self.route_table()
                .pending_frame_hint_mask_for_lane(self.lane),
        )
    }

    pub(crate) fn new<'tap>(init: PortInit<'r, 'tap, T, E>) -> Self
    where
        'tap: 'r,
    {
        let PortInit {
            transport,
            tap,
            clock,
            loops,
            routes,
            slab,
            image_frontier,
            frontier_workspace_bytes,
            endpoint_leases,
            endpoint_lease_capacity,
            lane,
            role,
            role_count,
            rv_id,
            tx,
            rx,
            _epoch,
        } = init;
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
            frontier_workspace_bytes,
            endpoint_leases,
            endpoint_lease_capacity,
            scratch_marker: PhantomData,
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
            recv_frame_receipt: RecvFrameReceiptState::new(),
            _epoch: PhantomData,
        }
    }

    #[inline]
    fn port_key(port: &Self) -> *const () {
        core::ptr::from_ref(port).cast()
    }

    #[inline]
    fn slab_ptr_and_len(&self) -> (*mut u8, usize) {
        unsafe {
            // SAFETY: `slab` points to the rendezvous-owned backing slice bound
            // during port construction and remains pinned for the port lifetime.
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
            // SAFETY: `endpoint_leases` has `endpoint_lease_capacity`
            // initialized slots owned by this port.
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
        // SAFETY: `loops` points to the rendezvous-local LoopTable bound for
        // this port and outliving every lane port reference.
        unsafe { &*self.loops }
    }

    #[inline]
    pub(crate) fn route_table(&self) -> &RouteTable {
        // SAFETY: `routes` points to the rendezvous-local RouteTable bound for
        // this port and outliving every lane port reference.
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
    pub(crate) fn has_pending_route_decision_for_lane(
        &self,
        scope: ScopeId,
        role: u8,
        target_lane: Lane,
    ) -> bool {
        self.route_table().has_pending_lane_with_role_count(
            self.role_count,
            role,
            scope,
            target_lane,
        )
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
        // SAFETY: the port owns this lane-local Rx handle. Hint draining may
        // mutate the transport hint sidecar but does not consume payload bytes.
        let rx = unsafe { &mut *self.rx.get() };
        hints.drain_from_transport(self.transport(), rx);
        self.sync_pending_route_frame_hint_lane_masks(before, hints.present_mask);
        hints.has_matching(matches)
    }

    #[inline]
    pub(crate) fn has_route_hint_for_frame_label_mask(
        &self,
        frame_label_mask: FrameLabelMask,
        drain_transport_hints: bool,
    ) -> bool {
        let mut hints = self.route_hints_from_table();
        let before = hints.present_mask;
        if drain_transport_hints {
            // SAFETY: the port owns this lane-local Rx handle. Hint draining may
            // mutate the transport hint sidecar but does not consume payload bytes.
            let rx = unsafe { &mut *self.rx.get() };
            hints.drain_from_transport(self.transport(), rx);
        }
        self.sync_pending_route_frame_hint_lane_masks(before, hints.present_mask);
        hints.has_any_frame_label_in_mask(frame_label_mask)
    }

    #[inline]
    pub(crate) fn has_pending_route_hint_for_lane(
        &self,
        frame_label_mask: FrameLabelMask,
        target_lane: Lane,
        drain_transport_hints: bool,
    ) -> bool {
        let mut hints = self.route_hints_from_table();
        let before = hints.present_mask;
        if drain_transport_hints {
            // SAFETY: the port owns this lane-local Rx handle. Hint draining may
            // mutate the transport hint sidecar but does not consume payload bytes.
            let rx = unsafe { &mut *self.rx.get() };
            hints.drain_from_transport(self.transport(), rx);
        }
        self.sync_pending_route_frame_hint_lane_masks(before, hints.present_mask);
        self.route_table()
            .has_pending_frame_hint_for_lane(target_lane, frame_label_mask)
    }

    #[inline]
    pub(crate) fn take_route_hint_for_frame_label_mask(
        &self,
        frame_label_mask: FrameLabelMask,
        drain_transport_hints: bool,
    ) -> Option<u8> {
        let mut hints = self.route_hints_from_table();
        let before = hints.present_mask;
        if drain_transport_hints {
            // SAFETY: the port owns this lane-local Rx handle. Hint draining may
            // mutate the transport hint sidecar but does not consume payload bytes.
            let rx = unsafe { &mut *self.rx.get() };
            hints.drain_from_transport(self.transport(), rx);
        }
        let taken = hints.take_from_frame_label_mask(frame_label_mask);
        self.sync_pending_route_frame_hint_lane_masks(before, hints.present_mask);
        taken
    }

    #[inline]
    pub(crate) fn clear_route_hints(&self) {
        let mut hints = self.route_hints_from_table();
        let before = hints.present_mask;
        hints.clear();
        self.sync_pending_route_frame_hint_lane_masks(before, hints.present_mask);
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
        // SAFETY: `image_frontier` and `frontier_workspace_bytes` are
        // port-owned initialized offsets into the pinned port slab.
        let base = unsafe { *self.image_frontier } as usize;
        let workspace = unsafe { *self.frontier_workspace_bytes } as usize;
        let start = base.saturating_add(workspace);
        let end = self.endpoint_storage_floor();
        let len = end.saturating_sub(start);
        // SAFETY: `start..start+len` is clamped to the endpoint storage floor
        // within the pinned slab returned by `slab_ptr_and_len`.
        unsafe { core::ptr::slice_from_raw_parts_mut(ptr.add(start), len) }
    }

    #[inline]
    pub(crate) fn frontier_scratch_ptr(&self) -> *mut [u8] {
        let (ptr, _) = self.slab_ptr_and_len();
        // SAFETY: `image_frontier` and `frontier_workspace_bytes` are
        // port-owned initialized offsets into the pinned port slab.
        let start = unsafe { *self.image_frontier } as usize;
        let workspace = unsafe { *self.frontier_workspace_bytes } as usize;
        let lease_floor = self.endpoint_storage_floor();
        let workspace_end = core::cmp::min(start.saturating_add(workspace), lease_floor);
        let scratch_start = core::cmp::min(
            align_up_absolute_offset(ptr as usize, start, Self::frontier_scratch_align()),
            workspace_end,
        );
        let len = workspace_end.saturating_sub(scratch_start);
        // SAFETY: `scratch_start..scratch_start+len` is clamped to the
        // frontier workspace region inside the pinned port slab.
        unsafe { core::ptr::slice_from_raw_parts_mut(ptr.add(scratch_start), len) }
    }

    #[inline]
    pub(crate) fn policy_digest(&self, slot: PolicySlot) -> u32 {
        let _ = slot;
        policy_runtime::POLICY_DIGEST_NONE
    }

    #[inline]
    pub(crate) fn tap(&self) -> &TapRing<'r> {
        // SAFETY: `tap` points to the rendezvous-owned TapRing bound during
        // port construction and outliving every lane port reference.
        unsafe { &*self.tap.cast::<TapRing<'r>>() }
    }

    #[inline]
    pub(crate) fn now32(&self) -> u32 {
        self.clock.now32()
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
    use super::{align_up_absolute_offset, route_hints::RouteHintQueue};

    #[test]
    fn frontier_scratch_offset_aligns_absolute_address_not_offset_only() {
        let base = 3usize;
        let start = 5usize;
        let align = 8usize;
        let aligned = align_up_absolute_offset(base, start, align);

        assert_eq!(
            (base + aligned) % align,
            0,
            "frontier scratch storage must be aligned as an absolute address"
        );
        assert_eq!(
            aligned, start,
            "offset-only alignment would incorrectly move an already aligned absolute address"
        );
    }

    #[test]
    fn route_hint_unmatched_is_not_discarded_across_scope_selection() {
        let mut queue = RouteHintQueue::new();
        queue.push(41);
        queue.push(42);

        let first = queue.take_matching(|frame_label| frame_label == 99);
        assert_eq!(
            first, None,
            "non-matching take must not clear buffered hints"
        );

        let second = queue.take_matching(|frame_label| frame_label == 42);
        assert_eq!(
            second,
            Some(42),
            "later matching hint must remain available"
        );

        let third = queue.take_matching(|frame_label| frame_label == 41);
        assert_eq!(
            third,
            Some(41),
            "earlier unmatched hint must still be available after sibling selection"
        );
    }

    #[test]
    fn route_hint_queue_deduplicates_same_frame_label() {
        let mut queue = RouteHintQueue::new();
        queue.push(25);
        queue.push(25);
        queue.push(25);

        let first = queue.take_matching(|frame_label| frame_label == 25);
        assert_eq!(first, Some(25));

        let second = queue.take_matching(|frame_label| frame_label == 25);
        assert_eq!(
            second, None,
            "duplicate frame labels must be coalesced in queue"
        );
    }

    #[test]
    fn route_hint_has_matching_is_non_consuming() {
        let mut queue = RouteHintQueue::new();
        queue.push(25);
        queue.push(201);

        assert!(queue.has_matching(|frame_label| frame_label == 201));
        assert_eq!(
            queue.take_matching(|frame_label| frame_label == 201),
            Some(201)
        );
        assert_eq!(
            queue.take_matching(|frame_label| frame_label == 25),
            Some(25)
        );
    }
}
