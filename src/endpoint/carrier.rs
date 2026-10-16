//! Crate-private carrier markers for internal endpoint packs.
//!
//! # Unsafe Owner Contract
//!
//! This module owns raw endpoint-carrier projections. Unsafe blocks here may
//! reborrow typed payload/session references from stored raw pointers only when
//! the creating endpoint lease still owns the referenced resident image.

use core::{
    marker::PhantomData,
    ptr::NonNull,
    task::{Context, Poll, Waker},
};

use crate::transport::wire::Payload;

mod lifecycle;
mod recv;
mod route;
mod send;

#[cfg(all(test, hibana_repo_tests))]
mod tests;

#[derive(Clone, Copy)]
pub(crate) struct RawPayload {
    bytes: *const [u8],
}

impl RawPayload {
    #[inline]
    pub(crate) fn from_payload(payload: Payload<'_>) -> Self {
        Self {
            bytes: payload.as_bytes() as *const [u8],
        }
    }

    #[inline]
    pub(crate) unsafe fn into_payload<'a>(self) -> Payload<'a> {
        let bytes = /* SAFETY: `RawPayload` is created from a staged endpoint
        payload slice and consumed before the endpoint borrow ends; this only
        restores the shared byte-slice view. */
            unsafe { &*self.bytes };
        Payload::new(bytes)
    }
}

struct OutSlot<T> {
    ptr: *mut T,
}

impl<T> OutSlot<T> {
    #[inline]
    fn new(ptr: *mut T) -> Self {
        Self { ptr }
    }

    #[inline]
    fn write(self, value: T) {
        unsafe {
            // SAFETY: `OutSlot` is only constructed by raw endpoint callbacks
            // for caller-owned uninitialized output storage. Each callback
            // writes the slot exactly once before returning to the erased
            // endpoint future boundary.
            self.ptr.write(value);
        }
    }
}

impl OutSlot<()> {
    #[inline]
    fn erased<T>(ptr: *mut ()) -> OutSlot<T> {
        OutSlot { ptr: ptr.cast() }
    }
}

pub(crate) struct RecvPollRequest<'a, 'cx> {
    pub(crate) ptr: NonNull<()>,
    pub(crate) handle: EndpointHandle,
    pub(crate) logical_label: u8,
    pub(crate) payload_schema: u32,
    pub(crate) validate:
        for<'payload> fn(Payload<'payload>) -> Result<(), crate::transport::wire::CodecError>,
    pub(crate) cx: &'a mut Context<'cx>,
    pub(crate) out: *mut Poll<crate::endpoint::RecvResult<RawPayload>>,
}

pub(crate) struct BranchRecvPollRequest<'a, 'cx> {
    pub(crate) ptr: NonNull<()>,
    pub(crate) handle: EndpointHandle,
    pub(crate) logical_label: u8,
    pub(crate) payload_schema: u32,
    pub(crate) validate:
        for<'payload> fn(Payload<'payload>) -> Result<(), crate::transport::wire::CodecError>,
    pub(crate) cx: &'a mut Context<'cx>,
    pub(crate) out: *mut Poll<crate::endpoint::RecvResult<RawPayload>>,
}

/// Callback-free Waker ownership crossing one raw endpoint carrier borrow.
///
/// One poll can retire at most the displaced waiter and the newly installed
/// waiter. The incoming clone reuses the first slot before either is deferred.
pub(crate) struct WaiterTransfer {
    first: Option<Waker>,
    second: Option<Waker>,
}

impl WaiterTransfer {
    #[inline]
    pub(crate) fn with_replacement(waker: &Waker) -> Self {
        Self {
            first: Some(waker.clone()),
            second: None,
        }
    }

    #[inline]
    pub(crate) const fn empty() -> Self {
        Self {
            first: None,
            second: None,
        }
    }

    #[inline]
    pub(crate) fn take_replacement(&mut self) -> Waker {
        crate::invariant_some(self.first.take())
    }

    #[inline]
    pub(crate) fn defer(&mut self, waker: Option<Waker>) {
        let Some(waker) = waker else {
            return;
        };
        if self.first.is_none() {
            self.first = Some(waker);
        } else if self.second.is_none() {
            self.second = Some(waker);
        } else {
            crate::invariant();
        }
    }
}

