//! Crate-private carrier markers and alias owners for internal endpoint packs.

use core::{
    marker::PhantomData,
    ptr::NonNull,
    task::{Context, Poll},
};

use crate::{
    control::types::RendezvousId, rendezvous::core::EndpointLeaseId, transport::wire::Payload,
};

pub(crate) struct SessionCfg<K>(pub(crate) PhantomData<fn() -> K>);

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
        let bytes = unsafe { &*self.bytes };
        Payload::new(bytes)
    }
}

pub(crate) struct EndpointOps<'r, const ROLE: u8> {
    pub(crate) drop_endpoint:
        unsafe fn(state: NonNull<()>, handle: PackedEndpointHandle, generation: u32),
    pub(crate) restore_public_route_branch:
        unsafe fn(state: NonNull<()>, handle: PackedEndpointHandle, generation: u32),
    pub(crate) reset_public_offer_state:
        unsafe fn(state: NonNull<()>, handle: PackedEndpointHandle, generation: u32),
    pub(crate) init_public_send_state: unsafe fn(
        state: NonNull<()>,
        handle: PackedEndpointHandle,
        generation: u32,
        preview: crate::endpoint::kernel::SendPreview,
        payload: Option<crate::endpoint::kernel::RawSendPayload>,
    ),
    pub(crate) reset_public_send_state:
        unsafe fn(state: NonNull<()>, handle: PackedEndpointHandle, generation: u32),
    pub(crate) init_public_recv_state:
        unsafe fn(state: NonNull<()>, handle: PackedEndpointHandle, generation: u32),
    pub(crate) reset_public_recv_state:
        unsafe fn(state: NonNull<()>, handle: PackedEndpointHandle, generation: u32),
    pub(crate) begin_public_decode_state:
        unsafe fn(state: NonNull<()>, handle: PackedEndpointHandle, generation: u32),
    pub(crate) reset_public_decode_state:
        unsafe fn(state: NonNull<()>, handle: PackedEndpointHandle, generation: u32),
    pub(crate) preview_flow:
        unsafe fn(
            state: NonNull<()>,
            handle: PackedEndpointHandle,
            generation: u32,
            desc: crate::endpoint::kernel::SendDesc,
        ) -> crate::endpoint::SendResult<crate::endpoint::kernel::SendPreview>,
    pub(crate) poll_recv: unsafe fn(
        state: NonNull<()>,
        handle: PackedEndpointHandle,
        generation: u32,
        desc: crate::endpoint::kernel::RecvDesc,
        cx: &mut Context<'_>,
    ) -> Poll<crate::endpoint::RecvResult<RawPayload>>,
    pub(crate) poll_offer: unsafe fn(
        state: NonNull<()>,
        handle: PackedEndpointHandle,
        generation: u32,
        cx: &mut Context<'_>,
    ) -> Poll<crate::endpoint::RecvResult<u8>>,
    pub(crate) poll_decode: unsafe fn(
        state: NonNull<()>,
        handle: PackedEndpointHandle,
        generation: u32,
        desc: crate::endpoint::kernel::DecodeDesc,
        cx: &mut Context<'_>,
    ) -> Poll<crate::endpoint::RecvResult<RawPayload>>,
    pub(crate) poll_send: unsafe fn(
        state: NonNull<()>,
        handle: PackedEndpointHandle,
        generation: u32,
        desc: crate::endpoint::kernel::SendDesc,
        cx: &mut Context<'_>,
    ) -> Poll<
        crate::endpoint::SendResult<crate::endpoint::kernel::SendControlOutcome<'r>>,
    >,
}

#[repr(transparent)]
#[derive(Clone, Copy)]
pub(crate) struct PackedEndpointHandle(u32);

impl PackedEndpointHandle {
    #[inline]
    pub(crate) fn new(rv: RendezvousId, slot: EndpointLeaseId) -> Self {
        Self(((rv.raw() as u32) << 16) | (u16::from(slot) as u32))
    }

    #[inline]
    pub(crate) fn rendezvous(self) -> RendezvousId {
        RendezvousId::new((self.0 >> 16) as u16)
    }

