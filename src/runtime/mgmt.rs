//! Runtime management session — typestate-first, macro-free, allocation-free.
//!
//! `Manager<State, const SLOTS>` models the typestate automaton that drives the
//! EPF VM management session. Bytecode load/commit/activate/rollback operations
//! are only available on the corresponding state variants, and the chunk stream
//! is statically checked through `ChunkCursor` using stable Rust. Memory backing
//! is provided by the rendezvous-resident `SlotArena`/`HostSlots` and is passed
//! in by the caller when constructing the manager.

/// Session choreography for EPF management.
pub mod session;

use core::{fmt, marker::PhantomData};

use crate::{
    control::{
        cap::ResourceKind,
        cap::resource_kinds::{
            LoadBeginKind, LoadCommitKind, PolicyActivateKind, PolicyAnnotateKind, PolicyLoadKind,
            PolicyRevertKind,
        },
        lease::{
            ControlAutomaton, ControlStep, RendezvousLease, SlotSpec,
            bundle::LeaseBundleFacet,
            graph::{LeaseGraph, LeaseSpec},
            planner::{FacetSlots, LeaseFacetNeeds, LeaseSpecFacetNeeds},
        },
        types::RendezvousId,
    },
    epf::{
        Slot,
        host::{HostError, HostSlots, Machine},
        loader::{ImageLoader, LoaderError},
        verifier::{Header, VerifyError},
    },
    observe::{
        self, PolicyCommit, PolicyEvent, PolicyEventKind, PolicyRollback, policy_commit,
        policy_rollback, push,
    },
    rendezvous::{SLOT_COUNT as RENDEZVOUS_SLOT_COUNT, SlotStorage, slot_index},
    transport::wire::{CodecError, WireDecode, WireEncode},
};

const SLOT_COUNT: usize = RENDEZVOUS_SLOT_COUNT;

const MGMT_RESOURCE_NEEDS: LeaseFacetNeeds = FacetSlots::NEEDS;

/// Facet/tap profile for the management session automaton.
pub struct MgmtFacetProfile;

impl MgmtFacetProfile {
    const RESOURCE_TAGS: [u8; 6] = [
        PolicyLoadKind::TAG,
        PolicyActivateKind::TAG,
        PolicyRevertKind::TAG,
        PolicyAnnotateKind::TAG,
        LoadBeginKind::TAG,
        LoadCommitKind::TAG,
    ];

    const POLICY_EVENT_IDS: [u16; 2] = [policy_commit(), policy_rollback()];

    /// Facet requirements advertised by the management automaton.
    pub const FACET_NEEDS: LeaseFacetNeeds = FacetSlots::NEEDS;
    /// Facet requirements derived from resource tags.
    pub const RESOURCE_FACETS: LeaseFacetNeeds = MGMT_RESOURCE_NEEDS;

    #[inline(always)]
    pub const fn facet_needs() -> LeaseFacetNeeds {
        Self::FACET_NEEDS
    }

    #[inline(always)]
    pub const fn resource_facets() -> LeaseFacetNeeds {
        Self::RESOURCE_FACETS
    }

    #[inline(always)]
    pub const fn resource_tags() -> [u8; 6] {
        Self::RESOURCE_TAGS
    }

    #[inline(always)]
    pub const fn policy_event_ids() -> [u16; 2] {
        Self::POLICY_EVENT_IDS
    }

    #[inline(always)]
    pub const fn supports_policy_id(id: u16) -> bool {
        id == Self::POLICY_EVENT_IDS[0] || id == Self::POLICY_EVENT_IDS[1]
    }

    #[inline(always)]
    pub const fn supports_policy_kind(kind: PolicyEventKind) -> bool {
        matches!(kind, PolicyEventKind::Commit | PolicyEventKind::Rollback)
    }
}

impl LeaseSpecFacetNeeds for MgmtFacetProfile {
    const FACET_NEEDS: LeaseFacetNeeds = MgmtFacetProfile::FACET_NEEDS;
}

const MGMT_FACET_NEEDS: LeaseFacetNeeds = MgmtFacetProfile::FACET_NEEDS;
pub const LOAD_CHUNK_MAX: usize = 1024;

/// Errors that can occur during the management session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MgmtError {
    InvalidSlot(u8),
    InvalidTransition,
    ChunkOutOfOrder { expected: u32, got: u32 },
    ChunkTooLarge { remaining: u32, provided: u32 },
    LoaderNotFinalised,
    NoStagedImage,
    NoActiveImage,
    NoPreviousImage,
    CapabilityMismatch,
    ObserveUnavailable,
    HostInstallFailed,
    HostUninstallFailed,
    StreamEnded,
}

impl From<LoaderError> for MgmtError {
    fn from(err: LoaderError) -> Self {
        match err {
            LoaderError::AlreadyLoading => MgmtError::InvalidTransition,
            LoaderError::NotLoading => MgmtError::InvalidTransition,
            LoaderError::CodeTooLarge { declared } => MgmtError::ChunkTooLarge {
                remaining: 0,
                provided: declared as u32,
            },
            LoaderError::UnexpectedOffset { expected, got } => {
                MgmtError::ChunkOutOfOrder { expected, got }
            }
            LoaderError::ChunkTooLarge {
                remaining,
                provided,
            } => MgmtError::ChunkTooLarge {
                remaining,
                provided,
            },
            LoaderError::HashMismatch { .. } => MgmtError::LoaderNotFinalised,
            LoaderError::Verify(err) => MgmtError::from(err),
        }
    }
}

impl From<VerifyError> for MgmtError {
    fn from(_: VerifyError) -> Self {
        MgmtError::LoaderNotFinalised
    }
}

