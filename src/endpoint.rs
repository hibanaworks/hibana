//! Localside endpoint facade built on the typestate DSL.
//!
//! Applications interact with `Endpoint` values that are materialised from
//! `RoleProgram` projections.

/// Affine endpoint helpers.
pub(crate) mod affine;
/// Control-plane helpers for endpoints.
pub(crate) mod control;
/// Internal endpoint kernel implementation.
pub(crate) mod cursor;
/// Flow-based send API.
pub(crate) mod flow;

/// Public endpoint facade for app-facing localside interaction.
pub struct Endpoint<
    'r,
    const ROLE: u8,
    T,
    U,
    C,
    E = crate::control::cap::mint::EpochTbl,
    const MAX_RV: usize = 4,
    Mint = crate::control::cap::mint::MintConfig,
    B = crate::binding::NoBinding,
> where
    T: crate::transport::Transport + 'r,
    U: crate::runtime::consts::LabelUniverse,
    C: crate::runtime::config::Clock,
    E: crate::control::cap::mint::EpochTable,
    Mint: crate::control::cap::mint::MintConfigMarker,
    B: crate::binding::BindingSlot,
{
    inner: cursor::CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
}

/// Public route-branch facade returned by [`Endpoint::offer`].
pub struct RouteBranch<
    'r,
    const ROLE: u8,
    T,
    U,
    C,
    E = crate::control::cap::mint::EpochTbl,
    const MAX_RV: usize = 4,
    Mint = crate::control::cap::mint::MintConfig,
    B = crate::binding::NoBinding,
> where
    T: crate::transport::Transport + 'r,
    U: crate::runtime::consts::LabelUniverse,
    C: crate::runtime::config::Clock,
    E: crate::control::cap::mint::EpochTable,
    Mint: crate::control::cap::mint::MintConfigMarker,
    B: crate::binding::BindingSlot,
{
    inner: cursor::RouteBranch<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
}

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>
    Endpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: crate::transport::Transport + 'r,
    U: crate::runtime::consts::LabelUniverse,
    C: crate::runtime::config::Clock,
    E: crate::control::cap::mint::EpochTable,
    Mint: crate::control::cap::mint::MintConfigMarker,
    B: crate::binding::BindingSlot,
{
    #[inline]
    pub(crate) fn from_cursor(
        inner: cursor::CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
    ) -> Self {
        Self { inner }
    }

    #[inline]
    pub(crate) fn into_cursor(
        self,
    ) -> cursor::CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B> {
        self.inner
    }
}

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>
    RouteBranch<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: crate::transport::Transport + 'r,
    U: crate::runtime::consts::LabelUniverse,
    C: crate::runtime::config::Clock,
    E: crate::control::cap::mint::EpochTable,
    Mint: crate::control::cap::mint::MintConfigMarker,
    B: crate::binding::BindingSlot,
{
    #[inline]
    pub(crate) fn from_cursor(
        inner: cursor::RouteBranch<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
    ) -> Self {
        Self { inner }
    }
}

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>
    Endpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: crate::transport::Transport + 'r,
    U: crate::runtime::consts::LabelUniverse,
    C: crate::runtime::config::Clock,
    E: crate::control::cap::mint::EpochTable,
    Mint: crate::control::cap::mint::MintConfigMarker,
    B: crate::binding::BindingSlot,
{
    #[inline]
    pub fn flow<M>(self) -> SendResult<flow::Flow<'r, ROLE, M, T, U, C, E, MAX_RV, Mint, B>>
    where
        M: crate::global::MessageSpec + crate::global::SendableLabel,
    {
        self.inner.flow::<M>().map(flow::Flow::from_cap_flow)
    }

    #[inline]
    pub async fn recv<M>(self) -> RecvResult<(Self, M::Payload)>
    where
        M: crate::global::MessageSpec,
        M::Payload: crate::transport::wire::WireDecodeOwned,
    {
        let (endpoint, payload) = self.inner.recv::<M>().await?;
        Ok((Self::from_cursor(endpoint), payload))
    }

    #[inline]
    pub async fn offer(self) -> RecvResult<RouteBranch<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>> {
        let branch = self.inner.offer().await?;
        Ok(RouteBranch::from_cursor(branch))
    }
}

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>
    RouteBranch<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: crate::transport::Transport + 'r,
    U: crate::runtime::consts::LabelUniverse,
    C: crate::runtime::config::Clock,
    E: crate::control::cap::mint::EpochTable,
    Mint: crate::control::cap::mint::MintConfigMarker,
    B: crate::binding::BindingSlot,
{
    #[inline]
    pub fn label(&self) -> u8 {
        self.inner.label()
    }

    #[inline]
    pub fn into_endpoint(self) -> Endpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B> {
        Endpoint::from_cursor(self.inner.into_endpoint())
    }

    #[inline]
    pub async fn decode<M>(
        self,
    ) -> RecvResult<(Endpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>, M::Payload)>
    where
        M: crate::global::MessageSpec,
        M::Payload: crate::transport::wire::WireDecodeOwned,
    {
        let (endpoint, payload) = self.inner.decode::<M>().await?;
        Ok((Endpoint::from_cursor(endpoint), payload))
    }
}

use crate::transport::{TransportError, wire::CodecError};

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
