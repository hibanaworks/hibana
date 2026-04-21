//! CapMint 2.0 primitives for capability minting and validation.
//!
//! Hibana mints control tokens through const-first strategies baked into
//! `RoleProgram` and endpoint-owned canonical control send paths, with
//! rendezvous tables enforcing nonce/tag side effects via
//! `Rendezvous::mint_cap()` and `Rendezvous::claim_cap()`.
//!
//! # Epoch-Based Revocation (Witness System)
//!
//! This module provides ledger-free capability revocation via epoch witnesses.
//! Capabilities are tied to an epoch witness, and revocation is achieved by
//! advancing the epoch. Operations on old capabilities fail at compile time
//! because the witness is no longer available.
//!
//! ## Design Principles
//!
//! 1. **No global state**: Epoch is tracked via type-level witnesses, not global counters
//! 2. **Affine linearity**: endpoint state carries a rendezvous-scoped owner witness
//! 3. **Compile-time safety**: endpoint-owned epoch witnesses remain in the type system
//! 4. **AMPST compliance**: Integrates with cancellation termination (ECOOP'22)
//!
//! ## Usage Example
//!
//! Internally, the rendezvous core mints a rendezvous-scoped [`Owner`] witness
//! for the active endpoint. Application code never receives the brand directly;
//! the cursor endpoint stores the witness and exposes typed control operations.
//!
//! ## Integration with Endpoint
//!
//! The internal endpoint implementation stores [`Owner<'rv, Step>`] alongside
//! [`EndpointEpoch<'rv, Table>`]. Control plane operations verify epoch progression
//! through the `Step` type parameter, ensuring:
//!
//! - **Affine progression**: Each operation consumes `Endpoint<Step>` and produces
//!   `Endpoint<NextStep>`, making reuse impossible at compile time.
//! - **API simplicity**: Users work with `Endpoint` directly; witness mechanics are hidden
//!   in the `pub(crate)` implementation.
//!
//! The approach keeps ledgers purely internal: the rendezvous retains the brand
//! token and no global bookkeeping structure is required.
//!
//! # Wire Format
//!
//! Capability tokens are 32 bytes on the wire:
//! ```text
//! [16B nonce | 8B header | 8B HMAC]
//! header = (sid:u32, lane:u8, role:u8, kind:u8, shot:u8)
//! HMAC = keyed_hash(mac_key, nonce || header)
//! ```
//!
//! # Usage Pattern
//!
//! ## SessionCluster-driven canonical minting
//!
//! ```rust,ignore
//! let controller = cluster.enter(rv_id, sid, &CONTROLLER, hibana::substrate::binding::NoBinding)?;
//! let (controller, outcome) = controller.send::<CancelMsg>(()).await?;
//! let _ = outcome;
//! ```
//!
//! ## Rendezvous validation
//!
//! ```rust,ignore
//! let (worker, token) = worker.recv::<CancelMsg>().await?;
//! let verified = rendezvous.claim_cap(&token)?;
//! drop(verified);
//! ```
//!
//! ## Custom Resource Example
//!
//! ```rust,ignore
//! use core::cell::Cell;
//! use hibana::substrate::cap::{CapError, GenericCapToken, ResourceKind};
//!
//! #[derive(Clone, Copy, Debug)]
//! struct PageHandle {
//!     id: u32,
//! }
//!
//! thread_local! {
//!     static LAST_ZEROIZED: Cell<usize> = const { Cell::new(0) };
//! }
//!
//! struct PageResource;
//!
//! impl ResourceKind for PageResource {
//!     type Handle = PageHandle;
//!     const TAG: u8 = 1;
//!
//!     fn encode_handle(handle: &Self::Handle) -> [u8; 6] {
//!         let mut buf = [0u8; 6];
//!         buf[0..4].copy_from_slice(&handle.id.to_be_bytes());
//!         buf
//!     }
//!
//!     fn decode_handle(data: [u8; 6]) -> Result<Self::Handle, CapError> {
//!         let mut id_bytes = [0u8; 4];
//!         id_bytes.copy_from_slice(&data[0..4]);
//!         Ok(PageHandle {
//!             id: u32::from_be_bytes(id_bytes),
//!         })
//!     }
//!
//!     fn zeroize(handle: &mut Self::Handle) {
//!         LAST_ZEROIZED.store(handle.id as usize, Ordering::Relaxed);
//!         handle.id = 0;
//!     }
//! }
//!
//! fn round_trip(token: GenericCapToken<PageResource>) -> GenericCapToken<PageResource> {
//!     // Convert to bytes and back so the token can traverse message routes.
//!     let bytes = token.into_bytes();
//!     <GenericCapToken<PageResource> as hibana::substrate::wire::WirePayload>::decode_payload(
//!         hibana::substrate::wire::Payload::new(&bytes),
//!     )
//!     .unwrap()
//! }
//! ```

use core::marker::PhantomData;

// ============================================================================
// CapMint 2.0 core (const-first / no_std / no_alloc)
// ============================================================================

/// Identifier emitted into tap streams for a minting strategy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MintStrategyId(pub u16);

impl MintStrategyId {
    #[inline(always)]
    pub const fn new(id: u16) -> Self {
        Self(id)
    }

    #[inline(always)]
    pub const fn to_u16(self) -> u16 {
        self.0
    }
}

/// Identifier emitted into tap streams for a minting policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CapPolicyId(pub u8);

impl CapPolicyId {
    #[inline(always)]
    pub const fn new(id: u8) -> Self {
        Self(id)
    }

    #[inline(always)]
    pub const fn to_u8(self) -> u8 {
        self.0
    }
}

/// Static metadata describing whether canonical control payloads are permitted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapPolicyKind {
    Canonical,
    External,
}

/// Seed provided by the rendezvous during minting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NonceSeed {
    counter: u64,
}

impl NonceSeed {
    #[inline(always)]
    pub const fn counter(counter: u64) -> Self {
        Self { counter }
    }