impl From<HostError> for MgmtError {
    fn from(err: HostError) -> Self {
        match err {
            HostError::SlotOccupied => MgmtError::InvalidTransition,
            HostError::SlotEmpty => MgmtError::HostUninstallFailed,
            HostError::InvalidFuel => MgmtError::HostInstallFailed,
            HostError::ScratchTooSmall { .. } => MgmtError::HostInstallFailed,
            HostError::ScratchTooLarge { .. } => MgmtError::HostInstallFailed,
        }
    }
}

/// Typical management protocol replies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reply {
    Activated(TransitionReport),
    Reverted(TransitionReport),
    Stats {
        stats: StatsResp,
        staged_version: Option<u32>,
    },
}

/// Commands understood by the management session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Activate { slot: Slot },
    Revert { slot: Slot },
    Stats { slot: Slot },
}

/// Slot-level metrics.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StatsResp {
    pub traps: u32,
    pub aborts: u32,
    pub fuel_used: u32,
    pub active_version: u32,
}

/// Observation snapshot collected during transitions.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TransitionReport {
    pub version: u32,
    pub policy: PolicySnapshot,
}

/// Policy event statistics harvested from tap events.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PolicySnapshot {
    pub aborts: u32,
    pub traps: u32,
    pub annotations: u32,
    pub effects: u32,
    pub effects_ok: u32,
    pub commits: u32,
    pub rollbacks: u32,
    pub last_commit: Option<u32>,
    pub last_rollback: Option<u32>,
}

impl PolicySnapshot {
    fn record(&mut self, event: PolicyEvent) {
        match event.kind {
            PolicyEventKind::Abort => self.aborts = self.aborts.saturating_add(1),
            PolicyEventKind::Trap => self.traps = self.traps.saturating_add(1),
            PolicyEventKind::Annotate => self.annotations = self.annotations.saturating_add(1),
            PolicyEventKind::Effect => self.effects = self.effects.saturating_add(1),
            PolicyEventKind::EffectOk => self.effects_ok = self.effects_ok.saturating_add(1),
            PolicyEventKind::Commit => {
                self.commits = self.commits.saturating_add(1);
                self.last_commit = Some(event.arg1);
            }
            PolicyEventKind::Rollback => {
                self.rollbacks = self.rollbacks.saturating_add(1);
                self.last_rollback = Some(event.arg1);
            }
        }
    }
}

const fn empty_policy_snapshot() -> PolicySnapshot {
    PolicySnapshot {
        aborts: 0,
        traps: 0,
        annotations: 0,
        effects: 0,
        effects_ok: 0,
        commits: 0,
        rollbacks: 0,
        last_commit: None,
        last_rollback: None,
    }
}

/// Payload carried by the `LoadBegin` message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoadBegin {
    pub slot: u8,
    pub code_len: u32,
    pub fuel_max: u16,
    pub mem_len: u16,
    pub hash: u32,
}

/// Payload for `LoadChunk`; the chunk body lives in a fixed-size buffer.
///
/// The `is_last` flag indicates whether this is the final chunk in the stream.
/// This allows the receiver (Cluster) to determine loop termination without
/// relying on route control messages, keeping CanonicalControl purely local.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadChunk {
    pub offset: u32,
    pub len: u16,
    /// Indicates this is the final chunk; Cluster breaks the recv loop.
    pub is_last: bool,
    pub bytes: [u8; LOAD_CHUNK_MAX],
}

/// `Activate` message payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Activate {
    pub slot: u8,
}

/// `Revert` message payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Revert {
    pub slot: u8,
}

/// `Stats` request payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StatsReq {
    pub slot: u8,
}

/// Subscribe request for streaming observe (single-event streaming).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SubscribeReq {
    /// Reserved for future use (filter flags, etc.)
    pub flags: u16,
}

impl WireEncode for SubscribeReq {
    fn encoded_len(&self) -> Option<usize> {
        Some(2)
    }

    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.len() < 2 {
            return Err(CodecError::Truncated);
        }
        out[0..2].copy_from_slice(&self.flags.to_be_bytes());
        Ok(2)
    }
}

impl<'a> WireDecode<'a> for SubscribeReq {
    fn decode_from(input: &'a [u8]) -> Result<Self, CodecError> {
        if input.len() < 2 {
            return Err(CodecError::Truncated);
        }
        let flags = u16::from_be_bytes([input[0], input[1]]);
        Ok(SubscribeReq { flags })
    }
}

// TapEvent wire encoding: 20 bytes
// ts(4) + id(2) + causal_key(2) + arg0(4) + arg1(4) + arg2(4)
const TAP_EVENT_WIRE_LEN: usize = 20;

impl WireEncode for observe::TapEvent {
    fn encoded_len(&self) -> Option<usize> {
        Some(TAP_EVENT_WIRE_LEN)
    }

    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.len() < TAP_EVENT_WIRE_LEN {
            return Err(CodecError::Truncated);
        }
        out[0..4].copy_from_slice(&self.ts.to_be_bytes());
        out[4..6].copy_from_slice(&self.id.to_be_bytes());
        out[6..8].copy_from_slice(&self.causal_key.to_be_bytes());
        out[8..12].copy_from_slice(&self.arg0.to_be_bytes());
        out[12..16].copy_from_slice(&self.arg1.to_be_bytes());
        out[16..20].copy_from_slice(&self.arg2.to_be_bytes());
        Ok(TAP_EVENT_WIRE_LEN)
    }
}

impl<'a> WireDecode<'a> for observe::TapEvent {
    fn decode_from(input: &'a [u8]) -> Result<Self, CodecError> {
        if input.len() < TAP_EVENT_WIRE_LEN {
            return Err(CodecError::Truncated);
        }
        Ok(observe::TapEvent {
            ts: u32::from_be_bytes([input[0], input[1], input[2], input[3]]),
            id: u16::from_be_bytes([input[4], input[5]]),
            causal_key: u16::from_be_bytes([input[6], input[7]]),
            arg0: u32::from_be_bytes([input[8], input[9], input[10], input[11]]),
            arg1: u32::from_be_bytes([input[12], input[13], input[14], input[15]]),
            arg2: u32::from_be_bytes([input[16], input[17], input[18], input[19]]),
        })
    }
}

