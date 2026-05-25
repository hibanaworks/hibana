//! CapMint 2.0 primitives for capability minting and validation.
//!
//! Hibana mints control tokens through const-first strategies baked into
//! `RoleProgram` and endpoint-owned local control send paths, with
//! rendezvous tables enforcing nonce-ledger side effects via
//! `Rendezvous::mint_cap()` and `Rendezvous::claim_cap()`.
//!
//! # Endpoint-Local Witnesses And Ledger Claims
//!
//! Endpoint-local control progression is witnessed by rendezvous-scoped brands
//! and epoch markers. Wire capability authority is separate: minted tokens are
//! claimed through the rendezvous-local nonce ledger below.
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
//! The approach keeps witness bookkeeping internal: the rendezvous retains the
//! brand token and application code never handles witness machinery directly.
//!
//! # Wire Format
//!
//! Capability tokens are 56 bytes on the wire:
//! ```text
//! [16B nonce | 40B descriptor header]
//! descriptor header = fixed control metadata plus resource-owned handle bytes
//! ```
//!
//! The default runtime is a trusted-domain nonce ledger, not a keyed verifier.
//! Claim authority comes from a nonce table entry minted by the same rendezvous
//! plus descriptor/header validation. Token bytes stop at the descriptor header;
//! `claim_cap()` does not authenticate any trailing extension.
//!
//! # Usage Pattern
//!
//! ## SessionCluster-driven endpoint minting
//!
//! ```rust,ignore
//! let controller = cluster
//!     .rendezvous(rv_id)
//!     .session(sid)
//!     .role(&CONTROLLER)
//!     .enter(hibana::integration::binding::NoBinding)?;
//! controller.flow::<CancelMsg>()?.send(()).await?;
//! ```
//!
//! ## Rendezvous validation
//!
//! ```rust,ignore
//! let (worker, token) = worker.recv::<CancelMsg>().await?;
//! rendezvous.claim_cap(&token)?;
//! ```
//!
//! ## Custom Resource Example
//!
//! ```rust,ignore
//! use core::cell::Cell;
//! use hibana::integration::cap::{CapError, GenericCapToken, ResourceKind};
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
//!         handle.id = 0;
//!     }
//! }
//!
//! fn round_trip(token: GenericCapToken<PageResource>) -> GenericCapToken<PageResource> {
//!     // Convert to bytes and back so the token can traverse message routes.
//!     let bytes = token.into_bytes();
//!     <GenericCapToken<PageResource> as hibana::integration::wire::WirePayload>::decode_payload(
//!         hibana::integration::wire::Payload::new(&bytes),
//!     )
//!     .unwrap()
//! }
//! ```

use core::marker::PhantomData;

mod epoch;
mod strategy;

pub use epoch::*;
pub(crate) use epoch::{EndpointEpoch, Owner};
pub use strategy::*;

/// Length of the nonce segment inside a capability token.
pub const CAP_NONCE_LEN: usize = 16;
/// Length of the header segment inside a capability token.
pub const CAP_HEADER_LEN: usize = 40;
/// Number of fixed bytes used by the descriptor-first control header codec.
///
/// Layout:
/// - version: 1
/// - sid: 4
/// - lane: 1
/// - role: 1
/// - tag: 1
/// - op: 1
/// - path: 1
/// - shot: 1
/// - scope_kind: 1
/// - flags: 1
/// - scope_id: 2
/// - epoch: 2
pub const CAP_CONTROL_HEADER_FIXED_LEN: usize = 17;
/// Number of bytes available for resource-specific handle encoding.
pub const CAP_HANDLE_LEN: usize = CAP_HEADER_LEN - CAP_CONTROL_HEADER_FIXED_LEN;
/// Total length of a capability token on the wire.
pub const CAP_TOKEN_LEN: usize = CAP_NONCE_LEN + CAP_HEADER_LEN;
use crate::control::types::Lane;
use crate::control::types::SessionId;
use crate::global::const_dsl::{ControlScopeKind, ScopeId};
use crate::transport::wire::{CodecError, Payload, WireEncode, WirePayload};

// ============================================================================
// Generic capability abstraction
// ============================================================================

/// Resource taxonomy for capabilities.
///
/// Each `ResourceKind` supplies a handle type that is encoded into the opaque
/// payload section of the capability header. The fixed descriptor prefix stores
/// session, routing, and control metadata; the remaining [`CAP_HANDLE_LEN`]
/// bytes are entirely owned by the resource kind for encoding operands.
pub trait ResourceKind {
    /// Handle associated with this capability.
    type Handle;

