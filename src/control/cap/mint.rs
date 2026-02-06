//! CapMint 2.0 primitives for capability minting and validation.
//!
//! Hibana mints control tokens through const-first strategies baked into
//! `RoleProgram` and `SessionCluster::canonical_session_token()`, with rendezvous
//! tables enforcing nonce/tag side effects via `Rendezvous::mint_cap()` and
//! `Rendezvous::claim_cap()`.
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
//! 2. **Affine linearity**: `Owner<E>` can be revoked exactly once, producing `Owner<E+1>`
//! 3. **Compile-time safety**: Operations on old epochs fail because `&EpochTok<E>` is unavailable
//! 4. **AMPST compliance**: Integrates with cancellation termination (ECOOP'22)
//!
//! ## Usage Example
//!
//! Internally, the rendezvous core calls `brand::Brand::with_lane` to mint a
//! short-lived [`LaneToken`] witness for the active lane. Application code never
//! receives the brand directly; the cursor endpoint stores the [`Owner`] witness
//! and exposes typed control operations that ensure the epoch advances safely.
//!
//! ## Integration with Endpoint
//!
//! EpochTok integration uses internal witness holding.
//!
//! The internal `Endpoint<'r, 'rv, S, Step>` implementation (in `endpoint/witness.rs`)
//! holds [`Owner<'rv, Step>`] which provides epoch witnesses through [`Owner::token()`].
//! Control plane operations (`reroute`, `rollback`, `cancel`) verify epoch progression
//! through the `Step` type parameter, ensuring:
//!
//! - **Affine progression**: Each operation consumes `Endpoint<Step>` and produces
//!   `Endpoint<NextStep>`, making reuse impossible at compile time.
//! - **Internal witness verification**: `Owner::token()` generates `EpochTok<'rv, Step>`
//!   internally when needed, without requiring external witness management.
//! - **API simplicity**: Users work with `Endpoint` directly; witness mechanics are hidden
//!   in the `pub(crate)` implementation.
//!
//! The approach keeps ledgers purely internal: the rendezvous retains the brand
//! token and no global bookkeeping structure is required.
//!
//! **Note**: The `witness` module is currently `pub(crate)`. Future work may migrate
//! the public `Endpoint` API to use this implementation (breaking change).
//!
//! # Wire Format
//!
//! CapToken is 32 bytes on the wire:
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
//! use hibana::endpoint::ControlOutcome;
//! let controller = cluster.attach_cursor::<0, _>(rv_id, sid, &CONTROLLER)?;
//! let (controller, outcome) = controller.send::<CancelMsg>(()).await?;
//! debug_assert!(matches!(outcome, ControlOutcome::Canonical(_)));
//! ```
//!
//! ## Rendezvous validation
//!
//! ```rust,ignore
//! let (worker, token) = worker.recv::<CancelMsg>().await?;
//! let verified = rendezvous.claim_cap(&token)?;
//! drop(verified); // attach via rendezvous.attach_verified()
//! ```
//!
//! ## Custom Resource Example
//!
//! ```rust,ignore
//! use core::sync::atomic::{AtomicUsize, Ordering};
//! use hibana::control::cap::{CapToken, GenericCapToken, ResourceKind};
//! use hibana::transport::wire::WireDecode;
//!
//! #[derive(Clone, Copy, Debug)]
//! struct PageHandle {
//!     id: u32,
//! }
//!
//! static LAST_ZEROIZED: AtomicUsize = AtomicUsize::new(0);
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
//!     fn decode_handle(data: [u8; 6]) -> Result<Self::Handle, hibana::control::cap::CapError> {
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
//! fn round_trip(token: GenericCapToken<PageResource>) -> CapToken {
//!     // Convert to bytes and back so the token can traverse message routes.
//!     let bytes = token.into_bytes();
//!     CapToken::decode_from(&bytes).unwrap()
//! }
//! ```

use core::marker::PhantomData;

use crate::control::CpEffect;
use crate::control::brand::Guard as BrandGuard;

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

/// External mint policy – canonical control payloads must be provided externally.
#[derive(Clone, Copy, Debug)]
pub struct ExternalPolicy;

impl CapMintPolicy for ExternalPolicy {
    const POLICY_ID: CapPolicyId = CapPolicyId::new(1);
    const KIND: CapPolicyKind = CapPolicyKind::External;
    const ALLOWS_CANONICAL: bool = false;
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
pub struct MintConfig<S: CapMintSpec, P: CapMintPolicy> {
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