// TapBatch wire encoding: 5 + (count × 20) bytes
// count(1) + lost_events(4) + events(count × 20)
const TAP_BATCH_HEADER_LEN: usize = 5;

impl WireEncode for observe::TapBatch {
    fn encoded_len(&self) -> Option<usize> {
        Some(TAP_BATCH_HEADER_LEN + self.len() * TAP_EVENT_WIRE_LEN)
    }

    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        let total_len = TAP_BATCH_HEADER_LEN + self.len() * TAP_EVENT_WIRE_LEN;
        if out.len() < total_len {
            return Err(CodecError::Truncated);
        }

        out[0] = self.len() as u8;
        out[1..5].copy_from_slice(&self.lost_events().to_be_bytes());

        let mut offset = TAP_BATCH_HEADER_LEN;
        for event in self.iter() {
            event.encode_into(&mut out[offset..])?;
            offset += TAP_EVENT_WIRE_LEN;
        }

        Ok(total_len)
    }
}

impl<'a> WireDecode<'a> for observe::TapBatch {
    fn decode_from(input: &'a [u8]) -> Result<Self, CodecError> {
        if input.len() < TAP_BATCH_HEADER_LEN {
            return Err(CodecError::Truncated);
        }

        let count = input[0] as usize;
        if count > observe::TAP_BATCH_MAX_EVENTS {
            return Err(CodecError::Invalid("batch count exceeds maximum"));
        }

        let lost_events = u32::from_be_bytes([input[1], input[2], input[3], input[4]]);
        let expected_len = TAP_BATCH_HEADER_LEN + count * TAP_EVENT_WIRE_LEN;
        if input.len() < expected_len {
            return Err(CodecError::Truncated);
        }

        let mut batch = observe::TapBatch::empty();
        batch.set_lost_events(lost_events);

        let mut offset = TAP_BATCH_HEADER_LEN;
        for _ in 0..count {
            let event = observe::TapEvent::decode_from(&input[offset..])?;
            batch.push(event);
            offset += TAP_EVENT_WIRE_LEN;
        }

        Ok(batch)
    }
}

impl WireEncode for LoadBegin {
    fn encoded_len(&self) -> Option<usize> {
        Some(13)
    }

    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.len() < 13 {
            return Err(CodecError::Truncated);
        }
        out[0] = self.slot;
        out[1..5].copy_from_slice(&self.code_len.to_be_bytes());
        out[5..7].copy_from_slice(&self.fuel_max.to_be_bytes());
        out[7..9].copy_from_slice(&self.mem_len.to_be_bytes());
        out[9..13].copy_from_slice(&self.hash.to_be_bytes());
        Ok(13)
    }
}

impl<'a> WireDecode<'a> for LoadBegin {
    fn decode_from(input: &'a [u8]) -> Result<Self, CodecError> {
        if input.len() < 13 {
            return Err(CodecError::Truncated);
        }
        let slot = input[0];
        let code_len = u32::from_be_bytes([input[1], input[2], input[3], input[4]]);
        let fuel_max = u16::from_be_bytes([input[5], input[6]]);
        let mem_len = u16::from_be_bytes([input[7], input[8]]);
        let hash = u32::from_be_bytes([input[9], input[10], input[11], input[12]]);
        Ok(LoadBegin {
            slot,
            code_len,
            fuel_max,
            mem_len,
            hash,
        })
    }
}

impl WireEncode for LoadChunk {
    fn encoded_len(&self) -> Option<usize> {
        // 4 (offset) + 2 (len) + 1 (is_last) + payload
        Some(7 + self.len as usize)
    }

    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        let len = self.len as usize;
        if len > LOAD_CHUNK_MAX {
            return Err(CodecError::Invalid("chunk length exceeds LOAD_CHUNK_MAX"));
        }
        let total = 7 + len;
        if out.len() < total {
            return Err(CodecError::Truncated);
        }
        out[..4].copy_from_slice(&self.offset.to_be_bytes());
        out[4..6].copy_from_slice(&self.len.to_be_bytes());
        out[6] = if self.is_last { 1 } else { 0 };
        out[7..total].copy_from_slice(&self.bytes[..len]);
        Ok(total)
    }
}

impl<'a> WireDecode<'a> for LoadChunk {
    fn decode_from(input: &'a [u8]) -> Result<Self, CodecError> {
        if input.len() < 7 {
            return Err(CodecError::Truncated);
        }
        let offset = u32::from_be_bytes([input[0], input[1], input[2], input[3]]);
        let len = u16::from_be_bytes([input[4], input[5]]);
        let is_last = input[6] != 0;
        let len_usize = len as usize;
        if len_usize > LOAD_CHUNK_MAX {
            return Err(CodecError::Invalid("chunk length exceeds LOAD_CHUNK_MAX"));
        }
        if input.len() < 7 + len_usize {
            return Err(CodecError::Truncated);
        }
        let mut bytes = [0u8; LOAD_CHUNK_MAX];
        bytes[..len_usize].copy_from_slice(&input[7..7 + len_usize]);
        Ok(LoadChunk {
            offset,
            len,
            is_last,
            bytes,
        })
    }
}

impl WireEncode for Activate {
    fn encoded_len(&self) -> Option<usize> {
        Some(1)
    }

    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.is_empty() {
            return Err(CodecError::Truncated);
        }
        out[0] = self.slot;
        Ok(1)
    }
}

