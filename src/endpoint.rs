//! Localside endpoint facade built on the typestate DSL.
//!
//! Applications interact with `Endpoint` values that are materialised from
//! `RoleProgram` projections.

use crate::{
    binding::{BindingSlot, NoBinding},
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

type EndpointCfg<K, Mint, B> = carrier::EndpointCfg<K, Mint, B>;
type KernelEndpoint<'r, const ROLE: u8, K, Mint, B> =
    carrier::KernelCursorEndpoint<'r, ROLE, K, EpochTbl, Mint, B>;
type KernelRouteBranch<'r, const ROLE: u8, K, Mint, B> =
    carrier::KernelRouteBranch<'r, ROLE, K, EpochTbl, Mint, B>;
#[cfg(feature = "std")]
type EndpointStorage<'r, const ROLE: u8, K, Mint, B> =
    std::boxed::Box<KernelEndpoint<'r, ROLE, K, Mint, B>>;
#[cfg(not(feature = "std"))]
type EndpointStorage<'r, const ROLE: u8, K, Mint, B> = KernelEndpoint<'r, ROLE, K, Mint, B>;
#[cfg(feature = "std")]
type RouteBranchStorage<'r, const ROLE: u8, K, Mint, B> =
    std::boxed::Box<KernelRouteBranch<'r, ROLE, K, Mint, B>>;
#[cfg(not(feature = "std"))]
type RouteBranchStorage<'r, const ROLE: u8, K, Mint, B> = KernelRouteBranch<'r, ROLE, K, Mint, B>;

struct EndpointInner<'r, const ROLE: u8, K, Mint, B>
where
    K: carrier::SessionKitFamily + 'r,
    Mint: MintConfigMarker,
    B: BindingSlot,
{
    inner: EndpointStorage<'r, ROLE, K, Mint, B>,
    _cfg: core::marker::PhantomData<EndpointCfg<K, Mint, B>>,
}

/// Public endpoint facade for app-facing localside interaction.
#[allow(private_bounds)]
pub struct Endpoint<'r, const ROLE: u8, K, Mint = MintConfig, B = NoBinding>
where
    K: carrier::SessionKitFamily + 'r,
    Mint: MintConfigMarker,
    B: BindingSlot,
{
    inner: EndpointInner<'r, ROLE, K, Mint, B>,
}

struct RouteBranchInner<'r, const ROLE: u8, K, Mint, B>
where
    K: carrier::SessionKitFamily + 'r,
    Mint: MintConfigMarker,
    B: BindingSlot,
{
    inner: RouteBranchStorage<'r, ROLE, K, Mint, B>,
    _cfg: core::marker::PhantomData<EndpointCfg<K, Mint, B>>,
}

/// Public route-branch facade returned by [`Endpoint::offer`].
#[allow(private_bounds)]
pub struct RouteBranch<'r, const ROLE: u8, K, Mint = MintConfig, B = NoBinding>
where
    K: carrier::SessionKitFamily + 'r,
    Mint: MintConfigMarker,
    B: BindingSlot,
{
    inner: RouteBranchInner<'r, ROLE, K, Mint, B>,
}

impl<'r, const ROLE: u8, K, Mint, B> EndpointInner<'r, ROLE, K, Mint, B>
where
    K: carrier::SessionKitFamily + 'r,
    Mint: MintConfigMarker,
    B: BindingSlot,
{
    #[inline]
    fn from_cursor(inner: KernelEndpoint<'r, ROLE, K, Mint, B>) -> Self {
        #[cfg(feature = "std")]
        {
            Self {
                inner: std::boxed::Box::new(inner),
                _cfg: core::marker::PhantomData,
            }
        }
        #[cfg(not(feature = "std"))]
        {
            Self {
                inner,
                _cfg: core::marker::PhantomData,
            }
        }
    }

    #[inline]
    fn into_cursor(self) -> KernelEndpoint<'r, ROLE, K, Mint, B> {
        #[cfg(feature = "std")]
        {
            *self.inner
        }
        #[cfg(not(feature = "std"))]
        {
            self.inner
        }
    }
}

impl<'r, const ROLE: u8, K, Mint, B> RouteBranchInner<'r, ROLE, K, Mint, B>
where
    K: carrier::SessionKitFamily + 'r,
    Mint: MintConfigMarker,
    B: BindingSlot,
{
    #[inline]
    fn from_cursor(inner: KernelRouteBranch<'r, ROLE, K, Mint, B>) -> Self {
        #[cfg(feature = "std")]
        {
            Self {
                inner: std::boxed::Box::new(inner),
                _cfg: core::marker::PhantomData,
            }
        }
        #[cfg(not(feature = "std"))]
        {
            Self {
                inner,
                _cfg: core::marker::PhantomData,
            }
        }
    }

    #[inline]
    fn into_cursor(self) -> KernelRouteBranch<'r, ROLE, K, Mint, B> {
        #[cfg(feature = "std")]
        {
            *self.inner
        }
        #[cfg(not(feature = "std"))]
        {
            self.inner
        }
    }
}

