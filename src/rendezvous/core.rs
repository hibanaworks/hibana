//! Rendezvous (control plane) primitives.
//!
//! The rendezvous component owns the association tables that map session
//! identifiers to transport lanes. A fully-fledged implementation would manage
//! splice/delegate bookkeeping and generation counters; the current version
//! keeps just enough structure to support endpoint scaffolding while leaving
//! clear extension points.

use core::{
    marker::PhantomData,
    ops::Range,
    sync::atomic::{AtomicU64, Ordering},
};

use super::{
    association::AssocTable,
    capability::{CapEntry, CapTable},
    error::{
        CancelError, CapError, CheckpointError, CommitError, GenError, GenerationRecord,
        RendezvousError, RollbackError, SpliceError,
    },
    port::Port,
    slots::{SLOT_COUNT, SlotArena, SlotStorage},
    splice::{DistributedSpliceTable, PendingSplice, SpliceStateTable},
    tables::{
        AckTable, CheckpointTable, FenceTable, GenTable, LoopTable, PolicyTable, RouteTable,
        VmCapsTable,
    },
};
use crate::{
    control::{
        automaton::txn::{NoopTap, Txn},
        brand::{self, Guard},
        cap::mint::{
            CapShot, CapsMask, EndpointHandle, EndpointResource, GenericCapToken, NonceSeed,
            ResourceKind, VerifiedCap,
        },
        cluster::{
            core::{CpCommand, EffectRunner, SpliceOperands},
            effects::CpEffect,
            error::CpError,
        },
        types::{IncreasingGen, One},
    },
    eff::EffIndex,
    endpoint::affine::LaneGuard,
    epf::{host::HostSlots, vm::Slot},
    global::const_dsl::{ControlMarker, ControlScopeKind, PolicyMode},
    observe::core::{TapEvent, TapRing, emit},
    observe::{
        events::{DelegBegin, DelegSplice, LaneRelease, RawEvent, RollbackOk},
        ids, policy_abort, policy_trap,
    },
    runtime::consts::{DefaultLabelUniverse, LabelUniverse},
    runtime::{
        config::{Clock, Config, ConfigParts, CounterClock},
        mgmt,
    },
    transport::{Transport, TransportEventKind, TransportMetrics},
};

const ENDPOINT_TAG: u8 = 0;
use super::splice::LocalSpliceInvariant;
use crate::control::automaton::distributed::{SpliceAck, SpliceIntent};
use crate::control::types::{Generation, Lane, RendezvousId, SessionId};

pub(crate) struct Rendezvous<
    'rv,
    'cfg,
    T: Transport,
    U: LabelUniverse = DefaultLabelUniverse,
    C: Clock = CounterClock,
    E: crate::control::cap::mint::EpochTable = crate::control::cap::mint::EpochTbl,
> where
    'cfg: 'rv,
{
    brand_marker: PhantomData<brand::Brand<'rv>>,
    id: RendezvousId,
    tap: TapRing<'cfg>,
    slab: *mut [u8],
    slab_marker: PhantomData<&'cfg mut [u8]>,
    lane_range: Range<u32>,
    universe_marker: PhantomData<U>,
    transport: T,
    r#gen: GenTable,
    fences: FenceTable,
    acks: AckTable,
    assoc: AssocTable,
    checkpoints: CheckpointTable,
    splice: SpliceStateTable,
    distributed_splice: DistributedSpliceTable,
    cap_nonce: AtomicU64,
    caps: CapTable,
    loops: LoopTable,
    routes: RouteTable,
    policies: PolicyTable,
    vm_caps: VmCapsTable,
    slot_arena: SlotArena,
    host_slots: HostSlots<'cfg>,
    clock: C,
    liveness_policy: crate::runtime::config::LivenessPolicy,
    /// Counter for generating unique RV IDs
    _next_rv_id: core::cell::Cell<u32>,
    _epoch_marker: PhantomData<E>,
}

/// Affine bundle combining slot storage and host registry access.
pub(crate) struct SlotBundle<'rv, 'cfg: 'rv> {
    arena: &'rv mut SlotArena,
    host_slots: &'rv mut HostSlots<'cfg>,
}

impl<'rv, 'cfg: 'rv> SlotBundle<'rv, 'cfg> {
    #[inline]
    fn new(arena: &'rv mut SlotArena, host_slots: &'rv mut HostSlots<'cfg>) -> Self {
        Self { arena, host_slots }
    }

    /// Borrow the underlying slot arena.
    #[inline]
    pub(crate) fn arena(&mut self) -> &mut SlotArena {
        self.arena
    }

    /// Borrow mutable storage for the given policy slot.
    #[inline]
    pub(crate) fn storage_mut(&mut self, slot: Slot) -> &mut SlotStorage {
        self.arena.storage_mut(slot)
    }

    pub(crate) fn load_commit_with<State>(
        &mut self,
        slot: Slot,
        manager: &mut mgmt::Manager<State, { SLOT_COUNT }>,
    ) -> Result<(), mgmt::MgmtError>
    where
        State: mgmt::ManagerState,
    {
        manager
            .load_commit(slot, self.storage_mut(slot))
            .map(|_| ())
    }

    pub(crate) fn schedule_activate_with<State>(
        &mut self,
        slot: Slot,
        manager: &mut mgmt::Manager<State, { SLOT_COUNT }>,
    ) -> Result<mgmt::TransitionReport, mgmt::MgmtError>
    where
        State: mgmt::ManagerState,
    {
        manager.schedule_activate(slot)
    }

    pub(crate) fn on_decision_boundary_with<State>(
        &mut self,
        manager: &mut mgmt::Manager<State, { SLOT_COUNT }>,
    ) -> Result<(), mgmt::MgmtError>
    where
        State: mgmt::ManagerState,
    {
        let mut idx = 0usize;
        while idx < crate::runtime::mgmt::ALL_SLOTS.len() {
            let slot = crate::runtime::mgmt::ALL_SLOTS[idx];
            let _ = self.on_decision_boundary_for_slot_with(slot, manager)?;
            idx += 1;
        }
        Ok(())
    }

    pub(crate) fn on_decision_boundary_for_slot_with<State>(
        &mut self,
        slot: Slot,
        manager: &mut mgmt::Manager<State, { SLOT_COUNT }>,
    ) -> Result<Option<mgmt::TransitionReport>, mgmt::MgmtError>
    where
        State: mgmt::ManagerState,
    {
        let host_ptr = &mut *self.host_slots as *mut HostSlots<'cfg>;
        let storage_ptr = self.arena.storage_mut(slot) as *mut SlotStorage;
        unsafe { manager.on_decision_boundary(slot, &mut *storage_ptr, &mut *host_ptr) }
    }

    pub(crate) fn revert_with<State>(
        &mut self,
        slot: Slot,
        manager: &mut mgmt::Manager<State, { SLOT_COUNT }>,
    ) -> Result<mgmt::TransitionReport, mgmt::MgmtError>
    where
        State: mgmt::ManagerState,
    {
        let storage_ptr = self.arena.storage_mut(slot) as *mut SlotStorage;
        let host_ptr = &mut *self.host_slots as *mut HostSlots<'cfg>;
        unsafe { manager.revert(slot, &mut *storage_ptr, &mut *host_ptr) }
    }
}

/// Lease guard that retains exclusive access to rendezvous slot resources.
pub(crate) struct SlotBundleLease<'rv, 'cfg: 'rv> {
    bundle: SlotBundle<'rv, 'cfg>,
}

impl<'rv, 'cfg: 'rv> SlotBundleLease<'rv, 'cfg> {
    #[inline]
    fn new(bundle: SlotBundle<'rv, 'cfg>) -> Self {
        Self { bundle }
    }

    #[inline]
    pub(crate) fn load_commit_with<State>(
        &mut self,
        slot: Slot,
        manager: &mut mgmt::Manager<State, { SLOT_COUNT }>,
    ) -> Result<(), mgmt::MgmtError>
    where
        State: mgmt::ManagerState,
    {
        self.bundle.load_commit_with(slot, manager)
    }

    #[inline]
    pub(crate) fn schedule_activate_with<State>(
        &mut self,
        slot: Slot,
        manager: &mut mgmt::Manager<State, { SLOT_COUNT }>,
    ) -> Result<mgmt::TransitionReport, mgmt::MgmtError>
    where
        State: mgmt::ManagerState,
    {
        self.bundle.schedule_activate_with(slot, manager)
    }

    #[inline]
    pub(crate) fn on_decision_boundary_with<State>(
        &mut self,
        manager: &mut mgmt::Manager<State, { SLOT_COUNT }>,
    ) -> Result<(), mgmt::MgmtError>
    where
        State: mgmt::ManagerState,
    {
        self.bundle.on_decision_boundary_with(manager)
    }

    #[inline]
    pub(crate) fn on_decision_boundary_for_slot_with<State>(
        &mut self,
        slot: Slot,
        manager: &mut mgmt::Manager<State, { SLOT_COUNT }>,
    ) -> Result<Option<mgmt::TransitionReport>, mgmt::MgmtError>
    where
        State: mgmt::ManagerState,
    {
        self.bundle
            .on_decision_boundary_for_slot_with(slot, manager)
    }

    #[inline]
    pub(crate) fn revert_with<State>(
        &mut self,
        slot: Slot,
        manager: &mut mgmt::Manager<State, { SLOT_COUNT }>,
    ) -> Result<mgmt::TransitionReport, mgmt::MgmtError>
    where
        State: mgmt::ManagerState,
    {
        self.bundle.revert_with(slot, manager)
    }
}

impl<'rv, 'cfg: 'rv> core::ops::Deref for SlotBundleLease<'rv, 'cfg> {
    type Target = SlotBundle<'rv, 'cfg>;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.bundle
    }
}

impl<'rv, 'cfg: 'rv> core::ops::DerefMut for SlotBundleLease<'rv, 'cfg> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.bundle
    }
}

#[derive(Clone, Copy, Debug)]
struct EffectContext {
    sid: SessionId,
    lane: Lane,
    generation: Option<Generation>,
    fences: Option<(u32, u32)>,
    delegate: Option<DelegateContext>,
}

impl EffectContext {
    fn new(sid: SessionId, lane: Lane) -> Self {
        Self {
            sid,
            lane,
            generation: None,
            fences: None,
            delegate: None,
        }
    }

    fn with_generation(mut self, generation: Generation) -> Self {
        self.generation = Some(generation);
        self
    }

    fn with_fences(mut self, fences: Option<(u32, u32)>) -> Self {
        self.fences = fences;
        self
    }

    fn with_delegate(mut self, delegate: DelegateContext) -> Self {
        self.delegate = Some(delegate);
        self
    }
}

enum EffectResult {
    None,
    Generation(Generation),
}