impl<'a> WireDecode<'a> for Activate {
    fn decode_from(input: &'a [u8]) -> Result<Self, CodecError> {
        if input.is_empty() {
            return Err(CodecError::Truncated);
        }
        Ok(Activate { slot: input[0] })
    }
}

impl WireEncode for Revert {
    fn encoded_len(&self) -> Option<usize> {
        Some(1)
    }

    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.is_empty() {
            return Err(CodecError::Truncated);
        }
        out[0] = self.slot;
        Ok(1)
    }
}

impl<'a> WireDecode<'a> for Revert {
    fn decode_from(input: &'a [u8]) -> Result<Self, CodecError> {
        if input.is_empty() {
            return Err(CodecError::Truncated);
        }
        Ok(Revert { slot: input[0] })
    }
}

impl WireEncode for StatsReq {
    fn encoded_len(&self) -> Option<usize> {
        Some(1)
    }

    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.is_empty() {
            return Err(CodecError::Truncated);
        }
        out[0] = self.slot;
        Ok(1)
    }
}

impl<'a> WireDecode<'a> for StatsReq {
    fn decode_from(input: &'a [u8]) -> Result<Self, CodecError> {
        if input.is_empty() {
            return Err(CodecError::Truncated);
        }
        Ok(StatsReq { slot: input[0] })
    }
}

const fn ensure_chunk_bounds(total: usize, written: usize, len: usize) -> usize {
    if len > LOAD_CHUNK_MAX {
        panic!("chunk length exceeds LOAD_CHUNK_MAX");
    }
    let next = written + len;
    if next > total {
        panic!("chunk sequence exceeds TOTAL");
    }
    next
}

const fn ensure_remaining(total: usize, written: usize) {
    if written >= total {
        panic!("chunk stream already complete");
    }
}

const fn ensure_complete(total: usize, written: usize) {
    if written != total {
        panic!("chunk stream not complete");
    }
}

/// Type-level cursor that tracks progress through the chunk stream.
pub trait ChunkCursor<const TOTAL: usize>: Sized {
    const WRITTEN: usize;
    type Append<const LEN: usize>: ChunkCursor<TOTAL>;
}

pub struct CursorNil;

impl<const TOTAL: usize> ChunkCursor<TOTAL> for CursorNil {
    const WRITTEN: usize = 0;
    type Append<const LEN: usize> = CursorCons<LEN, Self, TOTAL>;
}

pub struct CursorCons<const LEN: usize, Tail, const TOTAL: usize>
where
    Tail: ChunkCursor<TOTAL>,
{
    _tail: PhantomData<Tail>,
}

impl<const TOTAL: usize, const LEN: usize, Tail> ChunkCursor<TOTAL> for CursorCons<LEN, Tail, TOTAL>
where
    Tail: ChunkCursor<TOTAL>,
{
    const WRITTEN: usize = ensure_chunk_bounds(TOTAL, Tail::WRITTEN, LEN);
    type Append<const NEXT: usize> = CursorCons<NEXT, Self, TOTAL>;
}

impl<const LEN: usize, Tail, const TOTAL: usize> Default for CursorCons<LEN, Tail, TOTAL>
where
    Tail: ChunkCursor<TOTAL>,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<const LEN: usize, Tail, const TOTAL: usize> CursorCons<LEN, Tail, TOTAL>
