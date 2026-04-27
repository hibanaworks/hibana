//! Crate-private carrier markers for internal endpoint packs.

use core::{
    marker::PhantomData,
    ptr::NonNull,
    task::{Context, Poll},
};

use crate::{
    control::types::{Lane, RendezvousId, SessionId},
    rendezvous::core::EndpointLeaseId,
    transport::wire::Payload,
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

#[repr(C)]
pub(crate) struct KernelEndpointHeader {
    ops: *const (),
    generation: u32,
    role: u8,
    _reserved: [u8; 3],
}

impl KernelEndpointHeader {
    #[inline(always)]
    pub(crate) const fn new(ops: *const (), generation: u32, role: u8) -> Self {
        Self {
            ops,
            generation,
            role,
            _reserved: [0; 3],
        }
    }

    #[inline(always)]
    pub(crate) const fn ops(&self) -> *const () {
        self.ops
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
    pub(crate) fn invalidate(&mut self) {
        self.generation = 0;
    }
}

pub(crate) struct EndpointOps<'r> {
    pub(crate) drop_endpoint:
        unsafe fn(ptr: NonNull<KernelEndpointHeader>, handle: PackedEndpointHandle),
    pub(crate) revoke_for_session: unsafe fn(
        ptr: NonNull<KernelEndpointHeader>,
        sid: SessionId,
        lanes: *mut Lane,
        lane_capacity: usize,
    ) -> usize,
    pub(crate) restore_public_route_branch:
        unsafe fn(ptr: NonNull<KernelEndpointHeader>, handle: PackedEndpointHandle),
    pub(crate) reset_public_offer_state:
        unsafe fn(ptr: NonNull<KernelEndpointHeader>, handle: PackedEndpointHandle),
    pub(crate) init_public_send_state: unsafe fn(
        ptr: NonNull<KernelEndpointHeader>,
        handle: PackedEndpointHandle,
        desc: crate::endpoint::kernel::SendRuntimeDesc,
        preview: crate::endpoint::kernel::SendPreview,
        payload: Option<crate::endpoint::kernel::RawSendPayload>,
    ),
    pub(crate) reset_public_send_state:
        unsafe fn(ptr: NonNull<KernelEndpointHeader>, handle: PackedEndpointHandle),
    pub(crate) init_public_recv_state:
        unsafe fn(ptr: NonNull<KernelEndpointHeader>, handle: PackedEndpointHandle),
    pub(crate) reset_public_recv_state:
        unsafe fn(ptr: NonNull<KernelEndpointHeader>, handle: PackedEndpointHandle),
    pub(crate) begin_public_decode_state:
        unsafe fn(ptr: NonNull<KernelEndpointHeader>, handle: PackedEndpointHandle),
    pub(crate) reset_public_decode_state:
        unsafe fn(ptr: NonNull<KernelEndpointHeader>, handle: PackedEndpointHandle),
    pub(crate) preview_flow:
        unsafe fn(
            ptr: NonNull<KernelEndpointHeader>,
            handle: PackedEndpointHandle,
            desc: crate::endpoint::kernel::SendRuntimeDesc,
        ) -> crate::endpoint::SendResult<crate::endpoint::kernel::SendPreview>,
    pub(crate) poll_recv: unsafe fn(
        ptr: NonNull<KernelEndpointHeader>,
        handle: PackedEndpointHandle,
        desc: crate::endpoint::kernel::RecvRuntimeDesc,
        cx: &mut Context<'_>,
    ) -> Poll<crate::endpoint::RecvResult<RawPayload>>,
    pub(crate) poll_offer: unsafe fn(
        ptr: NonNull<KernelEndpointHeader>,
        handle: PackedEndpointHandle,
        cx: &mut Context<'_>,
    ) -> Poll<crate::endpoint::RecvResult<u8>>,
    pub(crate) poll_decode: unsafe fn(
        ptr: NonNull<KernelEndpointHeader>,
        handle: PackedEndpointHandle,
        desc: crate::endpoint::kernel::DecodeRuntimeDesc,
        cx: &mut Context<'_>,
    ) -> Poll<crate::endpoint::RecvResult<RawPayload>>,
    pub(crate) poll_send: unsafe fn(
        ptr: NonNull<KernelEndpointHeader>,
        handle: PackedEndpointHandle,
        cx: &mut Context<'_>,
    ) -> Poll<
        crate::endpoint::SendResult<crate::endpoint::kernel::SendControlOutcome<'r>>,
    >,
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
    pub(crate) fn matches_header(self, header: &KernelEndpointHeader, role: u8) -> bool {
        header.generation() == self.generation() && header.role() == role
    }
}

pub(crate) trait SessionKitFamily {
    fn endpoint_ops<const ROLE: u8>() -> *const ();
}

impl<'cfg, T, U, C, const MAX_RV: usize> crate::substrate::SessionKit<'cfg, T, U, C, MAX_RV>
where
    T: crate::transport::Transport + 'cfg,
    U: crate::runtime::consts::LabelUniverse + 'cfg,
    C: crate::runtime::config::Clock + 'cfg,
{
    unsafe fn public_endpoint_ptr_from_header<'r, const ROLE: u8>(
        ptr: NonNull<KernelEndpointHeader>,
        handle: PackedEndpointHandle,
    ) -> Option<
        *mut crate::endpoint::kernel::CursorEndpoint<
            'r,
            ROLE,
            T,
            U,
            C,
            crate::control::cap::mint::EpochTbl,
            MAX_RV,
            crate::control::cap::mint::MintConfig,
            crate::binding::BindingHandle<'r>,
        >,
    >
    where
        'cfg: 'r,
    {
        let header = unsafe { ptr.as_ref() };
        if !handle.matches_header(header, ROLE) {
            return None;
        }
        Some(
            ptr.cast::<crate::endpoint::kernel::CursorEndpoint<
                'r,
                ROLE,
                T,
                U,
                C,
                crate::control::cap::mint::EpochTbl,
                MAX_RV,
                crate::control::cap::mint::MintConfig,
                crate::binding::BindingHandle<'r>,
            >>()
            .as_ptr(),
        )
    }

    unsafe fn drop_public_endpoint_raw<const ROLE: u8>(
        ptr: NonNull<KernelEndpointHeader>,
        handle: PackedEndpointHandle,
    ) {
        let Some(endpoint) =
            (unsafe { Self::public_endpoint_ptr_from_header::<'_, ROLE>(ptr, handle) })
        else {
            return;
        };
        unsafe {
            core::ptr::drop_in_place(endpoint);
        }
    }

    unsafe fn revoke_public_endpoint_raw<const ROLE: u8>(
        ptr: NonNull<KernelEndpointHeader>,
        sid: SessionId,
        lanes: *mut Lane,
        lane_capacity: usize,
    ) -> usize {
        let header = unsafe { ptr.as_ref() };
        if header.role() != ROLE || header.generation() == 0 {
            return 0;
        }
        let endpoint = ptr
            .cast::<crate::endpoint::kernel::CursorEndpoint<
                'cfg,
                ROLE,
                T,
                U,
                C,
                crate::control::cap::mint::EpochTbl,
                MAX_RV,
                crate::control::cap::mint::MintConfig,
                crate::binding::BindingHandle<'cfg>,
            >>()
            .as_ptr();
        let endpoint = unsafe { &mut *endpoint };
        if !endpoint.matches_session(sid) {
            return 0;
        }

        let mut released = 0usize;
        endpoint.for_each_physical_lane(|owned_lane| {
            if released < lane_capacity {
                unsafe {
                    lanes.add(released).write(owned_lane);
                }
            }
            released += 1;
        });
        debug_assert!(
            released <= lane_capacity,
            "public endpoint revoke lane buffer must cover every owned lane"
        );
        endpoint.revoke_public_owner();
        unsafe {
            core::ptr::drop_in_place(endpoint);
        }
        core::cmp::min(released, lane_capacity)
    }

    unsafe fn reset_public_offer_state_raw<const ROLE: u8>(
        ptr: NonNull<KernelEndpointHeader>,
        handle: PackedEndpointHandle,
    ) {
        let Some(endpoint) =
            (unsafe { Self::public_endpoint_ptr_from_header::<'_, ROLE>(ptr, handle) })
        else {
            return;
        };
        unsafe {
            (&mut *endpoint).reset_public_offer_state();
        }
    }

    unsafe fn restore_public_route_branch_raw<const ROLE: u8>(
        ptr: NonNull<KernelEndpointHeader>,
        handle: PackedEndpointHandle,
    ) {
        let Some(endpoint) =
            (unsafe { Self::public_endpoint_ptr_from_header::<'_, ROLE>(ptr, handle) })
        else {
            return;
        };
        unsafe {
            (&mut *endpoint).restore_public_route_branch();
        }
    }

    unsafe fn preview_public_endpoint<const ROLE: u8>(
        ptr: NonNull<KernelEndpointHeader>,
        handle: PackedEndpointHandle,
        desc: crate::endpoint::kernel::SendRuntimeDesc,
    ) -> crate::endpoint::SendResult<crate::endpoint::kernel::SendPreview> {
        let Some(kernel) =
            (unsafe { Self::public_endpoint_ptr_from_header::<'_, ROLE>(ptr, handle) })
        else {
            return Err(crate::endpoint::SendError::Transport(
                crate::transport::TransportError::Failed,
            ));
        };
        unsafe { (&mut *kernel).preview_flow_meta(desc.label()) }
    }

    unsafe fn init_public_send_state_raw<const ROLE: u8>(
        ptr: NonNull<KernelEndpointHeader>,
        handle: PackedEndpointHandle,
        desc: crate::endpoint::kernel::SendRuntimeDesc,
        preview: crate::endpoint::kernel::SendPreview,
        payload: Option<crate::endpoint::kernel::RawSendPayload>,
    ) {
        let Some(kernel) =
            (unsafe { Self::public_endpoint_ptr_from_header::<'cfg, ROLE>(ptr, handle) })
        else {
            return;
        };
        unsafe {
            (&mut *kernel).init_public_send_state(desc, preview, payload);
        }
    }

    unsafe fn reset_public_send_state_raw<const ROLE: u8>(
        ptr: NonNull<KernelEndpointHeader>,
        handle: PackedEndpointHandle,
    ) {
        let Some(kernel) =
            (unsafe { Self::public_endpoint_ptr_from_header::<'cfg, ROLE>(ptr, handle) })
        else {
            return;
        };
        unsafe {
            (&mut *kernel).reset_public_send_state();
        }
    }

    unsafe fn init_public_recv_state_raw<const ROLE: u8>(
        ptr: NonNull<KernelEndpointHeader>,
        handle: PackedEndpointHandle,
    ) {
        let Some(kernel) =
            (unsafe { Self::public_endpoint_ptr_from_header::<'cfg, ROLE>(ptr, handle) })
        else {
            return;
        };
        unsafe {
            (&mut *kernel).init_public_recv_state();
        }
    }

    unsafe fn reset_public_recv_state_raw<const ROLE: u8>(
        ptr: NonNull<KernelEndpointHeader>,
        handle: PackedEndpointHandle,
    ) {
        let Some(kernel) =
            (unsafe { Self::public_endpoint_ptr_from_header::<'cfg, ROLE>(ptr, handle) })
        else {
            return;
        };
        unsafe {
            (&mut *kernel).reset_public_recv_state();
        }
    }

    unsafe fn begin_public_decode_state_raw<const ROLE: u8>(
        ptr: NonNull<KernelEndpointHeader>,
        handle: PackedEndpointHandle,
    ) {
        let Some(kernel) =
            (unsafe { Self::public_endpoint_ptr_from_header::<'cfg, ROLE>(ptr, handle) })
        else {
            return;
        };
        unsafe {
            (&mut *kernel).begin_public_decode_state();
        }
    }

    unsafe fn reset_public_decode_state_raw<const ROLE: u8>(
        ptr: NonNull<KernelEndpointHeader>,
        handle: PackedEndpointHandle,
    ) {
        let Some(kernel) =
            (unsafe { Self::public_endpoint_ptr_from_header::<'cfg, ROLE>(ptr, handle) })
        else {
            return;
        };
        unsafe {
            (&mut *kernel).reset_public_decode_state();
        }
    }

    unsafe fn poll_recv_public_endpoint<const ROLE: u8>(
        ptr: NonNull<KernelEndpointHeader>,
        handle: PackedEndpointHandle,
        desc: crate::endpoint::kernel::RecvRuntimeDesc,
        cx: &mut Context<'_>,
    ) -> Poll<crate::endpoint::RecvResult<RawPayload>> {
        let Some(kernel) =
            (unsafe { Self::public_endpoint_ptr_from_header::<'cfg, ROLE>(ptr, handle) })
        else {
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
        ptr: NonNull<KernelEndpointHeader>,
        handle: PackedEndpointHandle,
        cx: &mut Context<'_>,
    ) -> Poll<crate::endpoint::RecvResult<u8>> {
        let Some(kernel) =
            (unsafe { Self::public_endpoint_ptr_from_header::<'cfg, ROLE>(ptr, handle) })
        else {
            return Poll::Ready(Err(crate::endpoint::RecvError::Transport(
                crate::transport::TransportError::Failed,
            )));
        };
        unsafe { (&mut *kernel).poll_public_offer(cx) }
    }

    unsafe fn poll_decode_public_endpoint<const ROLE: u8>(
        ptr: NonNull<KernelEndpointHeader>,
        handle: PackedEndpointHandle,
        desc: crate::endpoint::kernel::DecodeRuntimeDesc,
        cx: &mut Context<'_>,
    ) -> Poll<crate::endpoint::RecvResult<RawPayload>> {
        let Some(kernel) =
            (unsafe { Self::public_endpoint_ptr_from_header::<'cfg, ROLE>(ptr, handle) })
        else {
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
        ptr: NonNull<KernelEndpointHeader>,
        handle: PackedEndpointHandle,
        cx: &mut Context<'_>,
    ) -> Poll<crate::endpoint::SendResult<crate::endpoint::kernel::SendControlOutcome<'cfg>>> {
        let Some(kernel) =
            (unsafe { Self::public_endpoint_ptr_from_header::<'cfg, ROLE>(ptr, handle) })
        else {
            return Poll::Ready(Err(crate::endpoint::SendError::Transport(
                crate::transport::TransportError::Failed,
            )));
        };
        unsafe { (&mut *kernel).poll_public_send(cx) }
    }
}

impl<'cfg, T, U, C, const MAX_RV: usize> SessionKitFamily
    for crate::substrate::SessionKit<'cfg, T, U, C, MAX_RV>
where
    T: crate::transport::Transport + 'cfg,
    U: crate::runtime::consts::LabelUniverse + 'cfg,
    C: crate::runtime::config::Clock + 'cfg,
{
    fn endpoint_ops<const ROLE: u8>() -> *const () {
        let ops: *const EndpointOps<'cfg> = &EndpointOps::<'cfg> {
            drop_endpoint: Self::drop_public_endpoint_raw::<ROLE>,
            revoke_for_session: Self::revoke_public_endpoint_raw::<ROLE>,
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
