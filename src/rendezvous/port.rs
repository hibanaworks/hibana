//! Port abstraction for transport layer access.
//!
//! Port provides lightweight access to transport Tx/Rx handles,
//! scratch buffers, tap rings, and state tables.

use core::{
    cell::UnsafeCell,
    marker::PhantomData,
    task::{Context, Poll},
};

use super::tables::RouteTable;
use crate::{
    endpoint::kernel::FrontierScratchLayout,
    global::const_dsl::ScopeId,
    observe::core::TapRing,
    resolver_audit::{self, ResolverSlot},
    runtime_core::config::Clock,
    session::types::{Lane, RendezvousId, SessionId},
    transport::{FrameLabelMask, Transport},
};

#[inline(always)]
const fn align_up(value: usize, align: usize) -> usize {
    if align == 0 {
        crate::invariant();
    }
    let mask = align - 1;
    if value > usize::MAX - mask {
        crate::invariant();
    }
    (value + mask) & !mask
}

#[inline(always)]
fn align_up_absolute_offset(base: usize, offset: usize, align: usize) -> usize {
    checked_sub_usize(align_up(checked_add_usize(base, offset), align), base)
}

#[inline(always)]
fn checked_add_usize(lhs: usize, rhs: usize) -> usize {
    match lhs.checked_add(rhs) {
        Some(value) => value,
        None => crate::invariant(),
    }
}

#[inline(always)]
fn checked_sub_usize(lhs: usize, rhs: usize) -> usize {
    match lhs.checked_sub(rhs) {
        Some(value) => value,
        None => crate::invariant(),
    }
}

mod recv_frame;
mod route_hints;

pub(crate) use self::recv_frame::{
    FrameMismatch, FrameObservation, PreambleFrame, ReceivedFrame, transport_frame_tap_event,
};
use self::{recv_frame::RecvFrameReceiptState, route_hints::RouteHintQueue};

/// Lightweight port describing how an endpoint reaches the transport.
///
/// The port is intentionally `!Send`/`!Sync` (via the hidden `PhantomData`) so
/// that transport handles remain affine. Endpoint methods keep the mutable
/// borrow for the duration of a single `.await`, but exclusivity is preserved
/// because owning endpoints themselves are affine.
pub(crate) struct Port<'r, T: Transport> {
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
    routes: *const RouteTable,
    routes_marker: PhantomData<&'r RouteTable>,
    recv_frame_receipt: RecvFrameReceiptState,
}

pub(crate) struct PortInit<'r, 'tap, T: Transport> {
    pub transport: &'r T,
    pub tap: &'tap TapRing<'tap>,
    pub clock: &'tap dyn Clock,
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
}

