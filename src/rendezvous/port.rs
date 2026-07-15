//! Port abstraction for transport layer access.
//!
//! Port provides lightweight access to transport Tx/Rx handles,
//! scratch buffers, tap rings, and state tables.

use core::{
    cell::{Cell, UnsafeCell},
    marker::PhantomData,
    ptr::NonNull,
};

use super::core::{EndpointLeaseRecord, RendezvousAccessState, Sidecar};
use crate::{
    endpoint::kernel::FrontierScratchLayout,
    observe::core::TapRing,
    session::types::{Lane, RendezvousId, SessionId},
    transport::Transport,
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

mod membership;
mod recv_frame;

use self::recv_frame::RecvFrameReceiptState;
pub(crate) use self::recv_frame::{
    FrameMismatch, FrameObservation, PreambleFrame, PreambleObservation, ReceivedFrame,
    transport_frame_tap_event,
};

/// Lightweight port describing how an endpoint reaches the transport.
///
/// The port is intentionally `!Send`/`!Sync` (via the hidden `PhantomData`) so
/// that transport handles remain affine. Endpoint methods keep the mutable
/// borrow for the duration of a single `.await`, but exclusivity is kept
/// because owning endpoints themselves are affine.
pub(crate) struct Port<'r, T: Transport> {
    transport: &'r T,
    tx: UnsafeCell<T::Tx<'r>>,
    rx: UnsafeCell<T::Rx<'r>>,
    slab_ptr: *mut u8,
    slab_len: usize,
    access_state: &'r Cell<RendezvousAccessState>,
    image_frontier: &'r Cell<u32>,
    frontier_workspace_bytes: &'r Cell<u32>,
    endpoint_lease_storage: &'r Cell<Sidecar<EndpointLeaseRecord>>,
    scratch_marker: PhantomData<&'r mut [u8]>,
    sid: SessionId,
    pub(crate) lane: Lane,
    rv_id: RendezvousId,
    _no_send_sync: PhantomData<*mut ()>,
    tap: *const TapRing<'static>,
    tap_marker: PhantomData<&'r TapRing<'r>>,
    recv_frame_receipt: RecvFrameReceiptState,
}

pub(crate) struct PortInit<'r, 'tap, T: Transport> {
    pub(crate) transport: &'r T,
    pub(crate) tap: &'tap TapRing<'tap>,
    pub(crate) slab_ptr: *mut u8,
    pub(crate) slab_len: usize,
    pub(crate) access_state: &'tap Cell<RendezvousAccessState>,
    pub(crate) image_frontier: &'tap Cell<u32>,
    pub(crate) frontier_workspace_bytes: &'tap Cell<u32>,
    pub(crate) endpoint_lease_storage: &'tap Cell<Sidecar<EndpointLeaseRecord>>,
    pub(crate) sid: SessionId,
    pub(crate) lane: Lane,
    pub(crate) rv_id: RendezvousId,
    pub(crate) tx: T::Tx<'r>,
    pub(crate) rx: T::Rx<'r>,
}

pub(crate) struct ScratchLease<'r> {
    state: &'r Cell<RendezvousAccessState>,
    restore: RendezvousAccessState,
}

impl Drop for ScratchLease<'_> {
    #[inline]
    fn drop(&mut self) {
        if self.state.get().finish_scratch() != Some(self.restore) {
            crate::invariant();
        }
        self.state.set(self.restore);
    }
}