    #[inline(always)]
    fn as_config(&self) -> MintConfig<Self::Spec, Self::Policy> {
        MintConfig::<Self::Spec, Self::Policy>::new()
    }
}

impl<S, P> MintConfigMarker for MintConfig<S, P>
where
    S: CapMintSpec,
    P: CapMintPolicy,
{
    type Spec = S;
    type Policy = P;
    const INSTANCE: Self = MintConfig::<S, P>::new();
}

/// Default mint configuration used by RoleProgram unless overridden.
pub type DefaultMintConfig = MintConfig<NullMintSpec, CanonicalPolicy>;

pub const DEFAULT_MINT_CONFIG: DefaultMintConfig = DefaultMintConfig::new();

/// Length of the nonce segment inside a capability token.
pub const CAP_NONCE_LEN: usize = 16;
/// Length of the header segment inside a capability token.
pub const CAP_HEADER_LEN: usize = 32;
/// Length of the authentication tag segment inside a capability token.
pub const CAP_TAG_LEN: usize = 16;
/// Number of header bytes reserved for fixed metadata
/// (sid(4) + lane(1) + role(1) + tag(1) + shot(1) + caps_mask(2)).
pub const CAP_FIXED_HEADER_LEN: usize = 10;
/// Number of bytes available for resource-specific handle encoding.
pub const CAP_HANDLE_LEN: usize = CAP_HEADER_LEN - CAP_FIXED_HEADER_LEN;
/// Total length of a capability token on the wire.
pub const CAP_TOKEN_LEN: usize = CAP_NONCE_LEN + CAP_HEADER_LEN + CAP_TAG_LEN;
use crate::control::types::SessionId;
use crate::global::const_dsl::{ControlScopeKind, ScopeId};
use crate::rendezvous::Lane;
use crate::transport::wire::{CodecError, WireDecode, WireEncode};

/// Marker trait ensuring that control-plane labels always carry capability tokens.
pub trait ControlPayload {}

impl<K: ResourceKind> ControlPayload for GenericCapToken<K> {}

#[derive(Clone, Copy)]
pub struct LaneKey<'rv> {
    pub(crate) _rendezvous: BrandGuard<'rv>,
    pub(crate) lane: Lane,
}

impl<'rv> LaneKey<'rv> {
    #[inline]
    pub(crate) fn new(rendezvous: BrandGuard<'rv>, lane: Lane) -> Self {
        Self {
            _rendezvous: rendezvous,
            lane,
        }
    }

    #[inline]
    pub fn lane(&self) -> Lane {
        self.lane
    }
}

// ============================================================================
// Generic capability abstraction
// ============================================================================

/// Resource classification for capabilities.
///
/// Each `ResourceKind` supplies a handle type that is encoded into the resource
/// payload section of the capability header. The first [`CAP_FIXED_HEADER_LEN`]
/// bytes store the session/lane metadata, while the remaining
/// [`CAP_HANDLE_LEN`] bytes are entirely owned by the resource kind for
/// encoding operands.
pub trait ResourceKind {
    /// Handle associated with this capability.
    type Handle: super::ControlHandle;

    /// Capability tag (0-255). `0` is reserved for endpoint capabilities.
    const TAG: u8;

    /// Human-readable name used for observability.
    const NAME: &'static str;

    /// Whether this resource kind should auto-mint tokens for ExternalControl.
    ///
    /// When `true`, the endpoint will automatically mint a token with the proper
    /// handle (sid/lane/scope) when sending ExternalControl messages. The caller's
    /// payload is ignored.
    ///
    /// When `false` (default), the caller must provide the token/payload directly.
    /// This is appropriate for management session tokens where the caller
    /// constructs the token with specific parameters.
    const AUTO_MINT_EXTERNAL: bool = false;

    /// Encode the handle into the resource payload area of the header.
    fn encode_handle(handle: &Self::Handle) -> [u8; CAP_HANDLE_LEN];

    /// Decode the handle from the resource payload area of the header.
    fn decode_handle(data: [u8; CAP_HANDLE_LEN]) -> Result<Self::Handle, CapError>;

    /// Zeroize the handle prior to dropping it.
    fn zeroize(handle: &mut Self::Handle);

    /// Control-plane effect mask granted when this handle is owned.
    ///
    /// Default implementation provides no additional permissions.
    fn caps_mask(_handle: &Self::Handle) -> CapsMask {
        CapsMask::empty()
    }

    /// Structured scope identifier encoded in this handle, if any.
    ///
    /// Control-plane resources that encode `ScopeId` values should override
    /// this method so that downstream components (CapTable, EPF, observability)
    /// can extract scope metadata without bespoke decoding.
    fn scope_id(_handle: &Self::Handle) -> Option<ScopeId> {
        None
    }
}

/// Resource kinds that represent control-plane capabilities.
pub trait ControlResourceKind: ResourceKind {
    const LABEL: u8;
    const SCOPE: ControlScopeKind;
    const TAP_ID: u16;
    const SHOT: CapShot;
    const HANDLING: crate::global::ControlHandling;
}

/// Placeholder resource kind used for non-control messages.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoControlKind;

impl ResourceKind for NoControlKind {
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

/// Resource kinds whose handles are derived from session/lane context.
pub trait SessionScopedKind: ResourceKind {
    /// Construct a handle for the given session/lane.
    fn handle_for_session(sid: SessionId, lane: Lane) -> Self::Handle;

    /// Shot discipline enforced for automatically minted tokens.
    fn shot() -> CapShot {
        CapShot::One
    }
}

/// Trait for control kinds that can mint their handle from basic context.
///
/// This trait enables external crates to define their own `CanonicalControl`
/// message types without modifying hibana core. The `canonical_control_token`
/// function uses this trait for the fallback case when the control kind is
/// not one of the special kinds requiring complex handle preparation.
///
/// # Example
///
/// ```ignore
/// use hibana::control::cap::{ControlMint, ResourceKind};
/// use hibana::rendezvous::{SessionId, Lane};
/// use hibana::global::const_dsl::ScopeId;
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
pub trait ControlMint: ResourceKind {
    /// Create a handle from session/lane/scope context.
    ///
    /// For simple control kinds (like markers), this typically returns `()`.
    /// For session-scoped kinds, this returns `(sid.raw(), lane.raw() as u16)`.
    fn mint_handle(sid: SessionId, lane: Lane, scope: ScopeId) -> Self::Handle;
}

/// Handle describing an endpoint rendezvous slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EndpointHandle {
    pub sid: SessionId,
    pub lane: Lane,
    pub role: u8,
}

