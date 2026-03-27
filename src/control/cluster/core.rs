//! SessionCluster - Distributed control-plane coordination.
//!
//! This module implements SessionCluster, which coordinates multiple Rendezvous
//! instances for local distributed session management.

use core::marker::PhantomData;

use crate::control::automaton::{
    delegation::{DelegateMintAutomaton, DelegateMintSeed, DelegationLeaseSpec},
    distributed::{DistributedSplice, DistributedSpliceInv, SpliceAck, SpliceIntent},
    splice::{
        SpliceBeginAutomaton, SpliceCommitAutomaton, SpliceGraphContext, SplicePrepareAutomaton,
        SplicePrepareSeed,
    },
};
use crate::control::cap::ControlHandle;
use crate::control::cap::mint::{AllowsCanonical, SessionScopedKind};
use crate::control::cap::mint::{
    CapShot, CapsMask, EndpointResource, GenericCapToken, MintConfigMarker, ResourceKind,
};
use crate::control::cap::resource_kinds::{
    CancelAckKind, CancelKind, CheckpointKind, CommitKind, LoopBreakKind, LoopContinueKind,
    RerouteHandle, RerouteKind, RollbackKind, RouteDecisionKind, SpliceAckKind, SpliceHandle,
    SpliceIntentKind,
};
use crate::control::cap::typed_tokens::CapFrameToken;
use crate::control::cluster::effects::CpEffect;
use crate::control::handle::{bag::HandleBag, spec};
use crate::control::lease::{
    bundle::{LeaseBundleContext, LeaseGraphBundleExt},
    core::{
        ControlAutomaton, ControlStep, DelegationDriveError, FullSpec, LeaseError,
        RegisterRendezvousError,
    },
    graph::{LeaseFacet, LeaseGraph, LeaseGraphError, LeaseSpec},
    map::ArrayMap,
    planner::{
        DELEGATION_CHILD_SET_CAPACITY, LeaseFacetNeeds, LeaseSpecFacetNeeds, facet_needs,
        facets_caps_delegation, facets_caps_splice,
    },
};
use crate::endpoint::affine::LaneGuard;

const MAX_CACHED_SPLICES: usize = 64;
const HANDLE_RESOLVER_SLOTS: usize = 128;

fn splice_operands_from_handle(handle: SpliceHandle) -> SpliceOperands {
    SpliceOperands::new(
        RendezvousId::new(handle.src_rv),
        RendezvousId::new(handle.dst_rv),
        Lane::new(handle.src_lane as u32),
        Lane::new(handle.dst_lane as u32),
        Generation::new(handle.old_gen),
        Generation::new(handle.new_gen),
        handle.seq_tx,
        handle.seq_rx,
    )
}

use super::error::{AttachError, CpError, DelegationError, SpliceError};
use crate::control::automaton::txn::{InAcked, InBegin, NoopTap};
use crate::control::types::{Generation, Lane, RendezvousId, SessionId};
use crate::eff::EffIndex;
use crate::global::{
    compiled::ProgramFacts,
    const_dsl::{PolicyMode, ScopeId},
};
use crate::observe::scope::ScopeTrace;
use crate::rendezvous::core::{LaneLease, Rendezvous};
use crate::rendezvous::error::RendezvousError;
use crate::rendezvous::slots::SLOT_COUNT;
use crate::runtime::mgmt::{
    AwaitBegin, Cold, Manager, MgmtAutomaton, MgmtError, MgmtLeaseSpec, MgmtSeed, Reply,
};
use crate::transport::context::{self, ContextValue};
use crate::transport::{TransportAlgorithm, TransportSnapshot};

/// Control-plane effect envelope encompassing the effect and its operands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SpliceOperands {
    pub(crate) src_rv: RendezvousId,
    pub(crate) dst_rv: RendezvousId,
    pub(crate) src_lane: Lane,
    pub(crate) dst_lane: Lane,
    pub(crate) old_gen: Generation,
    pub(crate) new_gen: Generation,
    pub(crate) seq_tx: u32,
    pub(crate) seq_rx: u32,
}

