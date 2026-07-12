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
    pub(crate) handle: PackedEndpointHandle,
    pub(crate) logical_label: u8,
    pub(crate) payload_schema: u32,
    pub(crate) validate:
        for<'payload> fn(Payload<'payload>) -> Result<(), crate::transport::wire::CodecError>,
    pub(crate) cx: &'a mut Context<'cx>,
    pub(crate) out: *mut Poll<crate::endpoint::RecvResult<RawPayload>>,
}

pub(crate) struct BranchRecvPollRequest<'a, 'cx> {
    pub(crate) ptr: NonNull<()>,
    pub(crate) handle: PackedEndpointHandle,
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
    generation: u32,
    role: u8,
}

impl<'r> KernelEndpointHeader<'r> {
    #[inline(always)]
    pub(crate) const fn new(ops: EndpointOps<'r>, generation: u32, role: u8) -> Self {
        Self {
            ops,
            generation,
            role,
        }
    }

    #[inline(always)]
    pub(crate) const fn ops(&self) -> &EndpointOps<'r> {
        &self.ops
    }

    #[inline(always)]
    pub(crate) const fn generation(&self) -> u32 {
        self.generation
    }

    #[inline(always)]
    pub(crate) const fn role(&self) -> u8 {
        self.role
    }

    #[inline(always)]
    pub(crate) fn retire_generation(&mut self) {
        self.generation = 0;
    }
}

#[derive(Clone, Copy)]
pub(crate) struct EndpointOps<'r> {
    _lifetime: PhantomData<&'r ()>,
    pub(crate) drop_endpoint: unsafe fn(ptr: NonNull<()>, handle: PackedEndpointHandle),
    pub(crate) restore_public_route_branch:
        unsafe fn(ptr: NonNull<()>, handle: PackedEndpointHandle),
    pub(crate) reset_public_offer_state: unsafe fn(ptr: NonNull<()>, handle: PackedEndpointHandle),
    pub(crate) init_public_offer_state: unsafe fn(
        ptr: NonNull<()>,
        handle: PackedEndpointHandle,
    ) -> crate::endpoint::kernel::PublicOpLease,
    pub(crate) init_public_send_state: unsafe fn(
        ptr: NonNull<()>,
        handle: PackedEndpointHandle,
        init: *const crate::endpoint::kernel::SendInit,
    ) -> crate::endpoint::kernel::PublicOpLease,
    pub(crate) reset_public_send_state: unsafe fn(ptr: NonNull<()>, handle: PackedEndpointHandle),
    pub(crate) init_public_recv_state: unsafe fn(
        ptr: NonNull<()>,
        handle: PackedEndpointHandle,
    ) -> crate::endpoint::kernel::PublicOpLease,
    pub(crate) reset_public_recv_state: unsafe fn(ptr: NonNull<()>, handle: PackedEndpointHandle),
    pub(crate) begin_public_branch_recv_state: unsafe fn(
        ptr: NonNull<()>,
        handle: PackedEndpointHandle,
    )
        -> crate::endpoint::kernel::PublicOpLease,
    pub(crate) reset_public_branch_recv_state:
        unsafe fn(ptr: NonNull<()>, handle: PackedEndpointHandle),
    pub(crate) preview_send: unsafe fn(
        ptr: NonNull<()>,
        handle: PackedEndpointHandle,
        logical_label: u8,
        payload_schema: u32,
        out: *mut crate::endpoint::kernel::SendPreview,
    ) -> crate::endpoint::SendResult<()>,
    pub(crate) poll_recv: for<'a, 'cx> unsafe fn(RecvPollRequest<'a, 'cx>),
    pub(crate) poll_offer: unsafe fn(
        ptr: NonNull<()>,
        handle: PackedEndpointHandle,
        cx: &mut Context<'_>,
        out: *mut Poll<crate::endpoint::RecvResult<u8>>,
    ),
    pub(crate) poll_branch_recv: for<'a, 'cx> unsafe fn(BranchRecvPollRequest<'a, 'cx>),
    pub(crate) poll_send: unsafe fn(
        ptr: NonNull<()>,
        handle: PackedEndpointHandle,
        payload: Option<crate::endpoint::kernel::RawSendPayload>,
        cx: &mut Context<'_>,
        out: *mut (),
    ),
}

#[repr(transparent)]
#[derive(Clone, Copy)]
pub(crate) struct PackedEndpointHandle(u32);