#[derive(Debug)]
enum EffectError {
    Rollback(RollbackError),
    Commit(super::error::CommitError),
    MissingGeneration,
    Unsupported,
    Splice(SpliceError),
    Delegation(super::error::CapError),
}

#[derive(Clone, Copy, Debug)]
struct DelegateContext {
    claim: bool,
    token: GenericCapToken<EndpointResource>,
}

impl<'rv, 'cfg, T: Transport, U: LabelUniverse, C: Clock, E: crate::control::cap::mint::EpochTable>
    Rendezvous<'rv, 'cfg, T, U, C, E>
where
    'cfg: 'rv,
{
    #[inline]
    pub(crate) fn shorten<'short>(&'short self) -> &'short Rendezvous<'short, 'cfg, T, U, C, E>
    where
        'cfg: 'short,
    {
        let ptr: *const Self = self;
        unsafe { &*ptr.cast::<Rendezvous<'short, 'cfg, T, U, C, E>>() }
    }

    pub(crate) fn with_slot_bundle_lease<'short, F, R>(&'short mut self, f: F) -> R
    where
        'cfg: 'short,
        F: FnOnce(&mut SlotBundleLease<'short, 'cfg>) -> R,
    {
        let mut lease = self.slot_bundle_lease();
        f(&mut lease)
    }

    #[inline]
    pub(crate) fn slot_bundle<'short>(&'short mut self) -> SlotBundle<'short, 'cfg>
    where
        'cfg: 'short,
    {
        SlotBundle::new(&mut self.slot_arena, &mut self.host_slots)
    }

    #[inline]
    pub(crate) fn slot_bundle_lease<'short>(&'short mut self) -> SlotBundleLease<'short, 'cfg>
    where
        'cfg: 'short,
    {
        SlotBundleLease::new(self.slot_bundle())
    }

    pub(crate) fn register_policy(
        &self,
        lane: Lane,
        eff_index: EffIndex,
        tag: u8,
        policy: PolicyMode,
    ) -> Result<(), CpError> {
        self.policies
            .register(lane, eff_index, tag, policy)
            .map_err(|_| CpError::ResourceExhausted)
    }

    pub(crate) fn policy(&self, lane: Lane, eff_index: EffIndex, tag: u8) -> Option<PolicyMode> {
        self.policies.get(lane, eff_index, tag)
    }

    pub(crate) fn reset_policy(&self, lane: Lane) {
        self.policies.reset_lane(lane);
    }

    fn prepare_distributed_splice_operands(
        &self,
        sid: SessionId,
        src_lane: Lane,
        dst_rv: RendezvousId,
        dst_lane: Lane,
        fences: Option<(u32, u32)>,
    ) -> Result<SpliceOperands, CpError> {
        let intent = self
            .begin_distributed_splice(sid, src_lane, dst_rv, dst_lane, fences)
            .map_err(map_splice_error)?;
        Ok(SpliceOperands::from_intent(&intent))
    }

    fn emit_effect(&self, effect: CpEffect, sid: SessionId, arg: u32) {
        let event_id = match effect {
            CpEffect::SpliceBegin => ids::SPLICE_BEGIN,
            CpEffect::SpliceCommit => ids::SPLICE_COMMIT,
            _ => effect.to_tap_event_id(),
        };
        emit(
            self.tap(),
            RawEvent::new(self.clock.now32(), event_id)
                .with_arg0(sid.raw())
                .with_arg1(arg),
        );
    }

    fn emit_policy_event(&self, id: u16, lane: Option<Lane>, arg0: u32, arg1: u32) {
        let causal = lane
            .map(|lane| {
                let raw = lane.raw();
                debug_assert!(
                    raw <= u32::from(u8::MAX),
                    "lane id must fit within causal key encoding"
                );
                let marker = raw as u8 + 1;
                TapEvent::make_causal_key(marker, 0)
            })
            .unwrap_or(0);

        emit(
            self.tap(),
            RawEvent::new(self.clock.now32(), id)
                .with_causal_key(causal)
                .with_arg0(arg0)
                .with_arg1(arg1),
        );
    }

    fn emit_policy_event_with_arg2(
        &self,
        id: u16,
        lane: Option<Lane>,
        arg0: u32,
        arg1: u32,
        arg2: u32,
    ) {
        let causal = lane
            .map(|lane| {
                let raw = lane.raw();
                debug_assert!(
                    raw <= u32::from(u8::MAX),
                    "lane id must fit within causal key encoding"
                );
                let marker = raw as u8 + 1;
                TapEvent::make_causal_key(marker, 0)
            })
            .unwrap_or(0);

        emit(
            self.tap(),
            RawEvent::new(self.clock.now32(), id)
                .with_causal_key(causal)
                .with_arg0(arg0)
                .with_arg1(arg1)
                .with_arg2(arg2),
        );
    }

    fn policy_cancel(&self, sid: SessionId, lane: Lane) {
        self.cancel_begin_at_lane(sid, lane);
        let generation = self.r#gen.last(lane).unwrap_or(Generation(0));
        let _ = self.eval_effect(
            CpEffect::CancelAck,
            EffectContext::new(sid, lane).with_generation(generation),
        );
    }

    fn apply_policy_action(
        &self,
        action: crate::epf::Action,
        sid: Option<SessionId>,
        lane: Option<Lane>,
    ) -> Result<(), CpError> {
        match action {
            crate::epf::Action::Proceed => Ok(()),
            crate::epf::Action::Abort(info) => {
                self.handle_policy_abort(info, sid, lane);
                Err(CpError::PolicyAbort {
                    reason: info.reason,
                })
            }
            crate::epf::Action::Tap { id, arg0, arg1 } => {
                self.emit_policy_event(id, lane, arg0, arg1);
                Ok(())
            }
            crate::epf::Action::Route { .. } => {
                // Route decisions are only meaningful for Slot::Route; ignore here.
                Ok(())
            }
            crate::epf::Action::Defer { .. } => {
                // Defer is a route re-evaluation signal; non-route slots treat it as proceed.
                Ok(())
            }
        }
    }

    fn handle_policy_abort(
        &self,
        info: crate::epf::AbortInfo,
        sid: Option<SessionId>,
        lane: Option<Lane>,
    ) {
        if let Some(sid_val) = sid {
            if let Some(lane_val) = lane {
                self.policy_cancel(sid_val, lane_val);
            }
            if info.trap.is_some() {
                self.emit_policy_event(policy_trap(), lane, info.reason as u32, sid_val.raw());
            }
            self.emit_policy_event(policy_abort(), lane, info.reason as u32, sid_val.raw());
        } else {
            if info.trap.is_some() {
                self.emit_policy_event(policy_trap(), lane, info.reason as u32, 0);
            }
            self.emit_policy_event(policy_abort(), lane, info.reason as u32, 0);
        }
    }

    fn perform_effect(&self, envelope: CpCommand) -> Result<(), CpError> {
        match envelope.effect {
            CpEffect::SpliceBegin => {
                let sid = envelope.sid.ok_or(CpError::Splice(
                    crate::control::cluster::error::SpliceError::InvalidSession,
                ))?;
                let lane = envelope.lane.ok_or(CpError::Splice(
                    crate::control::cluster::error::SpliceError::InvalidLane,
                ))?;
                let generation_input = envelope.generation.ok_or(CpError::Splice(
                    crate::control::cluster::error::SpliceError::GenerationMismatch,
                ))?;
                let fences = envelope.fences;
                let sid = SessionId::new(sid.raw());
                let lane = Lane::new(lane.raw());
                let generation = Generation(generation_input.raw());
                self.begin_splice(sid, lane, fences, generation)
                    .map_err(map_splice_error)
            }
            CpEffect::SpliceAck => {
                let sid = envelope.sid.ok_or(CpError::Splice(
                    crate::control::cluster::error::SpliceError::InvalidSession,
                ))?;
                let Some(intent) = envelope.intent else {
                    let lane = envelope.lane.ok_or(CpError::Splice(
                        crate::control::cluster::error::SpliceError::InvalidLane,
                    ))?;
                    let sid = SessionId::new(sid.raw());
                    let lane = Lane::new(lane.raw());
                    return match self
                        .eval_effect(CpEffect::SpliceAck, EffectContext::new(sid, lane))
                    {
                        Ok(_) => Ok(()),
                        Err(EffectError::Splice(err)) => Err(map_splice_error(err)),
                        Err(EffectError::MissingGeneration) | Err(EffectError::Rollback(_)) => {
                            Err(CpError::Splice(
                                crate::control::cluster::error::SpliceError::InvalidState,
                            ))
                        }
                        Err(EffectError::Unsupported) | Err(EffectError::Delegation(_)) => {
                            Err(CpError::UnsupportedEffect(CpEffect::SpliceAck as u8))
                        }
                        Err(EffectError::Commit(_)) => {
                            Err(CpError::UnsupportedEffect(CpEffect::SpliceAck as u8))
                        }
                    };
                };
                let ack_expected = envelope.ack.ok_or(CpError::Splice(
                    crate::control::cluster::error::SpliceError::InvalidState,
                ))?;

                let ack_result = self
                    .process_splice_intent(&intent)
                    .map_err(map_splice_error)?;

                if ack_result != ack_expected {
                    return Err(CpError::Splice(
                        crate::control::cluster::error::SpliceError::GenerationMismatch,
                    ));
                }

                let dst_lane = Lane::new(intent.dst_lane.raw());
                let sid = SessionId::new(intent.sid);
                self.assoc.register(dst_lane, sid);
                self.splice
                    .commit(dst_lane, sid)
                    .map_err(map_splice_error)?;
                Ok(())
            }
            CpEffect::SpliceCommit => {
                let sid = envelope.sid.ok_or(CpError::Splice(
                    crate::control::cluster::error::SpliceError::InvalidSession,
                ))?;
                let lane = envelope.lane.ok_or(CpError::Splice(
                    crate::control::cluster::error::SpliceError::InvalidLane,
                ))?;
                let sid = SessionId::new(sid.raw());
                let lane = Lane::new(lane.raw());
                let Some(operands) = envelope.splice else {
                    self.commit_splice(sid, lane).map_err(map_splice_error)?;
                    return Ok(());
                };
                self.commit_splice(sid, lane).map_err(map_splice_error)?;
                let released_lane = Lane::new(operands.src_lane.raw());
                if let Some(released_sid) = self.release_lane(released_lane) {
                    self.emit_lane_release(released_sid, released_lane);
                }
                Ok(())
            }
            CpEffect::Delegate => {
                let delegate = envelope.delegate.ok_or(CpError::Delegation(
                    crate::control::cluster::error::DelegationError::InvalidToken,
                ))?;

                let header = delegate.token.header();
                let sid_raw = u32::from_be_bytes([header[0], header[1], header[2], header[3]]);
                let lane_raw = header[4] as u32;

                if let Some(sid) = envelope.sid
                    && sid.raw() != sid_raw
                {
                    return Err(CpError::Delegation(
                        crate::control::cluster::error::DelegationError::InvalidToken,
                    ));
                }
                if let Some(lane) = envelope.lane
                    && lane.raw() != lane_raw
                {
                    return Err(CpError::Delegation(
                        crate::control::cluster::error::DelegationError::InvalidToken,
                    ));
                }

                let sid = SessionId::new(sid_raw);
                let lane = Lane::new(lane_raw);

                let ctx = EffectContext::new(sid, lane).with_delegate(DelegateContext {
                    claim: delegate.claim,
                    token: delegate.token,
                });

                match self.eval_effect(CpEffect::Delegate, ctx) {
                    Ok(_) => Ok(()),
                    Err(EffectError::Delegation(err)) => Err(map_delegate_error(err)),
                    Err(EffectError::Unsupported) => {
                        Err(CpError::UnsupportedEffect(CpEffect::Delegate as u8))
                    }
                    Err(EffectError::Splice(_))
                    | Err(EffectError::MissingGeneration)
                    | Err(EffectError::Rollback(_))
                    | Err(EffectError::Commit(_)) => Err(CpError::Delegation(
                        crate::control::cluster::error::DelegationError::InvalidToken,
                    )),
                }
            }
            CpEffect::Commit => {
                let sid = envelope.sid.ok_or(CpError::Commit(
                    crate::control::cluster::error::CommitError::SessionNotFound,
                ))?;
                let lane = envelope.lane.ok_or(CpError::Commit(
                    crate::control::cluster::error::CommitError::SessionNotFound,
                ))?;
                let generation_input = envelope.generation.ok_or(CpError::Commit(
                    crate::control::cluster::error::CommitError::GenerationMismatch,
                ))?;
                let sid = SessionId::new(sid.raw());
                let lane = Lane::new(lane.raw());
                if self.assoc.get_sid(lane) != Some(sid) {
                    return Err(CpError::Commit(
                        crate::control::cluster::error::CommitError::SessionNotFound,
                    ));
                }
                self.commit_at_lane(sid, lane, Generation(generation_input.raw()))
                    .map_err(map_commit_error)
            }
            CpEffect::CancelBegin => {
                let sid = envelope.sid.ok_or(CpError::Cancel(
                    crate::control::cluster::error::CancelError::SessionNotFound,
                ))?;
                self.cancel_begin(SessionId::new(sid.raw()))
                    .map_err(map_cancel_error)
            }
            CpEffect::CancelAck => {
                let sid = envelope.sid.ok_or(CpError::Cancel(
                    crate::control::cluster::error::CancelError::SessionNotFound,
                ))?;
                let generation_input = envelope.generation.ok_or(CpError::Cancel(
                    crate::control::cluster::error::CancelError::GenerationMismatch,
                ))?;
                self.cancel_ack(
                    SessionId::new(sid.raw()),
                    Generation(generation_input.raw()),
                )
                .map_err(map_cancel_error)
            }
            CpEffect::Checkpoint => {
                let sid = envelope.sid.ok_or(CpError::Checkpoint(
                    crate::control::cluster::error::CheckpointError::SessionNotFound,
                ))?;
                self.checkpoint(SessionId::new(sid.raw()))
                    .map(|_| ())
                    .map_err(map_checkpoint_error)
            }
            CpEffect::Rollback => {
                let sid = envelope.sid.ok_or(CpError::Rollback(
                    crate::control::cluster::error::RollbackError::SessionNotFound,
                ))?;
                let generation_input = envelope.generation.ok_or(CpError::Rollback(
                    crate::control::cluster::error::RollbackError::EpochMismatch,
                ))?;
                self.rollback(
                    SessionId::new(sid.raw()),
                    Generation(generation_input.raw()),
                )
                .map_err(map_rollback_error)
            }
            _ => Err(CpError::UnsupportedEffect(envelope.effect as u8)),
        }
    }

    fn eval_effect(
        &self,
        effect: CpEffect,
        ctx: EffectContext,
    ) -> Result<EffectResult, EffectError> {
        match effect {
            CpEffect::SpliceBegin => {
                let target = ctx.generation.ok_or(EffectError::MissingGeneration)?;
                let mut prev = self.r#gen.last(ctx.lane);
                if prev.is_none() {
                    let _ = self.r#gen.check_and_update(ctx.lane, Generation(0));
                    prev = Some(Generation(0));
                }
                let prev = prev.unwrap_or(Generation(0));

                self.validate_splice_generation(ctx.lane, target)
                    .map_err(EffectError::Splice)?;

                let txn: Txn<LocalSpliceInvariant, IncreasingGen, One> =
                    unsafe { Txn::new(ctx.lane, prev) };
                let mut tap = NoopTap;
                let in_begin = txn.begin(&mut tap);
                let in_acked = in_begin.ack(&mut tap);

                let pending = PendingSplice::new(ctx.sid, target, in_acked, ctx.fences);

                self.splice
                    .begin(ctx.lane, pending)
                    .map_err(EffectError::Splice)?;

                if let Some((tx, rx)) = ctx.fences {
                    self.fences.record_tx(ctx.lane, tx);
                    self.fences.record_rx(ctx.lane, rx);
                }

                let packed = ((ctx.lane.as_wire() as u32) & 0xFF) | ((target.0 as u32) << 16);
                self.emit_effect(effect, ctx.sid, packed);
                Ok(EffectResult::Generation(target))
            }
            CpEffect::SpliceAck => Ok(EffectResult::None),
            CpEffect::SpliceCommit => {
                let pending = self.splice.take(ctx.lane).ok_or(EffectError::Splice(
                    SpliceError::NoPending { lane: ctx.lane },
                ))?;

                let (sid, target, state, fences) = pending.into_parts();

                if sid != ctx.sid {
                    // Reinsert to preserve state before returning error.
                    let _ = self
                        .splice
                        .begin(ctx.lane, PendingSplice::new(sid, target, state, fences));
                    return Err(EffectError::Splice(SpliceError::UnknownSession {
                        sid: ctx.sid,
                    }));
                }

                self.validate_splice_generation(ctx.lane, target)
                    .map_err(EffectError::Splice)?;

                if let Err(err) = self.r#gen.check_and_update(ctx.lane, target) {
                    let _ = self
                        .splice
                        .begin(ctx.lane, PendingSplice::new(sid, target, state, fences));
                    let splice_err = match err {
                        GenError::StaleOrDuplicate(GenerationRecord { lane, last, new }) => {
                            SpliceError::StaleGeneration { lane, last, new }
                        }
                        GenError::Overflow { lane, last } => {
                            SpliceError::GenerationOverflow { lane, last }
                        }
                        GenError::InvalidInitial { lane, new } => {
                            SpliceError::InvalidInitial { lane, new }
                        }
                    };
                    return Err(EffectError::Splice(splice_err));
                }

                let mut tap = NoopTap;
                let _closed = state.commit(&mut tap);

                if let Some((tx, rx)) = fences {
                    self.fences.record_tx(ctx.lane, tx);
                    self.fences.record_rx(ctx.lane, rx);
                }

                let packed = ((ctx.lane.as_wire() as u32) & 0xFF) | ((target.0 as u32) << 16);
                self.emit_effect(effect, ctx.sid, packed);
                Ok(EffectResult::Generation(target))
            }
            CpEffect::Delegate => {
                let Some(delegate) = ctx.delegate else {
                    return Err(EffectError::Unsupported);
                };

                let token = delegate.token;
                let header = token.header();
                let nonce = token.nonce();

                let sid_raw = u32::from_be_bytes([header[0], header[1], header[2], header[3]]);
                let lane_raw = header[4] as u32;
                let role = header[5];
                let kind_raw = header[6];
                let shot_raw = header[7];

                if sid_raw != ctx.sid.raw() || lane_raw != ctx.lane.raw() {
                    return Err(EffectError::Delegation(super::error::CapError::Mismatch));
                }

                let cp_shot = crate::control::cap::mint::CapShot::from_u8(shot_raw)
                    .ok_or(EffectError::Delegation(super::error::CapError::Mismatch))?;
                if kind_raw != ENDPOINT_TAG {
                    return Err(EffectError::Delegation(super::error::CapError::Mismatch));
                }
                let shot = match cp_shot {
                    crate::control::cap::mint::CapShot::One => CapShot::One,
                    crate::control::cap::mint::CapShot::Many => CapShot::Many,
                };

                if !delegate.claim {
                    emit(
                        self.tap(),
                        DelegBegin::new(
                            self.clock.now32(),
                            ctx.sid.raw(),
                            ctx.lane.as_wire() as u32,
                        ),
                    );
                }

                if !delegate.claim {
                    let mut handle = EndpointHandle::new(
                        crate::control::types::SessionId::new(ctx.sid.raw()),
                        ctx.lane,
                        role,
                    );
                    self.mint_cap::<EndpointResource>(ctx.sid, ctx.lane, shot, role, nonce, handle);
                    EndpointResource::zeroize(&mut handle);
                    Ok(EffectResult::None)
                } else {
                    self.claim_cap(&token)
                        .map(|_cap| EffectResult::None)
                        .map_err(EffectError::Delegation)
                }
            }
            CpEffect::Commit => {
                let generation = ctx.generation.ok_or(EffectError::MissingGeneration)?;
                let checkpoint = self.checkpoints.last(ctx.lane).ok_or(EffectError::Commit(
                    CommitError::NoCheckpoint { sid: ctx.sid },
                ))?;

                if self.checkpoints.is_consumed(ctx.lane) {
                    return Err(EffectError::Commit(CommitError::AlreadyCommitted {
                        sid: ctx.sid,
                    }));
                }

                if checkpoint != generation {
                    return Err(EffectError::Commit(CommitError::GenerationMismatch {
                        sid: ctx.sid,
                        expected: checkpoint,
                        got: generation,
                    }));
                }

                self.checkpoints.mark_consumed(ctx.lane);
                self.emit_effect(effect, ctx.sid, generation.0 as u32);
                Ok(EffectResult::Generation(generation))
            }
            CpEffect::CancelBegin => {
                self.acks.record_cancel_begin(ctx.lane);
                self.emit_effect(effect, ctx.sid, ctx.lane.as_wire() as u32);
                Ok(EffectResult::None)
            }
            CpEffect::CancelAck => {
                let generation = ctx.generation.ok_or(EffectError::MissingGeneration)?;
                self.acks.record_cancel_ack(ctx.lane, generation);
                self.emit_effect(effect, ctx.sid, generation.0 as u32);
                Ok(EffectResult::None)
            }
            CpEffect::Checkpoint => {
                let epoch = self.r#gen.last(ctx.lane).unwrap_or(Generation(0));
                self.checkpoints.record(ctx.lane, epoch);
                self.emit_effect(effect, ctx.sid, epoch.0 as u32);
                Ok(EffectResult::Generation(epoch))
            }
            CpEffect::Rollback => {
                let requested = ctx.generation.ok_or(EffectError::MissingGeneration)?;
                let current = self.r#gen.last(ctx.lane).unwrap_or(Generation(0));
                let checkpoint = self.checkpoints.last(ctx.lane).ok_or({
                    EffectError::Rollback(RollbackError::NoCheckpoint { sid: ctx.sid })
                })?;

                if self.checkpoints.is_consumed(ctx.lane) {
                    return Err(EffectError::Rollback(RollbackError::AlreadyConsumed {
                        sid: ctx.sid,
                    }));
                }

                if requested != checkpoint {
                    return Err(EffectError::Rollback(RollbackError::StaleCheckpoint {
                        sid: ctx.sid,
                        requested,
                        current: checkpoint,
                    }));
                }

                if current != requested {
                    return Err(EffectError::Rollback(RollbackError::EpochMismatch {
                        expected: current,
                        got: requested,
                    }));
                }

                self.checkpoints.mark_consumed(ctx.lane);

                self.emit_effect(effect, ctx.sid, requested.0 as u32);
                emit(
                    self.tap(),
                    RollbackOk::new(self.clock.now32(), ctx.sid.raw(), requested.0 as u32),
                );

                Ok(EffectResult::Generation(requested))
            }
            _ => Err(EffectError::Unsupported),
        }
    }

    #[inline]
    pub(crate) fn caps_mask_for_lane(&self, lane: Lane) -> CapsMask {
        self.vm_caps.get(lane)
    }

    #[inline]
    pub(crate) fn set_caps_mask_for_lane(&self, lane: Lane, caps: CapsMask) {
        self.vm_caps.set(lane, caps);
    }
}

