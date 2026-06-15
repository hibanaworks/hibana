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
    task::{Context, Poll},
};

use crate::{
    rendezvous::core::EndpointLeaseId, session::types::RendezvousId, transport::wire::Payload,
};

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
        let bytes = /* SAFETY: the pointer comes from pinned owner storage and this path only creates a shared borrow. */ unsafe { &*self.bytes };
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
    pub(crate) payload_mode: crate::endpoint::kernel::RecvPayloadMode,
    pub(crate) validate:
        for<'payload> fn(Payload<'payload>) -> Result<(), crate::transport::wire::CodecError>,
    pub(crate) cx: &'a mut Context<'cx>,
    pub(crate) out: *mut Poll<crate::endpoint::RecvResult<RawPayload>>,
}

pub(crate) struct DecodePollRequest<'a, 'cx> {
    pub(crate) ptr: NonNull<()>,
    pub(crate) handle: PackedEndpointHandle,
    pub(crate) logical_label: u8,
    pub(crate) validate:
        for<'payload> fn(Payload<'payload>) -> Result<(), crate::transport::wire::CodecError>,
    pub(crate) zero_payload:
        for<'payload> fn(
            &'payload mut [u8],
        ) -> Result<Payload<'payload>, crate::transport::wire::CodecError>,
    pub(crate) cx: &'a mut Context<'cx>,
    pub(crate) out: *mut Poll<crate::endpoint::RecvResult<RawPayload>>,
}

#[repr(C)]
pub(crate) struct KernelEndpointHeader<'r> {
    ops: EndpointOps<'r>,
    generation: u32,
    role: u8,
    padding: [u8; 3],
}

impl<'r> KernelEndpointHeader<'r> {
    #[inline(always)]
    pub(crate) const fn new(ops: EndpointOps<'r>, generation: u32, role: u8) -> Self {
        Self {
            ops,
            generation,
            role,
            padding: [0; 3],
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
    pub(crate) begin_public_decode_state: unsafe fn(
        ptr: NonNull<()>,
        handle: PackedEndpointHandle,
    ) -> crate::endpoint::kernel::PublicOpLease,
    pub(crate) reset_public_decode_state: unsafe fn(ptr: NonNull<()>, handle: PackedEndpointHandle),
    pub(crate) preview_flow: unsafe fn(
        ptr: NonNull<()>,
        handle: PackedEndpointHandle,
        logical_label: u8,
        out: *mut crate::endpoint::kernel::SendPreview,
    ) -> crate::endpoint::SendResult<()>,
    pub(crate) poll_recv: for<'a, 'cx> unsafe fn(RecvPollRequest<'a, 'cx>),
    pub(crate) poll_offer: unsafe fn(
        ptr: NonNull<()>,
        handle: PackedEndpointHandle,
        cx: &mut Context<'_>,
        out: *mut Poll<crate::endpoint::RecvResult<u8>>,
    ),
    pub(crate) poll_decode: for<'a, 'cx> unsafe fn(DecodePollRequest<'a, 'cx>),
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
pub(crate) struct PackedEndpointHandle(u64);

impl PackedEndpointHandle {
    #[inline]
    pub(crate) fn new(rv: RendezvousId, slot: EndpointLeaseId, generation: u32) -> Self {
        Self(((generation as u64) << 32) | ((rv.raw() as u64) << 16) | (u16::from(slot) as u64))
    }

    #[inline]
    pub(crate) const fn generation(self) -> u32 {
        (self.0 >> 32) as u32
    }

    #[inline]
    pub(crate) fn matches_header(self, header: &KernelEndpointHeader<'_>, role: u8) -> bool {
        header.generation() == self.generation() && header.role() == role
    }
}

impl<'cfg, T, const MAX_RV: usize> crate::runtime::SessionKit<'cfg, T, MAX_RV>
where
    T: crate::transport::Transport + 'cfg,
{
    unsafe fn public_endpoint_ptr_from_header<'r, const ROLE: u8>(
        ptr: NonNull<()>,
        handle: PackedEndpointHandle,
    ) -> Option<*mut crate::endpoint::kernel::CursorEndpoint<'r, ROLE, T, MAX_RV>>
    where
        'cfg: 'r,
    {
        let header = /* SAFETY: endpoint carrier validates the resident header tag and generation before projecting the stored endpoint pointer. */ unsafe { ptr.cast::<KernelEndpointHeader<'r>>().as_ref() };
        if !handle.matches_header(header, ROLE) {
            return None;
        }
        Some(
            ptr.cast::<crate::endpoint::kernel::CursorEndpoint<'r, ROLE, T, MAX_RV>>()
                .as_ptr(),
        )
    }

    unsafe fn public_endpoint_mut_from_header<'r, const ROLE: u8>(
        ptr: NonNull<()>,
        handle: PackedEndpointHandle,
    ) -> Option<&'r mut crate::endpoint::kernel::CursorEndpoint<'r, ROLE, T, MAX_RV>>
    where
        'cfg: 'r,
    {
        let endpoint = unsafe {
            // SAFETY: this helper preserves the same header tag, generation,
            // and role preflight as the raw pointer projection above.
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
        f: impl FnOnce(&mut crate::endpoint::kernel::CursorEndpoint<'r, ROLE, T, MAX_RV>) -> R,
    ) -> R
    where
        'cfg: 'r,
    {
        let Some(endpoint) = (unsafe {
            // SAFETY: this callback-level helper applies the same carrier tag,
            // generation, and role preflight as direct raw endpoint projection.
            Self::public_endpoint_mut_from_header::<'r, ROLE>(ptr, handle)
        }) else {
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
            begin_public_decode_state: Self::begin_public_decode_state_raw::<ROLE>,
            reset_public_decode_state: Self::reset_public_decode_state_raw::<ROLE>,
            preview_flow: Self::preview_public_endpoint::<ROLE>,
            poll_recv: Self::poll_recv_public_endpoint::<ROLE>,
            poll_offer: Self::poll_offer_public_endpoint::<ROLE>,
            poll_decode: Self::poll_decode_public_endpoint::<ROLE>,
            poll_send: Self::poll_send_public_endpoint::<ROLE>,
        }
    }
}
