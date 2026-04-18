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
    control::cap::mint::{EpochTbl, MintConfig, MintConfigMarker},
    transport::{TransportError, wire::CodecError},
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
type EndpointCfg<'r, K, Mint> = carrier::EndpointCfg<K, Mint, EndpointBinding<'r>>;
type KernelEndpoint<'r, const ROLE: u8, K, Mint> =
    carrier::KernelCursorEndpoint<'r, ROLE, K, EpochTbl, Mint, EndpointBinding<'r>>;
type KernelRouteBranch<'r, const ROLE: u8, K, Mint> =
    carrier::KernelRouteBranch<'r, ROLE, K, EpochTbl, Mint, EndpointBinding<'r>>;

struct EndpointInner<'r, const ROLE: u8, K, Mint>
where
    K: carrier::SessionKitFamily + 'r,
    Mint: MintConfigMarker,
{
    endpoint: *mut KernelEndpoint<'r, ROLE, K, Mint>,
    _cfg: core::marker::PhantomData<EndpointCfg<'r, K, Mint>>,
    _local_only: crate::local::LocalOnly,
}

/// Public endpoint facade for app-facing localside interaction.
#[allow(private_bounds)]
pub struct Endpoint<'r, const ROLE: u8, K, Mint = MintConfig>
where
    K: carrier::SessionKitFamily + 'r,
    Mint: MintConfigMarker,
{
    inner: EndpointInner<'r, ROLE, K, Mint>,
}

/// Public route-branch facade returned by [`Endpoint::offer`].
#[allow(private_bounds)]
pub struct RouteBranch<'e, 'r, const ROLE: u8, K, Mint = MintConfig>
where
    K: carrier::SessionKitFamily + 'r,
    Mint: MintConfigMarker,
{
    endpoint: *mut KernelEndpoint<'r, ROLE, K, Mint>,
    branch: Option<KernelRouteBranch<'r, ROLE, K, Mint>>,
    _borrow: core::marker::PhantomData<&'e mut EndpointCfg<'r, K, Mint>>,
    _local_only: crate::local::LocalOnly,
}

struct OfferFuture<'e, 'r, 'cfg, const ROLE: u8, T, U, C, const MAX_RV: usize, Mint>
where
    T: crate::substrate::Transport + 'cfg,
    U: crate::substrate::runtime::LabelUniverse + 'cfg,
    C: crate::substrate::runtime::Clock + 'cfg,
    'cfg: 'r,
    Mint: MintConfigMarker,
{
    endpoint:
        *mut KernelEndpoint<'r, ROLE, crate::substrate::SessionKit<'cfg, T, U, C, MAX_RV>, Mint>,
    inner: kernel::RouteOfferFuture<
        'e,
        'r,
        ROLE,
        T,
        U,
        C,
        EpochTbl,
        MAX_RV,
        Mint,
        EndpointBinding<'r>,
    >,
}

struct DecodeFuture<'e, 'r, 'cfg, const ROLE: u8, T, U, C, const MAX_RV: usize, Mint, M>
where
    T: crate::substrate::Transport + 'cfg,
    U: crate::substrate::runtime::LabelUniverse + 'cfg,
    C: crate::substrate::runtime::Clock + 'cfg,
    'cfg: 'r,
    Mint: MintConfigMarker,
    M: crate::global::MessageSpec,
    M::Payload: crate::transport::wire::WirePayload,
{
    inner: kernel::RouteDecodeFuture<
        'e,
        'r,
        ROLE,
        T,
        U,
        C,
        EpochTbl,
        MAX_RV,
        Mint,
        EndpointBinding<'r>,
        M,
    >,
    _cfg: core::marker::PhantomData<&'cfg ()>,
}

impl<'e, 'r, 'cfg, const ROLE: u8, T, U, C, const MAX_RV: usize, Mint, M>
    DecodeFuture<'e, 'r, 'cfg, ROLE, T, U, C, MAX_RV, Mint, M>
