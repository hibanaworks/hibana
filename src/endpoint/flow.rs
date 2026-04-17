//! Capability-oriented flow pipeline tying typestate metadata, mint policy,
//! and transport emission into a single affine value.

use core::marker::PhantomData;

use crate::{
    binding::BindingHandle,
    control::cap::mint::{AllowsCanonical, EpochTbl, MintConfigMarker},
    endpoint::{SendResult, carrier, control::ControlOutcome},
    global::{ControlHandling, ControlPayloadKind, MessageSpec, SendableLabel},
};

type EndpointBinding<'r> = BindingHandle<'r>;
type EndpointCfg<'r, K, Mint> = carrier::EndpointCfg<K, Mint, EndpointBinding<'r>>;
type KernelEndpoint<'r, const ROLE: u8, K, Mint> =
    carrier::KernelCursorEndpoint<'r, ROLE, K, EpochTbl, Mint, EndpointBinding<'r>>;

/// Affine flow handle for a pending send transition.
///
/// Created by `Endpoint::flow()` and consumed by `.send(arg).await`.
/// The type name matches the constructor for clarity: `flow()` → `CapFlow`.
pub(crate) struct CapFlow<'e, 'r, const ROLE: u8, M, K, Mint>
where
    M: MessageSpec + SendableLabel,
    K: carrier::SessionKitFamily + 'r,
    Mint: MintConfigMarker,
{
    endpoint: *mut KernelEndpoint<'r, ROLE, K, Mint>,
    preview: crate::endpoint::kernel::SendPreview,
    _msg: PhantomData<(&'e mut EndpointCfg<'r, K, Mint>, M)>,
}

/// Public flow facade returned by [`Endpoint::flow`](crate::Endpoint::flow).
struct FlowInner<'e, 'r, const ROLE: u8, M, K, Mint>
where
    M: MessageSpec + SendableLabel,
    K: carrier::SessionKitFamily + 'r,
    Mint: MintConfigMarker,
{
    inner: CapFlow<'e, 'r, ROLE, M, K, Mint>,
}

#[allow(private_bounds)]
pub struct Flow<'e, 'r, const ROLE: u8, M, K, Mint>
where
    M: MessageSpec + SendableLabel,
    K: carrier::SessionKitFamily + 'r,
    Mint: MintConfigMarker,
{
    inner: FlowInner<'e, 'r, ROLE, M, K, Mint>,
}

#[allow(private_bounds)]
impl<'e, 'r, const ROLE: u8, M, K, Mint> Flow<'e, 'r, ROLE, M, K, Mint>
where
    M: MessageSpec + SendableLabel,
    K: carrier::SessionKitFamily + 'r,
    Mint: MintConfigMarker,
{
    #[inline]
    pub(crate) fn from_cap_flow(inner: CapFlow<'e, 'r, ROLE, M, K, Mint>) -> Self {
        Self {
            inner: FlowInner { inner },
        }
    }
}

impl<'e, 'r, const ROLE: u8, M, K, Mint> CapFlow<'e, 'r, ROLE, M, K, Mint>
where
    M: MessageSpec + SendableLabel,
    K: carrier::SessionKitFamily + 'r,
    Mint: MintConfigMarker,
{
    pub(crate) fn new(
        endpoint: *mut KernelEndpoint<'r, ROLE, K, Mint>,
        preview: crate::endpoint::kernel::SendPreview,
    ) -> Self {
        Self {
            endpoint,
            preview,
            _msg: PhantomData,
        }
    }

    fn into_parts(
        self,
    ) -> (
        *mut KernelEndpoint<'r, ROLE, K, Mint>,
        crate::endpoint::kernel::SendPreview,
    ) {
        (self.endpoint, self.preview)
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Unified send implementation
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

impl<'e, 'r, 'cfg, const ROLE: u8, M, T, U, C, const MAX_RV: usize, Mint>
    CapFlow<'e, 'r, ROLE, M, crate::substrate::SessionKit<'cfg, T, U, C, MAX_RV>, Mint>
where
    M: MessageSpec + SendableLabel,
    M::Payload: crate::transport::wire::WireEncode,
    M::ControlKind: crate::endpoint::kernel::CanonicalTokenProvider<
            'r,
            ROLE,
            T,
            U,
            C,
            EpochTbl,
            Mint,
            MAX_RV,
            M,
            EndpointBinding<'r>,
        >,
    T: crate::substrate::Transport + 'cfg,
    U: crate::substrate::runtime::LabelUniverse + 'cfg,
    C: crate::substrate::runtime::Clock + 'cfg,
    Mint: MintConfigMarker,
{
    /// Send the message with a payload provider.
    ///
    /// The argument type is constrained by the message's control handling:
    /// - `ControlHandling::Canonical`: must pass `()`, auto-mints token
    /// - `ControlHandling::External` or `None`: must pass `&M::Payload`
    ///
    /// Type resolution happens at compile-time via the `FlowSendArg` trait.
    #[inline]
    pub(crate) fn send<'a, A>(
        self,
        arg: A,
    ) -> impl core::future::Future<
        Output = SendResult<
            ControlOutcome<
                'r,
                <<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind,
            >,
        >,
    > + 'a
    where
        A: FlowSendArg<'a, M, Mint>,
        M::Payload: 'a,
        M: 'a,
        A: 'a,
        Mint: 'a,
        'e: 'a,
        'r: 'a,
        'cfg: 'a,
        <<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind: 'r,
    {
        let (endpoint, preview) = self.into_parts();
        let payload = arg.into_payload();
        let send: crate::endpoint::kernel::SendWithPreviewFuture<
            'e,
            'a,
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
        > = unsafe { (&mut *endpoint).send_with_preview_in_place::<M>(preview, payload) };
        send
    }
}

#[allow(private_bounds)]
impl<'e, 'r, 'cfg, const ROLE: u8, M, T, U, C, const MAX_RV: usize, Mint>
    Flow<'e, 'r, ROLE, M, crate::substrate::SessionKit<'cfg, T, U, C, MAX_RV>, Mint>
where
    M: MessageSpec + SendableLabel,
    M::Payload: crate::transport::wire::WireEncode,
    M::ControlKind: crate::endpoint::kernel::CanonicalTokenProvider<
            'r,
            ROLE,
            T,
            U,
            C,
            EpochTbl,
            Mint,
            MAX_RV,
            M,
            EndpointBinding<'r>,
        >,
    T: crate::substrate::Transport + 'cfg,
    U: crate::substrate::runtime::LabelUniverse + 'cfg,
    C: crate::substrate::runtime::Clock + 'cfg,
    Mint: MintConfigMarker,
{
    #[inline]
    pub fn send<'a, A>(
        self,
        arg: A,
    ) -> impl core::future::Future<
        Output = SendResult<
            ControlOutcome<
                'r,
                <<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind,
            >,
        >,
    > + 'a
    where
        A: FlowSendArg<'a, M, Mint>,
        M::Payload: 'a,
        M: 'a,
        A: 'a,
        Mint: 'a,
        'e: 'a,
        'r: 'a,
        'cfg: 'a,
        <<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind: 'r,
    {
        self.inner.inner.send(arg)
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
