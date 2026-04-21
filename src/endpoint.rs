//! Localside endpoint facade built on the typestate DSL.
//!
//! Applications interact with `Endpoint` values that are materialised from
//! `RoleProgram` projections.

use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use crate::{
    binding::BindingHandle,
    transport::{
        TransportError,
        wire::{CodecError, Payload, WirePayload},
    },
};

/// Affine endpoint helpers.
pub(crate) mod affine;
/// Crate-private carrier owners for internal endpoint type packs.
pub(crate) mod carrier;
/// Control-plane helpers for endpoints.
pub(crate) mod control;
/// Flow-based send API.
pub(crate) mod flow;
/// Internal endpoint kernel implementation.
pub(crate) mod kernel;

type EndpointBinding<'r> = BindingHandle<'r>;

#[inline]
fn validate_wire_payload<P: WirePayload>(payload: Payload<'_>) -> Result<(), CodecError> {
    P::decode_payload(payload).map(|_| ())
}

struct EndpointInner<'r, const ROLE: u8> {
    state: core::ptr::NonNull<()>,
    ops: *const (),
    handle: carrier::PackedEndpointHandle,
    generation: u32,
    _borrow: core::marker::PhantomData<&'r mut EndpointBinding<'r>>,
    _local_only: crate::local::LocalOnly,
}

/// Public endpoint facade for app-facing localside interaction.
pub struct Endpoint<'r, const ROLE: u8> {
    inner: EndpointInner<'r, ROLE>,
}

/// Public route-branch facade returned by [`Endpoint::offer`].
pub struct RouteBranch<'e, 'r, const ROLE: u8> {
    endpoint: *mut Endpoint<'r, ROLE>,
    label: u8,
    _borrow: core::marker::PhantomData<&'e mut EndpointBinding<'r>>,
    _local_only: crate::local::LocalOnly,
}

struct OfferFuture<'e, 'r, const ROLE: u8> {
    endpoint: *mut Endpoint<'r, ROLE>,
    completed: bool,
    _borrow: core::marker::PhantomData<&'e mut EndpointBinding<'r>>,
}

struct DecodeFuture<'e, 'r, const ROLE: u8, M>
where
    M: crate::global::MessageSpec,
    M::Payload: crate::transport::wire::WirePayload,
{
    endpoint: *mut Endpoint<'r, ROLE>,
    completed: bool,
    _borrow: core::marker::PhantomData<&'e mut EndpointBinding<'r>>,
    _msg: core::marker::PhantomData<M>,
}

struct RecvFuture<'e, 'r, const ROLE: u8, M>
where
    M: crate::global::MessageSpec,
    M::Payload: crate::transport::wire::WirePayload,
{
    endpoint: *mut Endpoint<'r, ROLE>,
    completed: bool,
    _borrow: core::marker::PhantomData<&'e mut EndpointBinding<'r>>,
    _msg: core::marker::PhantomData<M>,
}

impl<'e, 'r, const ROLE: u8, M> DecodeFuture<'e, 'r, ROLE, M>
where
    M: crate::global::MessageSpec,
    M::Payload: crate::transport::wire::WirePayload,
{
    #[inline]
    fn new(branch: RouteBranch<'e, 'r, ROLE>) -> Self {
        let endpoint = branch.endpoint;
        let _branch = core::mem::ManuallyDrop::new(branch);
        unsafe {
            let _ = (&mut *endpoint).begin_public_decode_state();
        }
        Self {
            endpoint,
            completed: false,
            _borrow: core::marker::PhantomData,
            _msg: core::marker::PhantomData,
        }
    }
}

impl<'e, 'r, const ROLE: u8, M> RecvFuture<'e, 'r, ROLE, M>
where
    M: crate::global::MessageSpec,
    M::Payload: crate::transport::wire::WirePayload,
{
    #[inline]
    fn new(endpoint: &'e mut Endpoint<'r, ROLE>) -> Self {
        unsafe {
            endpoint.init_public_recv_state();
        }
        Self {
            endpoint: core::ptr::from_mut(endpoint),
            completed: false,
            _borrow: core::marker::PhantomData,
            _msg: core::marker::PhantomData,
        }
    }
}

