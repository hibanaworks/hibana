//! Crate-private carrier markers for internal endpoint packs.
//!
//! # Unsafe Owner Contract
//!
//! This module owns raw endpoint-carrier projections. Unsafe blocks here may
//! reborrow typed payload/session references from stored raw pointers only when
//! the creating endpoint lease still owns the referenced resident image.

use core::{
    ptr::NonNull,
    task::{Context, Poll, Waker},
};

use crate::transport::wire::Payload;

mod lifecycle;
#[cfg(kani)]
#[path = "carrier/kani.rs"]
mod proofs;
mod recv;
mod route;
mod send;

pub(crate) const fn next_view_generation(current: u32) -> Option<u32> {
    if current & 1 == 0 {
        return None;
    }
    match current.checked_add(2) {
        Some(next) if next != u32::MAX => Some(next),
        Some(_) | None => None,
    }
}

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

pub(crate) enum RawRecvOutput {
    Payload(RawPayload),
    Endpoint(super::owner::EndpointOwner),
}

impl RawRecvOutput {
    fn from_kernel(output: super::transfer::RecvOutput<'_>) -> Self {
        match output {
            super::transfer::RecvOutput::Payload(payload) => {
                Self::Payload(RawPayload::from_payload(payload))
            }
            super::transfer::RecvOutput::Endpoint(owner) => Self::Endpoint(owner),
        }
    }

    pub(crate) unsafe fn into_payload<'a>(self) -> Payload<'a> {
        match self {
            Self::Payload(payload) => unsafe { payload.into_payload() },
            Self::Endpoint(_) => crate::invariant(),
        }
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
    pub(crate) payload_kind: crate::eff::EventPayload,
    pub(crate) validate:
        for<'payload> fn(Payload<'payload>) -> Result<(), crate::transport::wire::CodecError>,
    pub(crate) cx: &'a mut Context<'cx>,
    pub(crate) out: *mut Poll<crate::endpoint::RecvResult<RawRecvOutput>>,
}

pub(crate) struct BranchRecvPollRequest<'a, 'cx> {
    pub(crate) ptr: NonNull<()>,
    pub(crate) handle: EndpointHandle,
    pub(crate) logical_label: u8,
    pub(crate) payload_kind: crate::eff::EventPayload,
    pub(crate) validate:
        for<'payload> fn(Payload<'payload>) -> Result<(), crate::transport::wire::CodecError>,
    pub(crate) cx: &'a mut Context<'cx>,
    pub(crate) out: *mut Poll<crate::endpoint::RecvResult<RawRecvOutput>>,
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
pub(crate) struct KernelActorHeader {
    ops: EndpointOps,
}

impl KernelActorHeader {
    pub(crate) const fn new(ops: EndpointOps) -> Self {
        Self { ops }
    }
}

#[repr(C)]
pub(crate) struct KernelEndpointHeader {
    actor: NonNull<KernelActorHeader>,
    generation: u32,
    view_generation: core::cell::Cell<u32>,
    role: u8,
}

impl KernelEndpointHeader {
    #[inline(always)]
    pub(crate) const fn new(actor: NonNull<KernelActorHeader>, generation: u32, role: u8) -> Self {
        Self {
            actor,
            generation,
            view_generation: core::cell::Cell::new(0),
            role,
        }
    }

    #[inline(always)]
    pub(crate) const fn ops(&self) -> &EndpointOps {
        // SAFETY: the view and its common callback table occupy one pinned
        // actor allocation, retained for every issued view's lifetime.
        unsafe { &self.actor.as_ref().ops }
    }

    #[inline(always)]
    pub(crate) const fn generation(&self) -> u32 {
        if self.view_generation.get() & 1 == 1 {
            self.generation
        } else {
            0
        }
    }

    pub(crate) const fn view_generation(&self) -> u32 {
        self.view_generation.get()
    }