impl<'r, T: Transport + 'r> Port<'r, T> {
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

    pub(crate) fn new<'tap>(init: PortInit<'r, 'tap, T>) -> Self
    where
        'tap: 'r,
    {
        let PortInit {
            transport,
            tap,
            clock,
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
            routes: routes as *const RouteTable,
            routes_marker: PhantomData,
            recv_frame_receipt: RecvFrameReceiptState::new(),
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
    pub(crate) fn has_unresolved_recv_frame(&self) -> bool {
        self.recv_frame_receipt.has_outstanding()
    }

    #[inline]
    pub(crate) fn route_table(&self) -> &RouteTable {
        // SAFETY: `routes` points to the rendezvous-local RouteTable bound for
        // this port and outliving every lane port reference.
        unsafe { &*self.routes }
    }

    #[inline]
    pub(crate) fn record_route_arm_selection(&self, scope: ScopeId, arm: u8) -> u16 {
        self.route_table()
            .record_with_role_count(self.lane, self.role_count, self.role, scope, arm)
    }

    #[inline]
    pub(crate) fn poll_route_arm_selection(
        &self,
        scope: ScopeId,
        role: u8,
        cx: &mut Context<'_>,
    ) -> Poll<u8> {
        self.route_table()
            .poll_with_role_count(self.lane, self.role_count, role, scope, cx)
    }

    #[inline]
    pub(crate) fn ack_route_arm_selection(&self, scope: ScopeId, role: u8) -> Option<u8> {
        self.route_table()
            .acknowledge_with_role_count(self.lane, self.role_count, role, scope)
    }

    #[inline]
    pub(crate) fn peek_route_arm_selection(&self, scope: ScopeId, role: u8) -> Option<u8> {
        self.route_table()
            .peek_with_role_count(self.lane, self.role_count, role, scope)
    }

    #[inline]
    pub(crate) fn has_pending_route_arm_selection_for_lane(
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
    pub(crate) fn route_change_generation(&self) -> u16 {
        self.route_table().change_generation()
    }

    #[inline]
    pub(crate) fn has_route_hint_for_frame_label_mask(
        &self,
        _session: SessionId,
        frame_label_mask: FrameLabelMask,
    ) -> bool {
        let hints = self.route_hints_from_table();
        let before = hints.present_mask;
        self.sync_pending_route_frame_hint_lane_masks(before, hints.present_mask);
        hints.has_any_frame_label_in_mask(frame_label_mask)
    }

    #[inline]
    pub(crate) fn has_pending_route_hint_for_lane(
        &self,
        _session: SessionId,
        frame_label_mask: FrameLabelMask,
        target_lane: Lane,
    ) -> bool {
        let hints = self.route_hints_from_table();
        let before = hints.present_mask;
        self.sync_pending_route_frame_hint_lane_masks(before, hints.present_mask);
        self.route_table()
            .has_pending_frame_hint_for_lane(target_lane, frame_label_mask)
    }

    #[inline]
    pub(crate) fn take_route_hint_for_frame_label_mask(
        &self,
        _session: SessionId,
        frame_label_mask: FrameLabelMask,
    ) -> Option<u8> {
        let mut hints = self.route_hints_from_table();
        let before = hints.present_mask;
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
        let start = checked_add_usize(base, workspace);
        let end = self.endpoint_storage_floor();
        if start > end {
            crate::invariant();
        }
        let len = end - start;
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
        let workspace_end = checked_add_usize(start, workspace);
        if workspace_end > lease_floor {
            crate::invariant();
        }
        let scratch_start = if workspace == 0 {
            workspace_end
        } else {
            let scratch_start =
                align_up_absolute_offset(ptr as usize, start, Self::frontier_scratch_align());
            if scratch_start > workspace_end {
                crate::invariant();
            }
            scratch_start
        };
        let len = workspace_end - scratch_start;
        // SAFETY: `scratch_start..scratch_start+len` is clamped to the
        // frontier workspace region inside the pinned port slab.
        unsafe { core::ptr::slice_from_raw_parts_mut(ptr.add(scratch_start), len) }
    }

    #[inline]
    pub(crate) fn resolver_digest(&self, slot: ResolverSlot) -> u32 {
        match slot {
            ResolverSlot::EndpointRx | ResolverSlot::EndpointTx | ResolverSlot::Decision => {
                resolver_audit::RESOLVER_DIGEST_NONE
            }
        }
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
    use crate::transport::FrameLabelMask;

    fn route_hints(labels: &[u8]) -> RouteHintQueue {
        let mut mask = FrameLabelMask::EMPTY;
        for frame_label in labels {
            mask.insert_frame_label(*frame_label);
        }
        RouteHintQueue::from_mask(mask)
    }

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
        let mut queue = route_hints(&[41, 42]);

        let first = queue.take_from_frame_label_mask(FrameLabelMask::from_frame_label(99));
        assert_eq!(
            first, None,
            "non-matching take must not clear buffered hints"
        );

        let second = queue.take_from_frame_label_mask(FrameLabelMask::from_frame_label(42));
        assert_eq!(
            second,
            Some(42),
            "later matching hint must remain available"
        );

        let third = queue.take_from_frame_label_mask(FrameLabelMask::from_frame_label(41));
        assert_eq!(
            third,
            Some(41),
            "earlier unmatched hint must still be available after sibling selection"
        );
    }

    #[test]
    fn route_hint_queue_deduplicates_same_frame_label() {
        let mut queue = route_hints(&[25, 25, 25]);

        let first = queue.take_from_frame_label_mask(FrameLabelMask::from_frame_label(25));
        assert_eq!(first, Some(25));

        let second = queue.take_from_frame_label_mask(FrameLabelMask::from_frame_label(25));
        assert_eq!(
            second, None,
            "duplicate frame labels must be coalesced in queue"
        );
    }

    #[test]
    fn route_hint_has_matching_is_non_consuming() {
        let mut queue = route_hints(&[25, 201]);

        assert!(queue.has_any_frame_label_in_mask(FrameLabelMask::from_frame_label(201)));
        assert_eq!(
            queue.take_from_frame_label_mask(FrameLabelMask::from_frame_label(201)),
            Some(201)
        );
        assert_eq!(
            queue.take_from_frame_label_mask(FrameLabelMask::from_frame_label(25)),
            Some(25)
        );
    }
}
