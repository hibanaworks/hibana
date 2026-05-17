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
    pub(crate) fn invalidate(&mut self) {
        self.generation = 0;
    }
}

#[derive(Clone, Copy)]
pub(crate) struct EndpointOps<'r> {
    _lifetime: PhantomData<&'r ()>,
    pub(crate) drop_endpoint: unsafe fn(ptr: NonNull<()>, handle: PackedEndpointHandle),
    pub(crate) revoke_for_session: unsafe fn(
        ptr: NonNull<()>,
        sid: SessionId,
        lanes: *mut Lane,
        lane_capacity: usize,
    ) -> usize,
    pub(crate) restore_public_route_branch:
        unsafe fn(ptr: NonNull<()>, handle: PackedEndpointHandle),
    pub(crate) reset_public_offer_state: unsafe fn(ptr: NonNull<()>, handle: PackedEndpointHandle),
    pub(crate) init_public_send_state: unsafe fn(
        ptr: NonNull<()>,
        handle: PackedEndpointHandle,
        init: *const crate::endpoint::kernel::SendInit,
    ),
    pub(crate) set_public_send_payload: unsafe fn(
        ptr: NonNull<()>,
        handle: PackedEndpointHandle,
        payload: *const Option<crate::endpoint::kernel::RawSendPayload>,
    ),
    pub(crate) reset_public_send_state: unsafe fn(ptr: NonNull<()>, handle: PackedEndpointHandle),
    pub(crate) init_public_recv_state: unsafe fn(ptr: NonNull<()>, handle: PackedEndpointHandle),
    pub(crate) reset_public_recv_state: unsafe fn(ptr: NonNull<()>, handle: PackedEndpointHandle),
    pub(crate) begin_public_decode_state: unsafe fn(ptr: NonNull<()>, handle: PackedEndpointHandle),
    pub(crate) reset_public_decode_state: unsafe fn(ptr: NonNull<()>, handle: PackedEndpointHandle),
    pub(crate) preview_flow: unsafe fn(
        ptr: NonNull<()>,
        handle: PackedEndpointHandle,
        logical_label: u8,
        out: *mut crate::endpoint::kernel::SendPreview,
    ) -> crate::endpoint::SendResult<()>,
    pub(crate) poll_recv: unsafe fn(
        ptr: NonNull<()>,
        handle: PackedEndpointHandle,
        logical_label: u8,
        expects_control: bool,
        accepts_empty_payload: bool,
        validate: for<'a> fn(Payload<'a>) -> Result<(), crate::transport::wire::CodecError>,
        cx: &mut Context<'_>,
        out: *mut Poll<crate::endpoint::RecvResult<RawPayload>>,
    ),
    pub(crate) poll_offer: unsafe fn(
        ptr: NonNull<()>,
        handle: PackedEndpointHandle,
        cx: &mut Context<'_>,
        out: *mut Poll<crate::endpoint::RecvResult<u8>>,
    ),
    pub(crate) poll_decode: unsafe fn(
        ptr: NonNull<()>,
        handle: PackedEndpointHandle,
        logical_label: u8,
        expects_control: bool,
        validate: for<'a> fn(Payload<'a>) -> Result<(), crate::transport::wire::CodecError>,
        synthetic: for<'a> fn(
            &'a mut [u8],
        ) -> Result<Payload<'a>, crate::transport::wire::CodecError>,
        cx: &mut Context<'_>,
        out: *mut Poll<crate::endpoint::RecvResult<RawPayload>>,
    ),
    pub(crate) poll_send: unsafe fn(
        ptr: NonNull<()>,
        handle: PackedEndpointHandle,
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

impl<'cfg, T, U, C, const MAX_RV: usize> crate::integration::SessionKit<'cfg, T, U, C, MAX_RV>
where
    T: crate::transport::Transport + 'cfg,
    U: crate::runtime::consts::LabelUniverse + 'cfg,
    C: crate::runtime::config::Clock + 'cfg,
{
    unsafe fn public_endpoint_ptr_from_header<'r, const ROLE: u8>(
        ptr: NonNull<()>,
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
        let header = unsafe { ptr.cast::<KernelEndpointHeader<'r>>().as_ref() };
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
        ptr: NonNull<()>,
        handle: PackedEndpointHandle,
    ) {
        let Some(endpoint) =
            (unsafe { Self::public_endpoint_ptr_from_header::<'cfg, ROLE>(ptr, handle) })
        else {
            return;
        };
        unsafe {
            core::ptr::drop_in_place(endpoint);
        }
    }

    unsafe fn revoke_public_endpoint_raw<const ROLE: u8>(
        ptr: NonNull<()>,
        sid: SessionId,
        lanes: *mut Lane,
        lane_capacity: usize,
    ) -> usize {
        let header = unsafe { ptr.cast::<KernelEndpointHeader<'cfg>>().as_ref() };
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
        ptr: NonNull<()>,
        handle: PackedEndpointHandle,
    ) {
        let Some(endpoint) =
            (unsafe { Self::public_endpoint_ptr_from_header::<'cfg, ROLE>(ptr, handle) })
        else {
            return;
        };
        unsafe {
            (&mut *endpoint).reset_public_offer_state();
        }
    }

    unsafe fn restore_public_route_branch_raw<const ROLE: u8>(
        ptr: NonNull<()>,
        handle: PackedEndpointHandle,
    ) {
        let Some(endpoint) =
            (unsafe { Self::public_endpoint_ptr_from_header::<'cfg, ROLE>(ptr, handle) })
        else {
            return;
        };
        unsafe {
            (&mut *endpoint).restore_public_route_branch();
        }
    }

    unsafe fn preview_public_endpoint<const ROLE: u8>(
        ptr: NonNull<()>,
        handle: PackedEndpointHandle,
        logical_label: u8,
        out: *mut crate::endpoint::kernel::SendPreview,
    ) -> crate::endpoint::SendResult<()> {
        let Some(kernel) =
            (unsafe { Self::public_endpoint_ptr_from_header::<'_, ROLE>(ptr, handle) })
        else {
            return Err(crate::endpoint::SendError::Transport(
                crate::transport::TransportError::Failed,
            ));
        };
        let preview = unsafe { (&mut *kernel).preview_flow_meta(logical_label) }?;
        unsafe {
            out.write(preview);
        }
        Ok(())
    }

    unsafe fn init_public_send_state_raw<const ROLE: u8>(
        ptr: NonNull<()>,
        handle: PackedEndpointHandle,
        init: *const crate::endpoint::kernel::SendInit,
    ) {
        let Some(kernel) =
            (unsafe { Self::public_endpoint_ptr_from_header::<'cfg, ROLE>(ptr, handle) })
        else {
            return;
        };
        unsafe {
            (&mut *kernel).init_public_send_state(&*init);
        }
    }

    unsafe fn set_public_send_payload_raw<const ROLE: u8>(
        ptr: NonNull<()>,
        handle: PackedEndpointHandle,
        payload: *const Option<crate::endpoint::kernel::RawSendPayload>,
    ) {
        let Some(kernel) =
            (unsafe { Self::public_endpoint_ptr_from_header::<'cfg, ROLE>(ptr, handle) })
        else {
            return;
        };
        unsafe {
            (&mut *kernel).set_public_send_payload(*payload);
        }
    }

    unsafe fn reset_public_send_state_raw<const ROLE: u8>(
        ptr: NonNull<()>,
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
        ptr: NonNull<()>,
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
        ptr: NonNull<()>,
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
        ptr: NonNull<()>,
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
        ptr: NonNull<()>,
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
        ptr: NonNull<()>,
        handle: PackedEndpointHandle,
        logical_label: u8,
        expects_control: bool,
        accepts_empty_payload: bool,
        validate: for<'a> fn(Payload<'a>) -> Result<(), crate::transport::wire::CodecError>,
        cx: &mut Context<'_>,
        out: *mut Poll<crate::endpoint::RecvResult<RawPayload>>,
    ) {
        let poll = if let Some(kernel) =
            unsafe { Self::public_endpoint_ptr_from_header::<'cfg, ROLE>(ptr, handle) }
        {
            match unsafe {
                (&mut *kernel).poll_public_recv(
                    logical_label,
                    expects_control,
                    accepts_empty_payload,
                    validate,
                    cx,
                )
            } {
                Poll::Pending => Poll::Pending,
                Poll::Ready(Ok(payload)) => Poll::Ready(Ok(RawPayload::from_payload(payload))),
                Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
            }
        } else {
            Poll::Ready(Err(crate::endpoint::RecvError::Transport(
                crate::transport::TransportError::Failed,
            )))
        };
        unsafe {
            out.write(poll);
        }
    }

    unsafe fn poll_offer_public_endpoint<const ROLE: u8>(
        ptr: NonNull<()>,
        handle: PackedEndpointHandle,
        cx: &mut Context<'_>,
        out: *mut Poll<crate::endpoint::RecvResult<u8>>,
    ) {
        let poll = if let Some(kernel) =
            unsafe { Self::public_endpoint_ptr_from_header::<'cfg, ROLE>(ptr, handle) }
        {
            unsafe { (&mut *kernel).poll_public_offer(cx) }
        } else {
            Poll::Ready(Err(crate::endpoint::RecvError::Transport(
                crate::transport::TransportError::Failed,
            )))
        };
        unsafe {
            out.write(poll);
        };
    }

    unsafe fn poll_decode_public_endpoint<const ROLE: u8>(
        ptr: NonNull<()>,
        handle: PackedEndpointHandle,
        logical_label: u8,
        expects_control: bool,
        validate: for<'a> fn(Payload<'a>) -> Result<(), crate::transport::wire::CodecError>,
        synthetic: for<'a> fn(
            &'a mut [u8],
        ) -> Result<Payload<'a>, crate::transport::wire::CodecError>,
        cx: &mut Context<'_>,
        out: *mut Poll<crate::endpoint::RecvResult<RawPayload>>,
    ) {
        let poll = if let Some(kernel) =
            unsafe { Self::public_endpoint_ptr_from_header::<'cfg, ROLE>(ptr, handle) }
        {
            match unsafe {
                (&mut *kernel).poll_public_decode(
                    logical_label,
                    expects_control,
                    validate,
                    synthetic,
                    cx,
                )
            } {
                Poll::Pending => Poll::Pending,
                Poll::Ready(Ok(payload)) => Poll::Ready(Ok(RawPayload::from_payload(payload))),
                Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
            }
        } else {
            Poll::Ready(Err(crate::endpoint::RecvError::Transport(
                crate::transport::TransportError::Failed,
            )))
        };
        unsafe {
            out.write(poll);
        }
    }

    unsafe fn poll_send_public_endpoint<const ROLE: u8>(
        ptr: NonNull<()>,
        handle: PackedEndpointHandle,
        cx: &mut Context<'_>,
        out: *mut (),
    ) {
        let poll = if let Some(kernel) =
            unsafe { Self::public_endpoint_ptr_from_header::<'cfg, ROLE>(ptr, handle) }
        {
            unsafe { (&mut *kernel).poll_public_send(cx) }
        } else {
            Poll::Ready(Err(crate::endpoint::SendError::Transport(
                crate::transport::TransportError::Failed,
            )))
        };
        unsafe {
            out.cast::<
                Poll<crate::endpoint::SendResult<crate::endpoint::kernel::SendControlOutcome<'cfg>>>,
            >()
            .write(poll);
        };
    }

    pub(crate) const fn endpoint_ops<const ROLE: u8>() -> EndpointOps<'cfg> {
        EndpointOps::<'cfg> {
            _lifetime: PhantomData,
            drop_endpoint: Self::drop_public_endpoint_raw::<ROLE>,
            revoke_for_session: Self::revoke_public_endpoint_raw::<ROLE>,
            restore_public_route_branch: Self::restore_public_route_branch_raw::<ROLE>,
            reset_public_offer_state: Self::reset_public_offer_state_raw::<ROLE>,
            init_public_send_state: Self::init_public_send_state_raw::<ROLE>,
            set_public_send_payload: Self::set_public_send_payload_raw::<ROLE>,
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
