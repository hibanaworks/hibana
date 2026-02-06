//! Rendezvous (control plane) primitives.
//!
//! The rendezvous component owns the association tables that map session
//! identifiers to transport lanes. A fully-fledged implementation would manage
//! splice/delegate bookkeeping and generation counters; the current version
//! keeps just enough structure to support endpoint scaffolding while leaving
//! clear extension points.

use core::{
    convert::TryFrom,
    marker::PhantomData,
    mem,
    ops::Range,
    sync::atomic::{AtomicU64, Ordering},
};

use super::error::{
    CommitError as RaCommitError, GenError as RaGenError, GenerationRecord as RaGenerationRecord,
    SpliceError as RaSpliceError,
};
use super::slots::SlotArena;
use super::splice::PendingSplice as RaPendingSplice;
use crate::{
    control::{
        CancelError as CpCancelError, CheckpointError as CpCheckpointError,
        CommitError as CpCommitError, CpEffect, CpError, DelegationError as CpDelegationError,
        RollbackError as CpRollbackError, SpliceError as CpSpliceError,
        automaton::txn::{NoopTap, Txn},
        brand::{self, Guard as BrandGuard},
        cap::{
            CapShot, CapsMask, EndpointHandle, EndpointResource, GenericCapToken, NonceSeed,
            ResourceKind, VerifiedCap,
        },
        cluster::{CpCommand, DelegatePhase, EffectExecutor, SpliceOperands},
        types::{IncreasingGen, One},
    },
    eff::EffIndex,
    endpoint::affine::LaneGuard,
    epf::{host::HostSlots, vm::Slot},
    global::const_dsl::{ControlMarker, ControlScopeKind, HandlePlan},
    global::typestate::ScopeAtlasView,
    observe::{
        AckCounters, AssociationSnapshot, DelegAbort, DelegBegin, DelegSplice, EndpointControl,
        FenceCounters, LaneRelease, RawEvent, RollbackOk, SpliceCommit, TapEvent, TapRing, emit,
        events as tap_events, ids, policy_abort, policy_effect, policy_effect_ok, policy_trap,
    },
    runtime::consts::{DefaultLabelUniverse, LabelUniverse},
    runtime::{
        config::{Clock, Config, ConfigParts, CounterClock},
        mgmt,
    },
    transport::forward::Forward,
    transport::{
        Transport, TransportEvent as TransportEventData, TransportEventKind,
        TransportMetrics as TransportMetricsTrait,
    },
};

const ENDPOINT_TAG: u8 = 0;
const ROLE_CAPACITY: usize = 16;

struct ScopeAtlasTable {
    roles: core::cell::UnsafeCell<[Option<ScopeAtlasView<'static>>; ROLE_CAPACITY]>,
}

impl ScopeAtlasTable {
    const fn new() -> Self {
        Self {
            roles: core::cell::UnsafeCell::new([None; ROLE_CAPACITY]),
        }
    }

    fn install(&self, role: u8, view: ScopeAtlasView<'static>) {
        if (role as usize) >= ROLE_CAPACITY {
            return;
        }
        unsafe {
            (*self.roles.get())[role as usize] = Some(view);
        }
    }
}

pub use super::types::{Generation, Lane, LocalSpliceInvariant, RendezvousId, SessionId};
pub use super::error::*;
pub use super::tables::*;
pub use super::capability::*;
pub use super::association::*;
pub use super::splice::*;
pub use super::port::*;
pub use crate::control::automaton::distributed::{SpliceAck, SpliceIntent};

/// Abstraction over control-plane implementations capable of minting ports.
///
/// Note: Splice operations now go through `control::CpCommand`, keeping the
/// control-plane as the single authority for rendezvous mutations.
pub trait ControlPlane<'rv> {
    /// Transport implementation used by the control plane.
    type Transport: Transport;
    /// Epoch table bound to the control plane instance.
    type Epoch: crate::control::cap::EpochTable;

    /// Retrieve the rendezvous identifier.
    fn rendezvous_id(&self) -> RendezvousId;

    /// Release the specified lane back to the control plane.
    fn release_lane(&self, lane: Lane);
}

pub struct Rendezvous<
    'rv,
    'cfg,
    T: Transport,
    U: LabelUniverse = DefaultLabelUniverse,
    C: Clock = CounterClock,
    E: crate::control::cap::EpochTable = crate::control::cap::EpochInit,
> where
    'cfg: 'rv,
{
    brand_guard: BrandGuard<'static>,
    brand_marker: PhantomData<brand::Brand<'rv>>,
    id: RendezvousId,
    tap: TapRing<'cfg>,
    tap_registered: bool,
    slab: *mut [u8],
    slab_marker: PhantomData<&'cfg mut [u8]>,
    lane_range: Range<u32>,
    universe: U,
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
    control_plans: ControlPlanTable,
    vm_caps: VmCapsTable,
    slot_arena: SlotArena,
    host_slots: HostSlots<'cfg>,
    clock: C,
    /// Counter for generating unique RV IDs
    _next_rv_id: core::cell::Cell<u32>,
    _epoch_marker: PhantomData<E>,
    scope_atlas: ScopeAtlasTable,
}

/// Affine bundle combining slot storage and host registry access.
pub struct SlotBundle<'rv, 'cfg: 'rv> {
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
    pub fn arena(&mut self) -> &mut SlotArena {
        self.arena
    }

    /// Borrow mutable storage for the given policy slot.
    #[inline]
    pub fn storage_mut(&mut self, slot: Slot) -> &mut crate::rendezvous::SlotStorage {
        self.arena.storage_mut(slot)
    }

    /// Borrow the host machine registry associated with this rendezvous.
    #[inline]
    pub fn host_mut(&mut self) -> &mut HostSlots<'cfg> {
        self.host_slots
    }

    /// Borrow both storage and host registry for `slot` in a single call.
    #[inline]
    pub fn storage_and_host_mut(
        &mut self,
        slot: Slot,
    ) -> (&mut crate::rendezvous::SlotStorage, &mut HostSlots<'cfg>) {
        let storage = self.arena.storage_mut(slot);
        let host = &mut *self.host_slots;
        (storage, host)
    }

    /// Execute `f` with mutable access to storage and host registry for `slot`.
    #[inline]
    pub fn with_storage_and_host_mut<R, F>(&mut self, slot: Slot, f: F) -> R
    where
        F: FnOnce(&mut crate::rendezvous::SlotStorage, &mut HostSlots<'cfg>) -> R,
    {
        let storage = self.arena.storage_mut(slot);
        let host = &mut *self.host_slots;
        f(storage, host)
    }

    pub fn load_commit_with<State>(
        &mut self,
        slot: Slot,
        manager: &mut mgmt::Manager<State, { crate::rendezvous::SLOT_COUNT }>,
    ) -> Result<(), mgmt::MgmtError>
    where
        State: mgmt::ManagerState,
    {
        manager
            .load_commit(slot, self.storage_mut(slot))
            .map(|_| ())
    }

    pub fn activate_with<State>(
        &mut self,
        slot: Slot,
        manager: &mut mgmt::Manager<State, { crate::rendezvous::SLOT_COUNT }>,
    ) -> Result<mgmt::TransitionReport, mgmt::MgmtError>
    where
        State: mgmt::ManagerState,
    {
        let storage_ptr = self.arena.storage_mut(slot) as *mut crate::rendezvous::SlotStorage;
        let host_ptr = &mut *self.host_slots as *mut HostSlots<'cfg>;
        unsafe { manager.activate(slot, &mut *storage_ptr, &mut *host_ptr) }
    }

    pub fn revert_with<State>(
        &mut self,
        slot: Slot,
        manager: &mut mgmt::Manager<State, { crate::rendezvous::SLOT_COUNT }>,
    ) -> Result<mgmt::TransitionReport, mgmt::MgmtError>
    where
        State: mgmt::ManagerState,
    {
        let storage_ptr = self.arena.storage_mut(slot) as *mut crate::rendezvous::SlotStorage;
        let host_ptr = &mut *self.host_slots as *mut HostSlots<'cfg>;
        unsafe { manager.revert(slot, &mut *storage_ptr, &mut *host_ptr) }
    }
}

/// Lease guard that retains exclusive access to rendezvous slot resources.
pub struct SlotBundleLease<'rv, 'cfg: 'rv> {
    bundle: SlotBundle<'rv, 'cfg>,
}

impl<'rv, 'cfg: 'rv> SlotBundleLease<'rv, 'cfg> {
    #[inline]
    fn new(bundle: SlotBundle<'rv, 'cfg>) -> Self {
        Self { bundle }
    }

    #[inline]
    pub fn bundle_mut(&mut self) -> &mut SlotBundle<'rv, 'cfg> {
        &mut self.bundle
    }

    #[inline]
    pub fn load_commit_with<State>(
        &mut self,
        slot: Slot,
        manager: &mut mgmt::Manager<State, { crate::rendezvous::SLOT_COUNT }>,
    ) -> Result<(), mgmt::MgmtError>
    where
        State: mgmt::ManagerState,
    {
        self.bundle.load_commit_with(slot, manager)
    }

    #[inline]
    pub fn activate_with<State>(
        &mut self,
        slot: Slot,
        manager: &mut mgmt::Manager<State, { crate::rendezvous::SLOT_COUNT }>,
    ) -> Result<mgmt::TransitionReport, mgmt::MgmtError>
    where
        State: mgmt::ManagerState,
    {
        self.bundle.activate_with(slot, manager)
    }

    #[inline]
    pub fn revert_with<State>(
        &mut self,
        slot: Slot,
        manager: &mut mgmt::Manager<State, { crate::rendezvous::SLOT_COUNT }>,
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
    Splice(RaSpliceError),
    Delegation(super::error::CapError),
}

#[derive(Clone, Copy, Debug)]
struct DelegateContext {
    phase: DelegatePhase,
    token: crate::control::cap::CapToken,
}