where
    Tail: ChunkCursor<TOTAL>,
{
    pub const fn new() -> Self {
        Self { _tail: PhantomData }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cold;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AwaitBegin;
pub struct Streaming<const TOTAL: usize, Cursor>
where
    Cursor: ChunkCursor<TOTAL>,
{
    _cursor: PhantomData<Cursor>,
}

impl<const TOTAL: usize, Cursor> Default for Streaming<TOTAL, Cursor>
where
    Cursor: ChunkCursor<TOTAL>,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<const TOTAL: usize, Cursor> Streaming<TOTAL, Cursor>
where
    Cursor: ChunkCursor<TOTAL>,
{
    pub const fn new() -> Self {
        Self {
            _cursor: PhantomData,
        }
    }
}

impl<const TOTAL: usize, Cursor> Clone for Streaming<TOTAL, Cursor>
where
    Cursor: ChunkCursor<TOTAL>,
{
    fn clone(&self) -> Self {
        *self
    }
}
impl<const TOTAL: usize, Cursor> Copy for Streaming<TOTAL, Cursor> where Cursor: ChunkCursor<TOTAL> {}

impl<const TOTAL: usize, Cursor> fmt::Debug for Streaming<TOTAL, Cursor>
where
    Cursor: ChunkCursor<TOTAL>,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Streaming")
            .field("total", &TOTAL)
            .field("written", &Cursor::WRITTEN)
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Staged;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Active;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RollbackPending;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Quiescent;

pub trait ManagerState {}
impl ManagerState for Cold {}
impl ManagerState for AwaitBegin {}
impl<const TOTAL: usize, Cursor> ManagerState for Streaming<TOTAL, Cursor> where
    Cursor: ChunkCursor<TOTAL>
{
}
impl ManagerState for Staged {}
impl ManagerState for Active {}
impl ManagerState for RollbackPending {}
impl ManagerState for Quiescent {}

/// Per-slot state.
struct SlotState {
    loader: ImageLoader,
    inventory: SlotInventory,
    pending: PendingVersion,
    version_counter: u32,
    policy_cursor: usize,
    last_policy: PolicySnapshot,
}

impl SlotState {
    fn new(initial_cursor: usize) -> Self {
        Self {
            loader: ImageLoader::new(),
            inventory: SlotInventory::new(),
            pending: PendingVersion::default(),
            version_counter: 0,
            policy_cursor: initial_cursor,
            last_policy: PolicySnapshot::default(),
        }
    }
}

/// Typestate-driven management session that stages, activates, and rolls back
/// EPF policy images.
pub struct Manager<State, const SLOTS: usize>
where
    State: ManagerState,
{
    slot_states: [SlotState; SLOT_COUNT],
    _state: PhantomData<State>,
    timestamp: u32,
    policy: [PolicySnapshot; SLOTS],
}

impl<const SLOTS: usize> Default for Manager<Cold, SLOTS> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const SLOTS: usize> Manager<Cold, SLOTS> {
    /// Construct a management session manager in the initial `Cold` state.
    ///
    /// The manager snapshots the current tap cursor so subsequent policy
    /// metrics can be correlated with the tap stream.
    pub fn new() -> Self {
        let initial_cursor = observe::head().unwrap_or(0);
        Self {
            slot_states: [
                SlotState::new(initial_cursor),
                SlotState::new(initial_cursor),
                SlotState::new(initial_cursor),
                SlotState::new(initial_cursor),
                SlotState::new(initial_cursor),
            ],
            _state: PhantomData,
            timestamp: 0,
            policy: [empty_policy_snapshot(); SLOTS],
        }
    }

    /// Transition into `AwaitBegin`, enabling the load handshake.
    pub fn into_await_begin(self) -> Manager<AwaitBegin, SLOTS> {
        let ts = self.timestamp;
        self.transition(AwaitBegin, ts)
    }
}

impl<const SLOTS: usize> Manager<AwaitBegin, SLOTS> {
    /// Accept a `LoadBegin` header and start streaming bytecode chunks.
    pub fn into_streaming<const TOTAL: usize>(
        self,
        _slot: Slot,
        _header: Header,
    ) -> Manager<Streaming<TOTAL, CursorNil>, SLOTS> {
        let ts = self.timestamp.saturating_add(1);
        self.transition(Streaming::<TOTAL, CursorNil>::new(), ts)
    }
}

impl<const SLOTS: usize, const TOTAL: usize, Cursor> Manager<Streaming<TOTAL, Cursor>, SLOTS>
where
    Cursor: ChunkCursor<TOTAL>,
{
    /// Append a chunk to the in-flight bytecode image.
    pub fn append_chunk<const LEN: usize>(
        self,
        _chunk: &[u8; LEN],
    ) -> Manager<Streaming<TOTAL, <Cursor as ChunkCursor<TOTAL>>::Append<LEN>>, SLOTS> {
        ensure_remaining(TOTAL, Cursor::WRITTEN);
        let ts = self.timestamp.saturating_add(1);
        self.transition(
            Streaming::<TOTAL, <Cursor as ChunkCursor<TOTAL>>::Append<LEN>>::new(),
            ts,
        )
    }

    /// Finalise staging once all chunks were streamed.
    pub fn finish(self) -> Manager<Staged, SLOTS> {
        ensure_complete(TOTAL, Cursor::WRITTEN);
        let ts = self.timestamp;
        self.transition(Staged, ts)
    }
}

impl<const SLOTS: usize> Manager<Staged, SLOTS> {
    /// Activate the staged image, moving into `Active`.
    pub fn into_active(self) -> Manager<Active, SLOTS> {
        let ts = self.timestamp;
        self.transition(Active, ts)
    }

    /// Prepare to roll back the active image.
    pub fn into_rollback_pending(self) -> Manager<RollbackPending, SLOTS> {
        let ts = self.timestamp;
        self.transition(RollbackPending, ts)
    }
}

impl<const SLOTS: usize> Manager<Active, SLOTS> {
    /// Begin the rollback procedure from the `Active` state.
    pub fn begin_rollback(self) -> Manager<RollbackPending, SLOTS> {
        let ts = self.timestamp;
        self.transition(RollbackPending, ts)
    }
}

impl<const SLOTS: usize> Manager<RollbackPending, SLOTS> {
    /// Complete a rollback successfully, returning to `Active`.
    pub fn rollback_success(self) -> Manager<Active, SLOTS> {
        let ts = self.timestamp;
        self.transition(Active, ts)
    }

    /// Abort the rollback, transitioning to `Quiescent`.
    pub fn rollback_abort(self) -> Manager<Quiescent, SLOTS> {
        let ts = self.timestamp;
        self.transition(Quiescent, ts)
    }
}

impl<const SLOTS: usize> Manager<Quiescent, SLOTS> {
    /// Reset the automaton and prepare for the next load cycle.
    pub fn reset(self) -> Manager<AwaitBegin, SLOTS> {
        let ts = self.timestamp;
        self.transition(AwaitBegin, ts)
    }
}

impl<State, const SLOTS: usize> Manager<State, SLOTS>
where
    State: ManagerState,
{
    fn transition<Next: ManagerState>(self, _state: Next, timestamp: u32) -> Manager<Next, SLOTS> {
        let Manager {
            slot_states,
            policy,
            ..
        } = self;
        Manager {
            slot_states,
            _state: PhantomData,
            timestamp,
            policy,
        }
    }

    fn slot_state(&mut self, slot: Slot) -> &mut SlotState {
        &mut self.slot_states[slot_index(slot)]
    }

    fn slot_state_from_u8(&mut self, raw: u8) -> Result<(Slot, &mut SlotState), MgmtError> {
        let slot = slot_from_u8(raw).ok_or(MgmtError::InvalidSlot(raw))?;
        Ok((slot, self.slot_state(slot)))
    }

    fn next_ts(&mut self) -> u32 {
        let current = self.timestamp;
        self.timestamp = self.timestamp.saturating_add(1);
        current
    }

    pub fn load_begin(&mut self, slot: Slot, header: Header) -> Result<u32, MgmtError> {
        let state = self.slot_state(slot);
        state.loader.begin(header)?;
        let version = state.version_counter.saturating_add(1);
        state.pending.begin(version);
        Ok(version)
    }

    pub fn load_begin_raw(&mut self, slot: u8, header: Header) -> Result<u32, MgmtError> {
        let (_slot, state) = self.slot_state_from_u8(slot)?;
        state.loader.begin(header)?;
        let version = state.version_counter.saturating_add(1);
        state.pending.begin(version);
        Ok(version)
    }

    pub fn load_chunk(&mut self, slot: Slot, offset: u32, chunk: &[u8]) -> Result<(), MgmtError> {
        let state = self.slot_state(slot);
        state.loader.write(offset, chunk)?;
        Ok(())
    }

    pub fn load_chunk_raw(&mut self, slot: u8, offset: u32, chunk: &[u8]) -> Result<(), MgmtError> {
        let (_, state) = self.slot_state_from_u8(slot)?;
        state.loader.write(offset, chunk)?;
        Ok(())
    }

    pub fn load_commit(&mut self, slot: Slot, storage: &mut SlotStorage) -> Result<u32, MgmtError> {
        let state = self.slot_state(slot);
        let version = state.pending.take()?;
        let verified = state.loader.commit()?;
        let code = verified.code;
        storage.staging_mut()[..code.len()].copy_from_slice(code);

        let mut header = verified.header;
        header.code_len = code.len() as u16;
        let meta = ImageMeta::new(version, header);
        state.inventory.stage(meta);
        state.version_counter = version;
        Ok(version)
    }

    pub fn load_commit_raw(
        &mut self,
        slot: u8,
        storage: &mut SlotStorage,
    ) -> Result<u32, MgmtError> {
        let (slot, _) = self.slot_state_from_u8(slot)?;
        self.load_commit(slot, storage)
    }

    pub fn activate<'arena>(
        &mut self,
        slot: Slot,
        storage: &'arena mut SlotStorage,
        host_slots: &mut HostSlots<'arena>,
    ) -> Result<TransitionReport, MgmtError> {
        let state = self.slot_state(slot);
        let previous_active = state.inventory.current_active();
        let staged = state.inventory.take_stage()?;

        if let Some(active_meta) = previous_active {
            storage.copy_active_to_backup(active_meta.code_len());
            if let Err(err) = host_slots.uninstall(slot)
                && !matches!(err, HostError::SlotEmpty)
            {
                return Err(err.into());
            }
        }

        storage.copy_staging_to_active(staged.meta().code_len());

        let active_meta = staged.meta();
        state.inventory.install_active(active_meta, previous_active);

        let (active_buf, scratch) = storage.active_and_scratch_mut();
        let code_slice = &active_buf[..active_meta.code_len()];
        let machine = Machine::with_mem(
            code_slice,
            scratch,
            active_meta.header.mem_len as usize,
            active_meta.header.fuel_max,
        )?;
        if let Err(err) = host_slots.install(slot, machine) {
            return Err(err.into());
        }

        let ts = self.next_ts();
        push(PolicyCommit::new(ts, slot_id(slot), active_meta.version));
        let policy = self.harvest_policy(slot);
        Ok(TransitionReport {
            version: active_meta.version,
            policy,
        })
    }

    pub fn revert<'arena>(
        &mut self,
        slot: Slot,
        storage: &'arena mut SlotStorage,
        host_slots: &mut HostSlots<'arena>,
    ) -> Result<TransitionReport, MgmtError> {
        let state = self.slot_state(slot);
        let active_meta = state
            .inventory
            .current_active()
            .ok_or(MgmtError::NoActiveImage)?;

        storage.copy_active_to_staging(active_meta.code_len());
        state.inventory.stage(active_meta);

        if let Err(err) = host_slots.uninstall(slot)
            && !matches!(err, HostError::SlotEmpty)
        {
            return Err(err.into());
        }

        let new_active = {
            let entry = state.inventory.active_mut()?;
            entry.revert()?
        };

        storage.copy_backup_to_active(new_active.code_len());
        let (active_buf, scratch) = storage.active_and_scratch_mut();
        let code_slice = &active_buf[..new_active.code_len()];
        let machine = Machine::with_mem(
            code_slice,
            scratch,
            new_active.header.mem_len as usize,
            new_active.header.fuel_max,
        )?;
        host_slots.install(slot, machine)?;

        let ts = self.next_ts();
        push(PolicyRollback::new(ts, slot_id(slot), new_active.version));
        let policy = self.harvest_policy(slot);
        Ok(TransitionReport {
            version: new_active.version,
            policy,
        })
    }

    pub fn stats(&self, slot: Slot) -> Result<StatsResp, MgmtError> {
        let state = &self.slot_states[slot_index(slot)];
        Ok(StatsResp {
            traps: 0,
            aborts: 0,
            fuel_used: 0,
            active_version: state
                .inventory
                .current_active()
                .map(|meta| meta.version)
                .unwrap_or(0),
        })
    }

    pub fn staged_version(&self, slot: Slot) -> Option<u32> {
        self.slot_states[slot_index(slot)]
            .inventory
            .staged()
            .map(|meta| meta.version)
    }

    pub fn drain_policy_snapshot(&mut self, slot: Slot) -> Result<PolicySnapshot, MgmtError> {
        Ok(self.harvest_policy(slot))
    }

    pub fn policy_snapshot(&self, slot: Slot) -> Result<PolicySnapshot, MgmtError> {
        Ok(self.slot_states[slot_index(slot)].last_policy)
    }

    fn harvest_policy(&mut self, slot: Slot) -> PolicySnapshot {
        let state = &mut self.slot_states[slot_index(slot)];
        let mut snapshot = PolicySnapshot::default();
        observe::for_each_since(&mut state.policy_cursor, |event| {
            if let Some(policy) = PolicyEvent::from_tap(event) {
                snapshot.record(policy);
            }
        });
        state.last_policy = snapshot;
        snapshot
    }
}