impl SpliceOperands {
    #[allow(clippy::too_many_arguments)]
    pub(crate) const fn new(
        src_rv: RendezvousId,
        dst_rv: RendezvousId,
        src_lane: Lane,
        dst_lane: Lane,
        old_gen: Generation,
        new_gen: Generation,
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

    pub(crate) fn from_intent(intent: &SpliceIntent) -> Self {
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

    pub(crate) fn intent(&self, sid: SessionId) -> SpliceIntent {
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

    pub(crate) fn ack(&self, sid: SessionId) -> SpliceAck {
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
pub(crate) struct DelegateOperands {
    pub claim: bool,
    pub token: GenericCapToken<EndpointResource>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PendingEffect {
    None,
    Dispatch {
        target: RendezvousId,
        envelope: CpCommand,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CpCommand {
    pub(crate) effect: CpEffect,
    pub(crate) sid: Option<SessionId>,
    pub(crate) lane: Option<Lane>,
    pub(crate) generation: Option<Generation>,
    pub(crate) prev_generation: Option<Generation>,
    pub(crate) fences: Option<(u32, u32)>,
    pub(crate) splice: Option<SpliceOperands>,
    pub(crate) intent: Option<SpliceIntent>,
    pub(crate) ack: Option<SpliceAck>,
    pub(crate) delegate: Option<DelegateOperands>,
}

impl CpCommand {
    pub(crate) const fn new(effect: CpEffect) -> Self {
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

    pub(crate) fn with_sid(mut self, sid: SessionId) -> Self {
        self.sid = Some(sid);
        self
    }

    pub(crate) fn with_lane(mut self, lane: Lane) -> Self {
        self.lane = Some(lane);
        self
    }

    pub(crate) fn with_generation(mut self, generation: Generation) -> Self {
        self.generation = Some(generation);
        self
    }

    pub(crate) fn with_prev_generation(mut self, generation: Generation) -> Self {
        self.prev_generation = Some(generation);
        self
    }

    pub(crate) fn with_fences(mut self, fences: Option<(u32, u32)>) -> Self {
        self.fences = fences;
        self
    }

    pub(crate) fn with_splice(mut self, operands: SpliceOperands) -> Self {
        self.splice = Some(operands);
        self
    }

    pub(crate) fn with_intent(mut self, intent: SpliceIntent) -> Self {
        self.intent = Some(intent);
        self
    }

    pub(crate) fn with_ack(mut self, ack: SpliceAck) -> Self {
        self.ack = Some(ack);
        self
    }

    pub(crate) fn with_delegate(mut self, delegate: DelegateOperands) -> Self {
        self.delegate = Some(delegate);
        self
    }

    fn derive_sid_lane(token: GenericCapToken<EndpointResource>) -> (SessionId, Lane) {
        let header = token.header();
        let sid = u32::from_be_bytes([header[0], header[1], header[2], header[3]]);
        let lane = header[4] as u32;
        (SessionId::new(sid), Lane::new(lane))
    }

    pub(crate) fn canonicalize_delegate(mut self) -> Result<Self, CpError> {
        let delegate = self
            .delegate
            .ok_or(CpError::Delegation(DelegationError::InvalidToken))?;
        let (sid, lane) = Self::derive_sid_lane(delegate.token);
        if self.sid.is_some_and(|current| current != sid) {
            return Err(CpError::Delegation(DelegationError::InvalidToken));
        }
        if self.lane.is_some_and(|current| current != lane) {
            return Err(CpError::Delegation(DelegationError::InvalidToken));
        }
        self.sid = Some(sid);
        self.lane = Some(lane);
        self = self.with_delegate(DelegateOperands {
            claim: delegate.claim,
            token: delegate.token,
        });
        Ok(self)
    }

    pub(crate) fn splice_begin(sid: SessionId, operands: SpliceOperands) -> Self {
        Self::new(CpEffect::SpliceBegin)
            .with_sid(sid)
            .with_lane(operands.src_lane)
            .with_generation(operands.new_gen)
            .with_prev_generation(operands.old_gen)
            .with_fences(Some((operands.seq_tx, operands.seq_rx)))
            .with_splice(operands)
            .with_intent(operands.intent(sid))
    }

    pub(crate) fn splice_ack(sid: SessionId, operands: SpliceOperands) -> Self {
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

    pub(crate) fn splice_commit(sid: SessionId, operands: SpliceOperands) -> Self {
        Self::new(CpEffect::SpliceCommit)
            .with_sid(sid)
            .with_lane(operands.src_lane)
            .with_generation(operands.new_gen)
            .with_prev_generation(operands.old_gen)
            .with_fences(Some((operands.seq_tx, operands.seq_rx)))
            .with_splice(operands)
            .with_ack(operands.ack(sid))
    }

    pub(crate) fn cancel_begin(sid: SessionId, lane: Lane) -> Self {
        Self::new(CpEffect::CancelBegin)
            .with_sid(sid)
            .with_lane(lane)
    }

    pub(crate) fn cancel_ack(sid: SessionId, lane: Lane, generation: Generation) -> Self {
        Self::new(CpEffect::CancelAck)
            .with_sid(sid)
            .with_lane(lane)
            .with_generation(generation)
    }

    pub(crate) fn checkpoint(sid: SessionId, lane: Lane) -> Self {
        Self::new(CpEffect::Checkpoint)
            .with_sid(sid)
            .with_lane(lane)
    }

    pub(crate) fn rollback(sid: SessionId, lane: Lane, generation: Generation) -> Self {
        Self::new(CpEffect::Rollback)
            .with_sid(sid)
            .with_lane(lane)
            .with_generation(generation)
    }

    pub(crate) fn commit(sid: SessionId, lane: Lane, generation: Generation) -> Self {
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
        dst_lane: Lane,
        fences: Option<(u32, u32)>,
    },
    Reroute {
        dst_rv: RendezvousId,
        dst_lane: Lane,
        shard: Option<u32>,
    },
    RouteArm {
        arm: u8,
    },
    Loop {
        decision: bool,
    },
    Defer {
        retry_hint: u8,
    },
}

/// Semantic fail-closed error returned by Rust-side dynamic resolvers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResolverError {
    Reject,
}

type ResolverResult = Result<DynamicResolution, ResolverError>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolverContext {
    rv_id: RendezvousId,
    session: Option<SessionId>,
    lane: Lane,
    eff_index: EffIndex,
    tag: u8,
    metrics: TransportSnapshot,
    scope_id: ScopeId,
    scope_trace: Option<ScopeTrace>,
    /// Slot-scoped policy input arguments.
    policy_input: [u32; 4],
    /// Slot-scoped policy attributes.
    policy_attrs: crate::transport::context::PolicyAttrs,
}

impl ResolverContext {
    #[inline]
    pub(crate) fn new(
        rv_id: RendezvousId,
        session: Option<SessionId>,
        lane: Lane,
        eff_index: EffIndex,
        tag: u8,
        metrics: TransportSnapshot,
        scope_id: ScopeId,
        scope_trace: Option<ScopeTrace>,
        input: [u32; 4],
        attrs: crate::transport::context::PolicyAttrs,
    ) -> Self {
        Self {
            rv_id,
            session,
            lane,
            eff_index,
            tag,
            metrics,
            scope_id,
            scope_trace,
            policy_input: input,
            policy_attrs: attrs,
        }
    }

    #[inline]
    fn core_attr(&self, id: context::ContextId) -> Option<ContextValue> {
        let raw = id.raw();
        if raw == context::core::RV_ID.raw() {
            return Some(ContextValue::from_u16(self.rv_id.raw()));
        }
        if raw == context::core::SESSION_ID.raw() {
            return self
                .session
                .map(|session| ContextValue::from_u32(session.raw()));
        }
        if raw == context::core::LANE.raw() {
            return Some(ContextValue::from_u32(self.lane.raw()));
        }
        if raw == context::core::TAG.raw() {
            return Some(ContextValue::from_u8(self.tag));
        }
        if raw == context::core::LATENCY_US.raw() {
            return self.metrics.latency_us.map(ContextValue::from_u64);
        }
        if raw == context::core::QUEUE_DEPTH.raw() {
            return self.metrics.queue_depth.map(ContextValue::from_u32);
        }
        if raw == context::core::PACING_INTERVAL_US.raw() {
            return self.metrics.pacing_interval_us.map(ContextValue::from_u64);
        }
        if raw == context::core::CONGESTION_MARKS.raw() {
            return self.metrics.congestion_marks.map(ContextValue::from_u32);
        }
        if raw == context::core::RETRANSMISSIONS.raw() {
            return self.metrics.retransmissions.map(ContextValue::from_u32);
        }
        if raw == context::core::PTO_COUNT.raw() {
            return self.metrics.pto_count.map(ContextValue::from_u32);
        }
        if raw == context::core::SRTT_US.raw() {
            return self.metrics.srtt_us.map(ContextValue::from_u64);
        }
        if raw == context::core::LATEST_ACK_PN.raw() {
            return self.metrics.latest_ack_pn.map(ContextValue::from_u64);
        }
        if raw == context::core::CONGESTION_WINDOW.raw() {
            return self.metrics.congestion_window.map(ContextValue::from_u64);
        }
        if raw == context::core::IN_FLIGHT_BYTES.raw() {
            return self.metrics.in_flight_bytes.map(ContextValue::from_u64);
        }
        if raw == context::core::TRANSPORT_ALGORITHM.raw() {
            return self.metrics.algorithm.map(encode_transport_algorithm);
        }
        None
    }

    /// Query a policy attribute by opaque id.
    #[inline]
    pub fn attr(
        &self,
        id: crate::transport::context::ContextId,
    ) -> Option<crate::transport::context::ContextValue> {
        self.core_attr(id).or_else(|| self.policy_attrs.query(id))
    }

    /// Read slot-scoped policy input argument by index.
    #[inline]
    pub fn input(&self, idx: u8) -> u32 {
        self.policy_input.get(idx as usize).copied().unwrap_or(0)
    }
}

#[derive(Clone, Copy)]
pub struct ResolverRef<'cfg> {
    state: *const (),
    callback: usize,
    dispatch: unsafe fn(*const (), usize, ResolverContext) -> ResolverResult,
    _marker: PhantomData<&'cfg ()>,
}

impl<'cfg> ResolverRef<'cfg> {
    #[inline]
    pub fn from_state<S: 'cfg>(
        state: &'cfg S,
        resolver: fn(&S, ResolverContext) -> ResolverResult,
    ) -> Self {
        Self {
            state: core::ptr::from_ref(state).cast(),
            callback: resolver as usize,
            dispatch: dispatch_state::<S>,
            _marker: PhantomData,
        }
    }

    #[inline]
    pub fn from_fn(resolver: fn(ResolverContext) -> ResolverResult) -> Self {
        Self {
            state: core::ptr::null(),
            callback: resolver as usize,
            dispatch: dispatch_fn,
            _marker: PhantomData,
        }
    }

    #[inline]
    pub(crate) fn resolve(self, ctx: ResolverContext) -> ResolverResult {
        unsafe { (self.dispatch)(self.state, self.callback, ctx) }
    }
}

unsafe fn dispatch_state<S>(
    state: *const (),
    callback: usize,
    ctx: ResolverContext,
) -> ResolverResult {
    let state = unsafe { &*state.cast::<S>() };
    let resolver = unsafe {
        core::mem::transmute::<usize, fn(&S, ResolverContext) -> ResolverResult>(callback)
    };
    resolver(state, ctx)
}

unsafe fn dispatch_fn(_state: *const (), callback: usize, ctx: ResolverContext) -> ResolverResult {
    let resolver =
        unsafe { core::mem::transmute::<usize, fn(ResolverContext) -> ResolverResult>(callback) };
    resolver(ctx)
}

#[inline]
const fn encode_transport_algorithm(algorithm: TransportAlgorithm) -> ContextValue {
    match algorithm {
        TransportAlgorithm::Cubic => ContextValue::from_u32(1),
        TransportAlgorithm::Reno => ContextValue::from_u32(2),
        TransportAlgorithm::Other(tag) => ContextValue::from_u32(0x100 | tag as u32),
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

struct DynamicResolverEntry<'cfg> {
    resolver: ResolverRef<'cfg>,
    policy: PolicyMode,
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

fn session_caps_mask_for_tag(tag: u8, sid: SessionId, lane: Lane) -> Option<CapsMask> {
    use crate::control::cap::mint::ResourceKind;
    use crate::control::cap::mint::SessionScopedKind;
    use crate::control::cap::resource_kinds;

    let sid_rv = SessionId::new(sid.raw());
    let lane_rv = Lane::new(lane.raw());

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
fn initializer_command(effect: CpEffect, sid: SessionId, lane: Lane) -> Option<CpCommand> {
    let _ = (effect, sid, lane);
    None
}

/// Trait implemented by local Rendezvous instances that can apply control-plane effects.
pub(crate) trait EffectRunner {
    fn run_effect(&self, envelope: CpCommand) -> Result<(), CpError>;
    fn prepare_splice_operands(
        &self,
        sid: SessionId,
        src_lane: Lane,
        dst_rv: RendezvousId,
        dst_lane: Lane,
        fences: Option<(u32, u32)>,
    ) -> Result<SpliceOperands, CpError>;
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
pub(crate) struct DistributedSpliceState<const MAX: usize> {
    entries: ArrayMap<SessionId, DistributedEntry, MAX>,
}

impl<const MAX: usize> Default for DistributedSpliceState<MAX> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const MAX: usize> DistributedSpliceState<MAX> {
    /// Create a new empty state.
    pub(crate) const fn new() -> Self {
        Self {
            entries: ArrayMap::new(),
        }
    }

    pub(crate) fn begin(
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

    pub(crate) fn acknowledge(&mut self, sid: SessionId) -> Result<SpliceAck, CpError> {
        let entry = self
            .entries
            .get_mut(&sid)
            .ok_or(CpError::Splice(SpliceError::InvalidSession))?;

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

    pub(crate) fn commit(
        &mut self,
        sid: SessionId,
        expected: Option<SpliceAck>,
    ) -> Result<SpliceOperands, CpError> {
        let entry = self
            .entries
            .remove(&sid)
            .ok_or(CpError::Splice(SpliceError::InvalidSession))?;

        let DistributedEntry { operands, phase } = entry;

        match phase {
            DistributedPhase::Acked { txn, ack } => {
                if let Some(exp) = expected
                    && exp != ack
                {
                    return Err(CpError::Splice(SpliceError::CommitFailed));
                }

                let mut tap = NoopTap;
                let _closed = DistributedSplice::commit(txn, &mut tap);
                Ok(operands)
            }
            DistributedPhase::Begin { .. } => Err(CpError::Splice(SpliceError::InvalidState)),
        }
    }

    pub(crate) fn get(&self, sid: SessionId) -> Option<&SpliceOperands> {
        self.entries.get(&sid).map(|entry| &entry.operands)
    }
}

/// SessionCluster - Coordinates multiple Rendezvous instances.
///
/// This is the top-level local control-plane coordinator. It manages:
/// - Local Rendezvous instances
/// - Distributed splice coordination across registered local rendezvous
/// - Intent/Ack routing
///
/// # Type Parameters
///
/// - `MAX_RV`: Maximum number of Rendezvous instances
///
/// # Example
///
/// ```rust,ignore
/// use hibana::substrate::{RendezvousId, SessionCluster};
///
/// let mut cluster: SessionCluster<8> = SessionCluster::new(leak_clock());
///
/// // Register local Rendezvous
/// cluster.add_rendezvous(local_rendezvous)?;
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
    E: crate::control::cap::mint::EpochTable,
{
    /// Owned local Rendezvous instances (same process/node).
    locals: crate::control::lease::core::ControlCore<'cfg, T, U, C, E, MAX_RV>,

    /// Distributed splice state tracking.
    splice_state: DistributedSpliceState<MAX_RV>,

    /// Cached operands staged between minting intent and ack tokens.
    cached_operands: ArrayMap<SessionId, SpliceOperands, MAX_CACHED_SPLICES>,

    /// Number of active lane leases (affine witness count).
    active_leases: core::cell::Cell<u32>,

    /// Cached management manager state per rendezvous.
    mgmt_managers: ArrayMap<RendezvousId, Manager<AwaitBegin, { SLOT_COUNT }>, MAX_RV>,
}

impl<'cfg, T, U, C, E, const MAX_RV: usize> ControlCore<'cfg, T, U, C, E, MAX_RV>
where
    T: crate::transport::Transport,
    U: crate::runtime::consts::LabelUniverse,
    C: crate::runtime::config::Clock,
    E: crate::control::cap::mint::EpochTable,
{
    #[cfg(feature = "std")]
    unsafe fn init_empty(dst: *mut Self) {
        unsafe {
            crate::control::lease::core::ControlCore::init_empty(core::ptr::addr_of_mut!(
                (*dst).locals
            ));
            core::ptr::addr_of_mut!((*dst).splice_state).write(DistributedSpliceState::new());
            ArrayMap::init_empty(core::ptr::addr_of_mut!((*dst).cached_operands));
            core::ptr::addr_of_mut!((*dst).active_leases).write(core::cell::Cell::new(0));
            ArrayMap::init_empty(core::ptr::addr_of_mut!((*dst).mgmt_managers));
        }
    }
}

struct ResolverCore<'cfg> {
    table: ArrayMap<DynamicResolverKey, DynamicResolverEntry<'cfg>, HANDLE_RESOLVER_SLOTS>,
}

impl<'cfg> ResolverCore<'cfg> {
    #[cfg(feature = "std")]
    unsafe fn init_empty(dst: *mut Self) {
        unsafe {
            ArrayMap::init_empty(core::ptr::addr_of_mut!((*dst).table));
        }
    }
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
pub(crate) struct SessionCluster<'cfg, T, U, C, const MAX_RV: usize>
where
    T: crate::transport::Transport + 'cfg,
    U: crate::runtime::consts::LabelUniverse + 'cfg,
    C: crate::runtime::config::Clock + 'cfg,
{
    /// Control-plane state guarded by interior mutability.
    #[cfg(feature = "std")]
    control: core::cell::UnsafeCell<
        std::boxed::Box<ControlCore<'cfg, T, U, C, crate::control::cap::mint::EpochTbl, MAX_RV>>,
    >,
    #[cfg(not(feature = "std"))]
    control: core::cell::UnsafeCell<
        ControlCore<'cfg, T, U, C, crate::control::cap::mint::EpochTbl, MAX_RV>,
    >,
    /// Dynamic resolver table separated from core control state.
    #[cfg(feature = "std")]
    resolvers: core::cell::UnsafeCell<std::boxed::Box<ResolverCore<'cfg>>>,
    #[cfg(not(feature = "std"))]
    resolvers: core::cell::UnsafeCell<ResolverCore<'cfg>>,
    /// Clock for timestamping tap events.
    clock: &'cfg C,
}

impl<'cfg, T, U, C, const MAX_RV: usize> SessionCluster<'cfg, T, U, C, MAX_RV>
where
    T: crate::transport::Transport + 'cfg,
    U: crate::runtime::consts::LabelUniverse + 'cfg,
    C: crate::runtime::config::Clock + 'cfg,
{
    /// Create a new empty cluster with the given clock.
    pub(crate) fn new(clock: &'cfg C) -> Self {
        #[cfg(feature = "std")]
        unsafe {
            let mut control = std::boxed::Box::<
                ControlCore<'cfg, T, U, C, crate::control::cap::mint::EpochTbl, MAX_RV>,
            >::new_uninit();
            let mut resolvers = std::boxed::Box::<ResolverCore<'cfg>>::new_uninit();
            ControlCore::init_empty(control.as_mut_ptr());
            ResolverCore::init_empty(resolvers.as_mut_ptr());
            return Self {
                control: core::cell::UnsafeCell::new(control.assume_init()),
                resolvers: core::cell::UnsafeCell::new(resolvers.assume_init()),
                clock,
            };
        }

        #[cfg(not(feature = "std"))]
        Self {
            control: core::cell::UnsafeCell::new(ControlCore {
                locals: crate::control::lease::core::ControlCore::new(),
                splice_state: DistributedSpliceState::new(),
                cached_operands: ArrayMap::new(),
                active_leases: core::cell::Cell::new(0),
                mgmt_managers: ArrayMap::new(),
            }),
            resolvers: core::cell::UnsafeCell::new(ResolverCore {
                table: ArrayMap::new(),
            }),
            clock,
        }
    }

    #[cfg(feature = "std")]
    #[inline]
    fn control_ptr(
        &self,
    ) -> *mut ControlCore<'cfg, T, U, C, crate::control::cap::mint::EpochTbl, MAX_RV> {
        unsafe {
            (&mut *self.control.get()).as_mut()
                as *mut ControlCore<'cfg, T, U, C, crate::control::cap::mint::EpochTbl, MAX_RV>
        }
    }

    #[cfg(not(feature = "std"))]
    #[inline]
    fn control_ptr(
        &self,
    ) -> *mut ControlCore<'cfg, T, U, C, crate::control::cap::mint::EpochTbl, MAX_RV> {
        self.control.get()
    }

    #[cfg(feature = "std")]
    #[inline]
    fn control_ref_ptr(
        &self,
    ) -> *const ControlCore<'cfg, T, U, C, crate::control::cap::mint::EpochTbl, MAX_RV> {
        unsafe {
            (&*self.control.get()).as_ref()
                as *const ControlCore<'cfg, T, U, C, crate::control::cap::mint::EpochTbl, MAX_RV>
        }
    }

    #[cfg(not(feature = "std"))]
    #[inline]
    fn control_ref_ptr(
        &self,
    ) -> *const ControlCore<'cfg, T, U, C, crate::control::cap::mint::EpochTbl, MAX_RV> {
        self.control.get()
            as *const ControlCore<'cfg, T, U, C, crate::control::cap::mint::EpochTbl, MAX_RV>
    }

    #[cfg(feature = "std")]
    #[inline]
    fn resolvers_ptr(&self) -> *mut ResolverCore<'cfg> {
        unsafe { (&mut *self.resolvers.get()).as_mut() as *mut ResolverCore<'cfg> }
    }

    #[cfg(not(feature = "std"))]
    #[inline]
    fn resolvers_ptr(&self) -> *mut ResolverCore<'cfg> {
        self.resolvers.get()
    }

    #[cfg(feature = "std")]
    #[inline]
    fn resolvers_ref_ptr(&self) -> *const ResolverCore<'cfg> {
        unsafe { (&*self.resolvers.get()).as_ref() as *const ResolverCore<'cfg> }
    }

    #[cfg(not(feature = "std"))]
    #[inline]
    fn resolvers_ref_ptr(&self) -> *const ResolverCore<'cfg> {
        self.resolvers.get() as *const ResolverCore<'cfg>
    }

    /// Internal helper to access mutable control core (NOT PUBLIC).
    fn with_control_mut<F, R>(&self, f: F) -> R
    where
        F: FnOnce(
            &mut ControlCore<'cfg, T, U, C, crate::control::cap::mint::EpochTbl, MAX_RV>,
        ) -> R,
    {
        unsafe { f(&mut *self.control_ptr()) }
    }

    /// Internal helper to access mutable resolver state.
    fn with_resolvers_mut<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut ResolverCore<'cfg>) -> R,
    {
        unsafe { f(&mut *self.resolvers_ptr()) }
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
    #[cfg(test)]
    pub(crate) fn add_rendezvous(
        &self,
        rv: Rendezvous<'cfg, 'cfg, T, U, C, crate::control::cap::mint::EpochTbl>,
    ) -> Result<RendezvousId, CpError> {
        self.with_control_mut(|core| match core.locals.register_local(rv) {
            Ok(id) => Ok(id),
            Err(RegisterRendezvousError::CapacityExceeded) => Err(CpError::ResourceExhausted),
            Err(RegisterRendezvousError::Duplicate(_)) => Err(CpError::ResourceExhausted),
        })
    }

    /// Build and register a local rendezvous from runtime config + transport.
    ///
    /// Public callers should use this entrypoint instead of constructing
    /// rendezvous internals directly.
    pub(crate) fn add_rendezvous_from_config(
        &self,
        config: crate::runtime::config::Config<'cfg, U, C>,
        transport: T,
    ) -> Result<RendezvousId, CpError> {
        self.with_control_mut(|core| {
            match core.locals.register_local_from_config(config, transport) {
                Ok(id) => Ok(id),
                Err(RegisterRendezvousError::CapacityExceeded) => Err(CpError::ResourceExhausted),
                Err(RegisterRendezvousError::Duplicate(_)) => Err(CpError::ResourceExhausted),
            }
        })
    }

    /// Get a local Rendezvous by ID.
    ///
    /// # Safety
    ///
    /// Returns a shared reference to the Rendezvous. Caller must ensure
    /// no concurrent mutation through `with_control_mut`.
    pub(crate) fn get_local(
        &self,
        id: &RendezvousId,
    ) -> Option<&Rendezvous<'cfg, 'cfg, T, U, C, crate::control::cap::mint::EpochTbl>> {
        // SAFETY: We're returning a shared reference from UnsafeCell.
        // This is safe because:
        // - The reference is borrowed from `&self`, so it can't outlive the cluster
        // - Caller must not call mutable methods while holding this reference
        // - This pattern is documented in SessionCluster's safety contract
        unsafe { (*self.control_ref_ptr()).locals.get(id) }
    }

    pub(crate) fn canonical_session_token<K, Mint>(
        &self,
        rv_id: RendezvousId,
        sid: SessionId,
        lane: Lane,
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
        sid: SessionId,
        lane: Lane,
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

        let mint_needs = facets_caps_delegation();

        let outcome = self.drive::<DelegateMintAutomaton<K, Mint>, _, _>(
            rv_id,
            seed,
            |core, rv| Self::init_bundle_context_with_needs(core, rv, mint_needs),
            move |core, graph| Self::init_delegation_children(core, graph, rv_id, &links),
        );

        outcome.ok()
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
    pub(crate) fn lease_port(
        &self,
        rv_id: RendezvousId,
        sid: SessionId,
        lane: Lane,
        role: u8,
    ) -> Result<LaneLease<'cfg, T, U, C, MAX_RV>, RendezvousError> {
        // SAFETY: exclusive access is guaranteed by &self; we immediately move the
        // resulting rendezvous lease out, so no aliasing occurs.
        let core = unsafe { &mut *self.control_ptr() };

        let mut lease = match core.locals.lease::<FullSpec>(rv_id) {
            Ok(lease) => lease,
            Err(LeaseError::UnknownRendezvous(_)) => {
                return Err(RendezvousError::LaneOutOfRange { lane });
            }
            Err(LeaseError::AlreadyLeased(_)) => {
                return Err(RendezvousError::LaneBusy { lane });
            }
        };

        let active = &core.active_leases;

        let current = active.get();
        active.set(current + 1);

        // Extract rendezvous brand before moving lease into guard and emit acquire tap.
        let brand = lease.brand();
        lease.emit_lane_acquire(self.clock.now32(), rv_id, sid, lane);

        let guard = LaneGuard::new(lease, lane, active, true);

        Ok(LaneLease::new(guard, sid, lane, role, brand))
    }

    /// Execute a control-plane effect on a specific local Rendezvous.
    pub(crate) fn run_effect_step(
        &self,
        target: RendezvousId,
        envelope: CpCommand,
    ) -> Result<PendingEffect, CpError> {
        let envelope = match envelope.effect {
            CpEffect::Delegate => envelope.canonicalize_delegate()?,
            _ => envelope,
        };

        if self.get_local(&target).is_some() {
            match envelope.effect {
                CpEffect::SpliceBegin => {
                    if let Some(lane_id) = envelope.lane
                        && let Some(rv) = self.get_local(&target)
                    {
                        let lane = Lane::new(lane_id.raw());
                        let caps = rv.caps_mask_for_lane(lane);
                        if !caps.allows(CpEffect::SpliceBegin) {
                            return Err(CpError::Authorisation {
                                effect: CpEffect::SpliceBegin,
                            });
                        }
                    }

                    let intent = envelope
                        .intent
                        .ok_or(CpError::Splice(SpliceError::InvalidState))?;
                    let seed = intent;
                    let dst_rv = seed.dst_rv;

                    let begin_needs = facets_caps_splice();

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
                                CpError::Splice(SpliceError::InvalidState)
                            }
                            DelegationDriveError::Automaton(err) => err.into(),
                        });
                    }

                    return self.after_local_effect(envelope);
                }
                CpEffect::SpliceCommit => {
                    let sid = envelope
                        .sid
                        .ok_or(CpError::Splice(SpliceError::InvalidSession))?;
                    let ack = envelope
                        .ack
                        .ok_or(CpError::Splice(SpliceError::InvalidState))?;
                    let cached_intent = {
                        let ack_for_cache = ack;
                        self.with_control_mut(|core| {
                            core.locals.get_mut(&target).and_then(|rv| {
                                let session = SessionId::new(ack_for_cache.sid);
                                let dst = RendezvousId::new(ack_for_cache.dst_rv.raw());
                                rv.take_cached_distributed_intent(session, dst)
                            })
                        })
                        .or_else(|| self.distributed_operands(sid).map(|ops| ops.intent(sid)))
                    };

                    let dst_rv = ack.dst_rv;

                    let commit_needs = facets_caps_splice();

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
                                CpError::Splice(SpliceError::InvalidState)
                            }
                            DelegationDriveError::Automaton(err) => err.into(),
                        });
                    }

                    return self.after_local_effect(envelope);
                }
                _ => {
                    if let Some(rv) = self.get_local(&target) {
                        if let Some(lane_id) = envelope.lane {
                            let lane = Lane::new(lane_id.raw());
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
                        let run_result = EffectRunner::run_effect(rv, envelope.clone());
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

    fn dynamic_resolver(&self, key: DynamicResolverKey) -> Option<&DynamicResolverEntry<'cfg>> {
        unsafe { (*self.resolvers_ref_ptr()).table.get(&key) }
    }

    pub(crate) fn set_resolver<'prog, const POLICY: u16, const ROLE: u8, LocalSteps, Mint>(
        &self,
        rv_id: RendezvousId,
        program: &crate::g::advanced::RoleProgram<'prog, ROLE, LocalSteps, Mint>,
        resolver: ResolverRef<'cfg>,
    ) -> Result<(), CpError>
    where
        Mint: MintConfigMarker,
    {
        let facts = ProgramFacts::from_eff_list(program.eff_list());
        for site in facts.dynamic_policy_sites_for(POLICY) {
            let tag = site
                .resource_tag()
                .ok_or(CpError::UnsupportedEffect(site.label()))?;
            self.register_dynamic_policy_resolver(
                rv_id,
                site.eff_index(),
                site.label(),
                site.policy(),
                tag,
                None,
                resolver,
            )?;
        }
        Ok(())
    }

    pub(crate) fn register_dynamic_policy_resolver(
        &self,
        rv_id: RendezvousId,
        eff_index: EffIndex,
        label: u8,
        policy: PolicyMode,
        tag: u8,
        scope_trace: Option<ScopeTrace>,
        resolver: ResolverRef<'cfg>,
    ) -> Result<(), CpError> {
        let key = DynamicResolverKey::new(rv_id, eff_index, tag);
        let policy = match policy {
            PolicyMode::Dynamic { .. } => {
                let _ = policy
                    .dynamic_policy_id()
                    .ok_or(CpError::UnsupportedEffect(label))?;
                // Validate that the tag is a known dynamic control tag
                if !is_dynamic_control_tag(tag) {
                    return Err(CpError::UnsupportedEffect(tag));
                }
                policy
            }
            _ => return Err(CpError::UnsupportedEffect(label)),
        };
        let entry = DynamicResolverEntry {
            resolver,
            policy,
            scope_trace,
        };
        self.with_resolvers_mut(|core| {
            core.table
                .insert(key, entry)
                .map_err(|_| CpError::ResourceExhausted)
        })
    }

    pub(crate) fn resolve_dynamic_policy(
        &self,
        rv_id: RendezvousId,
        session: Option<SessionId>,
        lane: Lane,
        eff_index: EffIndex,
        tag: u8,
        metrics: TransportSnapshot,
        input: [u32; 4],
        attrs: crate::transport::context::PolicyAttrs,
    ) -> Result<DynamicResolution, CpError> {
        let key = DynamicResolverKey::new(rv_id, eff_index, tag);
        let entry = self
            .dynamic_resolver(key)
            .ok_or_else(|| CpError::PolicyAbort { reason: 0 })?;
        let policy = entry.policy;

        let policy_id = policy
            .dynamic_policy_id()
            .ok_or(CpError::PolicyAbort { reason: 6 })?;

        let scope_hint = policy.scope();

        let ctx = ResolverContext::new(
            rv_id,
            session,
            lane,
            eff_index,
            tag,
            metrics,
            scope_hint,
            entry.scope_trace,
            input,
            attrs,
        );

        let resolution = entry
            .resolver
            .resolve(ctx)
            .map_err(|_| CpError::PolicyAbort { reason: policy_id })?;

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
                shard,
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
            (
                LoopContinueKind::TAG | LoopBreakKind::TAG | RouteDecisionKind::TAG,
                DynamicResolution::Defer { retry_hint },
            ) => {
                if scope_hint.is_none() {
                    return Err(CpError::PolicyAbort { reason: policy_id });
                }
                Ok(DynamicResolution::Defer { retry_hint })
            }
            _ => Err(CpError::PolicyAbort { reason: policy_id }),
        }
    }

    pub(crate) fn policy_mode_for(
        &self,
        rv_id: RendezvousId,
        lane: Lane,
        eff_index: EffIndex,
        tag: u8,
    ) -> Result<PolicyMode, CpError> {
        let rv = self.get_local(&rv_id).ok_or(CpError::RendezvousMismatch {
            expected: rv_id.raw(),
            actual: 0,
        })?;
        let lane_rv = Lane::new(lane.raw());
        let key = DynamicResolverKey::new(rv_id, eff_index, tag);
        let policy = rv
            .policy(lane_rv, eff_index, tag)
            .or_else(|| self.dynamic_resolver(key).map(|entry| entry.policy));
        Ok(policy.unwrap_or(PolicyMode::Static))
    }

    pub(crate) fn prepare_splice_operands_from_policy(
        &self,
        rv_id: RendezvousId,
        sid: SessionId,
        src_lane: Lane,
        eff_index: EffIndex,
        tag: u8,
        policy: PolicyMode,
        metrics: TransportSnapshot,
        input: [u32; 4],
        attrs: crate::transport::context::PolicyAttrs,
    ) -> Result<SpliceOperands, CpError> {
        if self.get_local(&rv_id).is_none() {
            return Err(CpError::RendezvousMismatch {
                expected: rv_id.raw(),
                actual: 0,
            });
        }

        let policy_needs = facet_needs(tag, policy);
        let drive_prepare = |dst_rv: RendezvousId,
                             dst_lane: Lane,
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
                    let mut ctx = Self::init_bundle_context_with_needs(core, rv, policy_needs);
                    ctx.set_splice(SpliceGraphContext::default());
                    ctx
                },
                |core, graph| {
                    if dst_rv != rv_id && policy_needs.requires_splice() {
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
                    Err(CpError::Splice(SpliceError::InvalidState))
                }
                Err(DelegationDriveError::Automaton(err)) => Err(err),
            }
        };

        let operands = match policy {
            PolicyMode::Dynamic { .. } => {
                let policy_id = policy.dynamic_policy_id().unwrap_or(0);
                let resolution = self.resolve_dynamic_policy(
                    rv_id,
                    Some(sid),
                    src_lane,
                    eff_index,
                    tag,
                    metrics,
                    input,
                    attrs,
                )?;
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
            PolicyMode::Static => {
                return Err(CpError::UnsupportedEffect(CpEffect::SpliceBegin as u8));
            }
        };

        self.cache_splice_operands(sid, operands)?;
        Ok(operands)
    }

    pub(crate) fn prepare_reroute_handle_from_policy(
        &self,
        rv_id: RendezvousId,
        lane: Lane,
        eff_index: EffIndex,
        tag: u8,
        policy: PolicyMode,
        metrics: TransportSnapshot,
        input: [u32; 4],
        attrs: crate::transport::context::PolicyAttrs,
    ) -> Result<RerouteHandle, CpError> {
        let src_lane_u16 = lane.raw() as u16;
        match policy {
            PolicyMode::Dynamic { .. } => {
                let policy_id = policy
                    .dynamic_policy_id()
                    .ok_or(CpError::PolicyAbort { reason: 6 })?;
                let resolution = self.resolve_dynamic_policy(
                    rv_id, None, lane, eff_index, tag, metrics, input, attrs,
                )?;
                let (dst_rv, dst_lane, shard_override) = match resolution {
                    DynamicResolution::Reroute {
                        dst_rv,
                        dst_lane,
                        shard,
                    } => (dst_rv, dst_lane, shard),
                    _ => return Err(CpError::PolicyAbort { reason: policy_id }),
                };
                let shard = shard_override.unwrap_or_default();
                Ok(RerouteHandle {
                    src_rv: rv_id.raw(),
                    dst_rv: dst_rv.raw(),
                    src_lane: src_lane_u16,
                    dst_lane: dst_lane.raw() as u16,
                    seq_tx: 0,
                    seq_rx: 0,
                    shard,
                    flags: 0,
                })
            }
            PolicyMode::Static => Err(CpError::UnsupportedEffect(CpEffect::Delegate as u8)),
        }
    }

    pub(crate) fn take_cached_splice_operands(&self, sid: SessionId) -> Option<SpliceOperands> {
        self.with_control_mut(|core| core.cached_operands.remove(&sid))
    }

    fn dispatch_splice_intent_with_view(
        &self,
        rv_id: RendezvousId,
        cp_sid: SessionId,
        cp_lane: Lane,
        view: crate::control::cap::mint::HandleView<'_, SpliceIntentKind>,
        generation: Option<Generation>,
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
            return Err(CpError::Splice(SpliceError::GenerationMismatch));
        }

        if rv_id != operands.src_rv {
            return Err(CpError::RendezvousMismatch {
                expected: operands.src_rv.raw(),
                actual: rv_id.raw(),
            });
        }

        if let Some(rv) = self.get_local(&operands.src_rv) {
            let lane = Lane::new(operands.src_lane.raw());
            let current = rv.caps_mask_for_lane(lane);
            if !current.allows(CpEffect::SpliceBegin) || !current.allows(CpEffect::SpliceCommit) {
                let required = current
                    .union(CapsMask::empty().with(CpEffect::SpliceBegin))
                    .union(CapsMask::empty().with(CpEffect::SpliceCommit));
                rv.set_caps_mask_for_lane(lane, required);
            }
        }

        if let Some(rv) = self.get_local(&operands.dst_rv) {
            let lane = Lane::new(operands.dst_lane.raw());
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
        cp_lane: Lane,
        view: crate::control::cap::mint::HandleView<'_, SpliceAckKind>,
        generation: Option<Generation>,
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
            return Err(CpError::Splice(SpliceError::GenerationMismatch));
        }

        if rv_id != operands.dst_rv {
            return Err(CpError::RendezvousMismatch {
                expected: operands.dst_rv.raw(),
                actual: rv_id.raw(),
            });
        }

        if let Some(rv) = self.get_local(&operands.dst_rv) {
            let lane = Lane::new(operands.dst_lane.raw());
            let current = rv.caps_mask_for_lane(lane);
            if !current.allows(CpEffect::SpliceAck) {
                let mut ctx = self.with_control_mut(
                    |core| -> Result<
                        LeaseBundleContext<
                            'cfg,
                            'cfg,
                            T,
                            U,
                            C,
                            crate::control::cap::mint::EpochTbl,
                        >,
                        CpError,
                    > {
                        let mut ctx = Self::init_bundle_context(core, operands.dst_rv);
                        if let Some(rv) = core.locals.get_mut(&operands.dst_rv) {
                            let lane = Lane::new(operands.dst_lane.raw());
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
        cp_lane: Lane,
        view: crate::control::cap::mint::HandleView<'_, CancelKind>,
        generation: Option<Generation>,
    ) -> Result<(), CpError> {
        let (sid_raw, lane_raw) = *view.handle();
        let handle_sid = SessionId::new(sid_raw);
        let handle_lane = Lane::new(lane_raw as u32);
        if handle_sid != cp_sid || handle_lane != cp_lane {
            return Err(CpError::Authorisation {
                effect: CpEffect::CancelBegin,
            });
        }

        let effect_gen = generation.unwrap_or(Generation::ZERO);
        self.run_effect(rv_id, CpCommand::cancel_begin(cp_sid, cp_lane))?;
        self.run_effect(rv_id, CpCommand::cancel_ack(cp_sid, cp_lane, effect_gen))
    }

    fn dispatch_cancel_ack_with_view(
        &self,
        rv_id: RendezvousId,
        cp_sid: SessionId,
        cp_lane: Lane,
        view: crate::control::cap::mint::HandleView<'_, CancelAckKind>,
        generation: Option<Generation>,
    ) -> Result<(), CpError> {
        let (sid_raw, lane_raw) = *view.handle();
        let handle_sid = SessionId::new(sid_raw);
        let handle_lane = Lane::new(lane_raw as u32);
        if handle_sid != cp_sid || handle_lane != cp_lane {
            return Err(CpError::Authorisation {
                effect: CpEffect::CancelAck,
            });
        }

        let effect_gen = generation.unwrap_or(Generation::ZERO);
        self.run_effect(rv_id, CpCommand::cancel_ack(cp_sid, cp_lane, effect_gen))
    }

    fn dispatch_checkpoint_with_view(
        &self,
        rv_id: RendezvousId,
        cp_sid: SessionId,
        cp_lane: Lane,
        view: crate::control::cap::mint::HandleView<'_, CheckpointKind>,
    ) -> Result<(), CpError> {
        let (sid_raw, lane_raw) = *view.handle();
        if SessionId::new(sid_raw) != cp_sid || Lane::new(lane_raw as u32) != cp_lane {
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
        cp_lane: Lane,
        view: crate::control::cap::mint::HandleView<'_, CommitKind>,
        generation: Option<Generation>,
    ) -> Result<(), CpError> {
        let (sid_raw, lane_raw) = *view.handle();
        if SessionId::new(sid_raw) != cp_sid || Lane::new(lane_raw as u32) != cp_lane {
            return Err(CpError::Authorisation {
                effect: CpEffect::Commit,
            });
        }

        let effect_gen = generation.unwrap_or(Generation::ZERO);
        self.run_effect(rv_id, CpCommand::commit(cp_sid, cp_lane, effect_gen))
    }

    fn dispatch_rollback_with_view(
        &self,
        rv_id: RendezvousId,
        cp_sid: SessionId,
        cp_lane: Lane,
        view: crate::control::cap::mint::HandleView<'_, RollbackKind>,
        generation: Option<Generation>,
    ) -> Result<(), CpError> {
        let (sid_raw, lane_raw) = *view.handle();
        if SessionId::new(sid_raw) != cp_sid || Lane::new(lane_raw as u32) != cp_lane {
            return Err(CpError::Authorisation {
                effect: CpEffect::Rollback,
            });
        }

        let effect_gen = generation.unwrap_or(Generation::ZERO);
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
        frame: crate::control::handle::frame::ControlFrame<'_, K>,
        generation: Option<Generation>,
    ) -> Result<Option<crate::control::cap::typed_tokens::CapRegisteredToken<'cluster, K>>, CpError>
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
                    let cp_lane = Lane::new(token_ref.lane().raw());
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
                    let cp_lane = Lane::new(token_ref.lane().raw());
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
                    let cp_lane = Lane::new(token_ref.lane().raw());
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
                    let cp_lane = Lane::new(token_ref.lane().raw());
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
                    let cp_lane = Lane::new(token_ref.lane().raw());
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
                    let cp_lane = Lane::new(token_ref.lane().raw());
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
                    let cp_lane = Lane::new(token_ref.lane().raw());
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
            // External control kinds (defined in adapter crates, etc.) are simple markers
            // that don't require control-plane dispatch beyond token registration.
            // Examples: AcceptHookKind (0xE0), ServerZeroRttReplayReportKind (0xE1)
            _ => {}
        }

        Ok(Some(registered))
    }

    /// Initialize session effects from global protocol projection.
    ///
    /// This method wires the precompiled EffectEnvelope (owned by ProgramFacts) into
    /// the Rendezvous control-plane state. The envelope contains:
    /// - Control-plane effects (CpEffect) to pre-configure
    /// - Tap events to emit during execution
    /// - Resource handles (from `GenericCapToken<K>`) for control operations
    ///
    /// Phase2: This enables the "Global → Local → Rendezvous" pipeline where
    /// the global protocol's Eff tree is projected into runtime state tables.
    ///
    /// # Arguments
    ///
    /// * `rv_id` - The Rendezvous to initialize
    /// * `sid` - Session ID for this projection
    /// * `facts` - Crate-private lowering facts for the projected program
    ///
    /// # Errors
    ///
    /// Returns `CpError::RendezvousMismatch` if the Rendezvous ID is not registered.
    pub(crate) fn init_session_effects(
        &self,
        rv_id: RendezvousId,
        sid: SessionId,
        lane: Lane,
        facts: &ProgramFacts,
    ) -> Result<(), CpError> {
        let core = unsafe { &*self.control_ref_ptr() };

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

        if rv.is_session_registered(sid) {
            return Ok(());
        }

        let envelope = facts.effect_envelope();
        rv.reset_policy(lane);
        let mut control_marker_count = 0u32;
        for marker in envelope.controls() {
            rv.initialise_control_marker(lane, marker);
            control_marker_count = control_marker_count.saturating_add(1);
        }

        let cp_sid = crate::control::types::SessionId::new(sid.raw());
        let cp_lane = crate::control::types::Lane::new(lane.raw());

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
            rv.register_policy(
                lane,
                descriptor.eff_index(),
                descriptor.tag(),
                descriptor.policy(),
            )?;
            if let Some(mask) = session_caps_mask_for_tag(descriptor.tag(), sid, lane) {
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
            crate::observe::core::push(crate::observe::events::EffectInit::new(
                ts,
                sid.raw(),
                applied_effects,
            ));
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
                    .ok_or(CpError::Splice(SpliceError::InvalidSession))?;
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
                    .ok_or(CpError::Splice(SpliceError::InvalidSession))?;

                let expected_ack = envelope.ack.unwrap_or_else(|| operands.ack(sid));

                self.with_control_mut(|core| {
                    let ack = core.splice_state.acknowledge(sid)?;

                    if ack != expected_ack {
                        return Err(CpError::Splice(SpliceError::GenerationMismatch));
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
                    .ok_or(CpError::Splice(SpliceError::InvalidSession))?;

                self.with_control_mut(|core| core.splice_state.commit(sid, envelope.ack))?;
                Ok(PendingEffect::None)
            }
            _ => Ok(PendingEffect::None),
        }
    }
}

impl<'cfg, T, U, C, const MAX_RV: usize> SessionCluster<'cfg, T, U, C, MAX_RV>
where
    T: crate::transport::Transport + 'cfg,
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

    fn init_delegation_children(
        core: &mut ControlCore<'cfg, T, U, C, crate::control::cap::mint::EpochTbl, MAX_RV>,
        graph: &mut LeaseGraph<
            'cfg,
            DelegationLeaseSpec<T, U, C, crate::control::cap::mint::EpochTbl>,
        >,
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

    fn populate_mgmt_links<State>(
        &self,
        rv_id: RendezvousId,
        sid: SessionId,
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
        core: &mut ControlCore<'cfg, T, U, C, crate::control::cap::mint::EpochTbl, MAX_RV>,
        graph: &mut LeaseGraph<'cfg, MgmtLeaseSpec<T, U, C, crate::control::cap::mint::EpochTbl>>,
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

    pub(crate) fn take_mgmt_manager(
        &self,
        rv_id: RendezvousId,
    ) -> Manager<AwaitBegin, { SLOT_COUNT }> {
        self.with_control_mut(|core| {
            core.mgmt_managers
                .remove(&rv_id)
                .unwrap_or_else(|| Manager::<Cold, { SLOT_COUNT }>::new().into_await_begin())
        })
    }

    pub(crate) fn store_mgmt_manager(
        &self,
        rv_id: RendezvousId,
        manager: Manager<AwaitBegin, { SLOT_COUNT }>,
    ) {
        self.with_control_mut(|core| {
            let _ = core.mgmt_managers.insert(rv_id, manager);
        });
    }

    /// Attach a projected endpoint for the specified role with transport binding.
    ///
    /// The binding parameter enables flow operations to automatically invoke
    /// transport operations (e.g., STREAM writes). Use `NoBinding` when the
    /// transport layer is handled separately or for choreography-only tests.
    ///
    /// This method acquires all active lanes defined in the program's choreography.
    /// For `g::par` programs, multiple ports are acquired automatically.
    /// For single-lane programs, only lane 0 is acquired.
    pub(crate) fn attach_endpoint<'lease, 'prog, const ROLE: u8, LocalSteps, Mint, B>(
        &'lease self,
        rv_id: RendezvousId,
        sid: SessionId,
        program: &'prog crate::g::advanced::RoleProgram<'prog, ROLE, LocalSteps, Mint>,
        binding: B,
    ) -> Result<
        crate::endpoint::cursor::CursorEndpoint<
            'cfg,
            ROLE,
            T,
            U,
            C,
            crate::control::cap::mint::EpochTbl,
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
        Mint: crate::control::cap::mint::MintConfigMarker,
    {
        use crate::global::role_program::MAX_LANES;

        let machine = program.machine();
        let mut active_lanes = machine.active_lanes();
        let cursor = crate::global::typestate::PhaseCursor::from_machine(machine);
        let mint = program.mint_config();
        let facts = ProgramFacts::from_eff_list(program.eff_list());

        // Ensure at least lane 0 is active (every endpoint needs at least one lane)
        active_lanes[0] = true;

        // Find primary lane (first active lane, always 0 due to above)
        let primary_lane_index = active_lanes.iter().position(|&active| active).unwrap_or(0);

        // Acquire ports and guards for all active lanes directly on the
        // choreography-defined rendezvous lanes.
        let mut ports: [Option<
            crate::rendezvous::port::Port<'cfg, T, crate::control::cap::mint::EpochTbl>,
        >; MAX_LANES] = [None, None, None, None, None, None, None, None];
        let mut guards: [Option<LaneGuard<'cfg, T, U, C>>; MAX_LANES] =
            [None, None, None, None, None, None, None, None];

        let mut primary_brand: Option<crate::control::brand::Guard<'cfg>> = None;
        let mut primary_lane: Option<Lane> = None;

        for logical_idx in 0..MAX_LANES {
            if active_lanes[logical_idx] {
                let physical_lane = Lane::new(logical_idx as u32);
                self.init_session_effects(rv_id, sid, physical_lane, &facts)?;
                let lease = self.lease_port(rv_id, sid, physical_lane, ROLE)?;
                let (port, guard, brand) =
                    lease.into_port_guard().map_err(AttachError::Rendezvous)?;
                ports[logical_idx] = Some(port);
                guards[logical_idx] = Some(guard);

                // Store primary lane's rendezvous brand for Owner creation
                if logical_idx == primary_lane_index {
                    primary_brand = Some(brand);
                    primary_lane = Some(physical_lane);
                }
            }
        }

        let brand = primary_brand.expect("primary lane brand must be acquired");
        let primary_wire_lane = primary_lane.expect("primary lane must be acquired");

        let owner = crate::control::cap::mint::Owner::new(brand);
        let epoch = crate::control::cap::mint::EndpointEpoch::new();

        // SessionControlCtx uses the primary endpoint lane for control operations
        let liveness_policy = self.with_control_mut(|core| {
            core.locals
                .get_mut(&rv_id)
                .map(|rv| rv.liveness_policy())
                .unwrap_or_default()
        });
        let control = crate::endpoint::control::SessionControlCtx::new(
            rv_id,
            primary_wire_lane,
            Some(self),
            liveness_policy,
            None,
        );

        Ok(crate::endpoint::cursor::CursorEndpoint::from_parts(
            ports,
            guards,
            primary_lane_index,
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
    pub(crate) fn enter<'prog, const ROLE: u8, LocalSteps, Mint, B>(
        &'cfg self,
        rv_id: RendezvousId,
        sid: SessionId,
        program: &'prog crate::g::advanced::RoleProgram<'prog, ROLE, LocalSteps, Mint>,
        binding: B,
    ) -> Result<
        crate::endpoint::Endpoint<
            'cfg,
            ROLE,
            T,
            U,
            C,
            crate::control::cap::mint::EpochTbl,
            MAX_RV,
            Mint,
            B,
        >,
        AttachError,
    >
    where
        B: crate::binding::BindingSlot,
        Mint: crate::control::cap::mint::MintConfigMarker,
    {
        self.attach_endpoint(rv_id, sid, program, binding)
            .map(crate::endpoint::Endpoint::from_cursor)
    }

    #[inline]
    fn init_bundle_context_with_needs(
        core: &mut ControlCore<'cfg, T, U, C, crate::control::cap::mint::EpochTbl, MAX_RV>,
        rv_id: RendezvousId,
        needs: LeaseFacetNeeds,
    ) -> LeaseBundleContext<'cfg, 'cfg, T, U, C, crate::control::cap::mint::EpochTbl>
    where
        T: crate::transport::Transport,
        U: crate::runtime::consts::LabelUniverse,
        C: crate::runtime::config::Clock,
    {
        LeaseBundleContext::from_control_core_with_needs::<MAX_RV>(&mut core.locals, rv_id, needs)
            .unwrap_or_default()
    }

    fn init_bundle_context(
        core: &mut ControlCore<'cfg, T, U, C, crate::control::cap::mint::EpochTbl, MAX_RV>,
        rv_id: RendezvousId,
    ) -> LeaseBundleContext<'cfg, 'cfg, T, U, C, crate::control::cap::mint::EpochTbl>
    where
        T: crate::transport::Transport,
        U: crate::runtime::consts::LabelUniverse,
        C: crate::runtime::config::Clock,
    {
        Self::init_bundle_context_with_needs(core, rv_id, LeaseFacetNeeds::all())
    }

    pub(crate) fn drive_mgmt(
        &self,
        rv_id: RendezvousId,
        sid: SessionId,
        mut seed: MgmtSeed<AwaitBegin>,
    ) -> Result<Reply, MgmtError> {
        self.populate_mgmt_links(rv_id, sid, &mut seed);
        let mgmt_links = Self::collect_mgmt_links(&seed);
        let mgmt_needs =
            <MgmtLeaseSpec<T, U, C, crate::control::cap::mint::EpochTbl> as LeaseSpecFacetNeeds>::facet_needs();

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

    pub(crate) fn on_decision_boundary(&self, rv_id: RendezvousId) -> Result<(), MgmtError> {
        let mut manager = self.take_mgmt_manager(rv_id);
        let result = self.with_control_mut(|core| {
            let mut lease = match core.locals.lease::<FullSpec>(rv_id) {
                Ok(lease) => lease,
                Err(_) => return Err(MgmtError::InvalidTransition),
            };
            lease.with_rendezvous(|rv| {
                let facet = rv.slot_facet();
                facet.on_decision_boundary(rv, &mut manager)
            })
        });
        self.store_mgmt_manager(rv_id, manager);
        result
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
        A: ControlAutomaton<T, U, C, crate::control::cap::mint::EpochTbl>,
        A::GraphSpec: LeaseSpec<NodeId = RendezvousId> + 'cfg,
        Root: FnOnce(
            &mut ControlCore<'cfg, T, U, C, crate::control::cap::mint::EpochTbl, MAX_RV>,
            RendezvousId,
        ) -> <<A::GraphSpec as LeaseSpec>::Facet as LeaseFacet>::Context<'cfg>,
        Init: FnOnce(
            &mut ControlCore<'cfg, T, U, C, crate::control::cap::mint::EpochTbl, MAX_RV>,
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
    T: crate::transport::Transport + 'cfg,
    U: crate::runtime::consts::LabelUniverse + 'cfg,
    C: crate::runtime::config::Clock + 'cfg,
{
    fn drop(&mut self) {
        // SAFETY: `core` is owned by `self` and we're in `drop`, so no aliases exist.
        let core = unsafe { &*self.control_ref_ptr() };
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
    use crate::control::cap::mint::ResourceKind;
    use crate::control::types::{Generation, Lane, SessionId};
    use crate::runtime::config::CounterClock;
    use crate::runtime::consts::DefaultLabelUniverse;
    use crate::transport::{Transport, TransportError, wire::Payload};
    use core::future::{Ready, ready};
    use std::boxed::Box;
    use std::string::String;

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
        type Metrics = ();

        fn open<'a>(&'a self, _local_role: u8, _session_id: u32) -> (Self::Tx<'a>, Self::Rx<'a>) {
            ((), ())
        }

        fn send<'a, 'f>(
            &'a self,
            _tx: &'a mut Self::Tx<'a>,
            _outgoing: crate::transport::Outgoing<'f>,
        ) -> Self::Send<'a>
        where
            'a: 'f,
        {
            ready(Ok(()))
        }

        fn recv<'a>(&'a self, _rx: &'a mut Self::Rx<'a>) -> Self::Recv<'a> {
            ready(Err(TransportError::Failed))
        }

        fn requeue<'a>(&'a self, _rx: &'a mut Self::Rx<'a>) {}

        fn drain_events(&self, _emit: &mut dyn FnMut(crate::transport::TransportEvent)) {}

        fn recv_label_hint<'a>(&'a self, _rx: &'a Self::Rx<'a>) -> Option<u8> {
            None
        }

        fn metrics(&self) -> Self::Metrics {
            ()
        }

        fn apply_pacing_update(&self, _interval_us: u32, _burst_bytes: u16) {}
    }

    #[test]
    fn session_caps_mask_yields_checkpoint_permission() {
        let lane = Lane::new(0);
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
        use crate::control::cap::mint::CapsMask;
        use crate::observe::core::TapEvent;
        use crate::runtime::{config::Config, consts::RING_EVENTS};
        let mut tap_buf = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 256];
        let config = Config::new(&mut tap_buf, &mut slab);
        let rendezvous = Rendezvous::from_config(config, DummyTransport);
        rendezvous.set_caps_mask_for_lane(crate::control::types::Lane::new(0), CapsMask::empty());
        let rv_id = rendezvous.id();

        let cluster: &TestCluster<4> = Box::leak(Box::new(SessionCluster::new(leak_clock())));
        cluster
            .add_rendezvous(rendezvous)
            .expect("register rendezvous");

        let sid = SessionId::new(7);
        let lane = Lane::new(0);
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
            control::cap::mint::{CapsMask, HandleView},
            control::cap::resource_kinds::SpliceHandle,
            observe::core::TapEvent,
            runtime::{config::Config, consts::RING_EVENTS},
        };

        let cluster: &TestCluster<4> = Box::leak(Box::new(SessionCluster::new(leak_clock())));

        let tap_src = Box::leak(Box::new([TapEvent::default(); RING_EVENTS]));
        let slab_src = Box::leak(Box::new([0u8; 256]));
        let src_cfg = Config::new(tap_src, slab_src);
        let src_rendezvous = Rendezvous::from_config(src_cfg, DummyTransport);

        let tap_dst = Box::leak(Box::new([TapEvent::default(); RING_EVENTS]));
        let slab_dst = Box::leak(Box::new([0u8; 256]));
        let dst_cfg = Config::new(tap_dst, slab_dst);
        let dst_rendezvous = Rendezvous::from_config(dst_cfg, DummyTransport);

        let src_id = cluster
            .add_rendezvous(src_rendezvous)
            .expect("register src");
        let dst_id = cluster
            .add_rendezvous(dst_rendezvous)
            .expect("register dst");

        let src_lane = Lane::new(0);
        let dst_lane = Lane::new(1);

        cluster.with_control_mut(|core| {
            let src_rv = core.locals.get_mut(&src_id).unwrap();
            src_rv.set_caps_mask_for_lane(
                crate::control::types::Lane::new(src_lane.raw()),
                CapsMask::empty()
                    .with(CpEffect::SpliceBegin)
                    .with(CpEffect::SpliceCommit),
            );
            let dst_rv = core.locals.get_mut(&dst_id).unwrap();
            dst_rv.set_caps_mask_for_lane(
                crate::control::types::Lane::new(dst_lane.raw()),
                CapsMask::empty(),
            );
        });

        let ack_caps = CapsMask::empty().with(CpEffect::SpliceAck);

        let sid = SessionId::new(7);
        let operands = SpliceOperands::new(
            src_id,
            dst_id,
            src_lane,
            dst_lane,
            Generation::new(0),
            Generation::new(1),
            0,
            0,
        );

        let pending = cluster
            .run_effect_step(src_id, CpCommand::splice_begin(sid, operands))
            .expect("begin effect");
        assert!(matches!(pending, PendingEffect::Dispatch { .. }));

        let handle = SpliceHandle {
            src_rv: src_id.raw(),
            dst_rv: dst_id.raw(),
            src_lane: src_lane.raw() as u16,
            dst_lane: dst_lane.raw() as u16,
            old_gen: operands.old_gen.raw(),
            new_gen: operands.new_gen.raw(),
            seq_tx: operands.seq_tx,
            seq_rx: operands.seq_rx,
            flags: 0,
        };
        let handle_bytes = handle.encode();
        let view = HandleView::decode(&handle_bytes, ack_caps).expect("decode view");

        cluster
            .dispatch_splice_ack_with_view(dst_id, sid, dst_lane, view, None)
            .expect("dispatch succeeds");

        cluster.with_control_mut(|core| {
            let rv = core.locals.get_mut(&dst_id).unwrap();
            let mask = rv.caps_mask_for_lane(crate::control::types::Lane::new(dst_lane.raw()));
            assert!(mask.allows(CpEffect::SpliceAck));
            rv.set_caps_mask_for_lane(
                crate::control::types::Lane::new(dst_lane.raw()),
                CapsMask::empty(),
            );
        });

        let sid_fail = SessionId::new(9);
        let operands_fail = SpliceOperands::new(
            src_id,
            dst_id,
            src_lane,
            dst_lane,
            Generation::new(1),
            Generation::new(2),
            0,
            0,
        );

        cluster
            .run_effect_step(src_id, CpCommand::splice_begin(sid_fail, operands_fail))
            .expect("second begin effect");

        cluster.with_control_mut(|core| {
            let rv = core.locals.get_mut(&dst_id).unwrap();
            rv.set_caps_mask_for_lane(
                crate::control::types::Lane::new(dst_lane.raw()),
                CapsMask::empty(),
            );
        });

        let failure_handle = SpliceHandle {
            src_rv: src_id.raw(),
            dst_rv: dst_id.raw(),
            src_lane: src_lane.raw() as u16,
            dst_lane: dst_lane.raw() as u16,
            old_gen: operands_fail.old_gen.raw(),
            new_gen: operands_fail.new_gen.raw(),
            seq_tx: operands_fail.seq_tx,
            seq_rx: operands_fail.seq_rx,
            flags: 0,
        };
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
                    crate::control::cluster::error::SpliceError::LaneMismatch
                        | crate::control::cluster::error::SpliceError::InvalidState
                )
            ),
            "error was {:?}",
            err
        );

        cluster.with_control_mut(|core| {
            let rv = core.locals.get_mut(&dst_id).unwrap();
            let mask = rv.caps_mask_for_lane(crate::control::types::Lane::new(dst_lane.raw()));
            assert_eq!(mask.bits(), CapsMask::empty().bits());
        });
    }

    #[test]
    fn resolver_defer_for_splice_or_reroute_is_policy_abort() {
        use crate::control::cap::resource_kinds::{RerouteKind, SpliceIntentKind};
        use crate::observe::core::TapEvent;
        use crate::runtime::{config::Config, consts::RING_EVENTS};

        fn defer_resolution(_ctx: ResolverContext) -> Result<DynamicResolution, ResolverError> {
            Ok(DynamicResolution::Defer { retry_hint: 2 })
        }

        let cluster: &TestCluster<4> = Box::leak(Box::new(SessionCluster::new(leak_clock())));
        let mut tap = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 256];
        let config = Config::new(&mut tap, &mut slab);
        let rendezvous = Rendezvous::from_config(config, DummyTransport);
        let rv_id = rendezvous.id();
        cluster
            .add_rendezvous(rendezvous)
            .expect("register rendezvous");

        let policy_id = 913u16;
        let eff_index = EffIndex::new(7);
        let policy = crate::global::const_dsl::PolicyMode::dynamic(policy_id);

        cluster.with_resolvers_mut(|core| {
            assert!(
                core.table
                    .insert(
                        DynamicResolverKey::new(rv_id, eff_index, SpliceIntentKind::TAG),
                        DynamicResolverEntry {
                            resolver: ResolverRef::from_fn(defer_resolution),
                            policy,
                            scope_trace: None,
                        },
                    )
                    .is_ok()
            );
            assert!(
                core.table
                    .insert(
                        DynamicResolverKey::new(rv_id, eff_index, RerouteKind::TAG),
                        DynamicResolverEntry {
                            resolver: ResolverRef::from_fn(defer_resolution),
                            policy,
                            scope_trace: None,
                        },
                    )
                    .is_ok()
            );
        });

        let splice_err = cluster
            .resolve_dynamic_policy(
                rv_id,
                None,
                Lane::new(0),
                eff_index,
                SpliceIntentKind::TAG,
                crate::transport::TransportSnapshot::default(),
                [0; 4],
                crate::transport::context::PolicyAttrs::new(),
            )
            .expect_err("splice defer must be rejected");
        assert_eq!(splice_err, CpError::PolicyAbort { reason: policy_id });

        let reroute_err = cluster
            .resolve_dynamic_policy(
                rv_id,
                None,
                Lane::new(0),
                eff_index,
                RerouteKind::TAG,
                crate::transport::TransportSnapshot::default(),
                [0; 4],
                crate::transport::context::PolicyAttrs::new(),
            )
            .expect_err("reroute defer must be rejected");
        assert_eq!(reroute_err, CpError::PolicyAbort { reason: policy_id });
    }

    #[test]
    fn resolver_context_has_no_internal_coordinate_getters() {
        fn compact_ws(src: &str) -> String {
            let mut out = String::with_capacity(src.len());
            let mut prev_space = false;
            for ch in src.chars() {
                if ch.is_whitespace() {
                    if !prev_space {
                        out.push(' ');
                        prev_space = true;
                    }
                } else {
                    out.push(ch);
                    prev_space = false;
                }
            }
            out
        }

        fn resolver_context_impl_body(src: &str) -> &str {
            let impl_anchor = src
                .find("impl ResolverContext {")
                .expect("ResolverContext impl must exist");
            let open_brace = src[impl_anchor..]
                .find('{')
                .map(|idx| impl_anchor + idx)
                .expect("ResolverContext impl opening brace");
            let mut depth = 0usize;
            for (offset, ch) in src[open_brace..].char_indices() {
                match ch {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            let end = open_brace + offset;
                            return &src[open_brace + 1..end];
                        }
                    }
                    _ => {}
                }
            }
            panic!("ResolverContext impl closing brace");
        }

        let src = include_str!("core.rs");
        let impl_body = compact_ws(resolver_context_impl_body(src));
        assert!(
            !impl_body.contains("fn eff_index("),
            "ResolverContext must not expose eff_index getter"
        );
        assert!(
            !impl_body.contains("fn scope_id("),
            "ResolverContext must not expose scope_id getter"
        );
        assert!(
            !impl_body.contains("fn scope_trace("),
            "ResolverContext must not expose scope_trace getter"
        );
        assert!(
            !impl_body.contains("fn rv_id("),
            "ResolverContext must not expose rv_id getter"
        );
        assert!(
            !impl_body.contains("fn session("),
            "ResolverContext must not expose session getter"
        );
        assert!(
            !impl_body.contains("fn lane("),
            "ResolverContext must not expose lane getter"
        );
        assert!(
            !impl_body.contains("fn tag("),
            "ResolverContext must not expose tag getter"
        );
        assert!(
            !impl_body.contains("fn metrics("),
            "ResolverContext must not expose metrics getter"
        );
    }
}
