//! SessionCluster - Distributed control-plane coordination.
//!
//! This module implements SessionCluster, which coordinates multiple Rendezvous
//! instances (local and remote) for distributed session management.

use crate::control::automaton::{
    delegation::{
        DelegateClaimAutomaton, DelegateClaimSeed, DelegateMintAutomaton, DelegateMintSeed,
        DelegatedPortClaimGuard, DelegatedPortKey, DelegatedPortSlot, DelegatedPortTable,
        DelegatedPortWitness, DelegationGraphContext, DelegationLeaseSpec,
    },
    distributed::{DistributedSplice, DistributedSpliceInv, SpliceAck, SpliceIntent},
    splice::{
        SpliceBeginAutomaton, SpliceCommitAutomaton, SpliceGraphContext, SplicePrepareAutomaton,
        SplicePrepareSeed,
    },
};
use crate::control::cap::resource_kinds::{
    CancelAckKind, CancelKind, CheckpointKind, CommitKind, LoopBreakKind, LoopContinueKind,
    RerouteHandle, RerouteKind, RollbackKind, RouteDecisionHandle, RouteDecisionKind,
    SpliceAckKind, SpliceHandle, SpliceIntentKind,
};
use crate::control::cap::typed_tokens::CapFrameToken;
use crate::control::cap::{
    AllowsCanonical, CapShot, CapToken, CapsMask, ControlHandle, EndpointResource, GenericCapToken,
    MintConfigMarker, ResourceKind, SessionScopedKind, VerifiedCap,
};
use crate::control::cluster::effects::CpEffect;
use crate::control::handle::{bag::HandleBag, spec};
use crate::control::lease::{
    ControlAutomaton, ControlCore as LeaseControlCore, ControlStep, DelegationDriveError, FullSpec,
    LeaseError, RegisterRendezvousError,
    bundle::{LeaseBundleContext, LeaseGraphBundleExt},
    graph::{FacetContext, LeaseGraph, LeaseGraphError, LeaseSpec},
    map::ArrayMap,
    planner::{
        DELEGATION_CHILD_SET_CAPACITY, FacetCapsDelegation, FacetCapsSplice, LeaseFacetNeeds,
        LeaseSpecFacetNeeds, facet_needs,
    },
};
use crate::endpoint::affine::LaneGuard;
use core::{mem, ptr::NonNull};
const MAX_CACHED_SPLICES: usize = 64;
const HANDLE_RESOLVER_SLOTS: usize = 128;

type MgmtManager = Manager<AwaitBegin, { crate::rendezvous::SLOT_COUNT }>;

fn splice_operands_from_handle(handle: SpliceHandle) -> SpliceOperands {
    SpliceOperands::new(
        RendezvousId::new(handle.src_rv),
        RendezvousId::new(handle.dst_rv),
        LaneId::new(handle.src_lane as u32),
        LaneId::new(handle.dst_lane as u32),
        Gen::new(handle.old_gen),
        Gen::new(handle.new_gen),
        handle.seq_tx,
        handle.seq_rx,
    )
}

pub use super::effects::EffectEnvelope;
use super::error::{CpError, DelegationError, SpliceError as CpSpliceError};
use super::ffi::Hello;
use crate::control::automaton::txn::{InAcked, InBegin, NoopTap};
use crate::control::types::{Gen, LaneId, RendezvousId, SessionId};
use crate::eff::EffIndex;
use crate::endpoint::cursor::CursorEndpoint;
use crate::global::const_dsl::{HandlePlan, ScopeId, StaticPlanKind};
use crate::observe::ScopeTrace;
use crate::runtime::mgmt::session;
use crate::runtime::mgmt::{
    AwaitBegin, Cold, Manager, MgmtAutomaton, MgmtError, MgmtLeaseSpec, MgmtSeed, Reply,
};
use crate::transport::TransportSnapshot;

#[cfg(test)]
use std::boxed::Box;

/// Control-plane effect envelope encompassing the effect and its operands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpliceOperands {
    pub src_rv: RendezvousId,
    pub dst_rv: RendezvousId,
    pub src_lane: LaneId,
    pub dst_lane: LaneId,
    pub old_gen: Gen,
    pub new_gen: Gen,
    pub seq_tx: u32,
    pub seq_rx: u32,
}

impl SpliceOperands {
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        src_rv: RendezvousId,
        dst_rv: RendezvousId,
        src_lane: LaneId,
        dst_lane: LaneId,
        old_gen: Gen,
        new_gen: Gen,
        seq_tx: u32,
        seq_rx: u32,
    ) -> Self {
        Self {
            src_rv,
            dst_rv,
            src_lane,
            dst_lane,
            old_gen,
            new_gen,
            seq_tx,
            seq_rx,
        }
    }

    pub fn from_intent(intent: &SpliceIntent) -> Self {
        Self {
            src_rv: intent.src_rv,
            dst_rv: intent.dst_rv,
            src_lane: intent.src_lane,
            dst_lane: intent.dst_lane,
            old_gen: intent.old_gen,
            new_gen: intent.new_gen,
            seq_tx: intent.seq_tx,
            seq_rx: intent.seq_rx,
        }
    }

    pub fn intent(&self, sid: SessionId) -> SpliceIntent {
        SpliceIntent::new(
            self.src_rv,
            self.dst_rv,
            sid.raw(),
            self.old_gen,
            self.new_gen,
            self.seq_tx,
            self.seq_rx,
            self.src_lane,
            self.dst_lane,
        )
    }

    pub fn ack(&self, sid: SessionId) -> SpliceAck {
        SpliceAck::new(
            self.src_rv,
            self.dst_rv,
            sid.raw(),
            self.new_gen,
            self.dst_lane,
            self.seq_tx,
            self.seq_rx,
        )
    }
}

#[derive(Clone, Copy)]
struct DelegationChildSet {
    ids: [RendezvousId; DELEGATION_CHILD_SET_CAPACITY],
    len: usize,
}

impl DelegationChildSet {
    const fn new() -> Self {
        Self {
            ids: [RendezvousId::new(0); DELEGATION_CHILD_SET_CAPACITY],
            len: 0,
        }
    }

    fn push(&mut self, id: RendezvousId) {
        if self.len >= DELEGATION_CHILD_SET_CAPACITY || self.contains(id) {
            return;
        }
        self.ids[self.len] = id;
        self.len += 1;
    }

    fn contains(&self, id: RendezvousId) -> bool {
        (0..self.len).any(|idx| self.ids[idx] == id)
    }

