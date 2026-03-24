//! Capability-oriented flow pipeline tying typestate metadata, mint policy,
//! and transport emission into a single affine value.

use core::marker::PhantomData;

use crate::{
    binding::{BindingSlot, NoBinding},
    control::cap::mint::{AllowsCanonical, MintConfigMarker},
    endpoint::{
        Endpoint, SendResult,
        control::ControlOutcome,
        cursor::{CanonicalTokenProvider, CursorEndpoint},
    },
    global::typestate::SendMeta,
    global::{ControlHandling, ControlPayloadKind, MessageSpec, SendableLabel},
    runtime::{config::Clock, consts::LabelUniverse},
    transport::Transport,
};

/// Affine flow handle for a pending send transition.
///
/// Created by `Endpoint::flow()` and consumed by `.send(arg).await`.
/// The type name matches the constructor for clarity: `flow()` → `CapFlow`.
pub(crate) struct CapFlow<
    'r,
    const ROLE: u8,
    M,
    T: Transport + 'r,
    U,
    C,
    E,
    const MAX_RV: usize,
    Mint,
    B: BindingSlot = NoBinding,
> where
    M: MessageSpec + SendableLabel,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
    Mint: MintConfigMarker,
    B: BindingSlot,
{
    endpoint: CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
    meta: SendMeta,
    _msg: PhantomData<M>,
}

/// Public flow facade returned by [`Endpoint::flow`](crate::Endpoint::flow).
pub struct Flow<
    'r,
    const ROLE: u8,
    M,
    T: Transport + 'r,
    U,
    C,
    E,
    const MAX_RV: usize,
    Mint,
    B: BindingSlot = NoBinding,
> where
    M: MessageSpec + SendableLabel,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
    Mint: MintConfigMarker,
    B: BindingSlot,
{
    inner: CapFlow<'r, ROLE, M, T, U, C, E, MAX_RV, Mint, B>,
}

impl<'r, const ROLE: u8, M, T, U, C, E, const MAX_RV: usize, Mint, B>
    Flow<'r, ROLE, M, T, U, C, E, MAX_RV, Mint, B>
where
    M: MessageSpec + SendableLabel,
    T: Transport + 'r,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
    Mint: MintConfigMarker,
    B: BindingSlot,
{
    #[inline]
    pub(crate) fn from_cap_flow(inner: CapFlow<'r, ROLE, M, T, U, C, E, MAX_RV, Mint, B>) -> Self {
        Self { inner }
    }
}

impl<'r, const ROLE: u8, M, T, U, C, E, const MAX_RV: usize, Mint, B>
    CapFlow<'r, ROLE, M, T, U, C, E, MAX_RV, Mint, B>