impl<'rv, 'cfg, T: Transport, U: LabelUniverse, C: Clock>
    Rendezvous<'rv, 'cfg, T, U, C, crate::control::cap::mint::EpochTbl>
where
    'cfg: 'rv,
{
    #[inline]
    pub(crate) fn allocate_id() -> RendezvousId {
        static GLOBAL_RV_COUNTER: core::sync::atomic::AtomicU32 =
            core::sync::atomic::AtomicU32::new(1);
        RendezvousId::new(
            GLOBAL_RV_COUNTER.fetch_add(1, core::sync::atomic::Ordering::Relaxed) as u16,
        )
    }

    /// Write a rendezvous directly into the destination slot.
    ///
    /// # Safety
    /// `dst` must point to valid, writable storage for `Self`.
    pub(crate) unsafe fn init_from_config(
        dst: *mut Self,
        rv_id: RendezvousId,
        config: Config<'cfg, U, C>,
        transport: T,
    ) {
        let ConfigParts {
            tap_buf,
            slab,
            lane_range,
            clock,
            liveness_policy,
        } = config.into_parts();

        unsafe {
            core::ptr::addr_of_mut!((*dst).brand_marker).write(PhantomData);
            core::ptr::addr_of_mut!((*dst).id).write(rv_id);
            core::ptr::addr_of_mut!((*dst).tap).write(TapRing::from_storage(tap_buf));
            core::ptr::addr_of_mut!((*dst).slab).write(slab as *mut [u8]);
            core::ptr::addr_of_mut!((*dst).slab_marker).write(PhantomData);
            core::ptr::addr_of_mut!((*dst).lane_range)
                .write((lane_range.start as u32)..(lane_range.end as u32));
            core::ptr::addr_of_mut!((*dst).universe_marker).write(PhantomData);
            core::ptr::addr_of_mut!((*dst).transport).write(transport);
            core::ptr::addr_of_mut!((*dst).r#gen).write(GenTable::new());
            core::ptr::addr_of_mut!((*dst).fences).write(FenceTable::new());
            core::ptr::addr_of_mut!((*dst).acks).write(AckTable::new());
            core::ptr::addr_of_mut!((*dst).assoc).write(AssocTable::new());
            core::ptr::addr_of_mut!((*dst).checkpoints).write(CheckpointTable::new());
            core::ptr::addr_of_mut!((*dst).splice).write(SpliceStateTable::new());
            core::ptr::addr_of_mut!((*dst).distributed_splice).write(DistributedSpliceTable::new());
            core::ptr::addr_of_mut!((*dst).cap_nonce).write(AtomicU64::new(0));
            core::ptr::addr_of_mut!((*dst).caps).write(CapTable::new());
            core::ptr::addr_of_mut!((*dst).loops).write(LoopTable::new());
            core::ptr::addr_of_mut!((*dst).routes).write(RouteTable::new());
            core::ptr::addr_of_mut!((*dst).policies).write(PolicyTable::new());
            core::ptr::addr_of_mut!((*dst).vm_caps).write(VmCapsTable::new());
            core::ptr::addr_of_mut!((*dst).slot_arena).write(SlotArena::new());
            core::ptr::addr_of_mut!((*dst).host_slots).write(HostSlots::new());
            core::ptr::addr_of_mut!((*dst).clock).write(clock);
            core::ptr::addr_of_mut!((*dst).liveness_policy).write(liveness_policy);
            core::ptr::addr_of_mut!((*dst)._next_rv_id)
                .write(core::cell::Cell::new(u32::from(rv_id.raw()) + 1000));
            core::ptr::addr_of_mut!((*dst)._epoch_marker).write(PhantomData);
        }
    }

    #[cfg(test)]
    pub(crate) fn from_config(config: Config<'cfg, U, C>, transport: T) -> Self {
        let rv_id = Self::allocate_id();
        let mut rendezvous = core::mem::MaybeUninit::<Self>::uninit();
        unsafe {
            Self::init_from_config(rendezvous.as_mut_ptr(), rv_id, config, transport);
            rendezvous.assume_init()
        }
    }
}

impl<'rv, 'cfg, T: Transport, U: LabelUniverse, C: Clock, E: crate::control::cap::mint::EpochTable>
    Rendezvous<'rv, 'cfg, T, U, C, E>
where
    'cfg: 'rv,
{
    pub(crate) fn initialise_control_marker(&self, lane: Lane, marker: &ControlMarker) {
        match marker.scope_kind {
            ControlScopeKind::Loop => {
                self.loops.reset_lane(lane);
            }
            ControlScopeKind::Checkpoint => {
                self.checkpoints.reset_lane(lane);
            }
            ControlScopeKind::Cancel => {
                self.acks.reset_lane(lane);
            }
            ControlScopeKind::Splice => {
                self.splice.reset_lane(lane);
            }
            ControlScopeKind::Reroute
            | ControlScopeKind::Policy
            | ControlScopeKind::Route
            | ControlScopeKind::None => {}
        }
    }

    #[inline]
    pub(crate) fn checkpoint_at_lane(&self, sid: SessionId, lane: Lane) -> Generation {
        match self.eval_effect(CpEffect::Checkpoint, EffectContext::new(sid, lane)) {
            Ok(EffectResult::Generation(epoch)) => epoch,
            Ok(EffectResult::None) => unreachable!("checkpoint effect must yield generation"),
            Err(_) => unreachable!("checkpoint effect cannot fail"),
        }
    }

    #[inline]
    pub(crate) fn commit_at_lane(
        &self,
        sid: SessionId,
        lane: Lane,
        generation: Generation,
    ) -> Result<(), CommitError> {
        match self.eval_effect(
            CpEffect::Commit,
            EffectContext::new(sid, lane).with_generation(generation),
        ) {
            Ok(_) => Ok(()),
            Err(EffectError::Commit(err)) => Err(err),
            Err(EffectError::MissingGeneration)
            | Err(EffectError::Unsupported)
            | Err(EffectError::Splice(_))
            | Err(EffectError::Delegation(_))
            | Err(EffectError::Rollback(_)) => {
                unreachable!("commit effect failure is fully covered")
            }
        }
    }

    #[inline]
    pub(crate) fn cancel_begin_at_lane(&self, sid: SessionId, lane: Lane) {
        self.eval_effect(CpEffect::CancelBegin, EffectContext::new(sid, lane))
            .expect("cancel begin evaluation must not fail");
    }

    pub(crate) fn is_session_registered(&self, sid: SessionId) -> bool {
        self.assoc.find_lane(sid).is_some()
    }

    pub(crate) fn release_lane(&self, lane: Lane) -> Option<SessionId> {
        let sid = self.assoc.get_sid(lane)?;
        let remaining = self.assoc.decrement(lane, sid)?;
        if remaining > 0 {
            return None;
        }
        self.reset_lane_state(lane);
        Some(sid)
    }

    fn reset_lane_state(&self, lane: Lane) {
        self.r#gen.reset_lane(lane);
        self.fences.reset_lane(lane);
        self.acks.reset_lane(lane);
        self.checkpoints.reset_lane(lane);
        self.splice.reset_lane(lane);
        self.caps.purge_lane(lane);
        self.loops.reset_lane(lane);
        self.routes.reset_lane(lane);
        self.vm_caps.reset_lane(lane);
    }

    #[inline]
    pub(crate) fn emit_lane_release(&self, sid: SessionId, lane: Lane) {
        emit(
            self.tap(),
            LaneRelease::new(
                self.now32(),
                self.id.raw() as u32,
                sid.raw(),
                lane.raw() as u16,
            ),
        );
    }
}

/// **RAII witness for exclusive lane access.**
///
/// `LaneLease<'a, 'cfg, ...>` is the **affine witness** that guarantees exclusive access
/// to a transport lane. It is parameterized by a **borrow lifetime** `'a` to enforce
/// the invariant that **all leases must be dropped before the borrow expires**:
///
/// ```text
/// Drop order guarantee (enforced by lifetime 'a):
///   LaneLease<'a, ...> → Port<'a, ...> → &'a Rendezvous (borrow expires)
/// ```
///
/// The key insight is that `'a` is the **lifetime of the borrow** from `lease_port(&'a self)`,
/// which is **independent** of the `Rendezvous<'rv, 'cfg, ...>` invariant lifetime `'rv`.
/// This allows **nested scopes** where leases are dropped before the Rendezvous itself:
///
/// ```text
/// let rv = Rendezvous::from_config(...);  // 'rv starts
/// {
///     let lease = rv.lease_port(...);     // 'a: shorter borrow
/// }                                        // 'a ends, lease dropped
///                                          // rv can now be moved/dropped
/// ```
///
/// # Type-Level Guarantees
///
/// 1. **Affine Linearity**: Each `LaneLease` owns a unique lane slot; moving or dropping
///    it revokes access to that lane.
/// 2. **Lifetime Binding**: The `'a` lifetime ensures that the lease does not outlive
///    the borrow of the `Rendezvous`.
/// 3. **RAII Release**: On drop, the lane is automatically released back to the
///    `Rendezvous` unless explicitly transferred via `into_port()`.
///
/// # Example
///
/// ```ignore
/// let mut rv = Rendezvous::from_config(...);
/// {
///     let lease = rv.lease_port(sid, lane, role)?;
///     let port = lease.port();
///     // ... use port
/// } // ← lease dropped here, lane released, borrow 'a expires
/// // ← rv can now be safely dropped or moved
/// ```
///
/// # POPL Justification
///
/// This design implements **separation logic** with **region polymorphism**:
/// - `LaneLease<'a, ...>` is the **ownership token** for a lane, valid during region `'a`.
/// - The borrow `'a` acts as the **region annotation** ensuring temporal safety.
/// - Drop implementation is the **linear consumption** that releases the resource.
/// - The distinction between `'rv` (invariant lifetime of Rendezvous) and `'a` (covariant
///   borrow lifetime) enables **flexible scoping** without sacrificing safety.
///
/// Affine MPST + RAII underpin the theoretical foundation for this module.
///
/// # Visibility
///
/// This type is internal implementation, hidden from public docs but
/// accessible to integration tests. Public API users obtain endpoints via
/// [`SessionCluster::enter`](crate::substrate::SessionCluster::enter).
///
/// # Cluster Ownership Model
///
/// `LaneLease` now owns the rendezvous lease outright. This ties the borrow
/// lifetime `'lease` to the rendezvous itself and removes the need for raw
/// pointers or `PhantomData` hacks. The ownership chain is purely typed:
/// Cluster → RendezvousLease → LaneLease.
///
/// # Safety Invariants (documented for POPL/SOSP/OSDI)
///
/// 1. `cluster_ptr` always points to a valid `SessionCluster` during `'lease`
/// 2. Only `LaneLease::Drop` calls back into the cluster to release the lane
/// 3. SessionCluster guarantees: no duplicate leases for same lane
/// 4. SessionCluster guarantees: no Rendezvous write access while lease held
/// 5. Cluster must not move while lease is alive (enforced by the PhantomData borrow)
///
/// # Observable Properties
///
/// - LANE_ACQUIRE tap event on lease creation (via `SessionCluster::lease_port`)
/// - LANE_RELEASE tap event on Drop
/// - Streaming checker verifies acquire/release pairs match (similar to cancel begin/ack)
pub(crate) struct LaneLease<'cfg, T, U, C, const MAX_RV: usize>
where
    T: Transport,
    U: LabelUniverse + 'cfg,
    C: Clock + 'cfg,
{
    /// Lease-backed guard over the parent rendezvous.
    /// Uses the default EpochTbl because LaneLease is only used to create new endpoints.
    guard: Option<LaneGuard<'cfg, T, U, C>>,
    /// Session identifier.
    sid: SessionId,
    /// Lane identifier.
    lane: Lane,
    /// Role for the port.
    role: u8,
    /// Rendezvous brand for typed owner construction.
    brand: crate::control::brand::Guard<'cfg>,
}

impl<'cfg, T, U, C, const MAX_RV: usize> LaneLease<'cfg, T, U, C, MAX_RV>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
{
    /// Internal constructor (called by `SessionCluster::lease_port`).
    /// The caller must ensure no duplicate leases for the same `(rv_id, lane)` pair.
    pub(crate) fn new(
        guard: LaneGuard<'cfg, T, U, C>,
        sid: SessionId,
        lane: Lane,
        role: u8,
        brand: crate::control::brand::Guard<'cfg>,
    ) -> Self {
        Self {
            guard: Some(guard),
            sid,
            lane,
            role,
            brand,
        }
    }

    #[allow(clippy::type_complexity)]
    pub(crate) fn into_port_guard(
        mut self,
    ) -> Result<
        (
            Port<'cfg, T, crate::control::cap::mint::EpochTbl>,
            LaneGuard<'cfg, T, U, C>,
            crate::control::brand::Guard<'cfg>,
        ),
        RendezvousError,
    > {
        let mut guard = self.guard.take().expect("lane lease retains guard");
        let port = {
            let lease_ref = guard.lease.as_mut().expect("guard retains lease");
            let rv_ptr: *mut Rendezvous<'cfg, 'cfg, T, U, C, crate::control::cap::mint::EpochTbl> =
                lease_ref.with_rendezvous(core::ptr::from_mut);
            // SAFETY: `LaneLease` holds the unique rendezvous lease while the guard
            // is alive, so the rendezvous cannot move or be aliased here.
            let rv: &'cfg Rendezvous<'cfg, 'cfg, T, U, C, crate::control::cap::mint::EpochTbl> =
                unsafe { &*rv_ptr };
            rv.acquire_port(self.sid, self.lane, self.role)?
        };
        guard.detach_lease();
        Ok((port, guard, self.brand))
    }
}

impl<'cfg, T, U, C, const MAX_RV: usize> Drop for LaneLease<'cfg, T, U, C, MAX_RV>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
{
    fn drop(&mut self) {
        if let Some(guard) = self.guard.take() {
            drop(guard);
        }
    }
}

impl<'rv, 'cfg, T: Transport, U: LabelUniverse, C: Clock, E: crate::control::cap::mint::EpochTable>
    Rendezvous<'rv, 'cfg, T, U, C, E>
where
    'cfg: 'rv,
{
    /// Get the unique identifier for this Rendezvous instance.
    #[cfg(test)]
    pub(crate) fn id(&self) -> RendezvousId {
        self.id
    }

    #[inline]
    pub(crate) fn brand(&self) -> Guard<'rv> {
        Guard::new()
    }

    /// Observability ring; pushing events only needs `&self` because the ring
    /// is single-producer and internally synchronised.
    pub(crate) fn tap(&self) -> &TapRing<'cfg> {
        &self.tap
    }

    #[inline]
    pub(crate) fn liveness_policy(&self) -> crate::runtime::config::LivenessPolicy {
        self.liveness_policy
    }

    pub(crate) fn now32(&self) -> u32 {
        self.clock.now32()
    }

    /// Access the capability table for token registration.
    #[inline]
    pub(crate) fn caps(&self) -> &CapTable {
        &self.caps
    }

    /// Release a capability from the CapTable by nonce.
    #[inline]
    pub(crate) fn release_cap_by_nonce(
        &self,
        nonce: &[u8; crate::control::cap::mint::CAP_NONCE_LEN],
    ) {
        self.caps.release_by_nonce(nonce);
    }

    pub(crate) fn acquire_port<'a>(
        &'a self,
        sid: SessionId,
        lane: Lane,
        role: u8,
    ) -> Result<Port<'a, T, crate::control::cap::mint::EpochTbl>, RendezvousError>
    where
        'rv: 'a,
    {
        if !self.lane_range.contains(&lane.0) {
            return Err(RendezvousError::LaneOutOfRange { lane });
        }
        let first_attach = match self.assoc.get_sid(lane) {
            None => {
                self.assoc.register(lane, sid);
                true
            }
            Some(existing) if existing == sid => {
                self.assoc
                    .increment(lane, sid)
                    .expect("lane attachment count overflow");
                false
            }
            Some(_) => {
                return Err(RendezvousError::LaneBusy { lane });
            }
        };

        if first_attach {
            // Emit CpEffect::Open for the lane's inaugural attachment.
            emit(
                self.tap(),
                RawEvent::new(
                    self.clock.now32(),
                    crate::control::cluster::effects::CpEffect::Open.to_tap_event_id(),
                )
                .with_arg0(sid.raw())
                .with_arg1(lane.0),
            );

            self.r#gen.reset_lane(lane);
            self.fences.reset_lane(lane);
            self.acks.reset_lane(lane);
            self.checkpoints.reset_lane(lane);
            self.loops.reset_lane(lane);
            self.routes.reset_lane(lane);
        }
        let tap = self.tap();
        let clock_ref: &dyn Clock = &self.clock;
        let (tx, rx) = self.transport.open(role, sid.raw());
        Ok(Port::new(
            &self.transport,
            tap,
            clock_ref,
            &self.vm_caps,
            &self.loops,
            &self.routes,
            &self.host_slots,
            self.slab,
            lane,
            role,
            self.id,
            tx,
            rx,
        ))
    }

    // ============================================================================
    // Capability methods
    // ============================================================================

    #[inline]
    pub(crate) fn next_nonce_seed(&self) -> NonceSeed {
        let ordinal = self.cap_nonce.fetch_add(1, Ordering::Relaxed);
        NonceSeed::counter(ordinal)
    }

    pub(crate) fn mint_cap<K: ResourceKind>(
        &self,
        sid: SessionId,
        lane: Lane,
        shot: CapShot,
        dest_role: u8,
        nonce: [u8; 16],
        mut handle: K::Handle,
    ) {
        let kind_tag = K::TAG;
        let registered_sid = self
            .assoc
            .get_sid(lane)
            .expect("session must be registered before minting capabilities");
        debug_assert_eq!(
            registered_sid, sid,
            "capabilities must be minted on a lane registered to the session"
        );
        debug_assert!(
            self.assoc.is_active(lane),
            "lane must be active before minting capabilities"
        );

        let handle_bytes = K::encode_handle(&handle);
        let caps_mask = K::caps_mask(&handle);
        K::zeroize(&mut handle);

        let entry = CapEntry {
            sid,
            lane,
            kind_tag,
            shot,
            role: dest_role,
            consumed: false,
            nonce,
            caps_mask,
            handle: handle_bytes,
        };
        self.caps
            .insert_entry(entry)
            .expect("capability table is full");

        let tap = self.tap();
        emit(
            tap,
            RawEvent::new(self.clock.now32(), crate::observe::cap_mint::<K>())
                .with_arg0(sid.raw())
                .with_arg1(((lane.as_wire() as u32) << 16) | (dest_role as u32)),
        );
    }

    pub(crate) fn claim_cap<K: crate::control::cap::mint::ResourceKind>(
        &self,
        token: &GenericCapToken<K>,
    ) -> Result<VerifiedCap<K>, CapError> {
        // Extract fields from 40B token
        let header = token.header();
        let nonce = token.nonce();

        let sid = u32::from_be_bytes([header[0], header[1], header[2], header[3]]);
        let lane = header[4];
        let role = header[5];
        let kind_tag = header[6];
        let shot_u8 = header[7];

        let sid = SessionId(sid);
        let lane = Lane(lane as u32);
        let shot = CapShot::from_u8(shot_u8).ok_or(CapError::UnknownToken)?;

        // Check if AUTO (all zeros)
        if nonce == [0u8; crate::control::cap::mint::CAP_NONCE_LEN]
            && header == [0u8; crate::control::cap::mint::CAP_HEADER_LEN]
        {
            return Err(CapError::UnknownToken);
        }

        if self.assoc.get_sid(lane) != Some(sid) {
            return Err(CapError::WrongSessionOrLane);
        }

        if kind_tag != K::TAG {
            return Err(CapError::Mismatch);
        }

        // Use nonce-based claim path (trusted domain - no MAC verification)
        let (exhausted, handle_bytes) = self
            .caps
            .claim_by_nonce(&nonce, sid, lane, kind_tag, role, shot, token.caps_mask())
            .map_err(|e| match e {
                CapError::UnknownToken => CapError::UnknownToken,
                CapError::WrongSessionOrLane => CapError::WrongSessionOrLane,
                CapError::Exhausted => CapError::Exhausted,
                CapError::Mismatch => CapError::Mismatch,
            })?;

        let claim_id = crate::observe::cap_claim::<K>();
        let exhaust_id = crate::observe::cap_exhaust::<K>();

        if exhausted {
            let tap = self.tap();
            emit(
                tap,
                RawEvent::new(self.clock.now32(), exhaust_id)
                    .with_arg0(sid.raw())
                    .with_arg1(0),
            );
        }

        let tap = self.tap();
        emit(
            tap,
            RawEvent::new(self.clock.now32(), claim_id)
                .with_arg0(sid.raw())
                .with_arg1(0),
        );

        let handle = K::decode_handle(handle_bytes).map_err(|_| CapError::Mismatch)?;
        Ok(VerifiedCap::new(handle))
    }

    // ============================================================================
    // Distributed splice methods
    // ============================================================================

    pub(crate) fn begin_distributed_splice(
        &self,
        sid: SessionId,
        src_lane: Lane,
        dst_rv: RendezvousId,
        dst_lane: Lane,
        fences: Option<(u32, u32)>,
    ) -> Result<SpliceIntent, SpliceError> {
        // Verify session exists and is on the expected lane
        if self.assoc.get_sid(src_lane) != Some(sid) {
            return Err(SpliceError::UnknownSession { sid });
        }

        // Get current generation and calculate next
        let old_gen = self.r#gen.last(src_lane).unwrap_or(Generation(0));
        let new_gen = Generation(old_gen.0.saturating_add(1));

        if new_gen.0 == 0 {
            return Err(SpliceError::GenerationOverflow {
                lane: src_lane,
                last: old_gen,
            });
        }

        let intent = SpliceIntent::new(
            self.id,
            dst_rv,
            sid.raw(),
            old_gen,
            new_gen,
            fences.map(|f| f.0).unwrap_or(0),
            fences.map(|f| f.1).unwrap_or(0),
            src_lane,
            dst_lane,
        );

        // Store intent locally
        self.distributed_splice.insert(intent)?;

        // Emit tap event
        emit(
            self.tap(),
            DelegSplice::new(
                self.clock.now32(),
                src_lane.0 | (dst_lane.0 << 8) | ((new_gen.0 as u32) << 16),
                sid.raw(),
            ),
        );

        Ok(intent)
    }

    pub(crate) fn take_cached_distributed_intent(
        &self,
        sid: SessionId,
        dst_rv: RendezvousId,
    ) -> Option<SpliceIntent> {
        self.distributed_splice
            .take(sid, self.id, dst_rv)
            .map(|entry| entry.intent)
    }

    pub(crate) fn process_splice_intent(
        &self,
        intent: &SpliceIntent,
    ) -> Result<SpliceAck, SpliceError> {
        let dst_rv: RendezvousId = intent.dst_rv;
        let dst_lane: Lane = intent.dst_lane;
        let old_gen: Generation = intent.old_gen;
        let new_gen: Generation = intent.new_gen;

        // Validate this RV is the intended destination
        if dst_rv != self.id {
            return Err(SpliceError::RendezvousIdMismatch {
                expected: dst_rv,
                got: self.id,
            });
        }

        // Validate lane is in range
        if !self.lane_range.contains(&dst_lane.raw()) {
            return Err(SpliceError::LaneOutOfRange { lane: dst_lane });
        }

        // Check lane is available
        if self.assoc.is_active(dst_lane) {
            return Err(SpliceError::LaneMismatch {
                expected: dst_lane,
                provided: Lane(0), // Dummy value
            });
        }

        // Validate generation monotonicity
        let last_gen = self.r#gen.last(dst_lane).unwrap_or(Generation(0));

        // Allow old_gen to be 0 (new session) or match the last generation
        if old_gen.0 != 0 && old_gen.0 != last_gen.0 {
            return Err(SpliceError::StaleGeneration {
                lane: dst_lane,
                last: last_gen,
                new: new_gen,
            });
        }

        // Begin local splice using typestate transaction (ack immediately for local state).
        let txn: Txn<LocalSpliceInvariant, IncreasingGen, One> =
            unsafe { Txn::new(dst_lane, last_gen) };
        let mut tap = NoopTap;
        let in_begin = txn.begin(&mut tap);
        let in_acked = in_begin.ack(&mut tap);

        let pending = PendingSplice::new(
            SessionId(intent.sid),
            new_gen,
            in_acked,
            Some((intent.seq_tx, intent.seq_rx)),
        );
        let begin_result = self.splice.begin(dst_lane, pending);
        begin_result?;

        // Update generation table
        if last_gen.0 == 0 {
            let _ = self.r#gen.check_and_update(dst_lane, Generation(0));
            self.r#gen
                .check_and_update(dst_lane, new_gen)
                .map_err(|err| match err {
                    GenError::StaleOrDuplicate(GenerationRecord { lane, last, new }) => {
                        SpliceError::StaleGeneration { lane, last, new }
                    }
                    GenError::Overflow { lane, last } => {
                        SpliceError::GenerationOverflow { lane, last }
                    }
                    GenError::InvalidInitial { lane, new } => {
                        SpliceError::InvalidInitial { lane, new }
                    }
                })?;
        } else {
            self.r#gen
                .check_and_update(dst_lane, new_gen)
                .map_err(|err| match err {
                    GenError::StaleOrDuplicate(GenerationRecord { lane, last, new }) => {
                        SpliceError::StaleGeneration { lane, last, new }
                    }
                    GenError::Overflow { lane, last } => {
                        SpliceError::GenerationOverflow { lane, last }
                    }
                    GenError::InvalidInitial { lane, new } => {
                        SpliceError::InvalidInitial { lane, new }
                    }
                })?;
        }

        // Create ack using control::automaton::distributed::SpliceAck::new
        let ack = SpliceAck::new(
            intent.src_rv,
            self.id,
            intent.sid,
            new_gen,
            dst_lane,
            intent.seq_tx,
            intent.seq_rx,
        );

        Ok(ack)
    }

    // ============================================================================
    // Checkpoint / Cancel / Rollback methods
    // ============================================================================

    pub(crate) fn cancel_begin(&self, sid: SessionId) -> Result<(), CancelError> {
        let lane = self
            .assoc
            .find_lane(sid)
            .ok_or(CancelError::UnknownSession { sid })?;
        self.cancel_begin_at_lane(sid, lane);
        Ok(())
    }

    pub(crate) fn cancel_ack(&self, sid: SessionId, r#gen: Generation) -> Result<(), CancelError> {
        let lane = self
            .assoc
            .find_lane(sid)
            .ok_or(CancelError::UnknownSession { sid })?;
        self.eval_effect(
            CpEffect::CancelAck,
            EffectContext::new(sid, lane).with_generation(r#gen),
        )
        .expect("cancel ack evaluation must not fail");
        Ok(())
    }

    pub(crate) fn checkpoint(&self, sid: SessionId) -> Result<Generation, CheckpointError> {
        let lane = self
            .assoc
            .find_lane(sid)
            .ok_or(CheckpointError::UnknownSession { sid })?;
        Ok(self.checkpoint_at_lane(sid, lane))
    }

    pub(crate) fn rollback(&self, sid: SessionId, epoch: Generation) -> Result<(), RollbackError> {
        let lane = self
            .assoc
            .find_lane(sid)
            .ok_or(RollbackError::UnknownSession { sid })?;
        self.rollback_at_lane(sid, lane, epoch)
    }

    pub(crate) fn rollback_at_lane(
        &self,
        sid: SessionId,
        lane: Lane,
        epoch: Generation,
    ) -> Result<(), RollbackError> {
        match self.eval_effect(
            CpEffect::Rollback,
            EffectContext::new(sid, lane).with_generation(epoch),
        ) {
            Ok(_) => Ok(()),
            Err(EffectError::Rollback(err)) => Err(err),
            Err(EffectError::MissingGeneration)
            | Err(EffectError::Unsupported)
            | Err(EffectError::Splice(_))
            | Err(EffectError::Delegation(_))
            | Err(EffectError::Commit(_)) => {
                unreachable!("rollback effect failure is fully covered")
            }
        }
    }

    pub(crate) fn validate_splice_generation(
        &self,
        lane: Lane,
        new_gen: Generation,
    ) -> Result<(), SpliceError> {
        match self.r#gen.last(lane) {
            None => {
                if new_gen.0 >= 1 {
                    Ok(())
                } else {
                    Err(SpliceError::InvalidInitial { lane, new: new_gen })
                }
            }
            Some(prev) if prev.0 == u16::MAX => {
                Err(SpliceError::GenerationOverflow { lane, last: prev })
            }
            Some(prev) if new_gen.0 > prev.0 => Ok(()),
            Some(prev) => Err(SpliceError::StaleGeneration {
                lane,
                last: prev,
                new: new_gen,
            }),
        }
    }
}