#[repr(C)]
pub(crate) struct KernelEndpointHeader<'r> {
    ops: EndpointOps<'r>,
    generation: core::cell::Cell<u32>,
    role: u8,
    operation_access: &'r core::cell::Cell<crate::rendezvous::core::RendezvousAccessState>,
}

impl<'r> KernelEndpointHeader<'r> {
    #[inline(always)]
    pub(crate) const fn new(
        ops: EndpointOps<'r>,
        generation: u32,
        role: u8,
        operation_access: &'r core::cell::Cell<crate::rendezvous::core::RendezvousAccessState>,
    ) -> Self {
        Self {
            ops,
            generation: core::cell::Cell::new(generation),
            role,
            operation_access,
        }
    }

    #[inline(always)]
    pub(crate) const fn ops(&self) -> &EndpointOps<'r> {
        &self.ops
    }

    #[inline(always)]
    pub(crate) const fn generation(&self) -> u32 {
        self.generation.get()
    }

    #[inline(always)]
    pub(crate) const fn role(&self) -> u8 {
        self.role
    }

    #[inline(always)]
    pub(crate) fn retire_generation(&self) {
        self.generation.set(0);
    }

    pub(crate) fn try_operation_lease(
        &self,
    ) -> Option<crate::rendezvous::core::EndpointOperationLease<'r>> {
        crate::rendezvous::core::EndpointOperationLease::try_acquire(self.operation_access)
    }

    pub(crate) fn is_protected_by(
        &self,
        lease: &crate::rendezvous::core::EndpointOperationLease<'_>,
    ) -> bool {
        lease.protects(self.operation_access)
    }
}

#[derive(Clone, Copy)]
pub(crate) struct EndpointOps<'r> {
    _lifetime: PhantomData<&'r ()>,
    pub(crate) drop_endpoint: unsafe fn(ptr: NonNull<()>, handle: EndpointHandle),
    pub(crate) restore_public_route_branch: unsafe fn(ptr: NonNull<()>, handle: EndpointHandle),
    pub(crate) reset_public_offer_state: unsafe fn(ptr: NonNull<()>, handle: EndpointHandle),
    pub(crate) init_public_offer_state: unsafe fn(
        ptr: NonNull<()>,
        handle: EndpointHandle,
    ) -> crate::endpoint::kernel::PublicOpLease,
    pub(crate) init_public_send_state: unsafe fn(
        ptr: NonNull<()>,
        handle: EndpointHandle,
        init: *const crate::endpoint::kernel::SendInit,
    ) -> crate::endpoint::kernel::PublicOpLease,
    pub(crate) reset_public_send_state: unsafe fn(ptr: NonNull<()>, handle: EndpointHandle),
    pub(crate) init_public_recv_state: unsafe fn(
        ptr: NonNull<()>,
        handle: EndpointHandle,
    ) -> crate::endpoint::kernel::PublicOpLease,
    pub(crate) reset_public_recv_state: unsafe fn(ptr: NonNull<()>, handle: EndpointHandle),
    pub(crate) begin_public_branch_recv_state: unsafe fn(
        ptr: NonNull<()>,
        handle: EndpointHandle,
    )
        -> crate::endpoint::kernel::PublicOpLease,
    pub(crate) reset_public_branch_recv_state: unsafe fn(ptr: NonNull<()>, handle: EndpointHandle),
    pub(crate) preview_send: unsafe fn(
        ptr: NonNull<()>,
        handle: EndpointHandle,
        logical_label: u8,
        payload_schema: u32,
        out: *mut crate::endpoint::kernel::SendPreview,
    ) -> crate::endpoint::SendResult<()>,
    pub(crate) poll_recv: for<'a, 'cx> unsafe fn(RecvPollRequest<'a, 'cx>),
    pub(crate) poll_offer: unsafe fn(
        ptr: NonNull<()>,
        handle: EndpointHandle,
        cx: &mut Context<'_>,
        out: *mut Poll<crate::endpoint::RecvResult<u8>>,
    ),
    pub(crate) poll_branch_recv: for<'a, 'cx> unsafe fn(BranchRecvPollRequest<'a, 'cx>),
    pub(crate) poll_send: unsafe fn(
        ptr: NonNull<()>,
        handle: EndpointHandle,
        payload: Option<crate::endpoint::kernel::RawSendPayload>,
        cx: &mut Context<'_>,
        out: *mut (),
    ),
}