    fn iter(&self) -> impl Iterator<Item = RendezvousId> + '_ {
        self.ids[..self.len].iter().copied()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DelegatePhase {
    Mint,
    Claim,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DelegateOperands {
    pub phase: DelegatePhase,
    pub token: CapToken,
}

impl DelegateOperands {
    pub const fn mint(token: CapToken) -> Self {
        Self {
            phase: DelegatePhase::Mint,
            token,
        }
    }

    pub const fn claim(token: CapToken) -> Self {
        Self {
            phase: DelegatePhase::Claim,
            token,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PendingEffect {
    None,
    Dispatch {
        target: RendezvousId,
        envelope: CpCommand,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CpCommand {
    pub effect: CpEffect,
    pub sid: Option<SessionId>,
    pub lane: Option<LaneId>,
    pub generation: Option<Gen>,
    pub prev_generation: Option<Gen>,
    pub fences: Option<(u32, u32)>,
    pub splice: Option<SpliceOperands>,
    pub intent: Option<SpliceIntent>,
    pub ack: Option<SpliceAck>,
    pub delegate: Option<DelegateOperands>,
}

impl CpCommand {
    pub const fn new(effect: CpEffect) -> Self {
        Self {
            effect,
            sid: None,
            lane: None,
            generation: None,
            prev_generation: None,
            fences: None,
            splice: None,
            intent: None,
            ack: None,
            delegate: None,
        }
    }

    pub fn with_sid(mut self, sid: SessionId) -> Self {
        self.sid = Some(sid);
        self
    }

    pub fn with_lane(mut self, lane: LaneId) -> Self {
        self.lane = Some(lane);
        self
    }

    pub fn with_generation(mut self, generation: Gen) -> Self {
        self.generation = Some(generation);
        self
    }

    pub fn with_prev_generation(mut self, generation: Gen) -> Self {
        self.prev_generation = Some(generation);
        self
    }

    pub fn with_fences(mut self, fences: Option<(u32, u32)>) -> Self {
        self.fences = fences;
        self
    }

    pub fn with_splice(mut self, operands: SpliceOperands) -> Self {
        self.splice = Some(operands);
        self
    }

    pub fn with_intent(mut self, intent: SpliceIntent) -> Self {
        self.intent = Some(intent);
        self
    }

    pub fn with_ack(mut self, ack: SpliceAck) -> Self {
        self.ack = Some(ack);
        self
    }

    pub fn with_delegate(mut self, delegate: DelegateOperands) -> Self {
        self.delegate = Some(delegate);
        self
    }

    fn derive_sid_lane(token: CapToken) -> (SessionId, LaneId) {
        let header = token.header();
        let sid = u32::from_be_bytes([header[0], header[1], header[2], header[3]]);
        let lane = header[4] as u32;
        (SessionId::new(sid), LaneId::new(lane))
    }

    pub fn delegate_mint(token: CapToken) -> Self {
        let (sid, lane) = Self::derive_sid_lane(token);
        Self::new(CpEffect::Delegate)
            .with_sid(sid)
            .with_lane(lane)
            .with_delegate(DelegateOperands::mint(token))
    }

    pub fn delegate_claim(token: CapToken) -> Self {
        let (sid, lane) = Self::derive_sid_lane(token);
        Self::new(CpEffect::Delegate)
            .with_sid(sid)
            .with_lane(lane)
            .with_delegate(DelegateOperands::claim(token))
    }

    pub fn splice_begin(sid: SessionId, operands: SpliceOperands) -> Self {
        Self::new(CpEffect::SpliceBegin)
            .with_sid(sid)
            .with_lane(operands.src_lane)
            .with_generation(operands.new_gen)
            .with_prev_generation(operands.old_gen)
            .with_fences(Some((operands.seq_tx, operands.seq_rx)))
            .with_splice(operands)
            .with_intent(operands.intent(sid))
    }

    pub fn splice_ack(sid: SessionId, operands: SpliceOperands) -> Self {
        Self::new(CpEffect::SpliceAck)
            .with_sid(sid)
            .with_lane(operands.dst_lane)
            .with_generation(operands.new_gen)
            .with_prev_generation(operands.old_gen)
            .with_fences(Some((operands.seq_tx, operands.seq_rx)))
            .with_splice(operands)
            .with_intent(operands.intent(sid))
            .with_ack(operands.ack(sid))
    }

    pub fn splice_commit(sid: SessionId, operands: SpliceOperands) -> Self {
        Self::new(CpEffect::SpliceCommit)
            .with_sid(sid)
            .with_lane(operands.src_lane)
            .with_generation(operands.new_gen)
            .with_prev_generation(operands.old_gen)
            .with_fences(Some((operands.seq_tx, operands.seq_rx)))
            .with_splice(operands)
            .with_ack(operands.ack(sid))
    }

    pub fn cancel_begin(sid: SessionId, lane: LaneId) -> Self {
        Self::new(CpEffect::CancelBegin)
            .with_sid(sid)
            .with_lane(lane)
    }

    pub fn cancel_ack(sid: SessionId, lane: LaneId, generation: Gen) -> Self {
        Self::new(CpEffect::CancelAck)
            .with_sid(sid)
            .with_lane(lane)
            .with_generation(generation)
    }

    pub fn checkpoint(sid: SessionId, lane: LaneId) -> Self {
        Self::new(CpEffect::Checkpoint)
            .with_sid(sid)
            .with_lane(lane)
    }

    pub fn rollback(sid: SessionId, lane: LaneId, generation: Gen) -> Self {
        Self::new(CpEffect::Rollback)
            .with_sid(sid)
            .with_lane(lane)
            .with_generation(generation)
    }

    pub fn splice_local_begin(
        sid: SessionId,
        lane: LaneId,
        generation: Gen,
        fences: Option<(u32, u32)>,
    ) -> Self {
        Self::new(CpEffect::SpliceBegin)
            .with_sid(sid)
            .with_lane(lane)
            .with_generation(generation)
            .with_fences(fences)
    }

    pub fn splice_local_commit(sid: SessionId, lane: LaneId) -> Self {
        Self::new(CpEffect::SpliceCommit)
            .with_sid(sid)
            .with_lane(lane)
    }

    pub fn commit(sid: SessionId, lane: LaneId, generation: Gen) -> Self {
        Self::new(CpEffect::Commit)
            .with_sid(sid)
            .with_lane(lane)
            .with_generation(generation)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DynamicResolution {
    Splice {
        dst_rv: RendezvousId,
        dst_lane: LaneId,
        fences: Option<(u32, u32)>,
    },
    Reroute {
        dst_rv: RendezvousId,
        dst_lane: LaneId,
        shard: Option<u32>,
    },
    RouteArm {
        arm: u8,
    },
    Loop {
        decision: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolverContext {
    pub rv_id: RendezvousId,
    pub session: Option<SessionId>,
    pub lane: LaneId,
    pub eff_index: EffIndex,
    pub tag: u8,
    pub metrics: TransportSnapshot,
    pub scope_id: ScopeId,
    pub scope_trace: Option<ScopeTrace>,
    /// Transport context snapshot for resolver functions.
    /// This provides O(1) access to protocol-specific state without global registries.
    pub transport_ctx: crate::transport::context::ContextSnapshot,
}

impl ResolverContext {
    /// Query a transport context value by key.
    ///
    /// This is the primary mechanism for resolver functions to access
    /// protocol-specific state (e.g., QUIC stream loop decisions, path validation).
    #[inline]
    pub fn transport(&self, key: crate::transport::context::ContextKey) -> Option<crate::transport::context::ContextValue> {
        self.transport_ctx.query(key)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DynamicResolverKey {
    rv: RendezvousId,
    eff_index: EffIndex,
    tag: u8,
}

impl DynamicResolverKey {
    const fn new(rv: RendezvousId, eff_index: EffIndex, tag: u8) -> Self {
        Self { rv, eff_index, tag }
    }
}

type DynamicResolverFn<'cfg, T, U, C, const MAX_RV: usize> = fn(
    &SessionCluster<'cfg, T, U, C, MAX_RV>,
    &crate::global::const_dsl::DynamicMeta,
    ResolverContext,
) -> Result<DynamicResolution, ()>;

struct DynamicResolverEntry<'cfg, T, U, C, const MAX_RV: usize>
where
    T: crate::transport::Transport + 'cfg,
    U: crate::runtime::consts::LabelUniverse,
    C: crate::runtime::config::Clock,
{
    resolver: DynamicResolverFn<'cfg, T, U, C, MAX_RV>,
    plan: HandlePlan,
    scope_trace: Option<ScopeTrace>,
}

/// Validate that the resource tag represents a supported dynamic control operation.
/// The operation type (route, reroute, splice) is determined solely by the tag.
const fn is_dynamic_control_tag(tag: u8) -> bool {
    matches!(
        tag,
        LoopContinueKind::TAG
            | LoopBreakKind::TAG
            | RouteDecisionKind::TAG
            | RerouteKind::TAG
            | SpliceIntentKind::TAG
            | SpliceAckKind::TAG
    )
}

fn session_caps_mask_for_tag(
    tag: u8,
    sid: SessionId,
    lane: crate::rendezvous::Lane,
) -> Option<CapsMask> {
    use crate::control::cap::resource_kinds;
    use crate::control::cap::{ResourceKind, SessionScopedKind};

    let sid_rv = crate::rendezvous::SessionId::new(sid.raw());
    let lane_rv = crate::rendezvous::Lane::new(lane.raw());

    macro_rules! mask_for {
        ($kind:ty) => {{
            let mut handle = <$kind as SessionScopedKind>::handle_for_session(sid_rv, lane_rv);
            let mask = <$kind as ResourceKind>::caps_mask(&handle);
            <$kind as ResourceKind>::zeroize(&mut handle);
            Some(mask)
        }};
    }

    match tag {
        resource_kinds::LoopContinueKind::TAG => mask_for!(resource_kinds::LoopContinueKind),
        resource_kinds::LoopBreakKind::TAG => mask_for!(resource_kinds::LoopBreakKind),
        resource_kinds::CheckpointKind::TAG => mask_for!(resource_kinds::CheckpointKind),
        resource_kinds::CommitKind::TAG => mask_for!(resource_kinds::CommitKind),
        resource_kinds::RollbackKind::TAG => mask_for!(resource_kinds::RollbackKind),
        resource_kinds::CancelKind::TAG => mask_for!(resource_kinds::CancelKind),
        resource_kinds::CancelAckKind::TAG => mask_for!(resource_kinds::CancelAckKind),
        resource_kinds::SpliceIntentKind::TAG => mask_for!(resource_kinds::SpliceIntentKind),
        resource_kinds::SpliceAckKind::TAG => mask_for!(resource_kinds::SpliceAckKind),
        resource_kinds::RerouteKind::TAG => mask_for!(resource_kinds::RerouteKind),
        _ => None,
    }
}

#[inline]
fn initializer_command(effect: CpEffect, sid: SessionId, lane: LaneId) -> Option<CpCommand> {
    let _ = (effect, sid, lane);
    None
}

/// Trait implemented by local Rendezvous instances that can evaluate control-plane effects.
pub trait EffectExecutor {
    fn id(&self) -> RendezvousId;
    fn run_effect(&self, envelope: CpCommand) -> Result<(), CpError>;
    fn prepare_splice_operands(
        &self,
        sid: SessionId,
        src_lane: LaneId,
        dst_rv: RendezvousId,
        dst_lane: LaneId,
        fences: Option<(u32, u32)>,
    ) -> Result<SpliceOperands, CpError>;
    fn abort_distributed_splice(&self, sid: SessionId) -> Result<(), CpError>;
}

/// Control-plane adapter for remote Rendezvous communication.
///
/// This trait abstracts the communication channel between Rendezvous instances.
/// Implementations can use in-process channels, QUIC, TCP, or other transports.
pub trait ControlPlaneAdapter {
    /// Get the peer Rendezvous ID.
    fn peer(&self) -> RendezvousId;

    /// Send a handshake message to establish connection.
    fn handshake(&self, hello: &Hello) -> Result<Hello, CpError>;

    /// Execute a control-plane effect on the remote Rendezvous.
    ///
    /// This is the core primitive for distributed coordination. All higher-level
    /// operations (splice, delegation, etc.) are decomposed into effect sequences.
    fn run(&self, envelope: CpCommand) -> Result<(), CpError>;
}

/// Slot for storing a local Rendezvous reference.
///
/// Stores a reference to an effect executor implementing the control-plane
/// evaluation interface.
pub struct RendezvousSlot<'a> {
    executor: &'a dyn EffectExecutor,
}

impl<'a> RendezvousSlot<'a> {
    /// Create a new slot with the given Rendezvous reference.
    ///
    /// # Lifetime
    ///
    /// The lifetime `'a` is the lifetime of the borrow, not the lifetime
    /// of the Rendezvous itself. This allows temporary borrows to be registered
    /// in SessionCluster via lifetime widening.
    pub fn new<'b>(executor: &'b dyn EffectExecutor) -> RendezvousSlot<'b> {
        RendezvousSlot { executor }
    }

    /// Get the Rendezvous ID.
    pub fn id(&self) -> RendezvousId {
        self.executor.id()
    }

    /// Execute a control-plane effect on the Rendezvous.
    pub fn run(&self, envelope: CpCommand) -> Result<(), CpError> {
        self.executor.run_effect(envelope)
    }

    pub fn prepare_splice_operands(
        &self,
        sid: SessionId,
        src_lane: LaneId,
        dst_rv: RendezvousId,
        dst_lane: LaneId,
        fences: Option<(u32, u32)>,
    ) -> Result<SpliceOperands, CpError> {
        self.executor
            .prepare_splice_operands(sid, src_lane, dst_rv, dst_lane, fences)
    }

    pub fn abort_distributed_splice(&self, sid: SessionId) -> Result<(), CpError> {
        self.executor.abort_distributed_splice(sid)
    }
}

/// Slot for storing a remote Rendezvous adapter.
pub struct ControlPlaneSlot<'a> {
    adapter: &'a dyn ControlPlaneAdapter,
}

impl<'a> ControlPlaneSlot<'a> {
    /// Create a new slot with the given adapter.
    pub fn new(adapter: &'a dyn ControlPlaneAdapter) -> Self {
        Self { adapter }
    }

    /// Get the adapter.
    pub fn adapter(&self) -> &'a dyn ControlPlaneAdapter {
        self.adapter
    }
}

enum DistributedPhase {
    Begin {
        txn: Option<InBegin<DistributedSpliceInv, crate::control::types::One>>,
    },
    Acked {
        txn: InAcked<DistributedSpliceInv, crate::control::types::One>,
        ack: SpliceAck,
    },
}

struct DistributedEntry {
    operands: SpliceOperands,
    phase: DistributedPhase,
}

/// Distributed splice state tracking.
///
/// Tracks in-flight distributed splice operations to ensure exactly-once semantics.
pub struct DistributedSpliceState<const MAX: usize> {
    entries: ArrayMap<SessionId, DistributedEntry, MAX>,
}

impl<const MAX: usize> Default for DistributedSpliceState<MAX> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const MAX: usize> DistributedSpliceState<MAX> {
    /// Create a new empty state.
    pub const fn new() -> Self {
        Self {
            entries: ArrayMap::new(),
        }
    }

    pub fn begin(
        &mut self,
        sid: SessionId,
        operands: SpliceOperands,
    ) -> Result<(SpliceIntent, SpliceAck), CpError> {
        if self.entries.contains_key(&sid) {
            return Err(CpError::ReplayDetected {
                operation: CpEffect::SpliceBegin as u8,
                nonce: sid.raw(),
            });
        }

        let mut tap = NoopTap;
        let (in_begin, intent) = DistributedSplice::begin(
            operands.src_rv,
            operands.dst_rv,
            sid.raw(),
            operands.old_gen,
            operands.new_gen,
            operands.seq_tx,
            operands.seq_rx,
            operands.src_lane,
            operands.dst_lane,
            &mut tap,
        );

        let entry = DistributedEntry {
            operands,
            phase: DistributedPhase::Begin {
                txn: Some(in_begin),
            },
        };

        self.entries
            .insert(sid, entry)
            .map_err(|_| CpError::ResourceExhausted)?;

        Ok((intent, operands.ack(sid)))
    }

    pub fn acknowledge(&mut self, sid: SessionId) -> Result<SpliceAck, CpError> {
        let entry = self
            .entries
            .get_mut(&sid)
            .ok_or(CpError::Splice(CpSpliceError::InvalidSession))?;

        let txn = match &mut entry.phase {
            DistributedPhase::Begin { txn } => txn.take().ok_or(CpError::ReplayDetected {
                operation: CpEffect::SpliceAck as u8,
                nonce: sid.raw(),
            })?,
            DistributedPhase::Acked { .. } => {
                return Err(CpError::ReplayDetected {
                    operation: CpEffect::SpliceAck as u8,
                    nonce: sid.raw(),
                });
            }
        };

        let mut tap = NoopTap;
        let in_acked = DistributedSplice::acknowledge(txn, &mut tap);
        let ack = entry.operands.ack(sid);
        entry.phase = DistributedPhase::Acked { txn: in_acked, ack };

        Ok(ack)
    }

    pub fn commit(
        &mut self,
        sid: SessionId,
        expected: Option<SpliceAck>,
    ) -> Result<SpliceOperands, CpError> {
        let entry = self
            .entries
            .remove(&sid)
            .ok_or(CpError::Splice(CpSpliceError::InvalidSession))?;

        let DistributedEntry { operands, phase } = entry;

        match phase {
            DistributedPhase::Acked { txn, ack } => {
                if let Some(exp) = expected
                    && exp != ack
                {
                    return Err(CpError::Splice(CpSpliceError::CommitFailed));
                }

                let mut tap = NoopTap;
                let _closed = DistributedSplice::commit(txn, &mut tap);
                Ok(operands)
            }
            DistributedPhase::Begin { .. } => Err(CpError::Splice(CpSpliceError::InvalidState)),
        }
    }

    pub fn abort(&mut self, sid: SessionId) -> Option<SpliceOperands> {
        self.entries.remove(&sid).map(|entry| entry.operands)
    }

    pub fn get(&self, sid: SessionId) -> Option<&SpliceOperands> {
        self.entries.get(&sid).map(|entry| &entry.operands)
    }
}

/// SessionCluster - Coordinates multiple Rendezvous instances.
///
/// This is the top-level distributed control-plane coordinator. It manages:
/// - Local Rendezvous instances (same process/node)
/// - Remote Rendezvous instances (via ControlPlaneAdapter)
/// - Distributed splice coordination
/// - Intent/Ack routing
///
/// # Type Parameters
///
/// - `MAX_RV`: Maximum number of Rendezvous instances (local + remote)
///
/// # Example
///
/// ```rust,ignore
/// use hibana::control::{SessionCluster, RendezvousId};
///
/// let mut cluster: SessionCluster<8> = SessionCluster::new(leak_clock());
///
/// // Register local Rendezvous
/// cluster.register_local(RendezvousSlot::new(local_rv_executor))?;
///
/// // Register remote Rendezvous
/// cluster.register_remote(RendezvousId::new(2), remote_adapter)?;
///
/// // Perform distributed splice
/// cluster.distributed_splice(
///     sid,
///     src_lane,
///     RendezvousId::new(2),
///     dst_lane
/// )?;
/// ```
/// Internal mutable state of SessionCluster.
///
/// # Safety Invariants (POPL/SOSP/OSDI documentation)
///
/// The following invariants MUST be maintained by all code accessing `ControlCore`:
///
/// 1. **No duplicate lane leases**: At most one `LaneLease` exists per (rv_id, lane) pair
/// 2. **Lane exclusivity during lease**: While a lane is leased, only the lease guard may touch that lane's state
/// 3. **Rendezvous ownership**: Rendezvous instances are owned by the cluster and must not be removed while leases exist
/// 4. **Splice state consistency**: distributed_splice operations must maintain Begin→Ack→Commit ordering
///
/// Violations of these invariants are caught by:
/// - `debug_assert!` in development builds
/// - TAP events (LANE_ACQUIRE/LANE_RELEASE) for runtime monitoring
struct ControlCore<'cfg, T, U, C, E, const MAX_RV: usize>
where
    T: crate::transport::Transport,
    U: crate::runtime::consts::LabelUniverse,
    C: crate::runtime::config::Clock,
    E: crate::control::cap::EpochTable,
{
    /// Owned local Rendezvous instances (same process/node).
    locals: LeaseControlCore<'cfg, T, U, C, E, MAX_RV>,

    /// Remote Rendezvous instances (via control-plane adapters).
    remotes: ArrayMap<RendezvousId, ControlPlaneSlot<'cfg>, MAX_RV>,

    /// Distributed splice state tracking.
    splice_state: DistributedSpliceState<MAX_RV>,

    /// Cached operands staged between minting intent and ack tokens.
    cached_operands: ArrayMap<SessionId, SpliceOperands, MAX_CACHED_SPLICES>,

    /// Number of active lane leases (affine witness count).
    active_leases: core::cell::Cell<u32>,

    /// Cached management manager state per rendezvous.
    mgmt_managers: ArrayMap<RendezvousId, MgmtManager, MAX_RV>,

    /// Pending delegated port claims keyed by rendezvous/session/lane.
    delegated_ports: DelegatedPortTable,
}

impl<'cfg, T, U, C, E, const MAX_RV: usize> ControlCore<'cfg, T, U, C, E, MAX_RV>
where
    T: crate::transport::Transport,
    U: crate::runtime::consts::LabelUniverse,
    C: crate::runtime::config::Clock,
    E: crate::control::cap::EpochTable,
{
    fn issue_delegated_witness(&mut self, key: DelegatedPortKey) -> Option<DelegatedPortWitness> {
        self.delegated_ports
            .get_mut(&key)
            .and_then(|slot| slot.issue())
    }

    fn take_delegated_claim(
        &mut self,
        key: DelegatedPortKey,
    ) -> Result<DelegatedPortClaimGuard, DelegationError> {
        match self.delegated_ports.get_mut(&key) {
            Some(slot) => slot.claim(),
            None => Err(DelegationError::InvalidToken),
        }
    }

    fn has_delegated_claim(&self, key: DelegatedPortKey) -> bool {
        self.delegated_ports
            .get(&key)
            .map(DelegatedPortSlot::has_pending)
            .unwrap_or(false)
    }

    fn revoke_delegated_witness(&mut self, key: DelegatedPortKey) {
        if let Some(slot) = self.delegated_ports.get_mut(&key) {
            slot.revoke();
        }
    }
}

struct ResolverCore<'cfg, T, U, C, const MAX_RV: usize>
where
    T: crate::transport::Transport,
    U: crate::runtime::consts::LabelUniverse,
    C: crate::runtime::config::Clock,
{
    table: ArrayMap<
        DynamicResolverKey,
        DynamicResolverEntry<'cfg, T, U, C, MAX_RV>,
        HANDLE_RESOLVER_SLOTS,
    >,
}

/// SessionCluster - Distributed control-plane coordinator with interior mutability.
///
/// Uses `UnsafeCell` to allow `&self` methods while maintaining mutable internal state.
/// This enables `LaneLease` to hold `PhantomData<&'cluster SessionCluster>` (shared reference)
/// without blocking other cluster operations.
///
/// # Safety
///
/// All mutable access to the control or resolver tables goes through
/// `with_control_mut()` / `with_resolvers_mut()`, which enforce:
/// - Single writer at a time (Rust's `&mut` semantics within the closure scope)
/// - Documented invariants (see `ControlCore`)
/// - TAP event monitoring for lane lifecycle
pub struct SessionCluster<'cfg, T, U, C, const MAX_RV: usize>
where
    T: crate::transport::Transport,
    U: crate::runtime::consts::LabelUniverse,
    C: crate::runtime::config::Clock,
{
    /// Control-plane state guarded by interior mutability.
    control:
        core::cell::UnsafeCell<ControlCore<'cfg, T, U, C, crate::control::cap::EpochInit, MAX_RV>>,
    /// Dynamic resolver table separated from core control state.
    resolvers: core::cell::UnsafeCell<ResolverCore<'cfg, T, U, C, MAX_RV>>,
    /// Clock for timestamping tap events.
    clock: &'cfg C,
}

/// Errors raised while attaching cursor endpoints.
#[derive(Debug)]
pub enum AttachError {
    Control(CpError),
    Rendezvous(crate::rendezvous::RendezvousError),
}

impl From<CpError> for AttachError {
    fn from(err: CpError) -> Self {
        AttachError::Control(err)
    }
}

impl From<crate::rendezvous::RendezvousError> for AttachError {
    fn from(err: crate::rendezvous::RendezvousError) -> Self {
        AttachError::Rendezvous(err)
    }
}

impl<'cfg, T, U, C, const MAX_RV: usize> SessionCluster<'cfg, T, U, C, MAX_RV>
where
    T: crate::transport::Transport,
    U: crate::runtime::consts::LabelUniverse + 'cfg,
    C: crate::runtime::config::Clock + 'cfg,
{
    /// Create a new empty cluster with the given clock.
    pub fn new(clock: &'cfg C) -> Self {
        Self {
            control: core::cell::UnsafeCell::new(ControlCore {
                locals: LeaseControlCore::new(),
                remotes: ArrayMap::new(),
                splice_state: DistributedSpliceState::new(),
                cached_operands: ArrayMap::new(),
                active_leases: core::cell::Cell::new(0),
                mgmt_managers: ArrayMap::new(),
                delegated_ports: DelegatedPortTable::new(),
            }),
            resolvers: core::cell::UnsafeCell::new(ResolverCore {
                table: ArrayMap::new(),
            }),
            clock,
        }
    }

    /// Internal helper to access mutable control core (NOT PUBLIC).
    fn with_control_mut<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut ControlCore<'cfg, T, U, C, crate::control::cap::EpochInit, MAX_RV>) -> R,
    {
        unsafe { f(&mut *self.control.get()) }
    }

    /// Internal helper to access mutable resolver state.
    fn with_resolvers_mut<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut ResolverCore<'cfg, T, U, C, MAX_RV>) -> R,
    {
        unsafe { f(&mut *self.resolvers.get()) }
    }

    pub fn delegated_witness(
        &self,
        rv_id: RendezvousId,
        sid: SessionId,
        lane: LaneId,
    ) -> Option<DelegatedPortWitness> {
        let key = DelegatedPortKey::new(rv_id, sid, lane);
        self.with_control_mut(|core| core.issue_delegated_witness(key))
    }

    pub fn has_delegated_claim(&self, rv_id: RendezvousId, sid: SessionId, lane: LaneId) -> bool {
        let key = DelegatedPortKey::new(rv_id, sid, lane);
        self.with_control_mut(|core| core.has_delegated_claim(key))
    }

    pub fn delegate_claim(
        &self,
        rv_id: RendezvousId,
        token: GenericCapToken<EndpointResource>,
    ) -> Result<crate::endpoint::delegate::DelegatedPortClaim<'_, 'cfg, T, U, C, MAX_RV>, CpError>
    {
        let cp_sid = SessionId::new(token.sid().raw());
        let cp_lane = LaneId::new(token.lane().raw());
        self.claim_delegation_token(rv_id, token)?;
        let witness = self
            .delegated_witness(rv_id, cp_sid, cp_lane)
            .ok_or(CpError::Delegation(DelegationError::InvalidToken))?;
        Ok(crate::endpoint::delegate::DelegatedPortClaim::new(
            self, rv_id, cp_sid, cp_lane, witness,
        ))
    }

    pub(crate) fn revoke_delegated_witness(
        &self,
        rv_id: RendezvousId,
        sid: SessionId,
        lane: LaneId,
    ) {
        let key = DelegatedPortKey::new(rv_id, sid, lane);
        self.with_control_mut(|core| core.revoke_delegated_witness(key));
    }

    pub fn consume_delegated_claim(
        &self,
        rv_id: RendezvousId,
        sid: SessionId,
        lane: LaneId,
        witness: DelegatedPortWitness,
    ) -> Result<VerifiedCap<EndpointResource>, DelegationError> {
        let key = DelegatedPortKey::new(rv_id, sid, lane);
        let result = self.with_control_mut(|core| {
            core.take_delegated_claim(key)
                .map(|claim| claim.into_verified())
        });
        let _ = witness;
        result
    }

    pub(crate) fn attach_delegated_cursor<'lease, const ROLE: u8, LocalSteps, Mint>(
        &'lease self,
        rv_id: RendezvousId,
        sid: SessionId,
        lane: LaneId,
        program: &'static crate::g::RoleProgram<'static, ROLE, LocalSteps, Mint>,
        witness: DelegatedPortWitness,
    ) -> Result<
        crate::endpoint::CursorEndpoint<
            'cfg,
            ROLE,
            T,
            U,
            C,
            crate::control::cap::EpochInit,
            MAX_RV,
            Mint,
        >,
        AttachError,
    >
    where
        'cfg: 'lease,
        'lease: 'cfg,
        Mint: MintConfigMarker,
    {
        let clock = self.clock;
        let key = DelegatedPortKey::new(rv_id, sid, lane);
        let cursor = program.phase_cursor();
        let mint = program.mint_config();
        let lane_wire = crate::rendezvous::Lane::new(lane.raw());
        let sid_rv = crate::rendezvous::SessionId::new(sid.raw());
        let lane_rv = crate::rendezvous::Lane::new(lane.raw());

        // SAFETY: Exclusive access is guaranteed by &self usage pattern (identical to lease_port).
        let core = unsafe { &mut *self.control.get() };

        let claim_guard = core
            .take_delegated_claim(key)
            .map_err(|err| AttachError::Control(CpError::Delegation(err)))?;

        let mut lease = match core.locals.lease::<FullSpec>(rv_id) {
            Ok(lease) => lease,
            Err(LeaseError::UnknownRendezvous(_)) => {
                drop(claim_guard);
                return Err(AttachError::Control(CpError::RendezvousMismatch {
                    expected: rv_id.raw(),
                    actual: 0,
                }));
            }
            Err(LeaseError::AlreadyLeased(_)) => {
                drop(claim_guard);
                return Err(AttachError::Rendezvous(
                    crate::rendezvous::RendezvousError::LaneBusy { lane: lane_rv },
                ));
            }
        };

        let (lane_key, rv_ptr): (
            crate::control::cap::LaneKey<'cfg>,
            *mut crate::rendezvous::Rendezvous<'cfg, 'cfg, T, U, C, crate::control::cap::EpochInit>,
        ) = lease.with_full(|rv| {
            let key = crate::control::cap::LaneKey::new(rv.brand(), lane_rv);
            (key, rv as *mut _)
        });
        let rv = unsafe { &*rv_ptr };

        let raw_port = match rv.attach_verified(claim_guard.verified()) {
            Ok(port) => port,
            Err(err) => {
                drop(claim_guard);
                return Err(AttachError::Rendezvous(err));
            }
        };

        claim_guard.commit();

        let active = &core.active_leases;
        active.set(active.get() + 1);

        lease.emit_lane_acquire(clock.now32(), rv_id, sid_rv, lane_rv);

        let mut guard = LaneGuard::new(lease, lane_rv, active, true);

        guard.detach_lease();

        let port = unsafe {
            mem::transmute::<
                crate::rendezvous::Port<'_, T, crate::control::cap::EpochInit>,
                crate::rendezvous::Port<'cfg, T, crate::control::cap::EpochInit>,
            >(raw_port)
        };

        let control_ctx =
            crate::endpoint::control::SessionControlCtx::new(rv_id, lane_wire, Some(self), None);

        let owner = crate::control::cap::Owner::new(lane_key);
        let epoch = crate::control::cap::EndpointEpoch::new();
        let _ = witness;

        // Build ports/guards arrays with this single lane
        let lane_idx = lane_rv.as_wire() as usize;
        let mut ports = [None, None, None, None, None, None, None, None];
        let mut guards = [None, None, None, None, None, None, None, None];
        ports[lane_idx] = Some(port);
        guards[lane_idx] = Some(guard);

        Ok(crate::endpoint::CursorEndpoint::from_parts(
            ports,
            guards,
            lane_idx,
            sid_rv,
            owner,
            epoch,
            cursor,
            control_ctx,
            mint,
            crate::binding::NoBinding,
        ))
    }

    /// Add a local Rendezvous instance to the cluster (takes ownership).
    ///
    /// SessionCluster takes ownership of the Rendezvous, ensuring proper RAII:
    /// - Drop order: SessionCluster → Rendezvous → LaneLease
    /// - No self-referential lifetime issues
    /// - Type-level proof of affine resource management
    ///
    /// Returns the RendezvousId on success.
    ///
    /// # Errors
    ///
    /// Returns `CpError::ResourceExhausted` if the cluster is full or
    /// the ID is already registered.
    pub fn add_rendezvous(
        &self,
        rv: crate::rendezvous::Rendezvous<'cfg, 'cfg, T, U, C, crate::control::cap::EpochInit>,
    ) -> Result<RendezvousId, CpError> {
        self.with_control_mut(|core| match core.locals.register_local(rv) {
            Ok(id) => Ok(id),
            Err(RegisterRendezvousError::CapacityExceeded) => Err(CpError::ResourceExhausted),
            Err(RegisterRendezvousError::Duplicate(_)) => Err(CpError::ResourceExhausted),
        })
    }

    /// Emit an accept-time tap with packed ScopeTrace (range/nest) for observability alignment.
    pub fn emit_accept_tap(
        &self,
        rv_id: RendezvousId,
        sid: SessionId,
        lane: LaneId,
        scope: crate::global::const_dsl::ScopeId,
    ) {
        self.with_control_mut(|core| {
            if let Some(rv) = core.locals.get_mut(&rv_id) {
                let tap = rv.observe_facet();
                let ts = rv.now32();
                let causal = ((lane.as_wire() as u16) << 8) | 0;
                let event = crate::observe::TapEvent {
                    ts,
                    id: crate::observe::ids::ENDPOINT_CONTROL,
                    causal_key: causal,
                    arg0: sid.raw(),
                    arg1: scope.pack_range_nest(),
                    arg2: scope.pack_range_nest(),
                };
                tap.emit(event);
            }
        });
    }

    /// Register a remote Rendezvous via a control-plane adapter.
    ///
    /// Returns an error if the cluster is full or the ID is already registered.
    pub fn register_remote(&self, slot: ControlPlaneSlot<'cfg>) -> Result<(), CpError> {
        let id = slot.adapter().peer();
        self.with_control_mut(|core| {
            core.remotes
                .insert(id, slot)
                .map_err(|_| CpError::ResourceExhausted)
        })
    }

    /// Get a local Rendezvous by ID.
    ///
    /// # Safety
    ///
    /// Returns a shared reference to the Rendezvous. Caller must ensure
    /// no concurrent mutation through `with_control_mut`.
    pub fn get_local(
        &self,
        id: &RendezvousId,
    ) -> Option<&crate::rendezvous::Rendezvous<'cfg, 'cfg, T, U, C, crate::control::cap::EpochInit>>
    {
        // SAFETY: We're returning a shared reference from UnsafeCell.
        // This is safe because:
        // - The reference is borrowed from `&self`, so it can't outlive the cluster
        // - Caller must not call mutable methods while holding this reference
        // - This pattern is documented in SessionCluster's safety contract
        unsafe { (*self.control.get()).locals.get(id) }
    }

    /// Get a remote Rendezvous adapter by ID.
    pub fn get_remote(&self, id: &RendezvousId) -> Option<&ControlPlaneSlot<'cfg>> {
        // SAFETY: Same as get_local - returning shared reference from UnsafeCell
        unsafe { (*self.control.get()).remotes.get(id) }
    }

    pub(crate) fn canonical_session_token<K, Mint>(
        &self,
        rv_id: RendezvousId,
        sid: crate::rendezvous::SessionId,
        lane: crate::rendezvous::Lane,
        dest_role: u8,
        shot: CapShot,
        mint: Mint,
    ) -> Option<GenericCapToken<K>>
    where
        K: SessionScopedKind,
        Mint: MintConfigMarker,
        Mint::Policy: AllowsCanonical,
    {
        let handle = K::handle_for_session(sid, lane);
        self.canonical_token_with_handle::<K, Mint>(rv_id, sid, lane, dest_role, shot, handle, mint)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn canonical_token_with_handle<K, Mint>(
        &self,
        rv_id: RendezvousId,
        sid: crate::rendezvous::SessionId,
        lane: crate::rendezvous::Lane,
        dest_role: u8,
        shot: CapShot,
        handle: K::Handle,
        mint: Mint,
    ) -> Option<GenericCapToken<K>>
    where
        K: ResourceKind,
        Mint: MintConfigMarker,
        Mint::Policy: AllowsCanonical,
    {
        let seed = DelegateMintSeed::<K, Mint> {
            sid,
            lane,
            dest_role,
            shot,
            handle,
            mint,
        };
        let links = Self::collect_delegation_links::<K>(&seed.handle);

        let mint_needs = FacetCapsDelegation::NEEDS;

        let outcome = self.drive::<DelegateMintAutomaton<K, Mint>, _, _>(
            rv_id,
            seed,
            |core, rv| {
                let mut ctx = Self::init_bundle_context_with_needs(core, rv, mint_needs);
                let table_ptr = NonNull::from(&mut core.delegated_ports);
                ctx.set_delegation(DelegationGraphContext::with_table(table_ptr));
                ctx
            },
            move |core, graph| Self::init_delegation_children(core, graph, rv_id, &links),
        );

        outcome.ok()
    }

    fn claim_delegation_token(
        &self,
        rv_id: RendezvousId,
        token: CapToken,
    ) -> Result<DelegatedPortWitness, CpError> {
        let (sid, lane) = CpCommand::derive_sid_lane(token);
        let key = DelegatedPortKey::new(rv_id, sid, lane);
        let seed = DelegateClaimSeed { token };
        let links = Self::collect_claim_links(&seed.token);
        let claim_needs = FacetCapsDelegation::NEEDS;

        match self.drive::<DelegateClaimAutomaton, _, _>(
            rv_id,
            seed,
            |core, rv| {
                let mut ctx = Self::init_bundle_context_with_needs(core, rv, claim_needs);
                let table_ptr = NonNull::from(&mut core.delegated_ports);
                ctx.set_delegation(DelegationGraphContext::for_claim(table_ptr, key));
                ctx
            },
            move |core, graph| Self::init_delegation_children(core, graph, rv_id, &links),
        ) {
            Ok(outcome) => Ok(outcome),
            Err(DelegationDriveError::Lease(_) | DelegationDriveError::Graph(_)) => {
                Err(CpError::Delegation(DelegationError::InvalidToken))
            }
            Err(DelegationDriveError::Automaton(err)) => Err(err.into()),
        }
    }

    /// **Acquire a lane lease (RAII handle bound to this cluster).**
    ///
    /// Returns a `LaneLease` that borrows this cluster and automatically releases
    /// the lane on Drop.
    ///
    /// # Safety Invariants
    ///
    /// - Cluster must not move while lease is held (ensured by PhantomData)
    /// - Only one lease per (rv_id, lane) pair at a time
    /// - Rendezvous write access forbidden while lease held
    ///
    /// # Tap Events
    ///
    /// Emits `LANE_ACQUIRE` with:
    /// - `arg0`: Rendezvous ID (u32)
    /// - `arg1`: Packed session/lane (u32)
    pub fn lease_port(
        &self,
        rv_id: RendezvousId,
        sid: crate::rendezvous::SessionId,
        lane: crate::rendezvous::Lane,
        role: u8,
    ) -> Result<
        crate::rendezvous::LaneLease<'cfg, T, U, C, MAX_RV>,
        crate::rendezvous::RendezvousError,
    > {
        // SAFETY: exclusive access is guaranteed by &self; we immediately move the
        // resulting rendezvous lease out, so no aliasing occurs.
        let core = unsafe { &mut *self.control.get() };

        let mut lease = match core.locals.lease::<FullSpec>(rv_id) {
            Ok(lease) => lease,
            Err(LeaseError::UnknownRendezvous(_)) => {
                return Err(crate::rendezvous::RendezvousError::LaneOutOfRange { lane });
            }
            Err(LeaseError::AlreadyLeased(_)) => {
                return Err(crate::rendezvous::RendezvousError::LaneBusy { lane });
            }
        };

        let active = &core.active_leases;

        let current = active.get();
        active.set(current + 1);

        // Extract lane key before moving lease into guard and emit acquire tap.
        let lane_key = lease.lane_key(lane);
        lease.emit_lane_acquire(self.clock.now32(), rv_id, sid, lane);

        let guard = LaneGuard::new(lease, lane, active, true);

        Ok(crate::rendezvous::LaneLease::new(
            guard, sid, lane, role, lane_key,
        ))
    }

    /// Perform a handshake with a remote Rendezvous.
    ///
    /// This establishes the connection and negotiates protocol version.
    pub fn handshake(&self, remote_id: RendezvousId, hello: &Hello) -> Result<Hello, CpError> {
        let slot = self
            .get_remote(&remote_id)
            .ok_or(CpError::RendezvousMismatch {
                expected: remote_id.raw(),
                actual: 0,
            })?;

        slot.adapter().handshake(hello)
    }

    /// Execute a control-plane effect on a specific Rendezvous.
    ///
    /// If the Rendezvous is remote, this sends the effect via the adapter.
    /// If it's local, this executes the effect directly.
    pub(crate) fn run_effect_step(
        &self,
        target: RendezvousId,
        envelope: CpCommand,
    ) -> Result<PendingEffect, CpError> {
        // Try remote first
        if let Some(slot) = self.get_remote(&target) {
            let run_result = slot.adapter().run(envelope);
            run_result?;
            return Ok(PendingEffect::None);
        }

        if self.get_local(&target).is_some() {
            match envelope.effect {
                CpEffect::SpliceBegin => {
                    if let Some(lane_id) = envelope.lane
                        && let Some(rv) = self.get_local(&target)
                    {
                        let lane = crate::rendezvous::Lane::new(lane_id.raw());
                        let caps = rv.caps_mask_for_lane(lane);
                        if !caps.allows(CpEffect::SpliceBegin) {
                            return Err(CpError::Authorisation {
                                effect: CpEffect::SpliceBegin,
                            });
                        }
                    }

                    let intent = envelope
                        .intent
                        .ok_or(CpError::Splice(CpSpliceError::InvalidState))?;
                    let seed = intent;
                    let dst_rv = seed.dst_rv;

                    let begin_needs = FacetCapsSplice::NEEDS;

                    let drive_result = self.drive::<SpliceBeginAutomaton, _, _>(
                        target,
                        seed,
                        move |core, rv| {
                            let mut ctx =
                                Self::init_bundle_context_with_needs(core, rv, begin_needs);
                            ctx.set_splice(SpliceGraphContext::new(Some(seed)));
                            ctx
                        },
                        |core, graph| {
                            if dst_rv != target && begin_needs.requires_splice() {
                                graph.add_child_with_bundle_config(
                                    &mut core.locals,
                                    target,
                                    dst_rv,
                                    |child_ctx| {
                                        child_ctx.set_splice(SpliceGraphContext::default());
                                    },
                                )?;
                            }
                            Ok(())
                        },
                    );

                    if let Err(err) = drive_result {
                        return Err(match err {
                            DelegationDriveError::Lease(_) | DelegationDriveError::Graph(_) => {
                                CpError::Splice(CpSpliceError::InvalidState)
                            }
                            DelegationDriveError::Automaton(err) => err.into(),
                        });
                    }

                    return self.after_local_effect(envelope);
                }
                CpEffect::SpliceCommit => {
                    let sid = envelope
                        .sid
                        .ok_or(CpError::Splice(CpSpliceError::InvalidSession))?;
                    let ack = envelope
                        .ack
                        .ok_or(CpError::Splice(CpSpliceError::InvalidState))?;
                    let cached_intent = {
                        let ack_for_cache = ack;
                        self.with_control_mut(|core| {
                            core.locals.get_mut(&target).and_then(|rv| {
                                let session = crate::rendezvous::SessionId::new(ack_for_cache.sid);
                                let dst = crate::rendezvous::RendezvousId::new(
                                    ack_for_cache.dst_rv.raw(),
                                );
                                rv.take_cached_distributed_intent(session, dst)
                            })
                        })
                        .or_else(|| self.distributed_operands(sid).map(|ops| ops.intent(sid)))
                    };

                    let dst_rv = ack.dst_rv;

                    let commit_needs = FacetCapsSplice::NEEDS;

                    let drive_result = self.drive::<SpliceCommitAutomaton, _, _>(
                        target,
                        ack,
                        move |core, rv| {
                            let mut ctx =
                                Self::init_bundle_context_with_needs(core, rv, commit_needs);
                            ctx.set_splice(SpliceGraphContext::new(cached_intent));
                            ctx
                        },
                        |core, graph| {
                            if dst_rv != target && commit_needs.requires_splice() {
                                graph.add_child_with_bundle_config(
                                    &mut core.locals,
                                    target,
                                    dst_rv,
                                    |child_ctx| {
                                        child_ctx.set_splice(SpliceGraphContext::default());
                                    },
                                )?;
                            }
                            Ok(())
                        },
                    );

                    if let Err(err) = drive_result {
                        return Err(match err {
                            DelegationDriveError::Lease(_) | DelegationDriveError::Graph(_) => {
                                CpError::Splice(CpSpliceError::InvalidState)
                            }
                            DelegationDriveError::Automaton(err) => err.into(),
                        });
                    }

                    return self.after_local_effect(envelope);
                }
                CpEffect::Delegate => {
                    let delegate = envelope
                        .delegate
                        .ok_or(CpError::Delegation(DelegationError::InvalidToken))?;
                    match delegate.phase {
                        DelegatePhase::Claim => {
                            self.claim_delegation_token(target, delegate.token)?;
                            return self.after_local_effect(envelope);
                        }
                        DelegatePhase::Mint => {
                            // Fall through to direct rendezvous execution for mint path.
                        }
                    }
                }
                _ => {
                    if let Some(rv) = self.get_local(&target) {
                        if let Some(lane_id) = envelope.lane {
                            let lane = crate::rendezvous::Lane::new(lane_id.raw());
                            let caps = rv.caps_mask_for_lane(lane);
                            if !caps.allows(envelope.effect)
                                && !matches!(
                                    envelope.effect,
                                    CpEffect::SpliceAck | CpEffect::SpliceCommit
                                )
                            {
                                return Err(CpError::Authorisation {
                                    effect: envelope.effect,
                                });
                            }
                        }
                        use EffectExecutor as _;
                        let run_result = rv.run_effect(envelope.clone());
                        run_result?;
                        return self.after_local_effect(envelope);
                    }
                }
            }
        }

        Err(CpError::RendezvousMismatch {
            expected: target.raw(),
            actual: 0,
        })
    }

    pub(crate) fn run_effect(
        &self,
        target: RendezvousId,
        envelope: CpCommand,
    ) -> Result<(), CpError> {
        let mut next_target = target;
        let mut next_envelope = envelope;
        let mut pending = self.run_effect_step(next_target, next_envelope)?;

        while let PendingEffect::Dispatch { target, envelope } = pending {
            next_target = target;
            next_envelope = envelope;
            let step_result = self.run_effect_step(next_target, next_envelope.clone());
            pending = step_result?;
        }

        Ok(())
    }

    pub(crate) fn distributed_operands(&self, sid: SessionId) -> Option<SpliceOperands> {
        self.with_control_mut(|core| {
            core.splice_state
                .get(sid)
                .copied()
                .or_else(|| core.cached_operands.get(&sid).copied())
        })
    }

    fn cache_splice_operands(
        &self,
        sid: SessionId,
        operands: SpliceOperands,
    ) -> Result<(), CpError> {
        self.with_control_mut(|core| {
            core.cached_operands
                .insert(sid, operands)
                .map_err(|_| CpError::ResourceExhausted)
        })
    }

    fn dynamic_resolver(
        &self,
        key: DynamicResolverKey,
    ) -> Option<&DynamicResolverEntry<'cfg, T, U, C, MAX_RV>> {
        unsafe { (*self.resolvers.get()).table.get(&key) }
    }

    pub fn register_control_plan_resolver(
        &self,
        rv_id: RendezvousId,
        info: &crate::global::role_program::ControlPlanInfo,
        resolver: DynamicResolverFn<'cfg, T, U, C, MAX_RV>,
    ) -> Result<(), CpError> {
        let tag = info
            .resource_tag
            .ok_or(CpError::UnsupportedEffect(info.label))?;
        let key = DynamicResolverKey::new(rv_id, info.eff_index, tag);
        let plan = match info.plan {
            HandlePlan::Dynamic { .. } => {
                // Validate that the tag is a known dynamic control tag
                if !is_dynamic_control_tag(tag) {
                    return Err(CpError::UnsupportedEffect(tag));
                }
                info.plan
            }
            _ => return Err(CpError::UnsupportedEffect(tag)),
        };
        let entry = DynamicResolverEntry {
            resolver,
            plan,
            scope_trace: info.scope_trace,
        };
        self.with_resolvers_mut(|core| {
            core.table
                .insert(key, entry)
                .map_err(|_| CpError::ResourceExhausted)
        })
    }

    pub(crate) fn resolve_dynamic_plan(
        &self,
        rv_id: RendezvousId,
        session: Option<SessionId>,
        lane: LaneId,
        eff_index: EffIndex,
        tag: u8,
        metrics: TransportSnapshot,
        transport_ctx: crate::transport::context::ContextSnapshot,
    ) -> Result<DynamicResolution, CpError> {
        let key = DynamicResolverKey::new(rv_id, eff_index, tag);
        let entry = self
            .dynamic_resolver(key)
            .ok_or_else(|| {
                CpError::PolicyAbort { reason: 0 }
            })?;
        let plan = entry.plan;

        let (policy_id, meta) = plan
            .dynamic_components()
            .ok_or(CpError::PolicyAbort { reason: 6 })?;

        let scope_hint = plan.scope();

        let ctx = ResolverContext {
            rv_id,
            session,
            lane,
            eff_index,
            tag,
            metrics,
            scope_id: scope_hint,
            scope_trace: entry.scope_trace,
            transport_ctx,
        };

        let resolution = (entry.resolver)(self, &meta, ctx)
            .map_err(|_| {
                CpError::PolicyAbort { reason: policy_id }
            })?;

        // The resource tag determines the expected resolution type
        match (tag, resolution) {
            (
                SpliceIntentKind::TAG | SpliceAckKind::TAG,
                DynamicResolution::Splice {
                    dst_rv,
                    dst_lane,
                    fences,
                },
            ) => Ok(DynamicResolution::Splice {
                dst_rv,
                dst_lane,
                fences,
            }),
            (
                RerouteKind::TAG,
                DynamicResolution::Reroute {
                    dst_rv,
                    dst_lane,
                    shard,
                },
            ) => Ok(DynamicResolution::Reroute {
                dst_rv,
                dst_lane,
                shard: shard.or(meta.shard_hint),
            }),
            (
                LoopContinueKind::TAG | LoopBreakKind::TAG | RouteDecisionKind::TAG,
                DynamicResolution::RouteArm { arm },
            ) => {
                if scope_hint.is_none() {
                    return Err(CpError::PolicyAbort { reason: policy_id });
                }
                Ok(DynamicResolution::RouteArm { arm })
            }
            // Loop resolution is used for LoopContinue/LoopBreak route decisions.
            (
                LoopContinueKind::TAG | LoopBreakKind::TAG | RouteDecisionKind::TAG,
                DynamicResolution::Loop { decision },
            ) => {
                if scope_hint.is_none() {
                    return Err(CpError::PolicyAbort { reason: policy_id });
                }
                Ok(DynamicResolution::Loop { decision })
            }
            _ => Err(CpError::PolicyAbort { reason: policy_id }),
        }
    }

    pub(crate) fn control_plan_for(
        &self,
        rv_id: RendezvousId,
        lane: LaneId,
        eff_index: EffIndex,
        tag: u8,
    ) -> Result<HandlePlan, CpError> {
        let rv = self.get_local(&rv_id).ok_or(CpError::RendezvousMismatch {
            expected: rv_id.raw(),
            actual: 0,
        })?;
        let lane_rv = crate::rendezvous::Lane::new(lane.raw());
        let key = DynamicResolverKey::new(rv_id, eff_index, tag);
        let plan = rv
            .control_plan(lane_rv, eff_index, tag)
            .or_else(|| {
                self.dynamic_resolver(key).map(|entry| entry.plan)
            });
        Ok(plan.unwrap_or(HandlePlan::None))
    }

    pub(crate) fn prepare_splice_operands_from_plan(
        &self,
        rv_id: RendezvousId,
        sid: SessionId,
        src_lane: LaneId,
        eff_index: EffIndex,
        tag: u8,
        plan: HandlePlan,
        metrics: TransportSnapshot,
        transport_ctx: crate::transport::context::ContextSnapshot,
    ) -> Result<SpliceOperands, CpError> {
        if self.get_local(&rv_id).is_none() {
            return Err(CpError::RendezvousMismatch {
                expected: rv_id.raw(),
                actual: 0,
            });
        }

        let plan_needs = facet_needs(tag, plan);
        let drive_prepare = |dst_rv: RendezvousId,
                             dst_lane: LaneId,
                             fences: Option<(u32, u32)>|
         -> Result<SpliceOperands, CpError> {
            let result = self.drive::<SplicePrepareAutomaton, _, _>(
                rv_id,
                SplicePrepareSeed {
                    sid,
                    src_lane,
                    dst_rv,
                    dst_lane,
                    fences,
                },
                |core, rv| {
                    let mut ctx = Self::init_bundle_context_with_needs(core, rv, plan_needs);
                    ctx.set_splice(SpliceGraphContext::default());
                    ctx
                },
                |core, graph| {
                    if dst_rv != rv_id && plan_needs.requires_splice() {
                        graph.add_child_with_bundle_config(
                            &mut core.locals,
                            rv_id,
                            dst_rv,
                            |child_ctx| {
                                child_ctx.set_splice(SpliceGraphContext::default());
                            },
                        )?;
                    }
                    Ok(())
                },
            );
            match result {
                Ok(operands) => Ok(operands),
                Err(DelegationDriveError::Lease(_)) | Err(DelegationDriveError::Graph(_)) => {
                    Err(CpError::Splice(CpSpliceError::InvalidState))
                }
                Err(DelegationDriveError::Automaton(err)) => Err(err),
            }
        };

        let operands = match plan {
            HandlePlan::Static {
                kind: StaticPlanKind::SpliceLocal { dst_lane },
            } => {
                let dst_lane_id = LaneId::new(dst_lane as u32);
                drive_prepare(rv_id, dst_lane_id, None)?
            }
            HandlePlan::Dynamic { .. } => {
                let policy_id = plan.dynamic_components().map(|(id, _)| id).unwrap_or(0);
                let resolution =
                    self.resolve_dynamic_plan(rv_id, Some(sid), src_lane, eff_index, tag, metrics, transport_ctx)?;
                let (dst_rv, dst_lane, fences) = match resolution {
                    DynamicResolution::Splice {
                        dst_rv,
                        dst_lane,
                        fences,
                    } => (dst_rv, dst_lane, fences),
                    _ => return Err(CpError::PolicyAbort { reason: policy_id }),
                };
                let result = drive_prepare(dst_rv, dst_lane, fences);
                result?
            }
            HandlePlan::None
            | HandlePlan::Static {
                kind: StaticPlanKind::RerouteLocal { .. },
            } => {
                return Err(CpError::UnsupportedEffect(CpEffect::SpliceBegin as u8));
            }
        };

        self.cache_splice_operands(sid, operands)?;
        Ok(operands)
    }

    pub(crate) fn prepare_reroute_handle_from_plan(
        &self,
        rv_id: RendezvousId,
        lane: LaneId,
        eff_index: EffIndex,
        tag: u8,
        plan: HandlePlan,
        metrics: TransportSnapshot,
        transport_ctx: crate::transport::context::ContextSnapshot,
    ) -> Result<RerouteHandle, CpError> {
        let src_lane_u16 = lane.raw() as u16;
        match plan {
            HandlePlan::Static {
                kind: StaticPlanKind::RerouteLocal { dst_lane, shard },
            } => Ok(RerouteHandle::new(
                rv_id.raw(),
                rv_id.raw(),
                src_lane_u16,
                dst_lane,
                0,
                0,
                shard,
                0,
            )),
            HandlePlan::Dynamic { .. } => {
                let (policy_id, meta) = plan
                    .dynamic_components()
                    .ok_or(CpError::PolicyAbort { reason: 6 })?;
                let resolution =
                    self.resolve_dynamic_plan(rv_id, None, lane, eff_index, tag, metrics, transport_ctx)?;
                let (dst_rv, dst_lane, shard_override) = match resolution {
                    DynamicResolution::Reroute {
                        dst_rv,
                        dst_lane,
                        shard,
                    } => (dst_rv, dst_lane, shard),
                    _ => return Err(CpError::PolicyAbort { reason: policy_id }),
                };
                let shard = shard_override.or(meta.shard_hint).unwrap_or_default();
                Ok(RerouteHandle::new(
                    rv_id.raw(),
                    dst_rv.raw(),
                    src_lane_u16,
                    dst_lane.raw() as u16,
                    0,
                    0,
                    shard,
                    0,
                ))
            }
            HandlePlan::None
            | HandlePlan::Static {
                kind: StaticPlanKind::SpliceLocal { .. },
            } => Err(CpError::UnsupportedEffect(CpEffect::Delegate as u8)),
        }
    }

    pub(crate) fn prepare_route_decision_from_plan(
        &self,
        rv_id: RendezvousId,
        lane: LaneId,
        eff_index: EffIndex,
        tag: u8,
        plan: HandlePlan,
        metrics: TransportSnapshot,
        transport_ctx: crate::transport::context::ContextSnapshot,
    ) -> Result<RouteDecisionHandle, CpError> {
        match plan {
            HandlePlan::Dynamic { .. } => {
                let policy_id = plan.dynamic_components().map(|(id, _)| id).unwrap_or(0);
                let scope = plan.scope();
                if scope.is_none() {
                    return Err(CpError::PolicyAbort { reason: policy_id });
                }
                let resolution =
                    self.resolve_dynamic_plan(rv_id, None, lane, eff_index, tag, metrics, transport_ctx)?;
                match resolution {
                    DynamicResolution::RouteArm { arm } => Ok(RouteDecisionHandle::new(scope, arm)),
                    _ => Err(CpError::PolicyAbort { reason: policy_id }),
                }
            }
            HandlePlan::None | HandlePlan::Static { .. } => {
                Err(CpError::UnsupportedEffect(CpEffect::Delegate as u8))
            }
        }
    }

    pub(crate) fn take_cached_splice_operands(&self, sid: SessionId) -> Option<SpliceOperands> {
        self.with_control_mut(|core| core.cached_operands.remove(&sid))
    }

    fn dispatch_splice_intent_with_view(
        &self,
        rv_id: RendezvousId,
        cp_sid: SessionId,
        cp_lane: LaneId,
        view: crate::control::cap::HandleView<'_, SpliceIntentKind>,
        generation: Option<Gen>,
    ) -> Result<(), CpError> {
        let handle = *view.handle();

        if handle.src_rv == 0 || handle.dst_rv == 0 {
            return Err(CpError::Authorisation {
                effect: CpEffect::SpliceBegin,
            });
        }

        let operands = splice_operands_from_handle(handle);

        if cp_lane != operands.src_lane {
            return Err(CpError::Authorisation {
                effect: CpEffect::SpliceBegin,
            });
        }

        if let Some(header_gen) = generation
            && header_gen != operands.new_gen
        {
            return Err(CpError::Splice(CpSpliceError::GenerationMismatch));
        }

        if rv_id != operands.src_rv {
            return Err(CpError::RendezvousMismatch {
                expected: operands.src_rv.raw(),
                actual: rv_id.raw(),
            });
        }

        if let Some(rv) = self.get_local(&operands.src_rv) {
            let lane = crate::rendezvous::Lane::new(operands.src_lane.raw());
            let current = rv.caps_mask_for_lane(lane);
            if !current.allows(CpEffect::SpliceBegin) || !current.allows(CpEffect::SpliceCommit) {
                let required = current
                    .union(CapsMask::empty().with(CpEffect::SpliceBegin))
                    .union(CapsMask::empty().with(CpEffect::SpliceCommit));
                rv.set_caps_mask_for_lane(lane, required);
            }
        }

        if let Some(rv) = self.get_local(&operands.dst_rv) {
            let lane = crate::rendezvous::Lane::new(operands.dst_lane.raw());
            let current = rv.caps_mask_for_lane(lane);
            if !current.allows(CpEffect::SpliceAck) {
                let required = current.union(CapsMask::empty().with(CpEffect::SpliceAck));
                rv.set_caps_mask_for_lane(lane, required);
            }
        }

        let result = self.run_effect(operands.src_rv, CpCommand::splice_begin(cp_sid, operands));
        match result {
            Ok(()) => Ok(()),
            Err(CpError::Authorisation {
                effect: CpEffect::SpliceAck,
            }) => Ok(()),
            Err(err) => Err(err),
        }
    }

    fn dispatch_splice_ack_with_view(
        &self,
        rv_id: RendezvousId,
        cp_sid: SessionId,
        cp_lane: LaneId,
        view: crate::control::cap::HandleView<'_, SpliceAckKind>,
        generation: Option<Gen>,
    ) -> Result<(), CpError> {
        let handle = *view.handle();

        if handle.src_rv == 0 || handle.dst_rv == 0 {
            return Err(CpError::Authorisation {
                effect: CpEffect::SpliceAck,
            });
        }

        let operands = splice_operands_from_handle(handle);

        if cp_lane != operands.dst_lane {
            return Err(CpError::Authorisation {
                effect: CpEffect::SpliceAck,
            });
        }

        if let Some(header_gen) = generation
            && header_gen != operands.new_gen
        {
            return Err(CpError::Splice(CpSpliceError::GenerationMismatch));
        }

        if rv_id != operands.dst_rv {
            return Err(CpError::RendezvousMismatch {
                expected: operands.dst_rv.raw(),
                actual: rv_id.raw(),
            });
        }

        if let Some(rv) = self.get_local(&operands.dst_rv) {
            let lane = crate::rendezvous::Lane::new(operands.dst_lane.raw());
            let current = rv.caps_mask_for_lane(lane);
            if !current.allows(CpEffect::SpliceAck) {
                let mut ctx = self.with_control_mut(
                    |core| -> Result<
                        LeaseBundleContext<'cfg, 'cfg, T, U, C, crate::control::cap::EpochInit>,
                        CpError,
                    > {
                        let mut ctx = Self::init_bundle_context(core, operands.dst_rv);
                        if let Some(rv) = core.locals.get_mut(&operands.dst_rv) {
                            let lane = crate::rendezvous::Lane::new(operands.dst_lane.raw());
                            let current = rv.caps_mask_for_lane(lane);
                            if !current.allows(CpEffect::SpliceAck) {
                                if let Some(caps) = ctx.caps_mut() {
                                    caps.track_mask(lane, current)
                                        .map_err(|_| CpError::ResourceExhausted)?;
                                }
                                let required =
                                    current.union(CapsMask::empty().with(CpEffect::SpliceAck));
                                rv.set_caps_mask_for_lane(lane, required);
                            }
                        }
                        Ok(ctx)
                    },
                )?;

                match self.run_effect(operands.dst_rv, CpCommand::splice_ack(cp_sid, operands)) {
                    Ok(()) => {
                        ctx.on_commit();
                        return Ok(());
                    }
                    Err(err) => {
                        ctx.on_rollback();
                        return Err(err);
                    }
                }
            }
        }

        self.run_effect(operands.dst_rv, CpCommand::splice_ack(cp_sid, operands))
    }

    fn dispatch_cancel_begin_with_view(
        &self,
        rv_id: RendezvousId,
        cp_sid: SessionId,
        cp_lane: LaneId,
        view: crate::control::cap::HandleView<'_, CancelKind>,
        generation: Option<Gen>,
    ) -> Result<(), CpError> {
        let (sid_raw, lane_raw) = *view.handle();
        let handle_sid = SessionId::new(sid_raw);
        let handle_lane = LaneId::new(lane_raw as u32);
        if handle_sid != cp_sid || handle_lane != cp_lane {
            return Err(CpError::Authorisation {
                effect: CpEffect::CancelBegin,
            });
        }

        let effect_gen = generation.unwrap_or(Gen::ZERO);
        self.run_effect(rv_id, CpCommand::cancel_begin(cp_sid, cp_lane))?;
        self.run_effect(rv_id, CpCommand::cancel_ack(cp_sid, cp_lane, effect_gen))
    }

    fn dispatch_cancel_ack_with_view(
        &self,
        rv_id: RendezvousId,
        cp_sid: SessionId,
        cp_lane: LaneId,
        view: crate::control::cap::HandleView<'_, CancelAckKind>,
        generation: Option<Gen>,
    ) -> Result<(), CpError> {
        let (sid_raw, lane_raw) = *view.handle();
        let handle_sid = SessionId::new(sid_raw);
        let handle_lane = LaneId::new(lane_raw as u32);
        if handle_sid != cp_sid || handle_lane != cp_lane {
            return Err(CpError::Authorisation {
                effect: CpEffect::CancelAck,
            });
        }

        let effect_gen = generation.unwrap_or(Gen::ZERO);
        self.run_effect(rv_id, CpCommand::cancel_ack(cp_sid, cp_lane, effect_gen))
    }

    fn dispatch_checkpoint_with_view(
        &self,
        rv_id: RendezvousId,
        cp_sid: SessionId,
        cp_lane: LaneId,
        view: crate::control::cap::HandleView<'_, CheckpointKind>,
    ) -> Result<(), CpError> {
        let (sid_raw, lane_raw) = *view.handle();
        if SessionId::new(sid_raw) != cp_sid || LaneId::new(lane_raw as u32) != cp_lane {
            return Err(CpError::Authorisation {
                effect: CpEffect::Checkpoint,
            });
        }

        self.run_effect(rv_id, CpCommand::checkpoint(cp_sid, cp_lane))
    }

    fn dispatch_commit_with_view(
        &self,
        rv_id: RendezvousId,
        cp_sid: SessionId,
        cp_lane: LaneId,
        view: crate::control::cap::HandleView<'_, CommitKind>,
        generation: Option<Gen>,
    ) -> Result<(), CpError> {
        let (sid_raw, lane_raw) = *view.handle();
        if SessionId::new(sid_raw) != cp_sid || LaneId::new(lane_raw as u32) != cp_lane {
            return Err(CpError::Authorisation {
                effect: CpEffect::Commit,
            });
        }

        let effect_gen = generation.unwrap_or(Gen::ZERO);
        self.run_effect(rv_id, CpCommand::commit(cp_sid, cp_lane, effect_gen))
    }

    fn dispatch_rollback_with_view(
        &self,
        rv_id: RendezvousId,
        cp_sid: SessionId,
        cp_lane: LaneId,
        view: crate::control::cap::HandleView<'_, RollbackKind>,
        generation: Option<Gen>,
    ) -> Result<(), CpError> {
        let (sid_raw, lane_raw) = *view.handle();
        if SessionId::new(sid_raw) != cp_sid || LaneId::new(lane_raw as u32) != cp_lane {
            return Err(CpError::Authorisation {
                effect: CpEffect::Rollback,
            });
        }

        let effect_gen = generation.unwrap_or(Gen::ZERO);
        self.run_effect(rv_id, CpCommand::rollback(cp_sid, cp_lane, effect_gen))
    }

    /// Dispatch a typed control frame.
    ///
    /// Registers the frame with the rendezvous capability table, executes the
    /// authorisation-specific control effect, and returns the registered token
    /// to the caller. Callers that do not need the token may simply drop the
    /// returned value.
    pub(crate) fn dispatch_typed_control_frame<'cluster, K>(
        &'cluster self,
        rv_id: RendezvousId,
        frame: crate::control::ControlFrame<'_, K>,
        generation: Option<Gen>,
    ) -> Result<Option<crate::control::CapRegisteredToken<'cluster, K>>, CpError>
    where
        K: ResourceKind,
        'cfg: 'cluster,
    {
        use crate::control::cap::resource_kinds::*;

        let rendezvous = self
            .get_local(&rv_id)
            .ok_or(CpError::RendezvousMismatch {
                expected: rv_id.raw(),
                actual: 0,
            })?
            .shorten();

        let registered = frame.register(rendezvous)?;

        match K::TAG {
            SpliceIntentKind::TAG => {
                let bag = HandleBag::from_frame(
                    HandleBag::<spec::Nil>::new(),
                    CapFrameToken::<SpliceIntentKind>::new(registered.bytes()),
                );
                bag.with_token(|token_ref, _tail| {
                    let cp_sid = token_ref.sid();
                    let cp_lane = LaneId::new(token_ref.lane().raw());
                    let view = token_ref.as_view().map_err(|_| CpError::Authorisation {
                        effect: CpEffect::SpliceBegin,
                    })?;
                    self.dispatch_splice_intent_with_view(rv_id, cp_sid, cp_lane, view, generation)
                })?;
            }
            // SpliceIntentKind/SpliceAckKind use ExternalControl (cross-role, wire transmission)
            // with AUTO_MINT_EXTERNAL = true for automatic token minting.
            SpliceAckKind::TAG => {
                let bag = HandleBag::from_frame(
                    HandleBag::<spec::Nil>::new(),
                    CapFrameToken::<SpliceAckKind>::new(registered.bytes()),
                );
                bag.with_token(|token_ref, _tail| {
                    let cp_sid = token_ref.sid();
                    let cp_lane = LaneId::new(token_ref.lane().raw());
                    let view = token_ref.as_view().map_err(|_| CpError::Authorisation {
                        effect: CpEffect::SpliceAck,
                    })?;
                    self.dispatch_splice_ack_with_view(rv_id, cp_sid, cp_lane, view, generation)
                })?;
            }
            CancelKind::TAG => {
                let bag = HandleBag::from_frame(
                    HandleBag::<spec::Nil>::new(),
                    CapFrameToken::<CancelKind>::new(registered.bytes()),
                );
                bag.with_token(|token_ref, _tail| {
                    let cp_sid = token_ref.sid();
                    let cp_lane = LaneId::new(token_ref.lane().raw());
                    let view = token_ref.as_view().map_err(|_| CpError::Authorisation {
                        effect: CpEffect::CancelBegin,
                    })?;
                    self.dispatch_cancel_begin_with_view(rv_id, cp_sid, cp_lane, view, generation)
                })?;
            }
            CancelAckKind::TAG => {
                let bag = HandleBag::from_frame(
                    HandleBag::<spec::Nil>::new(),
                    CapFrameToken::<CancelAckKind>::new(registered.bytes()),
                );
                bag.with_token(|token_ref, _tail| {
                    let cp_sid = token_ref.sid();
                    let cp_lane = LaneId::new(token_ref.lane().raw());
                    let view = token_ref.as_view().map_err(|_| CpError::Authorisation {
                        effect: CpEffect::CancelAck,
                    })?;
                    self.dispatch_cancel_ack_with_view(rv_id, cp_sid, cp_lane, view, generation)
                })?;
            }
            CheckpointKind::TAG => {
                let bag = HandleBag::from_frame(
                    HandleBag::<spec::Nil>::new(),
                    CapFrameToken::<CheckpointKind>::new(registered.bytes()),
                );
                bag.with_token(|token_ref, _tail| {
                    let cp_sid = token_ref.sid();
                    let cp_lane = LaneId::new(token_ref.lane().raw());
                    let view = token_ref.as_view().map_err(|_| CpError::Authorisation {
                        effect: CpEffect::Checkpoint,
                    })?;
                    self.dispatch_checkpoint_with_view(rv_id, cp_sid, cp_lane, view)
                })?;
            }
            CommitKind::TAG => {
                let bag = HandleBag::from_frame(
                    HandleBag::<spec::Nil>::new(),
                    CapFrameToken::<CommitKind>::new(registered.bytes()),
                );
                bag.with_token(|token_ref, _tail| {
                    let cp_sid = token_ref.sid();
                    let cp_lane = LaneId::new(token_ref.lane().raw());
                    let view = token_ref.as_view().map_err(|_| CpError::Authorisation {
                        effect: CpEffect::Commit,
                    })?;
                    self.dispatch_commit_with_view(rv_id, cp_sid, cp_lane, view, generation)
                })?;
            }
            RollbackKind::TAG => {
                let bag = HandleBag::from_frame(
                    HandleBag::<spec::Nil>::new(),
                    CapFrameToken::<RollbackKind>::new(registered.bytes()),
                );
                bag.with_token(|token_ref, _tail| {
                    let cp_sid = token_ref.sid();
                    let cp_lane = LaneId::new(token_ref.lane().raw());
                    let view = token_ref.as_view().map_err(|_| CpError::Authorisation {
                        effect: CpEffect::Rollback,
                    })?;
                    self.dispatch_rollback_with_view(rv_id, cp_sid, cp_lane, view, generation)
                })?;
            }
            RerouteKind::TAG
            | crate::control::cap::resource_kinds::RouteDecisionKind::TAG
            | LoopContinueKind::TAG
            | LoopBreakKind::TAG => {}
            LoadBeginKind::TAG | LoadCommitKind::TAG => {
                // Management session load tokens do not require additional control effects.
            }
            // External control kinds (defined in hibana-quic, etc.) are simple markers
            // that don't require control-plane dispatch beyond token registration.
            // Examples: AcceptHookKind (0xE0), ServerZeroRttReplayReportKind (0xE1)
            _ => {}
        }

        Ok(Some(registered))
    }

    /// Number of registered Rendezvous instances (local + remote).
    pub fn len(&self) -> usize {
        // SAFETY: Read-only access to core fields
        unsafe {
            let core = &*self.control.get();
            core.locals.len() + core.remotes.len()
        }
    }

    /// Initialize session effects from global protocol projection.
    ///
    /// This method wires the EffectEnvelope (produced by interpret_eff_list) into
    /// the Rendezvous control-plane state. The envelope contains:
    /// - Control-plane effects (CpEffect) to pre-configure
    /// - Tap events to emit during execution
    /// - Resource handles (from CapToken<K>) for control operations
    ///
    /// Phase2: This enables the "Global → Local → Rendezvous" pipeline where
    /// the global protocol's Eff tree is projected into runtime state tables.
    ///
    /// # Arguments
    ///
    /// * `rv_id` - The Rendezvous to initialize
    /// * `sid` - Session ID for this projection
    /// * `program` - Const effect program defining the session
    ///
    /// # Errors
    ///
    /// Returns `CpError::RendezvousMismatch` if the Rendezvous ID is not registered.
    pub fn init_session_effects<const ROLE: u8, LocalSteps, Mint>(
        &self,
        rv_id: RendezvousId,
        sid: SessionId,
        lane: crate::rendezvous::Lane,
        program: &'static crate::g::RoleProgram<'static, ROLE, LocalSteps, Mint>,
    ) -> Result<(), CpError>
    where
        Mint: crate::control::cap::MintConfigMarker,
    {
        let core = unsafe { &*self.control.get() };

        if !core.locals.is_registered(&rv_id) {
            return Err(CpError::RendezvousMismatch {
                expected: rv_id.raw(),
                actual: 0,
            });
        }

        if core.locals.is_active(&rv_id) {
            return Ok(());
        }

        let rv = self.get_local(&rv_id).ok_or(CpError::RendezvousMismatch {
            expected: rv_id.raw(),
            actual: 0,
        })?;

        rv.install_scope_regions(ROLE, program.scope_atlas_view());
        if rv.is_session_registered(sid) {
            return Ok(());
        }

        let envelope = crate::control::cluster::effects::interpret_eff_list(program.eff_list());
        rv.reset_control_plan(lane);
        let mut control_marker_count = 0u32;
        for marker in envelope.controls() {
            rv.initialise_control_marker(lane, marker);
            control_marker_count = control_marker_count.saturating_add(1);
        }

        let cp_sid = crate::control::types::SessionId::new(sid.raw());
        let cp_lane = crate::control::types::LaneId::new(lane.raw());

        let mut applied_effects = 0u32;
        for effect in envelope.cp_effects() {
            if let Some(command) = initializer_command(*effect, cp_sid, cp_lane) {
                rv.run_effect(command)?;
                applied_effects = applied_effects.saturating_add(1);
            }
        }

        let mut resource_events = 0u32;
        let mut caps_mask_acc = CapsMask::empty();
        for descriptor in envelope.resources() {
            resource_events = resource_events.saturating_add(1);
            rv.register_control_plan(lane, descriptor.eff_index, descriptor.tag, descriptor.plan)?;
            if let Some(mask) = session_caps_mask_for_tag(descriptor.tag, sid, lane) {
                caps_mask_acc = caps_mask_acc.union(mask);
            }
        }

        let lane_caps = if caps_mask_acc.bits() != 0 {
            caps_mask_acc
        } else {
            CapsMask::allow_all()
        };
        rv.set_caps_mask_for_lane(lane, lane_caps);

        if resource_events > 0 {
            applied_effects = applied_effects.saturating_add(resource_events);
        }

        if applied_effects == 0 && control_marker_count > 0 {
            applied_effects = control_marker_count.max(1);
        }

        if applied_effects > 0 {
            let ts = rv.now32();
            crate::observe::push(crate::observe::EffectInit::new(ts, sid.raw(), applied_effects));
        }

        Ok(())
    }

    fn after_local_effect(&self, envelope: CpCommand) -> Result<PendingEffect, CpError> {
        match envelope.effect {
            CpEffect::SpliceBegin => {
                let Some(operands) = envelope.splice else {
                    return Ok(PendingEffect::None);
                };
                let sid = envelope
                    .sid
                    .ok_or(CpError::Splice(CpSpliceError::InvalidSession))?;
                self.with_control_mut(|core| {
                    let begin_result = core.splice_state.begin(sid, operands);
                    let (intent, ack) = begin_result?;
                    let dispatch = CpCommand::splice_ack(sid, operands)
                        .with_intent(intent)
                        .with_ack(ack);

                    Ok(PendingEffect::Dispatch {
                        target: operands.dst_rv,
                        envelope: dispatch,
                    })
                })
            }
            CpEffect::SpliceAck => {
                let Some(operands) = envelope.splice else {
                    return Ok(PendingEffect::None);
                };
                let sid = envelope
                    .sid
                    .ok_or(CpError::Splice(CpSpliceError::InvalidSession))?;

                let expected_ack = envelope.ack.unwrap_or_else(|| operands.ack(sid));

                self.with_control_mut(|core| {
                    let ack = core.splice_state.acknowledge(sid)?;

                    if ack != expected_ack {
                        return Err(CpError::Splice(CpSpliceError::GenerationMismatch));
                    }

                    let dispatch = CpCommand::splice_commit(sid, operands).with_ack(ack);
                    Ok(PendingEffect::Dispatch {
                        target: operands.src_rv,
                        envelope: dispatch,
                    })
                })
            }
            CpEffect::SpliceCommit => {
                if envelope.splice.is_none() {
                    return Ok(PendingEffect::None);
                }
                let sid = envelope
                    .sid
                    .ok_or(CpError::Splice(CpSpliceError::InvalidSession))?;

                self.with_control_mut(|core| core.splice_state.commit(sid, envelope.ack))?;
                Ok(PendingEffect::None)
            }
            _ => Ok(PendingEffect::None),
        }
    }

    /// Returns true if no Rendezvous instances are registered.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl<'cfg, T, U, C, const MAX_RV: usize> SessionCluster<'cfg, T, U, C, MAX_RV>
where
    T: crate::transport::Transport,
    U: crate::runtime::consts::LabelUniverse + 'cfg,
    C: crate::runtime::config::Clock + 'cfg,
{
    fn collect_delegation_links<K>(handle: &K::Handle) -> DelegationChildSet
    where
        K: ResourceKind,
    {
        let mut set = DelegationChildSet::new();
        handle.visit_delegation_links(&mut |child| set.push(child));
        set
    }

    fn collect_claim_links(token: &CapToken) -> DelegationChildSet {
        let mut set = DelegationChildSet::new();
        match token.resource_tag() {
            SpliceIntentKind::TAG => {
                if let Ok(handle) = SpliceIntentKind::decode_handle(token.handle_bytes()) {
                    set.push(RendezvousId::new(handle.src_rv));
                    set.push(RendezvousId::new(handle.dst_rv));
                }
            }
            SpliceAckKind::TAG => {
                if let Ok(handle) = SpliceAckKind::decode_handle(token.handle_bytes()) {
                    set.push(RendezvousId::new(handle.src_rv));
                    set.push(RendezvousId::new(handle.dst_rv));
                }
            }
            RerouteKind::TAG => {
                if let Ok(handle) = RerouteKind::decode_handle(token.handle_bytes()) {
                    set.push(RendezvousId::new(handle.src_rv));
                    set.push(RendezvousId::new(handle.dst_rv));
                }
            }
            _ => {}
        }
        set
    }

    fn init_delegation_children(
        core: &mut ControlCore<'cfg, T, U, C, crate::control::cap::EpochInit, MAX_RV>,
        graph: &mut LeaseGraph<'cfg, DelegationLeaseSpec<T, U, C, crate::control::cap::EpochInit>>,
        parent: RendezvousId,
        links: &DelegationChildSet,
    ) -> Result<(), LeaseGraphError> {
        let table_ptr = NonNull::from(&mut core.delegated_ports);
        for child in links.iter() {
            if child == parent {
                continue;
            }
            match graph.add_child_with_bundle_config(&mut core.locals, parent, child, |child_ctx| {
                child_ctx.set_delegation(DelegationGraphContext::with_table(table_ptr));
            }) {
                Ok(()) | Err(LeaseGraphError::DuplicateId) => {}
                Err(err) => return Err(err),
            }
        }
        Ok(())
    }

    fn populate_mgmt_links<State>(
        &self,
        rv_id: RendezvousId,
        sid: crate::rendezvous::SessionId,
        seed: &mut MgmtSeed<State>,
    ) where
        State: crate::runtime::mgmt::ManagerState,
    {
        self.with_control_mut(|core| {
            core.locals.for_each_available(|child_id, rv| {
                if child_id == rv_id {
                    return;
                }
                if rv.is_session_registered(sid) {
                    seed.links_mut().push(child_id);
                }
            });
        });
    }

    fn collect_mgmt_links(
        seed: &MgmtSeed<impl crate::runtime::mgmt::ManagerState>,
    ) -> DelegationChildSet {
        let mut set = DelegationChildSet::new();
        for id in seed.links.iter() {
            set.push(id);
        }
        set
    }

    fn init_mgmt_children(
        core: &mut ControlCore<'cfg, T, U, C, crate::control::cap::EpochInit, MAX_RV>,
        graph: &mut LeaseGraph<'cfg, MgmtLeaseSpec<T, U, C, crate::control::cap::EpochInit>>,
        parent: RendezvousId,
        links: &DelegationChildSet,
    ) -> Result<(), LeaseGraphError> {
        for child in links.iter() {
            if child == parent {
                continue;
            }
            match graph.add_child_with_bundle_config(&mut core.locals, parent, child, |_| {}) {
                Ok(()) | Err(LeaseGraphError::DuplicateId) => {}
                Err(err) => return Err(err),
            }
        }
        Ok(())
    }

    fn take_mgmt_manager(&self, rv_id: RendezvousId) -> MgmtManager {
        self.with_control_mut(|core| {
            core.mgmt_managers.remove(&rv_id).unwrap_or_else(|| {
                Manager::<Cold, { crate::rendezvous::SLOT_COUNT }>::new().into_await_begin()
            })
        })
    }

    fn store_mgmt_manager(&self, rv_id: RendezvousId, manager: MgmtManager) {
        self.with_control_mut(|core| {
            let _ = core.mgmt_managers.insert(rv_id, manager);
        });
    }

    /// Attach a cursor endpoint for the specified role with transport binding.
    ///
    /// The binding parameter enables flow operations to automatically invoke
    /// transport operations (e.g., STREAM writes). Use `NoBinding` when the
    /// transport layer is handled separately or for choreography-only tests.
    ///
    /// This method acquires all active lanes defined in the program's choreography.
    /// For `g::par` programs, multiple ports are acquired automatically.
    /// For single-lane programs, only lane 0 is acquired.
    pub fn attach_cursor<'lease, const ROLE: u8, LocalSteps, Mint, B>(
        &'lease self,
        rv_id: RendezvousId,
        sid: crate::rendezvous::SessionId,
        program: &'static crate::g::RoleProgram<'static, ROLE, LocalSteps, Mint>,
        binding: B,
    ) -> Result<
        crate::endpoint::CursorEndpoint<
            'cfg,
            ROLE,
            T,
            U,
            C,
            crate::control::cap::EpochInit,
            MAX_RV,
            Mint,
            B,
        >,
        AttachError,
    >
    where
        'cfg: 'lease,
        'lease: 'cfg,
        B: crate::binding::BindingSlot,
        Mint: crate::control::cap::MintConfigMarker,
    {
        use crate::global::role_program::MAX_LANES;

        let mut active_lanes = program.active_lanes();
        let cursor = program.phase_cursor();
        let mint = program.mint_config();

        // Ensure at least lane 0 is active (every endpoint needs at least one lane)
        active_lanes[0] = true;

        // Find primary lane (first active logical lane, always 0 due to above)
        let primary_logical_lane = active_lanes
            .iter()
            .position(|&active| active)
            .unwrap_or(0);

        // Acquire ports and guards for all active lanes
        // Use binder.map_lane() to translate logical lanes to physical lanes
        let mut ports: [Option<crate::rendezvous::Port<'cfg, T, crate::control::cap::EpochInit>>;
            MAX_LANES] = [None, None, None, None, None, None, None, None];
        let mut guards: [Option<LaneGuard<'cfg, T, U, C>>; MAX_LANES] =
            [None, None, None, None, None, None, None, None];

        let mut primary_lane_key: Option<crate::control::cap::LaneKey<'cfg>> = None;
        let mut primary_physical_lane: Option<crate::rendezvous::Lane> = None;

        for logical_idx in 0..MAX_LANES {
            if active_lanes[logical_idx] {
                // Ask the binder for the physical lane mapping
                let physical_lane = binding.map_lane(logical_idx as u8);
                self.init_session_effects(rv_id, sid, physical_lane, program)?;
                let lease = self.lease_port(rv_id, sid, physical_lane, ROLE)?;
                let (port, guard, lane_key) = lease
                    .into_port_guard()
                    .map_err(AttachError::Rendezvous)?;
                ports[logical_idx] = Some(port);
                guards[logical_idx] = Some(guard);

                // Store primary lane's key for Owner creation
                if logical_idx == primary_logical_lane {
                    primary_lane_key = Some(lane_key);
                    primary_physical_lane = Some(physical_lane);
                }
            }
        }

        let lane_key = primary_lane_key.expect("primary lane key must be acquired");
        let primary_wire_lane =
            primary_physical_lane.expect("primary physical lane must be acquired");

        let owner = crate::control::cap::Owner::new(lane_key);
        let epoch = crate::control::cap::EndpointEpoch::new();

        // SessionControlCtx uses primary physical lane for control operations
        let control = crate::endpoint::control::SessionControlCtx::new(
            rv_id,
            primary_wire_lane,
            Some(self),
            None,
        );

        Ok(crate::endpoint::CursorEndpoint::from_parts(
            ports,
            guards,
            primary_logical_lane,
            sid,
            owner,
            epoch,
            cursor,
            control,
            mint,
            binding,
        ))
    }

    #[inline]
    fn init_bundle_context_with_needs(
        core: &mut ControlCore<'cfg, T, U, C, crate::control::cap::EpochInit, MAX_RV>,
        rv_id: RendezvousId,
        needs: LeaseFacetNeeds,
    ) -> LeaseBundleContext<'cfg, 'cfg, T, U, C, crate::control::cap::EpochInit>
    where
        T: crate::transport::Transport,
        U: crate::runtime::consts::LabelUniverse,
        C: crate::runtime::config::Clock,
    {
        LeaseBundleContext::from_control_core_with_needs::<MAX_RV>(&mut core.locals, rv_id, needs)
            .unwrap_or_default()
    }

    fn init_bundle_context(
        core: &mut ControlCore<'cfg, T, U, C, crate::control::cap::EpochInit, MAX_RV>,
        rv_id: RendezvousId,
    ) -> LeaseBundleContext<'cfg, 'cfg, T, U, C, crate::control::cap::EpochInit>
    where
        T: crate::transport::Transport,
        U: crate::runtime::consts::LabelUniverse,
        C: crate::runtime::config::Clock,
    {
        Self::init_bundle_context_with_needs(core, rv_id, LeaseFacetNeeds::all())
    }

    pub async fn init_mgmt<'lease, Mint>(
        &'lease self,
        rv_id: RendezvousId,
        _sid: crate::rendezvous::SessionId,
        lane: crate::rendezvous::Lane,
        endpoint: CursorEndpoint<
            'lease,
            { session::ROLE_CLUSTER },
            T,
            U,
            C,
            crate::control::cap::EpochInit,
            MAX_RV,
            Mint,
        >,
    ) -> Result<
        (
            CursorEndpoint<
                'lease,
                { session::ROLE_CLUSTER },
                T,
                U,
                C,
                crate::control::cap::EpochInit,
                MAX_RV,
                Mint,
            >,
            MgmtSeed<AwaitBegin>,
        ),
        MgmtError,
    >
    where
        'cfg: 'lease,
        'lease: 'cfg,
        Mint: MintConfigMarker,
    {
        let _ = lane;
        let manager_state = self.take_mgmt_manager(rv_id);
        match session::drive_cluster(manager_state, endpoint).await {
            Ok(ok) => Ok(ok),
            Err((err, manager)) => {
                self.store_mgmt_manager(rv_id, manager);
                Err(err)
            }
        }
    }

    pub fn drive_mgmt(
        &self,
        rv_id: RendezvousId,
        sid: crate::rendezvous::SessionId,
        mut seed: MgmtSeed<AwaitBegin>,
    ) -> Result<Reply, MgmtError> {
        self.populate_mgmt_links(rv_id, sid, &mut seed);
        let mgmt_links = Self::collect_mgmt_links(&seed);
        let mgmt_needs =
            <MgmtLeaseSpec<T, U, C, crate::control::cap::EpochInit> as LeaseSpecFacetNeeds>::facet_needs();

        let drive_result = self.drive::<MgmtAutomaton<AwaitBegin>, _, _>(
            rv_id,
            seed,
            |core, rv| Self::init_bundle_context_with_needs(core, rv, mgmt_needs),
            move |core, graph| Self::init_mgmt_children(core, graph, rv_id, &mgmt_links),
        );

        let (manager, reply) = match drive_result {
            Ok(ok) => ok,
            Err(err) => {
                return Err(match err {
                    DelegationDriveError::Lease(_) | DelegationDriveError::Graph(_) => {
                        MgmtError::InvalidTransition
                    }
                    DelegationDriveError::Automaton(err) => err,
                });
            }
        };

        self.store_mgmt_manager(rv_id, manager);
        Ok(reply)
    }

    /// Drive a delegation automaton rooted at `rv_id` using a LeaseGraph.
    ///
    /// The `root_builder` closure constructs the root facet for the graph from the
    /// rendezvous lease, allowing callers to choose the facet bundle used to seed
    /// the graph (e.g. slot/caps/splice facets). The automaton receives both the
    /// prepared graph and the rendezvous lease so it can manipulate additional
    /// facets as needed. The `graph_init` closure may add child nodes or perform
    /// additional setup before the automaton is executed.
    fn drive<A, Root, Init>(
        &self,
        rv_id: RendezvousId,
        seed: A::Seed,
        root_builder: Root,
        graph_init: Init,
    ) -> Result<A::Output, DelegationDriveError<A::Error>>
    where
        A: ControlAutomaton<T, U, C, crate::control::cap::EpochInit>,
        A::GraphSpec: LeaseSpec<NodeId = RendezvousId> + 'cfg,
        Root: FnOnce(
            &mut ControlCore<'cfg, T, U, C, crate::control::cap::EpochInit, MAX_RV>,
            RendezvousId,
        ) -> FacetContext<'cfg, A::GraphSpec>,
        Init: FnOnce(
            &mut ControlCore<'cfg, T, U, C, crate::control::cap::EpochInit, MAX_RV>,
            &mut LeaseGraph<'cfg, A::GraphSpec>,
        ) -> Result<(), crate::control::lease::graph::LeaseGraphError>,
    {
        self.with_control_mut(|core| {
            let root_context = root_builder(core, rv_id);
            let mut graph = LeaseGraph::<A::GraphSpec>::new(
                rv_id,
                <A::GraphSpec as LeaseSpec>::Facet::default(),
                root_context,
            );
            let graph_ptr: *mut LeaseGraph<'cfg, A::GraphSpec> = &mut graph;

            if let Err(err) = graph_init(core, unsafe { &mut *graph_ptr }) {
                unsafe {
                    (*graph_ptr).rollback();
                }
                return Err(DelegationDriveError::Graph(err));
            }

            let mut lease = match core.locals.lease::<A::Spec>(rv_id) {
                Ok(lease) => lease,
                Err(err) => {
                    unsafe {
                        (*graph_ptr).rollback();
                    }
                    return Err(DelegationDriveError::Lease(err));
                }
            };

            let outcome = unsafe { A::run_with_graph(&mut *graph_ptr, &mut lease, seed) };

            drop(lease);

            match outcome {
                ControlStep::Complete(output) => {
                    unsafe {
                        (*graph_ptr).commit();
                    }
                    Ok(output)
                }
                ControlStep::Abort(err) => {
                    unsafe {
                        (*graph_ptr).rollback();
                    }
                    Err(DelegationDriveError::Automaton(err))
                }
            }
        })
    }
}

impl<'cfg, T, U, C, const MAX_RV: usize> Drop for SessionCluster<'cfg, T, U, C, MAX_RV>
where
    T: crate::transport::Transport,
    U: crate::runtime::consts::LabelUniverse,
    C: crate::runtime::config::Clock,
{
    fn drop(&mut self) {
        // SAFETY: `core` is owned by `self` and we're in `drop`, so no aliases exist.
        let core = unsafe { &*self.control.get() };
        debug_assert_eq!(
            core.active_leases.get(),
            0,
            "SessionCluster dropped with outstanding lane leases",
        );
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::cap::ResourceKind;
    use crate::control::types::{Gen, LaneId, SessionId};
    use crate::runtime::config::CounterClock;
    use crate::runtime::consts::DefaultLabelUniverse;
    use crate::transport::{Transport, TransportError, wire::Payload};
    use core::future::{Ready, ready};

    // Dummy transport for testing
    struct DummyTransport;

    impl Transport for DummyTransport {
        type Error = TransportError;
        type Tx<'a>
            = ()
        where
            Self: 'a;
        type Rx<'a>
            = ()
        where
            Self: 'a;
        type Send<'a>
            = Ready<Result<(), Self::Error>>
        where
            Self: 'a;
        type Recv<'a>
            = Ready<Result<Payload<'a>, Self::Error>>
        where
            Self: 'a;
        type Metrics = crate::transport::NoopMetrics;

        fn open<'a>(&'a self, _local_role: u8, _session_id: u32) -> (Self::Tx<'a>, Self::Rx<'a>) {
            ((), ())
        }

        fn send<'a, 'f>(
            &'a self,
            _tx: &'a mut Self::Tx<'a>,
            _payload: Payload<'f>,
            _dest_role: u8,
        ) -> Self::Send<'a>
        where
            'a: 'f,
        {
            ready(Ok(()))
        }

        fn recv<'a>(&'a self, _rx: &'a mut Self::Rx<'a>) -> Self::Recv<'a> {
            ready(Err(TransportError::Failed))
        }
    }

    #[test]
    fn session_caps_mask_yields_checkpoint_permission() {
        let lane = crate::rendezvous::Lane::new(0);
        let mask = super::session_caps_mask_for_tag(
            crate::control::cap::resource_kinds::CheckpointKind::TAG,
            SessionId::new(42),
            lane,
        )
        .expect("checkpoint mask");
        assert!(
            mask.allows(CpEffect::Checkpoint),
            "checkpoint capability must allow CpEffect::Checkpoint"
        );
    }

    // Type alias for test cluster
    type TestCluster<'cfg, const MAX_RV: usize> =
        SessionCluster<'cfg, DummyTransport, DefaultLabelUniverse, CounterClock, MAX_RV>;

    // Helper to create a leaked clock for tests
    fn leak_clock() -> &'static CounterClock {
        Box::leak(Box::new(CounterClock::new()))
    }

    #[test]
    fn run_effect_respects_caps_mask_before_dispatch() {
        use crate::observe::TapEvent;
        use crate::runtime::{config::Config, consts::RING_EVENTS};
        use crate::{control::cap::CapsMask, rendezvous::Lane as RaLane};
        let mut tap_buf = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 256];
        let config = Config::new(&mut tap_buf, &mut slab);
        let rendezvous = crate::rendezvous::Rendezvous::from_config(config, DummyTransport);
        rendezvous.set_caps_mask_for_lane(RaLane::new(0), CapsMask::empty());
        let rv_id = rendezvous.id();

        let cluster: &TestCluster<4> = Box::leak(Box::new(SessionCluster::new(leak_clock())));
        cluster
            .add_rendezvous(rendezvous)
            .expect("register rendezvous");

        let sid = SessionId::new(7);
        let lane = LaneId::new(0);
        let envelope = CpCommand::checkpoint(sid, lane);

        let err = cluster.run_effect(rv_id, envelope).unwrap_err();
        assert!(matches!(
            err,
            CpError::Authorisation {
                effect: CpEffect::Checkpoint
            }
        ));
    }