where
    M: MessageSpec + SendableLabel,
    T: Transport + 'r,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
    Mint: MintConfigMarker,
    B: BindingSlot,
{
    pub(crate) fn new(
        endpoint: CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
        meta: SendMeta,
    ) -> Self {
        Self {
            endpoint,
            meta,
            _msg: PhantomData,
        }
    }

    fn into_parts(
        self,
    ) -> (
        CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
        SendMeta,
    ) {
        (self.endpoint, self.meta)
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Unified send implementation
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

impl<'r, const ROLE: u8, M, T, U, C, E, const MAX_RV: usize, Mint, B>
    CapFlow<'r, ROLE, M, T, U, C, E, MAX_RV, Mint, B>
where
    M: MessageSpec + SendableLabel,
    M::Payload: crate::transport::wire::WireEncode,
    M::ControlKind: CanonicalTokenProvider<'r, ROLE, T, U, C, E, Mint, MAX_RV, M, B>,
    T: Transport + 'r,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
    Mint: MintConfigMarker,
    B: BindingSlot,
{
    /// Send the message with a payload provider.
    ///
    /// The argument type is constrained by the message's control handling:
    /// - `ControlHandling::Canonical`: must pass `()`, auto-mints token
    /// - `ControlHandling::External` or `None`: must pass `&M::Payload`
    ///
    /// Type resolution happens at compile-time via the `FlowSendArg` trait.
    #[inline]
    pub(crate) async fn send<'a, A>(
        self,
        arg: A,
    ) -> SendResult<(
        CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
        ControlOutcome<'r, <<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind>,
    )>
    where
        A: FlowSendArg<'a, M, Mint>,
        M::Payload: 'a,
    {
        let (endpoint, meta) = self.into_parts();
        let payload = arg.into_payload();
        endpoint.send_with_meta::<M>(&meta, payload).await
    }
}

impl<'r, const ROLE: u8, M, T, U, C, E, const MAX_RV: usize, Mint, B>
    Flow<'r, ROLE, M, T, U, C, E, MAX_RV, Mint, B>
where
    M: MessageSpec + SendableLabel,
    M::Payload: crate::transport::wire::WireEncode,
    M::ControlKind: CanonicalTokenProvider<'r, ROLE, T, U, C, E, Mint, MAX_RV, M, B>,
    T: Transport + 'r,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
    Mint: MintConfigMarker,
    B: BindingSlot,
{
    #[inline]
    pub async fn send<'a, A>(
        self,
        arg: A,
    ) -> SendResult<(
        Endpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
        ControlOutcome<'r, <<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind>,
    )>
    where
        A: FlowSendArg<'a, M, Mint>,
        M::Payload: 'a,
    {
        let (endpoint, outcome) = self.inner.send(arg).await?;
        Ok((Endpoint::from_cursor(endpoint), outcome))
    }
}

/// Sealed trait for type-level send argument resolution.
pub trait FlowSendArg<'a, M, Mint>
where
    M: MessageSpec + SendableLabel,
    Mint: MintConfigMarker,
{
    fn into_payload(self) -> Option<&'a M::Payload>
    where
        Self: Sized;
}

/// Unit type for auto-mint control path (Canonical or External with AUTO_MINT_EXTERNAL).
impl<'a, M, Mint> FlowSendArg<'a, M, Mint> for ()
where
    M: MessageSpec + SendableLabel,
    Mint: MintConfigMarker<Policy: AllowsCanonical>,
    M::ControlKind: ControlPayloadKind,
{
    #[inline(always)]
    fn into_payload(self) -> Option<&'a M::Payload> {
        // Compile-time assertion: this impl exists when:
        // - Canonical control (always auto-minted), or
        // - External control with AUTO_MINT_EXTERNAL = true (e.g., SpliceIntent/Ack)
        const {
            let handling = <M::ControlKind as ControlPayloadKind>::HANDLING as u8;
            let is_canonical = handling == ControlHandling::Canonical as u8;
            let is_external = handling == ControlHandling::External as u8;
            let auto_mint = <<M::ControlKind as ControlPayloadKind>::ResourceKind as crate::control::cap::mint::ResourceKind>::AUTO_MINT_EXTERNAL;
            assert!(
                is_canonical || (is_external && auto_mint),
                "Unit () can only be used with Canonical control or External control with AUTO_MINT_EXTERNAL"
            );
        }
        None
    }
}

/// Reference payload for external/data messages.
impl<'a, M, Mint> FlowSendArg<'a, M, Mint> for &'a M::Payload
where
    M: MessageSpec + SendableLabel,
    Mint: MintConfigMarker,
    M::ControlKind: ControlPayloadKind,
{
    #[inline(always)]
    fn into_payload(self) -> Option<&'a M::Payload> {
        // Compile-time assertion: this impl only exists for External or None
        const {
            let h = <M::ControlKind as ControlPayloadKind>::HANDLING as u8;
            assert!(
                h == ControlHandling::External as u8 || h == ControlHandling::None as u8,
                "Payload reference can only be used with External or None control messages"
            );
        }
        Some(self)
    }
}