// ============================================================================
// SpliceDelegate trait has been DELETED.
// All splice operations now go through control::CpCommand and EffectRunner.
// The control-plane mini-kernel architecture is responsible for rendezvous access control.

fn map_splice_error(err: SpliceError) -> CpError {
    match err {
        SpliceError::LaneOutOfRange { .. } => {
            CpError::Splice(crate::control::cluster::error::SpliceError::InvalidLane)
        }
        SpliceError::LaneMismatch { .. }
        | SpliceError::InProgress { .. }
        | SpliceError::NoPending { .. }
        | SpliceError::SeqnoMismatch { .. } => {
            CpError::Splice(crate::control::cluster::error::SpliceError::InvalidState)
        }
        SpliceError::UnknownSession { .. } => {
            CpError::Splice(crate::control::cluster::error::SpliceError::InvalidSession)
        }
        SpliceError::StaleGeneration { .. }
        | SpliceError::GenerationOverflow { .. }
        | SpliceError::InvalidInitial { .. } => {
            CpError::Splice(crate::control::cluster::error::SpliceError::GenerationMismatch)
        }
        SpliceError::RemoteRendezvousMismatch { expected, got }
        | SpliceError::RendezvousIdMismatch { expected, got } => CpError::RendezvousMismatch {
            expected: expected.raw(),
            actual: got.raw(),
        },
        SpliceError::PendingTableFull => CpError::ResourceExhausted,
    }
}