    #[test]
    fn dispatch_splice_ack_tracks_masks_and_rolls_back() {
        use crate::{
            control::cap::{CapsMask, HandleView, resource_kinds::SpliceHandle},
            observe::TapEvent,
            rendezvous::Lane as RaLane,
            runtime::{config::Config, consts::RING_EVENTS},
        };

        let cluster: &TestCluster<4> = Box::leak(Box::new(SessionCluster::new(leak_clock())));

        let tap_src = Box::leak(Box::new([TapEvent::default(); RING_EVENTS]));
        let slab_src = Box::leak(Box::new([0u8; 256]));
        let src_cfg = Config::new(tap_src, slab_src);
        let src_rendezvous = crate::rendezvous::Rendezvous::from_config(src_cfg, DummyTransport);

        let tap_dst = Box::leak(Box::new([TapEvent::default(); RING_EVENTS]));
        let slab_dst = Box::leak(Box::new([0u8; 256]));
        let dst_cfg = Config::new(tap_dst, slab_dst);
        let dst_rendezvous = crate::rendezvous::Rendezvous::from_config(dst_cfg, DummyTransport);

        let src_id = cluster
            .add_rendezvous(src_rendezvous)
            .expect("register src");
        let dst_id = cluster
            .add_rendezvous(dst_rendezvous)
            .expect("register dst");

        let src_lane = LaneId::new(0);
        let dst_lane = LaneId::new(1);

        cluster.with_control_mut(|core| {
            let src_rv = core.locals.get_mut(&src_id).unwrap();
            src_rv.set_caps_mask_for_lane(
                RaLane::new(src_lane.raw()),
                CapsMask::empty()
                    .with(CpEffect::SpliceBegin)
                    .with(CpEffect::SpliceCommit),
            );
            let dst_rv = core.locals.get_mut(&dst_id).unwrap();
            dst_rv.set_caps_mask_for_lane(RaLane::new(dst_lane.raw()), CapsMask::empty());
        });

        let ack_caps = CapsMask::empty().with(CpEffect::SpliceAck);

        let sid = SessionId::new(7);
        let operands = SpliceOperands::new(
            src_id,
            dst_id,
            src_lane,
            dst_lane,
            Gen::new(0),
            Gen::new(1),
            0,
            0,
        );

        let pending = cluster
            .run_effect_step(src_id, CpCommand::splice_begin(sid, operands))
            .expect("begin effect");
        assert!(matches!(pending, PendingEffect::Dispatch { .. }));

        let handle = SpliceHandle::new(
            src_id.raw(),
            dst_id.raw(),
            src_lane.raw() as u16,
            dst_lane.raw() as u16,
            operands.old_gen.raw(),
            operands.new_gen.raw(),
            operands.seq_tx,
            operands.seq_rx,
            0,
        );
        let handle_bytes = handle.encode();
        let view = HandleView::decode(&handle_bytes, ack_caps).expect("decode view");

        cluster
            .dispatch_splice_ack_with_view(dst_id, sid, dst_lane, view, None)
            .expect("dispatch succeeds");

        cluster.with_control_mut(|core| {
            let rv = core.locals.get_mut(&dst_id).unwrap();
            let mask = rv.caps_mask_for_lane(RaLane::new(dst_lane.raw()));
            assert!(mask.allows(CpEffect::SpliceAck));
            rv.set_caps_mask_for_lane(RaLane::new(dst_lane.raw()), CapsMask::empty());
        });

        let sid_fail = SessionId::new(9);
        let operands_fail = SpliceOperands::new(
            src_id,
            dst_id,
            src_lane,
            dst_lane,
            Gen::new(1),
            Gen::new(2),
            0,
            0,
        );

        cluster
            .run_effect_step(src_id, CpCommand::splice_begin(sid_fail, operands_fail))
            .expect("second begin effect");

        cluster.with_control_mut(|core| {
            let rv = core.locals.get_mut(&dst_id).unwrap();
            rv.set_caps_mask_for_lane(RaLane::new(dst_lane.raw()), CapsMask::empty());
        });

        let failure_handle = SpliceHandle::new(
            src_id.raw(),
            dst_id.raw(),
            src_lane.raw() as u16,
            dst_lane.raw() as u16,
            operands_fail.old_gen.raw(),
            operands_fail.new_gen.raw(),
            operands_fail.seq_tx,
            operands_fail.seq_rx,
            0,
        );
        let failure_bytes = failure_handle.encode();
        let failure_view =
            HandleView::decode(&failure_bytes, ack_caps).expect("decode failure view");

        let err = cluster
            .dispatch_splice_ack_with_view(dst_id, sid_fail, dst_lane, failure_view, None)
            .expect_err("second ack should fail due to busy lane");
        assert!(
            matches!(
                err,
                CpError::Splice(
                    crate::control::SpliceError::LaneMismatch
                        | crate::control::SpliceError::InvalidState
                )
            ),
            "error was {:?}",
            err
        );

        cluster.with_control_mut(|core| {
            let rv = core.locals.get_mut(&dst_id).unwrap();
            let mask = rv.caps_mask_for_lane(RaLane::new(dst_lane.raw()));
            assert_eq!(mask.bits(), CapsMask::empty().bits());
        });
    }

}