    #[inline]
    pub(crate) fn slot(self) -> EndpointLeaseId {
        EndpointLeaseId::from(self.0 as u16)
    }
}

pub(crate) trait SessionKitFamily {
    type Transport;
    type LabelUniverse;
    type Clock;

    type KernelSessionCluster<'cfg>
    where
        Self: 'cfg;

    type KernelCursorEndpoint<'r, const ROLE: u8, E, Mint, B>
    where
        Self: 'r,
        E: crate::control::cap::mint::EpochTable + 'r,
        Mint: crate::control::cap::mint::MintConfigMarker,
        B: crate::binding::BindingSlot + 'r;

    fn endpoint_ops<const ROLE: u8>() -> *const ();
}

pub(crate) type KernelCursorEndpoint<'r, const ROLE: u8, K, E, Mint, B> =
    <K as SessionKitFamily>::KernelCursorEndpoint<'r, ROLE, E, Mint, B>;

type EndpointBinding<'r> = crate::binding::BindingHandle<'r>;
type PublicKernelEndpoint<'r, const ROLE: u8, T, U, C, const MAX_RV: usize> =
    crate::endpoint::kernel::CursorEndpoint<
        'r,
        ROLE,
        T,
        U,
        C,
        crate::control::cap::mint::EpochTbl,
        MAX_RV,
        crate::control::cap::mint::MintConfig,
        EndpointBinding<'r>,
    >;