fn map_delegate_error(err: super::error::CapError) -> CpError {
    let deleg_err = match err {
        super::error::CapError::UnknownToken | super::error::CapError::WrongSessionOrLane => {
            crate::control::cluster::error::DelegationError::InvalidToken
        }
        super::error::CapError::Exhausted => {
            crate::control::cluster::error::DelegationError::Exhausted
        }
        super::error::CapError::Mismatch => {
            crate::control::cluster::error::DelegationError::ShotMismatch
        }
    };
    CpError::Delegation(deleg_err)
}

fn map_cancel_error(err: super::error::CancelError) -> CpError {
    match err {
        super::error::CancelError::UnknownSession { .. } => {
            CpError::Cancel(crate::control::cluster::error::CancelError::SessionNotFound)
        }
    }
}

fn map_checkpoint_error(err: super::error::CheckpointError) -> CpError {
    match err {
        super::error::CheckpointError::UnknownSession { .. } => {
            CpError::Checkpoint(crate::control::cluster::error::CheckpointError::SessionNotFound)
        }
    }
}

fn map_commit_error(err: super::error::CommitError) -> CpError {
    match err {
        super::error::CommitError::NoCheckpoint { .. } => {
            CpError::Commit(crate::control::cluster::error::CommitError::NoCheckpoint)
        }
        super::error::CommitError::AlreadyCommitted { .. } => {
            CpError::Commit(crate::control::cluster::error::CommitError::AlreadyCommitted)
        }
        super::error::CommitError::GenerationMismatch { .. } => {
            CpError::Commit(crate::control::cluster::error::CommitError::GenerationMismatch)
        }
    }
}