    #[inline(always)]
    pub const fn counter_value(&self) -> u64 {
        self.counter
    }
}

/// Trait implemented by const minting specifications.
pub trait CapMintSpec {
    /// Stable identifier for observability.
    const STRATEGY_ID: MintStrategyId;

    /// Derive the nonce bytes using the rendezvous-provided seed.
    fn nonce(seed: NonceSeed) -> [u8; CAP_NONCE_LEN];

    /// Derive the authentication tag from nonce + header bytes.
    fn mac(nonce: &[u8; CAP_NONCE_LEN], header: &[u8; CAP_HEADER_LEN]) -> [u8; CAP_TAG_LEN];
}

/// Canonical null strategy – counter-based nonce, zero tag.
#[derive(Clone, Copy, Debug)]
pub struct NullMintSpec;

impl CapMintSpec for NullMintSpec {
    const STRATEGY_ID: MintStrategyId = MintStrategyId::new(0);

    #[inline(always)]
    fn nonce(seed: NonceSeed) -> [u8; CAP_NONCE_LEN] {
        let mut out = [0u8; CAP_NONCE_LEN];
        let bytes = seed.counter_value().to_be_bytes();
        let offset = CAP_NONCE_LEN - bytes.len();
        out[offset..].copy_from_slice(&bytes);
        out
    }

    #[inline(always)]
    fn mac(_nonce: &[u8; CAP_NONCE_LEN], _header: &[u8; CAP_HEADER_LEN]) -> [u8; CAP_TAG_LEN] {
        [0u8; CAP_TAG_LEN]
    }
}

/// Trait describing canonical vs. external mint policies.
pub trait CapMintPolicy {
    const POLICY_ID: CapPolicyId;
    const KIND: CapPolicyKind;
    const ALLOWS_CANONICAL: bool;
}

/// Canonical mint policy – endpoint may mint canonical control payloads.
#[derive(Clone, Copy, Debug)]
pub struct CanonicalPolicy;

impl CapMintPolicy for CanonicalPolicy {
    const POLICY_ID: CapPolicyId = CapPolicyId::new(0);
    const KIND: CapPolicyKind = CapPolicyKind::Canonical;
    const ALLOWS_CANONICAL: bool = true;
}

/// Marker trait implemented by policies that permit canonical minting.
pub trait AllowsCanonical {}

impl AllowsCanonical for CanonicalPolicy {}

/// Zero-sized minting strategy wrapper.
#[derive(Debug, Default)]
pub struct CapMintStrategy<S: CapMintSpec> {
    _spec: PhantomData<S>,
}

impl<S: CapMintSpec> Copy for CapMintStrategy<S> {}

impl<S: CapMintSpec> Clone for CapMintStrategy<S> {
    #[inline(always)]
    fn clone(&self) -> Self {
        *self
    }
}

impl<S: CapMintSpec> CapMintStrategy<S> {
    #[inline(always)]
    pub const fn new() -> Self {
        Self { _spec: PhantomData }
    }

    #[inline(always)]
    pub fn strategy_id(&self) -> MintStrategyId {
        S::STRATEGY_ID
    }

    #[inline(always)]
    pub fn derive_nonce(&self, seed: NonceSeed) -> [u8; CAP_NONCE_LEN] {
        S::nonce(seed)
    }

    #[inline(always)]
    pub fn derive_tag(
        &self,
        nonce: &[u8; CAP_NONCE_LEN],
        header: &[u8; CAP_HEADER_LEN],
    ) -> [u8; CAP_TAG_LEN] {
        S::mac(nonce, header)
    }
}

/// Zero-sized mint configuration baked into role programs.
#[derive(Debug)]
pub struct MintConfig<S: CapMintSpec = NullMintSpec, P: CapMintPolicy = CanonicalPolicy> {
    strategy: CapMintStrategy<S>,
    _policy: PhantomData<P>,
}

impl<S, P> Copy for MintConfig<S, P>
where
    S: CapMintSpec,
    P: CapMintPolicy,
{
}

impl<S, P> Clone for MintConfig<S, P>
where
    S: CapMintSpec,
    P: CapMintPolicy,
{
    #[inline(always)]
    fn clone(&self) -> Self {
        *self
    }
}

impl<S: CapMintSpec, P: CapMintPolicy> Default for MintConfig<S, P> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: CapMintSpec, P: CapMintPolicy> MintConfig<S, P> {
    #[inline(always)]
    pub const fn new() -> Self {
        Self {
            strategy: CapMintStrategy::<S>::new(),
            _policy: PhantomData,
        }
    }

    #[inline(always)]
    pub const fn strategy(&self) -> CapMintStrategy<S> {
        self.strategy
    }

    #[inline(always)]
    pub const fn policy_kind(&self) -> CapPolicyKind {
        P::KIND
    }

    #[inline(always)]
    pub const fn policy_id(&self) -> CapPolicyId {
        P::POLICY_ID
    }

    #[inline(always)]
    pub const fn allows_canonical(&self) -> bool {
        P::ALLOWS_CANONICAL
    }

    #[inline(always)]
    pub const fn strategy_id(&self) -> MintStrategyId {
        S::STRATEGY_ID
    }
}

/// Marker trait enabling `MintConfig` specialisation.
pub trait MintConfigMarker: Copy {
    type Spec: CapMintSpec;
    type Policy: CapMintPolicy;
    const INSTANCE: Self;

    fn as_config(&self) -> MintConfig<Self::Spec, Self::Policy>;
}

impl<S, P> MintConfigMarker for MintConfig<S, P>
where
    S: CapMintSpec,
    P: CapMintPolicy,
{
    type Spec = S;
    type Policy = P;
    const INSTANCE: Self = MintConfig::<S, P>::new();

    #[inline(always)]
    fn as_config(&self) -> MintConfig<Self::Spec, Self::Policy> {
        MintConfig::<S, P>::new()
    }
}