impl PackedEndpointHandle {
    #[inline]
    pub(crate) const fn new(generation: u32) -> Self {
        Self(generation)
    }

    #[inline]
    pub(crate) const fn generation(self) -> u32 {
        self.0
    }

    #[inline]
    pub(crate) fn matches_header(self, header: &KernelEndpointHeader<'_>, role: u8) -> bool {
        header.generation() == self.generation() && header.role() == role
    }
}

impl<'cfg, T> crate::runtime::SessionKit<'cfg, T>
where
    T: crate::transport::Transport + 'cfg,
{
    unsafe fn public_endpoint_ptr_from_header<'r, const ROLE: u8>(
        ptr: NonNull<()>,
        handle: PackedEndpointHandle,
    ) -> Option<*mut crate::endpoint::kernel::CursorEndpoint<'r, ROLE, T>>
    where
        'cfg: 'r,
    {
        let header = /* SAFETY: endpoint carrier validates the resident header tag and generation before projecting the stored endpoint pointer. */ unsafe { ptr.cast::<KernelEndpointHeader<'r>>().as_ref() };
        if !handle.matches_header(header, ROLE) {
            return None;
        }
        Some(
            ptr.cast::<crate::endpoint::kernel::CursorEndpoint<'r, ROLE, T>>()
                .as_ptr(),
        )
    }

    unsafe fn public_endpoint_mut_from_header<'r, const ROLE: u8>(
        ptr: NonNull<()>,
        handle: PackedEndpointHandle,
    ) -> Option<&'r mut crate::endpoint::kernel::CursorEndpoint<'r, ROLE, T>>
    where
        'cfg: 'r,
    {
        let endpoint = unsafe {
            // SAFETY: this helper preserves the same header tag, generation,
            // and validated role preflight as the raw pointer projection above.
            Self::public_endpoint_ptr_from_header::<'r, ROLE>(ptr, handle)?
        };
        Some(unsafe {
            // SAFETY: public endpoint raw ops enter through a unique operation
            // table callback; the carrier owns the pinned endpoint storage for
            // the duration of this projection.
            &mut *endpoint
        })
    }

    unsafe fn with_public_endpoint_mut<'r, const ROLE: u8, R>(
        ptr: NonNull<()>,
        handle: PackedEndpointHandle,
        missing: R,
        f: impl FnOnce(&mut crate::endpoint::kernel::CursorEndpoint<'r, ROLE, T>) -> R,
    ) -> R
    where
        'cfg: 'r,
    {
        let Some(endpoint) = (unsafe {
            // SAFETY: this callback-level helper applies the same carrier tag,
            // generation, and validated role preflight as direct raw endpoint projection.
            Self::public_endpoint_mut_from_header::<'r, ROLE>(ptr, handle)
        }) else {
            return missing;
        };
        let Some(_operation_lease) = endpoint.try_public_operation_lease() else {
            return missing;
        };
        f(endpoint)
    }

    pub(crate) const fn endpoint_ops<const ROLE: u8>() -> EndpointOps<'cfg> {
        EndpointOps::<'cfg> {
            _lifetime: PhantomData,
            drop_endpoint: Self::drop_public_endpoint_raw::<ROLE>,
            restore_public_route_branch: Self::restore_public_route_branch_raw::<ROLE>,
            reset_public_offer_state: Self::reset_public_offer_state_raw::<ROLE>,
            init_public_offer_state: Self::init_public_offer_state_raw::<ROLE>,
            init_public_send_state: Self::init_public_send_state_raw::<ROLE>,
            reset_public_send_state: Self::reset_public_send_state_raw::<ROLE>,
            init_public_recv_state: Self::init_public_recv_state_raw::<ROLE>,
            reset_public_recv_state: Self::reset_public_recv_state_raw::<ROLE>,
            begin_public_branch_recv_state: Self::begin_public_branch_recv_state_raw::<ROLE>,
            reset_public_branch_recv_state: Self::reset_public_branch_recv_state_raw::<ROLE>,
            preview_send: Self::preview_public_send::<ROLE>,
            poll_recv: Self::poll_recv_public_endpoint::<ROLE>,
            poll_offer: Self::poll_offer_public_endpoint::<ROLE>,
            poll_branch_recv: Self::poll_branch_recv_public_endpoint::<ROLE>,
            poll_send: Self::poll_send_public_endpoint::<ROLE>,
        }
    }
}