fn map_rollback_error(err: super::error::RollbackError) -> CpError {
    match err {
        super::error::RollbackError::UnknownSession { .. } => {
            CpError::Rollback(crate::control::cluster::error::RollbackError::SessionNotFound)
        }
        super::error::RollbackError::NoCheckpoint { .. } => {
            CpError::Rollback(crate::control::cluster::error::RollbackError::EpochNotFound)
        }
        super::error::RollbackError::StaleCheckpoint { .. }
        | super::error::RollbackError::EpochMismatch { .. } => {
            CpError::Rollback(crate::control::cluster::error::RollbackError::EpochMismatch)
        }
        super::error::RollbackError::AlreadyConsumed { .. } => {
            CpError::Rollback(crate::control::cluster::error::RollbackError::AfterCommit)
        }
    }
}

// ============================================================================
// Local splice operations (used by EffectRunner)
// ============================================================================

impl<'rv, 'cfg, T, U, C, E> Rendezvous<'rv, 'cfg, T, U, C, E>
where
    'cfg: 'rv,
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
{
    /// Begin a local splice operation.
    ///
    /// This is called by EffectRunner::run_effect() for CpEffect::SpliceBegin.
    fn begin_splice(
        &self,
        sid: SessionId,
        lane: Lane,
        fences: Option<(u32, u32)>,
        generation: Generation,
    ) -> Result<(), SpliceError> {
        let ctx = EffectContext::new(sid, lane)
            .with_generation(generation)
            .with_fences(fences);

        match self.eval_effect(CpEffect::SpliceBegin, ctx) {
            Ok(_) => Ok(()),
            Err(EffectError::Splice(err)) => Err(err),
            Err(EffectError::MissingGeneration)
            | Err(EffectError::Unsupported)
            | Err(EffectError::Commit(_))
            | Err(EffectError::Delegation(_))
            | Err(EffectError::Rollback(_)) => {
                unreachable!("splice begin effect failure is fully covered")
            }
        }
    }

    /// Commit a local splice operation.
    ///
    /// This is called by EffectRunner::run_effect() for CpEffect::SpliceCommit.
    fn commit_splice(&self, sid: SessionId, lane: Lane) -> Result<(), SpliceError> {
        let ctx = EffectContext::new(sid, lane);
        match self.eval_effect(CpEffect::SpliceCommit, ctx) {
            Ok(_) => Ok(()),
            Err(EffectError::Splice(err)) => Err(err),
            Err(EffectError::MissingGeneration)
            | Err(EffectError::Unsupported)
            | Err(EffectError::Delegation(_))
            | Err(EffectError::Rollback(_))
            | Err(EffectError::Commit(_)) => {
                unreachable!("splice commit failure is fully covered")
            }
        }
    }

    /// Drain transport telemetry and emit tap events for downstream observers.
    fn flush_transport_events(&self) -> Option<crate::transport::TransportEvent> {
        let tap = self.tap();
        let clock = &self.clock;
        let mut last_loss = None;
        let mut emit_event = |event: crate::transport::TransportEvent| {
            let (arg0, arg1) = event.encode_tap_args();
            if matches!(event.kind, TransportEventKind::Loss) {
                last_loss = Some(event);
            }
            emit(
                tap,
                crate::observe::events::TransportEvent::new(clock.now32(), arg0, arg1),
            );
        };
        self.transport.drain_events(&mut emit_event);
        let snapshot = self.transport.metrics().snapshot();
        if let Some(payload) = snapshot.encode_tap_metrics() {
            let (arg0, arg1) = payload.primary;
            emit(
                tap,
                crate::observe::events::TransportMetrics::new(clock.now32(), arg0, arg1),
            );
            if let Some((ext0, ext1)) = payload.extension {
                emit(
                    tap,
                    crate::observe::events::TransportMetricsExt::new(clock.now32(), ext0, ext1),
                );
            }
        }
        last_loss
    }
}