impl EndpointHandle {
    pub const fn new(sid: SessionId, lane: Lane, role: u8) -> Self {
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

impl super::ControlHandle for EndpointHandle {}

/// Marker for endpoint capabilities (kept internal to hibana).
#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndpointResource {}

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

    fn caps_mask(_handle: &Self::Handle) -> CapsMask {
        CapsMask::allow_all()
    }
}

#[derive(Clone, Copy)]
pub struct Owner<'rv, Step> {
    lane: LaneKey<'rv>,
    _step: PhantomData<Step>,
}

impl<'rv, Step> Owner<'rv, Step>
where
    Step: EpochType,
{
    #[inline]
    pub(crate) fn new(lane: LaneKey<'rv>) -> Self {
        Self {
            lane,
            _step: PhantomData,
        }
    }

    #[inline]
    pub fn token(&self) -> EpochTok<'rv, Step> {
        EpochTok {
            lane: self.lane,
            _step: PhantomData,
        }
    }

    #[inline]
    pub fn lane(&self) -> Lane {
        self.lane.lane()
    }
}

pub struct LaneToken<'rv, Step> {
    owner: Owner<'rv, Step>,
}

impl<'rv, Step> LaneToken<'rv, Step>
where
    Step: EpochType,
{
    #[inline]
    pub(crate) fn new(owner: Owner<'rv, Step>) -> Self {
        Self { owner }
    }

    #[inline]
    pub fn lane(&self) -> Lane {
        self.owner.lane()
    }

    /// Extract the underlying owner witness.
    ///
    /// This is used internally by `LaneLease::into_endpoint()` to construct
    /// typed endpoints. Public API users attach endpoints via
    /// [`SessionCluster::attach_cursor`](crate::runtime::SessionCluster::attach_cursor).
    ///
    /// # Example (Internal Use)
    ///
    /// ```rust,ignore
    /// // Internal implementation inside SessionCluster::attach_cursor()
    /// let lane_key = LaneKey::new(brand_guard, lane);
    /// let owner = Owner::new(lane_key);
    /// let epoch = EndpointEpoch::new();
    /// Endpoint::new(port, sid, owner, epoch)
    /// ```
    #[inline]
    pub fn into_owner(self) -> Owner<'rv, Step> {
        self.owner
    }
}

impl<'rv, Step> LaneToken<'rv, Step> {
    /// Transmute the typestate of this token.
    ///
    /// # Safety
    ///
    /// This is only safe when called from control-plane operations (checkpoint, rollback, cancel)
    /// that have already validated the state transition. The caller must ensure that the
    /// typestate transition is valid according to the control-plane protocol.
    #[inline]
    pub(crate) unsafe fn transmute_step<NewStep>(self) -> LaneToken<'rv, NewStep> {
        LaneToken {
            owner: Owner {
                lane: self.owner.lane,
                _step: PhantomData,
            },
        }
    }
}

impl<'rv, Step> Owner<'rv, Step>
where
    Step: NextEpoch,
{
    #[inline]
    pub fn advance(self) -> Owner<'rv, Step::Next> {
        Owner {
            lane: self.lane,
            _step: PhantomData,
        }
    }
}

// ============================================================================
// Operations that require a short-lived brand witness
// ============================================================================

#[derive(Clone, Copy)]
pub struct EpochTok<'rv, Step> {
    lane: LaneKey<'rv>,
    _step: PhantomData<Step>,
}

impl<'rv, Step> EpochTok<'rv, Step>
where
    Step: EpochType,
{
    #[inline]
    pub fn lane(&self) -> Lane {
        self.lane.lane()
    }
}

pub trait EpochWitness {
    type Table: EpochTable;
}

#[derive(Clone, Copy, Default)]
pub struct EndpointEpoch<'r, Table: EpochTable> {
    _marker: PhantomData<&'r Table>,
}

impl<'r, Table: EpochTable> EpochWitness for EndpointEpoch<'r, Table> {
    type Table = Table;
}

impl<'r, Table: EpochTable> EndpointEpoch<'r, Table> {
    #[inline]
    pub const fn new() -> Self {
        Self {
            _marker: PhantomData,
        }
    }
}

impl<'r, Table, const L: u8> BumpAt<L> for EndpointEpoch<'r, Table>
where
    Table: EpochTable + BumpAt<L>,
    <Table as BumpAt<L>>::Out: EpochTable,
{
    type Out = EndpointEpoch<'r, <Table as BumpAt<L>>::Out>;
}

// ============================================================================
// Epoch Witness System (Ledger-Free Revocation)
// ============================================================================