where
    T: crate::substrate::Transport + 'cfg,
    U: crate::substrate::runtime::LabelUniverse + 'cfg,
    C: crate::substrate::runtime::Clock + 'cfg,
    'cfg: 'r,
    Mint: MintConfigMarker,
    M: crate::global::MessageSpec,
    M::Payload: crate::transport::wire::WirePayload,
{
    #[inline]
    fn new(
        mut branch: RouteBranch<
            'e,
            'r,
            ROLE,
            crate::substrate::SessionKit<'cfg, T, U, C, MAX_RV>,
            Mint,
        >,
    ) -> Self {
        let endpoint = branch.endpoint;
        Self {
            inner: unsafe {
                (&mut *endpoint).decode_route_branch::<M>(
                    branch
                        .branch
                        .take()
                        .expect("route branch payload must stay present until consumed"),
                )
            },
            _cfg: core::marker::PhantomData,
        }
    }
}

impl<'e, 'r, 'cfg, const ROLE: u8, T, U, C, const MAX_RV: usize, Mint, M> Future
    for DecodeFuture<'e, 'r, 'cfg, ROLE, T, U, C, MAX_RV, Mint, M>
where
    T: crate::substrate::Transport + 'cfg,
    U: crate::substrate::runtime::LabelUniverse + 'cfg,
    C: crate::substrate::runtime::Clock + 'cfg,
    'cfg: 'r,
    Mint: MintConfigMarker,
    M: crate::global::MessageSpec,
    M::Payload: crate::transport::wire::WirePayload,
{
    type Output = RecvResult<
        <<M as crate::global::MessageSpec>::Payload as crate::transport::wire::WirePayload>::Decoded<'e>,
    >;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        unsafe { Pin::new_unchecked(&mut self.get_unchecked_mut().inner) }.poll(cx)
    }
}

impl<'r, const ROLE: u8, K, Mint> EndpointInner<'r, ROLE, K, Mint>
where
    K: carrier::SessionKitFamily + 'r,
    Mint: MintConfigMarker,
{
    #[inline]
    fn from_ptr(endpoint: *mut KernelEndpoint<'r, ROLE, K, Mint>) -> Self {
        Self {
            endpoint,
            _cfg: core::marker::PhantomData,
            _local_only: crate::local::LocalOnly::new(),
        }
    }
}

#[allow(private_bounds)]
impl<'r, const ROLE: u8, K, Mint> Endpoint<'r, ROLE, K, Mint>
where
    K: carrier::SessionKitFamily + 'r,
    Mint: MintConfigMarker,
{
    #[inline]
    pub(crate) fn from_ptr(endpoint: *mut KernelEndpoint<'r, ROLE, K, Mint>) -> Self {
        Self {
            inner: EndpointInner::from_ptr(endpoint),
        }
    }
}

#[allow(private_bounds)]
impl<'e, 'r, const ROLE: u8, K, Mint> RouteBranch<'e, 'r, ROLE, K, Mint>
where
    K: carrier::SessionKitFamily + 'r,
    Mint: MintConfigMarker,
{
    #[inline]
    pub(crate) fn from_parts(
        endpoint: *mut KernelEndpoint<'r, ROLE, K, Mint>,
        branch: KernelRouteBranch<'r, ROLE, K, Mint>,
    ) -> Self {
        Self {
            endpoint,
            branch: Some(branch),
            _borrow: core::marker::PhantomData,
            _local_only: crate::local::LocalOnly::new(),
        }
    }
}

impl<'r, const ROLE: u8, K, Mint> Drop for Endpoint<'r, ROLE, K, Mint>
where
    K: carrier::SessionKitFamily + 'r,
    Mint: MintConfigMarker,
{
    fn drop(&mut self) {
        unsafe {
            core::ptr::drop_in_place(self.inner.endpoint);
        }
    }
}

impl<'e, 'r, const ROLE: u8, K, Mint> Drop for RouteBranch<'e, 'r, ROLE, K, Mint>
where
    K: carrier::SessionKitFamily + 'r,
    Mint: MintConfigMarker,
{
    fn drop(&mut self) {
        if let Some(branch) = self.branch.take() {
            unsafe {
                K::restore_materialized_route_branch(self.endpoint, branch);
            }
        }
    }
}

impl<'e, 'r, 'cfg, const ROLE: u8, T, U, C, const MAX_RV: usize, Mint>
    OfferFuture<'e, 'r, 'cfg, ROLE, T, U, C, MAX_RV, Mint>