impl<'rv, 'cfg, T, U, C, E> EffectRunner for Rendezvous<'rv, 'cfg, T, U, C, E>
where
    'cfg: 'rv,
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
{
    fn run_effect(&self, envelope: CpCommand) -> Result<(), CpError> {
        let envelope = match envelope.effect {
            CpEffect::Delegate => envelope.canonicalize_delegate()?,
            _ => envelope,
        };
        let lane_opt = envelope.lane.map(|lane| Lane::new(lane.raw()));
        let sid_opt = envelope.sid.map(|sid| SessionId::new(sid.raw()));
        let caps_mask = lane_opt
            .map(|lane| self.vm_caps.get(lane))
            .unwrap_or(CapsMask::allow_all());

        let policy_event = RawEvent::new(self.clock.now32(), envelope.effect.to_tap_event_id())
            .with_arg0(sid_opt.map_or(0, |sid| sid.raw()))
            .with_arg1(lane_opt.map_or(0, |lane| lane.raw()));

        let handle_data = envelope.delegate.as_ref().map(|delegate| {
            (
                delegate.token.resource_tag(),
                delegate.token.handle_bytes(),
                delegate.token.caps_mask(),
            )
        });

        let _ = self.flush_transport_events();
        let transport_metrics = self.transport.metrics().snapshot();
        let policy_input =
            crate::epf::slot_contract::slot_default_input(crate::epf::vm::Slot::Rendezvous);
        let policy_digest = self
            .host_slots
            .active_digest(crate::epf::vm::Slot::Rendezvous);
        let event_hash = crate::epf::hash_tap_event(&policy_event);
        let signals_input_hash = crate::epf::hash_policy_input(policy_input);
        let transport_snapshot_hash = crate::epf::hash_transport_snapshot(transport_metrics);
        let replay_transport = crate::epf::replay_transport_inputs(transport_metrics);
        let replay_transport_presence = crate::epf::replay_transport_presence(transport_metrics);
        let mode_id = crate::epf::policy_mode_tag(
            self.host_slots
                .policy_mode(crate::epf::vm::Slot::Rendezvous),
        );
        self.emit_policy_event_with_arg2(
            ids::POLICY_AUDIT,
            lane_opt,
            policy_digest,
            event_hash,
            signals_input_hash,
        );
        self.emit_policy_event_with_arg2(
            ids::POLICY_AUDIT_EXT,
            lane_opt,
            0,
            transport_snapshot_hash,
            ((crate::epf::slot_tag(crate::epf::vm::Slot::Rendezvous) as u32) << 24)
                | ((mode_id as u32) << 16),
        );
        self.emit_policy_event_with_arg2(
            ids::POLICY_REPLAY_EVENT,
            lane_opt,
            policy_event.ts,
            policy_event.id as u32,
            policy_event.arg0,
        );
        self.emit_policy_event_with_arg2(
            ids::POLICY_REPLAY_EVENT_EXT,
            lane_opt,
            policy_event.arg1,
            policy_event.arg2,
            policy_event.causal_key as u32,
        );
        self.emit_policy_event_with_arg2(
            ids::POLICY_REPLAY_INPUT0,
            lane_opt,
            policy_input[0],
            policy_input[1],
            policy_input[2],
        );
        self.emit_policy_event_with_arg2(
            ids::POLICY_REPLAY_INPUT1,
            lane_opt,
            policy_input[3],
            0,
            0,
        );
        self.emit_policy_event_with_arg2(
            ids::POLICY_REPLAY_TRANSPORT0,
            lane_opt,
            replay_transport[0],
            replay_transport[1],
            replay_transport[2],
        );
        self.emit_policy_event_with_arg2(
            ids::POLICY_REPLAY_TRANSPORT1,
            lane_opt,
            replay_transport[3],
            replay_transport_presence as u32,
            0,
        );
        let action = crate::epf::run_with(
            &self.host_slots,
            crate::epf::vm::Slot::Rendezvous,
            &policy_event,
            caps_mask,
            sid_opt,
            lane_opt,
            move |ctx| {
                let _ = handle_data;
                ctx.set_transport_snapshot(transport_metrics);
                ctx.set_policy_input(policy_input);
            },
        );
        let verdict = action.verdict();
        let verdict_meta = ((crate::epf::verdict_tag(verdict) as u32) << 24)
            | ((crate::epf::verdict_arm(verdict) as u32) << 16);
        self.emit_policy_event_with_arg2(
            ids::POLICY_AUDIT_RESULT,
            lane_opt,
            verdict_meta,
            crate::epf::verdict_reason(verdict) as u32,
            self.host_slots
                .last_fuel_used(crate::epf::vm::Slot::Rendezvous) as u32,
        );

        self.apply_policy_action(action, sid_opt, lane_opt)?;

        if !caps_mask.allows(envelope.effect) {
            return Err(CpError::Authorisation {
                effect: envelope.effect,
            });
        }

        self.perform_effect(envelope)
    }

    fn prepare_splice_operands(
        &self,
        sid: SessionId,
        src_lane: Lane,
        dst_rv: RendezvousId,
        dst_lane: Lane,
        fences: Option<(u32, u32)>,
    ) -> Result<SpliceOperands, CpError> {
        self.prepare_distributed_splice_operands(sid, src_lane, dst_rv, dst_lane, fences)
    }
}