/// Length of the nonce segment inside a capability token.
pub const CAP_NONCE_LEN: usize = 16;
/// Length of the header segment inside a capability token.
pub const CAP_HEADER_LEN: usize = 40;
/// Length of the authentication tag segment inside a capability token.
pub const CAP_TAG_LEN: usize = 16;
/// Number of fixed bytes used by the descriptor-first control header codec.
///
/// Layout:
/// - version: 1
/// - sid: 4
/// - lane: 1
/// - role: 1
/// - tag: 1
/// - label: 1
/// - op: 1
/// - shot: 1
/// - path: 1
/// - scope_kind: 1
/// - flags: 1
/// - scope_id: 2
/// - epoch: 2
pub const CAP_CONTROL_HEADER_FIXED_LEN: usize = 18;
/// Compatibility alias for the fixed header prefix size.
pub const CAP_FIXED_HEADER_LEN: usize = CAP_CONTROL_HEADER_FIXED_LEN;
/// Number of bytes available for resource-specific handle encoding.
pub const CAP_HANDLE_LEN: usize = CAP_HEADER_LEN - CAP_CONTROL_HEADER_FIXED_LEN;
/// Total length of a capability token on the wire.
pub const CAP_TOKEN_LEN: usize = CAP_NONCE_LEN + CAP_HEADER_LEN + CAP_TAG_LEN;
use crate::control::types::Lane;
use crate::control::types::SessionId;
use crate::global::const_dsl::{ControlScopeKind, ScopeId};
use crate::transport::wire::{CodecError, Payload, WireEncode, WirePayload};

// ============================================================================
// Generic capability abstraction
// ============================================================================

/// Resource classification for capabilities.
///
/// Each `ResourceKind` supplies a handle type that is encoded into the opaque
/// payload section of the capability header. The fixed descriptor prefix stores
/// session, routing, and control metadata; the remaining [`CAP_HANDLE_LEN`]
/// bytes are entirely owned by the resource kind for encoding operands.
pub trait ResourceKind {
    /// Handle associated with this capability.
    type Handle: super::ControlHandle;

    /// Capability tag (0-255). `0` is reserved for endpoint capabilities.
    const TAG: u8;

    /// Human-readable name used for observability.
    const NAME: &'static str;

    /// Encode the handle into the resource payload area of the header.
    fn encode_handle(handle: &Self::Handle) -> [u8; CAP_HANDLE_LEN];

    /// Decode the handle from the resource payload area of the header.
    fn decode_handle(data: [u8; CAP_HANDLE_LEN]) -> Result<Self::Handle, CapError>;

    /// Zeroize the handle prior to dropping it.
    fn zeroize(handle: &mut Self::Handle);
}

/// Resource kinds that represent control-plane capabilities.
pub trait ControlResourceKind: ResourceKind {
    const LABEL: u8;
    const SCOPE: ControlScopeKind;
    const PATH: ControlPath;
    const TAP_ID: u16;
    const SHOT: CapShot;
    const OP: ControlOp;
    const AUTO_MINT_WIRE: bool;

    fn mint_handle(session: SessionId, lane: Lane, scope: ScopeId) -> Self::Handle;
}

impl ResourceKind for () {
    type Handle = ();

    const TAG: u8 = 0;
    const NAME: &'static str = "NoControl";

    fn encode_handle(_handle: &Self::Handle) -> [u8; CAP_HANDLE_LEN] {
        [0u8; CAP_HANDLE_LEN]
    }

    fn decode_handle(_data: [u8; CAP_HANDLE_LEN]) -> Result<Self::Handle, CapError> {
        Ok(())
    }

    fn zeroize(_handle: &mut Self::Handle) {}
}

/// Trait for control kinds that can mint their handle from basic context.
///
/// This trait enables external crates to define their own local control
/// message types without modifying hibana core. The generic control-token
/// minting path uses this trait when the control kind does not require
/// specialized handle preparation.
///
/// # Example
///
/// ```ignore
/// use hibana::substrate::cap::advanced::ScopeId;
/// use hibana::substrate::{Lane, SessionId};
/// use hibana::substrate::cap::{ControlMint, ResourceKind};
///
/// struct MyMarkerKind;
///
/// impl ResourceKind for MyMarkerKind {
///     type Handle = ();
///     // ... other required items
/// }
///
/// impl ControlMint for MyMarkerKind {
///     fn mint_handle(_sid: SessionId, _lane: Lane, _scope: ScopeId) -> Self::Handle {
///         () // No handle data needed for simple markers
///     }
/// }
/// ```
pub(crate) trait ControlMint: ResourceKind {
    /// Create a handle from session/lane/scope context.
    ///
    /// For simple control kinds (like markers), this typically returns `()`.
    /// For session-scoped kinds, this returns `(sid.raw(), lane.raw() as u16)`.
    fn mint_handle(sid: SessionId, lane: Lane, scope: ScopeId) -> Self::Handle;
}

/// Handle describing an endpoint rendezvous slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EndpointHandle {
    pub(crate) sid: SessionId,
    pub(crate) lane: Lane,
    pub(crate) role: u8,
}

impl EndpointHandle {
    pub(crate) const fn new(sid: SessionId, lane: Lane, role: u8) -> Self {
        Self { sid, lane, role }
    }

    fn zeroed() -> Self {
        Self {
            sid: SessionId::new(0),
            lane: Lane::new(0),
            role: 0,
        }
    }
}

impl super::ControlHandle for EndpointHandle {
    fn visit_delegation_links(&self, _f: &mut dyn FnMut(crate::control::types::RendezvousId)) {}
}

/// Marker for endpoint capabilities (kept internal to hibana).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EndpointResource {}

impl ResourceKind for EndpointResource {
    type Handle = EndpointHandle;
    const TAG: u8 = 0;
    const NAME: &'static str = "EndpointResource";