    pub(crate) const fn actor(&self) -> NonNull<KernelActorHeader> {
        self.actor
    }

    pub(crate) fn issue_initial(&self, generation: u32) -> bool {
        if self.view_generation.get() != 0 || generation & 1 == 0 || generation == u32::MAX {
            return false;
        }
        self.view_generation.set(generation);
        true
    }

    pub(crate) fn issue_received(&self, generation: u32) {
        if self.view_generation.get() & 1 == 1 || generation & 1 == 0 || generation == u32::MAX {
            crate::invariant();
        }
        self.view_generation.set(generation);
    }

    #[inline(always)]
    pub(crate) const fn role(&self) -> u8 {
        self.role
    }

    #[inline(always)]
    pub(crate) fn retire_generation(&self) {
        let generation = self.view_generation.get();
        if generation & 1 == 1 {
            self.view_generation
                .set(crate::invariant_some(generation.checked_add(1)));
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct EndpointOps {
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
        payload_kind: crate::eff::EventPayload,
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
        payload: Option<crate::endpoint::kernel::lane_port::SendInput>,
        cx: &mut Context<'_>,
        out: *mut (),
    ),
}

#[derive(Clone, Copy)]
// This tag is not ownership: EndpointOwner alone owns the live view. View
// generations are unique within an allocation, so the immutable view header
// supplies the logical role without a duplicate role field in every handle.
pub(crate) struct EndpointHandle {
    generation: core::num::NonZeroU32,
    view_generation: core::num::NonZeroU32,
}

impl EndpointHandle {
    #[inline]
    pub(crate) const fn new(generation: u32, view_generation: u32) -> Self {
        let Some(generation) = core::num::NonZeroU32::new(generation) else {
            panic!("an endpoint handle requires a live generation");
        };
        Self {
            generation,
            view_generation: match core::num::NonZeroU32::new(view_generation) {
                Some(value) if view_generation & 1 == 1 && view_generation != u32::MAX => value,
                _ => panic!("an endpoint handle requires an issued view generation"),
            },
        }
    }

    #[inline]
    pub(crate) const fn generation(self) -> u32 {
        self.generation.get()
    }

    #[inline]
    pub(crate) const fn view_generation(self) -> u32 {
        self.view_generation.get()
    }

    #[inline]
    pub(crate) fn matches_header(self, header: &KernelEndpointHeader) -> bool {
        header.generation() == self.generation()
            && header.view_generation() == self.view_generation()
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
        let header = /* SAFETY: endpoint carrier validates the resident header tag and generation before projecting the stored endpoint pointer. */ unsafe { ptr.cast::<KernelEndpointHeader>().as_ref() };
        if !handle.matches_header(header) {
            return None;
        }
        Some(unsafe {
            // SAFETY: the view retains its pinned actor root. The common
            // callback table is the first repr(C) actor field.
            // Mutable progress and I/O are UnsafeCell-owned, so this shared
            // preflight never borrows the kernel before its lease is acquired.
            header
                .actor()
                .cast::<crate::endpoint::kernel::ResidentActor<'r, T>>()
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
        let Some(mut operation_lease) = actor.context.try_operation_lease() else {
            return missing;
        };
        // Read the narrow header, then recover the whole view from its actor's
        // original allocation pointer. A header reborrow grants no provenance
        // for expanding that pointer over the surrounding I/O record.
        let role = unsafe { ptr.cast::<KernelEndpointHeader>().as_ref() }.role();
        let view = crate::invariant_some(actor.views.get(role));
        if core::ptr::from_ref(&view.public_header).cast::<()>() != ptr.as_ptr().cast_const() {
            crate::invariant();
        }
        let result = actor.with_kernel(&mut operation_lease, view, f);
        if !actor.drain_retired_views() || actor.context.live_views.get() == 0 {
            crate::invariant();
        }
        result
    }

    pub(crate) const fn endpoint_ops() -> EndpointOps {
        EndpointOps {
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