impl<'e, 'r, const ROLE: u8, M> Future for DecodeFuture<'e, 'r, ROLE, M>
where
    M: crate::global::MessageSpec,
    M::Payload: crate::transport::wire::WirePayload,
{
    type Output = RecvResult<
        <<M as crate::global::MessageSpec>::Payload as crate::transport::wire::WirePayload>::Decoded<'e>,
    >;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = unsafe { self.get_unchecked_mut() };
        let endpoint = unsafe { &mut *this.endpoint };
        let desc = kernel::DecodeDesc::new(
            <M as crate::global::MessageSpec>::LABEL,
            <M::ControlKind as crate::global::ControlPayloadKind>::IS_CONTROL,
            validate_wire_payload::<M::Payload>,
        );
        match endpoint.poll_decode(desc, cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(payload)) => {
                this.completed = true;
                let payload: Payload<'e> = unsafe { payload.into_payload() };
                Poll::Ready(
                    <<M as crate::global::MessageSpec>::Payload as crate::transport::wire::WirePayload>::decode_payload(payload)
                        .map_err(RecvError::Codec),
                )
            }
            Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
        }
    }
}

impl<'e, 'r, const ROLE: u8, M> Future for RecvFuture<'e, 'r, ROLE, M>
where
    M: crate::global::MessageSpec,
    M::Payload: crate::transport::wire::WirePayload,
{
    type Output = RecvResult<
        <<M as crate::global::MessageSpec>::Payload as crate::transport::wire::WirePayload>::Decoded<'e>,
    >;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = unsafe { self.get_unchecked_mut() };
        let endpoint = unsafe { &mut *this.endpoint };
        let desc = kernel::RecvDesc::new(
            <M as crate::global::MessageSpec>::LABEL,
            <M::Payload as WirePayload>::decode_payload(Payload::new(&[])).is_ok(),
        );
        match endpoint.poll_recv(desc, cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(payload)) => {
                this.completed = true;
                let payload: Payload<'e> = unsafe { payload.into_payload() };
                Poll::Ready(
                    <<M as crate::global::MessageSpec>::Payload as crate::transport::wire::WirePayload>::decode_payload(payload)
                        .map_err(RecvError::Codec),
                )
            }
            Poll::Ready(Err(err)) => {
                this.completed = true;
                Poll::Ready(Err(err))
            }
        }
    }
}

impl<'e, 'r, const ROLE: u8, M> Drop for DecodeFuture<'e, 'r, ROLE, M>
where
    M: crate::global::MessageSpec,
    M::Payload: crate::transport::wire::WirePayload,
{
    fn drop(&mut self) {
        if !self.completed {
            unsafe {
                (&mut *self.endpoint).reset_public_decode_state();
            }
        }
    }
}

impl<'e, 'r, const ROLE: u8, M> Drop for RecvFuture<'e, 'r, ROLE, M>
where
    M: crate::global::MessageSpec,
    M::Payload: crate::transport::wire::WirePayload,
{
    fn drop(&mut self) {
        if !self.completed {
            unsafe {
                (&mut *self.endpoint).reset_public_recv_state();
            }
        }
    }
}

impl<'r, const ROLE: u8> EndpointInner<'r, ROLE> {
    #[inline]
    fn new<K: carrier::SessionKitFamily + 'r>(
        kit: &'r K,
        handle: carrier::PackedEndpointHandle,
        generation: u32,
    ) -> Self {
        Self {
            state: core::ptr::NonNull::from(kit).cast(),
            ops: K::endpoint_ops::<ROLE>(),
            handle,
            generation,
            _borrow: core::marker::PhantomData,
            _local_only: crate::local::LocalOnly::new(),
        }
    }
}

impl<'r, const ROLE: u8> Endpoint<'r, ROLE> {
    #[inline]
    fn ops(&self) -> &carrier::EndpointOps<'r, ROLE> {
        unsafe { &*self.inner.ops.cast::<carrier::EndpointOps<'r, ROLE>>() }
    }