// ============================================================================

#[cfg(test)]
mod epf_tests {
    use super::*;
    use crate::{
        control::cap::mint::CapsMask,
        control::cluster::core::{CpCommand, EffectRunner},
        control::types::{Lane, SessionId},
        observe::core::TapEvent,
        runtime::{config::Config, consts::RING_EVENTS},
        transport::{Transport, TransportError, wire::Payload},
    };
    use core::future::{Ready, ready};

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
            _payload: Payload<'f>,
            _dest_role: u8,
        ) -> Self::Send<'a>
        where
            'a: 'f,
        {
            ready(Ok(()))
        }

        fn recv<'a>(&'a self, _rx: &'a mut Self::Rx<'a>) -> Self::Recv<'a> {
            ready(Err(TransportError::Offline))
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
    fn run_effect_requires_authorised_caps() {
        let mut tap_buf = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 256];
        let config = Config::new(&mut tap_buf, &mut slab);
        let rendezvous = Rendezvous::from_config(config, DummyTransport);

        let sid = SessionId::new(1);
        let lane = Lane::new(0);

        rendezvous.vm_caps.set(lane, CapsMask::empty());

        let envelope = CpCommand::checkpoint(SessionId::new(sid.raw()), Lane::new(lane.raw()));

        let result = EffectRunner::run_effect(&rendezvous, envelope);

        assert!(matches!(
            result,
            Err(CpError::Authorisation {
                effect: CpEffect::Checkpoint
            })
        ));
    }

    #[test]
    fn run_effect_allows_when_caps_present() {
        let mut tap_buf = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 256];
        let config = Config::new(&mut tap_buf, &mut slab);
        let rendezvous = Rendezvous::from_config(config, DummyTransport);

        let sid = SessionId::new(2);
        let lane = Lane::new(1);

        let envelope = CpCommand::checkpoint(SessionId::new(sid.raw()), Lane::new(lane.raw()));

        let result = EffectRunner::run_effect(&rendezvous, envelope);

        assert!(matches!(result, Err(CpError::Checkpoint(_))));
    }
}

// ============================================================================
// Facet API - ZST-based constrained access
// ============================================================================

impl<'rv, 'cfg, T, U, C, E> Rendezvous<'rv, 'cfg, T, U, C, E>
where
    'cfg: 'rv,
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
{
    /// Borrow slot storage and host registry as a constrained facet.
    ///
    /// This returns a `SlotBundle` that provides access only to slot
    /// storage operations (staging/active/backup buffers) and host VM
    /// registry, without exposing other rendezvous state.
    pub(crate) fn slot_facet(&mut self) -> SlotFacet<T, U, C, E> {
        SlotFacet::new()
    }

    /// Borrow capability table as a constrained facet.
    pub(crate) fn caps_facet(&mut self) -> CapsFacet<T, U, C, E> {
        CapsFacet::new()
    }

    /// Borrow splice coordination state as a constrained facet.
    pub(crate) fn splice_facet(&mut self) -> SpliceFacet<T, U, C, E> {
        SpliceFacet::new()
    }

    /// Borrow observation ring as a constrained facet.
    pub(crate) fn observe_facet(&self) -> ObserveFacet<'_, 'cfg> {
        ObserveFacet::new(self.tap())
    }
}

/// Capability-focused facet that exposes only CapTable operations.
#[derive(Default)]
pub(crate) struct CapsFacet<T, U, C, E>(PhantomData<(T, U, C, E)>)
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable;

impl<T, U, C, E> Copy for CapsFacet<T, U, C, E>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
{
}

impl<T, U, C, E> Clone for CapsFacet<T, U, C, E>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
{
    fn clone(&self) -> Self {
        *self
    }
}

impl<T, U, C, E> CapsFacet<T, U, C, E>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
{
    #[inline]
    pub(crate) const fn new() -> Self {
        Self(PhantomData)
    }

    /// Mint a capability token and register it in the CapTable.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn mint_cap<K: crate::control::cap::mint::ResourceKind>(
        self,
        rendezvous: &Rendezvous<'_, '_, T, U, C, E>,
        sid: SessionId,
        lane: Lane,
        shot: crate::control::cap::mint::CapShot,
        dest_role: u8,
        nonce: [u8; 16],
        handle: K::Handle,
    ) {
        rendezvous.mint_cap::<K>(sid, lane, shot, dest_role, nonce, handle)
    }

    /// Generate the next nonce seed for capability minting.
    #[inline]
    pub(crate) fn next_nonce_seed(
        self,
        rendezvous: &Rendezvous<'_, '_, T, U, C, E>,
    ) -> crate::control::cap::mint::NonceSeed {
        rendezvous.next_nonce_seed()
    }
}

/// Splice-focused facet that exposes only splice coordination operations.
#[derive(Default)]
pub(crate) struct SpliceFacet<T, U, C, E>(PhantomData<(T, U, C, E)>)
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable;

impl<T, U, C, E> Copy for SpliceFacet<T, U, C, E>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
{
}

impl<T, U, C, E> Clone for SpliceFacet<T, U, C, E>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
{
    fn clone(&self) -> Self {
        *self
    }
}

impl<T, U, C, E> SpliceFacet<T, U, C, E>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
{
    #[inline]
    pub(crate) const fn new() -> Self {
        Self(PhantomData)
    }

    pub(crate) fn begin(
        self,
        rendezvous: &mut Rendezvous<'_, '_, T, U, C, E>,
        sid: SessionId,
        lane: Lane,
        fences: Option<(u32, u32)>,
        generation: Generation,
    ) -> Result<(), super::error::SpliceError> {
        rendezvous.begin_splice(sid, lane, fences, generation)
    }

    pub(crate) fn commit(
        self,
        rendezvous: &mut Rendezvous<'_, '_, T, U, C, E>,
        sid: SessionId,
        lane: Lane,
    ) -> Result<(), super::error::SpliceError> {
        rendezvous.commit_splice(sid, lane)
    }

    pub(crate) fn release_lane(self, rendezvous: &Rendezvous<'_, '_, T, U, C, E>, lane: Lane) {
        if let Some(sid) = rendezvous.release_lane(lane) {
            rendezvous.emit_lane_release(sid, lane);
        }
    }
}

/// Observation facet that exposes tap emission without leaking rendezvous state.
#[derive(Clone, Copy)]
pub(crate) struct ObserveFacet<'tap, 'cfg> {
    tap: &'tap crate::observe::core::TapRing<'cfg>,
}

impl<'tap, 'cfg> ObserveFacet<'tap, 'cfg> {
    #[inline]
    pub(crate) const fn new(tap: &'tap crate::observe::core::TapRing<'cfg>) -> Self {
        Self { tap }
    }

    /// Borrow the underlying tap ring (read-only).
    #[inline]
    pub(crate) fn tap(&self) -> &'tap crate::observe::core::TapRing<'cfg> {
        self.tap
    }
}

/// Slot management facet that exposes slot operations for policy bytecode management.
///
/// This facet is a zero-sized marker; all state lives in the rendezvous. Methods explicitly
/// receive the rendezvous handle, keeping the facet trivially copyable and suitable for
/// `const fn` projection.
#[derive(Default)]
pub(crate) struct SlotFacet<T, U, C, E>(PhantomData<(T, U, C, E)>)
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable;

impl<T, U, C, E> Copy for SlotFacet<T, U, C, E>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
{
}

impl<T, U, C, E> Clone for SlotFacet<T, U, C, E>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
{
    fn clone(&self) -> Self {
        *self
    }
}

impl<T, U, C, E> SlotFacet<T, U, C, E>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
{
    #[inline]
    pub(crate) const fn new() -> Self {
        Self(PhantomData)
    }

    /// Load and commit bytecode to a slot.
    pub(crate) fn load_commit<State>(
        self,
        rendezvous: &mut Rendezvous<'_, '_, T, U, C, E>,
        slot: crate::epf::vm::Slot,
        manager: &mut crate::runtime::mgmt::Manager<State, { SLOT_COUNT }>,
    ) -> Result<(), crate::runtime::mgmt::MgmtError>
    where
        State: crate::runtime::mgmt::ManagerState,
    {
        rendezvous.with_slot_bundle_lease(|lease| lease.load_commit_with(slot, manager))
    }

    /// Schedule activation for a slot after staging a verified policy image.
    pub(crate) fn schedule_activate<State>(
        self,
        rendezvous: &mut Rendezvous<'_, '_, T, U, C, E>,
        slot: crate::epf::vm::Slot,
        manager: &mut crate::runtime::mgmt::Manager<State, { SLOT_COUNT }>,
    ) -> Result<crate::runtime::mgmt::TransitionReport, crate::runtime::mgmt::MgmtError>
    where
        State: crate::runtime::mgmt::ManagerState,
    {
        rendezvous.with_slot_bundle_lease(|lease| lease.schedule_activate_with(slot, manager))
    }

    /// Apply pending policy activations at a decision boundary.
    pub(crate) fn on_decision_boundary<State>(
        self,
        rendezvous: &mut Rendezvous<'_, '_, T, U, C, E>,
        manager: &mut crate::runtime::mgmt::Manager<State, { SLOT_COUNT }>,
    ) -> Result<(), crate::runtime::mgmt::MgmtError>
    where
        State: crate::runtime::mgmt::ManagerState,
    {
        rendezvous.with_slot_bundle_lease(|lease| lease.on_decision_boundary_with(manager))
    }

    /// Apply pending policy activation for a specific slot at a decision boundary.
    pub(crate) fn on_decision_boundary_for_slot<State>(
        self,
        rendezvous: &mut Rendezvous<'_, '_, T, U, C, E>,
        slot: crate::epf::vm::Slot,
        manager: &mut crate::runtime::mgmt::Manager<State, { SLOT_COUNT }>,
    ) -> Result<Option<crate::runtime::mgmt::TransitionReport>, crate::runtime::mgmt::MgmtError>
    where
        State: crate::runtime::mgmt::ManagerState,
    {
        rendezvous
            .with_slot_bundle_lease(|lease| lease.on_decision_boundary_for_slot_with(slot, manager))
    }

    /// Revert a slot to the previous active policy.
    pub(crate) fn revert<State>(
        self,
        rendezvous: &mut Rendezvous<'_, '_, T, U, C, E>,
        slot: crate::epf::vm::Slot,
        manager: &mut crate::runtime::mgmt::Manager<State, { SLOT_COUNT }>,
    ) -> Result<crate::runtime::mgmt::TransitionReport, crate::runtime::mgmt::MgmtError>
    where
        State: crate::runtime::mgmt::ManagerState,
    {
        rendezvous.with_slot_bundle_lease(|lease| lease.revert_with(slot, manager))
    }
}