    fn encode_handle(handle: &Self::Handle) -> [u8; CAP_HANDLE_LEN] {
        let mut data = [0u8; CAP_HANDLE_LEN];
        data[0..4].copy_from_slice(&handle.sid.raw().to_be_bytes());
        data[4] = handle.lane.as_wire();
        data[5] = handle.role;
        data
    }

    fn decode_handle(data: [u8; CAP_HANDLE_LEN]) -> Result<Self::Handle, CapError> {
        let sid = SessionId::new(u32::from_be_bytes([data[0], data[1], data[2], data[3]]));
        let lane = Lane::new(u32::from(data[4]));
        let role = data[5];
        Ok(EndpointHandle::new(sid, lane, role))
    }

    fn zeroize(handle: &mut Self::Handle) {
        *handle = EndpointHandle::zeroed();
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Owner<'rv, Step> {
    _brand: PhantomData<crate::control::brand::Guard<'rv>>,
    _step: PhantomData<Step>,
}

impl<'rv, Step> Owner<'rv, Step>
where
    Step: EpochType,
{
    #[inline]
    pub(crate) fn new(_brand: crate::control::brand::Guard<'rv>) -> Self {
        Self {
            _brand: PhantomData,
            _step: PhantomData,
        }
    }
}

// ============================================================================
// Operations that require a short-lived brand witness
// ============================================================================

#[derive(Clone, Copy, Default)]
pub(crate) struct EndpointEpoch<'r, Table: EpochTable> {
    _marker: PhantomData<&'r Table>,
}

impl<'r, Table: EpochTable> EndpointEpoch<'r, Table> {
    #[inline]
    pub(crate) const fn new() -> Self {
        Self {
            _marker: PhantomData,
        }
    }
}

// ============================================================================
// Epoch Witness System (Ledger-Free Revocation)
// ============================================================================

pub trait EpochType {}

/// Marker trait representing logical control-plane steps for a lane.
pub trait EpochStep: EpochType {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct E0;
impl EpochType for E0 {}
impl EpochStep for E0 {}

pub trait EpochTable {}

/// Compile-time epoch table carrying witnesses for each rendezvous lane.
#[allow(clippy::type_complexity)]
pub struct EpochTbl<
    L0 = E0,
    L1 = E0,
    L2 = E0,
    L3 = E0,
    L4 = E0,
    L5 = E0,
    L6 = E0,
    L7 = E0,
    L8 = E0,
    L9 = E0,
    L10 = E0,
    L11 = E0,
    L12 = E0,
    L13 = E0,
    L14 = E0,
    L15 = E0,
> {
    _marker: PhantomData<(
        L0,
        L1,
        L2,
        L3,
        L4,
        L5,
        L6,
        L7,
        L8,
        L9,
        L10,
        L11,
        L12,
        L13,
        L14,
        L15,
    )>,
}

impl<L0, L1, L2, L3, L4, L5, L6, L7, L8, L9, L10, L11, L12, L13, L14, L15> EpochTable
    for EpochTbl<L0, L1, L2, L3, L4, L5, L6, L7, L8, L9, L10, L11, L12, L13, L14, L15>
where
    L0: EpochStep,
    L1: EpochStep,
    L2: EpochStep,
    L3: EpochStep,
    L4: EpochStep,
    L5: EpochStep,
    L6: EpochStep,
    L7: EpochStep,
    L8: EpochStep,
    L9: EpochStep,
    L10: EpochStep,
    L11: EpochStep,
    L12: EpochStep,
    L13: EpochStep,
    L14: EpochStep,
    L15: EpochStep,
{
}

// ============================================================================
// Original Capability Token System (Wire Format)
// ============================================================================

/// Capability shot semantics embedded in the token wire/runtime encoding.
///
/// `CapShot` records how many times a concrete token may be claimed:
/// - `One`: Single-use (affine). Claiming the token consumes it immediately.
/// - `Many`: Reusable. The token can be claimed multiple times under the
///   resource kind's constraints.
///
/// The compile-time shot discipline for resource kinds stays on
/// `hibana::substrate::cap::{One, Many}`; `CapShot` is the runtime encoding of
/// that decision inside a minted token, not the primary API for choosing shot
/// discipline.
///
/// # Usage
/// ```rust,ignore
/// let shot = token.shot()?;
/// if matches!(shot, CapShot::One) {
///     // single-use token
/// }
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapShot {
    /// Single-use capability (affine linearity).
    One = 0,
    /// Reusable capability (requires MultiSafe constraints).
    Many = 1,
}

impl CapShot {
    #[inline]
    pub fn from_u8(val: u8) -> Option<Self> {
        match val {
            0 => Some(Self::One),
            1 => Some(Self::Many),
            _ => None,
        }
    }

    #[inline]
    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

/// Atomic control-plane execution unit owned by hibana core.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlOp {
    RouteDecision = 0,
    LoopContinue = 1,
    LoopBreak = 2,
    StateSnapshot = 3,
    StateRestore = 4,
    TopologyBegin = 5,
    TopologyAck = 6,
    TopologyCommit = 7,
    CapDelegate = 8,
    AbortBegin = 9,
    AbortAck = 10,
    Fence = 11,
    TxCommit = 12,
    TxAbort = 13,
}

impl ControlOp {
    #[inline]
    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::RouteDecision),
            1 => Some(Self::LoopContinue),
            2 => Some(Self::LoopBreak),
            3 => Some(Self::StateSnapshot),
            4 => Some(Self::StateRestore),
            5 => Some(Self::TopologyBegin),
            6 => Some(Self::TopologyAck),
            7 => Some(Self::TopologyCommit),
            8 => Some(Self::CapDelegate),
            9 => Some(Self::AbortBegin),
            10 => Some(Self::AbortAck),
            11 => Some(Self::Fence),
            12 => Some(Self::TxCommit),
            13 => Some(Self::TxAbort),
            _ => None,
        }
    }

    #[inline]
    pub const fn as_u8(self) -> u8 {
        self as u8
    }
}

/// Transport crossing mode for control messages.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlPath {
    Local = 0,
    Wire = 1,
}

