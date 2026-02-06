//! Capability-oriented flow pipeline tying typestate metadata, mint policy,
//! and transport emission into a single affine value.

use core::marker::PhantomData;

use crate::{
    binding::{BindingSlot, NoBinding},
    control::{
        CapFlowToken,
        cap::{AllowsCanonical, ControlMint, MintConfigMarker, ResourceKind},
    },
    endpoint::{
        SendError, SendResult,
        control::ControlOutcome,
        cursor::{CanonicalTokenProvider, CursorEndpoint},
    },
    g::{ControlHandling, ControlPayloadKind, MessageSpec, SendableLabel},
    global::typestate::SendMeta,
    runtime::{config::Clock, consts::LabelUniverse},
    transport::Transport,
};

type ControlResource<M> = <<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind;

/// Identifier emitted alongside tap events for observability.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CapFlowId(pub u16);

impl CapFlowId {
    #[inline(always)]
    pub const fn new(id: u16) -> Self {
        Self(id)
    }

    #[inline(always)]
    pub const fn to_u16(self) -> u16 {
        self.0
    }
}

/// Affine flow handle for a pending send transition.
///
/// Created by `CursorEndpoint::flow()` and consumed by `.send(arg).await`.
/// The type name matches the constructor for clarity: `flow()` → `CapFlow`.
pub struct CapFlow<
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
    E: crate::control::cap::EpochTable,
    Mint: MintConfigMarker,
    B: BindingSlot,
{
    endpoint: CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
    meta: SendMeta,
    flow_id: CapFlowId,
    _msg: PhantomData<M>,
}

impl<'r, const ROLE: u8, M, T, U, C, E, const MAX_RV: usize, Mint, B>
    CapFlow<'r, ROLE, M, T, U, C, E, MAX_RV, Mint, B>
where
    M: MessageSpec + SendableLabel,
    T: Transport + 'r,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::EpochTable,
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
            flow_id: CapFlowId::new(meta.eff_index),
            _msg: PhantomData,
        }
    }

    /// Borrow the typestate metadata for the pending send.
    #[inline(always)]
    pub fn meta(&self) -> SendMeta {
        self.meta
    }

    /// Debug: check if the send meta's is_control flag is set.
    #[cfg(any(test, debug_assertions))]
    pub fn debug_send_meta_is_control(&self) -> bool {
        self.meta.is_control
    }

    /// Flow identifier matching the originating effect.
    #[inline(always)]
    pub fn id(&self) -> CapFlowId {
        self.flow_id
    }

    /// Consume the flow without sending anything.
    #[inline(always)]
    pub fn into_endpoint(self) -> CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B> {
        self.endpoint
    }

    /// Abort the flow and recover the endpoint without advancing typestate.
    #[inline(always)]
    pub fn abort(self) -> CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B> {
        self.into_endpoint()
    }

    /// Validate that the pending metadata matches an expected label.
    #[allow(clippy::result_large_err)]
    pub fn expect_label(self, label: u8) -> Result<Self, (Self, SendError)> {
        let expected = self.meta.label;
        if expected == label {
            Ok(self)
        } else {
            Err((
                self,
                SendError::LabelMismatch {
                    expected,
                    actual: label,
                },
            ))
        }
    }

    /// Access the underlying endpoint (borrow only, flow remains active).
    #[inline(always)]
    pub fn endpoint(&self) -> &CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B> {
        &self.endpoint
    }

    /// Mutably access the underlying endpoint.
    #[inline(always)]
    pub fn endpoint_mut(&mut self) -> &mut CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B> {
        &mut self.endpoint
    }

    fn into_parts(
        self,
    ) -> (
        CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
        SendMeta,
        CapFlowId,
    ) {
        (self.endpoint, self.meta, self.flow_id)
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// HandleView extraction for Canonical flows
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

impl<'r, const ROLE: u8, M, T, U, C, E, const MAX_RV: usize, Mint, B>
    CapFlow<'r, ROLE, M, T, U, C, E, MAX_RV, Mint, B>
where
    M: MessageSpec + SendableLabel,
    M::Payload: crate::transport::wire::WireEncode,
    M::ControlKind:
        CanonicalTokenProvider<'r, ROLE, T, U, C, E, Mint, MAX_RV, M, B> + ControlPayloadKind,
    T: Transport + 'r,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::EpochTable,
    Mint: MintConfigMarker<Policy: AllowsCanonical>,
    B: BindingSlot,
{
    /// Mint a canonical token for this flow.
    ///
    /// This method is only available for Canonical control flows where:
    /// - `M::ControlKind::HANDLING == ControlHandling::Canonical`
    /// - `Mint::Policy: AllowsCanonical`
    /// - `K: ResourceKind` matches `M::ControlKind::ResourceKind`
    ///
    /// The returned token can be inspected (e.g., extracting HandleView) before
    /// calling `send()`. The token is automatically sent when calling `send(())`.
    ///
    /// # Type Safety
    ///
    /// This is a zero-cost proof-carrying operation:
    /// 1. Mint embeds `K::caps_mask(&handle)` in the token header
    /// 2. Compiler proves `K` matches the message's `ResourceKind`
    /// 3. Token can be decoded into `HandleView<K>` for inspection
    ///
    /// # Example
    ///
    /// ```ignore
    /// let mut flow = cursor.flow::<LoopContinueMsg>()?;
    /// let token = flow.into_token::<LoopContinueKind>()?;
    /// // Inspect token if needed
    /// let (ep, outcome) = flow.send(()).await?;
    /// ```
    pub fn into_token<K>(&mut self) -> SendResult<CapFlowToken<K>>
    where
        K: ResourceKind + ControlMint,
        M::ControlKind: ControlPayloadKind<ResourceKind = K>,
    {
        // Compile-time assertion: only Canonical flows can mint
        const {
            assert!(
                <M::ControlKind as ControlPayloadKind>::HANDLING as u8
                    == ControlHandling::Canonical as u8,
                "into_token() requires Canonical control handling"
            );
        }

        // Copy meta to avoid borrow conflict
        let meta = self.meta;
        self.endpoint_mut().canonical_control_token::<K>(&meta)
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
    E: crate::control::cap::EpochTable,
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
    pub async fn send<'a, A>(
        self,
        arg: A,
    ) -> SendResult<(
        CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
        ControlOutcome<'r, ControlResource<M>>,
    )>
    where
        A: FlowSendArg<'a, M, Mint>,
        M::Payload: 'a,
    {
        let (endpoint, meta, _id) = self.into_parts();
        let payload = arg.into_payload();
        endpoint.send_with_meta::<M>(&meta, payload).await
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
            let auto_mint = <<M::ControlKind as ControlPayloadKind>::ResourceKind as crate::control::cap::ResourceKind>::AUTO_MINT_EXTERNAL;
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