    #[inline]
    pub(crate) fn from_handle<K: carrier::SessionKitFamily + 'r>(
        kit: &'r K,
        handle: carrier::PackedEndpointHandle,
        generation: u32,
    ) -> Self {
        Self {
            inner: EndpointInner::new(kit, handle, generation),
        }
    }

    #[inline]
    unsafe fn drop_kernel_endpoint(&mut self) {
        unsafe {
            (self.ops().drop_endpoint)(self.inner.state, self.inner.handle, self.inner.generation);
        }
    }

    #[inline]
    unsafe fn reset_public_offer_state(&mut self) {
        unsafe {
            (self.ops().reset_public_offer_state)(
                self.inner.state,
                self.inner.handle,
                self.inner.generation,
            );
        }
    }

    #[inline]
    unsafe fn restore_public_route_branch(&mut self) {
        unsafe {
            (self.ops().restore_public_route_branch)(
                self.inner.state,
                self.inner.handle,
                self.inner.generation,
            );
        }
    }

    #[inline]
    unsafe fn init_public_send_state(
        &mut self,
        preview: kernel::SendPreview,
        payload: Option<kernel::RawSendPayload>,
    ) {
        unsafe {
            (self.ops().init_public_send_state)(
                self.inner.state,
                self.inner.handle,
                self.inner.generation,
                preview,
                payload,
            );
        }
    }

    #[inline]
    unsafe fn reset_public_send_state(&mut self) {
        unsafe {
            (self.ops().reset_public_send_state)(
                self.inner.state,
                self.inner.handle,
                self.inner.generation,
            );
        }
    }

    #[inline]
    unsafe fn init_public_recv_state(&mut self) {
        unsafe {
            (self.ops().init_public_recv_state)(
                self.inner.state,
                self.inner.handle,
                self.inner.generation,
            );
        }
    }

    #[inline]
    unsafe fn reset_public_recv_state(&mut self) {
        unsafe {
            (self.ops().reset_public_recv_state)(
                self.inner.state,
                self.inner.handle,
                self.inner.generation,
            );
        }
    }

    #[inline]
    unsafe fn begin_public_decode_state(&mut self) -> RecvResult<()> {
        unsafe {
            (self.ops().begin_public_decode_state)(
                self.inner.state,
                self.inner.handle,
                self.inner.generation,
            );
        }
        Ok(())
    }

    #[inline]
    unsafe fn reset_public_decode_state(&mut self) {
        unsafe {
            (self.ops().reset_public_decode_state)(
                self.inner.state,
                self.inner.handle,
                self.inner.generation,
            );
        }
    }
    #[inline]
    fn preview_flow(&mut self, desc: kernel::SendDesc) -> SendResult<kernel::SendPreview> {
        unsafe {
            (self.ops().preview_flow)(
                self.inner.state,
                self.inner.handle,
                self.inner.generation,
                desc,
            )
        }
    }

    #[inline]
    fn poll_recv(
        &mut self,
        desc: kernel::RecvDesc,
        cx: &mut Context<'_>,
    ) -> Poll<RecvResult<carrier::RawPayload>> {
        unsafe {
            (self.ops().poll_recv)(
                self.inner.state,
                self.inner.handle,
                self.inner.generation,
                desc,
                cx,
            )
        }
    }

    #[inline]
    fn poll_offer(&mut self, cx: &mut Context<'_>) -> Poll<RecvResult<u8>> {
        unsafe {
            (self.ops().poll_offer)(
                self.inner.state,
                self.inner.handle,
                self.inner.generation,
                cx,
            )
        }
    }

    #[inline]
    fn poll_decode(
        &mut self,
        desc: kernel::DecodeDesc,
        cx: &mut Context<'_>,
    ) -> Poll<RecvResult<carrier::RawPayload>> {
        unsafe {
            (self.ops().poll_decode)(
                self.inner.state,
                self.inner.handle,
                self.inner.generation,
                desc,
                cx,
            )
        }
    }

    #[inline]
    pub(crate) fn poll_send(
        &mut self,
        desc: kernel::SendDesc,
        cx: &mut Context<'_>,
    ) -> Poll<SendResult<kernel::SendControlOutcome<'r>>> {
        unsafe {
            (self.ops().poll_send)(
                self.inner.state,
                self.inner.handle,
                self.inner.generation,
                desc,
                cx,
            )
        }
    }

    #[inline]
    pub fn flow<'e, M>(&'e mut self) -> SendResult<flow::Flow<'e, 'r, ROLE, M>>
    where
        M: crate::global::MessageSpec + crate::global::SendableLabel,
    {
        let endpoint = core::ptr::from_mut(self);
        let desc = flow::send_desc::<M>();
        let preview = self.preview_flow(desc)?;
        Ok(flow::Flow::from_cap_flow(flow::CapFlow::new(
            endpoint, preview, desc,
        )))
    }

    #[inline]
    pub fn recv<'e, M>(
        &'e mut self,
    ) -> impl core::future::Future<
        Output = RecvResult<<<M as crate::global::MessageSpec>::Payload as crate::transport::wire::WirePayload>::Decoded<'e>>,
    > + 'e
    where
        M: crate::global::MessageSpec + 'e,
        M::Payload: crate::transport::wire::WirePayload,
    {
        RecvFuture::<'e, 'r, ROLE, M>::new(self)
    }

    #[inline]
    pub fn offer<'e>(
        &'e mut self,
    ) -> impl core::future::Future<Output = RecvResult<RouteBranch<'e, 'r, ROLE>>> + 'e {
        OfferFuture::new(self)
    }
}

impl<'e, 'r, const ROLE: u8> RouteBranch<'e, 'r, ROLE> {
    #[inline]
    pub(crate) fn from_parts(endpoint: *mut Endpoint<'r, ROLE>, label: u8) -> Self {
        Self {
            endpoint,
            label,
            _borrow: core::marker::PhantomData,
            _local_only: crate::local::LocalOnly::new(),
        }
    }