#[allow(private_bounds)]
impl<'r, const ROLE: u8, K, Mint, B> Endpoint<'r, ROLE, K, Mint, B>
where
    K: carrier::SessionKitFamily + 'r,
    Mint: MintConfigMarker,
    B: BindingSlot,
{
    #[inline]
    pub(crate) fn from_cursor(inner: KernelEndpoint<'r, ROLE, K, Mint, B>) -> Self {
        Self {
            inner: EndpointInner::from_cursor(inner),
        }
    }

    #[inline]
    pub(crate) fn into_cursor(self) -> KernelEndpoint<'r, ROLE, K, Mint, B> {
        self.inner.into_cursor()
    }
}

#[allow(private_bounds)]
impl<'r, const ROLE: u8, K, Mint, B> RouteBranch<'r, ROLE, K, Mint, B>
where
    K: carrier::SessionKitFamily + 'r,
    Mint: MintConfigMarker,
    B: BindingSlot,
{
    #[inline]
    pub(crate) fn from_cursor(inner: KernelRouteBranch<'r, ROLE, K, Mint, B>) -> Self {
        Self {
            inner: RouteBranchInner::from_cursor(inner),
        }
    }

    #[inline]
    pub(crate) fn into_cursor(self) -> KernelRouteBranch<'r, ROLE, K, Mint, B> {
        self.inner.into_cursor()
    }
}

impl<'r, 'cfg, const ROLE: u8, T, U, C, const MAX_RV: usize, Mint, B>
    Endpoint<'r, ROLE, crate::substrate::SessionKit<'cfg, T, U, C, MAX_RV>, Mint, B>
where
    T: crate::substrate::Transport + 'cfg,
    U: crate::substrate::runtime::LabelUniverse + 'cfg,
    C: crate::substrate::runtime::Clock + 'cfg,
    'cfg: 'r,
    Mint: MintConfigMarker,
    B: BindingSlot,
{
    #[inline]
    pub fn flow<M>(
        self,
    ) -> SendResult<
        flow::Flow<'r, ROLE, M, crate::substrate::SessionKit<'cfg, T, U, C, MAX_RV>, Mint, B>,
    >
    where
        M: crate::global::MessageSpec + crate::global::SendableLabel,
        'cfg: 'r,
    {
        Ok(flow::Flow::from_cap_flow(
            self.into_cursor().flow_for_kit::<'cfg, M>()?,
        ))
    }

    #[inline]
    pub async fn recv<M>(self) -> RecvResult<(Self, M::Payload)>
    where
        M: crate::global::MessageSpec,
        M::Payload: crate::transport::wire::WireDecodeOwned,
    {
        let (endpoint, payload) = self.into_cursor().recv::<M>().await?;
        Ok((Self::from_cursor(endpoint), payload))
    }

    #[inline]
    pub async fn offer(
        self,
    ) -> RecvResult<
        RouteBranch<'r, ROLE, crate::substrate::SessionKit<'cfg, T, U, C, MAX_RV>, Mint, B>,
    > {
        let branch = self.into_cursor().offer().await?;
        Ok(RouteBranch::from_cursor(branch))
    }
}

impl<'r, 'cfg, const ROLE: u8, T, U, C, const MAX_RV: usize, Mint, B>
    RouteBranch<'r, ROLE, crate::substrate::SessionKit<'cfg, T, U, C, MAX_RV>, Mint, B>
where
    T: crate::substrate::Transport + 'cfg,
    U: crate::substrate::runtime::LabelUniverse + 'cfg,
    C: crate::substrate::runtime::Clock + 'cfg,
    'cfg: 'r,
    Mint: MintConfigMarker,
    B: BindingSlot,
{
    #[inline]
    pub fn label(&self) -> u8 {
        self.inner.inner.label()
    }

    #[inline]
    pub fn into_endpoint(
        self,
    ) -> Endpoint<'r, ROLE, crate::substrate::SessionKit<'cfg, T, U, C, MAX_RV>, Mint, B> {
        Endpoint::from_cursor(self.into_cursor().into_endpoint())
    }

    #[inline]
    pub async fn decode<M>(
        self,
    ) -> RecvResult<(
        Endpoint<'r, ROLE, crate::substrate::SessionKit<'cfg, T, U, C, MAX_RV>, Mint, B>,
        M::Payload,
    )>
    where
        M: crate::global::MessageSpec,
        M::Payload: crate::transport::wire::WireDecodeOwned,
    {
        let (endpoint, payload) = self.into_cursor().decode::<M>().await?;
        Ok((Endpoint::from_cursor(endpoint), payload))
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