    /// Capability tag.
    ///
    /// Control resource kinds must not use `0`. The zero tag is reserved
    /// internally for endpoint capabilities and the non-control `()` sentinel.
    const TAG: u8;

    /// Human-readable name used for observability.
    const NAME: &'static str;

    /// Encode the handle into the resource payload area of the header.
    fn encode_handle(handle: &Self::Handle) -> [u8; CAP_HANDLE_LEN];

    /// Decode the handle from the resource payload area of the header.
    ///
    /// Decoding must be deterministic, side-effect-free, and non-authoritative.
    /// Returning `Ok` only constructs a local handle value; it must not claim,
    /// consume, mutate, or observe rendezvous authority.
    fn decode_handle(data: [u8; CAP_HANDLE_LEN]) -> Result<Self::Handle, CapError>;

    /// Zeroize the handle prior to dropping it.
    fn zeroize(handle: &mut Self::Handle);
}

/// Resource kinds that represent control-plane capabilities.
pub trait ControlResourceKind: ResourceKind {
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

// ============================================================================
// Capability token runtime encoding
// ============================================================================

/// Capability shot semantics embedded in the token wire/runtime encoding.
///
/// `CapShot` records how many times a concrete token may be claimed:
/// - `One`: Single-use (affine). Claiming the token consumes it immediately.
/// - `Many`: Reusable. Claiming it does not mark the ledger entry consumed.
///
/// Control resource kinds choose this through [`ControlResourceKind::SHOT`].
/// `CapShot` is the runtime encoding of that descriptor decision inside a
/// minted token. Any additional reuse discipline belongs to the resource
/// owner's descriptor contract.
///
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapShot {
    /// Single-use capability (affine linearity).
    One = 0,
    /// Reusable capability that does not consume its ledger entry on claim.
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

/// Built-in control-plane operation owned by hibana core.
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
    const KNOWN_FLAGS_MASK: u8 = 0b0000_0001;

    #[inline]
    pub(crate) const fn new(
        sid: SessionId,
        lane: Lane,
        role: u8,
        tag: u8,
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
        out[8] = self.op.as_u8();
        out[9] = self.path.as_u8();
        out[10] = self.shot.as_u8();
        out[11] = self.scope_kind as u8;
        out[12] = self.flags;
        out[13..15].copy_from_slice(&self.scope_id.to_be_bytes());
        out[15..17].copy_from_slice(&self.epoch.to_be_bytes());
        out[17..].copy_from_slice(&self.handle);
    }

    #[inline]
    pub fn decode(raw: [u8; CAP_HEADER_LEN]) -> Result<Self, CapError> {
        if raw[0] != 1 {
            return Err(CapError::Mismatch);
        }
        let op = ControlOp::from_u8(raw[8]).ok_or(CapError::Mismatch)?;
        let path = ControlPath::from_u8(raw[9]).ok_or(CapError::Mismatch)?;
        let shot = CapShot::from_u8(raw[10]).ok_or(CapError::Mismatch)?;
        let scope_kind = ControlScopeKind::from_u8(raw[11]).ok_or(CapError::Mismatch)?;
        if raw[12] & !Self::KNOWN_FLAGS_MASK != 0 {
            return Err(CapError::Mismatch);
        }
        let mut handle = [0u8; CAP_HEADER_LEN - CAP_CONTROL_HEADER_FIXED_LEN];
        handle.copy_from_slice(&raw[17..]);
        Ok(Self {
            version: raw[0],
            sid: SessionId::new(u32::from_be_bytes([raw[1], raw[2], raw[3], raw[4]])),
            lane: Lane::new(u32::from(raw[5])),
            role: raw[6],
            tag: raw[7],
            op,
            path,
            shot,
            scope_kind,
            flags: raw[12],
            scope_id: u16::from_be_bytes([raw[13], raw[14]]),
            epoch: u16::from_be_bytes([raw[15], raw[16]]),
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
    #[cfg(test)]
    pub(crate) const fn handle(&self) -> &[u8; CAP_HEADER_LEN - CAP_CONTROL_HEADER_FIXED_LEN] {
        &self.handle
    }
}

#[inline]
pub(crate) const fn is_canonical_endpoint_header(header: CapHeader) -> bool {
    header.tag() == EndpointResource::TAG
        && matches!(header.op(), ControlOp::Fence)
        && matches!(header.path(), ControlPath::Local)
        && matches!(header.shot(), CapShot::One)
        && matches!(header.scope_kind(), ControlScopeKind::None)
        && header.flags() == 0
        && header.scope_id() == 0
        && header.epoch() == 0
}

#[inline]
fn decode_canonical_endpoint_identity(
    token: &GenericCapToken<EndpointResource>,
) -> Result<(CapHeader, EndpointHandle), CapError> {
    let header = token.control_header()?;
    if !is_canonical_endpoint_header(header) {
        return Err(CapError::Mismatch);
    }

    let mut handle =
        EndpointResource::decode_handle(token.handle_bytes()).map_err(|_| CapError::Mismatch)?;
    let matches_header =
        handle.sid == header.sid() && handle.lane == header.lane() && handle.role == header.role();
    let matches_encoding = EndpointResource::encode_handle(&handle) == token.handle_bytes();
    if !matches_header || !matches_encoding {
        EndpointResource::zeroize(&mut handle);
        return Err(CapError::Mismatch);
    }

    Ok((header, handle))
}

#[inline]
const fn scope_from_header(header: CapHeader) -> Option<ScopeId> {
    match header.scope_kind() {
        ControlScopeKind::Route => Some(ScopeId::route(header.scope_id())),
        ControlScopeKind::Loop => Some(ScopeId::loop_scope(header.scope_id())),
        _ => None,
    }
}

/// Typed view over a capability handle exposed to an external policy VM.
///
/// The view carries the original resource payload together with the structured
/// scope metadata recovered from the descriptor-first control header.
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
/// Discriminated variants preserve debugging information without implying an
/// external authentication path. `UnknownToken` identifies absent nonce-ledger
/// entries, `Mismatch` indicates fixed descriptor metadata or resource-owned
/// handle bytes did not match the rendezvous-local nonce ledger entry, and
/// `TableFull` tracks capacity exhaustion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapError {
    /// Token not found in capability table.
    UnknownToken,
    /// Session ID or lane does not exist in local Rendezvous.
    WrongSessionOrLane,
    /// One-shot token already consumed.
    Exhausted,
    /// Capability table reached its configured capacity.
    ///
    /// This can happen if too many capabilities are minted without being claimed,
    /// or if Many-shot capabilities accumulate over time.
    TableFull,
    /// Token descriptor metadata or resource-owned handle mismatch.
    ///
    /// This indicates the token was found in CapTable (nonce matched) but
    /// one or more fixed descriptor fields or handle bytes didn't match the
    /// rendezvous-local ledger entry. This is distinct from `UnknownToken`
    /// (nonce not found) and helps diagnose configuration errors.
    Mismatch,
}

/// Opaque capability-token payload carried by control messages.
///
/// Protocol authors name this type in a `g::Msg<..., GenericCapToken<K>, K>`
/// payload. Descriptor metadata and token header details live under the
/// integration capability metadata bucket; ordinary choreography code should only
/// pass the token as an opaque payload.
#[repr(C)]
#[derive(Debug, PartialEq, Eq)]
pub struct GenericCapToken<K: ResourceKind> {
    bytes: [u8; CAP_TOKEN_LEN],
    _marker: PhantomData<K>,
}

impl<K: ResourceKind> Copy for GenericCapToken<K> {}

impl<K: ResourceKind> Clone for GenericCapToken<K> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<K: ResourceKind> GenericCapToken<K> {
    pub const AUTO: Self = Self {
        bytes: [0u8; CAP_TOKEN_LEN],
        _marker: PhantomData,
    };

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

    pub(crate) fn nonce(&self) -> [u8; CAP_NONCE_LEN] {
        let mut nonce = [0u8; CAP_NONCE_LEN];
        nonce.copy_from_slice(&self.bytes[0..CAP_NONCE_LEN]);
        nonce
    }

    fn raw_header(&self) -> [u8; CAP_HEADER_LEN] {
        let mut header = [0u8; CAP_HEADER_LEN];
        header.copy_from_slice(self.header_slice());
        header
    }

    #[inline]
    pub(crate) fn control_header(&self) -> Result<CapHeader, CapError> {
        CapHeader::decode(self.raw_header())
    }

    #[inline]
    fn typed_header(&self) -> Result<CapHeader, CapError> {
        let header = self.control_header()?;
        if header.tag() != K::TAG {
            return Err(CapError::Mismatch);
        }
        Ok(header)
    }

    /// Extract the structured scope identifier encoded in the handle, if any.
    ///
    /// Header, tag, and handle decode failures are returned instead of being
    /// collapsed into `None`, which is reserved for valid tokens without
    /// structured scope metadata.
    pub fn scope(&self) -> Result<Option<ScopeId>, CapError> {
        self.as_view().map(|view| view.scope())
    }

    pub(crate) fn handle_bytes(&self) -> [u8; CAP_HANDLE_LEN] {
        *self.handle_bytes_ref()
    }

    #[inline]
    pub(crate) fn is_auto(&self) -> bool {
        self.bytes == [0u8; CAP_TOKEN_LEN]
    }

    /// Get a reference to the handle bytes within the token.
    ///
    /// This is a zero-copy operation that returns a slice reference
    /// to the handle payload embedded in the token header.
    #[inline(always)]
    pub(crate) fn handle_bytes_ref(&self) -> &[u8; CAP_HANDLE_LEN] {
        self.header_slice()
            [CAP_CONTROL_HEADER_FIXED_LEN..CAP_CONTROL_HEADER_FIXED_LEN + CAP_HANDLE_LEN]
            .try_into()
            .expect("CAP_HANDLE_LEN is compile-time constant")
    }

    pub fn decode_handle(&self) -> Result<K::Handle, CapError> {
        self.typed_header()?;
        K::decode_handle(self.handle_bytes())
    }

    /// Extract a HandleView from this token.
    ///
    /// This provides zero-copy access to the embedded handle and its capabilities.
    /// The HandleView lifetime is bounded by the token's lifetime.
    ///
    /// # Type Safety
    ///
    /// The type parameter selects the expected [`ResourceKind`]; the wire header
    /// tag is validated before exposing the typed view. The returned
    /// `HandleView` cannot outlive the token.
    ///
    /// # Example
    ///
    /// ```ignore
    /// fn inspect(token: GenericCapToken<LoopContinueKind>) -> Result<(), CapError> {
    ///     let view = token.as_view()?;
    ///     let scope = view.scope();
    ///     let _ = scope;
    ///     Ok(())
    /// }
    /// ```
    pub fn as_view(&self) -> Result<HandleView<'_, K>, CapError> {
        let header = self.typed_header()?;
        HandleView::decode(self.handle_bytes_ref(), scope_from_header(header))
    }
}

impl GenericCapToken<EndpointResource> {
    #[cfg(test)]
    #[inline]
    pub(crate) fn endpoint_header(&self) -> Result<CapHeader, CapError> {
        let (header, mut handle) = decode_canonical_endpoint_identity(self)?;
        EndpointResource::zeroize(&mut handle);
        Ok(header)
    }

    #[inline]
    pub(crate) fn endpoint_identity(&self) -> Result<EndpointHandle, CapError> {
        decode_canonical_endpoint_identity(self).map(|(_, handle)| handle)
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

    fn validate_payload(input: Payload<'_>) -> Result<(), CodecError> {
        let bytes_in = input.as_bytes();
        if bytes_in.len() < CAP_TOKEN_LEN {
            return Err(CodecError::Truncated);
        }
        if bytes_in.len() != CAP_TOKEN_LEN {
            return Err(CodecError::Invalid("trailing bytes after GenericCapToken"));
        }
        Ok(())
    }

    fn decode_validated_payload<'a>(input: Payload<'a>) -> Self::Decoded<'a> {
        let bytes_in = input.as_bytes();
        let mut bytes = [0u8; CAP_TOKEN_LEN];
        bytes.copy_from_slice(bytes_in);
        Self {
            bytes,
            _marker: PhantomData,
        }
    }
}

// ============================================================================
// Default implementation (trusted-domain nonce ledger)
// ============================================================================
//
// The default strategy is deliberately non-cryptographic. It is used when
// capability tokens stay inside a rendezvous-owned trust domain and claim
// authority is the nonce ledger, not a keyed authenticator. Cross-domain
// authentication belongs in a protocol/integration layer that can model and
// verify that trust boundary explicitly.

#[cfg(test)]
#[path = "mint_tests.rs"]
mod tests;