where
    T: crate::substrate::Transport + 'cfg,
    U: crate::substrate::runtime::LabelUniverse + 'cfg,
    C: crate::substrate::runtime::Clock + 'cfg,
    'cfg: 'r,
    Mint: MintConfigMarker,
{
    #[inline]
    fn new(
        endpoint: &'e mut Endpoint<
            'r,
            ROLE,
            crate::substrate::SessionKit<'cfg, T, U, C, MAX_RV>,
            Mint,
        >,
    ) -> Self {
        Self {
            endpoint: endpoint.inner.endpoint,
            inner: unsafe { (&mut *endpoint.inner.endpoint).offer() },
        }
    }
}

impl<'e, 'r, 'cfg, const ROLE: u8, T, U, C, const MAX_RV: usize, Mint> Future
    for OfferFuture<'e, 'r, 'cfg, ROLE, T, U, C, MAX_RV, Mint>
where
    T: crate::substrate::Transport + 'cfg,
    U: crate::substrate::runtime::LabelUniverse + 'cfg,
    C: crate::substrate::runtime::Clock + 'cfg,
    'cfg: 'r,
    Mint: MintConfigMarker,
{
    type Output = RecvResult<
        RouteBranch<'e, 'r, ROLE, crate::substrate::SessionKit<'cfg, T, U, C, MAX_RV>, Mint>,
    >;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        match Pin::new(&mut this.inner).poll(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
            Poll::Ready(Ok(branch)) => Poll::Ready(Ok(RouteBranch::from_parts(this.endpoint, branch))),
        }
    }
}

impl<'r, 'cfg, const ROLE: u8, T, U, C, const MAX_RV: usize, Mint>
    Endpoint<'r, ROLE, crate::substrate::SessionKit<'cfg, T, U, C, MAX_RV>, Mint>
where
    T: crate::substrate::Transport + 'cfg,
    U: crate::substrate::runtime::LabelUniverse + 'cfg,
    C: crate::substrate::runtime::Clock + 'cfg,
    'cfg: 'r,
    Mint: MintConfigMarker,
{
    #[inline]
    pub fn flow<'e, M>(
        &'e mut self,
    ) -> SendResult<
        flow::Flow<'e, 'r, ROLE, M, crate::substrate::SessionKit<'cfg, T, U, C, MAX_RV>, Mint>,
    >
    where
        M: crate::global::MessageSpec + crate::global::SendableLabel,
        'cfg: 'r,
    {
        unsafe {
            Ok(flow::Flow::from_cap_flow(
                (&mut *self.inner.endpoint).flow_for_kit::<M>()?,
            ))
        }
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
        unsafe { (&mut *self.inner.endpoint).recv_direct::<M>() }
    }

    #[inline]
    pub fn offer<'e>(
        &'e mut self,
    ) -> impl core::future::Future<
        Output = RecvResult<
            RouteBranch<'e, 'r, ROLE, crate::substrate::SessionKit<'cfg, T, U, C, MAX_RV>, Mint>,
        >,
    > + 'e {
        OfferFuture::new(self)
    }
}

impl<'e, 'r, 'cfg, const ROLE: u8, T, U, C, const MAX_RV: usize, Mint>
    RouteBranch<'e, 'r, ROLE, crate::substrate::SessionKit<'cfg, T, U, C, MAX_RV>, Mint>
where
    T: crate::substrate::Transport + 'cfg,
    U: crate::substrate::runtime::LabelUniverse + 'cfg,
    C: crate::substrate::runtime::Clock + 'cfg,
    'cfg: 'r,
    Mint: MintConfigMarker,
{
    #[inline]
    pub fn label(&self) -> u8 {
        self.branch
            .as_ref()
            .expect("route branch payload must stay present until consumed")
            .label()
    }

    #[inline]
    pub fn decode<M>(
        self,
    ) -> impl core::future::Future<
        Output = RecvResult<<<M as crate::global::MessageSpec>::Payload as crate::transport::wire::WirePayload>::Decoded<'e>>,
    > + use<'e, 'r, 'cfg, M, ROLE, T, U, C, MAX_RV, Mint>
    where
        M: crate::global::MessageSpec,
        M::Payload: crate::transport::wire::WirePayload,
    {
        DecodeFuture::<'e, 'r, 'cfg, ROLE, T, U, C, MAX_RV, Mint, M>::new(self)
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
