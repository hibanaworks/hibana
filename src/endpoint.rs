//! Localside endpoint facade.
//!
//! An [`Endpoint`] is the app-facing affine executor for one projected role. It
//! is created by [`crate::integration::SessionKit`] and then advanced with the
//! four localside operations: [`Endpoint::flow`], [`Endpoint::recv`],
//! [`Endpoint::offer`], and [`RouteBranch::decode`].
//!
//! `flow` and `offer` are non-consuming previews. Committed progress happens
//! when a send or route decode succeeds. Committed endpoint failures return
//! [`EndpointError`] as diagnostic evidence and poison the current session
//! generation; they do not authorize hidden fallback progress.

use core::{
    fmt,
    future::Future,
    panic::Location,
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

/// App-facing affine executor for a projected role.
///
/// The endpoint is intentionally local-only and moves forward one descriptor
/// step at a time. Successful sends and route decodes consume progress. Dropped
/// send/route previews restore the endpoint to its previous step. Once a
/// committed fault is observed, the same session generation cannot produce a
/// new continuation.
pub struct Endpoint<'r, const ROLE: u8> {
    ptr: core::ptr::NonNull<carrier::KernelEndpointHeader>,
    handle: carrier::PackedEndpointHandle,
    _borrow: core::marker::PhantomData<&'r mut crate::binding::BindingHandle<'r>>,
    _local_only: crate::local::LocalOnly,
}

/// Preview of a selected route branch returned by [`Endpoint::offer`].
///
/// `RouteBranch` exposes the selected logical label. If the selected arm begins
/// with a receive, call [`RouteBranch::decode`]. If it begins with a send, drop
/// the branch preview and call [`Endpoint::flow`] for that arm's first message.
/// The label is descriptor/resolver evidence, not the result of parsing payload
/// bytes.
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
    location: ErrorLocation,
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
    location: ErrorLocation,
    _msg: core::marker::PhantomData<M>,
}

struct RawRecvFuture<'e, 'r, const ROLE: u8> {
    endpoint: *mut Endpoint<'r, ROLE>,
    flags: RawRecvFlags,
    _borrow: core::marker::PhantomData<&'e mut crate::binding::BindingHandle<'r>>,
}

#[derive(Clone, Copy)]
struct RawRecvFlags(u8);

impl RawRecvFlags {
    const COMPLETED: u8 = 1 << 0;
    const ACCEPTS_EMPTY_PAYLOAD: u8 = 1 << 1;

    #[inline]
    const fn new(accepts_empty_payload: bool) -> Self {
        Self(if accepts_empty_payload {
            Self::ACCEPTS_EMPTY_PAYLOAD
        } else {
            0
        })
    }

    #[inline]
    fn mark_completed(&mut self) {
        self.0 |= Self::COMPLETED;
    }

    #[inline]
    const fn completed(self) -> bool {
        self.0 & Self::COMPLETED != 0
    }

    #[inline]
    const fn accepts_empty_payload(self) -> bool {
        self.0 & Self::ACCEPTS_EMPTY_PAYLOAD != 0
    }
}