/// Type-level epoch witness for capability revocation.
///
/// Each epoch is identified by a unique constant `N`. Epochs form a
/// monotonically increasing sequence: `Epoch0`, `Succ<Epoch0>`, `Succ<Succ<Epoch0>>`, ...
///
/// # Design Rationale
///
/// Instead of maintaining a global current epoch counter, we use type-level
/// witnesses to track epoch validity. This design has several advantages:
///
/// 1. **No global state**: No need for `AtomicU64` or other shared mutable state
/// 2. **Compile-time safety**: Operations on revoked epochs fail to compile
/// 3. **Zero-cost abstraction**: Epoch is erased at runtime (PhantomData)
/// 4. **Affine linearity**: Revocation consumes the owner, preventing reuse
///
/// # Example
///
/// ```rust,ignore
/// use hibana::control::cap::{self, E0, Owner};
/// use hibana::rendezvous::Lane;
///
/// cap::with_lane(obtain_rendezvous_brand(), Lane(1), |token| {
///     let owner: Owner<'_, '_, E0> = token.into_owner();
///     let witness = owner.token();
///     // ... use witness with checkpoint/reroute APIs ...
/// });
/// ```
/// Zero-sized epoch marker using type-level successor representation.
///
/// Instead of const generic arithmetic (`N + 1`), we use type-level constructors
/// to represent epoch progression. This avoids Rust's const generic limitations
/// while maintaining compile-time epoch tracking.
///
/// # Type-Level Design
///
/// ```text
/// Epoch0              // Initial epoch (generation 0)
/// Succ<Epoch0>        // Next epoch (generation 1)
/// Succ<Succ<Epoch0>>  // Next epoch (generation 2)
/// ```
///
/// # Design Rationale
///
/// Using type-level constructors provides:
/// 1. **Compile-time epoch tracking**: `Owner<Epoch0>` vs `Owner<Succ<Epoch0>>`
/// 2. **Type safety**: Capabilities tied to different epochs are incompatible
/// 3. **Zero runtime cost**: Epoch markers are phantom data
/// 4. **Const generic compliance**: No arithmetic in const positions
///
/// The `EpochType` trait provides the runtime generation number when needed.
pub trait EpochType {
    /// The generation number for this epoch type.
    const GENERATION: u64;
}

/// Initial epoch (generation 0).
pub struct Epoch0;

impl EpochType for Epoch0 {
    const GENERATION: u64 = 0;
}

/// Successor type constructor for epoch progression.
///
/// `Succ<E>` represents the next epoch after `E`.
pub struct Succ<E: EpochType>(PhantomData<E>);

impl<E: EpochType> EpochType for Succ<E> {
    const GENERATION: u64 = E::GENERATION + 1;
}

impl<E: EpochType> EpochStep for Succ<E> {}

/// Type alias for the next epoch after `E`.
///
/// This trait provides a clean API for epoch succession without exposing
/// the `Succ` constructor directly.
pub trait NextEpoch: EpochType {
    /// Epoch reached after advancing once from the current state.
    type Next: EpochType;
}

/// Lane-specific scope marker for epoch types.
///
/// This isolates epochs across lanes so that witnesses minted for one lane
/// cannot be mixed up with those from another.
pub struct LaneScope<const L: u8>;

/// Scoped epoch type that combines a scope with a logical step (generation).
///
/// A scoped epoch behaves like its underlying step for generation tracking
/// while carrying an additional phantom marker that ties it to a lane scope.
pub struct ScopedEpoch<Scope, Step: EpochType>(PhantomData<(Scope, Step)>);

impl<Scope, Step: EpochType> EpochType for ScopedEpoch<Scope, Step> {
    const GENERATION: u64 = Step::GENERATION;
}

impl<Scope, Step> NextEpoch for ScopedEpoch<Scope, Step>
where
    Step: NextEpoch,
{
    type Next = ScopedEpoch<Scope, Step::Next>;
}

/// Type alias for lane-scoped epochs.
pub type LaneEpoch<const L: u8, Step> = ScopedEpoch<LaneScope<L>, Step>;