impl<'r, T: Transport + 'r> Port<'r, T> {
    #[inline(always)]
    const fn frontier_scratch_align() -> usize {
        FrontierScratchLayout::new(0, 0).total_align()
    }

    pub(crate) fn new<'tap>(init: PortInit<'r, 'tap, T>) -> Self
    where
        'tap: 'r,
    {
        let PortInit {
            transport,
            tap,
            slab_ptr,
            slab_len,
            access_state,
            image_frontier,
            frontier_workspace_bytes,
            endpoint_lease_storage,
            sid,
            lane,
            rv_id,
            tx,
            rx,
        } = init;
        Self {
            transport,
            tx: UnsafeCell::new(tx),
            rx: UnsafeCell::new(rx),
            slab_ptr,
            slab_len,
            access_state,
            image_frontier,
            frontier_workspace_bytes,
            endpoint_lease_storage,
            scratch_marker: PhantomData,
            sid,
            lane,
            rv_id,
            _no_send_sync: PhantomData,
            tap: (tap as *const TapRing<'tap>).cast::<TapRing<'static>>(),
            tap_marker: PhantomData,
            recv_frame_receipt: RecvFrameReceiptState::new(),
        }
    }

    #[inline]
    fn port_key(port: &Self) -> NonNull<()> {
        NonNull::from(port).cast()
    }

    #[inline]
    fn slab_ptr_and_len(&self) -> (*mut u8, usize) {
        (self.slab_ptr, self.slab_len)
    }

    #[inline]
    pub(crate) fn try_scratch_lease(&self) -> Option<ScratchLease<'r>> {
        let (leased, restore) = self.access_state.get().begin_scratch()?;
        self.access_state.set(leased);
        Some(ScratchLease {
            state: self.access_state,
            restore,
        })
    }

    #[inline]
    pub(crate) fn require_access_barrier(&self) {
        match self.access_state.get() {
            RendezvousAccessState::RegistryLease
            | RendezvousAccessState::ScratchLease
            | RendezvousAccessState::EndpointOperation
            | RendezvousAccessState::EndpointScratchLease => {}
            RendezvousAccessState::Available => crate::invariant(),
        }
    }

    #[inline]
    fn require_scratch_lease(&self) {
        if !matches!(
            self.access_state.get(),
            RendezvousAccessState::ScratchLease | RendezvousAccessState::EndpointScratchLease
        ) {
            crate::invariant();
        }
    }

    #[inline]
    fn endpoint_lease_owner_view(&self) -> (*const EndpointLeaseRecord, usize) {
        // The pinned rendezvous owner cells outlive this port. Capacity growth
        // may replace the sidecar root, so every floor computation reloads it
        // and derives the slot count from the exact sidecar byte length.
        let storage = self.endpoint_lease_storage.get();
        (
            storage.ptr().cast_const(),
            EndpointLeaseRecord::storage_slot_count(storage),
        )
    }

    #[inline]
    fn endpoint_storage_floor(&self) -> usize {
        let (_, slab_len) = self.slab_ptr_and_len();
        let (endpoint_leases, endpoint_lease_slot_count) = self.endpoint_lease_owner_view();
        let mut floor = slab_len;
        let mut idx = 0usize;
        while idx < endpoint_lease_slot_count {
            // SAFETY: `endpoint_leases` has `endpoint_lease_slot_count`
            // initialized slots owned by the rendezvous and observed through
            // its live sidecar root.
            let slot = unsafe { (&*endpoint_leases.add(idx)).slot() };
            if slot.is_occupied() && slot.len != 0 && (slot.offset as usize) < floor {
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
    pub(crate) fn tx_ptr(&self) -> *mut T::Tx<'r> {
        self.tx.get()
    }

    #[inline]
    pub(crate) fn rx_ptr(&self) -> *mut T::Rx<'r> {
        self.rx.get()
    }

    #[inline]
    pub(crate) fn scratch_ptr(&self) -> *mut [u8] {
        self.require_scratch_lease();
        let (ptr, _) = self.slab_ptr_and_len();
        // The owner cells hold initialized offsets into the pinned rendezvous slab.
        let base = self.image_frontier.get() as usize;
        let workspace = self.frontier_workspace_bytes.get() as usize;
        let start = checked_add_usize(base, workspace);
        let end = self.endpoint_storage_floor();
        if start > end {
            crate::invariant();
        }
        let len = end - start;
        // SAFETY: `start..start+len` is bounded by the endpoint storage floor
        // inside the pinned slab returned by `slab_ptr_and_len`.
        unsafe { core::ptr::slice_from_raw_parts_mut(ptr.add(start), len) }
    }

    #[inline]
    pub(crate) fn frontier_scratch_ptr(&self) -> *mut [u8] {
        self.require_scratch_lease();
        let (ptr, _) = self.slab_ptr_and_len();
        // The owner cells hold initialized offsets into the pinned rendezvous slab.
        let start = self.image_frontier.get() as usize;
        let workspace = self.frontier_workspace_bytes.get() as usize;
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
        // SAFETY: `scratch_start..scratch_start+len` is bounded by the
        // frontier workspace region inside the pinned port slab.
        unsafe { core::ptr::slice_from_raw_parts_mut(ptr.add(scratch_start), len) }
    }

    #[inline]
    pub(crate) fn tap(&self) -> &TapRing<'r> {
        // SAFETY: `tap` points to the rendezvous-owned TapRing bound during
        // port construction and outliving every lane port reference.
        unsafe { &*self.tap.cast::<TapRing<'r>>() }
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
    use super::align_up_absolute_offset;

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
}