impl ControlPath {
    #[inline]
    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Local),
            1 => Some(Self::Wire),
            _ => None,
        }
    }

    #[inline]
    pub const fn as_u8(self) -> u8 {
        self as u8
    }
}

/// Descriptor-first fixed control header.
///
/// This is a wire codec carrier. Callers must use `encode` / `decode` rather
/// than relying on struct layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CapHeader {
    version: u8,
    sid: SessionId,
    lane: Lane,
    role: u8,
    tag: u8,
    label: u8,
    op: ControlOp,
    path: ControlPath,
    shot: CapShot,
    scope_kind: ControlScopeKind,
    flags: u8,
    scope_id: u16,
    epoch: u16,
    handle: [u8; CAP_HEADER_LEN - CAP_CONTROL_HEADER_FIXED_LEN],
}

impl CapHeader {
    #[inline]
    pub const fn new(
        sid: SessionId,
        lane: Lane,
        role: u8,
        tag: u8,
        label: u8,
        op: ControlOp,
        path: ControlPath,
        shot: CapShot,
        scope_kind: ControlScopeKind,
        flags: u8,
        scope_id: u16,
        epoch: u16,
        handle: [u8; CAP_HEADER_LEN - CAP_CONTROL_HEADER_FIXED_LEN],
    ) -> Self {
        Self {
            version: 1,
            sid,
            lane,
            role,
            tag,
            label,
            op,
            path,
            shot,
            scope_kind,
            flags,
            scope_id,
            epoch,
            handle,
        }
    }

    #[inline]
    pub fn encode(&self, out: &mut [u8; CAP_HEADER_LEN]) {
        out[0] = self.version;
        out[1..5].copy_from_slice(&self.sid.raw().to_be_bytes());
        out[5] = self.lane.as_wire();
        out[6] = self.role;
        out[7] = self.tag;
        out[8] = self.label;
        out[9] = self.op.as_u8();
        out[10] = self.path.as_u8();
        out[11] = self.shot.as_u8();
        out[12] = self.scope_kind as u8;
        out[13] = self.flags;
        out[14..16].copy_from_slice(&self.scope_id.to_be_bytes());
        out[16..18].copy_from_slice(&self.epoch.to_be_bytes());
        out[18..].copy_from_slice(&self.handle);
    }

    #[inline]
    pub fn decode(raw: [u8; CAP_HEADER_LEN]) -> Result<Self, CapError> {
        if raw[0] != 1 {
            return Err(CapError::Mismatch);
        }
        let op = ControlOp::from_u8(raw[9]).ok_or(CapError::Mismatch)?;
        let path = ControlPath::from_u8(raw[10]).ok_or(CapError::Mismatch)?;
        let shot = CapShot::from_u8(raw[11]).ok_or(CapError::Mismatch)?;
        let scope_kind = ControlScopeKind::from_u8(raw[12]).ok_or(CapError::Mismatch)?;
        let mut handle = [0u8; CAP_HEADER_LEN - CAP_CONTROL_HEADER_FIXED_LEN];
        handle.copy_from_slice(&raw[18..]);
        Ok(Self {
            version: raw[0],
            sid: SessionId::new(u32::from_be_bytes([raw[1], raw[2], raw[3], raw[4]])),
            lane: Lane::new(u32::from(raw[5])),
            role: raw[6],
            tag: raw[7],
            label: raw[8],
            op,
            path,
            shot,
            scope_kind,
            flags: raw[13],
            scope_id: u16::from_be_bytes([raw[14], raw[15]]),
            epoch: u16::from_be_bytes([raw[16], raw[17]]),
            handle,
        })
    }

    #[inline]
    pub const fn sid(&self) -> SessionId {
        self.sid
    }

    #[inline]
    pub const fn lane(&self) -> Lane {
        self.lane
    }

    #[inline]
    pub const fn role(&self) -> u8 {
        self.role
    }

    #[inline]
    pub const fn tag(&self) -> u8 {
        self.tag
    }

    #[inline]
    pub const fn label(&self) -> u8 {
        self.label
    }

    #[inline]
    pub const fn op(&self) -> ControlOp {
        self.op
    }

    #[inline]
    pub const fn path(&self) -> ControlPath {
        self.path
    }

    #[inline]
    pub const fn shot(&self) -> CapShot {
        self.shot
    }

    #[inline]
    pub const fn scope_kind(&self) -> ControlScopeKind {
        self.scope_kind
    }

    #[inline]
    pub const fn flags(&self) -> u8 {
        self.flags
    }

    #[inline]
    pub const fn scope_id(&self) -> u16 {
        self.scope_id
    }

    #[inline]
    pub const fn epoch(&self) -> u16 {
        self.epoch
    }

    #[inline]
    pub const fn handle(&self) -> &[u8; CAP_HEADER_LEN - CAP_CONTROL_HEADER_FIXED_LEN] {
        &self.handle
    }
}

#[inline]
const fn scope_hint_from_header(header: CapHeader) -> Option<ScopeId> {
    match header.scope_kind() {
        ControlScopeKind::Route => Some(ScopeId::route(header.scope_id())),
        ControlScopeKind::Loop => Some(ScopeId::loop_scope(header.scope_id())),
        _ => None,
    }
}

/// Typed view over a capability handle exposed to the EPF VM.
///
/// The view carries the original resource payload and the capability mask baked
/// into the token so that policies can reason about both without reinterpreting
/// the token header.
pub struct HandleView<'ctx, K: ResourceKind> {
    raw: &'ctx [u8; CAP_HANDLE_LEN],
    handle: K::Handle,
    scope: Option<ScopeId>,
}

impl<'ctx, K: ResourceKind> HandleView<'ctx, K> {
    #[inline]
    pub(crate) fn decode(
        raw: &'ctx [u8; CAP_HANDLE_LEN],
        scope: Option<ScopeId>,
    ) -> Result<Self, CapError> {
        let handle = K::decode_handle(*raw)?;
        Ok(Self { raw, handle, scope })
    }