#[inline]
pub(crate) fn slot_from_u8(raw: u8) -> Option<Slot> {
    match raw {
        0 => Some(Slot::Forward),
        1 => Some(Slot::EndpointRx),
        2 => Some(Slot::EndpointTx),
        3 => Some(Slot::Rendezvous),
        4 => Some(Slot::Route),
        _ => None,
    }
}

/// Maximum number of rendezvous links tracked by a management seed.
const MGMT_MAX_LINKS: usize = 8;

/// Fixed-capacity set of rendezvous identifiers referenced by the management seed.
#[derive(Clone, Copy)]
pub struct MgmtLinks {
    ids: [RendezvousId; MGMT_MAX_LINKS],
    len: usize,
}

impl Default for MgmtLinks {
    fn default() -> Self {
        Self::new()
    }
}

impl MgmtLinks {
    /// Construct an empty link set.
    pub const fn new() -> Self {
        Self {
            ids: [RendezvousId::new(0); MGMT_MAX_LINKS],
            len: 0,
        }
    }

    /// Returns true when the set already contains `id`.
    #[inline]
    pub fn contains(&self, id: RendezvousId) -> bool {
        (0..self.len).any(|idx| self.ids[idx] == id)
    }

    /// Append a rendezvous identifier if capacity allows and it is not present yet.
    #[inline]
    pub fn push(&mut self, id: RendezvousId) {
        if self.len >= MGMT_MAX_LINKS || self.contains(id) {
            return;
        }
        self.ids[self.len] = id;
        self.len += 1;
    }