#[derive(Clone, Copy)]
// This tag is not ownership: EndpointOwner alone owns the live lease. Keeping
// the issued role beside its nonzero generation checks a shared ops table
// without specializing the resident kernel for every role.
pub(crate) struct EndpointHandle {
    generation: core::num::NonZeroU32,
    role: u8,
}

impl EndpointHandle {
    #[inline]
    pub(crate) const fn new(generation: u32, role: u8) -> Self {
        let Some(generation) = core::num::NonZeroU32::new(generation) else {
            panic!("an endpoint handle requires a live generation");
        };
        Self { generation, role }
    }

    #[inline]
    pub(crate) const fn generation(self) -> u32 {
        self.generation.get()
    }

    #[inline]
    pub(crate) const fn role(self) -> u8 {
        self.role
    }

    #[inline]
    pub(crate) fn matches_header(self, header: &KernelEndpointHeader<'_>) -> bool {
        header.generation() == self.generation() && header.role() == self.role()
    }
}

impl<'cfg, T> crate::runtime::SessionKit<'cfg, T>
where
    T: crate::transport::Transport + 'cfg,
{
    unsafe fn resident_actor_from_header<'r>(
        ptr: NonNull<()>,
        handle: EndpointHandle,
    ) -> Option<&'r crate::endpoint::kernel::ResidentActor<'r, T>>
    where
        'cfg: 'r,
    {
        let header = /* SAFETY: endpoint carrier validates the resident header tag and generation before projecting the stored endpoint pointer. */ unsafe { ptr.cast::<KernelEndpointHeader<'r>>().as_ref() };
        if !handle.matches_header(header) {
            return None;
        }
        Some(unsafe {
            // SAFETY: the immutable header is the first repr(C) actor field.
            // Mutable progress and I/O are UnsafeCell-owned, so this shared
            // preflight never borrows the kernel before its lease is acquired.
            ptr.cast::<crate::endpoint::kernel::ResidentActor<'r, T>>()
                .as_ref()
        })
    }

    unsafe fn with_public_endpoint_mut<'r, R>(
        ptr: NonNull<()>,
        handle: EndpointHandle,
        missing: R,
        f: impl FnOnce(&mut crate::endpoint::kernel::OperationKernel<'_, 'r, T>) -> R,
    ) -> R
    where
        'cfg: 'r,
    {
        let Some(actor) = (unsafe {
            // SAFETY: this callback-level helper applies the same carrier tag,
            // generation, and validated role preflight as direct raw endpoint projection.
            Self::resident_actor_from_header::<'r>(ptr, handle)
        }) else {
            return missing;
        };
        let Some(mut operation_lease) = actor.public_header.try_operation_lease() else {
            return missing;
        };
        let mut kernel = actor.borrow_kernel(&mut operation_lease);
        f(&mut kernel)
    }

    pub(crate) const fn endpoint_ops() -> EndpointOps<'cfg> {
        EndpointOps::<'cfg> {
            _lifetime: PhantomData,
            drop_endpoint: Self::drop_public_endpoint_raw,
            restore_public_route_branch: Self::restore_public_route_branch_raw,
            reset_public_offer_state: Self::reset_public_offer_state_raw,
            init_public_offer_state: Self::init_public_offer_state_raw,
            init_public_send_state: Self::init_public_send_state_raw,
            reset_public_send_state: Self::reset_public_send_state_raw,
            init_public_recv_state: Self::init_public_recv_state_raw,
            reset_public_recv_state: Self::reset_public_recv_state_raw,
            begin_public_branch_recv_state: Self::begin_public_branch_recv_state_raw,
            reset_public_branch_recv_state: Self::reset_public_branch_recv_state_raw,
            preview_send: Self::preview_public_send,
            poll_recv: Self::poll_recv_public_endpoint,
            poll_offer: Self::poll_offer_public_endpoint,
            poll_branch_recv: Self::poll_branch_recv_public_endpoint,
            poll_send: Self::poll_send_public_endpoint,
        }
    }
}