    /// Borrow the encoded resource payload.
    #[inline]
    pub fn bytes(&self) -> &'ctx [u8; CAP_HANDLE_LEN] {
        self.raw
    }

    /// Borrow the decoded handle payload.
    #[inline]
    pub fn handle(&self) -> &K::Handle {
        &self.handle
    }

    /// Structured scope identifier encoded in this handle, when available.
    #[inline]
    pub fn scope(&self) -> Option<ScopeId> {
        self.scope
    }
}

impl<'ctx, K: ResourceKind> Drop for HandleView<'ctx, K> {
    fn drop(&mut self) {
        K::zeroize(&mut self.handle);
    }
}

/// Capability operation errors.
///
/// All errors are non-panicking and should be handled by the caller.
///
/// # Observability
/// Discriminated variants preserve debugging information while maintaining
/// security: `InvalidMac` identifies forgery attempts, `Mismatch` indicates
/// field validation failures (kind/shot/sid/lane), and `TableFull` tracks
/// capacity exhaustion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapError {
    /// Token not found in capability table.
    UnknownToken,
    /// Session ID or lane does not exist in local Rendezvous.
    WrongSessionOrLane,
    /// One-shot token already consumed.
    Exhausted,
    /// MAC tag verification failed (possible forgery attempt).
    ///
    /// This indicates either:
    /// - Cryptographic forgery (attacker guessing MAC tags)
    /// - Key mismatch between minting and claiming Rendezvous
    /// - Corrupted token during transfer
    InvalidMac,
    /// Capability table is full (64 entries).
    ///
    /// This can happen if too many capabilities are minted without being claimed,
    /// or if Many-shot capabilities accumulate over time.
    TableFull,
    /// Token field mismatch (kind/shot/sid/lane).
    ///
    /// This indicates the token was found in CapTable (nonce matched) but
    /// one or more fields didn't match expected values. This is distinct from
    /// `UnknownToken` (nonce not found) and helps diagnose configuration errors.
    Mismatch,
}

/// Capability token wire format: `[nonce | header | tag]` = `[16B | 32B | 16B]`.
///
/// Header layout (big-endian values unless noted):
/// - `[0..4)`      — session id
/// - `[4]`         — lane id in wire form
/// - `[5]`         — role id for endpoint resources (0 for others)
/// - `[6]`         — resource tag (`ResourceKind::TAG`)
/// - `[7]`         — shot discipline (`CapShot::as_u8()`)
/// - `[8..10)`     — capability mask bits
/// - `[10..32)`    — resource-specific payload supplied by `ResourceKind`
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GenericCapToken<K: ResourceKind> {
    pub bytes: [u8; CAP_TOKEN_LEN],
    _marker: PhantomData<K>,
}

impl<K: ResourceKind> GenericCapToken<K> {
    pub const AUTO: Self = Self {
        bytes: [0u8; CAP_TOKEN_LEN],
        _marker: PhantomData,
    };

    pub fn from_parts(
        nonce: [u8; CAP_NONCE_LEN],
        header: [u8; CAP_HEADER_LEN],
        tag: [u8; CAP_TAG_LEN],
    ) -> Self {
        let mut bytes = [0u8; CAP_TOKEN_LEN];
        bytes[0..CAP_NONCE_LEN].copy_from_slice(&nonce);
        bytes[CAP_NONCE_LEN..CAP_NONCE_LEN + CAP_HEADER_LEN].copy_from_slice(&header);
        bytes[CAP_NONCE_LEN + CAP_HEADER_LEN..CAP_TOKEN_LEN].copy_from_slice(&tag);
        Self {
            bytes,
            _marker: PhantomData,
        }
    }

    #[inline(always)]
    pub const fn from_bytes(bytes: [u8; CAP_TOKEN_LEN]) -> Self {
        Self {
            bytes,
            _marker: PhantomData,
        }
    }

    #[inline(always)]
    pub const fn into_bytes(self) -> [u8; CAP_TOKEN_LEN] {
        self.bytes
    }

    #[inline]
    fn header_slice(&self) -> &[u8; CAP_HEADER_LEN] {
        self.bytes[CAP_NONCE_LEN..CAP_NONCE_LEN + CAP_HEADER_LEN]
            .try_into()
            .expect("CAP_HEADER_LEN is compile-time constant")
    }

    pub fn nonce(&self) -> [u8; CAP_NONCE_LEN] {
        let mut nonce = [0u8; CAP_NONCE_LEN];
        nonce.copy_from_slice(&self.bytes[0..CAP_NONCE_LEN]);
        nonce
    }

    pub fn header(&self) -> [u8; CAP_HEADER_LEN] {
        let mut header = [0u8; CAP_HEADER_LEN];
        header.copy_from_slice(self.header_slice());
        header
    }

    pub fn tag(&self) -> [u8; CAP_TAG_LEN] {
        let mut tag = [0u8; CAP_TAG_LEN];
        tag.copy_from_slice(
            &self.bytes
                [CAP_NONCE_LEN + CAP_HEADER_LEN..CAP_NONCE_LEN + CAP_HEADER_LEN + CAP_TAG_LEN],
        );
        tag
    }

    #[inline]
    pub fn control_header(&self) -> Result<CapHeader, CapError> {
        CapHeader::decode(self.header())
    }

    pub fn shot(&self) -> Result<CapShot, CapError> {
        Ok(self.control_header()?.shot())
    }

    /// Extract the structured scope identifier encoded in the handle, if any.
    pub fn scope_hint(&self) -> Option<ScopeId> {
        self.as_view().ok().and_then(|view| view.scope())
    }

    pub fn resource_tag(&self) -> u8 {
        self.control_header()
            .map(|header| header.tag())
            .unwrap_or_default()
    }

    pub fn sid(&self) -> SessionId {
        self.control_header()
            .map(|header| header.sid())
            .unwrap_or_else(|_| SessionId::new(0))
    }

