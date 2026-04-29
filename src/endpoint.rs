//! Localside endpoint facade built on the typestate DSL.
//!
//! Applications interact with `Endpoint` values that are materialised from
//! `RoleProgram` projections.

use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use crate::transport::{
    TransportError,
    wire::{CodecError, Payload, WirePayload},
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

#[inline]
fn validate_wire_payload<P: WirePayload>(payload: Payload<'_>) -> Result<(), CodecError> {
    P::decode_payload(payload).map(|_| ())
}

#[inline]
fn synthetic_wire_payload<P: WirePayload>(scratch: &mut [u8]) -> Result<Payload<'_>, CodecError> {
    P::synthetic_payload(scratch)
}

/// Public endpoint facade for app-facing localside interaction.
pub struct Endpoint<'r, const ROLE: u8> {
    ptr: core::ptr::NonNull<carrier::KernelEndpointHeader>,
    handle: carrier::PackedEndpointHandle,
    _borrow: core::marker::PhantomData<&'r mut crate::binding::BindingHandle<'r>>,
    _local_only: crate::local::LocalOnly,
}

/// Public route-branch facade returned by [`Endpoint::offer`].
pub struct RouteBranch<'e, 'r, const ROLE: u8> {
    endpoint: *mut Endpoint<'r, ROLE>,
    label: u8,
    _borrow: core::marker::PhantomData<&'e mut crate::binding::BindingHandle<'r>>,
    _local_only: crate::local::LocalOnly,
}

struct RawOfferFuture<'e, 'r, const ROLE: u8> {
    endpoint: *mut Endpoint<'r, ROLE>,
    completed: bool,
    _borrow: core::marker::PhantomData<&'e mut crate::binding::BindingHandle<'r>>,
}

struct OfferFuture<'e, 'r, const ROLE: u8> {
    raw: RawOfferFuture<'e, 'r, ROLE>,
}

struct RawDecodeFuture<'e, 'r, const ROLE: u8> {
    endpoint: *mut Endpoint<'r, ROLE>,
    completed: bool,
    _borrow: core::marker::PhantomData<&'e mut crate::binding::BindingHandle<'r>>,
}

struct DecodeFuture<'e, 'r, const ROLE: u8, M>
where
    M: crate::global::MessageSpec,
    M::Payload: crate::transport::wire::WirePayload,
{
    raw: RawDecodeFuture<'e, 'r, ROLE>,
    _msg: core::marker::PhantomData<M>,
}

struct RawRecvFuture<'e, 'r, const ROLE: u8> {
    endpoint: *mut Endpoint<'r, ROLE>,
    completed: bool,
    _borrow: core::marker::PhantomData<&'e mut crate::binding::BindingHandle<'r>>,
}

struct RecvFuture<'e, 'r, const ROLE: u8, M>
where
    M: crate::global::MessageSpec,
    M::Payload: crate::transport::wire::WirePayload,
{
    raw: RawRecvFuture<'e, 'r, ROLE>,
    _msg: core::marker::PhantomData<M>,
}

impl<'e, 'r, const ROLE: u8> RawDecodeFuture<'e, 'r, ROLE> {
    #[inline]
    fn new(branch: RouteBranch<'e, 'r, ROLE>) -> Self {
        let endpoint = branch.endpoint;
        let branch_without_drop = core::mem::ManuallyDrop::new(branch);
        unsafe {
            let _ = (&mut *endpoint).begin_public_decode_state();
        }
        core::hint::black_box(&branch_without_drop);
        Self {
            endpoint,
            completed: false,
            _borrow: core::marker::PhantomData,
        }
    }

    #[inline]
    fn poll_raw(
        &mut self,
        logical_label: u8,
        expects_control: bool,
        validate: for<'a> fn(Payload<'a>) -> Result<(), CodecError>,
        synthetic: for<'a> fn(&'a mut [u8]) -> Result<Payload<'a>, CodecError>,
        cx: &mut Context<'_>,
    ) -> Poll<RecvResult<carrier::RawPayload>> {
        let endpoint = unsafe { &mut *self.endpoint };
        match endpoint.poll_decode(logical_label, expects_control, validate, synthetic, cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(payload)) => {
                self.completed = true;
                Poll::Ready(Ok(payload))
            }
            Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
        }
    }
}