impl<'rv, 'cfg, T: Transport, U: LabelUniverse, C: Clock, E: crate::control::cap::EpochTable>
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

    fn register_tap(&mut self) {
        if self.tap_registered {
            return;
        }
        if crate::observe::head().is_some() {
            return;
        }
        unsafe {
            let static_ref = self.tap.assume_static();
            let _ = crate::observe::install_ring(static_ref);
        }
        self.tap_registered = true;
    }

    fn unregister_tap(&mut self) {
        if !self.tap_registered {
            return;
        }
        unsafe {
            let ptr = self.tap.as_static_ptr();
            let _ = crate::observe::uninstall_ring(ptr);
        }
        self.tap_registered = false;
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

    pub(crate) fn register_control_plan(
        &self,
        lane: Lane,
        eff_index: EffIndex,
        tag: u8,
        plan: HandlePlan,
    ) -> Result<(), CpError> {
        self.control_plans
            .register(lane, eff_index, tag, plan)
            .map_err(|_| CpError::ResourceExhausted)
    }

    pub(crate) fn control_plan(
        &self,
        lane: Lane,
        eff_index: EffIndex,
        tag: u8,
    ) -> Option<HandlePlan> {
        self.control_plans.get(lane, eff_index, tag)
    }

    pub(crate) fn reset_control_plan(&self, lane: Lane) {
        self.control_plans.reset_lane(lane);
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
            RawEvent::new(self.clock.now32(), event_id, sid.raw(), arg),
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
            RawEvent::with_causal(self.clock.now32(), id, causal, arg0, arg1),
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

    #[allow(clippy::type_complexity)]
    fn apply_policy_action(
        &self,
        action: crate::epf::Action,
        sid: Option<SessionId>,
        lane: Option<Lane>,
    ) -> Result<(Option<CpCommand>, Option<(CpEffect, Option<u32>)>), CpError> {
        match action {
            crate::epf::Action::Proceed => Ok((None, None)),
            crate::epf::Action::Abort(info) => {
                self.handle_policy_abort(info, sid, lane);
                Err(CpError::PolicyAbort {
                    reason: info.reason,
                })
            }
            crate::epf::Action::Tap { id, arg0, arg1 } => {
                self.emit_policy_event(id, lane, arg0, arg1);
                Ok((None, None))
            }
            crate::epf::Action::Route { .. } => {
                // Route decisions are only meaningful for Slot::Route; ignore here.
                Ok((None, None))
            }
            crate::epf::Action::Ra(op) => {
                let decision = self.apply_policy_ra(op, sid, lane)?;

                if let Some((effect, operand)) = decision.1 {
                    self.emit_policy_event(
                        policy_effect(),
                        lane,
                        effect as u16 as u32,
                        operand.unwrap_or(0),
                    );
                }

                Ok(decision)
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

    #[allow(clippy::type_complexity)]
    fn apply_policy_ra(
        &self,
        op: crate::epf::RaOp,
        sid: Option<SessionId>,
        lane: Option<Lane>,
    ) -> Result<(Option<CpCommand>, Option<(CpEffect, Option<u32>)>), CpError> {
        let (sid_val, lane_val) = match (sid, lane) {
            (Some(s), Some(l)) => (s, l),
            _ => {
                return Err(CpError::UnsupportedEffect(op.effect() as u8));
            }
        };

        use crate::control::CpEffect;

        let cp_sid = crate::control::types::SessionId::new(sid_val.raw());
        let cp_lane = crate::control::types::LaneId::new(lane_val.raw());

        let command = match op {
            crate::epf::RaOp::Checkpoint => CpCommand::checkpoint(cp_sid, cp_lane),
            crate::epf::RaOp::Rollback { generation } => {
                if generation > u16::MAX as u32 {
                    return Err(CpError::UnsupportedEffect(CpEffect::Rollback as u8));
                }
                let generation_tag =
                    crate::control::types::Gen::new(u16::try_from(generation).unwrap());
                CpCommand::rollback(cp_sid, cp_lane, generation_tag)
            }
            crate::epf::RaOp::SpliceAbort { .. }
            | crate::epf::RaOp::SpliceBegin { .. }
            | crate::epf::RaOp::SpliceCommit { .. } => {
                return Err(CpError::UnsupportedEffect(op.effect() as u8));
            }
        };

        Ok((Some(command), Some((op.effect(), op.operand()))))
    }

    fn perform_effect(&self, envelope: CpCommand) -> Result<(), CpError> {
        match envelope.effect {
            CpEffect::SpliceBegin => {
                let sid = envelope
                    .sid
                    .ok_or(CpError::Splice(CpSpliceError::InvalidSession))?;
                let lane = envelope
                    .lane
                    .ok_or(CpError::Splice(CpSpliceError::InvalidLane))?;
                let generation_input = envelope
                    .generation
                    .ok_or(CpError::Splice(CpSpliceError::GenerationMismatch))?;
                let fences = envelope.fences;
                let sid = SessionId::new(sid.raw());
                let lane = Lane::new(lane.raw());
                let generation = Generation(generation_input.raw());
                self.begin_splice(sid, lane, fences, generation)
                    .map_err(map_splice_error)
            }
            CpEffect::SpliceAck => {
                let sid = envelope
                    .sid
                    .ok_or(CpError::Splice(CpSpliceError::InvalidSession))?;
                let Some(intent) = envelope.intent else {
                    let lane = envelope
                        .lane
                        .ok_or(CpError::Splice(CpSpliceError::InvalidLane))?;
                    let sid = SessionId::new(sid.raw());
                    let lane = Lane::new(lane.raw());
                    return match self
                        .eval_effect(CpEffect::SpliceAck, EffectContext::new(sid, lane))
                    {
                        Ok(_) => Ok(()),
                        Err(EffectError::Splice(err)) => Err(map_splice_error(err)),
                        Err(EffectError::MissingGeneration) | Err(EffectError::Rollback(_)) => {
                            Err(CpError::Splice(CpSpliceError::InvalidState))
                        }
                        Err(EffectError::Unsupported) | Err(EffectError::Delegation(_)) => {
                            Err(CpError::UnsupportedEffect(CpEffect::SpliceAck as u8))
                        }
                        Err(EffectError::Commit(_)) => {
                            Err(CpError::UnsupportedEffect(CpEffect::SpliceAck as u8))
                        }
                    };
                };
                let ack_expected = envelope
                    .ack
                    .ok_or(CpError::Splice(CpSpliceError::InvalidState))?;

                let ack_result = self
                    .process_splice_intent(&intent)
                    .map_err(map_splice_error)?;

                if ack_result != ack_expected {
                    return Err(CpError::Splice(CpSpliceError::GenerationMismatch));
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
                let sid = envelope
                    .sid
                    .ok_or(CpError::Splice(CpSpliceError::InvalidSession))?;
                let lane = envelope
                    .lane
                    .ok_or(CpError::Splice(CpSpliceError::InvalidLane))?;
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
                let delegate = envelope
                    .delegate
                    .ok_or(CpError::Delegation(CpDelegationError::InvalidToken))?;

                let header = delegate.token.header();
                let sid_raw = u32::from_be_bytes([header[0], header[1], header[2], header[3]]);
                let lane_raw = header[4] as u32;

                if let Some(sid) = envelope.sid
                    && sid.raw() != sid_raw
                {
                    return Err(CpError::Delegation(CpDelegationError::InvalidToken));
                }
                if let Some(lane) = envelope.lane
                    && lane.raw() != lane_raw
                {
                    return Err(CpError::Delegation(CpDelegationError::InvalidToken));
                }

                let sid = SessionId::new(sid_raw);
                let lane = Lane::new(lane_raw);

                let ctx = EffectContext::new(sid, lane).with_delegate(DelegateContext {
                    phase: delegate.phase,
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
                    | Err(EffectError::Commit(_)) => {
                        Err(CpError::Delegation(CpDelegationError::InvalidToken))
                    }
                }
            }
            CpEffect::Commit => {
                let sid = envelope
                    .sid
                    .ok_or(CpError::Commit(CpCommitError::SessionNotFound))?;
                let lane = envelope
                    .lane
                    .ok_or(CpError::Commit(CpCommitError::SessionNotFound))?;
                let generation_input = envelope
                    .generation
                    .ok_or(CpError::Commit(CpCommitError::GenerationMismatch))?;
                let sid = SessionId::new(sid.raw());
                let lane = Lane::new(lane.raw());
                if self.assoc.get_sid(lane) != Some(sid) {
                    return Err(CpError::Commit(CpCommitError::SessionNotFound));
                }
                self.commit_at_lane(sid, lane, Generation(generation_input.raw()))
                    .map_err(map_commit_error)
            }
            CpEffect::CancelBegin => {
                let sid = envelope
                    .sid
                    .ok_or(CpError::Cancel(CpCancelError::SessionNotFound))?;
                self.cancel_begin(SessionId::new(sid.raw()))
                    .map_err(map_cancel_error)
            }
            CpEffect::CancelAck => {
                let sid = envelope
                    .sid
                    .ok_or(CpError::Cancel(CpCancelError::SessionNotFound))?;
                let generation_input = envelope
                    .generation
                    .ok_or(CpError::Cancel(CpCancelError::GenerationMismatch))?;
                self.cancel_ack(
                    SessionId::new(sid.raw()),
                    Generation(generation_input.raw()),
                )
                .map_err(map_cancel_error)
            }
            CpEffect::Checkpoint => {
                let sid = envelope
                    .sid
                    .ok_or(CpError::Checkpoint(CpCheckpointError::SessionNotFound))?;
                self.checkpoint(SessionId::new(sid.raw()))
                    .map(|_| ())
                    .map_err(map_checkpoint_error)
            }
            CpEffect::Rollback => {
                let sid = envelope
                    .sid
                    .ok_or(CpError::Rollback(CpRollbackError::SessionNotFound))?;
                let generation_input = envelope
                    .generation
                    .ok_or(CpError::Rollback(CpRollbackError::EpochMismatch))?;
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

                let pending = RaPendingSplice::new(ctx.sid, target, in_acked, ctx.fences);

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
                    RaSpliceError::NoPending { lane: ctx.lane },
                ))?;

                let (sid, target, state, fences) = pending.into_parts();

                if sid != ctx.sid {
                    // Reinsert to preserve state before returning error.
                    let _ = self
                        .splice
                        .begin(ctx.lane, RaPendingSplice::new(sid, target, state, fences));
                    return Err(EffectError::Splice(RaSpliceError::UnknownSession {
                        sid: ctx.sid,
                    }));
                }

                self.validate_splice_generation(ctx.lane, target)
                    .map_err(EffectError::Splice)?;

                if let Err(err) = self.r#gen.check_and_update(ctx.lane, target) {
                    let _ = self
                        .splice
                        .begin(ctx.lane, RaPendingSplice::new(sid, target, state, fences));
                    let splice_err = match err {
                        RaGenError::StaleOrDuplicate(RaGenerationRecord { lane, last, new }) => {
                            RaSpliceError::StaleGeneration { lane, last, new }
                        }
                        RaGenError::Overflow { lane, last } => {
                            RaSpliceError::GenerationOverflow { lane, last }
                        }
                        RaGenError::InvalidInitial { lane, new } => {
                            RaSpliceError::InvalidInitial { lane, new }
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

                let cp_shot = crate::control::cap::CapShot::from_u8(shot_raw)
                    .ok_or(EffectError::Delegation(super::error::CapError::Mismatch))?;
                if kind_raw != ENDPOINT_TAG {
                    return Err(EffectError::Delegation(super::error::CapError::Mismatch));
                }
                let shot = match cp_shot {
                    crate::control::cap::CapShot::One => CapShot::One,
                    crate::control::cap::CapShot::Many => CapShot::Many,
                };

                if matches!(delegate.phase, DelegatePhase::Mint) {
                    emit(
                        self.tap(),
                        DelegBegin::new(
                            self.clock.now32(),
                            ctx.sid.raw(),
                            ctx.lane.as_wire() as u32,
                        ),
                    );
                }

                match delegate.phase {
                    DelegatePhase::Mint => {
                        let mut handle = EndpointHandle::new(
                            crate::control::types::SessionId::new(ctx.sid.raw()),
                            ctx.lane,
                            role,
                        );
                        self.mint_cap::<EndpointResource>(
                            ctx.sid, ctx.lane, shot, role, nonce, handle,
                        );
                        EndpointResource::zeroize(&mut handle);
                        Ok(EffectResult::None)
                    }
                    DelegatePhase::Claim => self
                        .claim_cap(&token)
                        .map(|_cap| EffectResult::None)
                        .map_err(EffectError::Delegation),
                }
            }
            CpEffect::Commit => {
                let generation = ctx.generation.ok_or(EffectError::MissingGeneration)?;
                let checkpoint = self.checkpoints.last(ctx.lane).ok_or(EffectError::Commit(
                    RaCommitError::NoCheckpoint { sid: ctx.sid },
                ))?;

                if self.checkpoints.is_consumed(ctx.lane) {
                    return Err(EffectError::Commit(RaCommitError::AlreadyCommitted {
                        sid: ctx.sid,
                    }));
                }

                if checkpoint != generation {
                    return Err(EffectError::Commit(RaCommitError::GenerationMismatch {
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
                    RollbackOk::new(
                        self.clock.now32(),
                        ctx.sid.raw(),
                        requested.0 as u32,
                    ),
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

    /// Central method to advance epoch state for a lane, updating Gen/Ack/Checkpoint tables
    /// and recording tap events. This ensures type and runtime state stay in sync.
    ///
    /// # Arguments
    ///
    /// * `tok` - Current epoch witness
    ///
    /// # Returns
    ///
    /// New epoch witness with advanced type state for the specified lane
    pub fn bump_const<const L: u8>(
        &self,
        _tok: &crate::control::cap::EndpointEpoch<'rv, E>,
    ) -> crate::control::cap::EndpointEpoch<'rv, <E as crate::control::cap::BumpAt<L>>::Out>
    where
        E: crate::control::cap::BumpAt<L>,
        <E as crate::control::cap::BumpAt<L>>::Out: crate::control::cap::EpochTable,
    {
        // Get current generation for the lane
        let lane = Lane::new(L as u32);
        let current_gen = self.r#gen.last(lane).unwrap_or(Generation(0));
        let new_gen = Generation(current_gen.0.saturating_add(1));

        // Update generation table
        if self.r#gen.check_and_update(lane, new_gen).is_err() {
            // Handle overflow or other errors if needed
            // For now, we'll continue with the saturated value
        }

        // Update ack table to maintain monotonicity
        let last_ack = self.acks.last_ack_gen(lane);
        if last_ack.map(|g| g.0 < new_gen.0).unwrap_or(true) {
            // Record this as an ack with the new generation
            self.acks.record_cancel_ack(lane, new_gen);
        }

        // Record tap event for the epoch advancement
        let ts = self.clock.now32();
        emit(
            self.tap(),
            EndpointControl::with_causal(ts, L as u16, new_gen.0 as u32, L as u32),
        );

        // Return new witness with advanced type
        crate::control::cap::EndpointEpoch::new()
    }
}

impl<'rv, 'cfg, T: Transport, U: LabelUniverse, C: Clock>
    Rendezvous<'rv, 'cfg, T, U, C, crate::control::cap::EpochInit>
where
    'cfg: 'rv,
{
    pub fn from_config(config: Config<'cfg, U, C>, transport: T) -> Self {
        static GLOBAL_RV_COUNTER: core::sync::atomic::AtomicU32 =
            core::sync::atomic::AtomicU32::new(1);
        let rv_id = GLOBAL_RV_COUNTER.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        let ConfigParts {
            tap_buf,
            slab,
            lane_range,
            universe,
            clock,
            global_tap,
        } = config.into_parts();

        brand::with_brand(|brand| {
            let guard = brand.guard();
            let guard_static =
                unsafe { mem::transmute::<BrandGuard<'_>, BrandGuard<'static>>(guard) };
            let mut rendezvous = Self {
                brand_guard: guard_static,
                brand_marker: PhantomData,
                id: RendezvousId(rv_id as u16),
                tap: TapRing::from_storage(tap_buf),
                tap_registered: false,
                slab: slab as *mut [u8],
                slab_marker: PhantomData,
                lane_range: (lane_range.start as u32)..(lane_range.end as u32),
                universe,
                transport,
                slot_arena: SlotArena::new(),
                host_slots: HostSlots::new(),
                r#gen: GenTable::new(),
                fences: FenceTable::new(),
                acks: AckTable::new(),
                assoc: AssocTable::new(),
                checkpoints: CheckpointTable::new(),
                splice: SpliceStateTable::new(),
                distributed_splice: DistributedSpliceTable::new(),
                cap_nonce: AtomicU64::new(0),
                caps: CapTable::new(),
                loops: LoopTable::new(),
                routes: RouteTable::new(),
                control_plans: ControlPlanTable::new(),
                vm_caps: VmCapsTable::new(),
                clock,
                _next_rv_id: core::cell::Cell::new(rv_id + 1000), // Reserve space for child RVs
                _epoch_marker: PhantomData,
                scope_atlas: ScopeAtlasTable::new(),
            };
            if global_tap {
                rendezvous.register_tap();
            }
            rendezvous
        })
    }

    /// Create a [`Forward`] helper for relay/splice operations.
    ///
    /// Forward never stores owner state; the rendezvous drives ownership through
    /// its `SpliceDelegate` implementation, which holds the internal brand needed
    /// to mint or retire capability tokens.
    ///
    /// # Example
    /// ```ignore
    /// rendezvous.with_forward(sid, lane, role, |mut fwd| {
    ///     block_on(fwd.relay(frame)).unwrap();
    ///     block_on(fwd.try_splice(&rendezvous_src, Generation(0))).unwrap();
    /// })
    /// ```
    pub fn with_forward<R>(
        &'rv self,
        sid: SessionId,
        lane: Lane,
        role: u8,
        f: impl FnOnce(Forward<'rv, T, crate::control::cap::EpochInit>) -> R,
    ) -> Result<R, RendezvousError> {
        let port = self.port(sid, lane, role)?;
        Ok(f(Forward::new(port, sid)))
    }
}

impl<'rv, 'cfg, T: Transport, U: LabelUniverse, C: Clock, E: crate::control::cap::EpochTable>
    Rendezvous<'rv, 'cfg, T, U, C, E>
where
    'cfg: 'rv,
{
    pub fn initialise_control_marker(&self, lane: Lane, marker: &ControlMarker) {
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

    pub(crate) fn install_scope_regions(&self, role: u8, view: ScopeAtlasView<'static>) {
        self.scope_atlas.install(role, view);
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
    ) -> Result<(), RaCommitError> {
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

    pub fn association(&self, sid: SessionId) -> Option<AssociationSnapshot> {
        let lane = self.assoc.find_lane(sid)?;
        let pending = self.splice.peek(lane);
        let pending_generation = pending.map(|p| p.target);
        let pending_fences = pending.and_then(|p| p.fences);
        let in_splice = pending.is_some();
        Some(AssociationSnapshot {
            sid,
            lane,
            active: self.assoc.is_active(lane),
            last_generation: self.r#gen.last(lane),
            last_checkpoint: self.checkpoints.last(lane),
            in_splice,
            pending_fences,
            pending_generation,
            fences: FenceCounters {
                tx: self.fences.last_tx(lane),
                rx: self.fences.last_rx(lane),
            },
            acks: AckCounters {
                last_gen: self.acks.last_ack_gen(lane),
                cancel_begin: self.acks.cancel_begin(lane),
                cancel_ack: self.acks.cancel_ack(lane),
            },
        })
    }

    pub fn is_session_registered(&self, sid: SessionId) -> bool {
        self.assoc.find_lane(sid).is_some()
    }

    pub fn release_lane(&self, lane: Lane) -> Option<SessionId> {
        let sid = self.assoc.get_sid(lane)?;
        let remaining = self.assoc.decrement(lane, sid)?;
        if remaining > 0 {
            return None;
        }
        self.reset_lane_state(lane);
        Some(sid)
    }

    pub fn force_release_lane(&self, lane: Lane) {
        let sid = self.assoc.get_sid(lane);
        self.assoc.unregister_lane(lane);
        self.reset_lane_state(lane);
        if let Some(sid) = sid {
            self.emit_lane_release(sid, lane);
        }
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

impl<'rv, 'cfg, T: Transport, U: LabelUniverse, C: Clock, E: crate::control::cap::EpochTable> Drop
    for Rendezvous<'rv, 'cfg, T, U, C, E>
{
    fn drop(&mut self) {
        self.unregister_tap();
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
/// [`SessionCluster::attach_cursor`](crate::runtime::SessionCluster::attach_cursor).
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
#[cfg_attr(not(test), doc(hidden))]
pub struct LaneLease<'cfg, T, U, C, const MAX_RV: usize>
where
    T: Transport,
    U: LabelUniverse + 'cfg,
    C: Clock + 'cfg,
{
    /// Lease-backed guard over the parent rendezvous.
    /// Uses EpochInit because LaneLease is only used to create new endpoints.
    guard: Option<LaneGuard<'cfg, T, U, C>>,
    /// Session identifier.
    sid: SessionId,
    /// Lane identifier.
    lane: Lane,
    /// Role for the port.
    role: u8,
    /// LaneKey for capability management (generated at lease creation).
    /// This is stored here to avoid needing Rendezvous access in into_endpoint().
    lane_key: crate::control::cap::LaneKey<'cfg>,
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
        lane_key: crate::control::cap::LaneKey<'cfg>,
    ) -> Self {
        Self {
            guard: Some(guard),
            sid,
            lane,
            role,
            lane_key,
        }
    }

    pub fn sid(&self) -> SessionId {
        self.sid
    }

    pub fn lane(&self) -> Lane {
        self.lane
    }

    #[allow(clippy::type_complexity)]
    pub fn into_port_guard(
        mut self,
    ) -> Result<
        (
            Port<'cfg, T, crate::control::cap::EpochInit>,
            LaneGuard<'cfg, T, U, C>,
            crate::control::cap::LaneKey<'cfg>,
        ),
        RendezvousError,
    > {
        let mut guard = self.guard.take().expect("lane lease retains guard");
        let port =
            {
                let lease_ref = guard.lease.as_mut().expect("guard retains lease");
                lease_ref.with_rendezvous(|rv| {
                    rv.acquire_port(self.sid, self.lane, self.role).map(|short| unsafe {
                    core::mem::transmute::<_, Port<'cfg, T, crate::control::cap::EpochInit>>(short)
                })
                })?
            };
        guard.detach_lease();
        Ok((port, guard, self.lane_key))
    }

    /// Convert this lease into a cursor endpoint with the given binding.
    ///
    /// The binding parameter enables flow operations to automatically invoke
    /// transport operations (e.g., STREAM writes). Use `NoBinding` when the
    /// transport layer is handled separately.
    pub fn into_cursor_endpoint<const ROLE: u8, Mint, B>(
        mut self,
        cursor: crate::global::typestate::PhaseCursor<ROLE>,
        control: crate::endpoint::control::SessionControlCtx<
            'cfg,
            T,
            U,
            C,
            crate::control::cap::EpochInit,
            MAX_RV,
        >,
        mint: Mint,
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
        RendezvousError,
    >
    where
        Mint: crate::control::cap::MintConfigMarker,
        B: crate::binding::BindingSlot,
    {
        let mut guard = self.guard.take().expect("lane lease retains guard");
        let port =
            {
                let lease_ref = guard.lease.as_mut().expect("guard retains lease");
                lease_ref.with_rendezvous(|rv| {
                    rv.acquire_port(self.sid, self.lane, self.role).map(|short| unsafe {
                    core::mem::transmute::<_, Port<'cfg, T, crate::control::cap::EpochInit>>(short)
                })
                })?
            };
        guard.detach_lease();
        let owner = crate::control::cap::Owner::new(self.lane_key);
        let epoch = crate::control::cap::EndpointEpoch::new();

        // Build ports/guards arrays with this single lane
        let lane_idx = self.lane.as_wire() as usize;
        let mut ports = [None, None, None, None, None, None, None, None];
        let mut guards = [None, None, None, None, None, None, None, None];
        ports[lane_idx] = Some(port);
        guards[lane_idx] = Some(guard);

        Ok(crate::endpoint::CursorEndpoint::from_parts(
            ports, guards, lane_idx, self.sid, owner, epoch, cursor, control, mint,
            binding,
        ))
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

impl<'rv, 'cfg, T: Transport, U: LabelUniverse, C: Clock, E: crate::control::cap::EpochTable>
    Rendezvous<'rv, 'cfg, T, U, C, E>
where
    'cfg: 'rv,
{
    /// Get the unique identifier for this Rendezvous instance.
    pub fn id(&self) -> RendezvousId {
        self.id
    }

    #[inline]
    pub(crate) fn brand(&self) -> BrandGuard<'rv> {
        unsafe { mem::transmute::<BrandGuard<'static>, BrandGuard<'rv>>(self.brand_guard) }
    }

    /// Observability ring; pushing events only needs `&self` because the ring
    /// is single-producer and internally synchronised.
    pub fn tap(&self) -> &TapRing<'cfg> {
        &self.tap
    }

    pub fn slab(&mut self) -> &mut [u8] {
        unsafe { &mut *self.slab }
    }

    pub fn universe(&self) -> &U {
        &self.universe
    }

    pub fn lane_range(&self) -> Range<u32> {
        self.lane_range.clone()
    }

    pub fn now32(&self) -> u32 {
        self.clock.now32()
    }

    /// Access the capability table for token registration.
    #[inline]
    pub(crate) fn caps(&self) -> &CapTable {
        &self.caps
    }

    /// Release a capability from the CapTable by nonce.
    #[inline]
    pub(crate) fn release_cap_by_nonce(&self, nonce: &[u8; crate::control::cap::CAP_NONCE_LEN]) {
        self.caps.release_by_nonce(nonce);
    }

    pub(crate) fn acquire_port<'a>(
        &'a self,
        sid: SessionId,
        lane: Lane,
        role: u8,
    ) -> Result<Port<'a, T, crate::control::cap::EpochInit>, RendezvousError>
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
                    crate::control::CpEffect::Open.to_tap_event_id(),
                    sid.raw(),
                    lane.0,
                ),
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
            sid,
            lane,
            role,
            self.id,
            tx,
            rx,
        ))
    }

    pub fn port<'a>(
        &'a self,
        sid: SessionId,
        lane: Lane,
        role: u8,
    ) -> Result<Port<'a, T, crate::control::cap::EpochInit>, RendezvousError>
    where
        'rv: 'a,
    {
        self.acquire_port(sid, lane, role)
    }

    /// Attach a verified capability to create a Port.
    ///
    /// This is the primary entry point for capability-based delegation. After claiming
    /// a capability token with `Rendezvous::claim_cap()`, use this method to attach the verified
    /// capability to this Rendezvous and obtain a `Port` for endpoint binding.
    ///
    /// # Parameters
    /// - `cap`: Verified capability from `Rendezvous::claim_cap()`
    ///
    /// # Returns
    /// - `Ok(Port)`: Port ready for protocol binding
    /// - `Err(RendezvousError::LaneOutOfRange)`: Lane not in this Rendezvous's range
    /// - `Err(RendezvousError::LaneBusy)`: Lane already in use
    ///
    /// # Usage
    /// ```rust,ignore
    /// let verified = broker.claim(&token)?;
    /// let port = rendezvous.attach_verified(&verified)?;
    /// let endpoint = protocol.bind::<Role<0>, _>(port, verified.sid);
    /// ```
    ///
    /// # Trust Assumption
    /// This method assumes `VerifiedCap` was obtained through `Rendezvous::claim_cap()`,
    /// which validates the capability against the CapTable. Direct construction
    /// of `VerifiedCap` bypasses security checks and should never be done.
    pub fn attach_verified(
        &'rv self,
        cap: &crate::control::cap::VerifiedCap<crate::control::cap::EndpointResource>,
    ) -> Result<Port<'rv, T, crate::control::cap::EpochInit>, RendezvousError> {
        let port = self.acquire_port(cap.sid, cap.lane, cap.role)?;
        self.vm_caps.set(cap.lane, cap.caps_mask);
        Ok(port)
    }

    /// Attach a verified capability with typestate tracking.
    ///
    /// Similar to [`attach_verified`](Self::attach_verified), but provides a lane token
    /// for epoch-based control operations.
    pub fn with_attach_verified<R>(
        &'rv self,
        cap: &crate::control::cap::VerifiedCap<crate::control::cap::EndpointResource>,
        f: impl FnOnce(
            Port<'rv, T, crate::control::cap::EpochInit>,
            crate::control::cap::LaneToken<'rv, crate::control::cap::E0>,
        ) -> R,
    ) -> Result<R, RendezvousError> {
        let port = self.attach_verified(cap)?;
        let brand = brand::Brand::from_guard(self.brand());
        let lane = cap.lane;
        Ok(brand.with_lane(lane, move |_brand_ref, lane_key| {
            let owner = crate::control::cap::Owner::new(lane_key);
            let token = crate::control::cap::LaneToken::new(owner);
            f(port, token)
        }))
    }

    /// Alias for `attach_verified()` with symmetric naming.
    ///
    /// Provided for API consistency with send/receive terminology.
    /// Functionally identical to `attach_verified()`.
    pub fn accept_verified(
        &'rv self,
        cap: &crate::control::cap::VerifiedCap<crate::control::cap::EndpointResource>,
    ) -> Result<Port<'rv, T, crate::control::cap::EpochInit>, RendezvousError> {
        self.attach_verified(cap)
    }

    /// Alias for `with_attach_verified()` with symmetric naming.
    pub fn with_accept_verified<R>(
        &'rv self,
        cap: &crate::control::cap::VerifiedCap<crate::control::cap::EndpointResource>,
        f: impl FnOnce(
            Port<'rv, T, crate::control::cap::EpochInit>,
            crate::control::cap::LaneToken<'rv, crate::control::cap::E0>,
        ) -> R,
    ) -> Result<R, RendezvousError> {
        self.with_attach_verified(cap, f)
    }

    /// Adopt a port for a session that has already been registered on this rendezvous.
    ///
    /// Unlike [`port`](Self::port), this helper assumes that the association table already
    /// records `sid` at `lane` (for example after a distributed splice commit) and therefore
    /// skips lane resets. It merely opens fresh transport handles and hands back a `Port`
    /// bound to the existing control-plane state.
    pub fn adopt_port(
        &'rv self,
        sid: SessionId,
        lane: Lane,
        role: u8,
    ) -> Result<Port<'rv, T, E>, RendezvousError> {
        if !self.lane_range.contains(&lane.0) {
            return Err(RendezvousError::LaneOutOfRange { lane });
        }

        match self.assoc.get_sid(lane) {
            Some(actual) if actual == sid => {
                if !self.assoc.is_active(lane) {
                    return Err(RendezvousError::LaneBusy { lane });
                }
            }
            _ => {
                return Err(RendezvousError::LaneBusy { lane });
            }
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
            sid,
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

    #[doc(hidden)]
    pub fn mint_cap<K: ResourceKind>(
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

        let scope = K::scope_id(&handle);
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
            scope,
        };
        self.caps
            .insert_entry(entry)
            .expect("capability table is full");

        let tap = self.tap();
        emit(
            tap,
            RawEvent::new(
                self.clock.now32(),
                crate::observe::cap_mint::<K>(),
                sid.raw(),
                ((lane.as_wire() as u32) << 16) | (dest_role as u32),
            ),
        );
    }

    #[doc(hidden)]
    #[doc(hidden)]
    pub fn claim_cap<K: crate::control::cap::ResourceKind>(
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
        if nonce == [0u8; crate::control::cap::CAP_NONCE_LEN]
            && header == [0u8; crate::control::cap::CAP_HEADER_LEN]
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
        let (claimed_role, computed_mask, exhausted, handle_bytes, scope) = self
            .caps
            .claim_by_nonce(&nonce, sid, lane, kind_tag, shot, token.caps_mask())
            .map_err(|e| match e {
                CapError::UnknownToken => CapError::UnknownToken,
                CapError::WrongSessionOrLane => CapError::WrongSessionOrLane,
                CapError::Exhausted => CapError::Exhausted,
                CapError::Mismatch => CapError::Mismatch,
            })?;

        if claimed_role != role {
            return Err(CapError::WrongSessionOrLane);
        }

        let claim_id = crate::observe::cap_claim::<K>();
        let exhaust_id = crate::observe::cap_exhaust::<K>();

        if exhausted {
            let tap = self.tap();
            emit(
                tap,
                RawEvent::new(self.clock.now32(), exhaust_id, sid.raw(), 0),
            );
        }

        let tap = self.tap();
        emit(
            tap,
            RawEvent::new(self.clock.now32(), claim_id, sid.raw(), 0),
        );

        let mut handle = K::decode_handle(handle_bytes).map_err(|_| CapError::Mismatch)?;
        let caps_mask = K::caps_mask(&handle);
        if caps_mask.bits() != computed_mask.bits() {
            K::zeroize(&mut handle);
            return Err(CapError::Mismatch);
        }

        Ok(VerifiedCap::new(
            sid,
            lane,
            claimed_role,
            shot,
            computed_mask,
            handle,
            scope,
        ))
    }

    // ============================================================================
    // Distributed splice methods
    // ============================================================================

    pub fn begin_distributed_splice(
        &self,
        sid: SessionId,
        src_lane: Lane,
        dst_rv: RendezvousId,
        dst_lane: Lane,
        fences: Option<(u32, u32)>,
    ) -> Result<SpliceIntent, RaSpliceError> {
        // Verify session exists and is on the expected lane
        if self.assoc.get_sid(src_lane) != Some(sid) {
            return Err(RaSpliceError::UnknownSession { sid });
        }

        // Get current generation and calculate next
        let old_gen = self.r#gen.last(src_lane).unwrap_or(Generation(0));
        let new_gen = Generation(old_gen.0.saturating_add(1));

        if new_gen.0 == 0 {
            return Err(RaSpliceError::GenerationOverflow {
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

    pub fn take_cached_distributed_intent(
        &self,
        sid: SessionId,
        dst_rv: RendezvousId,
    ) -> Option<SpliceIntent> {
        self.distributed_splice
            .take(sid, self.id, dst_rv)
            .map(|entry| entry.intent)
    }

    pub fn process_splice_intent(&self, intent: &SpliceIntent) -> Result<SpliceAck, RaSpliceError> {
        let dst_rv: RendezvousId = intent.dst_rv;
        let dst_lane: Lane = intent.dst_lane;
        let old_gen: Generation = intent.old_gen;
        let new_gen: Generation = intent.new_gen;

        // Validate this RV is the intended destination
        if dst_rv != self.id {
            return Err(RaSpliceError::RendezvousIdMismatch {
                expected: dst_rv,
                got: self.id,
            });
        }

        // Validate lane is in range
        if !self.lane_range.contains(&dst_lane.raw()) {
            return Err(RaSpliceError::LaneOutOfRange { lane: dst_lane });
        }

        // Check lane is available
        if self.assoc.is_active(dst_lane) {
            return Err(RaSpliceError::LaneMismatch {
                expected: dst_lane,
                provided: Lane(0), // Dummy value
            });
        }

        // Validate generation monotonicity
        let last_gen = self.r#gen.last(dst_lane).unwrap_or(Generation(0));

        // Allow old_gen to be 0 (new session) or match the last generation
        if old_gen.0 != 0 && old_gen.0 != last_gen.0 {
            return Err(RaSpliceError::StaleGeneration {
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

        let pending = RaPendingSplice::new(
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
                    RaGenError::StaleOrDuplicate(RaGenerationRecord { lane, last, new }) => {
                        RaSpliceError::StaleGeneration { lane, last, new }
                    }
                    RaGenError::Overflow { lane, last } => {
                        RaSpliceError::GenerationOverflow { lane, last }
                    }
                    RaGenError::InvalidInitial { lane, new } => {
                        RaSpliceError::InvalidInitial { lane, new }
                    }
                })?;
        } else {
            self.r#gen
                .check_and_update(dst_lane, new_gen)
                .map_err(|err| match err {
                    RaGenError::StaleOrDuplicate(RaGenerationRecord { lane, last, new }) => {
                        RaSpliceError::StaleGeneration { lane, last, new }
                    }
                    RaGenError::Overflow { lane, last } => {
                        RaSpliceError::GenerationOverflow { lane, last }
                    }
                    RaGenError::InvalidInitial { lane, new } => {
                        RaSpliceError::InvalidInitial { lane, new }
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

    pub fn commit_distributed_splice(&self, ack: &SpliceAck) -> Result<(), RaSpliceError> {
        let sid = SessionId(ack.sid);
        let src_rv: RendezvousId = ack.src_rv;
        let dst_rv: RendezvousId = ack.dst_rv;
        let new_lane: Lane = ack.new_lane;
        let new_gen: Generation = ack.new_gen;

        // Validate ack matches a pending intent
        let entry = self
            .distributed_splice
            .take(sid, src_rv, dst_rv)
            .ok_or(RaSpliceError::UnknownSession { sid })?;

        // Validate seqno matches
        if let DistributedSplicePhase::IntentSent = entry.phase
            && (ack.seq_tx != entry.intent.seq_tx || ack.seq_rx != entry.intent.seq_rx)
        {
            return Err(RaSpliceError::SeqnoMismatch {
                seq_tx: ack.seq_tx,
                seq_rx: ack.seq_rx,
            });
        }

        // Release old lane
        let src_lane: Lane = entry.intent.src_lane;
        self.force_release_lane(src_lane);

        // Register new association
        self.assoc.register(new_lane, sid);

        // Emit tap event for completion
        emit(
            self.tap(),
            SpliceCommit::new(
                self.clock.now32(),
                sid.raw(),
                new_gen.0 as u32,
            ),
        );

        Ok(())
    }

    pub fn abort_distributed_splice(&self, sid: SessionId) {
        self.splice.abort_sid(sid);
        self.distributed_splice.clear(sid);
        emit(
            self.tap(),
            DelegAbort::new(self.clock.now32(), sid.raw(), 0),
        );
    }

    pub fn clear_distributed_entry(&self, sid: SessionId) {
        self.distributed_splice.clear(sid);
    }

    // ============================================================================
    // Checkpoint / Cancel / Rollback methods
    // ============================================================================

    pub fn cancel_begin(&self, sid: SessionId) -> Result<(), CancelError> {
        let lane = self
            .assoc
            .find_lane(sid)
            .ok_or(CancelError::UnknownSession { sid })?;
        self.cancel_begin_at_lane(sid, lane);
        Ok(())
    }

    pub fn cancel_ack(&self, sid: SessionId, r#gen: Generation) -> Result<(), CancelError> {
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

    pub fn checkpoint(&self, sid: SessionId) -> Result<Generation, CheckpointError> {
        let lane = self
            .assoc
            .find_lane(sid)
            .ok_or(CheckpointError::UnknownSession { sid })?;
        Ok(self.checkpoint_at_lane(sid, lane))
    }

    pub fn commit(&self, sid: SessionId, generation: Generation) -> Result<(), CommitError> {
        let lane = self
            .assoc
            .find_lane(sid)
            .ok_or(CommitError::UnknownSession { sid })?;
        self.commit_at_lane(sid, lane, generation)
    }

    pub fn rollback(&self, sid: SessionId, epoch: Generation) -> Result<(), RollbackError> {
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
    ) -> Result<(), RaSpliceError> {
        match self.r#gen.last(lane) {
            None => {
                if new_gen.0 >= 1 {
                    Ok(())
                } else {
                    Err(RaSpliceError::InvalidInitial { lane, new: new_gen })
                }
            }
            Some(prev) if prev.0 == u16::MAX => {
                Err(RaSpliceError::GenerationOverflow { lane, last: prev })
            }
            Some(prev) if new_gen.0 > prev.0 => Ok(()),
            Some(prev) => Err(RaSpliceError::StaleGeneration {
                lane,
                last: prev,
                new: new_gen,
            }),
        }
    }
}

// ============================================================================
// SpliceDelegate trait has been DELETED.
// All splice operations now go through control::CpCommand and EffectExecutor.
// The control-plane mini-kernel architecture is responsible for rendezvous access control.

fn map_splice_error(err: RaSpliceError) -> CpError {
    match err {
        RaSpliceError::LaneOutOfRange { .. } => CpError::Splice(CpSpliceError::InvalidLane),
        RaSpliceError::LaneMismatch { .. }
        | RaSpliceError::InProgress { .. }
        | RaSpliceError::NoPending { .. }
        | RaSpliceError::SeqnoMismatch { .. } => CpError::Splice(CpSpliceError::InvalidState),
        RaSpliceError::UnknownSession { .. } => CpError::Splice(CpSpliceError::InvalidSession),
        RaSpliceError::StaleGeneration { .. }
        | RaSpliceError::GenerationOverflow { .. }
        | RaSpliceError::InvalidInitial { .. } => {
            CpError::Splice(CpSpliceError::GenerationMismatch)
        }
        RaSpliceError::RemoteRendezvousMismatch { expected, got }
        | RaSpliceError::RendezvousIdMismatch { expected, got } => CpError::RendezvousMismatch {
            expected: expected.raw(),
            actual: got.raw(),
        },
        RaSpliceError::PendingTableFull => CpError::ResourceExhausted,
    }
}

fn map_delegate_error(err: super::error::CapError) -> CpError {
    let deleg_err = match err {
        super::error::CapError::UnknownToken | super::error::CapError::WrongSessionOrLane => {
            CpDelegationError::InvalidToken
        }
        super::error::CapError::Exhausted => CpDelegationError::Exhausted,
        super::error::CapError::Mismatch => CpDelegationError::ShotMismatch,
    };
    CpError::Delegation(deleg_err)
}

fn map_cancel_error(err: super::error::CancelError) -> CpError {
    match err {
        super::error::CancelError::UnknownSession { .. } => {
            CpError::Cancel(CpCancelError::SessionNotFound)
        }
    }
}

fn map_checkpoint_error(err: super::error::CheckpointError) -> CpError {
    match err {
        super::error::CheckpointError::UnknownSession { .. } => {
            CpError::Checkpoint(CpCheckpointError::SessionNotFound)
        }
    }
}

fn map_commit_error(err: super::error::CommitError) -> CpError {
    match err {
        super::error::CommitError::UnknownSession { .. } => {
            CpError::Commit(CpCommitError::SessionNotFound)
        }
        super::error::CommitError::NoCheckpoint { .. } => {
            CpError::Commit(CpCommitError::NoCheckpoint)
        }
        super::error::CommitError::AlreadyCommitted { .. } => {
            CpError::Commit(CpCommitError::AlreadyCommitted)
        }
        super::error::CommitError::GenerationMismatch { .. } => {
            CpError::Commit(CpCommitError::GenerationMismatch)
        }
    }
}

fn map_rollback_error(err: super::error::RollbackError) -> CpError {
    match err {
        super::error::RollbackError::UnknownSession { .. } => {
            CpError::Rollback(CpRollbackError::SessionNotFound)
        }
        super::error::RollbackError::NoCheckpoint { .. } => {
            CpError::Rollback(CpRollbackError::EpochNotFound)
        }
        super::error::RollbackError::StaleCheckpoint { .. }
        | super::error::RollbackError::EpochMismatch { .. } => {
            CpError::Rollback(CpRollbackError::EpochMismatch)
        }
        super::error::RollbackError::AlreadyConsumed { .. } => {
            CpError::Rollback(CpRollbackError::AfterCommit)
        }
    }
}

// ============================================================================
// Local splice operations (used by EffectExecutor)
// ============================================================================

impl<'rv, 'cfg, T, U, C, E> Rendezvous<'rv, 'cfg, T, U, C, E>
where
    'cfg: 'rv,
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::EpochTable,
{
    /// Begin a local splice operation.
    ///
    /// This is called by EffectExecutor::run_effect() for CpEffect::SpliceBegin.
    fn begin_splice(
        &self,
        sid: SessionId,
        lane: Lane,
        fences: Option<(u32, u32)>,
        generation: Generation,
    ) -> Result<(), RaSpliceError> {
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

    /// Acknowledge a local splice operation.
    ///
    /// This is called by EffectExecutor::run_effect() for CpEffect::SpliceAck.
    fn acknowledge_splice(&self, sid: SessionId, lane: Lane) -> Result<(), RaSpliceError> {
        let ctx = EffectContext::new(sid, lane);
        match self.eval_effect(CpEffect::SpliceAck, ctx) {
            Ok(_) => Ok(()),
            Err(EffectError::Splice(err)) => Err(err),
            Err(EffectError::MissingGeneration)
            | Err(EffectError::Unsupported)
            | Err(EffectError::Delegation(_))
            | Err(EffectError::Rollback(_))
            | Err(EffectError::Commit(_)) => {
                unreachable!("splice ack failure is fully covered")
            }
        }
    }

    /// Commit a local splice operation.
    ///
    /// This is called by EffectExecutor::run_effect() for CpEffect::SpliceCommit.
    fn commit_splice(&self, sid: SessionId, lane: Lane) -> Result<(), RaSpliceError> {
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
    fn flush_transport_events(&self) -> Option<TransportEventData> {
        let tap = self.tap();
        let clock = &self.clock;
        let mut last_loss = None;
        let mut emit_event = |event: TransportEventData| {
            let (arg0, arg1) = event.encode_tap_args();
            if matches!(event.kind, TransportEventKind::Loss) {
                last_loss = Some(event);
            }
            emit(
                tap,
                tap_events::TransportEvent::new(clock.now32(), arg0, arg1),
            );
        };
        self.transport.drain_events(&mut emit_event);
        let snapshot = self.transport.metrics().snapshot();
        if let Some(payload) = snapshot.encode_tap_metrics() {
            let (arg0, arg1) = payload.primary;
            emit(
                tap,
                tap_events::TransportMetrics::new(clock.now32(), arg0, arg1),
            );
            if let Some((ext0, ext1)) = payload.extension {
                emit(
                    tap,
                    tap_events::TransportMetricsExt::new(clock.now32(), ext0, ext1),
                );
            }
        }
        last_loss
    }
}

// ============================================================================
// Capability token witness methods
// ============================================================================

impl<'rv, 'cfg, T, U, C, E> Rendezvous<'rv, 'cfg, T, U, C, E>
where
    'cfg: 'rv,
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::EpochTable,
{
    /// Checkpoint with witness-carrying token.
    ///
    /// This method performs a checkpoint operation and advances the LaneToken's
    /// typestate from E0 to Ckpt.
    pub fn checkpoint_token(
        &self,
        sid: SessionId,
        _epoch: &crate::control::cap::EndpointEpoch<'rv, E>,
        token: crate::control::cap::LaneToken<'rv, crate::control::cap::E0>,
    ) -> Result<
        (
            crate::control::cap::LaneToken<'rv, crate::control::cap::Ckpt>,
            Generation,
        ),
        CheckpointError,
    > {
        // Perform the checkpoint operation
        let generation = self.checkpoint(sid)?;
        // Advance the token typestate to Ckpt
        // SAFETY: checkpoint() succeeded, so the state transition E0 → Ckpt is valid
        let next_token = unsafe { token.transmute_step() };
        Ok((next_token, generation))
    }

    /// Commit with witness-carrying token.
    ///
    /// Advances the LaneToken typestate from Ckpt to Committed after a successful commit.
    pub fn commit_token(
        &self,
        sid: SessionId,
        _epoch: &crate::control::cap::EndpointEpoch<'rv, E>,
        token: crate::control::cap::LaneToken<'rv, crate::control::cap::Ckpt>,
        generation: Generation,
    ) -> Result<crate::control::cap::LaneToken<'rv, crate::control::cap::Committed>, CommitError>
    {
        self.commit(sid, generation)?;
        // SAFETY: commit() succeeded, so Ckpt → Committed transition is valid
        let next_token = unsafe { token.transmute_step() };
        Ok(next_token)
    }

    /// Rollback with witness-carrying token.
    ///
    /// This method performs a rollback operation and advances the LaneToken's
    /// typestate from Committed to RolledBack.
    pub fn rollback_token(
        &self,
        sid: SessionId,
        _epoch: &crate::control::cap::EndpointEpoch<'rv, E>,
        token: crate::control::cap::LaneToken<'rv, crate::control::cap::Committed>,
        at: Generation,
    ) -> Result<crate::control::cap::LaneToken<'rv, crate::control::cap::RolledBack>, RollbackError>
    {
        // Perform the rollback operation
        self.rollback(sid, at)?;
        // Advance the token typestate to RolledBack
        // SAFETY: rollback() succeeded, so the state transition Committed → RolledBack is valid
        let next_token = unsafe { token.transmute_step() };
        Ok(next_token)
    }

    /// Cancel with witness-carrying token.
    ///
    /// This method performs cancellation and advances the LaneToken's
    /// typestate from RolledBack to its terminal stop state.
    pub fn cancel_token(
        &self,
        sid: SessionId,
        _epoch: &crate::control::cap::EndpointEpoch<'rv, E>,
        token: crate::control::cap::LaneToken<'rv, crate::control::cap::RolledBack>,
    ) -> Result<
        crate::control::cap::LaneToken<
            'rv,
            crate::control::cap::Stop<crate::control::cap::RolledBack>,
        >,
        CancelError,
    > {
        // For cancel, we need both begin and ack
        self.cancel_begin(sid)?;
        // We don't have the generation here, so we use a placeholder
        // This is safe because cancel should be idempotent
        self.cancel_ack(sid, Generation(0))?;
        // Advance the token typestate to the terminal stop state
        // SAFETY: cancel operations succeeded, so the state transition is valid
        let next_token = unsafe { token.transmute_step() };
        Ok(next_token)
    }
}

impl<'rv, 'cfg, T, U, C, E> EffectExecutor for Rendezvous<'rv, 'cfg, T, U, C, E>
where
    'cfg: 'rv,
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::EpochTable,
{
    fn id(&self) -> RendezvousId {
        self.id
    }

    fn run_effect(&self, envelope: CpCommand) -> Result<(), CpError> {
        let lane_opt = envelope.lane.map(|lane| Lane::new(lane.raw()));
        let sid_opt = envelope.sid.map(|sid| SessionId::new(sid.raw()));
        let caps_mask = lane_opt
            .map(|lane| self.vm_caps.get(lane))
            .unwrap_or(CapsMask::allow_all());

        let policy_event = RawEvent::new(
            self.clock.now32(),
            envelope.effect.to_tap_event_id(),
            sid_opt.map_or(0, |sid| sid.raw()),
            lane_opt.map_or(0, |lane| lane.raw()),
        );

        let handle_data = envelope.delegate.as_ref().map(|delegate| {
            (
                delegate.token.resource_tag(),
                delegate.token.handle_bytes(),
                delegate.token.caps_mask(),
            )
        });

        let _ = self.flush_transport_events();
        let transport_metrics = self.transport.metrics().snapshot();
        let action = crate::epf::run_with(
            &self.host_slots,
            crate::epf::Slot::Rendezvous,
            &policy_event,
            caps_mask,
            sid_opt,
            lane_opt,
            move |ctx| {
                if let Some((tag, payload, mask)) = handle_data {
                    ctx.set_handle(tag, payload, mask);
                }
                ctx.set_transport_snapshot(transport_metrics);
            },
        );

        let (override_command, effect_feedback) =
            self.apply_policy_action(action, sid_opt, lane_opt)?;

        let selected_envelope = override_command.unwrap_or(envelope);

        if !caps_mask.allows(selected_envelope.effect) {
            return Err(CpError::Authorisation {
                effect: selected_envelope.effect,
            });
        }

        let result = self.perform_effect(selected_envelope);

        if let (Ok(()), Some((effect, operand))) = (&result, effect_feedback) {
            self.emit_policy_event(
                policy_effect_ok(),
                lane_opt,
                effect as u16 as u32,
                operand.unwrap_or(0),
            );
        }

        result
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

    fn abort_distributed_splice(&self, sid: SessionId) -> Result<(), CpError> {
        self.abort_distributed_splice(sid);
        Ok(())
    }
}

// ============================================================================

#[cfg(test)]
mod epf_tests {
    use super::*;
    use crate::{
        control::cap::CapsMask,
        control::cluster::{CpCommand, EffectExecutor},
        control::types::{LaneId as CpLaneId, SessionId as CpSessionId},
        observe::TapEvent,
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
            ready(Err(TransportError::Offline))
        }
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

        let envelope =
            CpCommand::checkpoint(CpSessionId::new(sid.raw()), CpLaneId::new(lane.raw()));

        let result = EffectExecutor::run_effect(&rendezvous, envelope);

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

        let envelope =
            CpCommand::checkpoint(CpSessionId::new(sid.raw()), CpLaneId::new(lane.raw()));

        let result = EffectExecutor::run_effect(&rendezvous, envelope);

        assert!(matches!(result, Err(CpError::Checkpoint(_))));
    }
}

#[cfg(disabled_test)]
mod tests {
    use super::*;
    use crate::{
        control::cap::{EndpointResource, ResourceKind},
        control::{
            cap::CapToken,
            cluster::{EffectEnvelope, EffectExecutor},
            effects::CpEffect,
            error::{
                CancelError as CpCancelError, CpError, DelegationError as CpDelegationError,
                SpliceError as CpSpliceError,
            },
            types::{Gen as CpGen, LaneId as CpLaneId, SessionId as CpSessionId},
        },
        global::const_dsl::{ControlMarker, ControlScopeKind},
        observe::{TapEvent, ids},
        rendezvous::capability::CapShot,
        runtime::{
            config::{Config, CounterClock},
            consts::{DefaultLabelUniverse, RING_EVENTS},
        },
        transport::{Transport, TransportError, wire::Payload},
    };
    use core::future::{Ready, ready};
    #[cfg(all(test, feature = "std"))]
    use proptest::test_runner::TestCaseResult;
    use static_assertions::assert_not_impl_any;

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
            = Ready<core::result::Result<(), Self::Error>>
        where
            Self: 'a;
        type Recv<'a>
            = Ready<core::result::Result<Payload<'a>, Self::Error>>
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
            ready(Err(TransportError::Offline))
        }
    }

    assert_not_impl_any!(Port<'static, DummyTransport, crate::control::cap::EpochInit>: Send, Sync);
    assert_not_impl_any!(Rendezvous<'static, 'static, DummyTransport>: Send, Sync);

    #[test]
    fn association_reports_witness_counters() {
        let mut tap_buf = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 64];
        let config = Config::new(&mut tap_buf, &mut slab);
        let rendezvous = Rendezvous::from_config(config, DummyTransport);

        let sid = SessionId(5);
        let lane = Lane(0);
        let role = 0;
        {
            let lease = rendezvous
                .lease_port(sid, lane, role)
                .expect("port registration succeeds");

            let initial = rendezvous.association(sid).expect("association exists");
            assert_eq!(initial.sid, sid);
            assert_eq!(initial.lane, lane);
            assert_eq!(initial.last_generation, None);
            assert_eq!(initial.last_checkpoint, None);
            assert!(initial.active);
            assert_eq!(initial.fences.tx, None);
            assert_eq!(initial.fences.rx, None);
            assert_eq!(initial.acks.last_gen, None);
            assert_eq!(initial.acks.cancel_begin, 0);
            assert_eq!(initial.acks.cancel_ack, 0);

            let port = lease.port();
            port.r#gen()
                .check_and_update(lane, Generation(0))
                .expect("initial zero accepted");
            port.r#gen()
                .check_and_update(lane, Generation(7))
                .expect("strictly increasing generation accepted");

            rendezvous.fences.record_tx(lane, 12);
            rendezvous.fences.record_rx(lane, 24);
            rendezvous.acks.record_cancel_begin(lane);
            rendezvous.acks.record_cancel_ack(lane, Generation(7));

            let snapshot = rendezvous.association(sid).expect("association exists");
            assert_eq!(snapshot.sid, sid);
            assert_eq!(snapshot.lane, lane);
            assert_eq!(snapshot.last_generation, Some(Generation(7)));
            assert_eq!(snapshot.last_checkpoint, None);
            assert!(snapshot.active);
            assert!(!snapshot.in_splice);
            assert_eq!(snapshot.pending_fences, None);
            assert_eq!(snapshot.pending_generation, None);
            assert_eq!(snapshot.fences.tx, Some(12));
            assert_eq!(snapshot.fences.rx, Some(24));
            assert_eq!(snapshot.acks.last_gen, Some(Generation(7)));
            assert_eq!(snapshot.acks.cancel_begin, 1);
            assert_eq!(snapshot.acks.cancel_ack, 1);
        }

        assert!(rendezvous.association(sid).is_none());
    }

    #[test]
    fn policy_tap_action_writes_event() {
        let mut tap_buf = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 64];
        let config = Config::new(&mut tap_buf, &mut slab);
        let rendezvous = Rendezvous::from_config(config, DummyTransport);

        let sid = SessionId(9);
        let lane = Lane(0);
        let head_before = rendezvous.tap().head();

        rendezvous
            .apply_policy_action(
                crate::epf::Action::Tap {
                    id: ids::SLO_BREACH,
                    arg0: 321,
                    arg1: 45,
                },
                Some(sid),
                Some(lane),
            )
            .expect("tap action succeeds");

        let tap = rendezvous.tap();
        let head_after = tap.head();
        assert_eq!(head_after, head_before + 1);
        let event = tap.as_slice()[(head_after - 1) % RING_EVENTS];
        assert_eq!(event.id, ids::SLO_BREACH);
        assert_eq!(event.arg0, 321);
        assert_eq!(event.arg1, 45);
    }

    #[test]
    fn initialise_control_marker_resets_cancel_tables() {
        let mut tap_buf = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 64];
        let config = Config::new(&mut tap_buf, &mut slab);
        let rendezvous = Rendezvous::from_config(config, DummyTransport);

        let sid = SessionId::new(42);
        let lane = Lane::new(0);

        {
            let _lease = rendezvous
                .lease_port(sid, lane, 0)
                .expect("lane registration succeeds");
            rendezvous.acks.record_cancel_begin(lane);
            rendezvous.acks.record_cancel_ack(lane, Generation(3));
        }

        let marker = ControlMarker {
            offset: 0,
            scope_kind: ControlScopeKind::Cancel,
            tap_id: 0,
        };
        rendezvous.initialise_control_marker(lane, &marker);

        assert_eq!(rendezvous.acks.cancel_begin(lane), 0);
        assert_eq!(rendezvous.acks.cancel_ack(lane), 0);
        assert!(rendezvous.acks.last_gen(lane).is_none());
    }

    #[test]
    fn association_tracks_lane_per_sid() {
        let mut tap_buf = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 64];
        let config = Config::new(&mut tap_buf, &mut slab);
        let rendezvous = Rendezvous::from_config(config, DummyTransport);

        let sid_a = SessionId(10);
        let sid_b = SessionId(11);
        let lane_a = Lane(2);
        let lane_b = Lane(3);

        {
            let _lease_a = rendezvous
                .lease_port(sid_a, lane_a, 0)
                .expect("sid_a registers lane");
            let _lease_b = rendezvous
                .lease_port(sid_b, lane_b, 0)
                .expect("sid_b registers lane");

            let snap_a = rendezvous.association(sid_a).expect("sid_a present");
            let snap_b = rendezvous.association(sid_b).expect("sid_b present");

            assert_eq!(snap_a.lane, lane_a);
            assert_eq!(snap_b.lane, lane_b);
            assert_eq!(snap_a.last_checkpoint, None);
            assert_eq!(snap_b.last_checkpoint, None);
            assert!(snap_a.active);
            assert!(snap_b.active);
        }
    }

    #[test]
    fn lane_reuse_resets_state() {
        let mut tap_buf = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 64];
        let config = Config::new(&mut tap_buf, &mut slab);
        let rendezvous = Rendezvous::from_config(config, DummyTransport);

        let lane = Lane(1);
        let sid_old = SessionId(42);
        let sid_new = SessionId(43);

        {
            let lease = rendezvous
                .lease_port(sid_old, lane, 0)
                .expect("initial port registration");
            let port = lease.port();
            port.r#gen()
                .check_and_update(lane, Generation(0))
                .expect("initial zero accepted");
            port.r#gen()
                .check_and_update(lane, Generation(10))
                .expect("first update accepted");
            rendezvous.fences.record_tx(lane, 100);
            rendezvous.fences.record_rx(lane, 200);
            rendezvous.acks.record_cancel_begin(lane);
            rendezvous.acks.record_cancel_ack(lane, Generation(10));
        }

        assert!(rendezvous.association(sid_old).is_none());

        {
            let lease = rendezvous
                .lease_port(sid_new, lane, 0)
                .expect("re-registration");
            let port_new = lease.port();
            let snapshot = rendezvous.association(sid_new).expect("sid_new present");
            assert_eq!(snapshot.sid, sid_new);
            assert_eq!(snapshot.lane, lane);
            assert!(snapshot.active);
            assert_eq!(snapshot.last_generation, None);
            assert_eq!(snapshot.last_checkpoint, None);
            assert!(!snapshot.in_splice);
            assert_eq!(snapshot.pending_fences, None);
            assert_eq!(snapshot.pending_generation, None);
            assert_eq!(snapshot.fences.tx, None);
            assert_eq!(snapshot.fences.rx, None);
            assert_eq!(snapshot.acks.last_gen, None);
            assert_eq!(snapshot.acks.cancel_begin, 0);
            assert_eq!(snapshot.acks.cancel_ack, 0);

            // ensure new updates proceed normally
            port_new
                .r#gen()
                .check_and_update(lane, Generation(0))
                .expect("initial zero accepted");
            port_new
                .r#gen()
                .check_and_update(lane, Generation(5))
                .expect("fresh lane accepts update");
        }
    }

    #[test]
    fn association_reflects_pending_splice() {
        let mut tap_buf = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 64];
        let config = Config::new(&mut tap_buf, &mut slab);
        let rendezvous = Rendezvous::from_config(config, DummyTransport);

        let sid = SessionId(55);
        let lane = Lane::new(2);
        let role = 1;

        let _lease = rendezvous
            .lease_port(sid, lane, role)
            .expect("port registration");

        // Begin splice using the new CpCommand-based API
        rendezvous
            .begin_splice(sid, lane, Some((11, 22)), Generation(1))
            .expect("begin splice");

        // Check that association reflects the pending splice
        let snapshot = rendezvous.association(sid).expect("snapshot present");
        assert!(snapshot.in_splice);
        assert_eq!(snapshot.pending_generation, Some(Generation(1)));
        assert_eq!(snapshot.pending_fences, Some((11, 22)));

        // Abort by clearing the pending splice (take removes it)
        rendezvous.splice.take(lane);

        let snapshot = rendezvous.association(sid).expect("snapshot present");
        assert!(!snapshot.in_splice);
        assert_eq!(snapshot.pending_generation, None);
        assert_eq!(snapshot.pending_fences, None);
    }

    #[test]
    fn association_returns_none_for_unknown_sid() {
        let mut tap_buf = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 64];
        let config = Config::new(&mut tap_buf, &mut slab);
        let rendezvous = Rendezvous::from_config(config, DummyTransport);

        assert!(rendezvous.association(SessionId(999)).is_none());
    }

    #[test]
    fn generation_overflow_after_max() {
        let mut tap_buf = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 64];
        let config = Config::new(&mut tap_buf, &mut slab);
        let rendezvous = Rendezvous::from_config(config, DummyTransport);

        let lane = Lane(0);
        let sid = SessionId(55);
        {
            let lease = rendezvous
                .lease_port(sid, lane, 0)
                .expect("port registration");
            let port = lease.port();

            port.r#gen()
                .check_and_update(lane, Generation(0))
                .expect("initial zero accepted");
            port.r#gen()
                .check_and_update(lane, Generation(u16::MAX))
                .expect("max accepted");

            let err = port
                .r#gen()
                .check_and_update(lane, Generation(u16::MAX))
                .unwrap_err();
            assert!(matches!(
                err,
                RaGenError::Overflow {
                    lane: l,
                    last: Generation(val)
                } if l == lane && val == u16::MAX
            ));
        }

        assert!(rendezvous.association(sid).is_none());

        {
            let lease = rendezvous
                .lease_port(SessionId(56), lane, 0)
                .expect("port re-registration after release");
            let port = lease.port();
            port.r#gen()
                .check_and_update(lane, Generation(0))
                .expect("lane reset accepts zero");
        }
    }

    #[test]
    fn capability_mint_and_claim_one_shot_records_events() {
        let mut tap_buf = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 64];
        let config = Config::new(&mut tap_buf, &mut slab);
        let rendezvous = Rendezvous::from_config(config, DummyTransport);

        let sid = SessionId(7);
        let lane = Lane(1);
        let role = 3;
        let _lease = rendezvous
            .lease_port(sid, lane, role)
            .expect("port registration succeeds");

        let dest_role = 5;
        let mut nonce = [0u8; crate::control::cap::CAP_NONCE_LEN];
        nonce[0..4].copy_from_slice(&sid.raw().to_be_bytes());
        nonce[4..8].copy_from_slice(&lane.raw().to_be_bytes());
        let mut handle = EndpointHandle::new(
            crate::control::types::SessionId::new(sid.raw()),
            lane,
            dest_role,
        );
        let encoded_handle = EndpointResource::encode_handle(&handle);
        let caps_mask = EndpointResource::caps_mask(&handle);
        rendezvous.mint_cap::<EndpointResource>(sid, lane, CapShot::One, dest_role, nonce, handle);

        let mut header = [0u8; crate::control::cap::CAP_HEADER_LEN];
        header[0..4].copy_from_slice(&sid.raw().to_be_bytes());
        header[4] = lane.as_wire();
        header[5] = dest_role;
        header[6] = EndpointResource::TAG;
        header[7] = crate::control::cap::CapShot::One.as_u8();
        header[8..10].copy_from_slice(&caps_mask.bits().to_be_bytes());
        header[crate::control::cap::CAP_FIXED_HEADER_LEN
            ..crate::control::cap::CAP_FIXED_HEADER_LEN + crate::control::cap::CAP_HANDLE_LEN]
            .copy_from_slice(&encoded_handle);
        EndpointResource::zeroize(&mut handle);

        let token = crate::control::cap::CapToken::from_parts(
            nonce,
            header,
            [0u8; crate::control::cap::CAP_TAG_LEN],
        );

        let token_sid = token.sid();
        let token_lane = token.lane();
        let token_role = token.role();
        let token_mask = token.caps_mask();
        assert_eq!(token_sid.raw(), sid.raw());
        assert_eq!(token_lane.raw(), lane.raw());
        assert_eq!(token_role, dest_role);
        assert_eq!(token_mask.bits(), caps_mask.bits());

        let verified = rendezvous.claim_cap(&token).expect("first claim succeeds");
        assert_eq!(verified.sid, sid);
        assert_eq!(verified.lane, lane);
        assert_eq!(verified.role, dest_role);
        assert_eq!(verified.shot, CapShot::One);
        assert_eq!(verified.caps_mask.bits(), caps_mask.bits());

        let err = rendezvous
            .claim_cap(&token)
            .expect_err("one-shot exhausted");
        assert_eq!(err, CapError::Exhausted);

        let events = rendezvous.tap().as_slice();
        let minted = events
            .iter()
            .filter(|e| e.id == crate::observe::cap_mint::<EndpointResource>())
            .count();
        let claimed = events
            .iter()
            .filter(|e| e.id == crate::observe::cap_claim::<EndpointResource>())
            .count();
        let exhausted = events
            .iter()
            .filter(|e| e.id == crate::observe::cap_exhaust::<EndpointResource>())
            .count();

        assert_eq!(minted, 1, "expected a single CAP_MINT event");
        assert_eq!(claimed, 1, "expected a single CAP_CLAIM event");
        assert_eq!(exhausted, 1, "expected a single CAP_EXHAUST event");
    }

    #[test]
    fn capability_many_allows_repeated_claims_without_exhaustion() {
        let mut tap_buf = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 64];
        let config = Config::new(&mut tap_buf, &mut slab);
        let rendezvous = Rendezvous::from_config(config, DummyTransport);

        let sid = SessionId(8);
        let lane = Lane(2);
        let role = 4;
        let _lease = rendezvous
            .lease_port(sid, lane, role)
            .expect("port registration succeeds");

        let dest_role = 4;
        let mut nonce = [0u8; crate::control::cap::CAP_NONCE_LEN];
        nonce[0..4].copy_from_slice(&sid.raw().to_be_bytes());
        nonce[4..8].copy_from_slice(&lane.raw().to_be_bytes());
        let mut handle = EndpointHandle::new(
            crate::control::types::SessionId::new(sid.raw()),
            lane,
            dest_role,
        );
        let encoded_handle = EndpointResource::encode_handle(&handle);
        let caps_mask = EndpointResource::caps_mask(&handle);
        rendezvous.mint_cap::<EndpointResource>(sid, lane, CapShot::Many, dest_role, nonce, handle);

        let mut header = [0u8; crate::control::cap::CAP_HEADER_LEN];
        header[0..4].copy_from_slice(&sid.raw().to_be_bytes());
        header[4] = lane.as_wire();
        header[5] = dest_role;
        header[6] = EndpointResource::TAG;
        header[7] = crate::control::cap::CapShot::Many.as_u8();
        header[8..10].copy_from_slice(&caps_mask.bits().to_be_bytes());
        header[crate::control::cap::CAP_FIXED_HEADER_LEN
            ..crate::control::cap::CAP_FIXED_HEADER_LEN + crate::control::cap::CAP_HANDLE_LEN]
            .copy_from_slice(&encoded_handle);
        EndpointResource::zeroize(&mut handle);

        let token = crate::control::cap::CapToken::from_parts(
            nonce,
            header,
            [0u8; crate::control::cap::CAP_TAG_LEN],
        );

        for _ in 0..3 {
            let verified = rendezvous
                .claim_cap(&token)
                .expect("many-shot claim succeeds");
            assert_eq!(verified.sid, sid);
            assert_eq!(verified.lane, lane);
            assert_eq!(verified.role, dest_role);
            assert_eq!(verified.shot, CapShot::Many);
            assert_eq!(verified.caps_mask.bits(), caps_mask.bits());
        }

        let events = rendezvous.tap().as_slice();
        let minted = events
            .iter()
            .filter(|e| e.id == crate::observe::cap_mint::<EndpointResource>())
            .count();
        let claimed = events
            .iter()
            .filter(|e| e.id == crate::observe::cap_claim::<EndpointResource>())
            .count();
        let exhausted = events
            .iter()
            .filter(|e| e.id == crate::observe::cap_exhaust::<EndpointResource>())
            .count();

        assert_eq!(minted, 1, "expected a single CAP_MINT event");
        assert_eq!(claimed, 3, "expected three CAP_CLAIM events");
        assert_eq!(
            exhausted, 0,
            "many-shot capabilities must not emit CAP_EXHAUST"
        );
    }

    #[test]
    fn capability_claim_rejects_wrong_lane_and_purges_on_release() {
        let mut tap_buf = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 64];
        let config = Config::new(&mut tap_buf, &mut slab);
        let rendezvous = Rendezvous::from_config(config, DummyTransport);

        let sid = SessionId(42);
        let lane = Lane(3);
        let role = 1;
        let lease = rendezvous
            .lease_port(sid, lane, role)
            .expect("port registration succeeds");

        let dest_role = 7;
        let mut nonce = [0u8; crate::control::cap::CAP_NONCE_LEN];
        nonce[0..4].copy_from_slice(&sid.raw().to_be_bytes());
        nonce[4..8].copy_from_slice(&lane.raw().to_be_bytes());
        let mut handle = EndpointHandle::new(
            crate::control::types::SessionId::new(sid.raw()),
            lane,
            dest_role,
        );
        let encoded_handle = EndpointResource::encode_handle(&handle);
        let caps_mask = EndpointResource::caps_mask(&handle);
        rendezvous.mint_cap::<EndpointResource>(sid, lane, CapShot::One, dest_role, nonce, handle);

        let mut header = [0u8; crate::control::cap::CAP_HEADER_LEN];
        header[0..4].copy_from_slice(&sid.raw().to_be_bytes());
        header[4] = lane.as_wire();
        header[5] = dest_role;
        header[6] = EndpointResource::TAG;
        header[7] = crate::control::cap::CapShot::One.as_u8();
        header[8..10].copy_from_slice(&caps_mask.bits().to_be_bytes());
        header[crate::control::cap::CAP_FIXED_HEADER_LEN
            ..crate::control::cap::CAP_FIXED_HEADER_LEN + crate::control::cap::CAP_HANDLE_LEN]
            .copy_from_slice(&encoded_handle);
        EndpointResource::zeroize(&mut handle);

        let token = crate::control::cap::CapToken::from_parts(
            nonce,
            header,
            [0u8; crate::control::cap::CAP_TAG_LEN],
        );

        // Forge lane by modifying header byte 4
        let mut forged = token;
        forged.bytes[crate::control::cap::CAP_NONCE_LEN + 4] ^= 1; // header[4] = lane
        let err = rendezvous
            .claim_cap(&forged)
            .expect_err("forged lane should be rejected");
        assert_eq!(err, CapError::WrongSessionOrLane);

        // Forge role by modifying header byte 5
        let mut forged_role = token;
        forged_role.bytes[crate::control::cap::CAP_NONCE_LEN + 5] ^= 1; // header[5] = role
        let err = rendezvous
            .claim_cap(&forged_role)
            .expect_err("forged role should be rejected");
        assert_eq!(err, CapError::WrongSessionOrLane);

        drop(lease);
        let err = rendezvous
            .claim_cap(&token)
            .expect_err("released lane should reject claim");
        assert_eq!(err, CapError::WrongSessionOrLane);
    }

    #[test]
    fn delegate_effect_mint_and_claim_records_events() {
        let mut tap_buf = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 128];
        let config = Config::<DefaultLabelUniverse, CounterClock>::new(&mut tap_buf, &mut slab)
            .with_lane_range(0..8);
        let rendezvous = Rendezvous::from_config(config, DummyTransport);

        let sid = SessionId(314);
        let lane = Lane::new(4);
        let role = 7;
        let _lease = rendezvous
            .lease_port(sid, lane, role)
            .expect("session registers lane");

        let mut handle =
            EndpointHandle::new(crate::control::types::SessionId::new(sid.raw()), lane, role);
        let handle_bytes = EndpointResource::encode_handle(&handle);
        let mask = EndpointResource::caps_mask(&handle);
        let mut header = [0u8; crate::control::cap::CAP_HEADER_LEN];
        header[0..4].copy_from_slice(&sid.raw().to_be_bytes());
        header[4] = lane.as_wire();
        header[5] = role;
        header[6] = ENDPOINT_TAG;
        header[7] = crate::control::cap::CapShot::One.as_u8();
        header[8..10].copy_from_slice(&mask.bits().to_be_bytes());
        header[crate::control::cap::CAP_FIXED_HEADER_LEN
            ..crate::control::cap::CAP_FIXED_HEADER_LEN + crate::control::cap::CAP_HANDLE_LEN]
            .copy_from_slice(&handle_bytes);
        EndpointResource::zeroize(&mut handle);
        let nonce = [0xA5; crate::control::cap::CAP_NONCE_LEN];
        let tag = [0u8; crate::control::cap::CAP_TAG_LEN];
        let token = CapToken::from_parts(nonce, header, tag);

        rendezvous
            .run_effect(CpCommand::delegate_mint(token))
            .expect("delegate mint succeeds");

        let tap_events = rendezvous.tap().as_slice();
        assert!(
            tap_events
                .iter()
                .any(|e| e.id == crate::observe::cap_mint::<EndpointResource>())
        );
        assert!(tap_events.iter().any(|e| e.id == ids::DELEG_BEGIN));

        rendezvous
            .run_effect(CpCommand::delegate_claim(token))
            .expect("delegate claim succeeds");

        let tap_events = rendezvous.tap().as_slice();
        assert!(
            tap_events
                .iter()
                .any(|e| e.id == crate::observe::cap_claim::<EndpointResource>())
        );
        assert!(
            tap_events
                .iter()
                .any(|e| e.id == crate::observe::cap_exhaust::<EndpointResource>())
        );

        let err = rendezvous
            .run_effect(CpCommand::delegate_claim(token))
            .expect_err("second claim must report exhaustion");
        assert!(matches!(
            err,
            CpError::Delegation(CpDelegationError::Exhausted)
        ));
    }

    #[test]
    fn splice_begin_without_sid_is_rejected() {
        let mut tap_buf = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 128];
        let config = Config::<DefaultLabelUniverse, CounterClock>::new(&mut tap_buf, &mut slab)
            .with_lane_range(0..4);
        let rendezvous = Rendezvous::from_config(config, DummyTransport);

        let sid = SessionId(1);
        let lane = Lane::new(0);
        let _lease = rendezvous
            .lease_port(sid, lane, 0)
            .expect("lane registration succeeds");

        let envelope = CpCommand::new(CpEffect::SpliceBegin).with_lane(CpLaneId::new(lane.raw()));

        let result = EffectExecutor::run_effect(&rendezvous, envelope);
        assert!(matches!(
            result,
            Err(CpError::Splice(CpSpliceError::InvalidSession))
        ));
    }

    #[test]
    fn splice_begin_without_lane_is_rejected() {
        let mut tap_buf = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 128];
        let config = Config::<DefaultLabelUniverse, CounterClock>::new(&mut tap_buf, &mut slab)
            .with_lane_range(0..4);
        let rendezvous = Rendezvous::from_config(config, DummyTransport);

        let sid = SessionId(2);
        let lane = Lane::new(1);
        let _lease = rendezvous
            .lease_port(sid, lane, 0)
            .expect("lane registration succeeds");

        let envelope = CpCommand::new(CpEffect::SpliceBegin).with_sid(CpSessionId::new(sid.raw()));

        let result = EffectExecutor::run_effect(&rendezvous, envelope);
        assert!(matches!(
            result,
            Err(CpError::Splice(CpSpliceError::InvalidLane))
        ));
    }

    #[test]
    fn cancel_ack_missing_generation_is_rejected() {
        let mut tap_buf = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 128];
        let config = Config::<DefaultLabelUniverse, CounterClock>::new(&mut tap_buf, &mut slab)
            .with_lane_range(0..4);
        let rendezvous = Rendezvous::from_config(config, DummyTransport);

        let sid = SessionId(3);
        let lane = Lane::new(2);
        let _lease = rendezvous
            .lease_port(sid, lane, 0)
            .expect("lane registration succeeds");

        // CancelBegin succeeds with sid registered
        EffectExecutor::run_effect(
            &rendezvous,
            CpCommand::new(CpEffect::CancelBegin)
                .with_sid(CpSessionId::new(sid.raw()))
                .with_lane(CpLaneId::new(lane.raw())),
        )
        .expect("cancel begin succeeds");

        // Missing generation in CancelAck should be rejected
        let envelope = CpCommand::new(CpEffect::CancelAck)
            .with_sid(CpSessionId::new(sid.raw()))
            .with_lane(CpLaneId::new(lane.raw()));
        let result = EffectExecutor::run_effect(&rendezvous, envelope);
        assert!(matches!(
            result,
            Err(CpError::Cancel(CpCancelError::GenerationMismatch))
        ));
    }

    #[test]
    fn splice_begin_records_fences() {
        let mut tap_buf = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 128];
        let config = Config::<DefaultLabelUniverse, CounterClock>::new(&mut tap_buf, &mut slab)
            .with_lane_range(0..4);
        let rendezvous = Rendezvous::from_config(config, DummyTransport);

        let sid = SessionId(11);
        let lane = Lane::new(0);
        let _lease = rendezvous.lease_port(sid, lane, 0).expect("lane registers");

        let envelope = CpCommand::splice_local_begin(
            CpSessionId::new(sid.raw()),
            CpLaneId::new(lane.raw()),
            CpGen::new(1),
            Some((11, 22)),
        );

        EffectExecutor::run_effect(&rendezvous, envelope).expect("splice begin succeeds");

        let pending = rendezvous
            .splice
            .peek(lane)
            .expect("pending splice present");
        assert_eq!(pending.fences, Some((11, 22)));
        assert_eq!(rendezvous.fences.last_tx(lane), Some(11));
        assert_eq!(rendezvous.fences.last_rx(lane), Some(22));
    }

    #[test]
    fn commit_without_checkpoint_fails() {
        let mut tap_buf = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 64];
        let config = Config::<DefaultLabelUniverse, CounterClock>::new(&mut tap_buf, &mut slab)
            .with_lane_range(0..4);
        let rendezvous = Rendezvous::from_config(config, DummyTransport);

        let sid = SessionId(41);
        let lane = Lane::new(0);
        let lease = rendezvous.lease_port(sid, lane, 0).expect("lane registers");

        let err = rendezvous
            .commit(sid, Generation(0))
            .expect_err("commit must fail without checkpoint");
        assert!(matches!(err, CommitError::NoCheckpoint { sid: s } if s == sid));

        drop(lease);
    }

    #[test]
    fn commit_marks_checkpoint_consumed() {
        let mut tap_buf = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 64];
        let config = Config::<DefaultLabelUniverse, CounterClock>::new(&mut tap_buf, &mut slab)
            .with_lane_range(0..4);
        let rendezvous = Rendezvous::from_config(config, DummyTransport);

        let sid = SessionId(42);
        let lane = Lane::new(1);
        let lease = rendezvous.lease_port(sid, lane, 0).expect("lane registers");

        let generation = rendezvous.checkpoint(sid).expect("checkpoint succeeds");
        rendezvous.commit(sid, generation).expect("commit succeeds");

        assert!(rendezvous.checkpoints.is_consumed(lane));

        let rollback_err = rendezvous.rollback(sid, generation);
        assert!(matches!(rollback_err, Err(RollbackError::AlreadyConsumed { sid: s }) if s == sid));

        let tap_events = rendezvous.tap().as_slice();
        assert!(
            tap_events
                .iter()
                .any(|event| event.id == CpEffect::Commit.to_tap_event_id())
        );

        drop(lease);
    }

    #[test]
    fn invalid_initial_generation_rejected() {
        let mut tap_buf = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 64];
        let config = Config::new(&mut tap_buf, &mut slab);
        let rendezvous = Rendezvous::from_config(config, DummyTransport);

        let lane = Lane(0);
        let sid = SessionId(90);
        let lease = rendezvous
            .lease_port(sid, lane, 0)
            .expect("port registration");
        let port = lease.port();

        let err = port
            .r#gen()
            .check_and_update(lane, Generation(5))
            .unwrap_err();
        assert!(matches!(
            err,
            RaGenError::InvalidInitial {
                lane: l,
                new: Generation(val)
            } if l == lane && val == 5
        ));
    }

    #[test]
    fn cancel_begin_ack_updates_snapshot_and_tap() {
        let mut tap_buf = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 64];
        let config = Config::new(&mut tap_buf, &mut slab);
        let rendezvous = Rendezvous::from_config(config, DummyTransport);

        let sid = SessionId(77);
        let lane = Lane(0);
        let _lease = rendezvous
            .lease_port(sid, lane, 0)
            .expect("port registration succeeded");

        let begin_head = rendezvous.tap().head();
        rendezvous.cancel_begin(sid).expect("cancel begin");
        let begin_idx = begin_head % RING_EVENTS;

        let r#gen = Generation(9);
        rendezvous.cancel_ack(sid, r#gen).expect("cancel ack");
        let ack_idx = (begin_head + 1) % RING_EVENTS;

        let snapshot = rendezvous.association(sid).expect("association present");
        assert_eq!(snapshot.acks.cancel_begin, 1);
        assert_eq!(snapshot.acks.cancel_ack, 1);
        assert_eq!(snapshot.acks.last_gen, Some(r#gen));

        let taps = rendezvous.tap().as_slice();
        assert_eq!(taps[begin_idx].id, ids::CANCEL_BEGIN);
        assert_eq!(taps[begin_idx].arg0, sid.0);
        assert_eq!(taps[begin_idx].arg1, lane.0 as u32);
        assert_eq!(taps[ack_idx].id, ids::CANCEL_ACK);
        assert_eq!(taps[ack_idx].arg0, sid.0);
        assert_eq!(taps[ack_idx].arg1, r#gen.0 as u32);
    }

    #[test]
    fn checkpoint_records_epoch_and_updates_snapshot() {
        let mut tap_buf = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 64];
        let config = Config::new(&mut tap_buf, &mut slab);
        let rendezvous = Rendezvous::from_config(config, DummyTransport);

        let sid = SessionId(11);
        let lane = Lane(0);
        let lease = rendezvous
            .lease_port(sid, lane, 0)
            .expect("port registration succeeded");
        let port = lease.port();
        port.r#gen()
            .check_and_update(lane, Generation(0))
            .expect("initial generation set");
        port.r#gen()
            .check_and_update(lane, Generation(5))
            .expect("generation advanced");

        let head_before = rendezvous.tap().head();
        let epoch = rendezvous.checkpoint(sid).expect("checkpoint succeeds");
        assert_eq!(epoch, Generation(5));
        assert_eq!(rendezvous.tap().head(), head_before + 1);

        let idx = head_before % RING_EVENTS;
        let tap = rendezvous.tap().as_slice()[idx];
        assert_eq!(tap.id, ids::CHECKPOINT_REQ);
        assert_eq!(tap.arg0, sid.0);
        assert_eq!(tap.arg1, 5);

        let snapshot = rendezvous.association(sid).expect("association present");
        assert_eq!(snapshot.last_checkpoint, Some(Generation(5)));
    }

    #[test]
    fn checkpoint_unknown_session_does_not_touch_tap() {
        let mut tap_buf = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 64];
        let config = Config::new(&mut tap_buf, &mut slab);
        let rendezvous = Rendezvous::from_config(config, DummyTransport);

        let head_before = rendezvous.tap().head();
        let err = rendezvous.checkpoint(SessionId(99));
        assert!(matches!(
            err,
            Err(CheckpointError::UnknownSession { sid }) if sid == SessionId(99)
        ));
        assert_eq!(rendezvous.tap().head(), head_before);
    }

    #[test]
    fn rollback_succeeds_when_epoch_matches_checkpoint_and_generation() {
        let mut tap_buf = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 64];
        let config = Config::new(&mut tap_buf, &mut slab);
        let rendezvous = Rendezvous::from_config(config, DummyTransport);

        let sid = SessionId(12);
        let lane = Lane(0);
        let lease = rendezvous
            .lease_port(sid, lane, 0)
            .expect("port registration succeeded");
        let port = lease.port();
        port.r#gen()
            .check_and_update(lane, Generation(0))
            .expect("initial generation set");
        port.r#gen()
            .check_and_update(lane, Generation(3))
            .expect("generation advanced");

        let epoch = rendezvous.checkpoint(sid).expect("checkpoint succeeds");
        assert_eq!(epoch, Generation(3));

        let head_before = rendezvous.tap().head();
        rendezvous.rollback(sid, epoch).expect("rollback succeeds");
        assert_eq!(rendezvous.tap().head(), head_before + 2);

        let start = head_before % RING_EVENTS;
        let tap = rendezvous.tap().as_slice();
        assert_eq!(tap[start].id, ids::ROLLBACK_REQ);
        assert_eq!(tap[start].arg0, sid.0);
        assert_eq!(tap[start].arg1, epoch.0 as u32);
        assert_eq!(tap[(start + 1) % RING_EVENTS].id, ids::ROLLBACK_OK);
        assert_eq!(tap[(start + 1) % RING_EVENTS].arg0, sid.0);
        assert_eq!(tap[(start + 1) % RING_EVENTS].arg1, epoch.0 as u32);
    }

    #[test]
    fn rollback_rejects_without_checkpoint_and_preserves_tap() {
        let mut tap_buf = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 64];
        let config = Config::new(&mut tap_buf, &mut slab);
        let rendezvous = Rendezvous::from_config(config, DummyTransport);

        let sid = SessionId(13);
        let lane = Lane(0);
        let lease = rendezvous
            .lease_port(sid, lane, 0)
            .expect("port registration succeeded");
        let port = lease.port();
        port.r#gen()
            .check_and_update(lane, Generation(0))
            .expect("initial generation set");

        let head_before = rendezvous.tap().head();
        let err = rendezvous.rollback(sid, Generation(0));
        assert!(
            matches!(err, Err(RollbackError::NoCheckpoint { sid: _ })),
            "expected NoCheckpoint, got {:?}",
            err
        );
        assert_eq!(rendezvous.tap().head(), head_before);
    }

    #[test]
    fn rollback_rejects_epoch_mismatch_and_keeps_tap_clean() {
        let mut tap_buf = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 64];
        let config = Config::new(&mut tap_buf, &mut slab);
        let rendezvous = Rendezvous::from_config(config, DummyTransport);

        let sid = SessionId(14);
        let lane = Lane(0);
        {
            let lease = rendezvous
                .lease_port(sid, lane, 0)
                .expect("port registration succeeded");
            let port = lease.port();
            port.r#gen()
                .check_and_update(lane, Generation(0))
                .expect("initial generation set");
            port.r#gen()
                .check_and_update(lane, Generation(4))
                .expect("generation advanced");

            let epoch = rendezvous.checkpoint(sid).expect("checkpoint succeeds");
            assert_eq!(epoch, Generation(4));

            let head_before = rendezvous.tap().head();
            let err = rendezvous.rollback(sid, Generation(3));
            assert!(
                matches!(
                    err,
                    Err(RollbackError::StaleCheckpoint {
                        sid: _,
                        requested: Generation(3),
                        current: Generation(4)
                    })
                ),
                "expected StaleCheckpoint, got {:?}",
                err
            );
            assert_eq!(rendezvous.tap().head(), head_before);

            // The failed rollback must not emit request/ok events; only the checkpoint remains.
            let idx = (head_before - 1) % RING_EVENTS;
            let tap = rendezvous.tap().as_slice()[idx];
            assert_eq!(tap.id, ids::CHECKPOINT_REQ);
        }
    }

    #[test]
    fn rollback_is_idempotent_and_does_not_dirty_tap() {
        let mut tap_buf = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 64];
        let config = Config::new(&mut tap_buf, &mut slab);
        let rendezvous = Rendezvous::from_config(config, DummyTransport);

        let sid = SessionId(0xA0);
        let lane = Lane(1);
        {
            let lease = rendezvous
                .lease_port(sid, lane, 0)
                .expect("port registration succeeds");
            let port = lease.port();
            port.r#gen()
                .check_and_update(lane, Generation(0))
                .expect("initial generation accepted");
            port.r#gen()
                .check_and_update(lane, Generation(9))
                .expect("generation update accepted");

            let epoch = rendezvous.checkpoint(sid).expect("checkpoint succeeds");
            let head_before = rendezvous.tap().head();
            rendezvous
                .rollback(sid, epoch)
                .expect("first rollback succeeds");
            let head_after = rendezvous.tap().head();
            assert_eq!(head_after, head_before + 2);

            let tap = rendezvous.tap().as_slice();
            let idx_req = (head_after - 2) % RING_EVENTS;
            let idx_ok = (head_after - 1) % RING_EVENTS;
            assert_eq!(tap[idx_req].id, ids::ROLLBACK_REQ);
            assert_eq!(tap[idx_ok].id, ids::ROLLBACK_OK);

            let head_retry = rendezvous.tap().head();
            let err = rendezvous
                .rollback(sid, epoch)
                .expect_err("second rollback reports consumed");
            assert!(
                matches!(err, RollbackError::AlreadyConsumed { sid: _ }),
                "expected AlreadyConsumed, got {:?}",
                err
            );
            assert_eq!(
                rendezvous.tap().head(),
                head_retry,
                "second rollback must not emit taps"
            );
        }
    }

    #[test]
    fn rollback_unknown_session_preserves_tap() {
        let mut tap_buf = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 64];
        let config = Config::new(&mut tap_buf, &mut slab);
        let rendezvous = Rendezvous::from_config(config, DummyTransport);

        let unknown_sid = SessionId(0xFFFF);
        let head_before = rendezvous.tap().head();

        let err = rendezvous.rollback(unknown_sid, Generation(42));
        assert!(matches!(
            err,
            Err(RollbackError::UnknownSession { sid }) if sid == unknown_sid
        ));

        assert_eq!(
            rendezvous.tap().head(),
            head_before,
            "UnknownSession rollback must not emit any tap events"
        );
    }

    #[test]
    fn rollback_epoch_mismatch_preserves_tap() {
        let mut tap_buf = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 64];
        let config = Config::new(&mut tap_buf, &mut slab);
        let rendezvous = Rendezvous::from_config(config, DummyTransport);

        let sid = SessionId(0x1234);
        let lane = Lane(0);
        {
            let lease = rendezvous
                .lease_port(sid, lane, 0)
                .expect("port registration succeeded");
            let port = lease.port();
            port.r#gen()
                .check_and_update(lane, Generation(0))
                .expect("initial generation set");
            port.r#gen()
                .check_and_update(lane, Generation(7))
                .expect("generation advanced");

            let epoch = rendezvous.checkpoint(sid).expect("checkpoint succeeds");
            assert_eq!(epoch, Generation(7));

            let head_before = rendezvous.tap().head();
            let err = rendezvous.rollback(sid, Generation(5));
            assert!(
                matches!(
                    err,
                    Err(RollbackError::StaleCheckpoint {
                        sid: _,
                        requested: Generation(5),
                        current: Generation(7)
                    })
                ),
                "expected StaleCheckpoint, got {:?}",
                err
            );
            assert_eq!(
                rendezvous.tap().head(),
                head_before,
                "StaleCheckpoint rollback must not emit any tap events"
            );
        }
    }

    #[test]
    fn rollback_consumed_session_preserves_tap() {
        let mut tap_buf = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 64];
        let config = Config::new(&mut tap_buf, &mut slab);
        let rendezvous = Rendezvous::from_config(config, DummyTransport);

        let sid = SessionId(0xABCD);
        let lane = Lane(3);
        {
            let lease = rendezvous
                .lease_port(sid, lane, 0)
                .expect("port registration succeeded");
            let port = lease.port();
            port.r#gen()
                .check_and_update(lane, Generation(0))
                .expect("initial generation set");
            port.r#gen()
                .check_and_update(lane, Generation(10))
                .expect("generation advanced");

            let epoch = rendezvous.checkpoint(sid).expect("checkpoint succeeds");
            assert_eq!(epoch, Generation(10));

            let head_before = rendezvous.tap().head();
            rendezvous
                .rollback(sid, epoch)
                .expect("first rollback succeeds");

            let head_after_first = rendezvous.tap().head();
            assert_eq!(
                head_after_first,
                head_before + 2,
                "first rollback emits ROLLBACK_REQ and ROLLBACK_OK"
            );

            let head_before_second = rendezvous.tap().head();
            let err = rendezvous
                .rollback(sid, epoch)
                .expect_err("second rollback should fail with consumed state");
            assert!(
                matches!(err, RollbackError::AlreadyConsumed { sid: _ }),
                "expected AlreadyConsumed, got {:?}",
                err
            );
            assert_eq!(
                rendezvous.tap().head(),
                head_before_second,
                "Consumed (second) rollback must not emit any tap events"
            );
        }
    }

    #[test]
    fn repeated_checkpoint_preserves_snapshot() {
        let mut tap_buf = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 64];
        let config = Config::new(&mut tap_buf, &mut slab);
        let rendezvous = Rendezvous::from_config(config, DummyTransport);

        let sid = SessionId(0xB0);
        let lane = Lane(2);
        {
            let lease = rendezvous
                .lease_port(sid, lane, 0)
                .expect("port registration succeeds");
            let port = lease.port();
            port.r#gen()
                .check_and_update(lane, Generation(0))
                .expect("initial generation accepted");
            port.r#gen()
                .check_and_update(lane, Generation(5))
                .expect("generation update accepted");

            let first = rendezvous.checkpoint(sid).expect("first checkpoint");
            assert_eq!(first, Generation(5));
            let head_before = rendezvous.tap().head();
            let second = rendezvous.checkpoint(sid).expect("second checkpoint");
            assert_eq!(second, first);
            assert_eq!(rendezvous.tap().head(), head_before + 1);

            let snapshot = rendezvous.association(sid).expect("snapshot present");
            assert_eq!(snapshot.last_checkpoint, Some(Generation(5)));
        }
    }

    #[test]
    fn splice_failure_does_not_emit_taps() {
        let mut tap_buf = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 64];
        let config = Config::new(&mut tap_buf, &mut slab);
        let rendezvous = Rendezvous::from_config(config, DummyTransport);

        let sid = SessionId(0xC0);
        let lane = Lane(3);
        {
            let lease = rendezvous
                .lease_port(sid, lane, 0)
                .expect("port registration succeeds");
            let port = lease.port();
            port.r#gen()
                .check_and_update(lane, Generation(0))
                .expect("initial generation accepted");
            port.r#gen()
                .check_and_update(lane, Generation(4))
                .expect("generation update accepted");

            // Try to begin splice with stale generation (2 < 4)
            let head_before = rendezvous.tap().head();
            let err = rendezvous
                .begin_splice(sid, lane, None, Generation(2))
                .unwrap_err();

            // Should fail with StaleGeneration
            assert!(matches!(err, RaSpliceError::StaleGeneration { .. }));

            // Should not emit any tap events on failure
            assert_eq!(rendezvous.tap().head(), head_before);
        }
    }

    #[test]
    fn tap_uses_injected_clock() {
        use core::cell::Cell;

        struct StepClock {
            next: Cell<u32>,
            step: u32,
        }

        impl StepClock {
            fn new(start: u32, step: u32) -> Self {
                Self {
                    next: Cell::new(start),
                    step,
                }
            }
        }

        impl Clock for StepClock {
            fn now32(&self) -> u32 {
                let current = self.next.get();
                self.next.set(current.saturating_add(self.step));
                current
            }
        }

        let mut tap_buf = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 64];
        let config = Config::new(&mut tap_buf, &mut slab).with_clock(StepClock::new(100, 7));
        let rendezvous = Rendezvous::from_config(config, DummyTransport);

        let sid = SessionId(0xD0);
        let lane = Lane(4);
        {
            let lease = rendezvous
                .lease_port(sid, lane, 0)
                .expect("port registration succeeds");
            let port = lease.port();
            port.r#gen()
                .check_and_update(lane, Generation(0))
                .expect("initial generation accepted");

            rendezvous.cancel_begin(sid).expect("begin succeeds");
            let head = rendezvous.tap().head();
            let idx_begin = (head - 1) % RING_EVENTS;
            assert_eq!(rendezvous.tap().as_slice()[idx_begin].ts, 107); // port() emitted CpEffect::Open at ts=100

            rendezvous
                .cancel_ack(sid, Generation(0))
                .expect("ack succeeds");
            let head = rendezvous.tap().head();
            let idx_ack = (head - 1) % RING_EVENTS;
            assert_eq!(rendezvous.tap().as_slice()[idx_ack].ts, 114); // Now at ts=114
        }
    }

    #[test]
    fn port_rejects_busy_lane() {
        let mut tap_buf = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 64];
        let config = Config::new(&mut tap_buf, &mut slab);
        let rendezvous = Rendezvous::from_config(config, DummyTransport);

        let lane = Lane(2);
        let sid_a = SessionId(100);
        let sid_b = SessionId(101);

        {
            let lease_a = rendezvous.lease_port(sid_a, lane, 0).expect("lane is free");

            let err = rendezvous.lease_port(sid_b, lane, 0);
            assert!(matches!(err, Err(RendezvousError::LaneBusy { lane: l }) if l == lane));

            drop(lease_a);
            rendezvous
                .lease_port(sid_b, lane, 0)
                .expect("lane available after release");
        }
    }

    #[test]
    fn port_allows_multi_lane_session() {
        let mut tap_buf = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 64];
        let config = Config::new(&mut tap_buf, &mut slab);
        let rendezvous = Rendezvous::from_config(config, DummyTransport);

        let sid = SessionId(200);
        let lane_a = Lane(0);
        let lane_b = Lane(1);

        {
            let lease_a = rendezvous.lease_port(sid, lane_a, 0).expect("lane a free");
            let lease_b = rendezvous.lease_port(sid, lane_b, 0).expect("lane b free for same sid");

            drop(lease_b);
            drop(lease_a);

            rendezvous
                .lease_port(sid, lane_a, 0)
                .expect("lane a available after release");
        }
    }

    #[cfg(all(test, feature = "std"))]
    mod prop {
        use super::*;
        use crate::observe::ids;
        use crate::runtime::consts::LANES_MAX;
        use proptest::collection;
        use proptest::prelude::*;
        use std::collections::{BTreeMap, btree_map::Entry};
        use std::vec::Vec;

        fn resize_or_fill(mut values: Vec<u16>, target_len: usize) -> Vec<u16> {
            if values.len() >= target_len {
                values.truncate(target_len);
                return values;
            }
            values.resize(target_len, 0);
            values
        }

        fn lane_distribution() -> impl Strategy<Value = Vec<(u8, u8, Vec<u16>)>> {
            // Generate up to eight programme entries; duplicates are folded by the
            // property under test so we don't need to enforce uniqueness here.
            let lanes = 0u8..LANES_MAX;
            let counts = 0u8..=5;
            let gens = collection::vec(any::<u16>(), 0..=5);
            collection::vec((lanes, counts, gens), 1..=8)
        }

        fn cancel_script() -> impl Strategy<Value = Vec<(u8, bool, u16, u8)>> {
            collection::vec(
                (0u8..LANES_MAX, any::<bool>(), any::<u16>(), any::<u8>()),
                1..=64,
            )
        }

        struct LaneState<'rv, 'cfg> {
            sid: SessionId,
            begins: u32,
            acks: u32,
            last_gen: Option<Generation>,
            _lease: LaneLease<
                'rv,
                'cfg,
                DummyTransport,
                DefaultLabelUniverse,
                CounterClock,
                crate::control::cap::EpochInit,
            >,
        }

        proptest! {
            #![proptest_config(ProptestConfig { cases: 256, .. ProptestConfig::default() })]

            #[test]
            fn cancel_counters_remain_balanced(entries in lane_distribution()) {
                let result: TestCaseResult = {
                    let mut tap_buf = [TapEvent::default(); RING_EVENTS];
                    let mut slab = [0u8; 64];
                    let config = Config::new(&mut tap_buf, &mut slab);
                    let rendezvous = Rendezvous::from_config(config, DummyTransport);

                    let mut totals: BTreeMap<u32, (Lane, u32, u32, Option<Generation>)> =
                        BTreeMap::new();
                    let mut leases: BTreeMap<
                        u8,
                        (
                            SessionId,
                            LaneLease<
                                '_,
                                '_,
                                DummyTransport,
                                DefaultLabelUniverse,
                                CounterClock,
                                crate::control::cap::EpochInit,
                            >,
                        ),
                    > = BTreeMap::new();
                    let mut ack_progress: BTreeMap<u8, Option<Generation>> = BTreeMap::new();

                    let head_before = rendezvous.tap().head();

                    for (lane_id, count_raw, gens_raw) in entries {
                        if count_raw == 0 {
                            continue;
                        }
                        let lane = Lane::new(lane_id as u32);
                        let sid = match leases.entry(lane_id) {
                            Entry::Occupied(entry) => entry.get().0,
                            Entry::Vacant(entry) => {
                                let sid = SessionId(0x1000 + lane_id as u32);
                                let lease = rendezvous
                                    .lease_port(sid, lane, 0)
                                    .expect("lane registration succeeds");
                                entry.insert((sid, lease));
                                sid
                            }
                        };
                        let sid_val = sid.raw();

                        let count = count_raw as usize;
                        let mut gens = resize_or_fill(gens_raw, count);

                        let mut last = ack_progress
                            .get(&lane_id)
                            .and_then(|opt| opt.map(|g| g.0))
                            .unwrap_or(0);

                        for value in gens.iter_mut() {
                            let delta = *value;
                            let next = last.saturating_add(delta % 4 + 1);
                            *value = next;
                            last = next;
                        }

                        for _ in 0..count {
                            rendezvous
                                .cancel_begin(sid)
                                .expect("begin increments succeed");
                        }

                        let mut last_ack = None;
                        for value in gens {
                            let generation = Generation(value);
                            if let Some(prev) = ack_progress.get(&lane_id).copied().flatten() {
                                prop_assert!(
                                    generation.0 >= prev.0,
                                    "ack generation must be non-decreasing"
                                );
                            }
                            rendezvous
                                .cancel_ack(sid, generation)
                                .expect("ack increments succeed");
                            last_ack = Some(generation);
                            ack_progress.insert(lane_id, Some(generation));
                        }

                        let entry = totals.entry(sid_val).or_insert((lane, 0, 0, None));
                        entry.1 += count_raw as u32;
                        entry.2 += count_raw as u32;
                        if let Some(last) = last_ack {
                            entry.3 = Some(last);
                        }
                    }

                    let head_after = rendezvous.tap().head();
                    let total_events = head_after.saturating_sub(head_before);

                    let storage = rendezvous.tap().as_slice();
                    let mut observed_begin: BTreeMap<u32, u32> = BTreeMap::new();
                    let mut observed_ack: BTreeMap<u32, u32> = BTreeMap::new();

                    for offset in 0..total_events {
                        let idx = (head_before + offset) % RING_EVENTS;
                        let event = storage[idx];
                        match event.id {
                            ids::CANCEL_BEGIN => {
                                *observed_begin.entry(event.arg0).or_default() += 1;
                            }
                            ids::CANCEL_ACK => {
                                *observed_ack.entry(event.arg0).or_default() += 1;
                            }
                            _ => {}
                        }
                    }

                    assert!(
                        total_events <= RING_EVENTS,
                        "trace must not wrap in test"
                    );

                    for (sid_val, (lane, expected_begin, expected_ack, expected_last)) in totals {
                        let sid = SessionId(sid_val);
                        let snapshot = rendezvous
                            .association(sid)
                            .expect("association must exist for registered lane");

                        assert_eq!(snapshot.acks.cancel_begin, expected_begin);
                        assert_eq!(snapshot.acks.cancel_ack, expected_ack);
                        match expected_last {
                            Some(val) => assert_eq!(snapshot.acks.last_gen, Some(val)),
                            None => assert_eq!(snapshot.acks.last_gen, None),
                        }

                        assert_eq!(snapshot.lane, lane);

                        let begins = observed_begin.get(&sid_val).copied().unwrap_or(0);
                        let acks = observed_ack.get(&sid_val).copied().unwrap_or(0);
                        assert_eq!(begins, expected_begin);
                        assert_eq!(acks, expected_ack);
                    }

                    let unknown_sid = SessionId(0xDEAD_0000);
                    let head_unknown = rendezvous.tap().head();
                    assert!(matches!(
                        rendezvous.cancel_begin(unknown_sid),
                        Err(CancelError::UnknownSession { sid }) if sid == unknown_sid
                    ));
                    assert_eq!(
                        rendezvous.tap().head(),
                        head_unknown,
                        "unknown cancel_begin must not emit taps"
                    );

                    let head_unknown_ack = rendezvous.tap().head();
                    assert!(matches!(
                        rendezvous.cancel_ack(unknown_sid, Generation(1)),
                        Err(CancelError::UnknownSession { sid }) if sid == unknown_sid
                    ));
                    assert_eq!(
                        rendezvous.tap().head(),
                        head_unknown_ack,
                        "unknown cancel_ack must not emit taps"
                    );

                    Ok(())
                };

                result
            }

        }

        proptest! {
            #![proptest_config(ProptestConfig { cases: 200, .. ProptestConfig::default() })]

            #[test]
            fn cancel_monotonic_and_lane_reuse(script in cancel_script()) {
                let result: TestCaseResult = {
                let mut tap_buf = [TapEvent::default(); RING_EVENTS];
                let mut slab = [0u8; 64];
                let config = Config::new(&mut tap_buf, &mut slab);
                let rendezvous = Rendezvous::from_config(config, DummyTransport);

                let mut lanes: BTreeMap<
                    u8,
                    LaneState<
                        '_,
                        '_,
                    >,
                > = BTreeMap::new();
                let mut sid_seed: u32 = 0xBEEF_0000;

                for (idx, (lane_id, is_ack, raw_gen, control)) in script.into_iter().enumerate() {
                    let lane = Lane::new(lane_id as u32);

                    if control % 11 == 0 {
                        if let Some(old_state) = lanes.remove(&lane_id) {
                            let old_sid = old_state.sid;
                            drop(old_state);
                            prop_assert!(rendezvous.association(old_sid).is_none());

                            let head_before = rendezvous.tap().head();
                            let err = rendezvous
                                .cancel_begin(old_sid)
                                .expect_err("released session must be unknown");
                            match err {
                                CancelError::UnknownSession { sid } => {
                                    prop_assert_eq!(sid, old_sid);
                                }
                            }
                            prop_assert_eq!(rendezvous.tap().head(), head_before);

                            let head_before_ack = rendezvous.tap().head();
                            let err = rendezvous
                                .cancel_ack(old_sid, Generation(raw_gen))
                                .expect_err("released session must be unknown");
                            match err {
                                CancelError::UnknownSession { sid } => {
                                    prop_assert_eq!(sid, old_sid);
                                }
                            }
                            prop_assert_eq!(rendezvous.tap().head(), head_before_ack);
                        }
                        continue;
                    }

                        if !lanes.contains_key(&lane_id) {
                            let sid = SessionId(sid_seed);
                            sid_seed = sid_seed.wrapping_add(1);
                            let lease = rendezvous
                                .lease_port(sid, lane, 0)
                                .expect("lane registration succeeds");
                            lanes.insert(
                                lane_id,
                                LaneState {
                                    sid,
                                    begins: 0,
                                    acks: 0,
                                    last_gen: None,
                                    _lease: lease,
                                },
                            );
                        }

                    if control % 5 == 0 {
                        let unknown_sid = SessionId(0xDEAD_0000u32.wrapping_add(idx as u32));
                        let head = rendezvous.tap().head();
                        let err = rendezvous
                            .cancel_begin(unknown_sid)
                            .expect_err("unknown cancel_begin must error");
                        match err {
                            CancelError::UnknownSession { sid } => {
                                prop_assert_eq!(sid, unknown_sid);
                            }
                        }
                        prop_assert_eq!(rendezvous.tap().head(), head);
                    }

                    if control % 7 == 0 {
                        let unknown_sid = SessionId(0xDEAD_8000u32.wrapping_add(idx as u32));
                        let head = rendezvous.tap().head();
                        let err = rendezvous
                            .cancel_ack(unknown_sid, Generation(raw_gen))
                            .expect_err("unknown cancel_ack must error");
                        match err {
                            CancelError::UnknownSession { sid } => {
                                prop_assert_eq!(sid, unknown_sid);
                            }
                        }
                        prop_assert_eq!(rendezvous.tap().head(), head);
                    }

                    let state = lanes.get_mut(&lane_id).expect("lane state exists");

                    if !is_ack {
                        let head_before = rendezvous.tap().head();
                        rendezvous
                            .cancel_begin(state.sid)
                            .expect("begin for registered lane succeeds");
                        state.begins = state.begins.saturating_add(1);

                        let snapshot = rendezvous
                            .association(state.sid)
                            .expect("association available for active lane");
                        prop_assert_eq!(snapshot.acks.cancel_begin, state.begins);
                        prop_assert_eq!(snapshot.acks.cancel_ack, state.acks);
                        prop_assert_eq!(snapshot.acks.last_gen, state.last_gen);
                        prop_assert_eq!(rendezvous.tap().head(), head_before.saturating_add(1));
                        continue;
                    }

                    let stale = control % 3 == 0 && state.last_gen.is_some();
                    let candidate_val = if stale {
                        let base = state.last_gen.unwrap().0;
                        base.saturating_sub(((raw_gen % 4) as u16).saturating_add(1))
                    } else {
                        let step = ((raw_gen % 5) as u16).saturating_add(1);
                        state.last_gen.map(|g| g.0).unwrap_or(0).saturating_add(step)
                    };
                    let candidate = Generation(candidate_val);

                    let head_before = rendezvous.tap().head();
                    rendezvous
                        .cancel_ack(state.sid, candidate)
                        .expect("ack for registered lane succeeds");
                    prop_assert_eq!(rendezvous.tap().head(), head_before.saturating_add(1));

                    state.acks = state.acks.saturating_add(1);
                    state.last_gen = match state.last_gen {
                        Some(prev) if prev.0 >= candidate.0 => Some(prev),
                        _ => Some(candidate),
                    };

                    let snapshot = rendezvous
                        .association(state.sid)
                        .expect("association available after ack");
                    prop_assert_eq!(snapshot.acks.cancel_begin, state.begins);
                    prop_assert_eq!(snapshot.acks.cancel_ack, state.acks);
                    prop_assert_eq!(snapshot.acks.last_gen, state.last_gen);
                }

                for (lane_id, state) in &lanes {
                    let lane = Lane::new(*lane_id as u32);
                    let snapshot = rendezvous
                        .association(state.sid)
                        .expect("association remains for active lane");
                    prop_assert!(snapshot.active);
                    prop_assert_eq!(snapshot.lane, lane);
                    prop_assert_eq!(snapshot.acks.cancel_begin, state.begins);
                    prop_assert_eq!(snapshot.acks.cancel_ack, state.acks);
                    prop_assert_eq!(snapshot.acks.last_gen, state.last_gen);
                }
                Ok(())
            };
            result
            }
        }
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
    E: crate::control::cap::EpochTable,
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
pub struct CapsFacet<T, U, C, E>(PhantomData<(T, U, C, E)>)
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::EpochTable;

impl<T, U, C, E> Copy for CapsFacet<T, U, C, E>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::EpochTable,
{
}

impl<T, U, C, E> Clone for CapsFacet<T, U, C, E>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::EpochTable,
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
    E: crate::control::cap::EpochTable,
{
    #[inline]
    pub const fn new() -> Self {
        Self(PhantomData)
    }

    /// Mint a capability token and register it in the CapTable.
    #[allow(clippy::too_many_arguments)]
    pub fn mint_cap<K: crate::control::cap::ResourceKind>(
        self,
        rendezvous: &Rendezvous<'_, '_, T, U, C, E>,
        sid: SessionId,
        lane: Lane,
        shot: crate::control::cap::CapShot,
        dest_role: u8,
        nonce: [u8; 16],
        handle: K::Handle,
    ) {
        rendezvous.mint_cap::<K>(sid, lane, shot, dest_role, nonce, handle)
    }

    /// Generate the next nonce seed for capability minting.
    #[inline]
    pub fn next_nonce_seed(
        self,
        rendezvous: &Rendezvous<'_, '_, T, U, C, E>,
    ) -> crate::control::cap::NonceSeed {
        rendezvous.next_nonce_seed()
    }

    /// Claim a capability from the CapTable.
    pub fn claim_cap<K: crate::control::cap::ResourceKind>(
        self,
        rendezvous: &mut Rendezvous<'_, '_, T, U, C, E>,
        token: &crate::control::cap::GenericCapToken<K>,
    ) -> Result<crate::control::cap::VerifiedCap<K>, crate::rendezvous::error::CapError> {
        rendezvous.claim_cap(token)
    }
}

/// Delegation-focused facet that builds on capability operations.
#[derive(Default)]
pub struct DelegationFacet<T, U, C, E>(PhantomData<(T, U, C, E)>)
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::EpochTable;

impl<T, U, C, E> Copy for DelegationFacet<T, U, C, E>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::EpochTable,
{
}

impl<T, U, C, E> Clone for DelegationFacet<T, U, C, E>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::EpochTable,
{
    fn clone(&self) -> Self {
        *self
    }
}

impl<T, U, C, E> DelegationFacet<T, U, C, E>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::EpochTable,
{
    #[inline]
    pub const fn new() -> Self {
        Self(PhantomData)
    }

    /// Derive the next nonce seed for delegation capability minting.
    #[inline]
    pub fn next_nonce_seed(
        self,
        rendezvous: &Rendezvous<'_, '_, T, U, C, E>,
    ) -> crate::control::cap::NonceSeed {
        CapsFacet::<T, U, C, E>::new().next_nonce_seed(rendezvous)
    }

    /// Mint a delegation capability token under canonical policy.
    #[inline]
    #[allow(clippy::too_many_arguments)]
    pub fn mint_cap<K: crate::control::cap::ResourceKind>(
        self,
        rendezvous: &Rendezvous<'_, '_, T, U, C, E>,
        sid: SessionId,
        lane: Lane,
        shot: crate::control::cap::CapShot,
        dest_role: u8,
        nonce: [u8; 16],
        handle: K::Handle,
    ) {
        CapsFacet::<T, U, C, E>::new()
            .mint_cap::<K>(rendezvous, sid, lane, shot, dest_role, nonce, handle);
    }

    /// Claim a previously minted capability token.
    #[inline]
    pub fn claim_cap<K: crate::control::cap::ResourceKind>(
        self,
        rendezvous: &mut Rendezvous<'_, '_, T, U, C, E>,
        token: &crate::control::cap::GenericCapToken<K>,
    ) -> Result<crate::control::cap::VerifiedCap<K>, crate::rendezvous::error::CapError> {
        CapsFacet::<T, U, C, E>::new().claim_cap(rendezvous, token)
    }
}

/// Splice-focused facet that exposes only splice coordination operations.
#[derive(Default)]
pub struct SpliceFacet<T, U, C, E>(PhantomData<(T, U, C, E)>)
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::EpochTable;

impl<T, U, C, E> Copy for SpliceFacet<T, U, C, E>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::EpochTable,
{
}

impl<T, U, C, E> Clone for SpliceFacet<T, U, C, E>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::EpochTable,
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
    E: crate::control::cap::EpochTable,
{
    #[inline]
    pub const fn new() -> Self {
        Self(PhantomData)
    }

    pub fn begin(
        self,
        rendezvous: &mut Rendezvous<'_, '_, T, U, C, E>,
        sid: SessionId,
        lane: Lane,
        fences: Option<(u32, u32)>,
        generation: Generation,
    ) -> Result<(), crate::rendezvous::error::SpliceError> {
        rendezvous.begin_splice(sid, lane, fences, generation)
    }

    pub fn acknowledge(
        self,
        rendezvous: &mut Rendezvous<'_, '_, T, U, C, E>,
        sid: SessionId,
        lane: Lane,
    ) -> Result<(), crate::rendezvous::error::SpliceError> {
        rendezvous.acknowledge_splice(sid, lane)
    }

    pub fn commit(
        self,
        rendezvous: &mut Rendezvous<'_, '_, T, U, C, E>,
        sid: SessionId,
        lane: Lane,
    ) -> Result<(), crate::rendezvous::error::SpliceError> {
        rendezvous.commit_splice(sid, lane)
    }

    pub fn release_lane(self, rendezvous: &Rendezvous<'_, '_, T, U, C, E>, lane: Lane) {
        if let Some(sid) = rendezvous.release_lane(lane) {
            rendezvous.emit_lane_release(sid, lane);
        }
    }
}

/// Observation facet that exposes tap emission without leaking rendezvous state.
#[derive(Clone, Copy)]
pub struct ObserveFacet<'tap, 'cfg> {
    tap: &'tap crate::observe::TapRing<'cfg>,
}

impl<'tap, 'cfg> ObserveFacet<'tap, 'cfg> {
    #[inline]
    pub const fn new(tap: &'tap crate::observe::TapRing<'cfg>) -> Self {
        Self { tap }
    }

    /// Emit an already constructed tap event.
    #[inline]
    pub fn emit(&self, event: crate::observe::TapEvent) {
        crate::observe::emit(self.tap, event);
    }

    /// Emit a tap event from individual fields.
    #[inline]
    pub fn emit_fields(&self, ts: u32, id: u16, arg0: u32, arg1: u32) {
        crate::observe::emit(self.tap, crate::observe::RawEvent::new(ts, id, arg0, arg1));
    }

    /// Borrow the underlying tap ring (read-only).
    #[inline]
    pub fn tap(&self) -> &'tap crate::observe::TapRing<'cfg> {
        self.tap
    }
}

/// Slot management facet that exposes slot operations for policy bytecode management.
///
/// This facet is a zero-sized marker; all state lives in the rendezvous. Methods explicitly
/// receive the rendezvous handle, keeping the facet trivially copyable and suitable for
/// `const fn` projection.
#[derive(Default)]
pub struct SlotFacet<T, U, C, E>(PhantomData<(T, U, C, E)>)
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::EpochTable;

impl<T, U, C, E> Copy for SlotFacet<T, U, C, E>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::EpochTable,
{
}

impl<T, U, C, E> Clone for SlotFacet<T, U, C, E>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::EpochTable,
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
    E: crate::control::cap::EpochTable,
{
    #[inline]
    pub const fn new() -> Self {
        Self(PhantomData)
    }

    /// Load and commit bytecode to a slot.
    pub fn load_commit<State>(
        self,
        rendezvous: &mut Rendezvous<'_, '_, T, U, C, E>,
        slot: crate::epf::Slot,
        manager: &mut crate::runtime::mgmt::Manager<State, { crate::rendezvous::SLOT_COUNT }>,
    ) -> Result<(), crate::runtime::mgmt::MgmtError>
    where
        State: crate::runtime::mgmt::ManagerState,
    {
        rendezvous.with_slot_bundle_lease(|lease| lease.load_commit_with(slot, manager))
    }

    /// Activate a slot, making its policy bytecode active.
    pub fn activate<State>(
        self,
        rendezvous: &mut Rendezvous<'_, '_, T, U, C, E>,
        slot: crate::epf::Slot,
        manager: &mut crate::runtime::mgmt::Manager<State, { crate::rendezvous::SLOT_COUNT }>,
    ) -> Result<crate::runtime::mgmt::TransitionReport, crate::runtime::mgmt::MgmtError>
    where
        State: crate::runtime::mgmt::ManagerState,
    {
        rendezvous.with_slot_bundle_lease(|lease| lease.activate_with(slot, manager))
    }

    /// Revert a slot to the previous active policy.
    pub fn revert<State>(
        self,
        rendezvous: &mut Rendezvous<'_, '_, T, U, C, E>,
        slot: crate::epf::Slot,
        manager: &mut crate::runtime::mgmt::Manager<State, { crate::rendezvous::SLOT_COUNT }>,
    ) -> Result<crate::runtime::mgmt::TransitionReport, crate::runtime::mgmt::MgmtError>
    where
        State: crate::runtime::mgmt::ManagerState,
    {
        rendezvous.with_slot_bundle_lease(|lease| lease.revert_with(slot, manager))
    }
}