impl<'cfg, T, U, C, const MAX_RV: usize> crate::substrate::SessionKit<'cfg, T, U, C, MAX_RV>
where
    T: crate::transport::Transport + 'cfg,
    U: crate::runtime::consts::LabelUniverse + 'cfg,
    C: crate::runtime::config::Clock + 'cfg,
{
    unsafe fn public_endpoint_ptr_from_parts<'r, const ROLE: u8>(
        state: NonNull<()>,
        handle: PackedEndpointHandle,
        generation: u32,
    ) -> Option<*mut PublicKernelEndpoint<'r, ROLE, T, U, C, MAX_RV>>
    where
        'cfg: 'r,
    {
        let kit = unsafe { state.cast::<Self>().as_ref() };
        unsafe {
            kit.public_endpoint_kernel_ptr::<ROLE, crate::control::cap::mint::MintConfig>(
                handle, generation,
            )
        }
    }

    unsafe fn drop_public_endpoint_raw<const ROLE: u8>(
        state: NonNull<()>,
        handle: PackedEndpointHandle,
        generation: u32,
    ) {
        let kit = unsafe { state.cast::<Self>().as_ref() };
        let Some(endpoint) = (unsafe {
            kit.public_endpoint_kernel_ptr::<ROLE, crate::control::cap::mint::MintConfig>(
                handle, generation,
            )
        }) else {
            return;
        };
        unsafe {
            core::ptr::drop_in_place(endpoint);
        }
    }

    unsafe fn reset_public_offer_state_raw<const ROLE: u8>(
        state: NonNull<()>,
        handle: PackedEndpointHandle,
        generation: u32,
    ) {
        let kit = unsafe { state.cast::<Self>().as_ref() };
        let Some(endpoint) = (unsafe {
            kit.public_endpoint_kernel_ptr::<ROLE, crate::control::cap::mint::MintConfig>(
                handle, generation,
            )
        }) else {
            return;
        };
        unsafe {
            (&mut *endpoint).reset_public_offer_state();
        }
    }

    unsafe fn restore_public_route_branch_raw<const ROLE: u8>(
        state: NonNull<()>,
        handle: PackedEndpointHandle,
        generation: u32,
    ) {
        let kit = unsafe { state.cast::<Self>().as_ref() };
        let Some(endpoint) = (unsafe {
            kit.public_endpoint_kernel_ptr::<ROLE, crate::control::cap::mint::MintConfig>(
                handle, generation,
            )
        }) else {
            return;
        };
        unsafe {
            (&mut *endpoint).restore_public_route_branch();
        }
    }

    unsafe fn preview_public_endpoint<const ROLE: u8>(
        state: NonNull<()>,
        handle: PackedEndpointHandle,
        generation: u32,
        desc: crate::endpoint::kernel::SendDesc,
    ) -> crate::endpoint::SendResult<crate::endpoint::kernel::SendPreview> {
        let Some(kernel) = (unsafe {
            Self::public_endpoint_ptr_from_parts::<'_, ROLE>(state, handle, generation)
        }) else {
            return Err(crate::endpoint::SendError::Transport(
                crate::transport::TransportError::Failed,
            ));
        };
        unsafe { (&mut *kernel).preview_flow_meta(desc.label()) }
    }

    unsafe fn init_public_send_state_raw<const ROLE: u8>(
        state: NonNull<()>,
        handle: PackedEndpointHandle,
        generation: u32,
        preview: crate::endpoint::kernel::SendPreview,
        payload: Option<crate::endpoint::kernel::RawSendPayload>,
    ) {
        let Some(kernel) = (unsafe {
            Self::public_endpoint_ptr_from_parts::<'cfg, ROLE>(state, handle, generation)
        }) else {
            return;
        };
        unsafe {
            (&mut *kernel).init_public_send_state(preview, payload);
        }
    }

    unsafe fn reset_public_send_state_raw<const ROLE: u8>(
        state: NonNull<()>,
        handle: PackedEndpointHandle,
        generation: u32,
    ) {
        let Some(kernel) = (unsafe {
            Self::public_endpoint_ptr_from_parts::<'cfg, ROLE>(state, handle, generation)
        }) else {
            return;
        };
        unsafe {
            (&mut *kernel).reset_public_send_state();
        }
    }

    unsafe fn init_public_recv_state_raw<const ROLE: u8>(
        state: NonNull<()>,
        handle: PackedEndpointHandle,
        generation: u32,
    ) {
        let Some(kernel) = (unsafe {
            Self::public_endpoint_ptr_from_parts::<'cfg, ROLE>(state, handle, generation)
        }) else {
            return;
        };
        unsafe {
            (&mut *kernel).init_public_recv_state();
        }
    }

    unsafe fn reset_public_recv_state_raw<const ROLE: u8>(
        state: NonNull<()>,
        handle: PackedEndpointHandle,
        generation: u32,
    ) {
        let Some(kernel) = (unsafe {
            Self::public_endpoint_ptr_from_parts::<'cfg, ROLE>(state, handle, generation)
        }) else {
            return;
        };
        unsafe {
            (&mut *kernel).reset_public_recv_state();
        }
    }

    unsafe fn begin_public_decode_state_raw<const ROLE: u8>(
        state: NonNull<()>,
        handle: PackedEndpointHandle,
        generation: u32,
    ) {
        let Some(kernel) = (unsafe {
            Self::public_endpoint_ptr_from_parts::<'cfg, ROLE>(state, handle, generation)
        }) else {
            return;
        };
        unsafe {
            (&mut *kernel).begin_public_decode_state();
        }
    }

    unsafe fn reset_public_decode_state_raw<const ROLE: u8>(
        state: NonNull<()>,
        handle: PackedEndpointHandle,
        generation: u32,
    ) {
        let Some(kernel) = (unsafe {
            Self::public_endpoint_ptr_from_parts::<'cfg, ROLE>(state, handle, generation)
        }) else {
            return;
        };
        unsafe {
            (&mut *kernel).reset_public_decode_state();
        }
    }

    unsafe fn poll_recv_public_endpoint<const ROLE: u8>(
        state: NonNull<()>,
        handle: PackedEndpointHandle,
        generation: u32,
        desc: crate::endpoint::kernel::RecvDesc,
        cx: &mut Context<'_>,
    ) -> Poll<crate::endpoint::RecvResult<RawPayload>> {
        let Some(kernel) = (unsafe {
            Self::public_endpoint_ptr_from_parts::<'cfg, ROLE>(state, handle, generation)
        }) else {
            return Poll::Ready(Err(crate::endpoint::RecvError::Transport(
                crate::transport::TransportError::Failed,
            )));
        };
        match unsafe { (&mut *kernel).poll_public_recv(desc, cx) } {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(payload)) => Poll::Ready(Ok(RawPayload::from_payload(payload))),
            Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
        }
    }

    unsafe fn poll_offer_public_endpoint<const ROLE: u8>(
        state: NonNull<()>,
        handle: PackedEndpointHandle,
        generation: u32,
        cx: &mut Context<'_>,
    ) -> Poll<crate::endpoint::RecvResult<u8>> {
        let Some(kernel) = (unsafe {
            Self::public_endpoint_ptr_from_parts::<'cfg, ROLE>(state, handle, generation)
        }) else {
            return Poll::Ready(Err(crate::endpoint::RecvError::Transport(
                crate::transport::TransportError::Failed,
            )));
        };
        unsafe { (&mut *kernel).poll_public_offer(cx) }
    }

    unsafe fn poll_decode_public_endpoint<const ROLE: u8>(
        state: NonNull<()>,
        handle: PackedEndpointHandle,
        generation: u32,
        desc: crate::endpoint::kernel::DecodeDesc,
        cx: &mut Context<'_>,
    ) -> Poll<crate::endpoint::RecvResult<RawPayload>> {
        let Some(kernel) = (unsafe {
            Self::public_endpoint_ptr_from_parts::<'cfg, ROLE>(state, handle, generation)
        }) else {
            return Poll::Ready(Err(crate::endpoint::RecvError::Transport(
                crate::transport::TransportError::Failed,
            )));
        };
        match unsafe { (&mut *kernel).poll_public_decode(desc, cx) } {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(payload)) => Poll::Ready(Ok(RawPayload::from_payload(payload))),
            Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
        }
    }

    unsafe fn poll_send_public_endpoint<const ROLE: u8>(
        state: NonNull<()>,
        handle: PackedEndpointHandle,
        generation: u32,
        desc: crate::endpoint::kernel::SendDesc,
        cx: &mut Context<'_>,
    ) -> Poll<crate::endpoint::SendResult<crate::endpoint::kernel::SendControlOutcome<'cfg>>> {
        let Some(kernel) = (unsafe {
            Self::public_endpoint_ptr_from_parts::<'cfg, ROLE>(state, handle, generation)
        }) else {
            return Poll::Ready(Err(crate::endpoint::SendError::Transport(
                crate::transport::TransportError::Failed,
            )));
        };
        unsafe { (&mut *kernel).poll_public_send(desc, cx) }
    }
}