struct RecvFuture<'e, 'r, const ROLE: u8, M>
where
    M: crate::global::MessageSpec,
    M::Payload: crate::transport::wire::WirePayload,
{
    raw: RawRecvFuture<'e, 'r, ROLE>,
    location: ErrorLocation,
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
    fn new(endpoint: &'e mut Endpoint<'r, ROLE>, accepts_empty_payload: bool) -> Self {
        unsafe {
            endpoint.init_public_recv_state();
        }
        Self {
            endpoint: core::ptr::from_mut(endpoint),
            flags: RawRecvFlags::new(accepts_empty_payload),
            _borrow: core::marker::PhantomData,
        }
    }

    #[inline]
    fn poll_raw(
        &mut self,
        logical_label: u8,
        expects_control: bool,
        validate: for<'a> fn(Payload<'a>) -> Result<(), CodecError>,
        cx: &mut Context<'_>,
    ) -> Poll<RecvResult<carrier::RawPayload>> {
        let endpoint = unsafe { &mut *self.endpoint };
        match endpoint.poll_recv(
            logical_label,
            expects_control,
            self.flags.accepts_empty_payload(),
            validate,
            cx,
        ) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(payload)) => {
                self.flags.mark_completed();
                Poll::Ready(Ok(payload))
            }
            Poll::Ready(Err(err)) => {
                self.flags.mark_completed();
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
    fn new(branch: RouteBranch<'e, 'r, ROLE>, location: ErrorLocation) -> Self {
        Self {
            raw: RawDecodeFuture::new(branch),
            location,
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
    fn new(endpoint: &'e mut Endpoint<'r, ROLE>, location: ErrorLocation) -> Self {
        let accepts_empty_payload =
            <M::Payload as WirePayload>::decode_payload(Payload::new(&[])).is_ok();
        Self {
            raw: RawRecvFuture::new(endpoint, accepts_empty_payload),
            location,
            _msg: core::marker::PhantomData,
        }
    }
}

impl<'e, 'r, const ROLE: u8, M> Future for DecodeFuture<'e, 'r, ROLE, M>
where
    M: crate::global::MessageSpec,
    M::Payload: crate::transport::wire::WirePayload,
{
    type Output = EndpointResult<
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
                let decoded =
                    <<M as crate::global::MessageSpec>::Payload as crate::transport::wire::WirePayload>::decode_payload(payload);
                Poll::Ready(match decoded {
                    Ok(decoded) => Ok(decoded),
                    Err(error) => Err(EndpointError::new(
                        EndpointOp::Decode,
                        this.location,
                        RecvError::Codec(error),
                    )),
                })
            }
            Poll::Ready(Err(err)) => Poll::Ready(Err(EndpointError::new(
                EndpointOp::Decode,
                this.location,
                err,
            ))),
        }
    }
}