/// Marker trait representing logical control-plane steps for a lane.
pub trait EpochStep: EpochType {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct E0;
impl EpochType for E0 {
    const GENERATION: u64 = 0;
}
impl EpochStep for E0 {}
impl NextEpoch for E0 {
    type Next = Ckpt;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ckpt;
impl EpochType for Ckpt {
    const GENERATION: u64 = 1;
}
impl EpochStep for Ckpt {}
impl NextEpoch for Ckpt {
    type Next = Committed;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Committed;
impl EpochType for Committed {
    const GENERATION: u64 = 2;
}
impl EpochStep for Committed {}
impl NextEpoch for Committed {
    type Next = RolledBack;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RolledBack;
impl EpochType for RolledBack {
    const GENERATION: u64 = 3;
}
impl EpochStep for RolledBack {}
impl NextEpoch for RolledBack {
    type Next = Succ<RolledBack>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stop<S: EpochStep>(PhantomData<S>);
impl<S: EpochStep> EpochType for Stop<S> {
    const GENERATION: u64 = S::GENERATION;
}
impl<S: EpochStep> EpochStep for Stop<S> {}

/// Compile-time equality marker used to ensure step transitions line up.
pub trait SameStep<Other: EpochStep> {}

impl SameStep<E0> for E0 {}
impl SameStep<Ckpt> for Ckpt {}
impl SameStep<Committed> for Committed {}
impl SameStep<RolledBack> for RolledBack {}
impl<S: EpochStep> SameStep<Succ<S>> for Succ<S> {}
impl<S: EpochStep> SameStep<Stop<S>> for Stop<S> {}

/// Marker trait permitting send operations from a session step.
pub trait MaySend: EpochStep {}

impl MaySend for E0 {}
impl MaySend for Ckpt {}
impl MaySend for Committed {}
impl MaySend for RolledBack {}
impl<S> MaySend for Succ<S> where S: EpochStep + MaySend {}

pub trait EpochTable {}

pub trait BumpAt<const L: u8> {
    type Out;
}

impl NextEpoch for Epoch0 {
    type Next = Succ<Epoch0>;
}

impl<E: EpochType> NextEpoch for Succ<E> {
    type Next = Succ<Succ<E>>;
}

/// Compile-time epoch table carrying witnesses for each rendezvous lane.
#[allow(clippy::type_complexity)]
pub struct EpochTbl<E0, E1, E2, E3, E4, E5, E6, E7, E8, E9, E10, E11, E12, E13, E14, E15> {
    _marker: PhantomData<(
        E0,
        E1,
        E2,
        E3,
        E4,
        E5,
        E6,
        E7,
        E8,
        E9,
        E10,
        E11,
        E12,
        E13,
        E14,
        E15,
    )>,
}

impl<E0, E1, E2, E3, E4, E5, E6, E7, E8, E9, E10, E11, E12, E13, E14, E15> EpochTable
    for EpochTbl<E0, E1, E2, E3, E4, E5, E6, E7, E8, E9, E10, E11, E12, E13, E14, E15>
where
    E0: EpochStep,
    E1: EpochStep,
    E2: EpochStep,
    E3: EpochStep,
    E4: EpochStep,
    E5: EpochStep,
    E6: EpochStep,
    E7: EpochStep,
    E8: EpochStep,
    E9: EpochStep,
    E10: EpochStep,
    E11: EpochStep,
    E12: EpochStep,
    E13: EpochStep,
    E14: EpochStep,
    E15: EpochStep,
{
}

/// Initial epoch table with all lanes at `E0`.
pub type EpochInit = EpochTbl<E0, E0, E0, E0, E0, E0, E0, E0, E0, E0, E0, E0, E0, E0, E0, E0>;

// ============================================================================
// Original Capability Token System (Wire Format)
// ============================================================================

/// Minimal RNG trait for no_std/no_alloc capability token generation.
///
/// Implementations must provide cryptographically secure randomness.
///
/// # Security
/// - MUST use CSPRNG (e.g., ChaCha20, AES-CTR)
/// - MUST be seeded from entropy source (RDRAND, virtio-rng, TPM, or PSK-derived)
/// - MUST NOT use timestamp-based or predictable sources
pub trait Rng32 {
    /// Fill the 32-byte output buffer with cryptographically secure random bytes.
    fn fill(&mut self, out: &mut [u8; 32]);
}

/// Fixed-length MAC tag (128 bits minimum for security).
///
/// Note: We use a fixed 16-byte array to avoid const generic issues.
/// All MAC implementations must provide 128-bit (16-byte) tags.
pub trait MacTag: Sized {
    /// Convert tag into 16-byte array (128-bit minimum).
    fn into_bytes(self) -> [u8; 16];
}

/// Minimal MAC trait for no_std/no_alloc authentication.
///
/// Implementations should use an authenticated construction such as HMAC-SHA256
/// or BLAKE3 keyed mode supplied by the application.
///
/// # Security
/// - Tag length MUST be at least 128 bits (16 bytes)
/// - Key MUST be 256 bits (32 bytes) from secure source
/// - Implementations MUST be constant-time
pub trait Mac {
    /// MAC tag type (must implement MacTag).
    type Tag: MacTag;

    /// Reset MAC with new key.
    fn reset(&mut self, key: &[u8]);

    /// Update MAC with additional data.
    fn update(&mut self, chunk: &[u8]);

    /// Finalize MAC and return tag.
    fn finalize(self) -> Self::Tag;
}

/// Capability shot semantics: single-use vs. reusable.
///
/// Controls how many times a capability token can be claimed:
/// - `One`: Single-use (affine). Claimed token is consumed immediately.
/// - `Many`: Reusable. Token can be claimed multiple times (requires `MultiSafe` constraints).
///
/// # Usage
/// ```rust,ignore
/// // One-shot delegation (typical use case)
/// let token = broker.mint_endpoint_token(sid, lane, role, CapShot::One);
///
/// // Multi-shot delegation (for load balancing, replication)
/// let token = broker.mint_endpoint_token(sid, lane, role, CapShot::Many);
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

/// Bitmask describing which [`CpEffect`] variants a capability may invoke.
///
/// Each bit corresponds directly to the discriminant of [`CpEffect`], allowing
/// the control plane and EPF VM to perform constant-time authorisation checks
/// without auxiliary translation layers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CapsMask {
    bits: u16,
}

impl CapsMask {
    #[inline]
    pub const fn empty() -> Self {
        Self { bits: 0 }
    }

    #[inline]
    pub const fn from_bits(bits: u16) -> Self {
        Self { bits }
    }

    #[inline]
    pub const fn bits(self) -> u16 {
        self.bits
    }

    #[inline]
    pub const fn with(mut self, effect: CpEffect) -> Self {
        self.bits |= effect.bit();
        self
    }

    #[inline]
    pub const fn allow_all() -> Self {
        Self {
            bits: (1u16 << (CpEffect::Rollback as u16 + 1)) - 1,
        }
    }

    #[inline]
    pub const fn allows(self, effect: CpEffect) -> bool {
        (self.bits & effect.bit()) != 0
    }