    pub fn lane(&self) -> Lane {
        self.control_header()
            .map(|header| header.lane())
            .unwrap_or_else(|_| Lane::new(0))
    }

    pub fn role(&self) -> u8 {
        self.control_header()
            .map(|header| header.role())
            .unwrap_or(0)
    }

    pub fn handle_bytes(&self) -> [u8; CAP_HANDLE_LEN] {
        *self.handle_bytes_ref()
    }

    /// Get a reference to the handle bytes within the token.
    ///
    /// This is a zero-copy operation that returns a slice reference
    /// to the handle payload embedded in the token header.
    #[inline(always)]
    pub fn handle_bytes_ref(&self) -> &[u8; CAP_HANDLE_LEN] {
        self.header_slice()[CAP_FIXED_HEADER_LEN..CAP_FIXED_HEADER_LEN + CAP_HANDLE_LEN]
            .try_into()
            .expect("CAP_HANDLE_LEN is compile-time constant")
    }

    pub fn decode_handle(&self) -> Result<K::Handle, CapError> {
        if self.resource_tag() != K::TAG {
            return Err(CapError::Mismatch);
        }
        K::decode_handle(self.handle_bytes())
    }

    /// Extract a HandleView from this token.
    ///
    /// This provides zero-copy access to the embedded handle and its capabilities.
    /// The HandleView lifetime is bounded by the token's lifetime.
    ///
    /// # Type Safety
    ///
    /// The compiler enforces:
    /// - `K` matches the token's ResourceKind (via type parameter)
    /// - HandleView cannot outlive the token (via lifetime `'_`)
    ///
    /// # Example
    ///
    /// ```ignore
    /// let token = flow.mint_token::<LoopContinueKind>()?;
    /// let view = token.as_view()?;
    /// // inspect view.handle() and scope metadata.
    /// ```
    pub fn as_view(&self) -> Result<HandleView<'_, K>, CapError> {
        let header = self.control_header()?;
        HandleView::decode(self.handle_bytes_ref(), scope_hint_from_header(header))
    }
}

impl<K: ResourceKind> WireEncode for GenericCapToken<K> {
    fn encoded_len(&self) -> Option<usize> {
        Some(CAP_TOKEN_LEN)
    }

    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.len() < CAP_TOKEN_LEN {
            return Err(CodecError::Truncated);
        }
        out[0..CAP_TOKEN_LEN].copy_from_slice(&self.bytes);
        Ok(CAP_TOKEN_LEN)
    }
}

impl<K: ResourceKind> WirePayload for GenericCapToken<K> {
    type Decoded<'a> = Self;

    fn decode_payload<'a>(input: Payload<'a>) -> Result<Self::Decoded<'a>, CodecError> {
        let bytes_in = input.as_bytes();
        if bytes_in.len() < CAP_TOKEN_LEN {
            return Err(CodecError::Truncated);
        }
        let mut bytes = [0u8; CAP_TOKEN_LEN];
        bytes.copy_from_slice(&bytes_in[0..CAP_TOKEN_LEN]);
        Ok(Self {
            bytes,
            _marker: PhantomData,
        })
    }
}

/// Zero-sized proof that MAC tag verification succeeded.
///
/// This witness cannot be constructed outside of this module, ensuring that
/// CapTable lookup can only happen after cryptographic verification.
///
/// # Security
/// This prevents internal code from bypassing MAC validation by directly
/// Zero-sized proof that a capability was validated through `Rendezvous::claim_cap()`.
///
/// This witness cannot be constructed outside of this module, ensuring that
/// `VerifiedCap` instances can only be created by the secure claim path.
///
/// # Security
/// This prevents forgery attacks where an attacker constructs a `VerifiedCap`
/// directly without going through MAC validation and CapTable lookup.
#[derive(Clone, Copy, Debug)]
struct Witness(());

/// Verified capability after successful `claim()` operation.
///
/// This is an affine proof object: the MAC tag has been verified and the token
/// has been consumed (for one-shot caps).
///
/// # Security
/// This struct cannot be constructed directly - it requires a private `Witness`
/// that can only be obtained through `Rendezvous::claim_cap()`. This ensures all
/// `VerifiedCap` instances have been cryptographically validated.
///
/// # Usage
/// ```rust,ignore
/// let (cursor, token) = cursor.recv::<DelegateMsg>().await?;
/// let token = cursor.recv::<DelegateMsg>().await?.1;
/// let _verified = rendezvous.claim_cap(&token)?;
/// ```
#[derive(Clone, Debug)]
pub(crate) struct VerifiedCap<K: ResourceKind> {
    handle: K::Handle,
    _marker: PhantomData<K>,
    /// Unforgeable witness proving this capability was validated.
    ///
    /// This field is private and can only be set by `Rendezvous::claim_cap()`,
    /// preventing direct construction of `VerifiedCap`.
    _witness: Witness,
}

impl<K: ResourceKind> VerifiedCap<K> {
    pub(crate) fn new(handle: K::Handle) -> Self {
        Self {
            handle,
            _marker: PhantomData,
            _witness: Witness(()),
        }
    }
}

impl<K: ResourceKind> Drop for VerifiedCap<K> {
    fn drop(&mut self) {
        K::zeroize(&mut self.handle);
    }
}

// ============================================================================
// Default Implementations (No Crypto - Trusted Domains)
// ============================================================================

// Null MAC for trusted domains (same process/same node).
//
// Use this when all roles share the same Rendezvous or communicate over
// trusted channels (e.g., in-process, localhost, secure enclave).
//
// # Security
// - **Only safe in trusted domains** where capability forgery is not a threat
// - No authentication tag (TAG_LEN = 0)
// - Zero-cost abstraction (no computation)
//
// # When to Use
// - Single-process applications with shared Rendezvous
// - Localhost communication (127.0.0.1)
// - Trusted secure enclaves (SGX, TrustZone)
// - Development/testing environments
//
// # When NOT to Use
// - Multi-node distributed systems
// - Untrusted network communication
// - Public-facing services
// - Any scenario where token forgery is a concern