    /// Iterate over the tracked rendezvous identifiers.
    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = RendezvousId> + '_ {
        self.ids[..self.len].iter().copied()
    }

    /// Returns true when no rendezvous links are recorded.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

// ======== LeaseGraph integration ============================================

/// Lease specification for management session rendezvous.
pub struct MgmtLeaseSpec<T, U, C, E>(PhantomData<(T, U, C, E)>);

impl<T, U, C, E> LeaseSpec for MgmtLeaseSpec<T, U, C, E>
where
    T: crate::transport::Transport,
    U: crate::runtime::consts::LabelUniverse,
    C: crate::runtime::config::Clock,
    E: crate::control::cap::EpochTable,
{
    type NodeId = RendezvousId;
    type Facet = LeaseBundleFacet<T, U, C, E>;
    const MAX_NODES: usize = MGMT_MAX_LINKS + 1;
    const MAX_CHILDREN: usize = MGMT_MAX_LINKS;
}

impl<T, U, C, E> LeaseSpecFacetNeeds for MgmtLeaseSpec<T, U, C, E>
where
    T: crate::transport::Transport,
    U: crate::runtime::consts::LabelUniverse,
    C: crate::runtime::config::Clock,
    E: crate::control::cap::EpochTable,
{
    const FACET_NEEDS: LeaseFacetNeeds = MGMT_FACET_NEEDS;
}

/// Seed passed to the management automaton.
pub struct MgmtSeed<State>
where
    State: ManagerState,
{
    pub load_slot: Slot,
    pub command: Command,
    pub manager: Manager<State, SLOT_COUNT>,
    pub links: MgmtLinks,
}

impl<State> MgmtSeed<State>
where
    State: ManagerState,
{
    #[inline]
    pub fn links(&self) -> &MgmtLinks {
        &self.links
    }

    #[inline]
    pub fn links_mut(&mut self) -> &mut MgmtLinks {
        &mut self.links
    }
}

/// Control automaton that applies management operations through LeaseGraph.
pub struct MgmtAutomaton<State>
where
    State: ManagerState,
{
    _marker: PhantomData<State>,
}

impl<State> Default for MgmtAutomaton<State>
where
    State: ManagerState,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<State> MgmtAutomaton<State>
where
    State: ManagerState,
{
    pub const fn new() -> Self {
        Self {
            _marker: PhantomData,
        }
    }
}

impl<State, T, U, C, E> ControlAutomaton<T, U, C, E> for MgmtAutomaton<State>
where
    State: ManagerState,
    T: crate::transport::Transport,
    U: crate::runtime::consts::LabelUniverse,
    C: crate::runtime::config::Clock,
    E: crate::control::cap::EpochTable,
{
    type Spec = SlotSpec;
    type Seed = MgmtSeed<State>;
    type Output = (Manager<State, SLOT_COUNT>, Reply);
    type Error = MgmtError;
    type GraphSpec = MgmtLeaseSpec<T, U, C, E>;

    fn run<'lease, 'lease_cfg>(
        _lease: &mut RendezvousLease<'lease, 'lease_cfg, T, U, C, E, Self::Spec>,
        _seed: Self::Seed,
    ) -> ControlStep<Self::Output, Self::Error>
    where
        'lease_cfg: 'lease,
    {
        ControlStep::Abort(MgmtError::InvalidTransition)
    }

    fn run_with_graph<'lease, 'lease_cfg, 'graph>(
        graph: &'graph mut LeaseGraph<'graph, MgmtLeaseSpec<T, U, C, E>>,
        root_lease: &mut RendezvousLease<'lease, 'lease_cfg, T, U, C, E, Self::Spec>,
        mut seed: Self::Seed,
    ) -> ControlStep<Self::Output, Self::Error>
    where
        'lease_cfg: 'lease,
    {
        if let Err(err) = root_lease.with_rendezvous(|rv| {
            let facet = rv.slot_facet();
            facet.load_commit(rv, seed.load_slot, &mut seed.manager)
        }) {
            return ControlStep::Abort(err);
        }

        if let Some(result) = {
            let mut handle = graph.root_handle_mut();
            handle
                .context()
                .slots_mut()
                .map(|slots| slots.track_stage(seed.load_slot))
        } && result.is_err()
        {
            return ControlStep::Abort(MgmtError::InvalidTransition);
        }

        let reply = match seed.command {
            Command::Activate { slot } => match root_lease.with_rendezvous(|rv| {
                let facet = rv.slot_facet();
                facet.activate(rv, slot, &mut seed.manager)
            }) {
                Ok(report) => Reply::Activated(report),
                Err(err) => return ControlStep::Abort(err),
            },
            Command::Revert { slot } => match root_lease.with_rendezvous(|rv| {
                let facet = rv.slot_facet();
                facet.revert(rv, slot, &mut seed.manager)
            }) {
                Ok(report) => Reply::Reverted(report),
                Err(err) => return ControlStep::Abort(err),
            },
            Command::Stats { slot } => match seed.manager.stats(slot) {
                Ok(stats) => Reply::Stats {
                    stats,
                    staged_version: seed.manager.staged_version(slot),
                },
                Err(err) => return ControlStep::Abort(err),
            },
        };
        ControlStep::Complete((seed.manager, reply))
    }
}