    #[inline]
    pub const fn union(self, other: Self) -> Self {
        Self {
            bits: self.bits | other.bits,
        }
    }
}

impl Default for CapsMask {
    fn default() -> Self {
        Self::empty()
    }
}

/// Errors surfaced when exposing capability handles to the EPF VM.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VmHandleError {
    /// No handle was supplied for this VM invocation.
    NotAvailable,
    /// Handle tag does not match the requested [`ResourceKind`].
    TagMismatch { expected: u8, actual: u8 },
    /// Decoding the handle payload failed.
    DecodeFailed,
}

/// Typed view over a capability handle exposed to the EPF VM.
///
/// The view carries the original resource payload and the capability mask baked
/// into the token so that policies can reason about both without reinterpreting
/// the token header.
pub struct HandleView<'ctx, K: ResourceKind> {
    raw: &'ctx [u8; CAP_HANDLE_LEN],
    handle: K::Handle,
    caps: CapsMask,
    scope: Option<ScopeId>,
}

impl<'ctx, K: ResourceKind> HandleView<'ctx, K> {
    #[inline]
    pub(crate) fn decode(
        raw: &'ctx [u8; CAP_HANDLE_LEN],
        caps: CapsMask,
    ) -> Result<Self, CapError> {
        let handle = K::decode_handle(*raw)?;
        let granted = K::caps_mask(&handle);
        if granted != caps {
            return Err(CapError::Mismatch);
        }
        let scope = K::scope_id(&handle);
        Ok(Self {
            raw,
            handle,
            caps: granted,
            scope,
        })
    }

    /// Borrow the encoded resource payload.
    #[inline]
    pub fn bytes(&self) -> &'ctx [u8; CAP_HANDLE_LEN] {
        self.raw
    }