impl<'e, 'r, const ROLE: u8, M> Future for RecvFuture<'e, 'r, ROLE, M>
where
    M: crate::global::MessageSpec,
    M::Payload: crate::transport::wire::WirePayload,
{
    type Output = EndpointResult<
        <<M as crate::global::MessageSpec>::Payload as crate::transport::wire::WirePayload>::Decoded<'e>,
    >;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = unsafe { self.get_unchecked_mut() };
        match this.raw.poll_raw(
            <M as crate::global::MessageSpec>::LOGICAL_LABEL,
            <M::ControlKind as crate::global::ControlPayloadKind>::IS_CONTROL,
            validate_wire_payload::<M::Payload>,
            cx,
        ) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(payload)) => {
                let payload: Payload<'e> = unsafe { payload.into_payload() };
                let decoded =
                    <<M as crate::global::MessageSpec>::Payload as crate::transport::wire::WirePayload>::decode_payload(payload);
                Poll::Ready(match decoded {
                    Ok(decoded) => Ok(decoded),
                    Err(error) => Err(EndpointError::new(
                        EndpointOp::Recv,
                        this.location,
                        RecvError::Codec(error),
                    )),
                })
            }
            Poll::Ready(Err(err)) => Poll::Ready(Err(EndpointError::new(
                EndpointOp::Recv,
                this.location,
                err,
            ))),
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
        if !self.flags.completed() {
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
        validate: for<'a> fn(Payload<'a>) -> Result<(), CodecError>,
        cx: &mut Context<'_>,
    ) -> Poll<RecvResult<carrier::RawPayload>> {
        unsafe {
            (self.ops().poll_recv)(
                self.ptr,
                self.handle,
                logical_label,
                expects_control,
                accepts_empty_payload,
                validate,
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
    /// Preview the next send for message `M`.
    ///
    /// The returned flow value must be consumed with `.send(...)` to make
    /// progress. Dropping it leaves the endpoint on the same typestate step. A
    /// preview mismatch reports [`EndpointError`] at this callsite and must not
    /// be treated as permission to choose another branch.
    #[track_caller]
    pub fn flow<'e, M>(&'e mut self) -> EndpointResult<flow::Flow<'e, 'r, ROLE, M>>
    where
        M: crate::global::MessageSpec + crate::global::SendableLabel,
    {
        let location = ErrorLocation::caller();
        let endpoint = core::ptr::from_mut(self);
        let (logical_label, expects_control, control, encode_control_handle) =
            flow::send_runtime_parts::<M>();
        let (preview, desc) = match self.preview_flow(
            logical_label,
            expects_control,
            control,
            encode_control_handle,
        ) {
            Ok(parts) => parts,
            Err(error) => return Err(EndpointError::new(EndpointOp::Flow, location, error)),
        };
        Ok(flow::Flow::new(endpoint, preview, desc))
    }

    #[inline]
    /// Receive the next deterministic message as `M`.
    ///
    /// The projected descriptor must expect the same choreography label and
    /// control kind as `M`. Payload decoding is exact: fixed-size payloads reject
    /// trailing bytes, while borrowed payloads may return views tied to the
    /// endpoint borrow. A committed receive fault poisons the session generation
    /// before the error is returned.
    #[track_caller]
    pub fn recv<'e, M>(
        &'e mut self,
    ) -> impl core::future::Future<
        Output = EndpointResult<<<M as crate::global::MessageSpec>::Payload as crate::transport::wire::WirePayload>::Decoded<'e>>,
    > + 'e
    where
        M: crate::global::MessageSpec + 'e,
        M::Payload: crate::transport::wire::WirePayload,
    {
        RecvFuture::<'e, 'r, ROLE, M>::new(self, ErrorLocation::caller())
    }

    #[inline]
    /// Observe the next route decision.
    ///
    /// This is a preview operation. It returns a [`RouteBranch`] whose
    /// [`RouteBranch::label`] is the selected choreography branch label.
    /// Dropping the future before completion leaves endpoint progress unchanged.
    /// Dynamic branches must be selected by an explicit resolver decision at a
    /// projected route point; transport hints and payload labels are demux
    /// evidence only.
    #[track_caller]
    pub fn offer<'e>(
        &'e mut self,
    ) -> impl core::future::Future<Output = EndpointResult<RouteBranch<'e, 'r, ROLE>>> + 'e {
        OfferFuture::new(self, ErrorLocation::caller())
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
    /// Return the selected choreography label for this route branch.
    pub fn label(&self) -> u8 {
        self.label
    }

    #[inline]
    /// Receive the first payload of a selected route arm.
    ///
    /// This consumes the branch preview on success. The message `M` must match
    /// the selected branch label and control kind. Physical frame-label or
    /// descriptor mismatches are reported as invariant failures, not as route
    /// choices. A decode failure is terminal for the current generation.
    #[track_caller]
    pub fn decode<M>(
        self,
    ) -> impl core::future::Future<
        Output = EndpointResult<<<M as crate::global::MessageSpec>::Payload as crate::transport::wire::WirePayload>::Decoded<'e>>,
    > + use<'e, 'r, M, ROLE>
    where
        M: crate::global::MessageSpec,
        M::Payload: crate::transport::wire::WirePayload,
    {
        DecodeFuture::<'e, 'r, ROLE, M>::new(self, ErrorLocation::caller())
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
    fn new(endpoint: &'e mut Endpoint<'r, ROLE>, location: ErrorLocation) -> Self {
        Self {
            raw: RawOfferFuture::new(endpoint),
            location,
        }
    }
}