fn slot_id(slot: Slot) -> u32 {
    match slot {
        Slot::Forward => 0,
        Slot::EndpointRx => 1,
        Slot::EndpointTx => 2,
        Slot::Rendezvous => 3,
        Slot::Route => 4,
    }
}

// ======== Slot inventory =====================================================

#[derive(Clone, Copy, Debug)]
struct ImageMeta {
    version: u32,
    header: Header,
}

impl ImageMeta {
    fn new(version: u32, header: Header) -> Self {
        Self { version, header }
    }

    fn code_len(&self) -> usize {
        self.header.code_len as usize
    }
}

#[derive(Clone, Copy, Debug)]
struct StageEntry(ImageMeta);

impl StageEntry {
    fn meta(&self) -> ImageMeta {
        self.0
    }
}

#[derive(Clone, Copy, Debug)]
struct ActiveEntry {
    current: ImageMeta,
    previous: Option<ImageMeta>,
}

impl ActiveEntry {
    fn new(current: ImageMeta) -> Self {
        Self {
            current,
            previous: None,
        }
    }

    fn install_previous(&mut self, previous: ImageMeta) {
        self.previous = Some(previous);
    }

    fn current(&self) -> ImageMeta {
        self.current
    }

    fn revert(&mut self) -> Result<ImageMeta, MgmtError> {
        let previous = self.previous.ok_or(MgmtError::NoPreviousImage)?;
        self.previous = Some(self.current);
        self.current = previous;
        Ok(self.current)
    }
}

#[derive(Debug)]
struct SlotInventory {
    staged: Option<StageEntry>,
    active: Option<ActiveEntry>,
}

impl SlotInventory {
    fn new() -> Self {
        Self {
            staged: None,
            active: None,
        }
    }

    fn stage(&mut self, meta: ImageMeta) {
        self.staged = Some(StageEntry(meta));
    }

    fn take_stage(&mut self) -> Result<StageEntry, MgmtError> {
        self.staged.take().ok_or(MgmtError::NoStagedImage)
    }

    fn current_active(&self) -> Option<ImageMeta> {
        self.active.as_ref().map(ActiveEntry::current)
    }

    fn staged(&self) -> Option<ImageMeta> {
        self.staged.as_ref().map(StageEntry::meta)
    }

    fn install_active(&mut self, current: ImageMeta, previous: Option<ImageMeta>) {
        let mut entry = ActiveEntry::new(current);
        if let Some(prev) = previous {
            entry.install_previous(prev);
        }
        self.active = Some(entry);
    }

    fn active_mut(&mut self) -> Result<&mut ActiveEntry, MgmtError> {
        self.active.as_mut().ok_or(MgmtError::NoActiveImage)
    }
}

#[derive(Debug, Default)]
struct PendingVersion(Option<u32>);

impl PendingVersion {
    fn begin(&mut self, version: u32) {
        self.0 = Some(version);
    }

    fn take(&mut self) -> Result<u32, MgmtError> {
        self.0.take().ok_or(MgmtError::LoaderNotFinalised)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        epf::{host::HostSlots, verifier::compute_hash},
        rendezvous::SlotStorage,
    };

    #[test]
    fn activate_flow_updates_version() {
        let mut manager = Manager::<Cold, { SLOT_COUNT }>::new().into_await_begin();
        let slot = Slot::Rendezvous;
        let code = [0x01u8, 0x02, 0x03, 0x04];
        let header = Header {
            code_len: code.len() as u16,
            fuel_max: 16,
            mem_len: 64,
            flags: 0,
            hash: compute_hash(&code),
        };

        manager.load_begin(slot, header).unwrap();
        manager.load_chunk(slot, 0, &code[..2]).unwrap();
        manager.load_chunk(slot, 2, &code[2..]).unwrap();

        let mut storage = SlotStorage::new();
        manager.load_commit(slot, &mut storage).unwrap();

        let mut host_slots = HostSlots::new();
        let report = manager
            .activate(slot, &mut storage, &mut host_slots)
            .unwrap();
        assert_eq!(report.version, 1);
    }

    #[test]
    fn chunk_out_of_order_is_rejected() {
        let mut manager = Manager::<Cold, { SLOT_COUNT }>::new().into_await_begin();
        let slot = Slot::Rendezvous;
        let header = Header {
            code_len: 2,
            fuel_max: 8,
            mem_len: 32,
            flags: 0,
            hash: compute_hash(&[0xAA, 0xBB]),
        };
        manager.load_begin(slot, header).unwrap();

        let err = manager
            .load_chunk(slot, 1, &[0xAA, 0xBB])
            .expect_err("chunk should be rejected");
        assert!(matches!(
            err,
            MgmtError::ChunkOutOfOrder {
                expected: 0,
                got: 1
            }
        ));
    }
}