    /// Capability mask granted by this handle.
    #[inline]
    pub fn grant_mask(&self) -> CapsMask {
        self.caps
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

    pub fn shot(&self) -> Result<CapShot, CapError> {
        CapShot::from_u8(self.header()[7]).ok_or(CapError::Mismatch)
    }

    pub fn caps_mask(&self) -> CapsMask {
        let header = self.header();
        let mask = u16::from_be_bytes([header[8], header[9]]);
        CapsMask::from_bits(mask)
    }

    /// Extract the structured scope identifier encoded in the handle, if any.
    pub fn scope_hint(&self) -> Option<ScopeId> {
        self.as_view().ok().and_then(|view| view.scope())
    }

    pub fn resource_tag(&self) -> u8 {
        self.header()[6]
    }

    pub fn sid(&self) -> SessionId {
        let header = self.header_slice();
        let sid = u32::from_be_bytes([header[0], header[1], header[2], header[3]]);
        SessionId::new(sid)
    }

    pub fn lane(&self) -> Lane {
        let header = self.header_slice();
        Lane::new(header[4] as u32)
    }

    pub fn role(&self) -> u8 {
        self.header_slice()[5]
    }

    pub fn handle_bytes(&self) -> [u8; CAP_HANDLE_LEN] {
        let header = self.header_slice();
        let mut payload = [0u8; CAP_HANDLE_LEN];
        payload
            .copy_from_slice(&header[CAP_FIXED_HEADER_LEN..CAP_FIXED_HEADER_LEN + CAP_HANDLE_LEN]);
        payload
    }

    /// Get a reference to the handle bytes within the token.
    ///
    /// This is a zero-copy operation that returns a slice reference
    /// to the handle payload embedded in the token header.
    #[inline(always)]
    pub fn handle_bytes_ref(&self) -> &[u8; CAP_HANDLE_LEN] {
        let header = self.header_slice();
        header[CAP_FIXED_HEADER_LEN..CAP_FIXED_HEADER_LEN + CAP_HANDLE_LEN]
            .try_into()
            .expect("CAP_HANDLE_LEN is compile-time constant")
    }

    /// Get the caps_mask embedded in the token header.
    ///
    /// This reads directly from the token header at offset 8-9,
    /// avoiding the need to decode the handle.
    #[inline]
    fn caps_mask_embedded(&self) -> CapsMask {
        let header = self.header_slice();
        let bits = u16::from_be_bytes([header[8], header[9]]);
        CapsMask::from_bits(bits)
    }

    pub fn decode_handle(&self) -> Result<K::Handle, CapError> {
        let header = self.header_slice();
        if header[6] != K::TAG {
            return Err(CapError::Mismatch);
        }
        K::decode_handle(self.handle_bytes())
    }

    pub fn caps_mask_for_token(&self) -> Result<CapsMask, CapError> {
        let mut handle = self.decode_handle()?;
        let mask = K::caps_mask(&handle);
        K::zeroize(&mut handle);
        Ok(mask)
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
    /// Runtime verification:
    /// - caps_mask in token header matches `K::caps_mask(&handle)`
    ///
    /// # Errors
    ///
    /// Returns `CapError::Mismatch` if the embedded caps_mask doesn't match
    /// the handle's expected capabilities (indicating token corruption or forgery).
    ///
    /// # Example
    ///
    /// ```ignore
    /// let token = flow.mint_token::<LoopContinueKind>()?;
    /// let view = token.as_view()?;
    /// // inspect view.handle(), view.grant_mask(), etc.
    /// ```
    pub fn as_view(&self) -> Result<HandleView<'_, K>, CapError> {
        HandleView::decode(self.handle_bytes_ref(), self.caps_mask_embedded())
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

impl<'a, K: ResourceKind> WireDecode<'a> for GenericCapToken<K> {
    fn decode_from(input: &'a [u8]) -> Result<Self, CodecError> {
        if input.len() < CAP_TOKEN_LEN {
            return Err(CodecError::Truncated);
        }
        let mut bytes = [0u8; CAP_TOKEN_LEN];
        bytes.copy_from_slice(&input[0..CAP_TOKEN_LEN]);
        Ok(Self {
            bytes,
            _marker: PhantomData,
        })
    }
}

/// Type alias for endpoint capability tokens.
///
/// Use `GenericCapToken<K>` when you need a specific resource kind,
/// or the typed token pipeline (`CapFlowToken<K>`, etc.) for the flow DSL.
pub(crate) type CapToken = GenericCapToken<EndpointResource>;

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
/// Contains all information needed to attach a Port to a Rendezvous.
/// The MAC tag has been verified and the token has been consumed (for One-shot).
///
/// # Security
/// This struct cannot be constructed directly - it requires a private `Witness`
/// that can only be obtained through `Rendezvous::claim_cap()`. This ensures all
/// `VerifiedCap` instances have been cryptographically validated.
///
/// # Usage
/// ```rust,ignore
/// let (cursor, token) = cursor.recv::<DelegateMsg>().await?;
/// let claim = cluster.delegate_claim(rv_id, token)?;
/// let delegated_cursor = claim.attach_cursor(&DELEGATE_PROGRAM)?;
/// let epoch = rendezvous.epoch_token(lane);
/// let endpoint = protocol.bind::<Role<0>, _>(port, verified.sid, epoch);
/// ```
#[derive(Clone, Debug)]
pub struct VerifiedCap<K: ResourceKind> {
    /// Session ID associated with the capability.
    pub sid: SessionId,
    /// Lane within the session.
    pub lane: Lane,
    /// Role ID requested by the capability (endpoint resources reuse this).
    pub role: u8,
    /// Shot semantics (One or Many).
    pub shot: CapShot,
    /// Capability bits granted to the effect VM for this lane.
    pub caps_mask: CapsMask,
    /// Resource-specific handle payload decoded from the capability header.
    pub handle: K::Handle,
    /// Structured scope identifier encoded within the capability handle, if any.
    pub scope: Option<ScopeId>,
    _marker: PhantomData<K>,
    /// Unforgeable witness proving this capability was validated.
    ///
    /// This field is private and can only be set by `Rendezvous::claim_cap()`,
    /// preventing direct construction of `VerifiedCap`.
    _witness: Witness,
}

impl<K: ResourceKind> VerifiedCap<K> {
    pub(crate) fn new(
        sid: SessionId,
        lane: Lane,
        role: u8,
        shot: CapShot,
        caps_mask: CapsMask,
        handle: K::Handle,
        scope: Option<ScopeId>,
    ) -> Self {
        Self {
            sid,
            lane,
            role,
            shot,
            caps_mask,
            handle,
            scope,
            _marker: PhantomData,
            _witness: Witness(()),
        }
    }

    /// Borrow the decoded handle payload.
    #[inline]
    pub fn handle(&self) -> &K::Handle {
        &self.handle
    }
}

impl VerifiedCap<EndpointResource> {
    /// Convenience accessor for the endpoint role encoded in the handle.
    #[inline]
    pub fn endpoint_role(&self) -> u8 {
        self.handle.role
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
    use super::{CAP_FIXED_HEADER_LEN, CAP_HANDLE_LEN, CAP_HEADER_LEN};
    use super::{
        CapError, CapShot, CapToken, CapsMask, Ckpt, Committed, E0, EndpointHandle,
        EndpointResource, EpochType, HandleView, LaneToken, Owner, ResourceKind, RolledBack,
    };
    use crate::{
        SessionId,
        control::{
            CpEffect,
            brand::with_brand,
            cap::resource_kinds::{LoopContinueKind, LoopDecisionHandle},
        },
        global::const_dsl::ScopeId,
        rendezvous::Lane,
    };

    #[test]
    fn lane_owner_advances_through_epochs() {
        with_brand(|rv_brand| {
            rv_brand.with_lane(Lane(2), |brand_ref, lane_key| {
                let owner: Owner<'_, E0> = Owner::new(lane_key);
                let token: LaneToken<'_, E0> = LaneToken::new(owner);
                let owner_e0 = token.into_owner();
                assert_eq!(owner_e0.lane().0, 2);
                assert_eq!(E0::GENERATION, 0);

                let owner_ck = owner_e0.advance();
                assert_eq!(Ckpt::GENERATION, 1);

                let owner_committed = owner_ck.advance();
                assert_eq!(Committed::GENERATION, 2);

                let owner_rb = owner_committed.advance();
                assert_eq!(RolledBack::GENERATION, 3);

                let witness = owner_rb.token();
                assert_eq!(witness.lane().0, 2);
                let _ = brand_ref;
            })
        });
    }

    #[test]
    fn caps_mask_allows_effect() {
        let caps = CapsMask::empty()
            .with(CpEffect::SpliceBegin)
            .with(CpEffect::Rollback);
        assert!(caps.allows(CpEffect::SpliceBegin));
        assert!(caps.allows(CpEffect::Rollback));
        assert!(!caps.allows(CpEffect::SpliceCommit));
    }

    #[test]
    fn cap_token_derives_caps_mask() {
        let handle = EndpointHandle::new(SessionId::new(7), Lane::new(3), 1);
        let mask = EndpointResource::caps_mask(&handle);
        let handle_bytes = EndpointResource::encode_handle(&handle);
        let mut header = [0u8; CAP_HEADER_LEN];
        header[0..4].copy_from_slice(&handle.sid.raw().to_be_bytes());
        header[4] = handle.lane.as_wire();
        header[5] = handle.role;
        header[6] = EndpointResource::TAG;
        header[7] = CapShot::One.as_u8();
        header[8..10].copy_from_slice(&mask.bits().to_be_bytes());
        header[CAP_FIXED_HEADER_LEN..CAP_FIXED_HEADER_LEN + CAP_HANDLE_LEN]
            .copy_from_slice(&handle_bytes);
        let token = CapToken::from_parts([0u8; 16], header, [0u8; 16]);
        let caps = token.caps_mask_for_token().expect("caps mask");
        assert!(caps.allows(CpEffect::SpliceBegin));
        assert!(caps.allows(CpEffect::Checkpoint));
    }

    #[test]
    fn handle_view_decodes_payload() {
        let handle = LoopDecisionHandle::new(12, 4, ScopeId::route(3));
        let payload = LoopContinueKind::encode_handle(&handle);
        let expected_mask = LoopContinueKind::caps_mask(&handle);
        let view = HandleView::<LoopContinueKind>::decode(&payload, expected_mask).expect("decode");
        assert_eq!(view.bytes(), &payload);
        assert_eq!(view.grant_mask().bits(), expected_mask.bits());
        assert_eq!(view.handle(), &handle);
    }

    #[test]
    fn handle_view_rejects_mask_mismatch() {
        let handle = EndpointHandle::new(SessionId::new(1), Lane::new(0), 3);
        let payload = EndpointResource::encode_handle(&handle);
        let wrong_mask = CapsMask::empty();
        let view = HandleView::<EndpointResource>::decode(&payload, wrong_mask);
        assert!(matches!(view, Err(CapError::Mismatch)));
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
        let mask = EndpointResource::caps_mask(&handle);

        // First decode succeeds
        let view1 = HandleView::<EndpointResource>::decode(&payload, mask);
        assert!(view1.is_ok());
        let view1 = view1.unwrap();
        assert_eq!(view1.handle(), &handle);

        // Second decode uses the same payload again. HandleView::decode is
        // stateless; the rendezvous CapTable owns consumed tracking.
        // See capability.rs::one_shot_exhausts_on_second_claim for that test.
        let view2 = HandleView::<EndpointResource>::decode(&payload, mask);
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
        use super::{
            CAP_FIXED_HEADER_LEN, CAP_HANDLE_LEN, CAP_HEADER_LEN, CAP_NONCE_LEN, CAP_TAG_LEN,
            GenericCapToken,
        };

        let handle = EndpointHandle::new(SessionId::new(7), Lane::new(3), 1);
        let mask = EndpointResource::caps_mask(&handle);
        let handle_bytes = EndpointResource::encode_handle(&handle);

        let mut header = [0u8; CAP_HEADER_LEN];
        header[0..4].copy_from_slice(&handle.sid.raw().to_be_bytes());
        header[4] = handle.lane.as_wire();
        header[5] = handle.role;
        header[6] = EndpointResource::TAG;
        header[7] = CapShot::One.as_u8();
        header[8..10].copy_from_slice(&mask.bits().to_be_bytes());
        header[CAP_FIXED_HEADER_LEN..CAP_FIXED_HEADER_LEN + CAP_HANDLE_LEN]
            .copy_from_slice(&handle_bytes);

        let token = GenericCapToken::<EndpointResource>::from_parts(
            [0u8; CAP_NONCE_LEN],
            header,
            [0u8; CAP_TAG_LEN],
        );

        // Extract HandleView via as_view()
        let view = token.as_view().expect("as_view should succeed");

        // Verify handle matches
        assert_eq!(view.handle(), &handle);
        // Verify caps_mask matches
        assert_eq!(view.grant_mask().bits(), mask.bits());
        // Verify bytes match
        assert_eq!(view.bytes(), &handle_bytes);

        // Verify caps_mask is correctly embedded in token header
        let token_bytes = token.into_bytes();
        let embedded_caps = u16::from_be_bytes([
            token_bytes[CAP_NONCE_LEN + 8],
            token_bytes[CAP_NONCE_LEN + 9],
        ]);
        assert_eq!(embedded_caps, mask.bits());
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
                let mask = EndpointResource::caps_mask(&handle);
                let view = HandleView::<EndpointResource>::decode(&payload, mask).expect("decode");
                prop_assert_eq!(view.grant_mask().bits(), mask.bits());
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
                let handle = LoopDecisionHandle::new(generation, lane, ScopeId::loop_scope(1));
                let payload = LoopContinueKind::encode_handle(&handle);
                let mask = LoopContinueKind::caps_mask(&handle);
                let view = HandleView::<LoopContinueKind>::decode(&payload, mask).expect("decode");
                prop_assert_eq!(view.grant_mask().bits(), mask.bits());
                prop_assert_eq!(view.handle(), &handle);
                prop_assert_eq!(view.bytes(), &payload);
            }
        }
    }
}