impl<'cfg, T, U, C, const MAX_RV: usize> SessionKitFamily
    for crate::substrate::SessionKit<'cfg, T, U, C, MAX_RV>
where
    T: crate::transport::Transport + 'cfg,
    U: crate::runtime::consts::LabelUniverse + 'cfg,
    C: crate::runtime::config::Clock + 'cfg,
{
    type Transport = T;
    type LabelUniverse = U;
    type Clock = C;

    type KernelSessionCluster<'lease>
        = crate::control::cluster::core::SessionCluster<'lease, T, U, C, MAX_RV>
    where
        Self: 'lease;

    type KernelCursorEndpoint<'r, const ROLE: u8, E, Mint, B>
        = crate::endpoint::kernel::CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
    where
        Self: 'r,
        E: crate::control::cap::mint::EpochTable + 'r,
        Mint: crate::control::cap::mint::MintConfigMarker,
        B: crate::binding::BindingSlot + 'r;

    fn endpoint_ops<const ROLE: u8>() -> *const () {
        let ops: *const EndpointOps<'cfg, ROLE> = &EndpointOps::<'cfg, ROLE> {
            drop_endpoint: Self::drop_public_endpoint_raw::<ROLE>,
            restore_public_route_branch: Self::restore_public_route_branch_raw::<ROLE>,
            reset_public_offer_state: Self::reset_public_offer_state_raw::<ROLE>,
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
        };
        ops.cast::<()>()
    }
}