    #[inline]
    pub fn label(&self) -> u8 {
        self.label
    }

    #[inline]
    pub fn decode<M>(
        self,
    ) -> impl core::future::Future<
        Output = RecvResult<<<M as crate::global::MessageSpec>::Payload as crate::transport::wire::WirePayload>::Decoded<'e>>,
    > + use<'e, 'r, M, ROLE>
    where
        M: crate::global::MessageSpec,
        M::Payload: crate::transport::wire::WirePayload,
    {
        DecodeFuture::<'e, 'r, ROLE, M>::new(self)
    }
}

impl<'r, const ROLE: u8> Drop for Endpoint<'r, ROLE> {
    fn drop(&mut self) {
        unsafe {
            self.drop_kernel_endpoint();
        }
    }
}

impl<'e, 'r, const ROLE: u8> Drop for RouteBranch<'e, 'r, ROLE> {
    fn drop(&mut self) {
        unsafe {
            (&mut *self.endpoint).restore_public_route_branch();
        }
    }
}

impl<'e, 'r, const ROLE: u8> OfferFuture<'e, 'r, ROLE> {
    #[inline]
    fn new(endpoint: &'e mut Endpoint<'r, ROLE>) -> Self {
        let endpoint_ptr = core::ptr::from_mut(endpoint);
        Self {
            endpoint: endpoint_ptr,
            completed: false,
            _borrow: core::marker::PhantomData,
        }
    }
}

impl<'e, 'r, const ROLE: u8> Future for OfferFuture<'e, 'r, ROLE> {
    type Output = RecvResult<RouteBranch<'e, 'r, ROLE>>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        match unsafe { (&mut *this.endpoint).poll_offer(cx) } {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Err(err)) => {
                this.completed = true;
                Poll::Ready(Err(err))
            }
            Poll::Ready(Ok(label)) => {
                this.completed = true;
                Poll::Ready(Ok(RouteBranch::from_parts(this.endpoint, label)))
            }
        }
    }
}

impl<'e, 'r, const ROLE: u8> Drop for OfferFuture<'e, 'r, ROLE> {
    fn drop(&mut self) {
        if !self.completed {
            unsafe {
                (&mut *self.endpoint).reset_public_offer_state();
            }
        }
    }
}

/// Send error placeholder (will specialise once send/recv API lands).
/// Errors surfaced when sending frames through a cursor endpoint.
#[derive(Debug)]
pub enum SendError {
    /// Payload encoding failed.
    Codec(CodecError),
    /// Transport returned an error while transmitting the frame.
    Transport(TransportError),
    /// Endpoint typestate did not permit a send at this point.
    PhaseInvariant,
    /// Attempted to send a message whose label does not match the typestate step.
    LabelMismatch { expected: u8, actual: u8 },
    /// Policy VM aborted the send operation.
    PolicyAbort { reason: u16 },
    /// Binding layer hook returned an error.
    Binding,
}

/// Errors surfaced when receiving frames through a cursor endpoint.
#[derive(Debug)]
pub enum RecvError {
    /// Transport returned an error while awaiting the next frame.
    Transport(TransportError),
    /// Binding layer failed to read from channel.
    Binding(crate::binding::TransportOpsError),
    /// Payload decoding failed.
    Codec(CodecError),
    /// Endpoint typestate did not permit a receive at this point.
    PhaseInvariant,
    /// Incoming frame label did not match the typestate step.
    LabelMismatch { expected: u8, actual: u8 },
    /// Incoming frame originated from an unexpected peer role.
    PeerMismatch { expected: u8, actual: u8 },
    /// Session or lane did not match the endpoint.
    SessionMismatch {
        expected_sid: u32,
        received_sid: u32,
        expected_lane: u8,
        received_lane: u8,
    },
    /// Policy VM aborted the receive operation.
    PolicyAbort { reason: u16 },
}

/// Send result alias.
pub type SendResult<T> = core::result::Result<T, SendError>;

/// Receive result alias.
pub type RecvResult<T> = core::result::Result<T, RecvError>;

/// Errors surfaced when executing local actions.
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LocalFailureReason {
    code: u16,
}

#[cfg(test)]
impl LocalFailureReason {
    /// Internal invariant violation detected by the runtime.
    pub(crate) const INTERNAL: Self = Self::custom(0xFFFF);

    /// Create a custom failure reason (0x0000-0xFFFE reserved for user space).
    pub(crate) const fn custom(code: u16) -> Self {
        Self { code }
    }

    #[inline]
    pub(crate) const fn from_raw(raw: u16) -> Self {
        Self { code: raw }
    }
}