impl<'e, 'r, const ROLE: u8> Future for OfferFuture<'e, 'r, ROLE> {
    type Output = EndpointResult<RouteBranch<'e, 'r, ROLE>>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        match this.raw.poll_raw(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Err(err)) => Poll::Ready(Err(EndpointError::new(
                EndpointOp::Offer,
                this.location,
                err,
            ))),
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ErrorLocation {
    location: &'static Location<'static>,
}

impl ErrorLocation {
    #[inline]
    #[track_caller]
    pub(crate) fn caller() -> Self {
        Self {
            location: Location::caller(),
        }
    }

    #[inline]
    const fn file(self) -> &'static str {
        self.location.file()
    }

    #[inline]
    const fn line(self) -> u32 {
        self.location.line()
    }

    #[inline]
    const fn column(self) -> u32 {
        self.location.column()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EndpointOp {
    Flow,
    Send,
    Recv,
    Offer,
    Decode,
}

/// Domain error for endpoint progress.
///
/// The API shape stays on `flow/send/recv/offer/decode`; this error records
/// which operation failed and where the public operation was started, so callers
/// can keep using plain `?` without wrappers. The diagnostic kind is deliberately
/// private: application code should not match endpoint failures to continue the
/// same generation on an alternate route.
pub struct EndpointError {
    op: EndpointOp,
    location: ErrorLocation,
    kind: EndpointErrorKind,
}

impl fmt::Debug for EndpointError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EndpointError")
            .field("operation", &self.operation())
            .field("file", &self.file())
            .field("line", &self.line())
            .field("column", &self.column())
            .field("kind", &self.kind)
            .finish()
    }
}

impl EndpointError {
    #[inline]
    fn new<E>(op: EndpointOp, location: ErrorLocation, error: E) -> Self
    where
        EndpointErrorKind: From<E>,
    {
        Self {
            op,
            location,
            kind: EndpointErrorKind::from(error),
        }
    }

    #[inline]
    pub const fn operation(&self) -> &'static str {
        match self.op {
            EndpointOp::Flow => "flow",
            EndpointOp::Send => "send",
            EndpointOp::Recv => "recv",
            EndpointOp::Offer => "offer",
            EndpointOp::Decode => "decode",
        }
    }

    #[inline]
    pub const fn file(&self) -> &'static str {
        self.location.file()
    }

    #[inline]
    pub const fn line(&self) -> u32 {
        self.location.line()
    }

    #[inline]
    pub const fn column(&self) -> u32 {
        self.location.column()
    }
}

/// Endpoint progress failure kind independent of the operation callsite.
enum EndpointErrorKind {
    Codec(CodecError),
    Transport(TransportError),
    BindingTransport(crate::binding::TransportOpsError),
    PhaseInvariant,
    LabelMismatch { expected: u8, actual: u8 },
    PeerMismatch { expected: u8, actual: u8 },
    PolicyAbort { reason: u16 },
    SessionFault(crate::rendezvous::SessionFaultKind),
}

impl fmt::Debug for EndpointErrorKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Codec(error) => formatter.debug_tuple("Codec").field(error).finish(),
            Self::Transport(error) => formatter.debug_tuple("Transport").field(error).finish(),
            Self::BindingTransport(error) => formatter
                .debug_tuple("BindingTransport")
                .field(error)
                .finish(),
            Self::PhaseInvariant => formatter.write_str("PhaseInvariant"),
            Self::LabelMismatch { expected, actual } => formatter
                .debug_struct("LabelMismatch")
                .field("expected", expected)
                .field("actual", actual)
                .finish(),
            Self::PeerMismatch { expected, actual } => formatter
                .debug_struct("PeerMismatch")
                .field("expected", expected)
                .field("actual", actual)
                .finish(),
            Self::PolicyAbort { reason } => formatter
                .debug_struct("PolicyAbort")
                .field("reason", reason)
                .finish(),
            Self::SessionFault(kind) => formatter.debug_tuple("SessionFault").field(kind).finish(),
        }
    }
}