impl<'e, 'r, const ROLE: u8> RawRecvFuture<'e, 'r, ROLE> {
    #[inline]
    fn new(endpoint: &'e mut Endpoint<'r, ROLE>) -> Self {
        unsafe {
            endpoint.init_public_recv_state();
        }
        Self {
            endpoint: core::ptr::from_mut(endpoint),
            completed: false,
            _borrow: core::marker::PhantomData,
        }
    }

    #[inline]
    fn poll_raw(
        &mut self,
        logical_label: u8,
        expects_control: bool,
        accepts_empty_payload: bool,
        cx: &mut Context<'_>,
    ) -> Poll<RecvResult<carrier::RawPayload>> {
        let endpoint = unsafe { &mut *self.endpoint };
        match endpoint.poll_recv(logical_label, expects_control, accepts_empty_payload, cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(payload)) => {
                self.completed = true;
                Poll::Ready(Ok(payload))
            }
            Poll::Ready(Err(err)) => {
                self.completed = true;
                Poll::Ready(Err(err))
            }
        }
    }
}

impl<'e, 'r, const ROLE: u8, M> DecodeFuture<'e, 'r, ROLE, M>
where
    M: crate::global::MessageSpec,
    M::Payload: crate::transport::wire::WirePayload,
{
    #[inline]
    fn new(branch: RouteBranch<'e, 'r, ROLE>) -> Self {
        Self {
            raw: RawDecodeFuture::new(branch),
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
        Self {
            raw: RawRecvFuture::new(endpoint),
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
        match this.raw.poll_raw(
            <M as crate::global::MessageSpec>::LOGICAL_LABEL,
            <M::ControlKind as crate::global::ControlPayloadKind>::IS_CONTROL,
            validate_wire_payload::<M::Payload>,
            synthetic_wire_payload::<M::Payload>,
            cx,
        ) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(payload)) => {
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
        match this.raw.poll_raw(
            <M as crate::global::MessageSpec>::LOGICAL_LABEL,
            <M::ControlKind as crate::global::ControlPayloadKind>::IS_CONTROL,
            <M::Payload as WirePayload>::decode_payload(Payload::new(&[])).is_ok(),
            cx,
        ) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(payload)) => {
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

impl<'e, 'r, const ROLE: u8> Drop for RawDecodeFuture<'e, 'r, ROLE> {
    fn drop(&mut self) {
        if !self.completed {
            unsafe {
                (&mut *self.endpoint).reset_public_decode_state();
            }
        }
    }
}

impl<'e, 'r, const ROLE: u8> Drop for RawRecvFuture<'e, 'r, ROLE> {
    fn drop(&mut self) {
        if !self.completed {
            unsafe {
                (&mut *self.endpoint).reset_public_recv_state();
            }
        }
    }
}

impl<'r, const ROLE: u8> Endpoint<'r, ROLE> {
    #[inline]
    fn new(
        ptr: core::ptr::NonNull<carrier::KernelEndpointHeader>,
        handle: carrier::PackedEndpointHandle,
    ) -> Self {
        Self {
            ptr,
            handle,
            _borrow: core::marker::PhantomData,
            _local_only: crate::local::LocalOnly::new(),
        }
    }

    #[inline]
    fn ops(&self) -> &carrier::EndpointOps<'r> {
        unsafe { &*self.ptr.as_ref().ops().cast::<carrier::EndpointOps<'r>>() }
    }

    #[inline]
    pub(crate) fn from_handle(
        ptr: core::ptr::NonNull<carrier::KernelEndpointHeader>,
        handle: carrier::PackedEndpointHandle,
    ) -> Self {
        Self::new(ptr, handle)
    }

    #[inline]
    unsafe fn drop_kernel_endpoint(&mut self) {
        unsafe {
            (self.ops().drop_endpoint)(self.ptr, self.handle);
        }
    }

    #[inline]
    unsafe fn reset_public_offer_state(&mut self) {
        unsafe {
            (self.ops().reset_public_offer_state)(self.ptr, self.handle);
        }
    }

    #[inline]
    unsafe fn restore_public_route_branch(&mut self) {
        unsafe {
            (self.ops().restore_public_route_branch)(self.ptr, self.handle);
        }
    }

    #[inline]
    unsafe fn init_public_send_state(
        &mut self,
        desc: kernel::SendRuntimeDesc,
        preview: kernel::SendPreview,
        payload: Option<kernel::RawSendPayload>,
    ) {
        unsafe {
            (self.ops().init_public_send_state)(self.ptr, self.handle, desc, preview, payload);
        }
    }

    #[inline]
    unsafe fn reset_public_send_state(&mut self) {
        unsafe {
            (self.ops().reset_public_send_state)(self.ptr, self.handle);
        }
    }

    #[inline]
    unsafe fn init_public_recv_state(&mut self) {
        unsafe {
            (self.ops().init_public_recv_state)(self.ptr, self.handle);
        }
    }

    #[inline]
    unsafe fn reset_public_recv_state(&mut self) {
        unsafe {
            (self.ops().reset_public_recv_state)(self.ptr, self.handle);
        }
    }

    #[inline]
    unsafe fn begin_public_decode_state(&mut self) -> RecvResult<()> {
        unsafe {
            (self.ops().begin_public_decode_state)(self.ptr, self.handle);
        }
        Ok(())
    }

    #[inline]
    unsafe fn reset_public_decode_state(&mut self) {
        unsafe {
            (self.ops().reset_public_decode_state)(self.ptr, self.handle);
        }
    }
    #[inline]
    fn preview_flow(
        &mut self,
        logical_label: u8,
        expects_control: bool,
        control: Option<crate::global::ControlDesc>,
        encode_control_handle: Option<
            fn(
                crate::control::types::SessionId,
                crate::control::types::Lane,
                crate::global::const_dsl::ScopeId,
            ) -> [u8; crate::control::cap::mint::CAP_HANDLE_LEN],
        >,
    ) -> SendResult<(kernel::SendPreview, kernel::SendRuntimeDesc)> {
        unsafe {
            (self.ops().preview_flow)(
                self.ptr,
                self.handle,
                logical_label,
                expects_control,
                control,
                encode_control_handle,
            )
        }
    }

    #[inline]
    fn poll_recv(
        &mut self,
        logical_label: u8,
        expects_control: bool,
        accepts_empty_payload: bool,
        cx: &mut Context<'_>,
    ) -> Poll<RecvResult<carrier::RawPayload>> {
        unsafe {
            (self.ops().poll_recv)(
                self.ptr,
                self.handle,
                logical_label,
                expects_control,
                accepts_empty_payload,
                cx,
            )
        }
    }

    #[inline]
    fn poll_offer(&mut self, cx: &mut Context<'_>) -> Poll<RecvResult<u8>> {
        unsafe { (self.ops().poll_offer)(self.ptr, self.handle, cx) }
    }

    #[inline]
    fn poll_decode(
        &mut self,
        logical_label: u8,
        expects_control: bool,
        validate: for<'a> fn(Payload<'a>) -> Result<(), CodecError>,
        synthetic: for<'a> fn(&'a mut [u8]) -> Result<Payload<'a>, CodecError>,
        cx: &mut Context<'_>,
    ) -> Poll<RecvResult<carrier::RawPayload>> {
        unsafe {
            (self.ops().poll_decode)(
                self.ptr,
                self.handle,
                logical_label,
                expects_control,
                validate,
                synthetic,
                cx,
            )
        }
    }

    #[inline]
    pub(crate) fn poll_send(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<SendResult<kernel::SendControlOutcome<'r>>> {
        unsafe { (self.ops().poll_send)(self.ptr, self.handle, cx) }
    }

    #[inline]
    pub fn flow<'e, M>(&'e mut self) -> SendResult<flow::Flow<'e, 'r, ROLE, M>>
    where
        M: crate::global::MessageSpec + crate::global::SendableLabel,
    {
        let endpoint = core::ptr::from_mut(self);
        let (logical_label, expects_control, control, encode_control_handle) =
            flow::send_runtime_parts::<M>();
        let (preview, desc) = self.preview_flow(
            logical_label,
            expects_control,
            control,
            encode_control_handle,
        )?;
        Ok(flow::Flow::new(endpoint, preview, desc))
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

impl<'e, 'r, const ROLE: u8> RawOfferFuture<'e, 'r, ROLE> {
    #[inline]
    fn new(endpoint: &'e mut Endpoint<'r, ROLE>) -> Self {
        let endpoint_ptr = core::ptr::from_mut(endpoint);
        Self {
            endpoint: endpoint_ptr,
            completed: false,
            _borrow: core::marker::PhantomData,
        }
    }

    #[inline]
    fn poll_raw(&mut self, cx: &mut Context<'_>) -> Poll<RecvResult<u8>> {
        match unsafe { (&mut *self.endpoint).poll_offer(cx) } {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Err(err)) => {
                self.completed = true;
                Poll::Ready(Err(err))
            }
            Poll::Ready(Ok(label)) => {
                self.completed = true;
                Poll::Ready(Ok(label))
            }
        }
    }
}

impl<'e, 'r, const ROLE: u8> OfferFuture<'e, 'r, ROLE> {
    #[inline]
    fn new(endpoint: &'e mut Endpoint<'r, ROLE>) -> Self {
        Self {
            raw: RawOfferFuture::new(endpoint),
        }
    }
}

impl<'e, 'r, const ROLE: u8> Future for OfferFuture<'e, 'r, ROLE> {
    type Output = RecvResult<RouteBranch<'e, 'r, ROLE>>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        match this.raw.poll_raw(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
            Poll::Ready(Ok(label)) => {
                Poll::Ready(Ok(RouteBranch::from_parts(this.raw.endpoint, label)))
            }
        }
    }
}

impl<'e, 'r, const ROLE: u8> Drop for RawOfferFuture<'e, 'r, ROLE> {
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
    /// Choreography logical label did not match the projected typestate step.
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

/// Canonical send result returned by endpoint send operations.
pub type SendResult<T> = core::result::Result<T, SendError>;

/// Canonical receive result returned by endpoint receive operations.
pub type RecvResult<T> = core::result::Result<T, RecvError>;

#[cfg(test)]
mod tests {
    use super::{DecodeFuture, Endpoint, OfferFuture, RecvFuture, RouteBranch, flow, kernel};
    use core::mem::size_of;

    type RecvFut = RecvFuture<'static, 'static, 0, crate::g::Msg<7, ()>>;
    type DecodeFut = DecodeFuture<'static, 'static, 0, crate::g::Msg<7, ()>>;
    type RecvFutU8 = RecvFuture<'static, 'static, 0, crate::g::Msg<8, u8>>;
    type RecvFutU64 = RecvFuture<'static, 'static, 0, crate::g::Msg<9, u64>>;
    type RecvFutBytes = RecvFuture<'static, 'static, 0, crate::g::Msg<10, [u8; 32]>>;
    type DecodeFutU8 = DecodeFuture<'static, 'static, 0, crate::g::Msg<11, u8>>;
    type DecodeFutU64 = DecodeFuture<'static, 'static, 0, crate::g::Msg<12, u64>>;
    type DecodeFutBytes = DecodeFuture<'static, 'static, 0, crate::g::Msg<13, [u8; 32]>>;
    type FlowU8 = flow::Flow<'static, 'static, 0, crate::g::Msg<14, u8>>;
    type FlowBytes = flow::Flow<'static, 'static, 0, crate::g::Msg<15, [u8; 32]>>;
    type SendFut = flow::SendFuture<'static, 'static, 0>;

    #[test]
    fn endpoint_surface_size_gates_hold() {
        const WORD: usize = size_of::<usize>();
        assert!(
            size_of::<Endpoint<'static, 0>>() <= 3 * WORD,
            "Endpoint<'_, ROLE> must stay within the 3-word budget"
        );
        assert!(
            size_of::<RouteBranch<'static, 'static, 0>>() <= 2 * WORD,
            "RouteBranch<'_, '_, ROLE> must stay within the 2-word budget"
        );
        assert!(
            size_of::<OfferFuture<'static, 'static, 0>>() <= 3 * WORD,
            "OfferFuture must stay within the 3-word budget"
        );
        assert!(
            size_of::<RecvFut>() <= 3 * WORD,
            "RecvFuture must stay within the 3-word budget"
        );
        assert!(
            size_of::<DecodeFut>() <= 3 * WORD,
            "DecodeFuture must stay within the 3-word budget"
        );
    }

    #[test]
    fn message_type_variation_does_not_change_future_layout() {
        assert_eq!(size_of::<RecvFut>(), size_of::<RecvFutU8>());
        assert_eq!(size_of::<RecvFut>(), size_of::<RecvFutU64>());
        assert_eq!(size_of::<RecvFut>(), size_of::<RecvFutBytes>());
        assert_eq!(size_of::<DecodeFut>(), size_of::<DecodeFutU8>());
        assert_eq!(size_of::<DecodeFut>(), size_of::<DecodeFutU64>());
        assert_eq!(size_of::<DecodeFut>(), size_of::<DecodeFutBytes>());
    }

    #[test]
    fn send_flow_and_runtime_descriptor_size_gates_hold() {
        const WORD: usize = size_of::<usize>();
        assert_eq!(
            size_of::<FlowU8>(),
            size_of::<FlowBytes>(),
            "Flow layout must remain payload-type independent",
        );
        assert!(
            size_of::<FlowU8>() <= 12 * WORD,
            "Flow must stay a thin send preview, not a transport/runtime owner",
        );
        assert!(
            size_of::<SendFut>() <= 3 * WORD,
            "SendFuture must stay within the raw-future wrapper budget",
        );
        assert!(
            size_of::<kernel::RecvRuntimeDesc>() <= WORD,
            "RecvRuntimeDesc must stay smaller than a pointer-sized descriptor",
        );
        assert!(
            size_of::<kernel::DecodeRuntimeDesc>() <= 3 * WORD,
            "DecodeRuntimeDesc must be core plus decode metadata only",
        );
        assert!(
            size_of::<kernel::SendRuntimeDesc>() <= 6 * WORD,
            "SendRuntimeDesc must be send-specific metadata, not a union descriptor",
        );
    }

    #[test]
    fn final_form_future_layout_measurement_report() {
        std::println!(
            "future-layout Endpoint={} RouteBranch={} OfferFuture={} RecvFuture={} DecodeFuture={} SendFuture={} Flow={} RecvFutureU8={} RecvFutureU64={} RecvFutureBytes={} DecodeFutureU8={} DecodeFutureU64={} DecodeFutureBytes={}",
            size_of::<Endpoint<'static, 0>>(),
            size_of::<RouteBranch<'static, 'static, 0>>(),
            size_of::<OfferFuture<'static, 'static, 0>>(),
            size_of::<RecvFut>(),
            size_of::<DecodeFut>(),
            size_of::<SendFut>(),
            size_of::<FlowU8>(),
            size_of::<RecvFutU8>(),
            size_of::<RecvFutU64>(),
            size_of::<RecvFutBytes>(),
            size_of::<DecodeFutU8>(),
            size_of::<DecodeFutU64>(),
            size_of::<DecodeFutBytes>(),
        );
    }
}