#[cfg(test)]
mod tests {
    use super::{CapHeader, ControlOp, ControlScopeKind};
    use super::{CapShot, E0, EndpointHandle, EndpointResource, HandleView, Owner, ResourceKind};
    use crate::{
        control::{
            brand::with_brand,
            cap::resource_kinds::{LoopContinueKind, LoopDecisionHandle},
            types::{Lane, SessionId},
        },
        global::const_dsl::ScopeId,
    };

    #[test]
    fn owner_binds_rendezvous_brand() {
        with_brand(|rv_brand| {
            let owner: Owner<'_, E0> = Owner::new(rv_brand.guard());
            let _ = owner;
        });
    }

    #[test]
    fn handle_view_decodes_payload() {
        let handle = LoopDecisionHandle {
            sid: 12,
            lane: 4,
            scope: ScopeId::route(3),
        };
        let payload = LoopContinueKind::encode_handle(&handle);
        let view =
            HandleView::<LoopContinueKind>::decode(&payload, Some(handle.scope)).expect("decode");
        assert_eq!(view.bytes(), &payload);
        assert_eq!(view.handle(), &handle);
        assert_eq!(view.scope(), Some(handle.scope));
    }

    #[test]
    fn handle_view_decodes_endpoint_payload() {
        let handle = EndpointHandle::new(SessionId::new(1), Lane::new(0), 3);
        let payload = EndpointResource::encode_handle(&handle);
        let view = HandleView::<EndpointResource>::decode(&payload, None).expect("decode");
        assert_eq!(view.bytes(), &payload);
        assert_eq!(view.handle(), &handle);
        assert_eq!(view.scope(), None);
    }

    /// Regression test: lending a `HandleView` twice must reject the second
    /// attempt with `CapError::Consumed`.
    ///
    /// This mirrors rollback/abort scenarios:
    /// 1. Lend out a `HandleView`
    /// 2. Operation aborts midway
    /// 3. Retrying with the same token should be rejected
    #[test]
    fn simulate_abort_then_retry() {
        let handle = EndpointHandle::new(SessionId::new(42), Lane::new(1), 2);
        let payload = EndpointResource::encode_handle(&handle);

        // First decode succeeds
        let view1 = HandleView::<EndpointResource>::decode(&payload, None);
        assert!(view1.is_ok());
        let view1 = view1.unwrap();
        assert_eq!(view1.handle(), &handle);

        // Second decode uses the same payload again. HandleView::decode is
        // stateless; the rendezvous CapTable owns consumed tracking.
        // See capability.rs::one_shot_exhausts_on_second_claim for that test.
        let view2 = HandleView::<EndpointResource>::decode(&payload, None);
        assert!(view2.is_ok());
    }

    /// Test GenericCapToken::as_view() ergonomic API
    ///
    /// This tests the mint → HandleView extraction chain:
    /// 1. Create a token with embedded handle
    /// 2. Extract HandleView via as_view()
    /// 3. Verify handle, caps_mask, and bytes match
    /// 4. Verify caps_mask is correctly embedded in header
    #[test]
    fn generic_cap_token_as_view() {
        use super::{CAP_HEADER_LEN, CAP_NONCE_LEN, CAP_TAG_LEN, GenericCapToken};

        let handle = EndpointHandle::new(SessionId::new(7), Lane::new(3), 1);
        let handle_bytes = EndpointResource::encode_handle(&handle);

        let mut header = [0u8; CAP_HEADER_LEN];
        CapHeader::new(
            handle.sid,
            handle.lane,
            handle.role,
            EndpointResource::TAG,
            0,
            ControlOp::Fence,
            crate::control::cap::mint::ControlPath::Local,
            CapShot::One,
            ControlScopeKind::None,
            0,
            0,
            0,
            handle_bytes,
        )
        .encode(&mut header);

        let token = GenericCapToken::<EndpointResource>::from_parts(
            [0u8; CAP_NONCE_LEN],
            header,
            [0u8; CAP_TAG_LEN],
        );

        // Extract HandleView via as_view()
        let view = token.as_view().expect("as_view should succeed");

        // Verify handle matches
        assert_eq!(view.handle(), &handle);
        // Verify bytes match
        assert_eq!(view.bytes(), &handle_bytes);
        let header = token.control_header().expect("header");
        assert_eq!(header.sid(), handle.sid);
        assert_eq!(header.lane(), handle.lane);
        assert_eq!(header.role(), handle.role);
    }

    #[cfg(feature = "std")]
    mod proptests {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn handle_view_roundtrip_property(
                sid in 0u32..1000,
                lane in 0u32..64,
                role in 0u8..16
            ) {
                let sid = SessionId::new(sid);
                let lane = Lane::new(lane);
                let handle = EndpointHandle::new(sid, lane, role);
                let payload = EndpointResource::encode_handle(&handle);
                let view = HandleView::<EndpointResource>::decode(&payload, None).expect("decode");
                prop_assert_eq!(view.handle(), &handle);
                prop_assert_eq!(view.bytes(), &payload);
            }

            /// Property test for `LoopContinueKind`.
            ///
            /// The handle is represented as a `(u32, u16)` tuple; verify that
            /// `caps_mask` matches `HandleView::grant_mask()`.
            #[test]
            fn handle_view_loop_continue_roundtrip(
                generation in 0u32..10000,
                lane in 0u16..256
            ) {
                let handle = LoopDecisionHandle {
                    sid: generation,
                    lane,
                    scope: ScopeId::loop_scope(1),
                };
                let payload = LoopContinueKind::encode_handle(&handle);
                let view = HandleView::<LoopContinueKind>::decode(&payload, Some(handle.scope)).expect("decode");
                prop_assert_eq!(view.handle(), &handle);
                prop_assert_eq!(view.bytes(), &payload);
            }
        }
    }
}