impl From<SendError> for EndpointErrorKind {
    #[inline]
    fn from(error: SendError) -> Self {
        match error {
            SendError::Codec(error) => Self::Codec(error),
            SendError::Transport(error) => Self::Transport(error),
            SendError::PhaseInvariant => Self::PhaseInvariant,
            SendError::LabelMismatch { expected, actual } => {
                Self::LabelMismatch { expected, actual }
            }
            SendError::PolicyAbort { reason } => Self::PolicyAbort { reason },
            SendError::SessionFault(kind) => Self::SessionFault(kind),
        }
    }
}

impl From<RecvError> for EndpointErrorKind {
    #[inline]
    fn from(error: RecvError) -> Self {
        match error {
            RecvError::Transport(error) => Self::Transport(error),
            RecvError::Binding(error) => Self::BindingTransport(error),
            RecvError::Codec(error) => Self::Codec(error),
            RecvError::PhaseInvariant => Self::PhaseInvariant,
            RecvError::LabelMismatch { expected, actual } => {
                Self::LabelMismatch { expected, actual }
            }
            RecvError::PeerMismatch { expected, actual } => Self::PeerMismatch { expected, actual },
            RecvError::PolicyAbort { reason } => Self::PolicyAbort { reason },
            RecvError::SessionFault(kind) => Self::SessionFault(kind),
        }
    }
}

/// Canonical endpoint result returned by public endpoint operations.
pub type EndpointResult<T> = core::result::Result<T, EndpointError>;

/// Errors surfaced inside the endpoint send kernel.
#[derive(Debug)]
pub(crate) enum SendError {
    /// Payload encoding failed.
    Codec(CodecError),
    /// Transport returned an error while transmitting the frame.
    Transport(TransportError),
    /// Endpoint typestate or descriptor facts did not permit this send.
    PhaseInvariant,
    /// Attempted to send a message whose label does not match the typestate step.
    LabelMismatch { expected: u8, actual: u8 },
    /// Policy VM aborted the send operation.
    PolicyAbort { reason: u16 },
    /// Current session generation has terminal fault evidence.
    SessionFault(crate::rendezvous::SessionFaultKind),
}

/// Errors surfaced inside the endpoint receive/decode kernel.
#[derive(Debug)]
pub(crate) enum RecvError {
    /// Transport returned an error while awaiting the next frame.
    Transport(TransportError),
    /// Binding layer failed to read from channel.
    Binding(crate::binding::TransportOpsError),
    /// Payload decoding failed.
    Codec(CodecError),
    /// Endpoint typestate or descriptor facts did not permit this receive.
    PhaseInvariant,
    /// Choreography logical label did not match the projected typestate step.
    LabelMismatch { expected: u8, actual: u8 },
    /// Incoming frame originated from an unexpected peer role.
    PeerMismatch { expected: u8, actual: u8 },
    /// Policy VM aborted the receive operation.
    PolicyAbort { reason: u16 },
    /// Current session generation has terminal fault evidence.
    SessionFault(crate::rendezvous::SessionFaultKind),
}

pub(crate) type SendResult<T> = core::result::Result<T, SendError>;

pub(crate) type RecvResult<T> = core::result::Result<T, RecvError>;

#[cfg(test)]
mod tests {
    use super::{
        DecodeFuture, Endpoint, OfferFuture, RawRecvFlags, RecvFuture, RouteBranch, flow, kernel,
    };
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
    fn raw_recv_flags_cache_empty_payload_fact_and_completion() {
        let mut accepts_empty = RawRecvFlags::new(true);
        assert!(accepts_empty.accepts_empty_payload());
        assert!(!accepts_empty.completed());
        accepts_empty.mark_completed();
        assert!(accepts_empty.completed());

        let rejects_empty = RawRecvFlags::new(false);
        assert!(!rejects_empty.accepts_empty_payload());
        assert!(!rejects_empty.completed());
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
