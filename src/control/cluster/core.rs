//! SessionCluster - Distributed control-plane coordination.
//!
//! This module implements SessionCluster, which coordinates multiple Rendezvous
//! instances for local distributed session management.

use core::cell::UnsafeCell;
use core::marker::PhantomData;
use core::mem::MaybeUninit;

#[cfg(test)]
use crate::control::automaton::delegation::DelegationLeaseSpec;
#[cfg(test)]
use crate::control::automaton::topology::TopologyLeaseSpec;
use crate::control::automaton::{
    distributed::{DistributedTopology, DistributedTopologyInv, TopologyAck, TopologyIntent},
    topology::{TopologyBeginAutomaton, TopologyGraphContext},
};
use crate::control::cap::atomic_codecs::{
    DelegationHandle, SessionLaneHandle, TopologyHandle, decode_session_lane_handle,
};
use crate::control::cap::mint::CapHeader;
use crate::control::cap::mint::{
    CAP_TOKEN_LEN, ControlOp, EndpointResource, GenericCapToken, MintConfigMarker,
};
use crate::control::cluster::effects::EffectEnvelopeRef;
use crate::control::cluster::error::{StateRestoreError, TxAbortError, TxCommitError};
use crate::control::lease::{
    bundle::{LeaseBundleContext, LeaseGraphBundleExt},
    core::{
        ControlAutomaton, ControlStep, DelegationDriveError, FullSpec, LeaseError,
        RegisterRendezvousError,
    },
    graph::{LeaseFacet, LeaseGraph, LeaseGraphError, LeaseSpec},
    planner::{LeaseFacetNeeds, facets_caps_topology},
};
use crate::global::ControlDesc;
use crate::global::const_dsl::ControlScopeKind;
use crate::rendezvous::TopologySessionState;

type PublicEndpointKernel<'r, const ROLE: u8, T, U, C, const MAX_RV: usize, Mint> =
    crate::endpoint::kernel::CursorEndpoint<
        'r,
        ROLE,
        T,
        U,
        C,
        crate::control::cap::mint::EpochTbl,
        MAX_RV,
        Mint,
        crate::binding::BindingHandle<'r>,
    >;
#[cfg(test)]
type PublicEndpointKernelRaw<'cfg, T, U, C, const MAX_RV: usize> =
    crate::endpoint::kernel::CursorEndpoint<
        'cfg,
        0,
        T,
        U,
        C,
        crate::control::cap::mint::EpochTbl,
        MAX_RV,
        crate::control::cap::mint::MintConfig,
        crate::binding::BindingHandle<'cfg>,
    >;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PublicEndpointStorageLayout {
    total_bytes: usize,
    total_align: usize,
    header_bytes: usize,
    port_slots_bytes: usize,
    guard_slots_bytes: usize,
    header_padding_bytes: usize,
    arena_offset: usize,
    arena_bytes: usize,
    arena_align: usize,
}

fn topology_operands_from_handle(handle: TopologyHandle) -> TopologyOperands {
    TopologyOperands::new(
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TopologyDescriptor {
    handle: TopologyHandle,
}

impl TopologyDescriptor {
    #[inline]
    pub(crate) fn decode(
        bytes: [u8; crate::control::cap::mint::CAP_HANDLE_LEN],
    ) -> Result<Self, CpError> {
        let handle = TopologyHandle::decode(bytes).map_err(|_| CpError::Authorisation {
            operation: ControlOp::TopologyBegin as u8,
        })?;
        Ok(Self { handle })
    }

    #[inline]
    pub(crate) const fn handle(self) -> TopologyHandle {
        self.handle
    }
}

#[inline]
fn validate_topology_rendezvous_pair(
    src_rv: RendezvousId,
    dst_rv: RendezvousId,
    operation: ControlOp,
) -> Result<(), CpError> {
    if src_rv.raw() == 0 || dst_rv.raw() == 0 || src_rv == dst_rv {
        return Err(CpError::Authorisation {
            operation: operation as u8,
        });
    }
    Ok(())
}

#[inline]
const fn unpack_u16_pair(word: u32) -> (u16, u16) {
    ((word >> 16) as u16, word as u16)
}

#[inline]
fn delegation_handle_from_route_input(
    rv_id: RendezvousId,
    src_lane: Lane,
    input: [u32; 4],
) -> Result<DelegationHandle, CpError> {
    let (dst_rv_raw, dst_lane_raw) = unpack_u16_pair(input[0]);
    if dst_rv_raw == 0 {
        return Err(CpError::Authorisation {
            operation: ControlOp::CapDelegate as u8,
        });
    }

    Ok(DelegationHandle {
        src_rv: rv_id.raw(),
        dst_rv: dst_rv_raw,
        src_lane: src_lane.raw() as u16,
        dst_lane: dst_lane_raw,
        seq_tx: input[1],
        seq_rx: input[2],
        shard: input[3],
        flags: 0,
    })
}

use super::error::{AttachError, CpError, DelegationError, TopologyError};
use crate::control::automaton::txn::{InAcked, InBegin, NoopTap};
use crate::control::types::{Generation, Lane, RendezvousId, SessionId};
use crate::eff::EffIndex;
#[cfg(test)]
use crate::global::compiled::images::CompiledProgramFacts;
use crate::global::{
    compiled::images::{CompiledProgramRef, CompiledRoleImage, RoleImageSlice},
    const_dsl::{PolicyMode, ScopeId},
};
use crate::observe::scope::ScopeTrace;
use crate::rendezvous::core::{EndpointLeaseId, LaneLease, Rendezvous};
use crate::rendezvous::error::RendezvousError;
use crate::transport::context::{self, ContextValue};

#[cfg(test)]
use std::thread_local;
/// Control-plane effect envelope encompassing the effect and its operands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TopologyOperands {
    pub(crate) src_rv: RendezvousId,
    pub(crate) dst_rv: RendezvousId,
    pub(crate) src_lane: Lane,
    pub(crate) dst_lane: Lane,
    pub(crate) old_gen: Generation,
    pub(crate) new_gen: Generation,
    pub(crate) seq_tx: u32,
    pub(crate) seq_rx: u32,
}

impl TopologyOperands {
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

    pub(crate) fn intent(&self, sid: SessionId) -> TopologyIntent {
        TopologyIntent::new(
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

    pub(crate) fn ack(&self, sid: SessionId) -> TopologyAck {
        TopologyAck::new(
            self.src_rv,
            self.dst_rv,
            sid.raw(),
            self.new_gen,
            self.src_lane,
            self.dst_lane,
            self.seq_tx,
            self.seq_rx,
        )
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CpCommand {
    pub(crate) effect: ControlOp,
    pub(crate) sid: Option<SessionId>,
    pub(crate) lane: Option<Lane>,
    pub(crate) generation: Option<Generation>,
    pub(crate) topology: Option<TopologyOperands>,
    pub(crate) delegate: Option<DelegateOperands>,
}

impl CpCommand {
    pub(crate) const fn new(effect: ControlOp) -> Self {
        Self {
            effect,
            sid: None,
            lane: None,
            generation: None,
            topology: None,
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

    pub(crate) fn with_topology(mut self, operands: TopologyOperands) -> Self {
        self.topology = Some(operands);
        self
    }

    pub(crate) fn with_delegate(mut self, delegate: DelegateOperands) -> Self {
        self.delegate = Some(delegate);
        self
    }

    fn derive_sid_lane(
        token: GenericCapToken<EndpointResource>,
    ) -> Result<(SessionId, Lane), CpError> {
        let handle = token
            .endpoint_identity()
            .map_err(|_| CpError::Delegation(DelegationError::InvalidToken))?;
        Ok((handle.sid, handle.lane))
    }

    pub(crate) fn canonicalize_delegate(mut self) -> Result<Self, CpError> {
        let delegate = self
            .delegate
            .ok_or(CpError::Delegation(DelegationError::InvalidToken))?;
        let (sid, lane) = Self::derive_sid_lane(delegate.token)?;
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

    pub(crate) fn canonicalize_topology(mut self) -> Result<Self, CpError> {
        let Some(operands) = self.topology else {
            return Err(CpError::Topology(TopologyError::InvalidState));
        };
        let (lane, generation) = match self.effect {
            ControlOp::TopologyBegin | ControlOp::TopologyCommit => {
                (operands.src_lane, operands.new_gen)
            }
            ControlOp::TopologyAck => (operands.dst_lane, operands.new_gen),
            _ => return Ok(self),
        };
        if let Some(current) = self.lane
            && current != lane
        {
            let _ = current;
            return Err(CpError::Topology(TopologyError::LaneMismatch));
        }
        if self.generation.is_some_and(|current| current != generation) {
            return Err(CpError::Topology(TopologyError::GenerationMismatch));
        }
        self.lane = Some(lane);
        self.generation = Some(generation);
        Ok(self)
    }

    pub(crate) fn topology_begin(sid: SessionId, operands: TopologyOperands) -> Self {
        Self::new(ControlOp::TopologyBegin)
            .with_sid(sid)
            .with_lane(operands.src_lane)
            .with_topology(operands)
    }

    pub(crate) fn topology_ack(sid: SessionId, operands: TopologyOperands) -> Self {
        Self::new(ControlOp::TopologyAck)
            .with_sid(sid)
            .with_lane(operands.dst_lane)
            .with_topology(operands)
    }

    pub(crate) fn topology_commit(sid: SessionId, operands: TopologyOperands) -> Self {
        Self::new(ControlOp::TopologyCommit)
            .with_sid(sid)
            .with_lane(operands.src_lane)
            .with_topology(operands)
    }

    pub(crate) fn abort_begin(sid: SessionId, lane: Lane) -> Self {
        Self::new(ControlOp::AbortBegin)
            .with_sid(sid)
            .with_lane(lane)
    }

    pub(crate) fn abort_ack(sid: SessionId, lane: Lane, generation: Generation) -> Self {
        Self::new(ControlOp::AbortAck)
            .with_sid(sid)
            .with_lane(lane)
            .with_generation(generation)
    }

    pub(crate) fn state_snapshot(sid: SessionId, lane: Lane) -> Self {
        Self::new(ControlOp::StateSnapshot)
            .with_sid(sid)
            .with_lane(lane)
    }

    pub(crate) fn state_restore(sid: SessionId, lane: Lane, generation: Generation) -> Self {
        Self::new(ControlOp::StateRestore)
            .with_sid(sid)
            .with_lane(lane)
            .with_generation(generation)
    }

    pub(crate) fn tx_commit(sid: SessionId, lane: Lane, generation: Generation) -> Self {
        Self::new(ControlOp::TxCommit)
            .with_sid(sid)
            .with_lane(lane)
            .with_generation(generation)
    }

    pub(crate) fn tx_abort(sid: SessionId, lane: Lane, generation: Generation) -> Self {
        Self::new(ControlOp::TxAbort)
            .with_sid(sid)
            .with_lane(lane)
            .with_generation(generation)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DynamicResolution {
    RouteArm { arm: u8 },
    Loop { decision: bool },
    Defer { retry_hint: u8 },
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
        scope_id: ScopeId,
        scope_trace: Option<ScopeTrace>,
        input: [u32; 4],
        attrs: &crate::transport::context::PolicyAttrs,
    ) -> Self {
        let mut policy_attrs = *attrs;
        let _ = policy_attrs.insert(context::core::RV_ID, ContextValue::from_u16(rv_id.raw()));
        if let Some(session) = session {
            let _ = policy_attrs.insert(
                context::core::SESSION_ID,
                ContextValue::from_u32(session.raw()),
            );
        }
        let _ = policy_attrs.insert(context::core::LANE, ContextValue::from_u32(lane.raw()));
        let _ = policy_attrs.insert(context::core::TAG, ContextValue::from_u8(tag));
        Self {
            rv_id,
            session,
            lane,
            eff_index,
            tag,
            scope_id,
            scope_trace,
            policy_input: input,
            policy_attrs,
        }
    }

    /// Query a policy attribute by opaque id.
    #[inline]
    pub fn attr(
        &self,
        id: crate::transport::context::ContextId,
    ) -> Option<crate::transport::context::ContextValue> {
        self.policy_attrs.get(id)
    }

    /// Read slot-scoped policy input argument by index.
    #[inline]
    pub fn input(&self, idx: u8) -> u32 {
        self.policy_input.get(idx as usize).copied().unwrap_or(0)
    }
}

type StatelessResolverFn = fn(ResolverContext) -> ResolverResult;
type ResolverStatePayload<S> = (*const S, fn(&S, ResolverContext) -> ResolverResult);
type ErasedResolverStatePayload = ResolverStatePayload<()>;

#[derive(Clone, Copy)]
union ResolverStorage {
    stateless: StatelessResolverFn,
    _stateful: ErasedResolverStatePayload,
}

#[derive(Clone, Copy)]
pub struct ResolverRef<'cfg> {
    storage: ResolverStorage,
    dispatch: unsafe fn(ResolverStorage, ResolverContext) -> ResolverResult,
    _marker: PhantomData<&'cfg ()>,
}

impl<'cfg> ResolverRef<'cfg> {
    #[inline]
    pub fn from_state<S: 'cfg>(
        state: &'cfg S,
        resolver: fn(&S, ResolverContext) -> ResolverResult,
    ) -> Self {
        const {
            assert!(
                core::mem::size_of::<ResolverStatePayload<S>>()
                    == core::mem::size_of::<ErasedResolverStatePayload>()
            );
            assert!(
                core::mem::align_of::<ResolverStatePayload<S>>()
                    == core::mem::align_of::<ErasedResolverStatePayload>()
            );
        }
        let payload = (core::ptr::from_ref(state), resolver);
        let mut storage = MaybeUninit::<ResolverStorage>::uninit();
        unsafe {
            storage
                .as_mut_ptr()
                .cast::<ResolverStatePayload<S>>()
                .write(payload);
        }
        Self {
            storage: unsafe { storage.assume_init() },
            dispatch: dispatch_state::<S>,
            _marker: PhantomData,
        }
    }

    #[inline]
    pub fn from_fn(resolver: fn(ResolverContext) -> ResolverResult) -> Self {
        Self {
            storage: ResolverStorage {
                stateless: resolver,
            },
            dispatch: dispatch_fn,
            _marker: PhantomData,
        }
    }

    #[inline]
    pub(crate) fn resolve(self, ctx: ResolverContext) -> ResolverResult {
        unsafe { (self.dispatch)(self.storage, ctx) }
    }
}

unsafe fn dispatch_state<S>(storage: ResolverStorage, ctx: ResolverContext) -> ResolverResult {
    const {
        assert!(
            core::mem::size_of::<ResolverStatePayload<S>>()
                == core::mem::size_of::<ErasedResolverStatePayload>()
        );
        assert!(
            core::mem::align_of::<ResolverStatePayload<S>>()
                == core::mem::align_of::<ErasedResolverStatePayload>()
        );
    }
    let (state, resolver) = unsafe {
        (&storage as *const ResolverStorage)
            .cast::<ResolverStatePayload<S>>()
            .read()
    };
    let state = unsafe { &*state };
    resolver(state, ctx)
}

unsafe fn dispatch_fn(storage: ResolverStorage, ctx: ResolverContext) -> ResolverResult {
    let resolver = unsafe { storage.stateless };
    resolver(ctx)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DynamicResolverKey {
    rv: RendezvousId,
    eff_index: EffIndex,
    op: ControlOp,
}

impl DynamicResolverKey {
    const fn new(rv: RendezvousId, eff_index: EffIndex, op: ControlOp) -> Self {
        Self { rv, eff_index, op }
    }
}

#[derive(Clone, Copy)]
struct DynamicResolverEntry<'cfg> {
    resolver: ResolverRef<'cfg>,
    policy: PolicyMode,
    scope_trace: Option<ScopeTrace>,
}

#[inline]
const fn cluster_rendezvous_slot<const MAX_RV: usize>(rv_id: RendezvousId) -> Option<usize> {
    let raw = rv_id.raw() as usize;
    if raw == 0 || raw > MAX_RV {
        None
    } else {
        Some(raw - 1)
    }
}

#[derive(Clone, Copy)]
struct ResolverBucketEntry<'cfg> {
    eff_index: EffIndex,
    op: ControlOp,
    entry: DynamicResolverEntry<'cfg>,
}

struct ResolverBucket<'cfg> {
    entries: UnsafeCell<*mut Option<ResolverBucketEntry<'cfg>>>,
    capacity: usize,
    _no_send_sync: PhantomData<*mut ()>,
}

impl<'cfg> ResolverBucket<'cfg> {
    const STORAGE_TAG_MASK: usize = Self::storage_align().saturating_sub(1);

    unsafe fn init_empty(dst: *mut Self) {
        unsafe {
            core::ptr::addr_of_mut!((*dst).entries).write(UnsafeCell::new(core::ptr::null_mut()));
            core::ptr::addr_of_mut!((*dst).capacity).write(0);
            core::ptr::addr_of_mut!((*dst)._no_send_sync).write(PhantomData);
        }
    }

    #[inline]
    const fn storage_align() -> usize {
        core::mem::align_of::<Option<ResolverBucketEntry<'cfg>>>()
    }

    #[inline]
    const fn storage_bytes(capacity: usize) -> usize {
        capacity.saturating_mul(core::mem::size_of::<Option<ResolverBucketEntry<'cfg>>>())
    }

    #[inline]
    fn raw_entries(&self) -> *mut Option<ResolverBucketEntry<'cfg>> {
        unsafe { *self.entries.get() }
    }

    #[inline]
    fn entries_ptr(&self) -> *mut Option<ResolverBucketEntry<'cfg>> {
        self.raw_entries()
            .map_addr(|addr| addr & !Self::STORAGE_TAG_MASK)
    }

    #[inline]
    fn encode_entries_ptr(
        entries: *mut Option<ResolverBucketEntry<'cfg>>,
        reclaim_delta: usize,
    ) -> *mut Option<ResolverBucketEntry<'cfg>> {
        debug_assert_eq!(entries.addr() & Self::STORAGE_TAG_MASK, 0);
        debug_assert!(reclaim_delta <= Self::STORAGE_TAG_MASK);
        entries.map_addr(|addr| addr | reclaim_delta)
    }

    #[inline]
    fn storage_ptr(&self) -> *mut u8 {
        self.entries_ptr().cast::<u8>()
    }

    #[inline]
    fn storage_reclaim_delta(&self) -> usize {
        self.raw_entries().addr() & Self::STORAGE_TAG_MASK
    }

    #[inline]
    fn storage_len(&self) -> usize {
        Self::storage_bytes(self.capacity)
    }

    #[inline]
    fn capacity(&self) -> usize {
        self.capacity
    }

    fn occupied_len(&self) -> usize {
        let entries = self.entries_ptr();
        if entries.is_null() {
            return 0;
        }
        let mut idx = 0usize;
        let mut occupied = 0usize;
        while idx < self.capacity {
            unsafe {
                if (*entries.add(idx)).is_some() {
                    occupied += 1;
                }
            }
            idx += 1;
        }
        occupied
    }

    unsafe fn bind_from_storage(
        &mut self,
        storage: *mut u8,
        capacity: usize,
        reclaim_delta: usize,
    ) {
        let entries = storage.cast::<Option<ResolverBucketEntry<'cfg>>>();
        let mut idx = 0usize;
        while idx < capacity {
            unsafe {
                entries.add(idx).write(None);
            }
            idx += 1;
        }
        *self.entries.get_mut() = Self::encode_entries_ptr(entries, reclaim_delta);
        self.capacity = capacity;
    }

    unsafe fn rebind_from_storage(
        &mut self,
        storage: *mut u8,
        new_capacity: usize,
        reclaim_delta: usize,
    ) {
        let old_entries = self.entries_ptr();
        let old_capacity = self.capacity;
        let new_entries = storage.cast::<Option<ResolverBucketEntry<'cfg>>>();
        let mut idx = 0usize;
        while idx < new_capacity {
            unsafe {
                new_entries.add(idx).write(None);
            }
            idx += 1;
        }

        if !old_entries.is_null() {
            let mut next = 0usize;
            let mut old_idx = 0usize;
            while old_idx < old_capacity {
                unsafe {
                    if let Some(entry) = (*old_entries.add(old_idx)).take() {
                        debug_assert!(next < new_capacity, "resolver bucket rebind overflow");
                        new_entries.add(next).write(Some(entry));
                        next += 1;
                    }
                }
                old_idx += 1;
            }
        }

        *self.entries.get_mut() = Self::encode_entries_ptr(new_entries, reclaim_delta);
        self.capacity = new_capacity;
    }

    fn insert(
        &mut self,
        eff_index: EffIndex,
        op: ControlOp,
        entry: DynamicResolverEntry<'cfg>,
    ) -> Result<(), CpError> {
        let entries = self.entries_ptr();
        if entries.is_null() {
            return Err(CpError::ResourceExhausted);
        }
        let mut first_empty = None;
        let mut idx = 0usize;
        while idx < self.capacity {
            unsafe {
                let slot = &mut *entries.add(idx);
                match slot {
                    Some(stored) if stored.eff_index == eff_index && stored.op == op => {
                        stored.entry = entry;
                        return Ok(());
                    }
                    None if first_empty.is_none() => first_empty = Some(idx),
                    _ => {}
                }
            }
            idx += 1;
        }
        let Some(idx) = first_empty else {
            return Err(CpError::ResourceExhausted);
        };
        unsafe {
            *entries.add(idx) = Some(ResolverBucketEntry {
                eff_index,
                op,
                entry,
            });
        }
        Ok(())
    }

    fn get(&self, eff_index: EffIndex, op: ControlOp) -> Option<&DynamicResolverEntry<'cfg>> {
        let entries = self.entries_ptr();
        if entries.is_null() {
            return None;
        }
        let mut idx = 0usize;
        while idx < self.capacity {
            unsafe {
                if let Some(stored) = (&*entries.add(idx)).as_ref()
                    && stored.eff_index == eff_index
                    && stored.op == op
                {
                    return Some(&stored.entry);
                }
            }
            idx += 1;
        }
        None
    }
}

#[cfg(test)]
const TEST_TRANSIENT_GRAPH_SCRATCH_BYTES: usize = 16_384;

#[cfg(test)]
thread_local! {
    static TEST_TRANSIENT_GRAPH_SCRATCH: UnsafeCell<[u8; TEST_TRANSIENT_GRAPH_SCRATCH_BYTES]> =
        const { UnsafeCell::new([0; TEST_TRANSIENT_GRAPH_SCRATCH_BYTES]) };
}

const fn is_dynamic_control_op(op: ControlOp) -> bool {
    matches!(
        op,
        ControlOp::LoopContinue | ControlOp::LoopBreak | ControlOp::RouteDecision
    )
}

/// Trait implemented by local Rendezvous instances that can apply control-plane effects.
pub(crate) trait EffectRunner {
    fn run_effect(&mut self, envelope: CpCommand) -> Result<(), CpError>;
}

enum DistributedPhase {
    Begin {
        txn: Option<InBegin<DistributedTopologyInv, crate::control::types::One>>,
    },
    Acked {
        txn: InAcked<DistributedTopologyInv, crate::control::types::One>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DistributedPhaseKind {
    Begin,
    Acked,
}

struct DistributedEntry {
    operands: TopologyOperands,
    phase: DistributedPhase,
}

struct DistributedTopologyBucketEntry {
    sid: SessionId,
    entry: DistributedEntry,
}

#[derive(Clone, Copy)]
struct DistributedTopologyBucket {
    entries: *mut Option<DistributedTopologyBucketEntry>,
    capacity: usize,
    _no_send_sync: PhantomData<*mut ()>,
}

impl DistributedTopologyBucket {
    const STORAGE_TAG_MASK: usize = Self::storage_align().saturating_sub(1);

    const fn empty() -> Self {
        Self {
            entries: core::ptr::null_mut(),
            capacity: 0,
            _no_send_sync: PhantomData,
        }
    }

    unsafe fn init_empty(dst: *mut Self) {
        unsafe {
            core::ptr::addr_of_mut!((*dst).entries).write(core::ptr::null_mut());
            core::ptr::addr_of_mut!((*dst).capacity).write(0);
            core::ptr::addr_of_mut!((*dst)._no_send_sync).write(PhantomData);
        }
    }

    #[inline]
    const fn storage_align() -> usize {
        core::mem::align_of::<Option<DistributedTopologyBucketEntry>>()
    }

    #[inline]
    const fn storage_bytes(capacity: usize) -> usize {
        capacity.saturating_mul(core::mem::size_of::<Option<DistributedTopologyBucketEntry>>())
    }

    #[inline]
    fn raw_entries(&self) -> *mut Option<DistributedTopologyBucketEntry> {
        self.entries
    }

    #[inline]
    fn entries_ptr(&self) -> *mut Option<DistributedTopologyBucketEntry> {
        self.raw_entries()
            .map_addr(|addr| addr & !Self::STORAGE_TAG_MASK)
    }

    #[inline]
    fn encode_entries_ptr(
        entries: *mut Option<DistributedTopologyBucketEntry>,
        reclaim_delta: usize,
    ) -> *mut Option<DistributedTopologyBucketEntry> {
        debug_assert_eq!(entries.addr() & Self::STORAGE_TAG_MASK, 0);
        debug_assert!(reclaim_delta <= Self::STORAGE_TAG_MASK);
        entries.map_addr(|addr| addr | reclaim_delta)
    }

    #[inline]
    fn storage_ptr(&self) -> *mut u8 {
        self.entries_ptr().cast::<u8>()
    }

    #[inline]
    fn storage_reclaim_delta(&self) -> usize {
        self.raw_entries().addr() & Self::STORAGE_TAG_MASK
    }

    #[inline]
    fn storage_len(&self) -> usize {
        Self::storage_bytes(self.capacity)
    }

    #[inline]
    fn capacity(&self) -> usize {
        self.capacity
    }

    fn occupied_len(&self) -> usize {
        let entries = self.entries_ptr();
        if entries.is_null() {
            return 0;
        }
        let mut idx = 0usize;
        let mut occupied = 0usize;
        while idx < self.capacity {
            unsafe {
                if (*entries.add(idx)).is_some() {
                    occupied += 1;
                }
            }
            idx += 1;
        }
        occupied
    }

    unsafe fn bind_from_storage(
        &mut self,
        storage: *mut u8,
        capacity: usize,
        reclaim_delta: usize,
    ) {
        let entries = storage.cast::<Option<DistributedTopologyBucketEntry>>();
        let mut idx = 0usize;
        while idx < capacity {
            unsafe {
                entries.add(idx).write(None);
            }
            idx += 1;
        }
        self.entries = Self::encode_entries_ptr(entries, reclaim_delta);
        self.capacity = capacity;
    }

    unsafe fn rebind_from_storage(
        &mut self,
        storage: *mut u8,
        new_capacity: usize,
        reclaim_delta: usize,
    ) {
        let old_entries = self.entries_ptr();
        let old_capacity = self.capacity;
        let new_entries = storage.cast::<Option<DistributedTopologyBucketEntry>>();
        let mut idx = 0usize;
        while idx < new_capacity {
            unsafe {
                new_entries.add(idx).write(None);
            }
            idx += 1;
        }

        if !old_entries.is_null() {
            let mut next = 0usize;
            let mut old_idx = 0usize;
            while old_idx < old_capacity {
                unsafe {
                    if let Some(entry) = (*old_entries.add(old_idx)).take() {
                        debug_assert!(next < new_capacity, "distributed topology rebind overflow");
                        new_entries.add(next).write(Some(entry));
                        next += 1;
                    }
                }
                old_idx += 1;
            }
        }

        self.entries = Self::encode_entries_ptr(new_entries, reclaim_delta);
        self.capacity = new_capacity;
    }

    fn contains_sid(&self, sid: SessionId) -> bool {
        let entries = self.entries_ptr();
        if entries.is_null() {
            return false;
        }
        let mut idx = 0usize;
        while idx < self.capacity {
            unsafe {
                if let Some(stored) = (&*entries.add(idx)).as_ref()
                    && stored.sid == sid
                {
                    return true;
                }
            }
            idx += 1;
        }
        false
    }

    fn insert(&mut self, sid: SessionId, entry: DistributedEntry) -> Result<(), CpError> {
        let entries = self.entries_ptr();
        if entries.is_null() {
            return Err(CpError::ResourceExhausted);
        }
        let mut first_empty = None;
        let mut idx = 0usize;
        while idx < self.capacity {
            unsafe {
                let slot = &mut *entries.add(idx);
                match slot {
                    Some(stored) if stored.sid == sid => {
                        return Err(CpError::ReplayDetected {
                            operation: ControlOp::TopologyBegin as u8,
                            nonce: sid.raw(),
                        });
                    }
                    None if first_empty.is_none() => first_empty = Some(idx),
                    _ => {}
                }
            }
            idx += 1;
        }
        let Some(idx) = first_empty else {
            return Err(CpError::ResourceExhausted);
        };
        unsafe {
            *entries.add(idx) = Some(DistributedTopologyBucketEntry { sid, entry });
        }
        Ok(())
    }

    fn get(&self, sid: SessionId) -> Option<&DistributedEntry> {
        let entries = self.entries_ptr();
        if entries.is_null() {
            return None;
        }
        let mut idx = 0usize;
        while idx < self.capacity {
            unsafe {
                if let Some(stored) = (&*entries.add(idx)).as_ref()
                    && stored.sid == sid
                {
                    return Some(&stored.entry);
                }
            }
            idx += 1;
        }
        None
    }

    fn get_mut(&mut self, sid: SessionId) -> Option<&mut DistributedEntry> {
        let entries = self.entries_ptr();
        if entries.is_null() {
            return None;
        }
        let mut idx = 0usize;
        while idx < self.capacity {
            unsafe {
                if let Some(stored) = (&mut *entries.add(idx)).as_mut()
                    && stored.sid == sid
                {
                    return Some(&mut stored.entry);
                }
            }
            idx += 1;
        }
        None
    }

    fn remove(&mut self, sid: SessionId) -> Option<DistributedEntry> {
        let entries = self.entries_ptr();
        if entries.is_null() {
            return None;
        }
        let mut idx = 0usize;
        while idx < self.capacity {
            unsafe {
                let slot = &mut *entries.add(idx);
                if slot.as_ref().is_some_and(|stored| stored.sid == sid) {
                    return slot.take().map(|stored| stored.entry);
                }
            }
            idx += 1;
        }
        None
    }
}

/// Distributed topology state tracking.
///
/// Tracks in-flight distributed topology operations to ensure exactly-once semantics.
pub(crate) struct DistributedTopologyState<const MAX: usize> {
    buckets: [DistributedTopologyBucket; MAX],
}

impl<const MAX: usize> Default for DistributedTopologyState<MAX> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const MAX: usize> DistributedTopologyState<MAX> {
    /// Create a new empty state.
    pub(crate) const fn new() -> Self {
        Self {
            buckets: [DistributedTopologyBucket::empty(); MAX],
        }
    }

    unsafe fn init_empty(dst: *mut Self) {
        unsafe {
            let mut slot = 0usize;
            while slot < MAX {
                DistributedTopologyBucket::init_empty(core::ptr::addr_of_mut!(
                    (*dst).buckets[slot]
                ));
                slot += 1;
            }
        }
    }

    fn bucket(&self, rv_id: RendezvousId) -> Option<&DistributedTopologyBucket> {
        let slot = cluster_rendezvous_slot::<MAX>(rv_id)?;
        Some(&self.buckets[slot])
    }

    fn bucket_mut(&mut self, rv_id: RendezvousId) -> Option<&mut DistributedTopologyBucket> {
        let slot = cluster_rendezvous_slot::<MAX>(rv_id)?;
        Some(&mut self.buckets[slot])
    }

    fn contains_sid(&self, sid: SessionId) -> bool {
        let mut slot = 0usize;
        while slot < MAX {
            if self.buckets[slot].contains_sid(sid) {
                return true;
            }
            slot += 1;
        }
        false
    }

    fn phase(&self, sid: SessionId) -> Option<DistributedPhaseKind> {
        let mut slot = 0usize;
        while slot < MAX {
            if let Some(entry) = self.buckets[slot].get(sid) {
                return Some(match &entry.phase {
                    DistributedPhase::Begin { .. } => DistributedPhaseKind::Begin,
                    DistributedPhase::Acked { .. } => DistributedPhaseKind::Acked,
                });
            }
            slot += 1;
        }
        None
    }

    fn ensure_capacity<FA, FF>(
        &mut self,
        rv_id: RendezvousId,
        additional_entries: usize,
        allocate: FA,
        free: FF,
    ) -> Result<(), CpError>
    where
        FA: FnOnce(usize, usize) -> Option<(*mut u8, usize)>,
        FF: FnOnce(*mut u8, usize, usize),
    {
        if additional_entries == 0 {
            return Ok(());
        }
        let bucket = self.bucket_mut(rv_id).ok_or(CpError::RendezvousMismatch {
            expected: rv_id.raw(),
            actual: 0,
        })?;
        let required = bucket
            .occupied_len()
            .checked_add(additional_entries)
            .ok_or(CpError::ResourceExhausted)?;
        if bucket.capacity() >= required {
            return Ok(());
        }

        let old_ptr = bucket.storage_ptr();
        let old_len = bucket.storage_len();
        let old_reclaim_delta = bucket.storage_reclaim_delta();
        let (storage, reclaim_delta) = allocate(
            DistributedTopologyBucket::storage_bytes(required),
            DistributedTopologyBucket::storage_align(),
        )
        .ok_or(CpError::ResourceExhausted)?;
        unsafe {
            if old_ptr.is_null() {
                bucket.bind_from_storage(storage, required, reclaim_delta);
            } else {
                bucket.rebind_from_storage(storage, required, reclaim_delta);
                free(old_ptr, old_len, old_reclaim_delta);
            }
        }
        Ok(())
    }

    fn preflight_ack(
        &self,
        sid: SessionId,
        src_rv: RendezvousId,
        expected: TopologyAck,
    ) -> Result<(), CpError> {
        let entry = self
            .bucket(src_rv)
            .and_then(|bucket| bucket.get(sid))
            .ok_or(CpError::Topology(TopologyError::InvalidSession))?;

        match &entry.phase {
            DistributedPhase::Begin { txn } => {
                if txn.is_none() {
                    return Err(CpError::ReplayDetected {
                        operation: ControlOp::TopologyAck as u8,
                        nonce: sid.raw(),
                    });
                }
            }
            DistributedPhase::Acked { .. } => {
                return Err(CpError::ReplayDetected {
                    operation: ControlOp::TopologyAck as u8,
                    nonce: sid.raw(),
                });
            }
        }

        if entry.operands.ack(sid) != expected {
            return Err(CpError::Topology(TopologyError::GenerationMismatch));
        }

        Ok(())
    }

    fn preflight_commit(
        &self,
        sid: SessionId,
        src_rv: RendezvousId,
        expected: Option<TopologyAck>,
    ) -> Result<(), CpError> {
        let entry = self
            .bucket(src_rv)
            .and_then(|bucket| bucket.get(sid))
            .ok_or(CpError::Topology(TopologyError::InvalidSession))?;

        match &entry.phase {
            DistributedPhase::Acked { .. } => {}
            DistributedPhase::Begin { .. } => {
                return Err(CpError::Topology(TopologyError::InvalidState));
            }
        }

        if let Some(exp) = expected
            && entry.operands.ack(sid) != exp
        {
            return Err(CpError::Topology(TopologyError::CommitFailed));
        }

        Ok(())
    }

    pub(crate) fn begin(
        &mut self,
        sid: SessionId,
        operands: TopologyOperands,
    ) -> Result<(TopologyIntent, TopologyAck), CpError> {
        if self.contains_sid(sid) {
            return Err(CpError::ReplayDetected {
                operation: ControlOp::TopologyBegin as u8,
                nonce: sid.raw(),
            });
        }

        let mut tap = NoopTap;
        let (in_begin, intent) = DistributedTopology::begin(
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
        self.bucket_mut(operands.src_rv)
            .ok_or(CpError::RendezvousMismatch {
                expected: operands.src_rv.raw(),
                actual: 0,
            })?
            .insert(sid, entry)?;

        Ok((intent, operands.ack(sid)))
    }

    pub(crate) fn acknowledge(
        &mut self,
        sid: SessionId,
        src_rv: RendezvousId,
    ) -> Result<TopologyAck, CpError> {
        let entry = self
            .bucket_mut(src_rv)
            .and_then(|bucket| bucket.get_mut(sid))
            .ok_or(CpError::Topology(TopologyError::InvalidSession))?;

        let txn = match &mut entry.phase {
            DistributedPhase::Begin { txn } => txn.take().ok_or(CpError::ReplayDetected {
                operation: ControlOp::TopologyAck as u8,
                nonce: sid.raw(),
            })?,
            DistributedPhase::Acked { .. } => {
                return Err(CpError::ReplayDetected {
                    operation: ControlOp::TopologyAck as u8,
                    nonce: sid.raw(),
                });
            }
        };

        let mut tap = NoopTap;
        let in_acked = DistributedTopology::acknowledge(txn, &mut tap);
        let ack = entry.operands.ack(sid);
        entry.phase = DistributedPhase::Acked { txn: in_acked };

        Ok(ack)
    }

    pub(crate) fn topology_commit(
        &mut self,
        sid: SessionId,
        src_rv: RendezvousId,
        expected: Option<TopologyAck>,
    ) -> Result<TopologyOperands, CpError> {
        self.preflight_commit(sid, src_rv, expected)?;
        let entry = self
            .bucket_mut(src_rv)
            .and_then(|bucket| bucket.remove(sid))
            .ok_or(CpError::Topology(TopologyError::InvalidSession))?;

        let DistributedEntry { operands, phase } = entry;

        match phase {
            DistributedPhase::Acked { txn } => {
                let mut tap = NoopTap;
                let _closed = DistributedTopology::topology_commit(txn, &mut tap);
                Ok(operands)
            }
            DistributedPhase::Begin { .. } => unreachable!(
                "topology commit preflight guarantees an acked distributed entry before removal"
            ),
        }
    }

    pub(crate) fn abort(
        &mut self,
        sid: SessionId,
        src_rv: RendezvousId,
    ) -> Result<TopologyOperands, CpError> {
        let entry = self
            .bucket_mut(src_rv)
            .and_then(|bucket| bucket.remove(sid))
            .ok_or(CpError::Topology(TopologyError::InvalidSession))?;
        Ok(entry.operands)
    }

    pub(crate) fn get(&self, sid: SessionId) -> Option<&TopologyOperands> {
        let mut slot = 0usize;
        while slot < MAX {
            if let Some(entry) = self.buckets[slot].get(sid) {
                return Some(&entry.operands);
            }
            slot += 1;
        }
        None
    }
}

#[derive(Clone, Copy)]
struct CachedTopologyBucketEntry {
    sid: SessionId,
    operands: TopologyOperands,
}

#[derive(Clone, Copy)]
struct CachedTopologyBucket {
    entries: *mut Option<CachedTopologyBucketEntry>,
    capacity: usize,
    _no_send_sync: PhantomData<*mut ()>,
}

impl CachedTopologyBucket {
    const STORAGE_TAG_MASK: usize = Self::storage_align().saturating_sub(1);

    unsafe fn init_empty(dst: *mut Self) {
        unsafe {
            core::ptr::addr_of_mut!((*dst).entries).write(core::ptr::null_mut());
            core::ptr::addr_of_mut!((*dst).capacity).write(0);
            core::ptr::addr_of_mut!((*dst)._no_send_sync).write(PhantomData);
        }
    }

    #[inline]
    const fn storage_align() -> usize {
        core::mem::align_of::<Option<CachedTopologyBucketEntry>>()
    }

    #[cfg(test)]
    #[inline]
    const fn storage_bytes(capacity: usize) -> usize {
        capacity.saturating_mul(core::mem::size_of::<Option<CachedTopologyBucketEntry>>())
    }

    #[inline]
    fn raw_entries(&self) -> *mut Option<CachedTopologyBucketEntry> {
        self.entries
    }

    #[inline]
    fn entries_ptr(&self) -> *mut Option<CachedTopologyBucketEntry> {
        self.raw_entries()
            .map_addr(|addr| addr & !Self::STORAGE_TAG_MASK)
    }

    #[cfg(test)]
    #[inline]
    fn encode_entries_ptr(
        entries: *mut Option<CachedTopologyBucketEntry>,
        reclaim_delta: usize,
    ) -> *mut Option<CachedTopologyBucketEntry> {
        debug_assert_eq!(entries.addr() & Self::STORAGE_TAG_MASK, 0);
        debug_assert!(reclaim_delta <= Self::STORAGE_TAG_MASK);
        entries.map_addr(|addr| addr | reclaim_delta)
    }

    #[cfg(test)]
    #[inline]
    fn storage_ptr(&self) -> *mut u8 {
        self.entries_ptr().cast::<u8>()
    }

    #[cfg(test)]
    #[inline]
    fn storage_reclaim_delta(&self) -> usize {
        self.raw_entries().addr() & Self::STORAGE_TAG_MASK
    }

    #[cfg(test)]
    #[inline]
    fn storage_len(&self) -> usize {
        Self::storage_bytes(self.capacity)
    }

    #[cfg(test)]
    #[inline]
    fn capacity(&self) -> usize {
        self.capacity
    }

    #[cfg(test)]
    fn occupied_len(&self) -> usize {
        let entries = self.entries_ptr();
        if entries.is_null() {
            return 0;
        }
        let mut idx = 0usize;
        let mut occupied = 0usize;
        while idx < self.capacity {
            unsafe {
                if (*entries.add(idx)).is_some() {
                    occupied += 1;
                }
            }
            idx += 1;
        }
        occupied
    }

    #[cfg(test)]
    unsafe fn bind_from_storage(
        &mut self,
        storage: *mut u8,
        capacity: usize,
        reclaim_delta: usize,
    ) {
        let entries = storage.cast::<Option<CachedTopologyBucketEntry>>();
        let mut idx = 0usize;
        while idx < capacity {
            unsafe {
                entries.add(idx).write(None);
            }
            idx += 1;
        }
        self.entries = Self::encode_entries_ptr(entries, reclaim_delta);
        self.capacity = capacity;
    }

    #[cfg(test)]
    unsafe fn rebind_from_storage(
        &mut self,
        storage: *mut u8,
        new_capacity: usize,
        reclaim_delta: usize,
    ) {
        let old_entries = self.entries_ptr();
        let old_capacity = self.capacity;
        let new_entries = storage.cast::<Option<CachedTopologyBucketEntry>>();
        let mut idx = 0usize;
        while idx < new_capacity {
            unsafe {
                new_entries.add(idx).write(None);
            }
            idx += 1;
        }

        if !old_entries.is_null() {
            let mut next = 0usize;
            let mut old_idx = 0usize;
            while old_idx < old_capacity {
                unsafe {
                    if let Some(entry) = (*old_entries.add(old_idx)).take() {
                        debug_assert!(
                            next < new_capacity,
                            "cached topology bucket rebind overflow"
                        );
                        new_entries.add(next).write(Some(entry));
                        next += 1;
                    }
                }
                old_idx += 1;
            }
        }

        self.entries = Self::encode_entries_ptr(new_entries, reclaim_delta);
        self.capacity = new_capacity;
    }

    #[cfg(test)]
    fn contains_sid(&self, sid: SessionId) -> bool {
        let entries = self.entries_ptr();
        if entries.is_null() {
            return false;
        }
        let mut idx = 0usize;
        while idx < self.capacity {
            unsafe {
                if let Some(stored) = (&*entries.add(idx)).as_ref()
                    && stored.sid == sid
                {
                    return true;
                }
            }
            idx += 1;
        }
        false
    }

    fn get(&self, sid: SessionId) -> Option<&TopologyOperands> {
        let entries = self.entries_ptr();
        if entries.is_null() {
            return None;
        }
        let mut idx = 0usize;
        while idx < self.capacity {
            unsafe {
                if let Some(stored) = (&*entries.add(idx)).as_ref()
                    && stored.sid == sid
                {
                    return Some(&stored.operands);
                }
            }
            idx += 1;
        }
        None
    }

    #[cfg(test)]
    fn insert(&mut self, sid: SessionId, operands: TopologyOperands) -> Result<(), CpError> {
        let entries = self.entries_ptr();
        if entries.is_null() {
            return Err(CpError::ResourceExhausted);
        }
        let mut first_empty = None;
        let mut idx = 0usize;
        while idx < self.capacity {
            unsafe {
                let slot = &mut *entries.add(idx);
                match slot {
                    Some(stored) if stored.sid == sid => {
                        stored.operands = operands;
                        return Ok(());
                    }
                    None if first_empty.is_none() => first_empty = Some(idx),
                    _ => {}
                }
            }
            idx += 1;
        }
        let Some(idx) = first_empty else {
            return Err(CpError::ResourceExhausted);
        };
        unsafe {
            entries
                .add(idx)
                .write(Some(CachedTopologyBucketEntry { sid, operands }));
        }
        Ok(())
    }

    fn remove(&mut self, sid: SessionId) -> Option<TopologyOperands> {
        let entries = self.entries_ptr();
        if entries.is_null() {
            return None;
        }
        let mut idx = 0usize;
        while idx < self.capacity {
            unsafe {
                if let Some(stored) = (&mut *entries.add(idx)).take() {
                    if stored.sid == sid {
                        return Some(stored.operands);
                    }
                    entries.add(idx).write(Some(stored));
                }
            }
            idx += 1;
        }
        None
    }
}

/// SessionCluster - Coordinates multiple Rendezvous instances.
///
/// This is the top-level local control-plane coordinator. It manages:
/// - Local Rendezvous instances
/// - Distributed topology coordination across registered local rendezvous
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
/// let clock = CounterClock::new();
/// let mut cluster: SessionCluster<8> = SessionCluster::new(&clock);
///
/// // Register local Rendezvous from runtime config + transport
/// cluster.add_rendezvous_from_config(config, transport)?;
///
/// // Perform distributed topology
/// cluster.distributed_topology(
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
/// 4. **Topology state consistency**: distributed topology operations must maintain Begin→Ack→Commit ordering
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

    /// Distributed topology state tracking.
    topology_state: DistributedTopologyState<MAX_RV>,

    /// Cached operands staged between minting intent and ack tokens.
    cached_operands: [CachedTopologyBucket; MAX_RV],

    /// Number of active lane leases (affine witness count).
    active_leases: core::cell::Cell<u32>,
}

impl<'cfg, T, U, C, E, const MAX_RV: usize> ControlCore<'cfg, T, U, C, E, MAX_RV>
where
    T: crate::transport::Transport,
    U: crate::runtime::consts::LabelUniverse,
    C: crate::runtime::config::Clock,
    E: crate::control::cap::mint::EpochTable,
{
    unsafe fn init_empty(dst: *mut Self) {
        unsafe {
            crate::control::lease::core::ControlCore::init_empty(core::ptr::addr_of_mut!(
                (*dst).locals
            ));
            DistributedTopologyState::init_empty(core::ptr::addr_of_mut!((*dst).topology_state));
            core::ptr::addr_of_mut!((*dst).active_leases).write(core::cell::Cell::new(0));
            let mut slot = 0usize;
            while slot < MAX_RV {
                CachedTopologyBucket::init_empty(core::ptr::addr_of_mut!(
                    (*dst).cached_operands[slot]
                ));
                slot += 1;
            }
        }
    }

    #[cfg(test)]
    #[inline]
    fn cached_operands_slot(rv_id: RendezvousId) -> Option<usize> {
        cluster_rendezvous_slot::<MAX_RV>(rv_id)
    }

    fn cached_operands_get(&self, sid: SessionId) -> Option<&TopologyOperands> {
        let mut slot = 0usize;
        while slot < MAX_RV {
            if let Some(operands) = self.cached_operands[slot].get(sid) {
                return Some(operands);
            }
            slot += 1;
        }
        None
    }

    #[cfg(test)]
    fn cached_operands_remove_other_shards(&mut self, sid: SessionId, keep_slot: usize) {
        let mut slot = 0usize;
        while slot < MAX_RV {
            if slot != keep_slot {
                self.cached_operands[slot].remove(sid);
            }
            slot += 1;
        }
    }

    #[cfg(test)]
    fn cached_operands_insert(
        &mut self,
        sid: SessionId,
        operands: TopologyOperands,
    ) -> Result<(), CpError> {
        let target_slot =
            Self::cached_operands_slot(operands.src_rv).ok_or(CpError::RendezvousMismatch {
                expected: operands.src_rv.raw(),
                actual: 0,
            })?;
        if !self.locals.is_registered(&operands.src_rv) {
            return Err(CpError::RendezvousMismatch {
                expected: operands.src_rv.raw(),
                actual: 0,
            });
        }
        let additional_entries = usize::from(!self.cached_operands[target_slot].contains_sid(sid));
        self.ensure_cached_operands_capacity(operands.src_rv, additional_entries)?;
        self.cached_operands_remove_other_shards(sid, target_slot);
        self.cached_operands[target_slot].insert(sid, operands)
    }

    fn cached_operands_remove(&mut self, sid: SessionId) -> Option<TopologyOperands> {
        let mut slot = 0usize;
        while slot < MAX_RV {
            if let Some(operands) = self.cached_operands[slot].remove(sid) {
                return Some(operands);
            }
            slot += 1;
        }
        None
    }

    #[cfg(test)]
    fn ensure_cached_operands_capacity(
        &mut self,
        rv_id: RendezvousId,
        additional_entries: usize,
    ) -> Result<(), CpError> {
        if additional_entries == 0 {
            return Ok(());
        }
        let slot = Self::cached_operands_slot(rv_id).ok_or(CpError::RendezvousMismatch {
            expected: rv_id.raw(),
            actual: 0,
        })?;
        if !self.locals.is_registered(&rv_id) {
            return Err(CpError::RendezvousMismatch {
                expected: rv_id.raw(),
                actual: 0,
            });
        }
        let bucket_ptr = core::ptr::addr_of_mut!(self.cached_operands[slot]);
        let bucket = unsafe { &mut *bucket_ptr };
        let required = bucket
            .occupied_len()
            .checked_add(additional_entries)
            .ok_or(CpError::ResourceExhausted)?;
        if bucket.capacity() >= required {
            return Ok(());
        }

        let rv = self
            .locals
            .get_mut(&rv_id)
            .ok_or(CpError::RendezvousMismatch {
                expected: rv_id.raw(),
                actual: 0,
            })?;
        let rv_ptr = core::ptr::from_mut(rv);
        let old_ptr = bucket.storage_ptr();
        let old_len = bucket.storage_len();
        let old_reclaim_delta = bucket.storage_reclaim_delta();
        let (storage, reclaim_delta) = unsafe {
            (&mut *rv_ptr).allocate_external_persistent_sidecar_bytes(
                CachedTopologyBucket::storage_bytes(required),
                CachedTopologyBucket::storage_align(),
            )
        }
        .ok_or(CpError::ResourceExhausted)?;
        unsafe {
            if old_ptr.is_null() {
                bucket.bind_from_storage(storage, required, reclaim_delta);
            } else {
                bucket.rebind_from_storage(storage, required, reclaim_delta);
                (&mut *rv_ptr).free_external_persistent_sidecar_bytes(
                    old_ptr,
                    old_len,
                    old_reclaim_delta,
                );
            }
        }
        Ok(())
    }

    fn ensure_distributed_topology_capacity(
        &mut self,
        rv_id: RendezvousId,
        additional_entries: usize,
    ) -> Result<(), CpError> {
        if additional_entries == 0 {
            return Ok(());
        }
        let rv = self
            .locals
            .get_mut(&rv_id)
            .ok_or(CpError::RendezvousMismatch {
                expected: rv_id.raw(),
                actual: 0,
            })?;
        let rv_ptr = core::ptr::from_mut(rv);
        self.topology_state.ensure_capacity(
            rv_id,
            additional_entries,
            |bytes, align| unsafe {
                (&mut *rv_ptr).allocate_external_persistent_sidecar_bytes(bytes, align)
            },
            |ptr, bytes, reclaim_delta| unsafe {
                (&mut *rv_ptr).free_external_persistent_sidecar_bytes(ptr, bytes, reclaim_delta)
            },
        )
    }
}

struct ResolverCore<'cfg, const MAX_RV: usize> {
    buckets: [ResolverBucket<'cfg>; MAX_RV],
}

impl<'cfg, const MAX_RV: usize> ResolverCore<'cfg, MAX_RV> {
    unsafe fn init_empty(dst: *mut Self) {
        unsafe {
            let mut slot = 0usize;
            while slot < MAX_RV {
                ResolverBucket::init_empty(core::ptr::addr_of_mut!((*dst).buckets[slot]));
                slot += 1;
            }
        }
    }

    fn bucket(&self, rv_id: RendezvousId) -> Option<&ResolverBucket<'cfg>> {
        let slot = cluster_rendezvous_slot::<MAX_RV>(rv_id)?;
        Some(&self.buckets[slot])
    }

    fn bucket_mut(&mut self, rv_id: RendezvousId) -> Option<&mut ResolverBucket<'cfg>> {
        let slot = cluster_rendezvous_slot::<MAX_RV>(rv_id)?;
        Some(&mut self.buckets[slot])
    }

    fn ensure_capacity<FA, FF>(
        &mut self,
        rv_id: RendezvousId,
        additional_entries: usize,
        allocate: FA,
        free: FF,
    ) -> Result<(), CpError>
    where
        FA: FnOnce(usize, usize) -> Option<(*mut u8, usize)>,
        FF: FnOnce(*mut u8, usize, usize),
    {
        if additional_entries == 0 {
            return Ok(());
        }
        let bucket = self.bucket_mut(rv_id).ok_or(CpError::RendezvousMismatch {
            expected: rv_id.raw(),
            actual: 0,
        })?;
        let required = bucket
            .occupied_len()
            .checked_add(additional_entries)
            .ok_or(CpError::ResourceExhausted)?;
        if bucket.capacity() >= required {
            return Ok(());
        }

        let old_ptr = bucket.storage_ptr();
        let old_len = bucket.storage_len();
        let old_reclaim_delta = bucket.storage_reclaim_delta();
        let (storage, reclaim_delta) = allocate(
            ResolverBucket::storage_bytes(required),
            ResolverBucket::storage_align(),
        )
        .ok_or(CpError::ResourceExhausted)?;
        unsafe {
            if old_ptr.is_null() {
                bucket.bind_from_storage(storage, required, reclaim_delta);
            } else {
                bucket.rebind_from_storage(storage, required, reclaim_delta);
                free(old_ptr, old_len, old_reclaim_delta);
            }
        }
        Ok(())
    }

    fn insert(
        &mut self,
        key: DynamicResolverKey,
        entry: DynamicResolverEntry<'cfg>,
    ) -> Result<(), CpError> {
        self.bucket_mut(key.rv)
            .ok_or(CpError::RendezvousMismatch {
                expected: key.rv.raw(),
                actual: 0,
            })?
            .insert(key.eff_index, key.op, entry)
    }

    fn get(&self, key: DynamicResolverKey) -> Option<&DynamicResolverEntry<'cfg>> {
        self.bucket(key.rv)?.get(key.eff_index, key.op)
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
    control: core::cell::UnsafeCell<
        ControlCore<'cfg, T, U, C, crate::control::cap::mint::EpochTbl, MAX_RV>,
    >,
    /// Dynamic resolver table separated from core control state.
    resolvers: core::cell::UnsafeCell<ResolverCore<'cfg, MAX_RV>>,
    /// Clock for timestamping tap events.
    clock: &'cfg C,
    _local_only: crate::local::LocalOnly,
}

impl<'cfg, T, U, C, const MAX_RV: usize> SessionCluster<'cfg, T, U, C, MAX_RV>
where
    T: crate::transport::Transport + 'cfg,
    U: crate::runtime::consts::LabelUniverse + 'cfg,
    C: crate::runtime::config::Clock + 'cfg,
{
    pub(crate) unsafe fn init_empty(dst: *mut Self, clock: &'cfg C) {
        unsafe {
            ControlCore::<T, U, C, crate::control::cap::mint::EpochTbl, MAX_RV>::init_empty(
                core::ptr::addr_of_mut!((*dst).control).cast(),
            );
            ResolverCore::<'cfg, MAX_RV>::init_empty(
                core::ptr::addr_of_mut!((*dst).resolvers).cast(),
            );
            core::ptr::addr_of_mut!((*dst).clock).write(clock);
            core::ptr::addr_of_mut!((*dst)._local_only).write(crate::local::LocalOnly::new());
        }
    }

    #[inline]
    fn control_ptr(
        &self,
    ) -> *mut ControlCore<'cfg, T, U, C, crate::control::cap::mint::EpochTbl, MAX_RV> {
        self.control.get()
    }

    #[inline]
    fn control_ref_ptr(
        &self,
    ) -> *const ControlCore<'cfg, T, U, C, crate::control::cap::mint::EpochTbl, MAX_RV> {
        self.control.get()
            as *const ControlCore<'cfg, T, U, C, crate::control::cap::mint::EpochTbl, MAX_RV>
    }

    #[inline]
    fn resolvers_ptr(&self) -> *mut ResolverCore<'cfg, MAX_RV> {
        self.resolvers.get()
    }

    #[inline]
    fn resolvers_ref_ptr(&self) -> *const ResolverCore<'cfg, MAX_RV> {
        self.resolvers.get() as *const ResolverCore<'cfg, MAX_RV>
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
        F: FnOnce(&mut ResolverCore<'cfg, MAX_RV>) -> R,
    {
        unsafe { f(&mut *self.resolvers_ptr()) }
    }

    unsafe fn transient_graph_storage_ptr<Spec>(
        core: &mut ControlCore<'cfg, T, U, C, crate::control::cap::mint::EpochTbl, MAX_RV>,
        rv_id: RendezvousId,
    ) -> Result<*mut LeaseGraph<'cfg, Spec>, LeaseGraphError>
    where
        Spec: LeaseSpec + 'cfg,
    {
        #[cfg(test)]
        {
            let mut test_graph = None;
            TEST_TRANSIENT_GRAPH_SCRATCH.with(|storage| {
                test_graph = unsafe {
                    Self::graph_storage_ptr_from_bytes::<Spec>(
                        (*storage.get()).as_mut_ptr(),
                        TEST_TRANSIENT_GRAPH_SCRATCH_BYTES,
                    )
                };
            });
            if let Some(graph) = test_graph {
                return Ok(graph);
            }
        }

        let rv = core
            .locals
            .get(&rv_id)
            .ok_or(LeaseGraphError::NodeNotFound)?;
        let (storage, len) = rv.scratch_storage_ptr_and_len();
        unsafe { Self::graph_storage_ptr_from_bytes::<Spec>(storage, len) }
            .ok_or(LeaseGraphError::GraphFull)
    }

    unsafe fn graph_storage_ptr_from_bytes<Spec>(
        storage: *mut u8,
        len: usize,
    ) -> Option<*mut LeaseGraph<'cfg, Spec>>
    where
        Spec: LeaseSpec + 'cfg,
    {
        let base = storage as usize;
        let align = core::mem::align_of::<LeaseGraph<'cfg, Spec>>();
        let bytes = core::mem::size_of::<LeaseGraph<'cfg, Spec>>();
        let aligned = Self::align_up(base, align);
        let offset = aligned.wrapping_sub(base);
        if offset + bytes > len {
            return None;
        }
        Some(aligned as *mut LeaseGraph<'cfg, Spec>)
    }

    #[inline(always)]
    const fn align_up(value: usize, align: usize) -> usize {
        let mask = align.saturating_sub(1);
        (value + mask) & !mask
    }

    #[inline]
    fn public_endpoint_storage_requirement<const ROLE: u8>(
        role_image: RoleImageSlice<ROLE>,
        binding_enabled: bool,
    ) -> PublicEndpointStorageLayout {
        let arena_layout = role_image.endpoint_arena_layout_for_binding(binding_enabled);
        let storage_layout = crate::endpoint::kernel::cursor_endpoint_storage_layout::<
            0,
            T,
            U,
            C,
            crate::control::cap::mint::EpochTbl,
            MAX_RV,
            crate::control::cap::mint::MintConfig,
            crate::binding::BindingHandle<'cfg>,
        >(&arena_layout, role_image.endpoint_lane_slot_count());
        PublicEndpointStorageLayout {
            total_bytes: storage_layout.total_bytes(),
            total_align: storage_layout.total_align(),
            header_bytes: storage_layout.header_bytes(),
            port_slots_bytes: storage_layout.port_slots_bytes(),
            guard_slots_bytes: storage_layout.guard_slots_bytes(),
            header_padding_bytes: storage_layout.arena_offset().saturating_sub(
                storage_layout.header_bytes()
                    + storage_layout.port_slots_bytes()
                    + storage_layout.guard_slots_bytes(),
            ),
            arena_offset: storage_layout.arena_offset(),
            arena_bytes: storage_layout.arena_bytes(),
            arena_align: storage_layout.arena_align(),
        }
    }

    #[inline]
    fn public_endpoint_resident_budget<const ROLE: u8>(
        compiled_role: RoleImageSlice<ROLE>,
    ) -> crate::rendezvous::core::EndpointResidentBudget {
        crate::rendezvous::core::EndpointResidentBudget::with_route_storage(
            compiled_role.route_table_frame_slots(),
            compiled_role.route_table_lane_slots(),
            compiled_role.loop_table_slots(),
            compiled_role.resident_cap_entries(),
        )
    }

    fn allocate_storage_for_rv(
        &self,
        rv_id: RendezvousId,
        required_bytes: usize,
        required_align: usize,
        resident_budget: crate::rendezvous::core::EndpointResidentBudget,
    ) -> Option<(EndpointLeaseId, u32, *mut u8)> {
        let mut result = None;
        self.with_control_mut(|core| {
            let Some(rv) = core.locals.get_mut(&rv_id) else {
                return;
            };
            if rv
                .ensure_endpoint_resident_budget(resident_budget)
                .is_none()
            {
                return;
            }
            let Some((slot, generation, offset, _len)) = (unsafe {
                rv.allocate_endpoint_lease(required_bytes, required_align, resident_budget)
            }) else {
                return;
            };
            let (slab_ptr, slab_len) = rv.slab_ptr_and_len();
            if offset + required_bytes > slab_len {
                rv.release_endpoint_lease(slot, generation);
                return;
            }
            result = Some((slot, generation, unsafe { slab_ptr.add(offset) }));
        });
        result
    }

    fn allocate_public_endpoint_storage_for_rv<'r, const ROLE: u8, Mint>(
        &self,
        rv_id: RendezvousId,
        required_bytes: usize,
        required_align: usize,
        resident_budget: crate::rendezvous::core::EndpointResidentBudget,
    ) -> Option<(
        EndpointLeaseId,
        u32,
        *mut PublicEndpointKernel<'r, ROLE, T, U, C, MAX_RV, Mint>,
    )>
    where
        Mint: crate::control::cap::mint::MintConfigMarker,
        'cfg: 'r,
    {
        self.allocate_storage_for_rv(rv_id, required_bytes, required_align, resident_budget)
            .map(|(slot, generation, ptr)| {
                (
                    slot,
                    generation,
                    ptr.cast::<PublicEndpointKernel<'r, ROLE, T, U, C, MAX_RV, Mint>>(),
                )
            })
    }

    #[inline(never)]
    fn pin_compiled_images_for_public_endpoint<const ROLE: u8>(
        &self,
        rv_id: RendezvousId,
        slot: EndpointLeaseId,
        generation: u32,
        program_ref: CompiledProgramRef,
    ) -> Result<(), AttachError> {
        let pinned = self.with_control_mut(|core| {
            let Some(rv) = core.locals.get_mut(&rv_id) else {
                return false;
            };
            rv.pin_endpoint_images::<ROLE>(slot, generation, program_ref.stamp())
        });
        if pinned {
            Ok(())
        } else {
            Err(AttachError::Control(CpError::ResourceExhausted))
        }
    }

    fn public_endpoint_storage_raw_ptr(
        &self,
        rv_id: RendezvousId,
        slot: EndpointLeaseId,
        generation: u32,
    ) -> Option<*mut ()> {
        let rv = self.get_local(&rv_id)?;
        let (slab_ptr, slab_len) = rv.slab_ptr_and_len();
        let (offset, len) = rv.endpoint_lease_storage(slot, generation)?;
        if len == 0 || offset + len > slab_len {
            return None;
        }
        Some(unsafe { slab_ptr.add(offset).cast() })
    }

    fn ensure_compiled_program_ref<'prog, const ROLE: u8, P>(
        &self,
        rv_id: RendezvousId,
        program: &P,
    ) -> Result<CompiledProgramRef, AttachError>
    where
        P: crate::global::RoleProgramView<ROLE>,
    {
        let core = unsafe { &mut *self.control_ptr() };
        let rv = core
            .locals
            .get_mut(&rv_id)
            .ok_or(AttachError::Control(CpError::ResourceExhausted))?;
        if let Some(existing) = rv.program_image(program.stamp()) {
            return Ok(unsafe { CompiledProgramRef::from_raw(program.stamp(), existing) });
        }
        let lowering = program.lowering_input();
        let (mut storage, mut len) = rv.scratch_storage_ptr_and_len();
        let guard = rv.program_image_guard_bytes();
        if guard > len {
            return Err(AttachError::Control(CpError::ResourceExhausted));
        }
        storage = unsafe { storage.add(guard) };
        len -= guard;
        unsafe {
            crate::global::compiled::materialize::with_lowering_lease(
                lowering,
                storage,
                len,
                crate::global::compiled::materialize::LoweringLeaseMode::SummaryOnly,
                |lease| rv.materialize_program_image_from_summary(program.stamp(), lease.summary()),
            )
        }
        .flatten()
        .map(|compiled| unsafe { CompiledProgramRef::from_raw(program.stamp(), compiled) })
        .ok_or(AttachError::Control(CpError::ResourceExhausted))
    }

    #[inline(never)]
    fn ensure_role_image_slice<'prog, const ROLE: u8, P>(
        &self,
        rv_id: RendezvousId,
        program: &P,
    ) -> Result<RoleImageSlice<ROLE>, AttachError>
    where
        P: crate::global::RoleProgramView<ROLE>,
    {
        let core = unsafe { &mut *self.control_ptr() };
        let rv = core
            .locals
            .get_mut(&rv_id)
            .ok_or(AttachError::Control(CpError::ResourceExhausted))?;
        if let (Some(program_image), Some(role_image)) = (
            rv.program_image(program.stamp()),
            rv.role_image::<ROLE>(program.stamp()),
        ) {
            let program_ref =
                unsafe { CompiledProgramRef::from_raw(program.stamp(), program_image) };
            return Ok(unsafe { RoleImageSlice::from_raw(program_ref, role_image) });
        }
        let lowering = program.lowering_input();
        let (mut storage, mut len) = rv.scratch_storage_ptr_and_len();
        let has_program = rv.has_program_image(program.stamp());
        let has_role = rv.has_role_image::<ROLE>(program.stamp());
        let role_image_bytes = if has_role {
            0
        } else {
            CompiledRoleImage::persistent_bytes_for_program(lowering.footprint())
        };
        let guard = if has_program {
            if has_role {
                0
            } else {
                rv.role_image_guard_bytes(role_image_bytes)
            }
        } else if has_role {
            rv.program_image_guard_bytes()
        } else {
            rv.program_and_role_image_guard_bytes(role_image_bytes)
        };
        if guard > len {
            return Err(AttachError::Control(CpError::ResourceExhausted));
        }
        storage = unsafe { storage.add(guard) };
        len -= guard;

        unsafe {
            crate::global::compiled::materialize::with_lowering_lease(
                lowering,
                storage,
                len,
                crate::global::compiled::materialize::LoweringLeaseMode::SummaryAndRoleScratch,
                |lease| {
                    let (summary, scratch) = lease.into_parts();
                    Self::materialize_role_image_slice_from_lease::<ROLE>(
                        rv,
                        program.stamp(),
                        has_program,
                        has_role,
                        lowering.footprint(),
                        summary,
                        &mut scratch.expect("role scratch requested by lowering lease mode"),
                    )
                },
            )
        }
        .flatten()
        .ok_or(AttachError::Control(CpError::ResourceExhausted))
    }

    #[inline(never)]
    fn materialize_role_image_slice_from_lease<const ROLE: u8>(
        rv: &mut crate::rendezvous::core::Rendezvous<'_, 'cfg, T, U, C>,
        stamp: crate::global::compiled::lowering::ProgramStamp,
        has_program: bool,
        has_role: bool,
        footprint: crate::global::role_program::RoleFootprint,
        summary: &crate::global::compiled::lowering::LoweringSummary,
        scratch: &mut crate::global::compiled::materialize::RoleLoweringScratch<'_>,
    ) -> Option<RoleImageSlice<ROLE>> {
        let program_image = if has_program {
            rv.program_image(stamp)
        } else {
            unsafe { rv.materialize_program_image_from_summary(stamp, summary) }
        }?;
        let role_image = if has_role {
            rv.role_image::<ROLE>(stamp)
        } else {
            unsafe {
                rv.materialize_role_image_from_summary_for_program_dyn(
                    stamp, ROLE, summary, scratch, footprint,
                )
            }
        }?;
        let program_ref = unsafe { CompiledProgramRef::from_raw(stamp, program_image) };
        Some(unsafe { RoleImageSlice::from_raw(program_ref, role_image) })
    }

    #[cfg(test)]
    fn materialize_test_role_image<'prog, const ROLE: u8, P>(
        &self,
        rv_id: RendezvousId,
        program: &P,
    ) -> Result<RoleImageSlice<ROLE>, AttachError>
    where
        P: crate::global::RoleProgramView<ROLE>,
    {
        self.ensure_role_image_slice(rv_id, program)
    }

    unsafe fn public_endpoint_storage_ptr<'r, const ROLE: u8, Mint>(
        &self,
        rv_id: RendezvousId,
        slot: EndpointLeaseId,
        generation: u32,
    ) -> Option<*mut PublicEndpointKernel<'r, ROLE, T, U, C, MAX_RV, Mint>>
    where
        Mint: crate::control::cap::mint::MintConfigMarker,
        'cfg: 'r,
    {
        self.public_endpoint_storage_raw_ptr(rv_id, slot, generation)
            .map(|ptr| ptr.cast::<PublicEndpointKernel<'r, ROLE, T, U, C, MAX_RV, Mint>>())
    }

    pub(crate) unsafe fn public_endpoint_ptr<'r, const ROLE: u8, Mint>(
        &self,
        rv_id: RendezvousId,
        slot: EndpointLeaseId,
        generation: u32,
    ) -> Option<*mut PublicEndpointKernel<'r, ROLE, T, U, C, MAX_RV, Mint>>
    where
        Mint: crate::control::cap::mint::MintConfigMarker,
        'cfg: 'r,
    {
        unsafe { self.public_endpoint_storage_ptr::<ROLE, Mint>(rv_id, slot, generation) }
    }

    pub(crate) unsafe fn release_public_endpoint_slot(
        &self,
        rv_id: RendezvousId,
        slot: EndpointLeaseId,
        generation: u32,
    ) {
        self.with_control_mut(|core| {
            if let Some(rv) = core.locals.get_mut(&rv_id) {
                rv.release_endpoint_lease(slot, generation);
            }
        });
    }

    fn mark_public_endpoint_lease(
        &self,
        rv_id: RendezvousId,
        slot: EndpointLeaseId,
        generation: u32,
    ) -> Result<(), AttachError> {
        let marked = self.with_control_mut(|core| {
            let Some(rv) = core.locals.get_mut(&rv_id) else {
                return false;
            };
            rv.mark_public_endpoint_lease(slot, generation)
        });
        if !marked {
            return Err(AttachError::Control(CpError::ResourceExhausted));
        }
        Ok(())
    }

    unsafe fn revoke_public_endpoint<const ROLE: u8>(
        endpoint: *mut (),
        sid: SessionId,
        lanes: *mut Lane,
        lane_capacity: usize,
    ) -> usize {
        let endpoint = endpoint.cast::<PublicEndpointKernel<
            'cfg,
            ROLE,
            T,
            U,
            C,
            MAX_RV,
            crate::control::cap::mint::MintConfig,
        >>();
        let endpoint = unsafe { &mut *endpoint };
        if !endpoint.matches_session(sid) {
            return 0;
        }

        let mut released = 0usize;
        endpoint.for_each_physical_lane(|owned_lane| {
            if released < lane_capacity {
                unsafe {
                    lanes.add(released).write(owned_lane);
                }
            }
            released += 1;
        });
        debug_assert!(
            released <= lane_capacity,
            "public endpoint revoke lane buffer must cover every owned lane"
        );
        endpoint.revoke_public_owner();
        unsafe {
            core::ptr::drop_in_place(endpoint);
        }
        core::cmp::min(released, lane_capacity)
    }

    #[inline]
    pub(crate) fn release_public_endpoint_slot_owned(
        &self,
        rv_id: RendezvousId,
        slot: EndpointLeaseId,
        generation: u32,
    ) {
        unsafe {
            self.release_public_endpoint_slot(rv_id, slot, generation);
        }
    }

    fn with_transient_compiled_program<'prog, const ROLE: u8, P, F, R, E>(
        &self,
        rv_id: RendezvousId,
        program: &P,
        f: F,
    ) -> Result<R, E>
    where
        E: From<CpError>,
        P: crate::global::RoleProgramView<ROLE>,
        F: FnOnce(CompiledProgramRef) -> Result<R, E>,
    {
        let compiled =
            self.ensure_compiled_program_ref(rv_id, program)
                .map_err(|err| match err {
                    AttachError::Control(cp) => E::from(cp),
                    AttachError::Rendezvous(_) => E::from(CpError::ResourceExhausted),
                })?;
        f(compiled)
    }

    #[cfg(test)]
    fn with_transient_compiled_role<'prog, const ROLE: u8, P, F, R, E>(
        &self,
        rv_id: RendezvousId,
        program: &P,
        f: F,
    ) -> Result<R, E>
    where
        E: From<CpError>,
        P: crate::global::RoleProgramView<ROLE>,
        F: FnOnce(RoleImageSlice<ROLE>) -> Result<R, E>,
    {
        let role_image = self
            .ensure_role_image_slice(rv_id, program)
            .map_err(|err| match err {
                AttachError::Control(cp) => E::from(cp),
                AttachError::Rendezvous(_) => E::from(CpError::ResourceExhausted),
            })?;
        f(role_image)
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
    /// Returns `CpError::ResourceExhausted` if the cluster is full.
    /// Build and register a local rendezvous from runtime config + transport.
    ///
    /// Public callers should use this entrypoint instead of constructing
    /// rendezvous internals directly.
    #[cfg(not(test))]
    pub(crate) fn add_rendezvous_from_config(
        &self,
        config: crate::runtime::config::Config<'cfg, U, C>,
        transport: T,
    ) -> Result<RendezvousId, CpError> {
        self.with_control_mut(|core| {
            match core
                .locals
                .register_local_from_config_auto(config, transport)
            {
                Ok(id) => Ok(id),
                Err(
                    RegisterRendezvousError::CapacityExceeded
                    | RegisterRendezvousError::StorageExhausted,
                ) => Err(CpError::ResourceExhausted),
            }
        })
    }

    #[cfg(test)]
    pub(crate) fn add_rendezvous_from_config(
        &self,
        config: crate::runtime::config::Config<'cfg, U, C>,
        transport: T,
    ) -> Result<RendezvousId, CpError> {
        self.with_control_mut(|core| {
            match core
                .locals
                .register_local_from_config(config, transport, MAX_RV)
            {
                Ok(id) => Ok(id),
                Err(
                    RegisterRendezvousError::CapacityExceeded
                    | RegisterRendezvousError::StorageExhausted,
                ) => Err(CpError::ResourceExhausted),
            }
        })
    }

    #[cfg(test)]
    pub(crate) fn add_rendezvous_from_config_auto(
        &self,
        config: crate::runtime::config::Config<'cfg, U, C>,
        transport: T,
    ) -> Result<RendezvousId, CpError> {
        self.with_control_mut(|core| {
            match core
                .locals
                .register_local_from_config_auto(config, transport)
            {
                Ok(id) => Ok(id),
                Err(
                    RegisterRendezvousError::CapacityExceeded
                    | RegisterRendezvousError::StorageExhausted,
                ) => Err(CpError::ResourceExhausted),
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

    fn ensure_local_topology_storage(
        &self,
        target: RendezvousId,
        _lane: Lane,
    ) -> Result<(), CpError> {
        self.with_control_mut(|core| {
            let rv = core
                .locals
                .get_mut(&target)
                .ok_or(CpError::RendezvousMismatch {
                    expected: target.raw(),
                    actual: 0,
                })?;
            rv.ensure_topology_control_storage()
                .ok_or(CpError::ResourceExhausted)
        })
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
    pub(crate) fn lease_port<'lease>(
        &'lease self,
        rv_id: RendezvousId,
        sid: SessionId,
        lane: Lane,
        role: u8,
        role_count: u8,
    ) -> Result<LaneLease<'lease, 'cfg, T, U, C, MAX_RV>, RendezvousError>
    where
        'cfg: 'lease,
    {
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

        Ok(LaneLease::new(
            lease, sid, lane, role, role_count, active, brand,
        ))
    }

    /// Execute a control-plane effect on a specific local Rendezvous.
    pub(crate) fn run_effect_step(
        &self,
        target: RendezvousId,
        envelope: CpCommand,
    ) -> Result<PendingEffect, CpError> {
        let envelope = match envelope.effect {
            ControlOp::CapDelegate => envelope.canonicalize_delegate()?,
            ControlOp::TopologyBegin | ControlOp::TopologyAck | ControlOp::TopologyCommit => {
                envelope.canonicalize_topology()?
            }
            _ => envelope,
        };

        if let Some(operands) = envelope.topology {
            Self::validate_topology_target(envelope.effect, target, operands)?;
        }

        if self.get_local(&target).is_some() {
            match envelope.effect {
                ControlOp::TopologyBegin => {
                    let sid = envelope
                        .sid
                        .ok_or(CpError::Topology(TopologyError::InvalidSession))?;
                    let operands = envelope
                        .topology
                        .ok_or(CpError::Topology(TopologyError::InvalidState))?;
                    self.preflight_topology_begin(sid, operands)?;
                    self.ensure_local_topology_storage(target, operands.src_lane)?;
                    let seed = operands.intent(sid);
                    let dst_rv = seed.dst_rv;

                    let begin_needs = facets_caps_topology();

                    let drive_result = self.drive::<TopologyBeginAutomaton, _, _>(
                        target,
                        seed,
                        move |core, rv| {
                            let mut ctx =
                                Self::init_bundle_context_with_needs(core, rv, begin_needs);
                            ctx.set_topology(TopologyGraphContext::new(Some(seed)));
                            ctx
                        },
                        |core, graph| {
                            if dst_rv != target && begin_needs.requires_topology() {
                                graph.add_child_with_bundle_config(
                                    &mut core.locals,
                                    target,
                                    dst_rv,
                                    |child_ctx| {
                                        child_ctx.set_topology(TopologyGraphContext::default());
                                    },
                                )?;
                            }
                            Ok(())
                        },
                    );

                    if let Err(err) = drive_result {
                        return Err(match err {
                            DelegationDriveError::Lease(_) | DelegationDriveError::Graph(_) => {
                                CpError::Topology(TopologyError::InvalidState)
                            }
                            DelegationDriveError::Automaton(err) => err.into(),
                        });
                    }
                    return self.after_local_effect(envelope);
                }
                ControlOp::TopologyAck => {
                    let sid = envelope
                        .sid
                        .ok_or(CpError::Topology(TopologyError::InvalidSession))?;
                    let operands = envelope
                        .topology
                        .ok_or(CpError::Topology(TopologyError::InvalidState))?;
                    self.preflight_topology_ack(sid, operands)?;
                    self.ensure_local_topology_storage(target, operands.dst_lane)?;
                    return self.with_control_mut(|core| {
                        let ack = match core.locals.get_mut(&operands.dst_rv) {
                            Some(rv) => match rv.acknowledge_topology_intent(&operands.intent(sid))
                            {
                                Ok(ack) => ack,
                                Err(err) => {
                                    let err = CpError::Topology(err.into());
                                    let _ = Self::abort_inflight_topology_entry(
                                        core,
                                        sid,
                                        operands.src_rv,
                                    );
                                    return Err(err);
                                }
                            },
                            None => {
                                return Err(CpError::RendezvousMismatch {
                                    expected: operands.dst_rv.raw(),
                                    actual: 0,
                                });
                            }
                        };
                        if ack != operands.ack(sid) {
                            let err = CpError::Topology(TopologyError::GenerationMismatch);
                            let _ = Self::abort_inflight_topology_entry(core, sid, operands.src_rv);
                            return Err(err);
                        }
                        let recorded = core
                            .topology_state
                            .acknowledge(sid, operands.src_rv)
                            .expect(
                                "topology ack bookkeeping was preflighted before local mutation",
                            );
                        debug_assert_eq!(recorded, ack);
                        Ok(PendingEffect::None)
                    });
                }
                ControlOp::TopologyCommit => {
                    let sid = envelope
                        .sid
                        .ok_or(CpError::Topology(TopologyError::InvalidSession))?;
                    let operands = envelope
                        .topology
                        .ok_or(CpError::Topology(TopologyError::InvalidState))?;
                    self.ensure_local_topology_storage(target, operands.src_lane)?;
                    return self.with_control_mut(|core| {
                        let tracked = core
                            .topology_state
                            .get(sid)
                            .copied()
                            .ok_or(CpError::Topology(TopologyError::InvalidSession))?;
                        debug_assert_eq!(tracked.src_rv, operands.src_rv);

                        if let Err(err) = core.topology_state.preflight_commit(
                            sid,
                            operands.src_rv,
                            Some(operands.ack(sid)),
                        ) {
                            let _ = Self::abort_inflight_topology_entry(core, sid, operands.src_rv);
                            return Err(err);
                        }

                        let source_lane = match core.locals.get_mut(&operands.src_rv) {
                            Some(rv) => match rv.validate_topology_commit_operands(sid, operands) {
                                Ok(lane) => lane,
                                Err(err) => {
                                    let err = CpError::Topology(err.into());
                                    let _ = Self::abort_inflight_topology_entry(
                                        core,
                                        sid,
                                        tracked.src_rv,
                                    );
                                    return Err(err);
                                }
                            },
                            None => {
                                return Err(CpError::RendezvousMismatch {
                                    expected: operands.src_rv.raw(),
                                    actual: 0,
                                });
                            }
                        };

                        {
                            let rv = core.locals.get_mut(&operands.dst_rv).ok_or(
                                CpError::RendezvousMismatch {
                                    expected: operands.dst_rv.raw(),
                                    actual: 0,
                                },
                            )?;
                            if let Err(err) =
                                rv.preflight_destination_topology_commit(sid, operands.dst_lane)
                            {
                                let err = CpError::Topology(err.into());
                                let _ =
                                    Self::abort_inflight_topology_entry(core, sid, tracked.src_rv);
                                return Err(err);
                            }
                        }

                        {
                            let rv = core.locals.get_mut(&operands.dst_rv).ok_or(
                                CpError::RendezvousMismatch {
                                    expected: operands.dst_rv.raw(),
                                    actual: 0,
                                },
                            )?;
                            if let Err(err) =
                                rv.finalize_destination_topology_commit(sid, operands.dst_lane)
                            {
                                let err = CpError::Topology(err.into());
                                let _ =
                                    Self::abort_inflight_topology_entry(core, sid, tracked.src_rv);
                                return Err(err);
                            }
                        }

                        {
                            let rv = core.locals.get_mut(&operands.src_rv).ok_or(
                                CpError::RendezvousMismatch {
                                    expected: operands.src_rv.raw(),
                                    actual: 0,
                                },
                            )?;
                            if let Err(err) = rv.topology_commit(sid, source_lane) {
                                let err = CpError::Topology(err.into());
                                let _ =
                                    Self::abort_inflight_topology_entry(core, sid, tracked.src_rv);
                                return Err(err);
                            }
                        }

                        let committed = core
                            .topology_state
                            .topology_commit(sid, operands.src_rv, Some(operands.ack(sid)))
                            .expect(
                                "topology commit bookkeeping was preflighted before local mutation",
                            );
                        debug_assert_eq!(committed, operands);
                        Ok(PendingEffect::None)
                    });
                }
                _ => {
                    if self.get_local(&target).is_some() {
                        self.with_control_mut(|core| {
                            let rv = core
                                .locals
                                .get_mut(&target)
                                .expect("local rendezvous must remain available");
                            EffectRunner::run_effect(rv, envelope.clone())
                        })?;
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

    #[inline]
    fn validate_topology_target(
        effect: ControlOp,
        target: RendezvousId,
        operands: TopologyOperands,
    ) -> Result<(), CpError> {
        let expected = match effect {
            ControlOp::TopologyBegin | ControlOp::TopologyCommit => operands.src_rv,
            ControlOp::TopologyAck => operands.dst_rv,
            _ => return Ok(()),
        };

        if target != expected {
            return Err(CpError::RendezvousMismatch {
                expected: expected.raw(),
                actual: target.raw(),
            });
        }

        Ok(())
    }

    pub(crate) fn run_effect(
        &self,
        target: RendezvousId,
        envelope: CpCommand,
    ) -> Result<(), CpError> {
        self.run_effect_step(target, envelope)?;
        Ok(())
    }

    pub(crate) fn distributed_topology_operands(&self, sid: SessionId) -> Option<TopologyOperands> {
        self.with_control_mut(|core| {
            core.topology_state
                .get(sid)
                .copied()
                .or_else(|| core.cached_operands_get(sid).copied())
        })
    }

    pub(crate) fn cached_topology_operands(&self, sid: SessionId) -> Option<TopologyOperands> {
        self.with_control_mut(|core| core.cached_operands_get(sid).copied())
    }

    #[cfg(test)]
    fn cache_topology_operands(
        &self,
        sid: SessionId,
        operands: TopologyOperands,
    ) -> Result<(), CpError> {
        self.with_control_mut(|core| core.cached_operands_insert(sid, operands))
    }

    fn ensure_dynamic_resolver_capacity(
        &self,
        rv_id: RendezvousId,
        additional_entries: usize,
    ) -> Result<(), CpError> {
        if additional_entries == 0 {
            return Ok(());
        }
        self.with_control_mut(|core| {
            let rv = core
                .locals
                .get_mut(&rv_id)
                .ok_or(CpError::RendezvousMismatch {
                    expected: rv_id.raw(),
                    actual: 0,
                })?;
            let rv_ptr = ::core::ptr::from_mut(rv);
            unsafe { &mut *self.resolvers_ptr() }.ensure_capacity(
                rv_id,
                additional_entries,
                |bytes, align| unsafe {
                    (&mut *rv_ptr).allocate_external_persistent_sidecar_bytes(bytes, align)
                },
                |ptr, bytes, reclaim_delta| unsafe {
                    (&mut *rv_ptr).free_external_persistent_sidecar_bytes(ptr, bytes, reclaim_delta)
                },
            )
        })
    }

    fn dynamic_resolver(&self, key: DynamicResolverKey) -> Option<&DynamicResolverEntry<'cfg>> {
        unsafe { (*self.resolvers_ref_ptr()).get(key) }
    }

    pub(crate) fn set_resolver<'prog, const POLICY: u16, const ROLE: u8>(
        &self,
        rv_id: RendezvousId,
        program: &crate::g::advanced::RoleProgram<ROLE>,
        resolver: ResolverRef<'cfg>,
    ) -> Result<(), CpError> {
        self.with_transient_compiled_program(rv_id, program, |compiled| {
            self.ensure_dynamic_resolver_capacity(
                rv_id,
                compiled.dynamic_policy_sites_for(POLICY).count(),
            )?;
            for site in compiled.dynamic_policy_sites_for(POLICY) {
                let tag = site
                    .resource_tag()
                    .ok_or(CpError::UnsupportedEffect(site.label()))?;
                let op = site.op().ok_or(CpError::UnsupportedEffect(site.label()))?;
                self.register_dynamic_policy_resolver(
                    rv_id,
                    site.eff_index(),
                    site.label(),
                    site.policy(),
                    tag,
                    op,
                    None,
                    resolver,
                )?;
            }
            Ok(())
        })
    }

    pub(crate) fn register_dynamic_policy_resolver(
        &self,
        rv_id: RendezvousId,
        eff_index: EffIndex,
        label: u8,
        policy: PolicyMode,
        _tag: u8,
        op: ControlOp,
        scope_trace: Option<ScopeTrace>,
        resolver: ResolverRef<'cfg>,
    ) -> Result<(), CpError> {
        let key = DynamicResolverKey::new(rv_id, eff_index, op);
        let policy = match policy {
            PolicyMode::Dynamic { .. } => {
                let _ = policy
                    .dynamic_policy_id()
                    .ok_or(CpError::UnsupportedEffect(label))?;
                if !is_dynamic_control_op(op) {
                    return Err(CpError::UnsupportedEffect(op as u8));
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
        self.ensure_dynamic_resolver_capacity(rv_id, 1)?;
        self.with_resolvers_mut(|core| core.insert(key, entry))
    }

    pub(crate) fn resolve_dynamic_policy(
        &self,
        rv_id: RendezvousId,
        session: Option<SessionId>,
        lane: Lane,
        eff_index: EffIndex,
        tag: u8,
        op: ControlOp,
        input: [u32; 4],
        attrs: &crate::transport::context::PolicyAttrs,
    ) -> Result<DynamicResolution, CpError> {
        let key = DynamicResolverKey::new(rv_id, eff_index, op);
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
            scope_hint,
            entry.scope_trace,
            input,
            attrs,
        );

        let resolution = entry
            .resolver
            .resolve(ctx)
            .map_err(|_| CpError::PolicyAbort { reason: policy_id })?;

        match (op, resolution) {
            (ControlOp::RouteDecision, DynamicResolution::RouteArm { arm }) => {
                if scope_hint.is_none() {
                    return Err(CpError::PolicyAbort { reason: policy_id });
                }
                if arm > 1 {
                    return Err(CpError::PolicyAbort { reason: policy_id });
                }
                Ok(DynamicResolution::RouteArm { arm })
            }
            (
                ControlOp::LoopContinue | ControlOp::LoopBreak,
                DynamicResolution::Loop { decision },
            ) => {
                if scope_hint.is_none() {
                    return Err(CpError::PolicyAbort { reason: policy_id });
                }
                Ok(DynamicResolution::Loop { decision })
            }
            (
                ControlOp::LoopContinue | ControlOp::LoopBreak | ControlOp::RouteDecision,
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
        op: ControlOp,
    ) -> Result<PolicyMode, CpError> {
        let rv = self.get_local(&rv_id).ok_or(CpError::RendezvousMismatch {
            expected: rv_id.raw(),
            actual: 0,
        })?;
        let lane_rv = Lane::new(lane.raw());
        let key = DynamicResolverKey::new(rv_id, eff_index, op);
        let policy = rv
            .policy(lane_rv, eff_index, tag)
            .or_else(|| self.dynamic_resolver(key).map(|entry| entry.policy));
        Ok(policy.unwrap_or(PolicyMode::Static))
    }

    pub(crate) fn prepare_topology_operands_from_descriptor(
        &self,
        rv_id: RendezvousId,
        src_lane: Lane,
        desc: ControlDesc,
        descriptor: TopologyDescriptor,
    ) -> Result<TopologyOperands, CpError> {
        if !matches!(desc.op(), ControlOp::TopologyBegin)
            || !matches!(desc.scope_kind(), ControlScopeKind::Topology)
        {
            return Err(CpError::Authorisation {
                operation: ControlOp::TopologyBegin as u8,
            });
        }
        self.validate_topology_begin_with_handle(rv_id, src_lane, descriptor.handle(), None)
    }

    pub(crate) fn validate_topology_operands_from_descriptor(
        &self,
        rv_id: RendezvousId,
        src_lane: Lane,
        desc: ControlDesc,
        descriptor: TopologyDescriptor,
        operands: TopologyOperands,
    ) -> Result<(), CpError> {
        let expected = match desc.op() {
            ControlOp::TopologyAck => {
                self.validate_topology_ack_with_handle(rv_id, src_lane, descriptor.handle(), None)?
            }
            ControlOp::TopologyCommit => self.validate_topology_commit_with_handle(
                rv_id,
                src_lane,
                descriptor.handle(),
                None,
            )?,
            _ => {
                return Err(CpError::Authorisation {
                    operation: desc.op() as u8,
                });
            }
        };
        if expected != operands {
            return Err(CpError::Authorisation {
                operation: desc.op() as u8,
            });
        }
        Ok(())
    }

    pub(crate) fn prepare_reroute_handle_from_policy(
        &self,
        rv_id: RendezvousId,
        lane: Lane,
        eff_index: EffIndex,
        tag: u8,
        op: ControlOp,
        policy: PolicyMode,
        input: [u32; 4],
        attrs: &crate::transport::context::PolicyAttrs,
    ) -> Result<DelegationHandle, CpError> {
        let _ = (eff_index, tag, op, attrs);
        match policy {
            PolicyMode::Static => delegation_handle_from_route_input(rv_id, lane, input),
            PolicyMode::Dynamic { .. } => {
                Err(CpError::UnsupportedEffect(ControlOp::CapDelegate as u8))
            }
        }
    }

    pub(crate) fn take_cached_topology_operands(&self, sid: SessionId) -> Option<TopologyOperands> {
        self.with_control_mut(|core| core.cached_operands_remove(sid))
    }

    fn dispatch_topology_begin_with_handle(
        &self,
        rv_id: RendezvousId,
        cp_sid: SessionId,
        cp_lane: Lane,
        handle: TopologyHandle,
        generation: Option<Generation>,
    ) -> Result<(), CpError> {
        let operands =
            self.validate_topology_begin_with_handle(rv_id, cp_lane, handle, generation)?;
        self.run_effect(operands.src_rv, CpCommand::topology_begin(cp_sid, operands))
    }

    fn validate_topology_begin_with_handle(
        &self,
        rv_id: RendezvousId,
        cp_lane: Lane,
        handle: TopologyHandle,
        generation: Option<Generation>,
    ) -> Result<TopologyOperands, CpError> {
        let operands = topology_operands_from_handle(handle);
        validate_topology_rendezvous_pair(
            operands.src_rv,
            operands.dst_rv,
            ControlOp::TopologyBegin,
        )?;

        if cp_lane != operands.src_lane {
            return Err(CpError::Authorisation {
                operation: ControlOp::TopologyBegin as u8,
            });
        }

        if let Some(header_gen) = generation
            && header_gen != operands.new_gen
        {
            return Err(CpError::Topology(TopologyError::GenerationMismatch));
        }

        if rv_id != operands.src_rv {
            return Err(CpError::RendezvousMismatch {
                expected: operands.src_rv.raw(),
                actual: rv_id.raw(),
            });
        }
        Ok(operands)
    }

    fn dispatch_topology_ack_with_handle(
        &self,
        rv_id: RendezvousId,
        cp_sid: SessionId,
        cp_lane: Lane,
        handle: TopologyHandle,
        generation: Option<Generation>,
    ) -> Result<(), CpError> {
        let operands =
            self.validate_topology_ack_with_handle(rv_id, cp_lane, handle, generation)?;
        self.run_effect(operands.dst_rv, CpCommand::topology_ack(cp_sid, operands))
    }

    fn validate_topology_ack_with_handle(
        &self,
        rv_id: RendezvousId,
        cp_lane: Lane,
        handle: TopologyHandle,
        generation: Option<Generation>,
    ) -> Result<TopologyOperands, CpError> {
        let operands = topology_operands_from_handle(handle);
        validate_topology_rendezvous_pair(
            operands.src_rv,
            operands.dst_rv,
            ControlOp::TopologyAck,
        )?;

        if cp_lane != operands.dst_lane {
            return Err(CpError::Authorisation {
                operation: ControlOp::TopologyAck as u8,
            });
        }

        if let Some(header_gen) = generation
            && header_gen != operands.new_gen
        {
            return Err(CpError::Topology(TopologyError::GenerationMismatch));
        }

        if rv_id != operands.dst_rv {
            return Err(CpError::RendezvousMismatch {
                expected: operands.dst_rv.raw(),
                actual: rv_id.raw(),
            });
        }
        Ok(operands)
    }

    fn dispatch_topology_commit_with_handle(
        &self,
        rv_id: RendezvousId,
        cp_sid: SessionId,
        cp_lane: Lane,
        handle: TopologyHandle,
        generation: Option<Generation>,
    ) -> Result<(), CpError> {
        let operands =
            self.validate_topology_commit_with_handle(rv_id, cp_lane, handle, generation)?;
        self.run_effect(
            operands.src_rv,
            CpCommand::topology_commit(cp_sid, operands),
        )
    }

    fn validate_topology_commit_with_handle(
        &self,
        rv_id: RendezvousId,
        cp_lane: Lane,
        handle: TopologyHandle,
        generation: Option<Generation>,
    ) -> Result<TopologyOperands, CpError> {
        let operands = topology_operands_from_handle(handle);
        validate_topology_rendezvous_pair(
            operands.src_rv,
            operands.dst_rv,
            ControlOp::TopologyCommit,
        )?;

        if cp_lane != operands.src_lane {
            return Err(CpError::Authorisation {
                operation: ControlOp::TopologyCommit as u8,
            });
        }

        if let Some(header_gen) = generation
            && header_gen != operands.new_gen
        {
            return Err(CpError::Topology(TopologyError::GenerationMismatch));
        }

        if rv_id != operands.src_rv {
            return Err(CpError::RendezvousMismatch {
                expected: operands.src_rv.raw(),
                actual: rv_id.raw(),
            });
        }
        Ok(operands)
    }

    #[inline]
    fn validate_session_lane_handle(
        expected_sid: SessionId,
        expected_lane: Lane,
        handle: SessionLaneHandle,
        operation: ControlOp,
    ) -> Result<(), CpError> {
        let (sid_raw, lane_raw) = handle;
        let handle_sid = SessionId::new(sid_raw);
        let handle_lane = Lane::new(lane_raw as u32);
        if handle_sid != expected_sid || handle_lane != expected_lane {
            return Err(CpError::Authorisation {
                operation: operation as u8,
            });
        }
        Ok(())
    }

    fn dispatch_abort_begin_with_handle(
        &self,
        rv_id: RendezvousId,
        cp_sid: SessionId,
        cp_lane: Lane,
        handle: SessionLaneHandle,
        generation: Option<Generation>,
    ) -> Result<(), CpError> {
        Self::validate_session_lane_handle(cp_sid, cp_lane, handle, ControlOp::AbortBegin)?;

        self.run_effect(rv_id, CpCommand::abort_begin(cp_sid, cp_lane))?;
        let _ = generation;
        Ok(())
    }

    fn dispatch_abort_ack_with_handle(
        &self,
        rv_id: RendezvousId,
        cp_sid: SessionId,
        cp_lane: Lane,
        handle: SessionLaneHandle,
        generation: Option<Generation>,
    ) -> Result<(), CpError> {
        Self::validate_session_lane_handle(cp_sid, cp_lane, handle, ControlOp::AbortAck)?;

        let effect_gen = generation.ok_or(CpError::Authorisation {
            operation: ControlOp::AbortAck as u8,
        })?;
        self.require_local_lane_generation(rv_id, cp_lane, effect_gen)?;
        self.run_effect(rv_id, CpCommand::abort_ack(cp_sid, cp_lane, effect_gen))
    }

    fn dispatch_state_snapshot_with_handle(
        &self,
        rv_id: RendezvousId,
        cp_sid: SessionId,
        cp_lane: Lane,
        handle: SessionLaneHandle,
        generation: Option<Generation>,
    ) -> Result<(), CpError> {
        Self::validate_session_lane_handle(cp_sid, cp_lane, handle, ControlOp::StateSnapshot)?;

        if let Some(effect_gen) = generation {
            self.require_local_lane_generation(rv_id, cp_lane, effect_gen)?;
        }
        self.run_effect(rv_id, CpCommand::state_snapshot(cp_sid, cp_lane))
    }

    fn dispatch_tx_commit_with_handle(
        &self,
        rv_id: RendezvousId,
        cp_sid: SessionId,
        cp_lane: Lane,
        handle: SessionLaneHandle,
        generation: Option<Generation>,
    ) -> Result<(), CpError> {
        Self::validate_session_lane_handle(cp_sid, cp_lane, handle, ControlOp::TxCommit)?;

        let effect_gen = generation.ok_or(CpError::Authorisation {
            operation: ControlOp::TxCommit as u8,
        })?;
        self.run_effect(rv_id, CpCommand::tx_commit(cp_sid, cp_lane, effect_gen))
    }

    fn dispatch_state_restore_with_handle(
        &self,
        rv_id: RendezvousId,
        cp_sid: SessionId,
        cp_lane: Lane,
        handle: SessionLaneHandle,
        generation: Option<Generation>,
    ) -> Result<(), CpError> {
        Self::validate_session_lane_handle(cp_sid, cp_lane, handle, ControlOp::StateRestore)?;

        let effect_gen = generation.ok_or(CpError::Authorisation {
            operation: ControlOp::StateRestore as u8,
        })?;
        self.run_effect(rv_id, CpCommand::state_restore(cp_sid, cp_lane, effect_gen))
    }

    fn dispatch_tx_abort_with_handle(
        &self,
        rv_id: RendezvousId,
        cp_sid: SessionId,
        cp_lane: Lane,
        handle: SessionLaneHandle,
        generation: Option<Generation>,
    ) -> Result<(), CpError> {
        Self::validate_session_lane_handle(cp_sid, cp_lane, handle, ControlOp::TxAbort)?;

        let effect_gen = generation.ok_or(CpError::Authorisation {
            operation: ControlOp::TxAbort as u8,
        })?;
        self.run_effect(rv_id, CpCommand::tx_abort(cp_sid, cp_lane, effect_gen))
    }

    #[inline]
    fn descriptor_epoch_generation(op: ControlOp, expected_epoch: u16) -> Option<Generation> {
        match op {
            ControlOp::AbortAck
            | ControlOp::StateSnapshot
            | ControlOp::StateRestore
            | ControlOp::TxCommit
            | ControlOp::TxAbort => Some(Generation::new(expected_epoch)),
            _ => None,
        }
    }

    #[inline]
    fn descriptor_dispatch_generation(
        op: ControlOp,
        expected_epoch: u16,
        generation: Option<Generation>,
    ) -> Result<Option<Generation>, CpError> {
        let Some(descriptor_generation) = Self::descriptor_epoch_generation(op, expected_epoch)
        else {
            return Ok(generation);
        };
        if let Some(generation) = generation
            && generation != descriptor_generation
        {
            return Err(CpError::GenerationViolation {
                expected: descriptor_generation.raw(),
                actual: generation.raw(),
            });
        }
        Ok(Some(descriptor_generation))
    }

    #[inline]
    fn require_generation(actual: Generation, expected: Generation) -> Result<(), CpError> {
        if actual == expected {
            Ok(())
        } else {
            Err(CpError::GenerationViolation {
                expected: expected.raw(),
                actual: actual.raw(),
            })
        }
    }

    #[inline]
    fn require_local_lane_generation(
        &self,
        rv_id: RendezvousId,
        lane: Lane,
        expected: Generation,
    ) -> Result<(), CpError> {
        Self::require_generation(self.local_lane_generation(rv_id, lane)?, expected)
    }

    #[inline]
    fn local_lane_generation(
        &self,
        rv_id: RendezvousId,
        lane: Lane,
    ) -> Result<Generation, CpError> {
        self.get_local(&rv_id)
            .map(|rv| rv.lane_generation(lane))
            .ok_or(CpError::RendezvousMismatch {
                expected: rv_id.raw(),
                actual: 0,
            })
    }

    #[inline]
    fn local_snapshot_generation_for_commit(
        &self,
        rv_id: RendezvousId,
        lane: Lane,
    ) -> Result<Generation, CpError> {
        let rv = self.get_local(&rv_id).ok_or(CpError::RendezvousMismatch {
            expected: rv_id.raw(),
            actual: 0,
        })?;
        rv.snapshot_generation(lane)
            .ok_or(CpError::TxCommit(TxCommitError::NoStateSnapshot))
    }

    #[inline]
    fn local_snapshot_generation_for_restore(
        &self,
        rv_id: RendezvousId,
        lane: Lane,
    ) -> Result<Generation, CpError> {
        let rv = self.get_local(&rv_id).ok_or(CpError::RendezvousMismatch {
            expected: rv_id.raw(),
            actual: 0,
        })?;
        rv.snapshot_generation(lane)
            .ok_or(CpError::StateRestore(StateRestoreError::EpochNotFound))
    }

    #[inline]
    fn local_snapshot_generation_for_abort(
        &self,
        rv_id: RendezvousId,
        lane: Lane,
    ) -> Result<Generation, CpError> {
        let rv = self.get_local(&rv_id).ok_or(CpError::RendezvousMismatch {
            expected: rv_id.raw(),
            actual: 0,
        })?;
        rv.snapshot_generation(lane)
            .ok_or(CpError::TxAbort(TxAbortError::NoStateSnapshot))
    }

    fn preflight_topology_begin(
        &self,
        sid: SessionId,
        operands: TopologyOperands,
    ) -> Result<(), CpError> {
        self.with_control_mut(|core| {
            if core.topology_state.contains_sid(sid) {
                return Err(CpError::ReplayDetected {
                    operation: ControlOp::TopologyBegin as u8,
                    nonce: sid.raw(),
                });
            }
            core.ensure_distributed_topology_capacity(operands.src_rv, 1)
        })
    }

    fn preflight_topology_ack(
        &self,
        sid: SessionId,
        operands: TopologyOperands,
    ) -> Result<(), CpError> {
        self.with_control_mut(|core| {
            core.topology_state
                .preflight_ack(sid, operands.src_rv, operands.ack(sid))
        })
    }

    fn abort_inflight_topology_entry(
        core: &mut ControlCore<'cfg, T, U, C, crate::control::cap::mint::EpochTbl, MAX_RV>,
        sid: SessionId,
        src_rv: RendezvousId,
    ) -> Result<TopologyOperands, CpError> {
        let operands = core
            .topology_state
            .get(sid)
            .copied()
            .ok_or(CpError::Topology(TopologyError::InvalidSession))?;
        debug_assert_eq!(operands.src_rv, src_rv);

        {
            let rv = core
                .locals
                .get_mut(&operands.src_rv)
                .ok_or(CpError::RendezvousMismatch {
                    expected: operands.src_rv.raw(),
                    actual: 0,
                })?;
            rv.abort_topology_state(sid)
                .map_err(|err| CpError::Topology(err.into()))?;
        }

        if operands.dst_rv != operands.src_rv {
            let rv = core
                .locals
                .get_mut(&operands.dst_rv)
                .ok_or(CpError::RendezvousMismatch {
                    expected: operands.dst_rv.raw(),
                    actual: 0,
                })?;
            rv.abort_topology_state(sid)
                .map_err(|err| CpError::Topology(err.into()))?;
        }

        core.topology_state.abort(sid, src_rv)
    }

    fn verify_control_header(
        desc: ControlDesc,
        header: CapHeader,
        expected_scope_id: u16,
        expected_epoch: u16,
    ) -> Result<(), CpError> {
        let mismatch = CpError::Authorisation {
            operation: desc.op() as u8,
        };
        if header.tag() != desc.resource_tag()
            || header.label() != desc.label()
            || header.op() != desc.op()
            || header.path() != desc.path()
            || header.shot() != desc.shot()
            || header.scope_kind() != desc.scope_kind()
            || header.flags() != desc.header_flags()
            || header.scope_id() != expected_scope_id
            || header.epoch() != expected_epoch
        {
            return Err(mismatch);
        }
        Ok(())
    }

    pub(crate) fn validate_descriptor_control_frame(
        &self,
        bytes: [u8; CAP_TOKEN_LEN],
        desc: ControlDesc,
        expected_scope_id: u16,
        expected_epoch: u16,
    ) -> Result<(), CpError> {
        let token = GenericCapToken::<()>::from_bytes(bytes);
        let header = token.control_header().map_err(|_| CpError::Authorisation {
            operation: desc.op() as u8,
        })?;
        Self::verify_control_header(desc, header, expected_scope_id, expected_epoch)?;

        match desc.op() {
            ControlOp::TopologyBegin | ControlOp::TopologyAck | ControlOp::TopologyCommit => {
                TopologyHandle::decode(token.handle_bytes()).map_err(|_| {
                    CpError::Authorisation {
                        operation: desc.op() as u8,
                    }
                })?;
            }
            ControlOp::AbortBegin
            | ControlOp::AbortAck
            | ControlOp::StateSnapshot
            | ControlOp::TxCommit
            | ControlOp::TxAbort
            | ControlOp::StateRestore => {
                decode_session_lane_handle(token.handle_bytes()).map_err(|_| {
                    CpError::Authorisation {
                        operation: desc.op() as u8,
                    }
                })?;
            }
            ControlOp::Fence
            | ControlOp::CapDelegate
            | ControlOp::RouteDecision
            | ControlOp::LoopContinue
            | ControlOp::LoopBreak => {}
        }

        Ok(())
    }

    pub(crate) fn validate_send_bound_descriptor_control_frame(
        &self,
        rv_id: RendezvousId,
        bytes: [u8; CAP_TOKEN_LEN],
        desc: ControlDesc,
        expected_sid: SessionId,
        expected_lane: Lane,
        expected_role: u8,
        expected_scope_id: u16,
        expected_epoch: u16,
    ) -> Result<(), CpError> {
        let token = GenericCapToken::<()>::from_bytes(bytes);
        let header = token.control_header().map_err(|_| CpError::Authorisation {
            operation: desc.op() as u8,
        })?;
        Self::verify_control_header(desc, header, expected_scope_id, expected_epoch)?;
        if header.sid() != expected_sid
            || header.lane() != expected_lane
            || header.role() != expected_role
        {
            return Err(CpError::Authorisation {
                operation: desc.op() as u8,
            });
        }

        let cp_sid = header.sid();
        let cp_lane = header.lane();
        match desc.op() {
            ControlOp::TopologyBegin => {
                let handle = TopologyHandle::decode(token.handle_bytes()).map_err(|_| {
                    CpError::Authorisation {
                        operation: ControlOp::TopologyBegin as u8,
                    }
                })?;
                let _ = self.validate_topology_begin_with_handle(rv_id, cp_lane, handle, None)?;
            }
            ControlOp::TopologyAck => {
                let handle = TopologyHandle::decode(token.handle_bytes()).map_err(|_| {
                    CpError::Authorisation {
                        operation: ControlOp::TopologyAck as u8,
                    }
                })?;
                let _ = self.validate_topology_ack_with_handle(rv_id, cp_lane, handle, None)?;
            }
            ControlOp::TopologyCommit => {
                let handle = TopologyHandle::decode(token.handle_bytes()).map_err(|_| {
                    CpError::Authorisation {
                        operation: ControlOp::TopologyCommit as u8,
                    }
                })?;
                let _ = self.validate_topology_commit_with_handle(rv_id, cp_lane, handle, None)?;
            }
            ControlOp::AbortBegin => {
                let handle = decode_session_lane_handle(token.handle_bytes()).map_err(|_| {
                    CpError::Authorisation {
                        operation: ControlOp::AbortBegin as u8,
                    }
                })?;
                Self::validate_session_lane_handle(cp_sid, cp_lane, handle, ControlOp::AbortBegin)?;
            }
            ControlOp::AbortAck => {
                let handle = decode_session_lane_handle(token.handle_bytes()).map_err(|_| {
                    CpError::Authorisation {
                        operation: ControlOp::AbortAck as u8,
                    }
                })?;
                Self::validate_session_lane_handle(cp_sid, cp_lane, handle, ControlOp::AbortAck)?;
                self.require_local_lane_generation(
                    rv_id,
                    cp_lane,
                    Generation::new(expected_epoch),
                )?;
            }
            ControlOp::StateSnapshot => {
                let handle = decode_session_lane_handle(token.handle_bytes()).map_err(|_| {
                    CpError::Authorisation {
                        operation: ControlOp::StateSnapshot as u8,
                    }
                })?;
                Self::validate_session_lane_handle(
                    cp_sid,
                    cp_lane,
                    handle,
                    ControlOp::StateSnapshot,
                )?;
                self.require_local_lane_generation(
                    rv_id,
                    cp_lane,
                    Generation::new(expected_epoch),
                )?;
            }
            ControlOp::TxCommit => {
                let handle = decode_session_lane_handle(token.handle_bytes()).map_err(|_| {
                    CpError::Authorisation {
                        operation: ControlOp::TxCommit as u8,
                    }
                })?;
                Self::validate_session_lane_handle(cp_sid, cp_lane, handle, ControlOp::TxCommit)?;
                Self::require_generation(
                    self.local_snapshot_generation_for_commit(rv_id, cp_lane)?,
                    Generation::new(expected_epoch),
                )?;
            }
            ControlOp::TxAbort => {
                let handle = decode_session_lane_handle(token.handle_bytes()).map_err(|_| {
                    CpError::Authorisation {
                        operation: ControlOp::TxAbort as u8,
                    }
                })?;
                Self::validate_session_lane_handle(cp_sid, cp_lane, handle, ControlOp::TxAbort)?;
                Self::require_generation(
                    self.local_snapshot_generation_for_abort(rv_id, cp_lane)?,
                    Generation::new(expected_epoch),
                )?;
            }
            ControlOp::StateRestore => {
                let handle = decode_session_lane_handle(token.handle_bytes()).map_err(|_| {
                    CpError::Authorisation {
                        operation: ControlOp::StateRestore as u8,
                    }
                })?;
                Self::validate_session_lane_handle(
                    cp_sid,
                    cp_lane,
                    handle,
                    ControlOp::StateRestore,
                )?;
                Self::require_generation(
                    self.local_snapshot_generation_for_restore(rv_id, cp_lane)?,
                    Generation::new(expected_epoch),
                )?;
            }
            ControlOp::Fence
            | ControlOp::RouteDecision
            | ControlOp::LoopContinue
            | ControlOp::LoopBreak => {}
            ControlOp::CapDelegate => {
                return Err(CpError::UnsupportedEffect(ControlOp::CapDelegate as u8));
            }
        }

        Ok(())
    }

    pub(crate) fn dispatch_descriptor_control_frame(
        &self,
        rv_id: RendezvousId,
        bytes: [u8; CAP_TOKEN_LEN],
        desc: ControlDesc,
        expected_scope_id: u16,
        expected_epoch: u16,
        generation: Option<Generation>,
    ) -> Result<(), CpError> {
        let _ = self.get_local(&rv_id).ok_or(CpError::RendezvousMismatch {
            expected: rv_id.raw(),
            actual: 0,
        })?;
        self.validate_descriptor_control_frame(bytes, desc, expected_scope_id, expected_epoch)?;
        let generation =
            Self::descriptor_dispatch_generation(desc.op(), expected_epoch, generation)?;
        let token = GenericCapToken::<()>::from_bytes(bytes);
        let header = token.control_header().map_err(|_| CpError::Authorisation {
            operation: desc.op() as u8,
        })?;

        let cp_sid = header.sid();
        let cp_lane = header.lane();
        match desc.op() {
            ControlOp::TopologyBegin => {
                let handle = TopologyHandle::decode(token.handle_bytes()).map_err(|_| {
                    CpError::Authorisation {
                        operation: ControlOp::TopologyBegin as u8,
                    }
                })?;
                self.dispatch_topology_begin_with_handle(
                    rv_id, cp_sid, cp_lane, handle, generation,
                )?;
            }
            ControlOp::TopologyAck => {
                let handle = TopologyHandle::decode(token.handle_bytes()).map_err(|_| {
                    CpError::Authorisation {
                        operation: ControlOp::TopologyAck as u8,
                    }
                })?;
                self.dispatch_topology_ack_with_handle(rv_id, cp_sid, cp_lane, handle, generation)?;
            }
            ControlOp::AbortBegin => {
                let handle = decode_session_lane_handle(token.handle_bytes()).map_err(|_| {
                    CpError::Authorisation {
                        operation: ControlOp::AbortBegin as u8,
                    }
                })?;
                self.dispatch_abort_begin_with_handle(rv_id, cp_sid, cp_lane, handle, generation)?;
            }
            ControlOp::AbortAck => {
                let handle = decode_session_lane_handle(token.handle_bytes()).map_err(|_| {
                    CpError::Authorisation {
                        operation: ControlOp::AbortAck as u8,
                    }
                })?;
                self.dispatch_abort_ack_with_handle(rv_id, cp_sid, cp_lane, handle, generation)?;
            }
            ControlOp::StateSnapshot => {
                let handle = decode_session_lane_handle(token.handle_bytes()).map_err(|_| {
                    CpError::Authorisation {
                        operation: ControlOp::StateSnapshot as u8,
                    }
                })?;
                self.dispatch_state_snapshot_with_handle(
                    rv_id, cp_sid, cp_lane, handle, generation,
                )?;
            }
            ControlOp::TxCommit => {
                let handle = decode_session_lane_handle(token.handle_bytes()).map_err(|_| {
                    CpError::Authorisation {
                        operation: ControlOp::TxCommit as u8,
                    }
                })?;
                self.dispatch_tx_commit_with_handle(rv_id, cp_sid, cp_lane, handle, generation)?;
            }
            ControlOp::TxAbort => {
                let handle = decode_session_lane_handle(token.handle_bytes()).map_err(|_| {
                    CpError::Authorisation {
                        operation: ControlOp::TxAbort as u8,
                    }
                })?;
                self.dispatch_tx_abort_with_handle(rv_id, cp_sid, cp_lane, handle, generation)?;
            }
            ControlOp::StateRestore => {
                let handle = decode_session_lane_handle(token.handle_bytes()).map_err(|_| {
                    CpError::Authorisation {
                        operation: desc.op() as u8,
                    }
                })?;
                self.dispatch_state_restore_with_handle(
                    rv_id, cp_sid, cp_lane, handle, generation,
                )?;
            }
            ControlOp::Fence
            | ControlOp::RouteDecision
            | ControlOp::LoopContinue
            | ControlOp::LoopBreak => {}
            ControlOp::CapDelegate => {
                return Err(CpError::UnsupportedEffect(ControlOp::CapDelegate as u8));
            }
            ControlOp::TopologyCommit => {
                let handle = TopologyHandle::decode(token.handle_bytes()).map_err(|_| {
                    CpError::Authorisation {
                        operation: ControlOp::TopologyCommit as u8,
                    }
                })?;
                self.dispatch_topology_commit_with_handle(
                    rv_id, cp_sid, cp_lane, handle, generation,
                )?;
            }
        }
        Ok(())
    }

    /// Initialize session effects from global protocol projection.
    ///
    /// This method wires the precompiled EffectEnvelope (owned by CompiledProgram) into
    /// the Rendezvous control-plane state. The envelope contains:
    /// - Control-plane effects (ControlOp) to pre-configure
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
    /// * `effect_envelope` - Crate-private effect facts for the projected program
    ///
    /// # Errors
    ///
    /// Returns `CpError::RendezvousMismatch` if the Rendezvous ID is not registered.
    fn init_session_effects_for_lane(
        rv: &mut crate::rendezvous::core::Rendezvous<'_, 'cfg, T, U, C>,
        sid: SessionId,
        lane: Lane,
        effect_envelope: EffectEnvelopeRef<'_>,
    ) -> Result<(), CpError> {
        let mut has_resources = false;
        let effects_already_installed = effect_envelope.resources().all(|descriptor| {
            has_resources = true;
            rv.policy(lane, descriptor.eff_index(), descriptor.tag())
                == Some(effect_envelope.resource_policy(descriptor))
        });
        if has_resources && effects_already_installed {
            return Ok(());
        }

        rv.reset_policy(lane);
        let mut control_marker_count = 0u32;
        for scope_kind in effect_envelope.control_scopes() {
            if matches!(scope_kind, ControlScopeKind::Topology) {
                rv.prepare_topology_control_scope(lane)
                    .ok_or(CpError::ResourceExhausted)?;
            } else {
                rv.initialise_control_scope(lane, scope_kind);
            }
            control_marker_count = control_marker_count.saturating_add(1);
        }

        let mut applied_effects = 0u32;
        let mut resource_events = 0u32;
        for descriptor in effect_envelope.resources() {
            resource_events = resource_events.saturating_add(1);
            rv.register_policy(
                lane,
                descriptor.eff_index(),
                descriptor.tag(),
                effect_envelope.resource_policy(descriptor),
            )?;
        }

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
            ControlOp::TopologyBegin => {
                let Some(operands) = envelope.topology else {
                    return Ok(PendingEffect::None);
                };
                let sid = envelope
                    .sid
                    .ok_or(CpError::Topology(TopologyError::InvalidSession))?;
                self.with_control_mut(|core| {
                    let (_intent, _ack) = core
                        .topology_state
                        .begin(sid, operands)
                        .expect("topology begin bookkeeping was preflighted before local mutation");
                    Ok(PendingEffect::None)
                })
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
    #[inline(never)]
    fn attach_secondary_endpoint_lanes<'lease, const ROLE: u8, Mint, B>(
        &'lease self,
        dst: *mut crate::endpoint::kernel::CursorEndpoint<
            'lease,
            ROLE,
            T,
            U,
            C,
            crate::control::cap::mint::EpochTbl,
            MAX_RV,
            Mint,
            B,
        >,
        rv_id: RendezvousId,
        sid: SessionId,
        role_count: u8,
        effect_envelope: EffectEnvelopeRef<'_>,
        role_image: RoleImageSlice<ROLE>,
        logical_lane_count: usize,
        occupied_lane_index: usize,
    ) -> Result<(), AttachError>
    where
        'cfg: 'lease,
        B: crate::binding::BindingSlot,
        Mint: crate::control::cap::mint::MintConfigMarker,
    {
        let mut logical_idx = 0usize;
        while logical_idx < logical_lane_count {
            if logical_idx == occupied_lane_index || !role_image.has_active_lane(logical_idx) {
                logical_idx += 1;
                continue;
            }

            let physical_lane = Lane::new(logical_idx as u32);
            let mut lease = self
                .lease_port(rv_id, sid, physical_lane, ROLE, role_count)
                .map_err(AttachError::from)?;
            lease.with_rendezvous_mut(|rv| -> Result<(), AttachError> {
                rv.activate_lane_attachment(sid, physical_lane)
                    .map_err(AttachError::from)?;
                Self::init_session_effects_for_lane(rv, sid, physical_lane, effect_envelope)
                    .map_err(AttachError::from)
            })?;
            let (port, guard, _brand) = lease.into_port_guard().map_err(AttachError::from)?;
            unsafe {
                crate::endpoint::kernel::endpoint_init::write_port_slot(dst, logical_idx, port);
                crate::endpoint::kernel::endpoint_init::write_guard_slot(dst, logical_idx, guard);
            }
            logical_idx += 1;
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    #[inline(never)]
    unsafe fn init_endpoint_with_compiled_into<'r, const ROLE: u8, Mint, B>(
        &'r self,
        dst: *mut crate::endpoint::kernel::CursorEndpoint<
            'r,
            ROLE,
            T,
            U,
            C,
            crate::control::cap::mint::EpochTbl,
            MAX_RV,
            Mint,
            B,
        >,
        arena_storage: *mut u8,
        rv_id: RendezvousId,
        sid: SessionId,
        role_image: RoleImageSlice<ROLE>,
        public_slot: EndpointLeaseId,
        public_generation: u32,
        public_slot_owned: bool,
        public_revoke: crate::endpoint::kernel::PublicEndpointRevoke,
        mint: Mint,
        binding_enabled: bool,
        binding: B,
    ) -> Result<(), AttachError>
    where
        'cfg: 'r,
        B: crate::binding::BindingSlot,
        Mint: crate::control::cap::mint::MintConfigMarker,
    {
        let program_image = role_image.program();
        let effect_envelope = program_image.effect_envelope();
        let role_count = core::cmp::min(program_image.role_count(), u8::MAX as usize) as u8;
        let logical_lane_count = role_image.logical_lane_count().max(1);
        let primary_lane_index = role_image.first_active_lane().unwrap_or(0usize);
        debug_assert!(primary_lane_index < logical_lane_count);
        let control_lane_index = 0usize;
        let control_wire_lane = Lane::new(control_lane_index as u32);
        let mut control_lease = self
            .lease_port(rv_id, sid, control_wire_lane, ROLE, role_count)
            .map_err(AttachError::from)?;
        control_lease.with_rendezvous_mut(|rv| -> Result<(), AttachError> {
            rv.activate_lane_attachment(sid, control_wire_lane)
                .map_err(AttachError::from)?;
            Self::init_session_effects_for_lane(rv, sid, control_wire_lane, effect_envelope)
                .map_err(AttachError::from)
        })?;
        let (control_port, control_guard, control_brand) =
            control_lease.into_port_guard().map_err(AttachError::from)?;
        let owner: crate::control::cap::mint::Owner<'r, crate::control::cap::mint::E0> = unsafe {
            core::mem::transmute(crate::control::cap::mint::Owner::<
                'cfg,
                crate::control::cap::mint::E0,
            >::new(control_brand))
        };
        let epoch = crate::control::cap::mint::EndpointEpoch::new();
        let liveness_policy = self.with_control_mut(|core| {
            core.locals
                .get_mut(&rv_id)
                .map(|rv| rv.liveness_policy())
                .unwrap_or_default()
        });
        let control: crate::endpoint::control::SessionControlCtx<
            'r,
            T,
            U,
            C,
            crate::control::cap::mint::EpochTbl,
            MAX_RV,
        > = unsafe {
            core::mem::transmute(crate::endpoint::control::SessionControlCtx::<
                'cfg,
                T,
                U,
                C,
                crate::control::cap::mint::EpochTbl,
                MAX_RV,
            >::new(
                control_wire_lane,
                Some(&*(self as *const Self)),
                liveness_policy,
                None,
            ))
        };

        unsafe {
            crate::endpoint::kernel::endpoint_init::init_empty_from_compiled(
                dst,
                arena_storage,
                primary_lane_index,
                sid,
                owner,
                epoch,
                role_image.compiled_ptr(),
                program_image,
                rv_id,
                public_slot,
                public_generation,
                public_slot_owned,
                public_revoke,
                liveness_policy,
                control,
                mint,
                binding_enabled,
                binding,
            );
            crate::endpoint::kernel::endpoint_init::write_port_slot(
                dst,
                control_lane_index,
                control_port,
            );
            crate::endpoint::kernel::endpoint_init::write_guard_slot(
                dst,
                control_lane_index,
                control_guard,
            );
        }
        let init_result = self.attach_secondary_endpoint_lanes::<ROLE, Mint, B>(
            dst,
            rv_id,
            sid,
            role_count,
            effect_envelope,
            role_image,
            logical_lane_count,
            control_lane_index,
        );

        if let Err(err) = init_result {
            unsafe {
                core::ptr::drop_in_place(dst);
            }
            return Err(err);
        }

        unsafe {
            crate::endpoint::kernel::endpoint_init::finish_init(dst);
        }
        Ok(())
    }

    #[inline]
    pub(crate) fn attach_public_endpoint<'r, const ROLE: u8>(
        &'r self,
        rv_id: RendezvousId,
        sid: SessionId,
        program: &crate::g::advanced::RoleProgram<ROLE>,
        binding: crate::binding::BindingHandle<'r>,
    ) -> Result<(EndpointLeaseId, u32), AttachError>
    where
        'cfg: 'r,
    {
        self.attach_public_endpoint_inner(rv_id, sid, program, binding)
    }

    #[inline]
    fn role_image_attaches_lane<const ROLE: u8>(
        role_image: RoleImageSlice<ROLE>,
        lane: Lane,
    ) -> bool {
        let lane_idx = lane.raw() as usize;
        lane_idx == 0
            || (lane_idx < role_image.logical_lane_count() && role_image.has_active_lane(lane_idx))
    }

    #[inline]
    fn attach_public_endpoint_inner<'r, 'prog, const ROLE: u8, P>(
        &'r self,
        rv_id: RendezvousId,
        sid: SessionId,
        program: &P,
        binding: crate::binding::BindingHandle<'r>,
    ) -> Result<(EndpointLeaseId, u32), AttachError>
    where
        P: crate::global::RoleProgramView<ROLE>,
        'cfg: 'r,
    {
        self.with_control_mut(|core| {
            let topology_phase = core.topology_state.phase(sid);

            if topology_phase.is_some() {
                let operands =
                    core.topology_state
                        .get(sid)
                        .copied()
                        .ok_or(AttachError::Control(CpError::Topology(
                            TopologyError::InvalidSession,
                        )))?;
                if operands.src_rv == rv_id || operands.dst_rv == rv_id {
                    Self::abort_inflight_topology_entry(core, sid, operands.src_rv)
                        .map_err(AttachError::Control)?;
                }
                return Err(AttachError::Control(CpError::Topology(
                    TopologyError::InvalidState,
                )));
            }

            let rv = core.locals.get_mut(&rv_id).ok_or(AttachError::Control(
                CpError::RendezvousMismatch {
                    expected: rv_id.raw(),
                    actual: 0,
                },
            ))?;
            let topology_session_state = rv.topology_session_state(sid);
            match topology_session_state {
                None | Some(TopologySessionState::DestinationAttachReady { .. }) => {}
                Some(TopologySessionState::SourcePending { .. }) => {
                    return Err(AttachError::Control(CpError::Topology(
                        TopologyError::InvalidState,
                    )));
                }
                Some(TopologySessionState::DestinationPending { .. }) => {
                    rv.rollback_destination_topology_prepare(sid)
                        .map_err(|err| AttachError::Control(CpError::Topology(err.into())))?;
                }
            }
            Ok(())
        })?;

        match self.ensure_role_image_slice(rv_id, program) {
            Ok(role_image) => unsafe {
                self.with_control_mut(|core| {
                    let topology_session_state = core
                        .locals
                        .get_mut(&rv_id)
                        .ok_or(AttachError::Control(CpError::RendezvousMismatch {
                            expected: rv_id.raw(),
                            actual: 0,
                        }))?
                        .topology_session_state(sid);

                    if let Some(TopologySessionState::DestinationAttachReady { lane }) =
                        topology_session_state
                        && !Self::role_image_attaches_lane(role_image, lane)
                    {
                        return Err(AttachError::Control(CpError::Topology(
                            TopologyError::InvalidState,
                        )));
                    }

                    Ok(())
                })?;
                let binding_enabled = binding.uses_binding_storage();
                let storage_layout =
                    Self::public_endpoint_storage_requirement(role_image, binding_enabled);
                let resident_budget = Self::public_endpoint_resident_budget(role_image);
                let (slot, generation, dst) = match self
                    .allocate_public_endpoint_storage_for_rv::<
                        ROLE,
                        crate::control::cap::mint::MintConfig,
                    >(
                        rv_id,
                        storage_layout.total_bytes,
                        storage_layout.total_align,
                        resident_budget,
                    ) {
                    Some(parts) => parts,
                    None => return Err(AttachError::Control(CpError::ResourceExhausted)),
                };
                let arena_storage = dst.cast::<u8>().add(storage_layout.arena_offset);
                if let Err(err) = self.init_endpoint_with_compiled_into::<
                    ROLE,
                    crate::control::cap::mint::MintConfig,
                    crate::binding::BindingHandle<'r>,
                >(
                    dst,
                    arena_storage,
                    rv_id,
                    sid,
                    role_image,
                    slot,
                    generation,
                    true,
                    crate::endpoint::kernel::PublicEndpointRevoke::new(
                        Self::revoke_public_endpoint::<ROLE>,
                    ),
                    crate::control::cap::mint::MintConfig::INSTANCE,
                    binding_enabled,
                    binding,
                ) {
                    self.with_control_mut(|core| {
                        if let Some(rv) = core.locals.get_mut(&rv_id) {
                            rv.release_endpoint_lease(slot, generation);
                        }
                    });
                    return Err(err);
                }
                if let Err(err) = self.mark_public_endpoint_lease(rv_id, slot, generation) {
                    core::ptr::drop_in_place(dst);
                    return Err(err);
                }
                if let Err(err) = self.pin_compiled_images_for_public_endpoint::<ROLE>(
                    rv_id,
                    slot,
                    generation,
                    role_image.program(),
                ) {
                    core::ptr::drop_in_place(dst);
                    return Err(err);
                }
                Ok((slot, generation))
            },
            Err(err) => Err(err),
        }
    }

    #[cfg(test)]
    pub(crate) unsafe fn attach_endpoint_into<'r, 'prog, const ROLE: u8, P, Mint, B>(
        &'r self,
        dst: *mut crate::endpoint::kernel::CursorEndpoint<
            'r,
            ROLE,
            T,
            U,
            C,
            crate::control::cap::mint::EpochTbl,
            MAX_RV,
            Mint,
            B,
        >,
        rv_id: RendezvousId,
        sid: SessionId,
        program: &P,
        binding: B,
    ) -> Result<(), AttachError>
    where
        'cfg: 'r,
        B: crate::binding::BindingSlot,
        Mint: crate::control::cap::mint::MintConfigMarker,
        P: crate::global::RoleProgramView<ROLE>,
    {
        let role_image = self.ensure_role_image_slice::<ROLE, _>(rv_id, program)?;
        let binding_enabled = true;
        let resident_budget = Self::public_endpoint_resident_budget(role_image);
        let arena_layout = role_image.endpoint_arena_layout_for_binding(binding_enabled);
        let Some((slot, generation, arena_storage)) = self.allocate_storage_for_rv(
            rv_id,
            arena_layout.total_bytes(),
            arena_layout.total_align(),
            resident_budget,
        ) else {
            return Err(AttachError::Control(CpError::ResourceExhausted));
        };
        let init_result = unsafe {
            self.init_endpoint_with_compiled_into::<ROLE, Mint, B>(
                dst,
                arena_storage,
                rv_id,
                sid,
                role_image,
                slot,
                generation,
                true,
                crate::endpoint::kernel::PublicEndpointRevoke::UNARMED,
                Mint::INSTANCE,
                binding_enabled,
                binding,
            )
        };
        if let Err(err) = init_result {
            self.with_control_mut(|core| {
                if let Some(rv) = core.locals.get_mut(&rv_id) {
                    rv.release_endpoint_lease(slot, generation);
                }
            });
            return Err(err);
        }
        Ok(())
    }

    #[inline]
    pub(crate) fn enter<'r, const ROLE: u8>(
        &'r self,
        rv_id: RendezvousId,
        sid: SessionId,
        program: &crate::g::advanced::RoleProgram<ROLE>,
        binding: crate::binding::BindingHandle<'r>,
    ) -> Result<(EndpointLeaseId, u32), AttachError>
    where
        'cfg: 'r,
    {
        self.enter_with_binding::<ROLE>(rv_id, sid, program, binding)
    }

    #[inline]
    fn enter_with_binding<'r, const ROLE: u8>(
        &'r self,
        rv_id: RendezvousId,
        sid: SessionId,
        program: &crate::g::advanced::RoleProgram<ROLE>,
        binding: crate::binding::BindingHandle<'r>,
    ) -> Result<(EndpointLeaseId, u32), AttachError>
    where
        'cfg: 'r,
    {
        self.attach_public_endpoint::<ROLE>(rv_id, sid, program, binding)
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

    /// Drive a delegation automaton rooted at `rv_id` using a LeaseGraph.
    ///
    /// The `root_builder` closure constructs the root facet for the graph from the
    /// rendezvous lease, allowing callers to choose the facet bundle used to seed
    /// the graph (e.g. slot/caps/topology facets). The automaton receives both the
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
            let graph_ptr =
                match unsafe { Self::transient_graph_storage_ptr::<A::GraphSpec>(core, rv_id) } {
                    Ok(graph_ptr) => graph_ptr,
                    Err(err) => return Err(DelegationDriveError::Graph(err)),
                };
            unsafe {
                LeaseGraph::<A::GraphSpec>::init_new(
                    graph_ptr,
                    rv_id,
                    <A::GraphSpec as LeaseSpec>::Facet::default(),
                    root_context,
                );
            }

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
    extern crate self as hibana;
    use crate::control::cap::atomic_codecs::{
        TAG_CAP_DELEGATE_CONTROL, TAG_TOPOLOGY_BEGIN_CONTROL, encode_session_lane_handle,
        mint_session_lane_handle,
    };

    mod fanout_program {
        extern crate self as hibana;
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/internal/pico_smoke/src/fanout_program.rs"
        ));
    }
    mod huge_program {
        extern crate self as hibana;
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/internal/pico_smoke/src/huge_program.rs"
        ));
    }
    mod linear_program {
        extern crate self as hibana;
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/internal/pico_smoke/src/linear_program.rs"
        ));
    }
    mod localside {
        extern crate self as hibana;
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/internal/pico_smoke/src/localside.rs"
        ));
    }
    mod route_localside {
        extern crate self as hibana;
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/internal/pico_smoke/src/route_localside.rs"
        ));
    }
    mod route_control_kinds {
        extern crate self as hibana;
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/internal/pico_smoke/src/route_control_kinds.rs"
        ));
    }

    fn drive<F: core::future::Future>(future: F) -> F::Output {
        futures::executor::block_on(future)
    }

    use crate::control::cap::mint::{
        CAP_HANDLE_LEN, CAP_HEADER_LEN, CAP_NONCE_LEN, CAP_TAG_LEN, CAP_TOKEN_LEN, CapError,
        CapHeader, CapShot, ControlPath, ControlResourceKind, GenericCapToken, MintConfig,
        ResourceKind,
    };
    use crate::control::cap::resource_kinds::{RouteArmHandle, RouteDecisionKind};
    use crate::control::types::{Generation, Lane, SessionId};
    use crate::g::{self, Msg, Role};
    use crate::global::compiled::lowering::LoweringSummary;
    use crate::global::program::Program;
    use crate::global::role_program;
    use crate::global::steps::{PolicySteps, SendStep, StepCons, StepNil};
    use crate::observe::core::TapEvent;
    use crate::runtime::config::{Config, CounterClock};
    use crate::runtime::consts::{DefaultLabelUniverse, LABEL_ROUTE_DECISION, RING_EVENTS};
    use crate::transport::{Transport, TransportError, wire::Payload};
    use core::mem::size_of;
    use core::{cell::UnsafeCell, mem::MaybeUninit};
    use std::{string::String, thread_local};

    #[test]
    fn resolver_ref_from_state_dispatches_borrowed_state() {
        #[derive(Clone, Copy)]
        struct RouteState {
            preferred_arm: u8,
        }

        fn route_resolver(
            state: &RouteState,
            _ctx: ResolverContext,
        ) -> Result<DynamicResolution, ResolverError> {
            Ok(DynamicResolution::RouteArm {
                arm: state.preferred_arm,
            })
        }

        let state = RouteState { preferred_arm: 7 };
        let resolver = ResolverRef::from_state(&state, route_resolver);
        let ctx = ResolverContext::new(
            RendezvousId::new(1),
            Some(SessionId::new(9)),
            Lane::new(3),
            EffIndex::new(2),
            0x40,
            ScopeId::none(),
            None,
            [11, 22, 33, 44],
            &crate::transport::context::PolicyAttrs::EMPTY,
        );

        assert_eq!(
            resolver.resolve(ctx),
            Ok(DynamicResolution::RouteArm { arm: 7 })
        );
    }

    type SharedBorrowSteps = StepCons<
        SendStep<
            Role<0>,
            Role<0>,
            Msg<{ LABEL_ROUTE_DECISION }, GenericCapToken<RouteDecisionKind>, RouteDecisionKind>,
            0,
        >,
        StepNil,
    >;

    type SharedBorrowProgram = Program<SharedBorrowSteps>;
    type SharedBorrowPolicyProgram<const POLICY_ID: u16> =
        Program<PolicySteps<SharedBorrowSteps, POLICY_ID>>;
    type SharedBorrowRoleProgram = crate::g::advanced::RoleProgram<0>;

    fn shared_borrow_program_a() -> SharedBorrowProgram {
        g::send::<
            Role<0>,
            Role<0>,
            Msg<{ LABEL_ROUTE_DECISION }, GenericCapToken<RouteDecisionKind>, RouteDecisionKind>,
            0,
        >()
    }
    fn shared_borrow_program_b() -> SharedBorrowProgram {
        g::send::<
            Role<0>,
            Role<0>,
            Msg<{ LABEL_ROUTE_DECISION }, GenericCapToken<RouteDecisionKind>, RouteDecisionKind>,
            0,
        >()
    }
    const ROUTE_POLICY_ONE: u16 = 9901;
    const ROUTE_POLICY_TWO: u16 = 9902;

    fn route_policy_program_one() -> SharedBorrowPolicyProgram<ROUTE_POLICY_ONE> {
        g::send::<
            Role<0>,
            Role<0>,
            Msg<{ LABEL_ROUTE_DECISION }, GenericCapToken<RouteDecisionKind>, RouteDecisionKind>,
            0,
        >()
        .policy::<ROUTE_POLICY_ONE>()
    }
    fn route_policy_program_two() -> SharedBorrowPolicyProgram<ROUTE_POLICY_TWO> {
        g::send::<
            Role<0>,
            Role<0>,
            Msg<{ LABEL_ROUTE_DECISION }, GenericCapToken<RouteDecisionKind>, RouteDecisionKind>,
            0,
        >()
        .policy::<ROUTE_POLICY_TWO>()
    }
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
        type Metrics = ();

        fn open<'a>(&'a self, _local_role: u8, _session_id: u32) -> (Self::Tx<'a>, Self::Rx<'a>) {
            ((), ())
        }

        fn poll_send<'a, 'f>(
            &'a self,
            _tx: &'a mut Self::Tx<'a>,
            _outgoing: crate::transport::Outgoing<'f>,
            _cx: &mut core::task::Context<'_>,
        ) -> core::task::Poll<Result<(), Self::Error>>
        where
            'a: 'f,
        {
            core::task::Poll::Ready(Ok(()))
        }

        fn poll_recv<'a>(
            &'a self,
            _rx: &'a mut Self::Rx<'a>,
            _cx: &mut core::task::Context<'_>,
        ) -> core::task::Poll<Result<Payload<'a>, Self::Error>> {
            core::task::Poll::Ready(Err(TransportError::Failed))
        }

        fn cancel_send<'a>(&'a self, _tx: &'a mut Self::Tx<'a>) {}

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

    fn retain_pico_smoke_fixture_symbols() {
        let _ = fanout_program::ROUTE_SCOPE_COUNT;
        let _ = fanout_program::EXPECTED_WORKER_BRANCH_LABELS;
        let _ = fanout_program::ACK_LABELS;
        let _ = huge_program::ROUTE_SCOPE_COUNT;
        let _ = huge_program::EXPECTED_WORKER_BRANCH_LABELS;
        let _ = huge_program::ACK_LABELS;
        let _ = linear_program::ROUTE_SCOPE_COUNT;
        let _ = linear_program::EXPECTED_WORKER_BRANCH_LABELS;
        let _ = linear_program::ACK_LABELS;
        let _ = huge_program::run
            as fn(&mut localside::ControllerEndpoint<'_>, &mut localside::WorkerEndpoint<'_>);
        let _ = huge_program::controller_program as fn() -> role_program::RoleProgram<0>;
        let _ = linear_program::run
            as fn(&mut localside::ControllerEndpoint<'_>, &mut localside::WorkerEndpoint<'_>);
        let _ = linear_program::controller_program as fn() -> role_program::RoleProgram<0>;
        let _ = fanout_program::run
            as fn(&mut localside::ControllerEndpoint<'_>, &mut localside::WorkerEndpoint<'_>);
        let _ = fanout_program::controller_program as fn() -> role_program::RoleProgram<0>;
        let _ =
            localside::worker_offer_decode_u8::<0> as fn(&mut localside::WorkerEndpoint<'_>) -> u8;
    }

    #[test]
    fn pico_smoke_fixture_symbols_are_reachable() {
        retain_pico_smoke_fixture_symbols();
    }

    fn route_decision_header(scope_id: u16, epoch: u16, flags: u8) -> (ControlDesc, CapHeader) {
        let desc = ControlDesc::of::<RouteDecisionKind>();
        let handle = RouteArmHandle {
            scope: ScopeId::route(scope_id),
            arm: 1,
        };
        (
            desc,
            CapHeader::new(
                SessionId::new(7),
                Lane::new(0),
                0,
                desc.resource_tag(),
                desc.label(),
                desc.op(),
                desc.path(),
                desc.shot(),
                desc.scope_kind(),
                flags,
                scope_id,
                epoch,
                RouteDecisionKind::encode_handle(&handle),
            ),
        )
    }

    struct LocalAbortAckControl;

    impl ResourceKind for LocalAbortAckControl {
        type Handle = SessionLaneHandle;
        const TAG: u8 = 0xA0;
        const NAME: &'static str = "LocalAbortAckControl";

        fn encode_handle(handle: &Self::Handle) -> [u8; CAP_HANDLE_LEN] {
            encode_session_lane_handle(*handle)
        }

        fn decode_handle(data: [u8; CAP_HANDLE_LEN]) -> Result<Self::Handle, CapError> {
            decode_session_lane_handle(data)
        }

        fn zeroize(_handle: &mut Self::Handle) {}
    }

    impl ControlResourceKind for LocalAbortAckControl {
        const LABEL: u8 = 0xA0;
        const SCOPE: ControlScopeKind = ControlScopeKind::Abort;
        const PATH: ControlPath = ControlPath::Local;
        const TAP_ID: u16 = crate::observe::ids::ABORT_ACK;
        const SHOT: CapShot = CapShot::One;
        const OP: ControlOp = ControlOp::AbortAck;
        const AUTO_MINT_WIRE: bool = false;

        fn mint_handle(
            sid: SessionId,
            lane: Lane,
            _scope: ScopeId,
        ) -> <Self as ResourceKind>::Handle {
            mint_session_lane_handle(sid, lane)
        }
    }

    struct LocalStateSnapshotControl;

    impl ResourceKind for LocalStateSnapshotControl {
        type Handle = SessionLaneHandle;
        const TAG: u8 = 0xA5;
        const NAME: &'static str = "LocalStateSnapshotControl";

        fn encode_handle(handle: &Self::Handle) -> [u8; CAP_HANDLE_LEN] {
            encode_session_lane_handle(*handle)
        }

        fn decode_handle(data: [u8; CAP_HANDLE_LEN]) -> Result<Self::Handle, CapError> {
            decode_session_lane_handle(data)
        }

        fn zeroize(_handle: &mut Self::Handle) {}
    }

    impl ControlResourceKind for LocalStateSnapshotControl {
        const LABEL: u8 = 0xA5;
        const SCOPE: ControlScopeKind = ControlScopeKind::State;
        const PATH: ControlPath = ControlPath::Local;
        const TAP_ID: u16 = crate::observe::ids::STATE_SNAPSHOT_REQ;
        const SHOT: CapShot = CapShot::One;
        const OP: ControlOp = ControlOp::StateSnapshot;
        const AUTO_MINT_WIRE: bool = false;

        fn mint_handle(
            sid: SessionId,
            lane: Lane,
            _scope: ScopeId,
        ) -> <Self as ResourceKind>::Handle {
            mint_session_lane_handle(sid, lane)
        }
    }

    struct LocalStateRestoreControl;

    impl ResourceKind for LocalStateRestoreControl {
        type Handle = SessionLaneHandle;
        const TAG: u8 = 0xA1;
        const NAME: &'static str = "LocalStateRestoreControl";

        fn encode_handle(handle: &Self::Handle) -> [u8; CAP_HANDLE_LEN] {
            encode_session_lane_handle(*handle)
        }

        fn decode_handle(data: [u8; CAP_HANDLE_LEN]) -> Result<Self::Handle, CapError> {
            decode_session_lane_handle(data)
        }

        fn zeroize(_handle: &mut Self::Handle) {}
    }

    impl ControlResourceKind for LocalStateRestoreControl {
        const LABEL: u8 = 0xA1;
        const SCOPE: ControlScopeKind = ControlScopeKind::State;
        const PATH: ControlPath = ControlPath::Local;
        const TAP_ID: u16 = crate::observe::ids::STATE_RESTORE_REQ;
        const SHOT: CapShot = CapShot::One;
        const OP: ControlOp = ControlOp::StateRestore;
        const AUTO_MINT_WIRE: bool = false;

        fn mint_handle(
            sid: SessionId,
            lane: Lane,
            _scope: ScopeId,
        ) -> <Self as ResourceKind>::Handle {
            mint_session_lane_handle(sid, lane)
        }
    }

    struct LocalTxCommitControl;

    impl ResourceKind for LocalTxCommitControl {
        type Handle = SessionLaneHandle;
        const TAG: u8 = 0xA2;
        const NAME: &'static str = "LocalTxCommitControl";

        fn encode_handle(handle: &Self::Handle) -> [u8; CAP_HANDLE_LEN] {
            encode_session_lane_handle(*handle)
        }

        fn decode_handle(data: [u8; CAP_HANDLE_LEN]) -> Result<Self::Handle, CapError> {
            decode_session_lane_handle(data)
        }

        fn zeroize(_handle: &mut Self::Handle) {}
    }

    impl ControlResourceKind for LocalTxCommitControl {
        const LABEL: u8 = 0xA2;
        const SCOPE: ControlScopeKind = ControlScopeKind::State;
        const PATH: ControlPath = ControlPath::Local;
        const TAP_ID: u16 = crate::observe::ids::POLICY_COMMIT;
        const SHOT: CapShot = CapShot::One;
        const OP: ControlOp = ControlOp::TxCommit;
        const AUTO_MINT_WIRE: bool = false;

        fn mint_handle(
            sid: SessionId,
            lane: Lane,
            _scope: ScopeId,
        ) -> <Self as ResourceKind>::Handle {
            mint_session_lane_handle(sid, lane)
        }
    }

    struct LocalTxAbortControl;

    impl ResourceKind for LocalTxAbortControl {
        type Handle = SessionLaneHandle;
        const TAG: u8 = 0xA3;
        const NAME: &'static str = "LocalTxAbortControl";

        fn encode_handle(handle: &Self::Handle) -> [u8; CAP_HANDLE_LEN] {
            encode_session_lane_handle(*handle)
        }

        fn decode_handle(data: [u8; CAP_HANDLE_LEN]) -> Result<Self::Handle, CapError> {
            decode_session_lane_handle(data)
        }

        fn zeroize(_handle: &mut Self::Handle) {}
    }

    impl ControlResourceKind for LocalTxAbortControl {
        const LABEL: u8 = 0xA3;
        const SCOPE: ControlScopeKind = ControlScopeKind::State;
        const PATH: ControlPath = ControlPath::Local;
        const TAP_ID: u16 = crate::observe::ids::POLICY_TX_ABORT;
        const SHOT: CapShot = CapShot::One;
        const OP: ControlOp = ControlOp::TxAbort;
        const AUTO_MINT_WIRE: bool = false;

        fn mint_handle(
            sid: SessionId,
            lane: Lane,
            _scope: ScopeId,
        ) -> <Self as ResourceKind>::Handle {
            mint_session_lane_handle(sid, lane)
        }
    }

    struct WireCapDelegateControl;

    impl ResourceKind for WireCapDelegateControl {
        type Handle = SessionLaneHandle;
        const TAG: u8 = 0xA4;
        const NAME: &'static str = "WireCapDelegateControl";

        fn encode_handle(handle: &Self::Handle) -> [u8; CAP_HANDLE_LEN] {
            encode_session_lane_handle(*handle)
        }

        fn decode_handle(data: [u8; CAP_HANDLE_LEN]) -> Result<Self::Handle, CapError> {
            decode_session_lane_handle(data)
        }

        fn zeroize(_handle: &mut Self::Handle) {}
    }

    impl ControlResourceKind for WireCapDelegateControl {
        const LABEL: u8 = 0xA4;
        const SCOPE: ControlScopeKind = ControlScopeKind::Delegate;
        const PATH: ControlPath = ControlPath::Wire;
        const TAP_ID: u16 = crate::observe::ids::DELEG_BEGIN;
        const SHOT: CapShot = CapShot::One;
        const OP: ControlOp = ControlOp::CapDelegate;
        const AUTO_MINT_WIRE: bool = false;

        fn mint_handle(
            sid: SessionId,
            lane: Lane,
            _scope: ScopeId,
        ) -> <Self as ResourceKind>::Handle {
            mint_session_lane_handle(sid, lane)
        }
    }

    type AttachRoleProgram = crate::g::advanced::RoleProgram<0>;
    fn attach_program() -> AttachRoleProgram {
        role_program::project(&g::send::<Role<0>, Role<1>, Msg<0x41, u8>, 0>())
    }

    type Lane1WorkerRoleProgram = crate::g::advanced::RoleProgram<1>;
    fn lane1_worker_program() -> Lane1WorkerRoleProgram {
        role_program::project(&g::send::<Role<1>, Role<0>, Msg<0x42, u8>, 1>())
    }

    fn attach_session_lane_for_program<const ROLE: u8>(
        cluster: &'static StaticTestCluster<4>,
        rv_id: RendezvousId,
        sid: SessionId,
        program: &crate::g::advanced::RoleProgram<ROLE>,
    ) -> ((EndpointLeaseId, u32), Lane) {
        let handle = cluster
            .enter(
                rv_id,
                sid,
                program,
                crate::binding::BindingHandle::None(crate::binding::NoBinding),
            )
            .expect("attach test endpoint");
        let lane = cluster
            .get_local(&rv_id)
            .expect("registered rendezvous")
            .session_lane(sid)
            .expect("attached session must own a lane");
        (handle, lane)
    }

    fn attach_session_lane(
        cluster: &'static StaticTestCluster<4>,
        rv_id: RendezvousId,
        sid: SessionId,
    ) -> ((EndpointLeaseId, u32), Lane) {
        attach_session_lane_for_program::<0>(cluster, rv_id, sid, &attach_program())
    }

    fn topology_handle(
        operands: TopologyOperands,
    ) -> crate::control::cap::atomic_codecs::TopologyHandle {
        crate::control::cap::atomic_codecs::TopologyHandle {
            src_rv: operands.src_rv.raw(),
            dst_rv: operands.dst_rv.raw(),
            src_lane: operands.src_lane.raw() as u16,
            dst_lane: operands.dst_lane.raw() as u16,
            old_gen: operands.old_gen.raw(),
            new_gen: operands.new_gen.raw(),
            seq_tx: operands.seq_tx,
            seq_rx: operands.seq_rx,
        }
    }

    fn advance_lane_generation(
        cluster: &'static StaticTestCluster<4>,
        rv_id: RendezvousId,
        lane: Lane,
        target: Generation,
    ) {
        cluster
            .get_local(&rv_id)
            .expect("registered rendezvous")
            .advance_lane_generation_for_test(lane, target);
    }

    fn session_lane_control_token<K: ControlResourceKind>(
        sid: SessionId,
        lane: Lane,
    ) -> [u8; CAP_TOKEN_LEN] {
        session_lane_control_token_with_epoch::<K>(sid, lane, 0)
    }

    fn session_lane_control_token_with_epoch<K: ControlResourceKind>(
        sid: SessionId,
        lane: Lane,
        epoch: u16,
    ) -> [u8; CAP_TOKEN_LEN] {
        let desc = ControlDesc::of::<K>();
        let handle = K::encode_handle(&K::mint_handle(sid, lane, ScopeId::none()));
        let mut header = [0u8; CAP_HEADER_LEN];
        CapHeader::new(
            sid,
            lane,
            0,
            desc.resource_tag(),
            desc.label(),
            desc.op(),
            desc.path(),
            desc.shot(),
            desc.scope_kind(),
            desc.header_flags(),
            0,
            epoch,
            handle,
        )
        .encode(&mut header);
        GenericCapToken::<()>::from_parts([0; CAP_NONCE_LEN], header, [0; CAP_TAG_LEN]).into_bytes()
    }

    #[inline]
    const fn pack_u16_pair(hi: u16, lo: u16) -> u32 {
        ((hi as u32) << 16) | lo as u32
    }

    struct DecodePoisonKind;

    impl ResourceKind for DecodePoisonKind {
        type Handle = ();
        const TAG: u8 = 0x7C;
        const NAME: &'static str = "DecodePoison";

        fn encode_handle(_handle: &Self::Handle) -> [u8; CAP_HANDLE_LEN] {
            [0; CAP_HANDLE_LEN]
        }

        fn decode_handle(_data: [u8; CAP_HANDLE_LEN]) -> Result<Self::Handle, CapError> {
            panic!("core auth must not decode the typed handle")
        }

        fn zeroize(_handle: &mut Self::Handle) {}
    }

    impl ControlResourceKind for DecodePoisonKind {
        const LABEL: u8 = 0x7C;
        const SCOPE: ControlScopeKind = ControlScopeKind::Route;
        const PATH: ControlPath = ControlPath::Local;
        const TAP_ID: u16 = 0x047C;
        const SHOT: crate::control::cap::mint::CapShot = crate::control::cap::mint::CapShot::One;
        const OP: ControlOp = ControlOp::Fence;
        const AUTO_MINT_WIRE: bool = false;

        fn mint_handle(_session: SessionId, _lane: Lane, _scope: ScopeId) -> Self::Handle {}
    }

    #[test]
    fn descriptor_control_header_accepts_exact_match() {
        let (desc, header) = route_decision_header(3, 11, 0);
        StaticTestCluster::<1>::verify_control_header(desc, header, 3, 11)
            .expect("exact descriptor/header match must verify");
    }

    #[test]
    fn descriptor_control_header_rejects_flags_scope_and_epoch_mismatch() {
        let (desc, header) =
            route_decision_header(3, 11, ControlDesc::of::<RouteDecisionKind>().header_flags());
        assert!(
            matches!(
                StaticTestCluster::<1>::verify_control_header(
                    desc,
                    CapHeader::new(
                        header.sid(),
                        header.lane(),
                        header.role(),
                        header.tag(),
                        header.label(),
                        header.op(),
                        header.path(),
                        header.shot(),
                        header.scope_kind(),
                        header.flags() | 0x80,
                        header.scope_id(),
                        header.epoch(),
                        *header.handle(),
                    ),
                    3,
                    11,
                ),
                Err(CpError::Authorisation { operation })
                    if operation == desc.op() as u8
            ),
            "reserved flag bits must fail closed",
        );
        assert!(
            matches!(
                StaticTestCluster::<1>::verify_control_header(desc, header, 4, 11),
                Err(CpError::Authorisation { operation })
                    if operation == desc.op() as u8
            ),
            "scope mismatch must fail closed",
        );
        assert!(
            matches!(
                StaticTestCluster::<1>::verify_control_header(desc, header, 3, 12),
                Err(CpError::Authorisation { operation })
                    if operation == desc.op() as u8
            ),
            "epoch mismatch must fail closed",
        );
    }

    #[test]
    fn descriptor_control_header_rejects_tag_label_op_path_and_shot_mismatch() {
        let (desc, header) =
            route_decision_header(3, 11, ControlDesc::of::<RouteDecisionKind>().header_flags());

        for mismatched in [
            CapHeader::new(
                header.sid(),
                header.lane(),
                header.role(),
                header.tag().wrapping_add(1),
                header.label(),
                header.op(),
                header.path(),
                header.shot(),
                header.scope_kind(),
                header.flags(),
                header.scope_id(),
                header.epoch(),
                *header.handle(),
            ),
            CapHeader::new(
                header.sid(),
                header.lane(),
                header.role(),
                header.tag(),
                header.label().wrapping_add(1),
                header.op(),
                header.path(),
                header.shot(),
                header.scope_kind(),
                header.flags(),
                header.scope_id(),
                header.epoch(),
                *header.handle(),
            ),
            CapHeader::new(
                header.sid(),
                header.lane(),
                header.role(),
                header.tag(),
                header.label(),
                ControlOp::LoopContinue,
                header.path(),
                header.shot(),
                header.scope_kind(),
                header.flags(),
                header.scope_id(),
                header.epoch(),
                *header.handle(),
            ),
            CapHeader::new(
                header.sid(),
                header.lane(),
                header.role(),
                header.tag(),
                header.label(),
                header.op(),
                crate::control::cap::mint::ControlPath::Wire,
                header.shot(),
                header.scope_kind(),
                header.flags(),
                header.scope_id(),
                header.epoch(),
                *header.handle(),
            ),
            CapHeader::new(
                header.sid(),
                header.lane(),
                header.role(),
                header.tag(),
                header.label(),
                header.op(),
                header.path(),
                crate::control::cap::mint::CapShot::Many,
                header.scope_kind(),
                header.flags(),
                header.scope_id(),
                header.epoch(),
                *header.handle(),
            ),
        ] {
            assert!(
                matches!(
                    StaticTestCluster::<1>::verify_control_header(desc, mismatched, 3, 11),
                    Err(CpError::Authorisation { operation })
                        if operation == desc.op() as u8
                ),
                "descriptor/header mismatch must fail closed",
            );
        }
    }

    #[test]
    fn no_handle_decode_in_core_auth() {
        let desc = ControlDesc::of::<DecodePoisonKind>();
        let header = CapHeader::new(
            SessionId::new(7),
            Lane::new(0),
            0,
            desc.resource_tag(),
            desc.label(),
            desc.op(),
            desc.path(),
            desc.shot(),
            desc.scope_kind(),
            desc.header_flags(),
            3,
            11,
            DecodePoisonKind::encode_handle(&()),
        );

        StaticTestCluster::<1>::verify_control_header(desc, header, 3, 11)
            .expect("core auth must not decode the typed handle");
    }

    #[test]
    fn local_descriptor_tx_commit_uses_header_snapshot_generation() {
        run_on_transient_compiled_test_stack(
            "local_descriptor_tx_commit_uses_header_snapshot_generation",
            || {
                with_cluster_fixture(|clock, config| {
                    with_test_cluster(clock, |cluster| {
                        let rv_id = cluster
                            .add_rendezvous_from_config(config, DummyTransport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(41);
                        let (endpoint_handle, lane) = attach_session_lane(cluster, rv_id, sid);

                        advance_lane_generation(cluster, rv_id, lane, Generation::new(3));
                        let snapshot = cluster
                            .get_local(&rv_id)
                            .expect("registered rendezvous")
                            .state_snapshot_at_lane(sid, lane);
                        assert_eq!(snapshot, Generation::new(3));

                        advance_lane_generation(cluster, rv_id, lane, Generation::new(7));

                        let bytes = session_lane_control_token_with_epoch::<LocalTxCommitControl>(
                            sid,
                            lane,
                            snapshot.raw(),
                        );
                        let result = cluster.dispatch_descriptor_control_frame(
                            rv_id,
                            bytes,
                            ControlDesc::of::<LocalTxCommitControl>(),
                            0,
                            snapshot.raw(),
                            None,
                        );
                        assert!(
                            result.is_ok(),
                            "local descriptor tx commit must execute against the header snapshot generation",
                        );
                        assert!(
                            matches!(
                                cluster
                                    .get_local(&rv_id)
                                    .expect("registered rendezvous")
                                    .tx_commit_at_lane(sid, lane, snapshot),
                                Err(crate::rendezvous::error::TxCommitError::AlreadyFinalized {
                                    sid: err_sid,
                                }) if err_sid == sid
                            ),
                            "descriptor tx commit must finalize the recorded snapshot",
                        );

                        unsafe {
                            drop_test_public_endpoint(cluster, rv_id, endpoint_handle);
                        }
                    });
                });
            },
        );
    }

    #[test]
    fn local_descriptor_tx_commit_rejects_stale_header_snapshot_generation() {
        run_on_transient_compiled_test_stack(
            "local_descriptor_tx_commit_rejects_stale_header_snapshot_generation",
            || {
                with_cluster_fixture(|clock, config| {
                    with_test_cluster(clock, |cluster| {
                        let rv_id = cluster
                            .add_rendezvous_from_config(config, DummyTransport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(45);
                        let (endpoint_handle, lane) = attach_session_lane(cluster, rv_id, sid);

                        advance_lane_generation(cluster, rv_id, lane, Generation::new(3));
                        let stale_snapshot = cluster
                            .get_local(&rv_id)
                            .expect("registered rendezvous")
                            .state_snapshot_at_lane(sid, lane);
                        advance_lane_generation(cluster, rv_id, lane, Generation::new(7));
                        let current_snapshot = cluster
                            .get_local(&rv_id)
                            .expect("registered rendezvous")
                            .state_snapshot_at_lane(sid, lane);

                        let bytes = session_lane_control_token_with_epoch::<LocalTxCommitControl>(
                            sid,
                            lane,
                            stale_snapshot.raw(),
                        );
                        let err = cluster
                            .dispatch_descriptor_control_frame(
                                rv_id,
                                bytes,
                                ControlDesc::of::<LocalTxCommitControl>(),
                                0,
                                stale_snapshot.raw(),
                                None,
                            )
                            .expect_err("stale descriptor epoch must not commit current snapshot");
                        assert!(matches!(
                            err,
                            CpError::TxCommit(TxCommitError::GenerationMismatch)
                        ));

                        cluster
                            .get_local(&rv_id)
                            .expect("registered rendezvous")
                            .tx_commit_at_lane(sid, lane, current_snapshot)
                            .expect(
                                "rejected stale descriptor must leave current snapshot available",
                            );

                        unsafe {
                            drop_test_public_endpoint(cluster, rv_id, endpoint_handle);
                        }
                    });
                });
            },
        );
    }

    #[test]
    fn local_descriptor_state_restore_uses_header_snapshot_generation() {
        run_on_transient_compiled_test_stack(
            "local_descriptor_state_restore_uses_header_snapshot_generation",
            || {
                with_cluster_fixture(|clock, config| {
                    with_test_cluster(clock, |cluster| {
                        let rv_id = cluster
                            .add_rendezvous_from_config(config, DummyTransport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(42);
                        let (endpoint_handle, lane) = attach_session_lane(cluster, rv_id, sid);

                        advance_lane_generation(cluster, rv_id, lane, Generation::new(3));
                        let snapshot = cluster
                            .get_local(&rv_id)
                            .expect("registered rendezvous")
                            .state_snapshot_at_lane(sid, lane);
                        advance_lane_generation(cluster, rv_id, lane, Generation::new(7));

                        let bytes = session_lane_control_token_with_epoch::<LocalStateRestoreControl>(
                            sid,
                            lane,
                            snapshot.raw(),
                        );
                        let result = cluster.dispatch_descriptor_control_frame(
                            rv_id,
                            bytes,
                            ControlDesc::of::<LocalStateRestoreControl>(),
                            0,
                            snapshot.raw(),
                            None,
                        );
                        assert!(
                            result.is_ok(),
                            "local descriptor state restore must execute against the header snapshot generation",
                        );
                        assert_eq!(
                            cluster
                                .get_local(&rv_id)
                                .expect("registered rendezvous")
                                .lane_generation(lane),
                            snapshot,
                            "state restore must rewind to the recorded snapshot generation",
                        );

                        unsafe {
                            drop_test_public_endpoint(cluster, rv_id, endpoint_handle);
                        }
                    });
                });
            },
        );
    }

    #[test]
    fn local_descriptor_tx_abort_uses_header_snapshot_generation() {
        run_on_transient_compiled_test_stack(
            "local_descriptor_tx_abort_uses_header_snapshot_generation",
            || {
                with_cluster_fixture(|clock, config| {
                    with_test_cluster(clock, |cluster| {
                        let rv_id = cluster
                            .add_rendezvous_from_config(config, DummyTransport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(43);
                        let (endpoint_handle, lane) = attach_session_lane(cluster, rv_id, sid);

                        advance_lane_generation(cluster, rv_id, lane, Generation::new(3));
                        let snapshot = cluster
                            .get_local(&rv_id)
                            .expect("registered rendezvous")
                            .state_snapshot_at_lane(sid, lane);
                        advance_lane_generation(cluster, rv_id, lane, Generation::new(7));

                        let bytes = session_lane_control_token_with_epoch::<LocalTxAbortControl>(
                            sid,
                            lane,
                            snapshot.raw(),
                        );
                        let result = cluster.dispatch_descriptor_control_frame(
                            rv_id,
                            bytes,
                            ControlDesc::of::<LocalTxAbortControl>(),
                            0,
                            snapshot.raw(),
                            None,
                        );
                        assert!(
                            result.is_ok(),
                            "local descriptor tx abort must execute against the header snapshot generation",
                        );
                        assert_eq!(
                            cluster
                                .get_local(&rv_id)
                                .expect("registered rendezvous")
                                .lane_generation(lane),
                            snapshot,
                            "tx abort must rewind to the recorded snapshot generation",
                        );

                        unsafe {
                            drop_test_public_endpoint(cluster, rv_id, endpoint_handle);
                        }
                    });
                });
            },
        );
    }

    #[test]
    fn local_descriptor_abort_ack_uses_header_lane_generation() {
        run_on_transient_compiled_test_stack(
            "local_descriptor_abort_ack_uses_header_lane_generation",
            || {
                with_cluster_runtime(|fixture| {
                    let config = fixture.config0();
                    let tap = unsafe { &*fixture.tap0 };
                    with_test_cluster(fixture.clock(), |cluster| {
                        let rv_id = cluster
                            .add_rendezvous_from_config(config, DummyTransport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(43);
                        let (endpoint_handle, lane) = attach_session_lane(cluster, rv_id, sid);

                        advance_lane_generation(cluster, rv_id, lane, Generation::new(7));

                        let bytes = session_lane_control_token_with_epoch::<LocalAbortAckControl>(
                            sid, lane, 7,
                        );
                        let result = cluster.dispatch_descriptor_control_frame(
                            rv_id,
                            bytes,
                            ControlDesc::of::<LocalAbortAckControl>(),
                            0,
                            7,
                            None,
                        );
                        assert!(
                            result.is_ok(),
                            "local descriptor abort ack must execute against the header lane generation",
                        );
                        assert!(
                            tap.iter().any(|event| {
                                event.id == crate::observe::ids::ABORT_ACK
                                    && event.arg0 == sid.raw()
                                    && event.arg1 == 7
                            }),
                            "abort ack tap payload must carry the authoritative lane generation",
                        );

                        unsafe {
                            drop_test_public_endpoint(cluster, rv_id, endpoint_handle);
                        }
                    });
                });
            },
        );
    }

    #[test]
    fn local_descriptor_abort_ack_rejects_stale_header_lane_generation() {
        run_on_transient_compiled_test_stack(
            "local_descriptor_abort_ack_rejects_stale_header_lane_generation",
            || {
                with_cluster_runtime(|fixture| {
                    let config = fixture.config0();
                    let tap = unsafe { &*fixture.tap0 };
                    with_test_cluster(fixture.clock(), |cluster| {
                        let rv_id = cluster
                            .add_rendezvous_from_config(config, DummyTransport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(46);
                        let (endpoint_handle, lane) = attach_session_lane(cluster, rv_id, sid);

                        advance_lane_generation(cluster, rv_id, lane, Generation::new(3));
                        let bytes = session_lane_control_token_with_epoch::<LocalAbortAckControl>(
                            sid, lane, 3,
                        );
                        advance_lane_generation(cluster, rv_id, lane, Generation::new(7));

                        let err = cluster
                            .dispatch_descriptor_control_frame(
                                rv_id,
                                bytes,
                                ControlDesc::of::<LocalAbortAckControl>(),
                                0,
                                3,
                                None,
                            )
                            .expect_err("stale descriptor epoch must not execute abort ack");
                        assert_eq!(
                            err,
                            CpError::GenerationViolation {
                                expected: 3,
                                actual: 7,
                            }
                        );
                        assert!(
                            !tap.iter().any(|event| {
                                event.id == crate::observe::ids::ABORT_ACK
                                    && event.arg0 == sid.raw()
                            }),
                            "stale descriptor epoch must fail before abort ack tap emission",
                        );

                        unsafe {
                            drop_test_public_endpoint(cluster, rv_id, endpoint_handle);
                        }
                    });
                });
            },
        );
    }

    #[test]
    fn local_descriptor_state_snapshot_rejects_stale_header_lane_generation() {
        run_on_transient_compiled_test_stack(
            "local_descriptor_state_snapshot_rejects_stale_header_lane_generation",
            || {
                with_cluster_fixture(|clock, config| {
                    with_test_cluster(clock, |cluster| {
                        let rv_id = cluster
                            .add_rendezvous_from_config(config, DummyTransport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(47);
                        let (endpoint_handle, lane) = attach_session_lane(cluster, rv_id, sid);

                        advance_lane_generation(cluster, rv_id, lane, Generation::new(3));
                        let bytes = session_lane_control_token_with_epoch::<
                            LocalStateSnapshotControl,
                        >(sid, lane, 3);
                        advance_lane_generation(cluster, rv_id, lane, Generation::new(7));

                        let err = cluster
                            .dispatch_descriptor_control_frame(
                                rv_id,
                                bytes,
                                ControlDesc::of::<LocalStateSnapshotControl>(),
                                0,
                                3,
                                None,
                            )
                            .expect_err("stale descriptor epoch must not snapshot current state");
                        assert_eq!(
                            err,
                            CpError::GenerationViolation {
                                expected: 3,
                                actual: 7,
                            }
                        );
                        assert_eq!(
                            cluster
                                .get_local(&rv_id)
                                .expect("registered rendezvous")
                                .snapshot_generation(lane),
                            None,
                            "stale descriptor epoch must fail before recording a snapshot",
                        );

                        unsafe {
                            drop_test_public_endpoint(cluster, rv_id, endpoint_handle);
                        }
                    });
                });
            },
        );
    }

    #[test]
    fn descriptor_cap_delegate_fails_closed() {
        run_on_transient_compiled_test_stack("descriptor_cap_delegate_fails_closed", || {
            with_cluster_fixture(|clock, config| {
                with_test_cluster(clock, |cluster| {
                    let rv_id = cluster
                        .add_rendezvous_from_config(config, DummyTransport)
                        .expect("register rendezvous");
                    let sid = SessionId::new(44);
                    let (endpoint_handle, lane) = attach_session_lane(cluster, rv_id, sid);

                    let bytes = session_lane_control_token::<WireCapDelegateControl>(sid, lane);
                    let err = cluster
                        .dispatch_descriptor_control_frame(
                            rv_id,
                            bytes,
                            ControlDesc::of::<WireCapDelegateControl>(),
                            0,
                            0,
                            None,
                        )
                        .expect_err("descriptor cap-delegate must fail closed");
                    assert_eq!(
                        err,
                        CpError::UnsupportedEffect(ControlOp::CapDelegate as u8)
                    );

                    unsafe {
                        drop_test_public_endpoint(cluster, rv_id, endpoint_handle);
                    }
                });
            });
        });
    }

    type StaticTestCluster<const MAX_RV: usize> =
        SessionCluster<'static, DummyTransport, DefaultLabelUniverse, CounterClock, MAX_RV>;

    const CLUSTER_TEST_SLAB_CAPACITY: usize = 262_144;
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct MeasuredResidentShape {
        route_scope_count: usize,
        active_lane_count: usize,
        max_route_stack_depth: usize,
        max_loop_stack_depth: usize,
        route_bytes: usize,
        loop_bytes: usize,
        cap_bytes: usize,
        endpoint_bytes: usize,
        endpoint_header_bytes: usize,
        endpoint_port_slots_bytes: usize,
        endpoint_guard_slots_bytes: usize,
        endpoint_header_padding_bytes: usize,
        compiled_program_header_bytes: usize,
        compiled_role_header_bytes: usize,
        compiled_program_persistent_bytes: usize,
        compiled_role_persistent_bytes: usize,
        endpoint_phase_cursor_state_bytes: usize,
        endpoint_route_state_bytes: usize,
        endpoint_route_arm_stack_bytes: usize,
        endpoint_lane_offer_state_slots_bytes: usize,
        endpoint_frontier_state_bytes: usize,
        endpoint_frontier_root_rows_bytes: usize,
        endpoint_frontier_root_active_slots_bytes: usize,
        endpoint_frontier_root_observed_key_slots_bytes: usize,
        endpoint_frontier_offer_entry_slots_bytes: usize,
        endpoint_binding_inbox_bytes: usize,
        endpoint_binding_slots_bytes: usize,
        endpoint_binding_len_bytes: usize,
        endpoint_binding_label_masks_bytes: usize,
        endpoint_scope_evidence_store_bytes: usize,
        endpoint_scope_evidence_slots_bytes: usize,
        endpoint_padding_bytes: usize,
    }

    fn measure_huge_shape<const ROLE: u8>(
        projected: &role_program::RoleProgram<ROLE>,
    ) -> MeasuredResidentShape {
        with_cluster_fixture(|clock, config| {
            with_test_cluster(clock, |cluster| {
                let rv_id = cluster
                    .add_rendezvous_from_config(config, DummyTransport)
                    .expect("register rendezvous");
                let lowering = crate::global::lowering_input(&projected);
                let summary = lowering.summary();
                let counts = summary.compiled_program_counts();
                let program_bytes = CompiledProgramFacts::persistent_bytes_for_counts(counts);
                let role_image = cluster
                    .materialize_test_role_image::<ROLE, _>(rv_id, projected)
                    .expect("materialize actual role image");
                let compiled_role = unsafe { &*role_image.compiled_ptr() };
                let active_lane_count = compiled_role.active_lane_count();
                let endpoint_layout = role_image.endpoint_arena_layout_for_binding(false);
                let endpoint_storage =
                    StaticTestCluster::<4>::public_endpoint_storage_requirement(role_image, false);
                let endpoint_section_bytes = endpoint_layout.phase_cursor_state().bytes()
                    + endpoint_layout.route_state().bytes()
                    + endpoint_layout.route_arm_stack().bytes()
                    + endpoint_layout.lane_offer_state_slots().bytes()
                    + endpoint_layout.frontier_state().bytes()
                    + endpoint_layout.frontier_root_rows().bytes()
                    + endpoint_layout.frontier_root_active_slots().bytes()
                    + endpoint_layout.frontier_root_observed_key_slots().bytes()
                    + endpoint_layout.frontier_offer_entry_slots().bytes()
                    + endpoint_layout.binding_inbox().bytes()
                    + endpoint_layout.binding_slots().bytes()
                    + endpoint_layout.binding_len().bytes()
                    + endpoint_layout.binding_label_masks().bytes()
                    + endpoint_layout.scope_evidence_slots().bytes();

                MeasuredResidentShape {
                    route_scope_count: compiled_role.route_scope_count(),
                    active_lane_count,
                    max_route_stack_depth: compiled_role.max_route_stack_depth(),
                    max_loop_stack_depth: compiled_role.max_loop_stack_depth(),
                    route_bytes: crate::rendezvous::tables::RouteTable::storage_bytes(
                        compiled_role.route_table_frame_slots(),
                        compiled_role.route_table_lane_slots(),
                    ),
                    loop_bytes: crate::rendezvous::tables::LoopTable::storage_bytes(
                        compiled_role.loop_table_slots(),
                        compiled_role.loop_table_lane_slots(),
                    ),
                    cap_bytes: crate::rendezvous::capability::CapTable::storage_bytes(
                        compiled_role.resident_cap_entries(),
                    ),
                    endpoint_bytes: endpoint_layout.total_bytes(),
                    endpoint_header_bytes: endpoint_storage.header_bytes,
                    endpoint_port_slots_bytes: endpoint_storage.port_slots_bytes,
                    endpoint_guard_slots_bytes: endpoint_storage.guard_slots_bytes,
                    endpoint_header_padding_bytes: endpoint_storage.header_padding_bytes,
                    compiled_program_header_bytes: size_of::<CompiledProgramFacts>(),
                    compiled_role_header_bytes: size_of::<CompiledRoleImage>(),
                    compiled_program_persistent_bytes: program_bytes,
                    compiled_role_persistent_bytes: compiled_role.actual_persistent_bytes(),
                    endpoint_phase_cursor_state_bytes: endpoint_layout.phase_cursor_state().bytes(),
                    endpoint_route_state_bytes: endpoint_layout.route_state().bytes(),
                    endpoint_route_arm_stack_bytes: endpoint_layout.route_arm_stack().bytes(),
                    endpoint_lane_offer_state_slots_bytes: endpoint_layout
                        .lane_offer_state_slots()
                        .bytes(),
                    endpoint_frontier_state_bytes: endpoint_layout.frontier_state().bytes(),
                    endpoint_frontier_root_rows_bytes: endpoint_layout.frontier_root_rows().bytes(),
                    endpoint_frontier_root_active_slots_bytes: endpoint_layout
                        .frontier_root_active_slots()
                        .bytes(),
                    endpoint_frontier_root_observed_key_slots_bytes: endpoint_layout
                        .frontier_root_observed_key_slots()
                        .bytes(),
                    endpoint_frontier_offer_entry_slots_bytes: endpoint_layout
                        .frontier_offer_entry_slots()
                        .bytes(),
                    endpoint_binding_inbox_bytes: endpoint_layout.binding_inbox().bytes(),
                    endpoint_binding_slots_bytes: endpoint_layout.binding_slots().bytes(),
                    endpoint_binding_len_bytes: endpoint_layout.binding_len().bytes(),
                    endpoint_binding_label_masks_bytes: endpoint_layout
                        .binding_label_masks()
                        .bytes(),
                    endpoint_scope_evidence_store_bytes: 0,
                    endpoint_scope_evidence_slots_bytes: endpoint_layout
                        .scope_evidence_slots()
                        .bytes(),
                    endpoint_padding_bytes: endpoint_layout
                        .total_bytes()
                        .saturating_sub(endpoint_section_bytes),
                }
            })
        })
    }

    #[test]
    fn public_endpoint_leases_stay_small_and_metadata_only() {
        assert!(
            size_of::<crate::rendezvous::core::EndpointLeaseSlot>() <= 6 * size_of::<usize>(),
            "public endpoint lease must stay a small metadata owner"
        );
        let endpoint_storage_bytes = size_of::<
            PublicEndpointKernelRaw<'static, DummyTransport, DefaultLabelUniverse, CounterClock, 2>,
        >();
        assert!(
            endpoint_storage_bytes <= CLUSTER_TEST_SLAB_CAPACITY,
            "shared cluster test slab must cover one leased public endpoint (required={}, cap={})",
            endpoint_storage_bytes,
            CLUSTER_TEST_SLAB_CAPACITY,
        );
    }

    #[test]
    fn same_rendezvous_multi_enter_is_not_limited_by_max_rv() {
        run_on_transient_compiled_test_stack(
            "same_rendezvous_multi_enter_is_not_limited_by_max_rv",
            || {
                with_cluster_fixture(|clock, config| {
                    with_test_cluster_1(clock, |cluster| {
                        let controller_program = linear_program::controller_program();
                        let worker_program = linear_program::worker_program();
                        let rv_id = cluster
                            .add_rendezvous_from_config_auto(config, DummyTransport)
                            .expect("register rendezvous");
                        let lease_capacity = cluster
                            .get_local(&rv_id)
                            .expect("registered rendezvous")
                            .endpoint_lease_capacity();
                        assert_eq!(
                            lease_capacity,
                            EndpointLeaseId::from(crate::global::ROLE_DOMAIN_SIZE as u16),
                            "public-path auto lease table must follow the compiled public role domain, not MAX_RV"
                        );

                        let first = cluster
                            .enter(
                                rv_id,
                                SessionId::new(1),
                                &controller_program,
                                crate::binding::BindingHandle::None(crate::binding::NoBinding),
                            )
                            .expect("enter controller on single rendezvous");
                        let second = cluster
                            .enter(
                                rv_id,
                                SessionId::new(1),
                                &worker_program,
                                crate::binding::BindingHandle::None(crate::binding::NoBinding),
                            )
                            .expect("enter worker on same rendezvous");

                        assert_ne!(
                            first.0, second.0,
                            "same-session controller/worker enters must keep distinct lease identities"
                        );

                        unsafe {
                            let worker_endpoint = cluster
                                .public_endpoint_ptr::<1, MintConfig>(rv_id, second.0, second.1)
                                .expect("live worker endpoint");
                            core::ptr::drop_in_place(worker_endpoint);
                            cluster.release_public_endpoint_slot_owned(rv_id, second.0, second.1);
                            drop_test_public_endpoint(cluster, rv_id, first);
                        }
                    });
                });
            },
        );
    }

    #[test]
    fn public_endpoint_slot_ids_do_not_truncate_above_u8() {
        run_on_transient_compiled_test_stack(
            "public_endpoint_slot_ids_do_not_truncate_above_u8",
            || {
                with_cluster_fixture(|clock, config| {
                    with_test_cluster_1(clock, |cluster| {
                        let rv_id = cluster
                            .with_control_mut(|core| {
                                core.locals.register_local_from_config(
                                    config,
                                    DummyTransport,
                                    u8::MAX as usize + 2,
                                )
                            })
                            .expect("register explicit wide rendezvous");
                        let lease_capacity = cluster
                            .get_local(&rv_id)
                            .expect("registered rendezvous")
                            .endpoint_lease_capacity();
                        assert!(
                            lease_capacity > EndpointLeaseId::from(u8::MAX),
                            "public-path rendezvous must expose lease ids above u8 for this regression test (capacity={lease_capacity})"
                        );

                        let mut handles =
                            [(EndpointLeaseId::ZERO, 0u32, 0usize, 0usize); u8::MAX as usize + 2];
                        cluster.with_control_mut(|core| {
                            let rv = core.locals.get_mut(&rv_id).expect("registered rendezvous");
                            for handle in &mut handles {
                                *handle = unsafe {
                                    rv.allocate_endpoint_lease(
                                        1,
                                        1,
                                        crate::rendezvous::core::EndpointResidentBudget::ZERO,
                                    )
                                }
                                .expect("lease across wide slot ids");
                            }
                        });

                        assert_eq!(
                            handles[u8::MAX as usize].0,
                            EndpointLeaseId::from(u8::MAX),
                            "slot 255 must remain addressable without narrowing"
                        );
                        assert_eq!(
                            handles[u8::MAX as usize + 1].0,
                            u16::from(EndpointLeaseId::from(u8::MAX))
                                .saturating_add(1)
                                .into(),
                            "slot 256 must survive without truncation"
                        );

                        let slot_255_storage = cluster
                            .get_local(&rv_id)
                            .expect("registered rendezvous")
                            .endpoint_lease_storage(
                                handles[u8::MAX as usize].0,
                                handles[u8::MAX as usize].1,
                            )
                            .expect("slot 255 storage");
                        let slot_256_storage = cluster
                            .get_local(&rv_id)
                            .expect("registered rendezvous")
                            .endpoint_lease_storage(
                                handles[u8::MAX as usize + 1].0,
                                handles[u8::MAX as usize + 1].1,
                            )
                            .expect("slot 256 storage");
                        assert_ne!(
                            slot_255_storage.0, slot_256_storage.0,
                            "distinct wide lease ids must resolve to distinct storage offsets"
                        );
                        assert_eq!(slot_255_storage.1, 1);
                        assert_eq!(slot_256_storage.1, 1);

                        cluster.with_control_mut(|core| {
                            let rv = core.locals.get_mut(&rv_id).expect("registered rendezvous");
                            for handle in handles.into_iter().rev() {
                                rv.release_endpoint_lease(handle.0, handle.1);
                            }
                        });
                    });
                });
            },
        );
    }

    #[test]
    fn pico2_resident_component_sizes() {
        let session_cluster_bytes = size_of::<StaticTestCluster<1>>();
        let control_core_bytes = size_of::<
            ControlCore<
                'static,
                DummyTransport,
                DefaultLabelUniverse,
                CounterClock,
                crate::control::cap::mint::EpochTbl,
                1,
            >,
        >();
        let rv_core_bytes = size_of::<
            crate::control::lease::core::ControlCore<
                'static,
                DummyTransport,
                DefaultLabelUniverse,
                CounterClock,
                crate::control::cap::mint::EpochTbl,
                1,
            >,
        >();
        let resolver_core_bytes = size_of::<ResolverCore<'static, 1>>();
        let lowering_summary_bytes = size_of::<LoweringSummary>();
        let compiled_program_bytes = size_of::<CompiledProgramFacts>();
        let compiled_role_bytes = size_of::<CompiledRoleImage>();
        let route_heavy_worker = huge_program::worker_program();
        let role_compile_scratch_bytes =
            crate::global::compiled::materialize::role_lowering_scratch_storage_bytes(
                crate::global::lowering_input(&route_heavy_worker).footprint(),
            );
        let endpoint_storage_bytes = size_of::<
            PublicEndpointKernelRaw<'static, DummyTransport, DefaultLabelUniverse, CounterClock, 1>,
        >();
        let rendezvous_header_bytes = size_of::<
            crate::rendezvous::core::Rendezvous<
                'static,
                'static,
                DummyTransport,
                DefaultLabelUniverse,
                CounterClock,
                crate::control::cap::mint::EpochTbl,
            >,
        >();
        let route_table_bytes = size_of::<crate::rendezvous::tables::RouteTable>();
        let loop_table_bytes = size_of::<crate::rendezvous::tables::LoopTable>();
        let cap_table_bytes = size_of::<crate::rendezvous::capability::CapTable>();
        let slot_arena_bytes = size_of::<crate::rendezvous::slots::SlotArena>();
        let delegation_graph_bytes = size_of::<
            LeaseGraph<
                'static,
                DelegationLeaseSpec<
                    DummyTransport,
                    DefaultLabelUniverse,
                    CounterClock,
                    crate::control::cap::mint::EpochTbl,
                >,
            >,
        >();
        let topology_graph_bytes = size_of::<
            LeaseGraph<
                'static,
                TopologyLeaseSpec<
                    DummyTransport,
                    DefaultLabelUniverse,
                    CounterClock,
                    crate::control::cap::mint::EpochTbl,
                >,
            >,
        >();

        assert!(
            session_cluster_bytes <= 1_700_000
                && control_core_bytes <= 1_700_000
                && rv_core_bytes <= 250_000
                && resolver_core_bytes <= 8_000
                && lowering_summary_bytes <= 20_000
                && compiled_program_bytes <= 64
                && compiled_role_bytes <= 64
                && role_compile_scratch_bytes <= 66_000
                && endpoint_storage_bytes <= 90_000
                && rendezvous_header_bytes <= 32_768
                && route_table_bytes <= 128
                && loop_table_bytes <= 64
                && cap_table_bytes <= 64
                && slot_arena_bytes <= 512
                && delegation_graph_bytes <= 3_000
                && topology_graph_bytes <= 2_000,
            "resident regression: session_cluster={session_cluster_bytes} control_core={control_core_bytes} rv_core={rv_core_bytes} resolver={resolver_core_bytes} lowering_summary={lowering_summary_bytes} compiled_program={compiled_program_bytes} compiled_role={compiled_role_bytes} role_compile_scratch={role_compile_scratch_bytes} endpoint_storage={endpoint_storage_bytes} rendezvous_header={rendezvous_header_bytes} route_table={route_table_bytes} loop_table={loop_table_bytes} cap_table={cap_table_bytes} slot_arena={slot_arena_bytes} delegation_graph={delegation_graph_bytes} topology_graph={topology_graph_bytes}"
        );
    }

    #[test]
    fn huge_shape_matrix_resident_bytes_stay_measured_and_local() {
        let route_worker = huge_program::worker_program();
        let route = measure_huge_shape::<1>(&route_worker);
        let linear_worker = linear_program::worker_program();
        let linear = measure_huge_shape::<1>(&linear_worker);
        let fanout_worker = fanout_program::worker_program();
        let fanout = measure_huge_shape::<1>(&fanout_worker);

        for (name, measured) in [
            ("route_heavy", route),
            ("linear_heavy", linear),
            ("fanout_heavy", fanout),
        ] {
            std::println!(
                "resident-shape name={name} route_bytes={} loop_bytes={} cap_bytes={} endpoint_bytes={} endpoint_header_bytes={} endpoint_port_slots_bytes={} endpoint_guard_slots_bytes={} endpoint_header_padding_bytes={} compiled_program_header_bytes={} compiled_role_header_bytes={} compiled_program_persistent_bytes={} compiled_role_persistent_bytes={} endpoint_phase_cursor_state_bytes={} endpoint_route_state_bytes={} endpoint_route_arm_stack_bytes={} endpoint_lane_offer_state_slots_bytes={} endpoint_frontier_state_bytes={} endpoint_frontier_root_rows_bytes={} endpoint_frontier_root_active_slots_bytes={} endpoint_frontier_root_observed_key_slots_bytes={} endpoint_frontier_offer_entry_slots_bytes={} endpoint_binding_inbox_bytes={} endpoint_binding_slots_bytes={} endpoint_binding_len_bytes={} endpoint_binding_label_masks_bytes={} endpoint_scope_evidence_store_bytes={} endpoint_scope_evidence_slots_bytes={} endpoint_padding_bytes={}",
                measured.route_bytes,
                measured.loop_bytes,
                measured.cap_bytes,
                measured.endpoint_bytes,
                measured.endpoint_header_bytes,
                measured.endpoint_port_slots_bytes,
                measured.endpoint_guard_slots_bytes,
                measured.endpoint_header_padding_bytes,
                measured.compiled_program_header_bytes,
                measured.compiled_role_header_bytes,
                measured.compiled_program_persistent_bytes,
                measured.compiled_role_persistent_bytes,
                measured.endpoint_phase_cursor_state_bytes,
                measured.endpoint_route_state_bytes,
                measured.endpoint_route_arm_stack_bytes,
                measured.endpoint_lane_offer_state_slots_bytes,
                measured.endpoint_frontier_state_bytes,
                measured.endpoint_frontier_root_rows_bytes,
                measured.endpoint_frontier_root_active_slots_bytes,
                measured.endpoint_frontier_root_observed_key_slots_bytes,
                measured.endpoint_frontier_offer_entry_slots_bytes,
                measured.endpoint_binding_inbox_bytes,
                measured.endpoint_binding_slots_bytes,
                measured.endpoint_binding_len_bytes,
                measured.endpoint_binding_label_masks_bytes,
                measured.endpoint_scope_evidence_store_bytes,
                measured.endpoint_scope_evidence_slots_bytes,
                measured.endpoint_padding_bytes,
            );
        }

        assert_eq!(route.route_scope_count, huge_program::ROUTE_SCOPE_COUNT);
        assert_eq!(linear.route_scope_count, linear_program::ROUTE_SCOPE_COUNT);
        assert_eq!(fanout.route_scope_count, fanout_program::ROUTE_SCOPE_COUNT);

        assert!(
            route.route_bytes <= 2 * 1024,
            "route-heavy route resident bytes regressed: {:?}",
            route
        );
        assert!(
            linear.route_bytes <= 2 * 1024,
            "linear-heavy route resident bytes regressed: {:?}",
            linear
        );
        assert!(
            fanout.route_bytes <= 2 * 1024,
            "fanout-heavy route resident bytes regressed: {:?}",
            fanout
        );

        assert!(
            route.loop_bytes <= 2 * 1024,
            "route-heavy loop resident bytes regressed: {:?}",
            route
        );
        assert!(
            linear.loop_bytes <= 2 * 1024,
            "linear-heavy loop resident bytes regressed: {:?}",
            linear
        );
        assert!(
            fanout.loop_bytes <= 2 * 1024,
            "fanout-heavy loop resident bytes regressed: {:?}",
            fanout
        );

        assert!(
            route.cap_bytes <= 512,
            "route-heavy cap resident bytes regressed: {:?}",
            route
        );
        assert!(
            linear.cap_bytes <= 512,
            "linear-heavy cap resident bytes regressed: {:?}",
            linear
        );
        assert!(
            fanout.cap_bytes <= 512,
            "fanout-heavy cap resident bytes regressed: {:?}",
            fanout
        );

        assert!(
            route.endpoint_bytes <= 12 * 1024,
            "route-heavy endpoint resident bytes regressed: {:?}",
            route
        );
        assert!(
            linear.endpoint_bytes <= 8 * 1024,
            "linear-heavy endpoint resident bytes regressed: {:?}",
            linear
        );
        assert!(
            fanout.endpoint_bytes <= 12 * 1024,
            "fanout-heavy endpoint resident bytes regressed: {:?}",
            fanout
        );
        assert_eq!(
            route.endpoint_bytes,
            route.endpoint_phase_cursor_state_bytes
                + route.endpoint_route_state_bytes
                + route.endpoint_route_arm_stack_bytes
                + route.endpoint_lane_offer_state_slots_bytes
                + route.endpoint_frontier_state_bytes
                + route.endpoint_frontier_root_rows_bytes
                + route.endpoint_frontier_root_active_slots_bytes
                + route.endpoint_frontier_root_observed_key_slots_bytes
                + route.endpoint_frontier_offer_entry_slots_bytes
                + route.endpoint_binding_inbox_bytes
                + route.endpoint_binding_slots_bytes
                + route.endpoint_binding_len_bytes
                + route.endpoint_binding_label_masks_bytes
                + route.endpoint_scope_evidence_store_bytes
                + route.endpoint_scope_evidence_slots_bytes
                + route.endpoint_padding_bytes,
            "route-heavy endpoint arena breakdown must cover the full resident total: {route:?}"
        );
        assert_eq!(
            linear.endpoint_bytes,
            linear.endpoint_phase_cursor_state_bytes
                + linear.endpoint_route_state_bytes
                + linear.endpoint_route_arm_stack_bytes
                + linear.endpoint_lane_offer_state_slots_bytes
                + linear.endpoint_frontier_state_bytes
                + linear.endpoint_frontier_root_rows_bytes
                + linear.endpoint_frontier_root_active_slots_bytes
                + linear.endpoint_frontier_root_observed_key_slots_bytes
                + linear.endpoint_frontier_offer_entry_slots_bytes
                + linear.endpoint_binding_inbox_bytes
                + linear.endpoint_binding_slots_bytes
                + linear.endpoint_binding_len_bytes
                + linear.endpoint_binding_label_masks_bytes
                + linear.endpoint_scope_evidence_store_bytes
                + linear.endpoint_scope_evidence_slots_bytes
                + linear.endpoint_padding_bytes,
            "linear-heavy endpoint arena breakdown must cover the full resident total: {linear:?}"
        );
        assert_eq!(
            fanout.endpoint_bytes,
            fanout.endpoint_phase_cursor_state_bytes
                + fanout.endpoint_route_state_bytes
                + fanout.endpoint_route_arm_stack_bytes
                + fanout.endpoint_lane_offer_state_slots_bytes
                + fanout.endpoint_frontier_state_bytes
                + fanout.endpoint_frontier_root_rows_bytes
                + fanout.endpoint_frontier_root_active_slots_bytes
                + fanout.endpoint_frontier_root_observed_key_slots_bytes
                + fanout.endpoint_frontier_offer_entry_slots_bytes
                + fanout.endpoint_binding_inbox_bytes
                + fanout.endpoint_binding_slots_bytes
                + fanout.endpoint_binding_len_bytes
                + fanout.endpoint_binding_label_masks_bytes
                + fanout.endpoint_scope_evidence_store_bytes
                + fanout.endpoint_scope_evidence_slots_bytes
                + fanout.endpoint_padding_bytes,
            "fanout-heavy endpoint arena breakdown must cover the full resident total: {fanout:?}"
        );

        assert!(
            route.compiled_program_header_bytes <= 64
                && linear.compiled_program_header_bytes <= 64
                && fanout.compiled_program_header_bytes <= 64,
            "compiled program header must stay small-header only: route={route:?} linear={linear:?} fanout={fanout:?}"
        );
        assert!(
            route.compiled_role_header_bytes <= 64
                && linear.compiled_role_header_bytes <= 64
                && fanout.compiled_role_header_bytes <= 64,
            "compiled role header must stay compact-offset only: route={route:?} linear={linear:?} fanout={fanout:?}"
        );

        assert!(
            route.compiled_program_persistent_bytes <= 256
                && linear.compiled_program_persistent_bytes <= 64
                && fanout.compiled_program_persistent_bytes <= 384,
            "compiled program atlas tail regressed: route={route:?} linear={linear:?} fanout={fanout:?}"
        );
        assert!(
            route.compiled_role_persistent_bytes <= 3 * 1024
                && linear.compiled_role_persistent_bytes <= 1536
                && fanout.compiled_role_persistent_bytes <= 4 * 1024,
            "compiled role blob tail regressed: route={route:?} linear={linear:?} fanout={fanout:?}"
        );

        assert!(
            route.route_bytes >= linear.route_bytes,
            "route-heavy resident bytes must not fall below linear when route scopes are present: route={route:?} linear={linear:?}"
        );
        assert_eq!(
            route.route_bytes, fanout.route_bytes,
            "route resident bytes must stay tied to live route depth rather than total scope count: route={route:?} fanout={fanout:?}"
        );
        assert!(
            fanout.endpoint_bytes >= route.endpoint_bytes,
            "fanout-heavy endpoint resident bytes should dominate route-heavy due to larger branch fan-out: route={route:?} fanout={fanout:?}"
        );
    }

    struct ClusterRuntimeGuard {
        tap0: *mut [TapEvent; RING_EVENTS],
        tap1: *mut [TapEvent; RING_EVENTS],
        slab0: *mut [u8; CLUSTER_TEST_SLAB_CAPACITY],
        slab1: *mut [u8; CLUSTER_TEST_SLAB_CAPACITY],
        clock: *const CounterClock,
    }

    thread_local! {
        static CLUSTER_TAP0: UnsafeCell<[TapEvent; RING_EVENTS]> =
            const { UnsafeCell::new([TapEvent::zero(); RING_EVENTS]) };
        static CLUSTER_TAP1: UnsafeCell<[TapEvent; RING_EVENTS]> =
            const { UnsafeCell::new([TapEvent::zero(); RING_EVENTS]) };
        static CLUSTER_SLAB0: UnsafeCell<[u8; CLUSTER_TEST_SLAB_CAPACITY]> =
            const { UnsafeCell::new([0u8; CLUSTER_TEST_SLAB_CAPACITY]) };
        static CLUSTER_SLAB1: UnsafeCell<[u8; CLUSTER_TEST_SLAB_CAPACITY]> =
            const { UnsafeCell::new([0u8; CLUSTER_TEST_SLAB_CAPACITY]) };
        static CLUSTER_SLOT_1: UnsafeCell<MaybeUninit<StaticTestCluster<1>>> =
            const { UnsafeCell::new(MaybeUninit::uninit()) };
        static CLUSTER_SLOT_4: UnsafeCell<MaybeUninit<StaticTestCluster<4>>> =
            const { UnsafeCell::new(MaybeUninit::uninit()) };
        static CLUSTER_TEST_CLOCK: CounterClock = const { CounterClock::new() };
    }

    fn with_cluster_runtime<R>(f: impl FnOnce(&mut ClusterRuntimeGuard) -> R) -> R {
        CLUSTER_TAP0.with(|tap0| {
            CLUSTER_TAP1.with(|tap1| {
                CLUSTER_SLAB0.with(|slab0| {
                    CLUSTER_SLAB1.with(|slab1| {
                        CLUSTER_TEST_CLOCK.with(|clock| unsafe {
                            let tap0 = &mut *tap0.get();
                            tap0.fill(TapEvent::zero());
                            let tap1 = &mut *tap1.get();
                            tap1.fill(TapEvent::zero());
                            let slab0 = &mut *slab0.get();
                            slab0.fill(0);
                            let slab1 = &mut *slab1.get();
                            slab1.fill(0);
                            let mut fixture = ClusterRuntimeGuard {
                                tap0,
                                tap1,
                                slab0,
                                slab1,
                                clock: clock as *const CounterClock,
                            };
                            f(&mut fixture)
                        })
                    })
                })
            })
        })
    }

    impl ClusterRuntimeGuard {
        fn config0(&mut self) -> Config<'static, DefaultLabelUniverse, CounterClock> {
            let tap = unsafe { &mut *self.tap0 };
            let slab = unsafe { &mut *self.slab0 };
            Config::new(tap, slab)
        }

        fn config1(&mut self) -> Config<'static, DefaultLabelUniverse, CounterClock> {
            let tap = unsafe { &mut *self.tap1 };
            let slab = unsafe { &mut *self.slab1 };
            Config::new(tap, slab)
        }

        fn clock(&self) -> &'static CounterClock {
            unsafe { &*self.clock }
        }
    }

    fn with_cluster_fixture<R>(
        f: impl FnOnce(&'static CounterClock, Config<'static, DefaultLabelUniverse, CounterClock>) -> R,
    ) -> R {
        with_cluster_runtime(|fixture| {
            let config = fixture.config0();
            f(fixture.clock(), config)
        })
    }

    fn with_cluster_fixture_pair<R>(
        f: impl FnOnce(
            &'static CounterClock,
            Config<'static, DefaultLabelUniverse, CounterClock>,
            Config<'static, DefaultLabelUniverse, CounterClock>,
        ) -> R,
    ) -> R {
        with_cluster_runtime(|fixture| {
            let config0 = fixture.config0();
            let config1 = fixture.config1();
            f(fixture.clock(), config0, config1)
        })
    }

    fn with_test_cluster<R>(
        clock: &'static CounterClock,
        f: impl FnOnce(&'static StaticTestCluster<4>) -> R,
    ) -> R {
        CLUSTER_SLOT_4.with(|slot| unsafe {
            let ptr = (*slot.get()).as_mut_ptr();
            SessionCluster::init_empty(ptr, clock);
            let result = f(&*ptr);
            core::ptr::drop_in_place(ptr);
            result
        })
    }

    fn with_test_cluster_1<R>(
        clock: &'static CounterClock,
        f: impl FnOnce(&'static StaticTestCluster<1>) -> R,
    ) -> R {
        CLUSTER_SLOT_1.with(|slot| unsafe {
            let ptr = (*slot.get()).as_mut_ptr();
            SessionCluster::init_empty(ptr, clock);
            let result = f(&*ptr);
            core::ptr::drop_in_place(ptr);
            result
        })
    }

    unsafe fn drop_test_public_endpoint_for_role<const ROLE: u8, const MAX_RV: usize>(
        cluster: &'static StaticTestCluster<MAX_RV>,
        rv_id: RendezvousId,
        handle: (crate::rendezvous::core::EndpointLeaseId, u32),
    ) {
        if let Some(endpoint) =
            unsafe { cluster.public_endpoint_ptr::<ROLE, MintConfig>(rv_id, handle.0, handle.1) }
        {
            unsafe {
                core::ptr::drop_in_place(endpoint);
            }
        }
        cluster.release_public_endpoint_slot_owned(rv_id, handle.0, handle.1);
    }

    unsafe fn drop_test_public_endpoint<const MAX_RV: usize>(
        cluster: &'static StaticTestCluster<MAX_RV>,
        rv_id: RendezvousId,
        handle: (crate::rendezvous::core::EndpointLeaseId, u32),
    ) {
        unsafe {
            drop_test_public_endpoint_for_role::<0, MAX_RV>(cluster, rv_id, handle);
        }
    }

    fn run_on_transient_compiled_test_stack<F>(name: &'static str, test: F)
    where
        F: FnOnce() + Send + 'static,
    {
        let _ = name;
        test();
    }

    fn route_resolver(_ctx: ResolverContext) -> ResolverResult {
        Ok(DynamicResolution::RouteArm { arm: 0 })
    }

    #[test]
    fn topology_begin_and_ack_execute_without_hidden_dispatch() {
        run_on_transient_compiled_test_stack(
            "topology_begin_and_ack_execute_without_hidden_dispatch",
            || {
                use crate::control::cap::atomic_codecs::TopologyHandle;

                with_cluster_fixture_pair(|clock, src_cfg, dst_cfg| {
                    with_test_cluster(clock, |cluster| {
                        let src_id = cluster
                            .add_rendezvous_from_config(src_cfg, DummyTransport)
                            .expect("register src");
                        let dst_id = cluster
                            .add_rendezvous_from_config(dst_cfg, DummyTransport)
                            .expect("register dst");

                        let sid = SessionId::new(7);
                        let (src_handle, src_lane) = attach_session_lane(cluster, src_id, sid);
                        let dst_lane = Lane::new(1);
                        let operands = TopologyOperands::new(
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
                            .run_effect_step(src_id, CpCommand::topology_begin(sid, operands))
                            .expect("begin effect");
                        assert!(matches!(pending, PendingEffect::None));

                        let handle = TopologyHandle {
                            src_rv: src_id.raw(),
                            dst_rv: dst_id.raw(),
                            src_lane: src_lane.raw() as u16,
                            dst_lane: dst_lane.raw() as u16,
                            old_gen: operands.old_gen.raw(),
                            new_gen: operands.new_gen.raw(),
                            seq_tx: operands.seq_tx,
                            seq_rx: operands.seq_rx,
                        };
                        let decoded = TopologyHandle::decode(handle.encode())
                            .expect("decode topology handle");

                        cluster
                            .dispatch_topology_ack_with_handle(dst_id, sid, dst_lane, decoded, None)
                            .expect("dispatch succeeds");

                        let sid_fail = SessionId::new(9);
                        let operands_fail = TopologyOperands::new(
                            src_id,
                            dst_id,
                            src_lane,
                            dst_lane,
                            Generation::new(1),
                            Generation::new(2),
                            0,
                            0,
                        );

                        let err = cluster
                            .run_effect_step(
                                src_id,
                                CpCommand::topology_begin(sid_fail, operands_fail),
                            )
                            .expect_err("second begin must fail while begin+ack remains in-flight");
                        assert!(
                            matches!(
                                err,
                                CpError::Topology(
                                    crate::control::cluster::error::TopologyError::InProgress
                                        | crate::control::cluster::error::TopologyError::LaneMismatch
                                        | crate::control::cluster::error::TopologyError::StaleGeneration
                                        | crate::control::cluster::error::TopologyError::InvalidState
                                        | crate::control::cluster::error::TopologyError::InvalidSession
                                )
                            ),
                            "error was {:?}",
                            err
                        );

                        unsafe {
                            drop_test_public_endpoint(cluster, src_id, src_handle);
                        }
                    });
                });
            },
        );
    }

    #[test]
    fn topology_begin_rejects_target_mismatch_before_mutation() {
        run_on_transient_compiled_test_stack(
            "topology_begin_rejects_target_mismatch_before_mutation",
            || {
                with_cluster_fixture_pair(|clock, src_cfg, dst_cfg| {
                    with_test_cluster(clock, |cluster| {
                        let src_id = cluster
                            .add_rendezvous_from_config(src_cfg, DummyTransport)
                            .expect("register src");
                        let dst_id = cluster
                            .add_rendezvous_from_config(dst_cfg, DummyTransport)
                            .expect("register dst");

                        let sid = SessionId::new(15);
                        let src_lane = Lane::new(0);
                        let dst_lane = Lane::new(1);
                        let operands = TopologyOperands::new(
                            src_id,
                            dst_id,
                            src_lane,
                            dst_lane,
                            Generation::new(0),
                            Generation::new(1),
                            0,
                            0,
                        );

                        let err = cluster
                            .run_effect_step(dst_id, CpCommand::topology_begin(sid, operands))
                            .expect_err("wrong target must fail before any topology mutation");
                        assert_eq!(
                            err,
                            CpError::RendezvousMismatch {
                                expected: src_id.raw(),
                                actual: dst_id.raw(),
                            }
                        );

                        cluster.with_control_mut(|core| {
                            assert!(
                                core.topology_state.get(sid).is_none(),
                                "distributed bookkeeping must stay empty after target mismatch",
                            );
                        });
                        assert_eq!(
                            cluster
                                .get_local(&src_id)
                                .expect("registered source rendezvous")
                                .lane_generation(src_lane),
                            Generation::ZERO
                        );
                        assert_eq!(
                            cluster
                                .get_local(&dst_id)
                                .expect("registered destination rendezvous")
                                .lane_generation(src_lane),
                            Generation::ZERO
                        );
                    });
                });
            },
        );
    }

    #[test]
    fn topology_handle_validation_rejects_same_rendezvous_before_mutation() {
        run_on_transient_compiled_test_stack(
            "topology_handle_validation_rejects_same_rendezvous_before_mutation",
            || {
                use crate::control::cap::atomic_codecs::TopologyHandle;

                with_cluster_fixture_pair(|clock, src_cfg, _dst_cfg| {
                    with_test_cluster(clock, |cluster| {
                        let rv_id = cluster
                            .add_rendezvous_from_config(src_cfg, DummyTransport)
                            .expect("register rendezvous");

                        let sid = SessionId::new(16);
                        let (src_handle, src_lane) = attach_session_lane(cluster, rv_id, sid);
                        let dst_lane = Lane::new(1);
                        let handle = TopologyHandle {
                            src_rv: rv_id.raw(),
                            dst_rv: rv_id.raw(),
                            src_lane: src_lane.raw() as u16,
                            dst_lane: dst_lane.raw() as u16,
                            old_gen: 0,
                            new_gen: 1,
                            seq_tx: 0,
                            seq_rx: 0,
                        };

                        assert_eq!(
                            cluster
                                .validate_topology_begin_with_handle(rv_id, src_lane, handle, None),
                            Err(CpError::Authorisation {
                                operation: ControlOp::TopologyBegin as u8,
                            }),
                            "same-rendezvous topology begin must be rejected at descriptor validation",
                        );
                        assert_eq!(
                            cluster
                                .validate_topology_ack_with_handle(rv_id, dst_lane, handle, None),
                            Err(CpError::Authorisation {
                                operation: ControlOp::TopologyAck as u8,
                            }),
                            "same-rendezvous topology ack must be rejected at descriptor validation",
                        );
                        assert_eq!(
                            cluster.validate_topology_commit_with_handle(
                                rv_id, src_lane, handle, None
                            ),
                            Err(CpError::Authorisation {
                                operation: ControlOp::TopologyCommit as u8,
                            }),
                            "same-rendezvous topology commit must be rejected at descriptor validation",
                        );

                        cluster.with_control_mut(|core| {
                            assert!(
                                core.topology_state.get(sid).is_none(),
                                "same-rendezvous validation must not create distributed topology state",
                            );
                        });
                        assert_eq!(
                            cluster
                                .get_local(&rv_id)
                                .expect("registered rendezvous")
                                .topology_session_state(sid),
                            None,
                            "same-rendezvous validation must not stage local topology state",
                        );

                        unsafe {
                            drop_test_public_endpoint(cluster, rv_id, src_handle);
                        }
                    });
                });
            },
        );
    }

    #[test]
    fn topology_begin_rejects_stale_source_generation_before_mutation() {
        run_on_transient_compiled_test_stack(
            "topology_begin_rejects_stale_source_generation_before_mutation",
            || {
                with_cluster_fixture_pair(|clock, src_cfg, dst_cfg| {
                    with_test_cluster(clock, |cluster| {
                        let src_id = cluster
                            .add_rendezvous_from_config(src_cfg, DummyTransport)
                            .expect("register src");
                        let dst_id = cluster
                            .add_rendezvous_from_config(dst_cfg, DummyTransport)
                            .expect("register dst");

                        let sid = SessionId::new(16);
                        let (src_handle, src_lane) = attach_session_lane(cluster, src_id, sid);
                        let dst_lane = Lane::new(1);

                        advance_lane_generation(cluster, src_id, src_lane, Generation::new(1));

                        let stale = TopologyOperands::new(
                            src_id,
                            dst_id,
                            src_lane,
                            dst_lane,
                            Generation::new(0),
                            Generation::new(2),
                            0,
                            0,
                        );

                        let err = cluster
                            .run_effect_step(src_id, CpCommand::topology_begin(sid, stale))
                            .expect_err("stale predecessor generation must fail before mutation");
                        assert!(matches!(
                            err,
                            CpError::Topology(TopologyError::StaleGeneration)
                        ));

                        cluster.with_control_mut(|core| {
                            assert!(
                                core.topology_state.get(sid).is_none(),
                                "distributed bookkeeping must stay empty after stale begin",
                            );
                        });
                        assert_eq!(
                            cluster
                                .get_local(&src_id)
                                .expect("registered source rendezvous")
                                .lane_generation(src_lane),
                            Generation::new(1)
                        );

                        let correct = TopologyOperands::new(
                            src_id,
                            dst_id,
                            src_lane,
                            dst_lane,
                            Generation::new(1),
                            Generation::new(2),
                            0,
                            0,
                        );

                        let pending = cluster
                            .run_effect_step(src_id, CpCommand::topology_begin(sid, correct))
                            .expect("correct predecessor generation must still succeed");
                        assert!(matches!(pending, PendingEffect::None));

                        unsafe {
                            drop_test_public_endpoint(cluster, src_id, src_handle);
                        }
                    });
                });
            },
        );
    }

    #[test]
    fn topology_ack_preflight_rejects_mismatch_before_destination_mutation() {
        run_on_transient_compiled_test_stack(
            "topology_ack_preflight_rejects_mismatch_before_destination_mutation",
            || {
                use crate::control::cap::atomic_codecs::TopologyHandle;

                with_cluster_fixture_pair(|clock, src_cfg, dst_cfg| {
                    with_test_cluster(clock, |cluster| {
                        let src_id = cluster
                            .add_rendezvous_from_config(src_cfg, DummyTransport)
                            .expect("register src");
                        let dst_id = cluster
                            .add_rendezvous_from_config(dst_cfg, DummyTransport)
                            .expect("register dst");

                        let sid = SessionId::new(17);
                        let (src_handle, src_lane) = attach_session_lane(cluster, src_id, sid);
                        let dst_lane = Lane::new(1);
                        let operands = TopologyOperands::new(
                            src_id,
                            dst_id,
                            src_lane,
                            dst_lane,
                            Generation::new(0),
                            Generation::new(1),
                            0,
                            0,
                        );

                        cluster
                            .run_effect_step(src_id, CpCommand::topology_begin(sid, operands))
                            .expect("begin effect");

                        let bad_handle = TopologyHandle {
                            src_rv: src_id.raw(),
                            dst_rv: dst_id.raw(),
                            src_lane: src_lane.raw() as u16,
                            dst_lane: dst_lane.raw() as u16,
                            old_gen: operands.old_gen.raw(),
                            new_gen: Generation::new(2).raw(),
                            seq_tx: operands.seq_tx,
                            seq_rx: operands.seq_rx,
                        };

                        let err = cluster
                            .dispatch_topology_ack_with_handle(
                                dst_id,
                                sid,
                                dst_lane,
                                TopologyHandle::decode(bad_handle.encode())
                                    .expect("decode topology handle"),
                                None,
                            )
                            .expect_err("mismatched ack must fail before mutating destination");
                        assert!(matches!(
                            err,
                            CpError::Topology(TopologyError::GenerationMismatch)
                        ));

                        let good_handle = TopologyHandle {
                            src_rv: src_id.raw(),
                            dst_rv: dst_id.raw(),
                            src_lane: src_lane.raw() as u16,
                            dst_lane: dst_lane.raw() as u16,
                            old_gen: operands.old_gen.raw(),
                            new_gen: operands.new_gen.raw(),
                            seq_tx: operands.seq_tx,
                            seq_rx: operands.seq_rx,
                        };

                        cluster
                            .dispatch_topology_ack_with_handle(
                                dst_id,
                                sid,
                                dst_lane,
                                TopologyHandle::decode(good_handle.encode())
                                    .expect("decode topology handle"),
                                None,
                            )
                            .expect("correct ack must still succeed after the rejected attempt");

                        unsafe {
                            drop_test_public_endpoint(cluster, src_id, src_handle);
                        }
                    });
                });
            },
        );
    }

    #[test]
    fn topology_commit_preflight_rejects_mismatch_before_source_mutation() {
        run_on_transient_compiled_test_stack(
            "topology_commit_preflight_rejects_mismatch_before_source_mutation",
            || {
                use crate::control::cap::atomic_codecs::TopologyHandle;

                with_cluster_fixture_pair(|clock, src_cfg, dst_cfg| {
                    with_test_cluster(clock, |cluster| {
                        let src_id = cluster
                            .add_rendezvous_from_config(src_cfg, DummyTransport)
                            .expect("register src");
                        let dst_id = cluster
                            .add_rendezvous_from_config(dst_cfg, DummyTransport)
                            .expect("register dst");

                        let sid = SessionId::new(19);
                        let (src_handle, src_lane) = attach_session_lane(cluster, src_id, sid);
                        let dst_lane = Lane::new(1);
                        let operands = TopologyOperands::new(
                            src_id,
                            dst_id,
                            src_lane,
                            dst_lane,
                            Generation::new(0),
                            Generation::new(1),
                            0,
                            0,
                        );

                        cluster
                            .run_effect_step(src_id, CpCommand::topology_begin(sid, operands))
                            .expect("begin effect");

                        let handle = TopologyHandle {
                            src_rv: src_id.raw(),
                            dst_rv: dst_id.raw(),
                            src_lane: src_lane.raw() as u16,
                            dst_lane: dst_lane.raw() as u16,
                            old_gen: operands.old_gen.raw(),
                            new_gen: operands.new_gen.raw(),
                            seq_tx: operands.seq_tx,
                            seq_rx: operands.seq_rx,
                        };
                        cluster
                            .dispatch_topology_ack_with_handle(
                                dst_id,
                                sid,
                                dst_lane,
                                TopologyHandle::decode(handle.encode())
                                    .expect("decode topology handle"),
                                None,
                            )
                            .expect("ack succeeds");
                        assert_eq!(
                            cluster
                                .get_local(&src_id)
                                .expect("source rendezvous")
                                .expected_topology_ack(sid),
                            Ok(operands.ack(sid)),
                            "ack must preserve source-side expected ACK until commit",
                        );

                        let bad_operands = TopologyOperands::new(
                            src_id,
                            dst_id,
                            src_lane,
                            dst_lane,
                            operands.old_gen,
                            Generation::new(2),
                            operands.seq_tx,
                            operands.seq_rx,
                        );

                        let err = cluster
                            .run_effect_step(src_id, CpCommand::topology_commit(sid, bad_operands))
                            .expect_err("mismatched commit must fail before mutating source");
                        assert!(matches!(
                            err,
                            CpError::Topology(TopologyError::CommitFailed)
                        ));
                        assert!(
                            matches!(
                                cluster
                                    .get_local(&src_id)
                                    .expect("source rendezvous")
                                    .expected_topology_ack(sid),
                                Err(crate::rendezvous::error::TopologyError::UnknownSession { .. })
                            ),
                            "rejected commit must abort the source-side topology owner instead of preserving a wedged retry path",
                        );
                        assert!(
                            cluster
                                .get_local(&dst_id)
                                .expect("destination rendezvous")
                                .preflight_destination_topology_commit(sid, dst_lane)
                                .is_err(),
                            "rejected commit must roll back destination prepare state",
                        );
                        assert_eq!(
                            cluster.distributed_topology_operands(sid),
                            None,
                            "rejected commit must clear cluster-owned distributed topology state",
                        );
                        assert!(
                            unsafe {
                                cluster
                                    .public_endpoint_ptr::<0, MintConfig>(
                                        src_id,
                                        src_handle.0,
                                        src_handle.1,
                                    )
                                    .is_some()
                            },
                            "rejected commit must not revoke the source public endpoint",
                        );

                        unsafe {
                            drop_test_public_endpoint(cluster, src_id, src_handle);
                        }
                    });
                });
            },
        );
    }

    #[test]
    fn destination_attach_aborts_acked_topology_before_retry() {
        run_on_transient_compiled_test_stack(
            "destination_attach_aborts_acked_topology_before_retry",
            || {
                use crate::control::cap::atomic_codecs::TopologyHandle;

                with_cluster_fixture_pair(|clock, src_cfg, dst_cfg| {
                    with_test_cluster(clock, |cluster| {
                        let src_id = cluster
                            .add_rendezvous_from_config(src_cfg, DummyTransport)
                            .expect("register src");
                        let dst_id = cluster
                            .add_rendezvous_from_config(dst_cfg, DummyTransport)
                            .expect("register dst");

                        let sid = SessionId::new(21);
                        let (src_handle, src_lane) = attach_session_lane(cluster, src_id, sid);
                        let dst_lane = Lane::new(1);
                        let operands = TopologyOperands::new(
                            src_id,
                            dst_id,
                            src_lane,
                            dst_lane,
                            Generation::new(0),
                            Generation::new(1),
                            0,
                            0,
                        );

                        cluster
                            .run_effect_step(src_id, CpCommand::topology_begin(sid, operands))
                            .expect("begin effect");
                        let handle = TopologyHandle {
                            src_rv: src_id.raw(),
                            dst_rv: dst_id.raw(),
                            src_lane: src_lane.raw() as u16,
                            dst_lane: dst_lane.raw() as u16,
                            old_gen: operands.old_gen.raw(),
                            new_gen: operands.new_gen.raw(),
                            seq_tx: operands.seq_tx,
                            seq_rx: operands.seq_rx,
                        };
                        cluster
                            .dispatch_topology_ack_with_handle(
                                dst_id,
                                sid,
                                dst_lane,
                                TopologyHandle::decode(handle.encode())
                                    .expect("decode topology handle"),
                                None,
                            )
                            .expect("ack succeeds");
                        assert!(
                            !cluster
                                .get_local(&dst_id)
                                .expect("destination rendezvous")
                                .is_session_registered(sid),
                            "destination ack must keep the destination rendezvous provisional until source commit",
                        );
                        assert_eq!(
                            cluster
                                .get_local(&dst_id)
                                .expect("destination rendezvous")
                                .session_lane(sid),
                            None,
                            "destination ack must not expose a committed session lane before source commit",
                        );

                        assert_eq!(
                            cluster.distributed_topology_operands(sid),
                            Some(operands),
                            "distributed topology state must own the in-flight session until source commit",
                        );
                        let err = cluster.enter(
                            dst_id,
                            sid,
                            &attach_program(),
                            crate::binding::BindingHandle::None(crate::binding::NoBinding),
                        );
                        assert!(matches!(
                            err,
                            Err(AttachError::Control(CpError::Topology(
                                TopologyError::InvalidState,
                            )))
                        ));
                        assert!(
                            !cluster
                                .get_local(&dst_id)
                                .expect("destination rendezvous")
                                .is_session_registered(sid),
                            "rejected destination attach must not make the prepared destination session live",
                        );
                        assert_eq!(
                            cluster.distributed_topology_operands(sid),
                            None,
                            "rejected destination attach must close the cluster-owned topology owner",
                        );
                        assert!(
                            cluster
                                .get_local(&dst_id)
                                .expect("destination rendezvous")
                                .preflight_destination_topology_commit(sid, dst_lane)
                                .is_err(),
                            "rejected destination attach must roll back the destination prepare state",
                        );
                        assert!(
                            matches!(
                                cluster
                                    .get_local(&src_id)
                                    .expect("source rendezvous")
                                    .expected_topology_ack(sid),
                                Err(crate::rendezvous::error::TopologyError::UnknownSession { .. })
                            ),
                            "rejected destination attach must roll back the source-side topology owner",
                        );
                        assert!(
                            unsafe {
                                cluster
                                    .public_endpoint_ptr::<0, MintConfig>(
                                        src_id,
                                        src_handle.0,
                                        src_handle.1,
                                    )
                                    .is_some()
                            },
                            "rejected destination attach must not revoke the existing source public endpoint",
                        );

                        let late_commit = cluster
                            .run_effect_step(src_id, CpCommand::topology_commit(sid, operands))
                            .expect_err(
                                "late source commit must not revive an attach-closed topology",
                            );
                        assert!(matches!(
                            late_commit,
                            CpError::Topology(TopologyError::InvalidSession)
                        ));

                        cluster
                            .run_effect_step(src_id, CpCommand::topology_begin(sid, operands))
                            .expect(
                                "closed acked topology must not block a fresh begin for the sid",
                            );

                        unsafe {
                            drop_test_public_endpoint(cluster, src_id, src_handle);
                        }
                    });
                });
            },
        );
    }

    #[test]
    fn destination_attach_aborts_begin_topology_before_ack_retry() {
        run_on_transient_compiled_test_stack(
            "destination_attach_aborts_begin_topology_before_ack_retry",
            || {
                with_cluster_fixture_pair(|clock, src_cfg, dst_cfg| {
                    with_test_cluster(clock, |cluster| {
                        let src_id = cluster
                            .add_rendezvous_from_config(src_cfg, DummyTransport)
                            .expect("register src");
                        let dst_id = cluster
                            .add_rendezvous_from_config(dst_cfg, DummyTransport)
                            .expect("register dst");

                        let sid = SessionId::new(32);
                        let (src_handle, src_lane) = attach_session_lane(cluster, src_id, sid);
                        let dst_lane = Lane::new(1);
                        let operands = TopologyOperands::new(
                            src_id,
                            dst_id,
                            src_lane,
                            dst_lane,
                            Generation::new(0),
                            Generation::new(1),
                            0,
                            0,
                        );

                        cluster
                            .run_effect_step(src_id, CpCommand::topology_begin(sid, operands))
                            .expect("begin effect");
                        assert_eq!(
                            cluster.distributed_topology_operands(sid),
                            Some(operands),
                            "begin topology must be cluster-owned before closeout",
                        );
                        assert_eq!(
                            cluster
                                .get_local(&src_id)
                                .expect("source rendezvous")
                                .expected_topology_ack(sid),
                            Ok(operands.ack(sid)),
                            "begin topology must stage the source owner before closeout",
                        );
                        assert!(
                            cluster
                                .get_local(&dst_id)
                                .expect("destination rendezvous")
                                .preflight_destination_topology_commit(sid, dst_lane)
                                .is_err(),
                            "begin topology must not expose destination commit before ack",
                        );

                        let err = cluster.enter(
                            dst_id,
                            sid,
                            &attach_program(),
                            crate::binding::BindingHandle::None(crate::binding::NoBinding),
                        );
                        assert!(matches!(
                            err,
                            Err(AttachError::Control(CpError::Topology(
                                TopologyError::InvalidState,
                            )))
                        ));
                        assert_eq!(
                            cluster.distributed_topology_operands(sid),
                            None,
                            "destination attach must close begin-phase distributed topology",
                        );
                        assert!(
                            cluster
                                .get_local(&dst_id)
                                .expect("destination rendezvous")
                                .preflight_destination_topology_commit(sid, dst_lane)
                                .is_err(),
                            "destination attach must roll back begin-phase destination prepare",
                        );
                        assert!(
                            matches!(
                                cluster
                                    .get_local(&src_id)
                                    .expect("source rendezvous")
                                    .expected_topology_ack(sid),
                                Err(crate::rendezvous::error::TopologyError::UnknownSession { .. })
                            ),
                            "destination attach must roll back begin-phase source owner",
                        );

                        let late_ack = cluster
                            .dispatch_topology_ack_with_handle(
                                dst_id,
                                sid,
                                dst_lane,
                                topology_handle(operands),
                                None,
                            )
                            .expect_err("late ack must not revive an attach-closed topology");
                        assert!(matches!(
                            late_ack,
                            CpError::Topology(TopologyError::InvalidSession)
                        ));

                        cluster
                            .run_effect_step(src_id, CpCommand::topology_begin(sid, operands))
                            .expect(
                                "closed begin topology must not block a fresh begin for the sid",
                            );

                        unsafe {
                            drop_test_public_endpoint(cluster, src_id, src_handle);
                        }
                    });
                });
            },
        );
    }

    #[test]
    fn source_attach_aborts_acked_topology_before_retry() {
        run_on_transient_compiled_test_stack(
            "source_attach_aborts_acked_topology_before_retry",
            || {
                with_cluster_fixture_pair(|clock, src_cfg, dst_cfg| {
                    with_test_cluster(clock, |cluster| {
                        let src_id = cluster
                            .add_rendezvous_from_config(src_cfg, DummyTransport)
                            .expect("register src");
                        let dst_id = cluster
                            .add_rendezvous_from_config(dst_cfg, DummyTransport)
                            .expect("register dst");

                        let sid = SessionId::new(33);
                        let (src_handle, src_lane) = attach_session_lane(cluster, src_id, sid);
                        let dst_lane = Lane::new(1);
                        let operands = TopologyOperands::new(
                            src_id,
                            dst_id,
                            src_lane,
                            dst_lane,
                            Generation::new(0),
                            Generation::new(1),
                            0,
                            0,
                        );

                        cluster
                            .run_effect_step(src_id, CpCommand::topology_begin(sid, operands))
                            .expect("begin effect");
                        cluster
                            .dispatch_topology_ack_with_handle(
                                dst_id,
                                sid,
                                dst_lane,
                                topology_handle(operands),
                                None,
                            )
                            .expect("ack succeeds");
                        assert_eq!(
                            cluster.distributed_topology_operands(sid),
                            Some(operands),
                            "acked topology must be cluster-owned before source closeout",
                        );

                        let err = cluster.enter(
                            src_id,
                            sid,
                            &attach_program(),
                            crate::binding::BindingHandle::None(crate::binding::NoBinding),
                        );
                        assert!(matches!(
                            err,
                            Err(AttachError::Control(CpError::Topology(
                                TopologyError::InvalidState,
                            )))
                        ));
                        assert_eq!(
                            cluster.distributed_topology_operands(sid),
                            None,
                            "source attach must close acked distributed topology",
                        );
                        assert!(
                            cluster
                                .get_local(&dst_id)
                                .expect("destination rendezvous")
                                .preflight_destination_topology_commit(sid, dst_lane)
                                .is_err(),
                            "source attach must roll back acked destination prepare",
                        );
                        assert!(
                            matches!(
                                cluster
                                    .get_local(&src_id)
                                    .expect("source rendezvous")
                                    .expected_topology_ack(sid),
                                Err(crate::rendezvous::error::TopologyError::UnknownSession { .. })
                            ),
                            "source attach must roll back acked source owner",
                        );

                        let late_commit = cluster
                            .run_effect_step(src_id, CpCommand::topology_commit(sid, operands))
                            .expect_err(
                                "late source commit must not revive a source-attach-closed topology",
                            );
                        assert!(matches!(
                            late_commit,
                            CpError::Topology(TopologyError::InvalidSession)
                        ));

                        cluster
                            .run_effect_step(src_id, CpCommand::topology_begin(sid, operands))
                            .expect(
                                "source-closed acked topology must not block a fresh begin for the sid",
                            );

                        unsafe {
                            drop_test_public_endpoint(cluster, src_id, src_handle);
                        }
                    });
                });
            },
        );
    }

    #[test]
    fn destination_attach_ready_requires_and_consumes_exact_lane() {
        run_on_transient_compiled_test_stack(
            "destination_attach_ready_requires_and_consumes_exact_lane",
            || {
                use crate::control::cap::atomic_codecs::TopologyHandle;

                with_cluster_fixture_pair(|clock, src_cfg, dst_cfg| {
                    with_test_cluster(clock, |cluster| {
                        let src_id = cluster
                            .add_rendezvous_from_config(src_cfg, DummyTransport)
                            .expect("register src");
                        let dst_id = cluster
                            .add_rendezvous_from_config(dst_cfg, DummyTransport)
                            .expect("register dst");

                        let sid = SessionId::new(22);
                        let (_src_handle, src_lane) = attach_session_lane(cluster, src_id, sid);
                        let dst_lane = Lane::new(1);
                        let operands = TopologyOperands::new(
                            src_id,
                            dst_id,
                            src_lane,
                            dst_lane,
                            Generation::new(0),
                            Generation::new(1),
                            0,
                            0,
                        );

                        cluster
                            .run_effect_step(src_id, CpCommand::topology_begin(sid, operands))
                            .expect("begin effect");
                        let handle = TopologyHandle {
                            src_rv: src_id.raw(),
                            dst_rv: dst_id.raw(),
                            src_lane: src_lane.raw() as u16,
                            dst_lane: dst_lane.raw() as u16,
                            old_gen: operands.old_gen.raw(),
                            new_gen: operands.new_gen.raw(),
                            seq_tx: operands.seq_tx,
                            seq_rx: operands.seq_rx,
                        };
                        let decoded = TopologyHandle::decode(handle.encode())
                            .expect("decode topology handle");
                        cluster
                            .dispatch_topology_ack_with_handle(dst_id, sid, dst_lane, decoded, None)
                            .expect("ack succeeds");
                        cluster
                            .run_effect_step(src_id, CpCommand::topology_commit(sid, operands))
                            .expect("source commit succeeds");
                        assert!(cluster.distributed_topology_operands(sid).is_none());
                        assert!(
                            !cluster
                                .get_local(&dst_id)
                                .expect("destination rendezvous")
                                .is_session_registered(sid),
                            "successful source commit must leave the destination in attach-ready state rather than pre-registering the lane",
                        );
                        assert_eq!(
                            cluster
                                .get_local(&dst_id)
                                .expect("destination rendezvous")
                                .session_lane(sid),
                            None,
                            "destination lane ownership must materialize on the first real attach",
                        );
                        assert_eq!(
                            cluster
                                .get_local(&dst_id)
                                .expect("destination rendezvous")
                                .topology_session_state(sid),
                            Some(TopologySessionState::DestinationAttachReady { lane: dst_lane }),
                            "successful source commit must leave a destination attach-ready reservation owned by topology state",
                        );

                        let partial = cluster.enter(
                            dst_id,
                            sid,
                            &attach_program(),
                            crate::binding::BindingHandle::None(crate::binding::NoBinding),
                        );
                        assert!(matches!(
                            partial,
                            Err(AttachError::Control(CpError::Topology(
                                TopologyError::InvalidState,
                            )))
                        ));
                        assert!(
                            !cluster
                                .get_local(&dst_id)
                                .expect("destination rendezvous")
                                .is_session_registered(sid),
                            "partial attach must not make the destination session live",
                        );
                        assert_eq!(
                            cluster
                                .get_local(&dst_id)
                                .expect("destination rendezvous")
                                .topology_session_state(sid),
                            Some(TopologySessionState::DestinationAttachReady { lane: dst_lane }),
                            "partial attach must preserve the exact-lane reservation for retry",
                        );

                        let dst_handle = cluster
                            .enter(
                                dst_id,
                                sid,
                                &lane1_worker_program(),
                                crate::binding::BindingHandle::None(crate::binding::NoBinding),
                            )
                            .expect("exact destination attach must open after source commit");
                        assert!(
                            cluster
                                .get_local(&dst_id)
                                .expect("destination rendezvous")
                                .is_session_registered(sid),
                            "first attach must materialize the migrated destination lane",
                        );
                        assert_eq!(
                            cluster
                                .get_local(&dst_id)
                                .expect("destination rendezvous")
                                .topology_session_state(sid),
                            None,
                            "exact-lane attach must consume the migrated destination lane reservation",
                        );
                        assert_eq!(
                            cluster
                                .get_local(&dst_id)
                                .expect("destination rendezvous")
                                .lane_generation(dst_lane),
                            operands.new_gen,
                            "first attach after topology commit must preserve the committed destination generation",
                        );

                        assert!(
                            cluster
                                .get_local(&dst_id)
                                .expect("destination rendezvous")
                                .preflight_destination_topology_commit(sid, dst_lane)
                                .is_err(),
                            "consumed attach-ready state must not keep blocking future topology as in-progress",
                        );

                        unsafe {
                            drop_test_public_endpoint_for_role::<1, 4>(cluster, dst_id, dst_handle);
                        }
                    });
                });
            },
        );
    }

    #[test]
    fn enter_rolls_back_orphaned_destination_prepare_without_cluster_topology_state() {
        run_on_transient_compiled_test_stack(
            "enter_rolls_back_orphaned_destination_prepare_without_cluster_topology_state",
            || {
                with_cluster_fixture(|clock, dst_cfg| {
                    with_test_cluster(clock, |cluster| {
                        let dst_id = cluster
                            .add_rendezvous_from_config(dst_cfg, DummyTransport)
                            .expect("register destination rendezvous");

                        let sid = SessionId::new(22);
                        let dst_lane = Lane::new(1);
                        cluster.with_control_mut(|core| {
                            let rv = core
                                .locals
                                .get_mut(&dst_id)
                                .expect("destination rendezvous");
                            rv.prepare_topology_control_scope(dst_lane)
                                .expect("orphan prepare test must bind topology storage");
                            rv.process_topology_intent(
                                &crate::control::automaton::distributed::TopologyIntent::new(
                                    RendezvousId::new(99),
                                    dst_id,
                                    sid.raw(),
                                    Generation::ZERO,
                                    Generation::new(1),
                                    0,
                                    0,
                                    Lane::new(0),
                                    dst_lane,
                                ),
                            )
                            .expect("direct orphan prepare must stage destination pending state");
                        });

                        assert_eq!(
                            cluster.distributed_topology_operands(sid),
                            None,
                            "orphan prepare test must not create cluster-owned distributed topology state",
                        );
                        assert_eq!(
                            cluster
                                .get_local(&dst_id)
                                .expect("destination rendezvous")
                                .preflight_destination_topology_commit(sid, dst_lane),
                            Ok(()),
                            "direct orphan prepare must leave destination pending before attach cleanup",
                        );

                        let handle = cluster
                            .enter(
                                dst_id,
                                sid,
                                &lane1_worker_program(),
                                crate::binding::BindingHandle::None(crate::binding::NoBinding),
                            )
                            .expect(
                                "attach must roll back orphaned destination prepare before materializing the endpoint",
                            );

                        assert!(
                            cluster
                                .get_local(&dst_id)
                                .expect("destination rendezvous")
                                .is_session_registered(sid),
                            "attach must still register the session after orphan cleanup",
                        );
                        assert_eq!(
                            cluster
                                .get_local(&dst_id)
                                .expect("destination rendezvous")
                                .rollback_destination_topology_prepare(sid),
                            Ok(false),
                            "orphan cleanup must consume the stale prepared topology before attach completes",
                        );

                        unsafe {
                            drop_test_public_endpoint(cluster, dst_id, handle);
                        }
                    });
                });
            },
        );
    }

    #[test]
    fn direct_topology_begin_is_rejected_before_source_pending_state() {
        run_on_transient_compiled_test_stack(
            "direct_topology_begin_is_rejected_before_source_pending_state",
            || {
                with_cluster_fixture_pair(|clock, src_cfg, dst_cfg| {
                    with_test_cluster(clock, |cluster| {
                        let src_id = cluster
                            .add_rendezvous_from_config(src_cfg, DummyTransport)
                            .expect("register src");
                        let dst_id = cluster
                            .add_rendezvous_from_config(dst_cfg, DummyTransport)
                            .expect("register dst");

                        let sid = SessionId::new(35);
                        let (src_handle, src_lane) = attach_session_lane(cluster, src_id, sid);
                        let operands = TopologyOperands::new(
                            src_id,
                            dst_id,
                            src_lane,
                            Lane::new(1),
                            Generation::new(0),
                            Generation::new(1),
                            0,
                            0,
                        );

                        cluster
                            .with_control_mut(|core| {
                                let rv = core.locals.get_mut(&src_id).expect("source rendezvous");
                                rv.prepare_topology_control_scope(src_lane)
                                    .expect("source topology begin must bind topology storage");
                                EffectRunner::run_effect(rv, CpCommand::topology_begin(sid, operands))
                            })
                            .expect_err(
                                "direct topology begin must stay cluster-owned and reject before mutation",
                            );

                        assert_eq!(
                            cluster.distributed_topology_operands(sid),
                            None,
                            "direct topology begin rejection must not create cluster-owned distributed state",
                        );
                        assert_eq!(
                            cluster
                                .get_local(&src_id)
                                .expect("source rendezvous")
                                .topology_session_state(sid),
                            None,
                            "direct topology begin rejection must not strand source-pending topology state",
                        );
                        assert!(
                            matches!(
                                cluster
                                    .get_local(&src_id)
                                    .expect("source rendezvous")
                                    .expected_topology_ack(sid),
                                Err(crate::rendezvous::error::TopologyError::UnknownSession { .. })
                            ),
                            "direct topology begin rejection must not install a topology owner",
                        );

                        unsafe {
                            drop_test_public_endpoint(cluster, src_id, src_handle);
                        }
                    });
                });
            },
        );
    }

    #[test]
    fn destination_attach_after_topology_commit_still_initializes_effect_image() {
        run_on_transient_compiled_test_stack(
            "destination_attach_after_topology_commit_still_initializes_effect_image",
            || {
                use crate::control::cap::atomic_codecs::TopologyHandle;

                with_cluster_fixture_pair(|clock, src_cfg, dst_cfg| {
                    with_test_cluster(clock, |cluster| {
                        let src_id = cluster
                            .add_rendezvous_from_config(src_cfg, DummyTransport)
                            .expect("register src");
                        let dst_id = cluster
                            .add_rendezvous_from_config(dst_cfg, DummyTransport)
                            .expect("register dst");

                        let route_policy_program = route_policy_program_one();
                        let route_policy_projected: SharedBorrowRoleProgram =
                            role_program::project(&route_policy_program);
                        let role_image = cluster
                            .ensure_role_image_slice(dst_id, &route_policy_projected)
                            .expect("materialize destination role image");
                        let program_image = role_image.program();
                        let effect_envelope = program_image.effect_envelope();
                        let descriptor = *effect_envelope
                            .resources()
                            .next()
                            .expect("route-policy program must expose a control resource");
                        let expected_policy = effect_envelope.resource_policy(&descriptor);

                        let sid = SessionId::new(31);
                        let (src_handle, src_lane) = attach_session_lane_for_program::<0>(
                            cluster,
                            src_id,
                            sid,
                            &route_policy_projected,
                        );
                        let dst_lane = Lane::new(0);
                        let operands = TopologyOperands::new(
                            src_id,
                            dst_id,
                            src_lane,
                            dst_lane,
                            Generation::new(0),
                            Generation::new(1),
                            0,
                            0,
                        );

                        cluster
                            .run_effect_step(src_id, CpCommand::topology_begin(sid, operands))
                            .expect("begin effect");
                        cluster
                            .dispatch_topology_ack_with_handle(
                                dst_id,
                                sid,
                                dst_lane,
                                TopologyHandle {
                                    src_rv: src_id.raw(),
                                    dst_rv: dst_id.raw(),
                                    src_lane: src_lane.raw() as u16,
                                    dst_lane: dst_lane.raw() as u16,
                                    old_gen: operands.old_gen.raw(),
                                    new_gen: operands.new_gen.raw(),
                                    seq_tx: operands.seq_tx,
                                    seq_rx: operands.seq_rx,
                                },
                                None,
                            )
                            .expect("ack succeeds");
                        cluster
                            .run_effect_step(src_id, CpCommand::topology_commit(sid, operands))
                            .expect("commit succeeds");

                        let control_lane = Lane::new(0);
                        assert_eq!(
                            cluster
                                .get_local(&dst_id)
                                .expect("destination rendezvous")
                                .policy(control_lane, descriptor.eff_index(), descriptor.tag()),
                            None,
                            "topology finalization alone must not masquerade as endpoint effect initialization",
                        );

                        let dst_handle = cluster
                            .enter(
                                dst_id,
                                sid,
                                &route_policy_projected,
                                crate::binding::BindingHandle::None(crate::binding::NoBinding),
                            )
                            .expect("destination attach must succeed after source commit");

                        assert_eq!(
                            cluster
                                .get_local(&dst_id)
                                .expect("destination rendezvous")
                                .policy(control_lane, descriptor.eff_index(), descriptor.tag()),
                            Some(expected_policy),
                            "destination attach must still install the compiled effect image even when topology commit pre-registered the migrated session",
                        );

                        unsafe {
                            drop_test_public_endpoint(cluster, src_id, src_handle);
                            drop_test_public_endpoint(cluster, dst_id, dst_handle);
                        }
                    });
                });
            },
        );
    }

    #[test]
    fn topology_commit_run_effect_rejects_direct_commit_outside_cluster_owner() {
        run_on_transient_compiled_test_stack(
            "topology_commit_run_effect_rejects_direct_commit_outside_cluster_owner",
            || {
                use crate::control::cap::atomic_codecs::TopologyHandle;

                with_cluster_fixture_pair(|clock, src_cfg, dst_cfg| {
                    with_test_cluster(clock, |cluster| {
                        let src_id = cluster
                            .add_rendezvous_from_config(src_cfg, DummyTransport)
                            .expect("register src");
                        let dst_id = cluster
                            .add_rendezvous_from_config(dst_cfg, DummyTransport)
                            .expect("register dst");

                        let sid = SessionId::new(23);
                        let (src_handle, src_lane) = attach_session_lane(cluster, src_id, sid);
                        let dst_lane = Lane::new(1);
                        let operands = TopologyOperands::new(
                            src_id,
                            dst_id,
                            src_lane,
                            dst_lane,
                            Generation::new(0),
                            Generation::new(1),
                            0,
                            0,
                        );

                        cluster
                            .run_effect_step(src_id, CpCommand::topology_begin(sid, operands))
                            .expect("begin effect");

                        let handle = TopologyHandle {
                            src_rv: src_id.raw(),
                            dst_rv: dst_id.raw(),
                            src_lane: src_lane.raw() as u16,
                            dst_lane: dst_lane.raw() as u16,
                            old_gen: operands.old_gen.raw(),
                            new_gen: operands.new_gen.raw(),
                            seq_tx: operands.seq_tx,
                            seq_rx: operands.seq_rx,
                        };
                        cluster
                            .dispatch_topology_ack_with_handle(
                                dst_id,
                                sid,
                                dst_lane,
                                TopologyHandle::decode(handle.encode())
                                    .expect("decode topology handle"),
                                None,
                            )
                            .expect("ack succeeds");

                        assert!(
                            unsafe {
                                cluster
                                    .public_endpoint_ptr::<0, MintConfig>(
                                        src_id,
                                        src_handle.0,
                                        src_handle.1,
                                    )
                                    .is_some()
                            },
                            "source endpoint must be live before the rejected direct commit",
                        );

                        assert_eq!(
                            cluster.with_control_mut(|core| {
                                let rv = core.locals.get_mut(&src_id).expect("source rendezvous");
                                EffectRunner::run_effect(
                                    rv,
                                    CpCommand::topology_commit(sid, operands),
                                )
                            }),
                            Err(CpError::Topology(TopologyError::InvalidState)),
                            "direct rendezvous commit must fail closed because distributed topology commit is cluster-owned",
                        );

                        assert!(
                            unsafe {
                                cluster
                                    .public_endpoint_ptr::<0, MintConfig>(
                                        src_id,
                                        src_handle.0,
                                        src_handle.1,
                                    )
                                    .is_some()
                            },
                            "rejected direct commit must not revoke the source public endpoint",
                        );
                        assert_eq!(
                            cluster
                                .get_local(&src_id)
                                .expect("source rendezvous")
                                .session_lane(sid),
                            Some(src_lane),
                            "rejected direct commit must not retire the source lane",
                        );
                        assert_eq!(
                            cluster.distributed_topology_operands(sid),
                            Some(operands),
                            "rejected direct commit must preserve the distributed topology owner",
                        );
                        assert!(
                            cluster
                                .get_local(&dst_id)
                                .expect("destination rendezvous")
                                .preflight_destination_topology_commit(sid, dst_lane)
                                .is_ok(),
                            "rejected direct commit must leave the destination pending topology intact",
                        );

                        cluster
                            .run_effect_step(src_id, CpCommand::topology_commit(sid, operands))
                            .expect("cluster-owned topology commit must succeed");

                        assert!(
                            unsafe {
                                cluster
                                    .public_endpoint_ptr::<0, MintConfig>(
                                        src_id,
                                        src_handle.0,
                                        src_handle.1,
                                    )
                                    .is_none()
                            },
                            "cluster-owned topology commit must revoke the source public endpoint",
                        );
                        assert_eq!(
                            cluster
                                .get_local(&src_id)
                                .expect("source rendezvous")
                                .session_lane(sid),
                            None,
                            "cluster-owned topology commit must retire the source lane",
                        );
                        assert!(
                            !cluster
                                .get_local(&dst_id)
                                .expect("destination rendezvous")
                                .is_session_registered(sid),
                            "cluster-owned topology commit must leave the destination attach-ready rather than pre-registering the session",
                        );
                        assert_eq!(
                            cluster
                                .get_local(&dst_id)
                                .expect("destination rendezvous")
                                .topology_session_state(sid),
                            Some(TopologySessionState::DestinationAttachReady { lane: dst_lane }),
                            "cluster-owned topology commit must transfer destination ownership into topology attach-ready state",
                        );
                    });
                });
            },
        );
    }

    #[test]
    fn topology_commit_revokes_nonzero_role_public_endpoint() {
        run_on_transient_compiled_test_stack(
            "topology_commit_revokes_nonzero_role_public_endpoint",
            || {
                use crate::control::cap::atomic_codecs::TopologyHandle;

                with_cluster_fixture_pair(|clock, src_cfg, dst_cfg| {
                    with_test_cluster(clock, |cluster| {
                        let src_id = cluster
                            .add_rendezvous_from_config(src_cfg, DummyTransport)
                            .expect("register src");
                        let dst_id = cluster
                            .add_rendezvous_from_config(dst_cfg, DummyTransport)
                            .expect("register dst");

                        let sid = SessionId::new(24);
                        let worker_program = linear_program::worker_program();
                        let (src_handle, src_lane) = attach_session_lane_for_program::<1>(
                            cluster,
                            src_id,
                            sid,
                            &worker_program,
                        );
                        let dst_lane = Lane::new(1);
                        let operands = TopologyOperands::new(
                            src_id,
                            dst_id,
                            src_lane,
                            dst_lane,
                            Generation::new(0),
                            Generation::new(1),
                            0,
                            0,
                        );

                        cluster
                            .run_effect_step(src_id, CpCommand::topology_begin(sid, operands))
                            .expect("begin effect");

                        let handle = TopologyHandle {
                            src_rv: src_id.raw(),
                            dst_rv: dst_id.raw(),
                            src_lane: src_lane.raw() as u16,
                            dst_lane: dst_lane.raw() as u16,
                            old_gen: operands.old_gen.raw(),
                            new_gen: operands.new_gen.raw(),
                            seq_tx: operands.seq_tx,
                            seq_rx: operands.seq_rx,
                        };
                        cluster
                            .dispatch_topology_ack_with_handle(
                                dst_id,
                                sid,
                                dst_lane,
                                TopologyHandle::decode(handle.encode())
                                    .expect("decode topology handle"),
                                None,
                            )
                            .expect("ack succeeds");

                        cluster
                            .run_effect_step(src_id, CpCommand::topology_commit(sid, operands))
                            .expect("commit succeeds");

                        assert!(
                            unsafe {
                                cluster
                                    .public_endpoint_ptr::<1, MintConfig>(
                                        src_id,
                                        src_handle.0,
                                        src_handle.1,
                                    )
                                    .is_none()
                            },
                            "commit must revoke a nonzero-role source endpoint without role punning",
                        );
                        assert_eq!(
                            cluster
                                .get_local(&src_id)
                                .expect("source rendezvous")
                                .session_lane(sid),
                            None,
                            "commit must retire the worker-owned source lane",
                        );
                    });
                });
            },
        );
    }

    #[test]
    fn topology_commit_revokes_same_session_endpoints_even_when_only_one_owns_source_lane() {
        run_on_transient_compiled_test_stack(
            "topology_commit_revokes_same_session_endpoints_even_when_only_one_owns_source_lane",
            || {
                use crate::control::cap::atomic_codecs::TopologyHandle;

                with_cluster_fixture_pair(|clock, src_cfg, dst_cfg| {
                    with_test_cluster(clock, |cluster| {
                        let src_id = cluster
                            .add_rendezvous_from_config(src_cfg, DummyTransport)
                            .expect("register src");
                        let dst_id = cluster
                            .add_rendezvous_from_config(dst_cfg, DummyTransport)
                            .expect("register dst");

                        let sid = SessionId::new(26);
                        let (controller_handle, controller_lane) =
                            attach_session_lane(cluster, src_id, sid);
                        let worker_program = lane1_worker_program();
                        let (worker_handle, _worker_lane) = attach_session_lane_for_program::<1>(
                            cluster,
                            src_id,
                            sid,
                            &worker_program,
                        );
                        let src_lane = Lane::new(1);
                        let dst_lane = Lane::new(2);

                        assert_eq!(
                            controller_lane,
                            Lane::new(0),
                            "controller endpoint should remain on the canonical control lane",
                        );

                        let operands = TopologyOperands::new(
                            src_id,
                            dst_id,
                            src_lane,
                            dst_lane,
                            Generation::new(0),
                            Generation::new(1),
                            0,
                            0,
                        );

                        cluster
                            .run_effect_step(src_id, CpCommand::topology_begin(sid, operands))
                            .expect("begin effect for non-control source lane");
                        cluster
                            .dispatch_topology_ack_with_handle(
                                dst_id,
                                sid,
                                dst_lane,
                                TopologyHandle {
                                    src_rv: src_id.raw(),
                                    dst_rv: dst_id.raw(),
                                    src_lane: src_lane.raw() as u16,
                                    dst_lane: dst_lane.raw() as u16,
                                    old_gen: operands.old_gen.raw(),
                                    new_gen: operands.new_gen.raw(),
                                    seq_tx: operands.seq_tx,
                                    seq_rx: operands.seq_rx,
                                },
                                None,
                            )
                            .expect("ack succeeds");

                        assert!(
                            unsafe {
                                cluster
                                    .public_endpoint_ptr::<0, MintConfig>(
                                        src_id,
                                        controller_handle.0,
                                        controller_handle.1,
                                    )
                                    .is_some()
                            },
                            "controller endpoint must be live before commit",
                        );
                        assert!(
                            unsafe {
                                cluster
                                    .public_endpoint_ptr::<1, MintConfig>(
                                        src_id,
                                        worker_handle.0,
                                        worker_handle.1,
                                    )
                                    .is_some()
                            },
                            "worker endpoint must be live before commit",
                        );

                        cluster
                            .run_effect_step(src_id, CpCommand::topology_commit(sid, operands))
                            .expect("commit succeeds");

                        assert!(
                            unsafe {
                                cluster
                                    .public_endpoint_ptr::<0, MintConfig>(
                                        src_id,
                                        controller_handle.0,
                                        controller_handle.1,
                                    )
                                    .is_none()
                            },
                            "commit must revoke controller endpoints that share the migrated session even without owning the source lane",
                        );
                        assert!(
                            unsafe {
                                cluster
                                    .public_endpoint_ptr::<1, MintConfig>(
                                        src_id,
                                        worker_handle.0,
                                        worker_handle.1,
                                    )
                                    .is_none()
                            },
                            "commit must also revoke the endpoint that owns the migrated source lane",
                        );
                        assert_eq!(
                            cluster
                                .get_local(&src_id)
                                .expect("source rendezvous")
                                .session_lane(sid),
                            None,
                            "session-wide revoke must retire the migrated session after commit",
                        );
                    });
                });
            },
        );
    }

    #[test]
    fn topology_commit_retires_hidden_source_lane_even_when_same_session_public_endpoint_exists() {
        run_on_transient_compiled_test_stack(
            "topology_commit_retires_hidden_source_lane_even_when_same_session_public_endpoint_exists",
            || {
                use crate::control::cap::atomic_codecs::TopologyHandle;

                with_cluster_fixture_pair(|clock, src_cfg, dst_cfg| {
                    with_test_cluster(clock, |cluster| {
                        let src_id = cluster
                            .add_rendezvous_from_config(src_cfg, DummyTransport)
                            .expect("register src");
                        let dst_id = cluster
                            .add_rendezvous_from_config(dst_cfg, DummyTransport)
                            .expect("register dst");

                        let sid = SessionId::new(27);
                        let (controller_handle, controller_lane) =
                            attach_session_lane(cluster, src_id, sid);
                        let src_lane = Lane::new(2);
                        let dst_lane = Lane::new(3);

                        assert_eq!(
                            controller_lane,
                            Lane::new(0),
                            "public controller endpoint should stay on the canonical control lane",
                        );

                        cluster.with_control_mut(|core| {
                            let rv = core.locals.get_mut(&src_id).expect("source rendezvous");
                            rv.activate_lane_for_test(sid, src_lane)
                                .expect("test fixture must create a non-public source lane");
                        });

                        let operands = TopologyOperands::new(
                            src_id,
                            dst_id,
                            src_lane,
                            dst_lane,
                            Generation::new(0),
                            Generation::new(1),
                            0,
                            0,
                        );

                        cluster
                            .run_effect_step(src_id, CpCommand::topology_begin(sid, operands))
                            .expect("begin effect for non-public source lane");
                        cluster
                            .dispatch_topology_ack_with_handle(
                                dst_id,
                                sid,
                                dst_lane,
                                TopologyHandle {
                                    src_rv: src_id.raw(),
                                    dst_rv: dst_id.raw(),
                                    src_lane: src_lane.raw() as u16,
                                    dst_lane: dst_lane.raw() as u16,
                                    old_gen: operands.old_gen.raw(),
                                    new_gen: operands.new_gen.raw(),
                                    seq_tx: operands.seq_tx,
                                    seq_rx: operands.seq_rx,
                                },
                                None,
                            )
                            .expect("ack succeeds");

                        assert!(
                            unsafe {
                                cluster
                                    .public_endpoint_ptr::<0, MintConfig>(
                                        src_id,
                                        controller_handle.0,
                                        controller_handle.1,
                                    )
                                    .is_some()
                            },
                            "controller endpoint must be live before commit",
                        );

                        cluster
                            .run_effect_step(src_id, CpCommand::topology_commit(sid, operands))
                            .expect("commit succeeds");

                        assert!(
                            unsafe {
                                cluster
                                    .public_endpoint_ptr::<0, MintConfig>(
                                        src_id,
                                        controller_handle.0,
                                        controller_handle.1,
                                    )
                                    .is_none()
                            },
                            "commit must still revoke same-session public endpoints before retiring the hidden source lane",
                        );
                        assert_eq!(
                            cluster
                                .get_local(&src_id)
                                .expect("source rendezvous")
                                .session_lane(sid),
                            None,
                            "commit must explicitly retire the migrated source lane even when revoke found only unrelated public endpoints",
                        );
                    });
                });
            },
        );
    }

    #[test]
    fn topology_commit_retires_extra_hidden_same_session_lanes() {
        run_on_transient_compiled_test_stack(
            "topology_commit_retires_extra_hidden_same_session_lanes",
            || {
                use crate::control::cap::atomic_codecs::TopologyHandle;

                with_cluster_fixture_pair(|clock, src_cfg, dst_cfg| {
                    with_test_cluster(clock, |cluster| {
                        let src_id = cluster
                            .add_rendezvous_from_config(src_cfg, DummyTransport)
                            .expect("register src");
                        let dst_id = cluster
                            .add_rendezvous_from_config(dst_cfg, DummyTransport)
                            .expect("register dst");

                        let sid = SessionId::new(28);
                        let (controller_handle, controller_lane) =
                            attach_session_lane(cluster, src_id, sid);
                        let extra_lane = Lane::new(1);
                        let src_lane = Lane::new(2);
                        let dst_lane = Lane::new(3);

                        assert_eq!(
                            controller_lane,
                            Lane::new(0),
                            "public controller endpoint should stay on the canonical control lane",
                        );

                        cluster.with_control_mut(|core| {
                            let rv = core.locals.get_mut(&src_id).expect("source rendezvous");
                            rv.activate_lane_for_test(sid, extra_lane)
                                .expect("fixture must create an extra hidden same-session lane");
                            rv.activate_lane_for_test(sid, src_lane)
                                .expect("fixture must create the hidden source lane");
                        });

                        let operands = TopologyOperands::new(
                            src_id,
                            dst_id,
                            src_lane,
                            dst_lane,
                            Generation::new(0),
                            Generation::new(1),
                            0,
                            0,
                        );

                        cluster
                            .run_effect_step(src_id, CpCommand::topology_begin(sid, operands))
                            .expect("begin effect for hidden source lane");
                        cluster
                            .dispatch_topology_ack_with_handle(
                                dst_id,
                                sid,
                                dst_lane,
                                TopologyHandle {
                                    src_rv: src_id.raw(),
                                    dst_rv: dst_id.raw(),
                                    src_lane: src_lane.raw() as u16,
                                    dst_lane: dst_lane.raw() as u16,
                                    old_gen: operands.old_gen.raw(),
                                    new_gen: operands.new_gen.raw(),
                                    seq_tx: operands.seq_tx,
                                    seq_rx: operands.seq_rx,
                                },
                                None,
                            )
                            .expect("ack succeeds");

                        assert!(
                            unsafe {
                                cluster
                                    .public_endpoint_ptr::<0, MintConfig>(
                                        src_id,
                                        controller_handle.0,
                                        controller_handle.1,
                                    )
                                    .is_some()
                            },
                            "controller endpoint must be live before commit",
                        );
                        assert_eq!(
                            cluster
                                .get_local(&src_id)
                                .expect("source rendezvous")
                                .session_lane(sid),
                            Some(controller_lane),
                            "same-session lookup should still see a live source-side lane before commit",
                        );

                        cluster
                            .run_effect_step(src_id, CpCommand::topology_commit(sid, operands))
                            .expect("commit succeeds");

                        assert!(
                            unsafe {
                                cluster
                                    .public_endpoint_ptr::<0, MintConfig>(
                                        src_id,
                                        controller_handle.0,
                                        controller_handle.1,
                                    )
                                    .is_none()
                            },
                            "commit must revoke the public controller endpoint before session teardown",
                        );
                        assert_eq!(
                            cluster
                                .get_local(&src_id)
                                .expect("source rendezvous")
                                .session_lane(sid),
                            None,
                            "commit must retire every same-session source lane, not just the migrated source lane",
                        );
                    });
                });
            },
        );
    }

    #[test]
    fn topology_commit_revokes_all_public_endpoints_for_the_source_session() {
        run_on_transient_compiled_test_stack(
            "topology_commit_revokes_all_public_endpoints_for_the_source_session",
            || {
                use crate::control::cap::atomic_codecs::TopologyHandle;

                with_cluster_fixture_pair(|clock, src_cfg, dst_cfg| {
                    with_test_cluster(clock, |cluster| {
                        let src_id = cluster
                            .add_rendezvous_from_config(src_cfg, DummyTransport)
                            .expect("register src");
                        let dst_id = cluster
                            .add_rendezvous_from_config(dst_cfg, DummyTransport)
                            .expect("register dst");

                        let sid = SessionId::new(25);
                        let worker_program = linear_program::worker_program();
                        let (controller_handle, controller_lane) =
                            attach_session_lane(cluster, src_id, sid);
                        let (worker_handle, worker_lane) = attach_session_lane_for_program::<1>(
                            cluster,
                            src_id,
                            sid,
                            &worker_program,
                        );
                        assert_eq!(
                            controller_lane, worker_lane,
                            "same-session public endpoints must share the source lane authority"
                        );

                        let dst_lane = Lane::new(1);
                        let operands = TopologyOperands::new(
                            src_id,
                            dst_id,
                            controller_lane,
                            dst_lane,
                            Generation::new(0),
                            Generation::new(1),
                            0,
                            0,
                        );

                        cluster
                            .run_effect_step(src_id, CpCommand::topology_begin(sid, operands))
                            .expect("begin effect");
                        cluster
                            .dispatch_topology_ack_with_handle(
                                dst_id,
                                sid,
                                dst_lane,
                                TopologyHandle {
                                    src_rv: src_id.raw(),
                                    dst_rv: dst_id.raw(),
                                    src_lane: controller_lane.raw() as u16,
                                    dst_lane: dst_lane.raw() as u16,
                                    old_gen: operands.old_gen.raw(),
                                    new_gen: operands.new_gen.raw(),
                                    seq_tx: operands.seq_tx,
                                    seq_rx: operands.seq_rx,
                                },
                                None,
                            )
                            .expect("ack succeeds");

                        assert!(
                            unsafe {
                                cluster
                                    .public_endpoint_ptr::<0, MintConfig>(
                                        src_id,
                                        controller_handle.0,
                                        controller_handle.1,
                                    )
                                    .is_some()
                            },
                            "controller endpoint must be live before commit",
                        );
                        assert!(
                            unsafe {
                                cluster
                                    .public_endpoint_ptr::<1, MintConfig>(
                                        src_id,
                                        worker_handle.0,
                                        worker_handle.1,
                                    )
                                    .is_some()
                            },
                            "worker endpoint must be live before commit",
                        );

                        cluster
                            .run_effect_step(src_id, CpCommand::topology_commit(sid, operands))
                            .expect("commit succeeds");

                        assert!(
                            unsafe {
                                cluster
                                    .public_endpoint_ptr::<0, MintConfig>(
                                        src_id,
                                        controller_handle.0,
                                        controller_handle.1,
                                    )
                                    .is_none()
                            },
                            "commit must revoke the controller endpoint",
                        );
                        assert!(
                            unsafe {
                                cluster
                                    .public_endpoint_ptr::<1, MintConfig>(
                                        src_id,
                                        worker_handle.0,
                                        worker_handle.1,
                                    )
                                    .is_none()
                            },
                            "commit must revoke every public endpoint sharing the source lane",
                        );
                        assert_eq!(
                            cluster
                                .get_local(&src_id)
                                .expect("source rendezvous")
                                .session_lane(sid),
                            None,
                            "commit must retire the source lane after all public leases are revoked",
                        );
                    });
                });
            },
        );
    }

    #[test]
    fn topology_commit_descriptor_rejects_fixed_header_lane_mismatch_before_mutation() {
        run_on_transient_compiled_test_stack(
            "topology_commit_descriptor_rejects_fixed_header_lane_mismatch_before_mutation",
            || {
                use crate::control::cap::atomic_codecs::TopologyHandle;

                fn topology_commit_token(
                    desc: ControlDesc,
                    sid: SessionId,
                    lane: Lane,
                    handle: TopologyHandle,
                ) -> [u8; CAP_TOKEN_LEN] {
                    let mut header = [0u8; CAP_HEADER_LEN];
                    CapHeader::new(
                        sid,
                        lane,
                        0,
                        desc.resource_tag(),
                        desc.label(),
                        desc.op(),
                        desc.path(),
                        desc.shot(),
                        desc.scope_kind(),
                        desc.header_flags(),
                        0,
                        0,
                        handle.encode(),
                    )
                    .encode(&mut header);
                    GenericCapToken::<()>::from_parts([0; CAP_NONCE_LEN], header, [0; CAP_TAG_LEN])
                        .into_bytes()
                }

                with_cluster_fixture_pair(|clock, src_cfg, dst_cfg| {
                    with_test_cluster(clock, |cluster| {
                        let src_id = cluster
                            .add_rendezvous_from_config(src_cfg, DummyTransport)
                            .expect("register src");
                        let dst_id = cluster
                            .add_rendezvous_from_config(dst_cfg, DummyTransport)
                            .expect("register dst");

                        let sid = SessionId::new(20);
                        let (src_handle, src_lane) = attach_session_lane(cluster, src_id, sid);
                        let dst_lane = Lane::new(1);
                        let wrong_header_lane = Lane::new(2);
                        let operands = TopologyOperands::new(
                            src_id,
                            dst_id,
                            src_lane,
                            dst_lane,
                            Generation::new(0),
                            Generation::new(1),
                            0,
                            0,
                        );

                        cluster
                            .run_effect_step(src_id, CpCommand::topology_begin(sid, operands))
                            .expect("begin effect");
                        cluster
                            .dispatch_topology_ack_with_handle(
                                dst_id,
                                sid,
                                dst_lane,
                                TopologyHandle {
                                    src_rv: src_id.raw(),
                                    dst_rv: dst_id.raw(),
                                    src_lane: src_lane.raw() as u16,
                                    dst_lane: dst_lane.raw() as u16,
                                    old_gen: operands.old_gen.raw(),
                                    new_gen: operands.new_gen.raw(),
                                    seq_tx: operands.seq_tx,
                                    seq_rx: operands.seq_rx,
                                },
                                None,
                            )
                            .expect("ack succeeds");

                        let desc = ControlDesc::new(
                            EffIndex::MAX,
                            ControlDesc::STATIC_POLICY_SITE,
                            0x04A7,
                            0x7A,
                            TAG_TOPOLOGY_BEGIN_CONTROL,
                            ControlOp::TopologyCommit,
                            crate::global::const_dsl::ControlScopeKind::Topology,
                            ControlPath::Wire,
                            CapShot::One,
                            true,
                        );
                        let bytes = topology_commit_token(
                            desc,
                            sid,
                            wrong_header_lane,
                            TopologyHandle {
                                src_rv: src_id.raw(),
                                dst_rv: dst_id.raw(),
                                src_lane: src_lane.raw() as u16,
                                dst_lane: dst_lane.raw() as u16,
                                old_gen: operands.old_gen.raw(),
                                new_gen: operands.new_gen.raw(),
                                seq_tx: operands.seq_tx,
                                seq_rx: operands.seq_rx,
                            },
                        );

                        let err = match cluster
                            .dispatch_descriptor_control_frame(src_id, bytes, desc, 0, 0, None)
                        {
                            Ok(_) => {
                                panic!("fixed-header lane mismatch must fail before commit")
                            }
                            Err(err) => err,
                        };
                        assert_eq!(
                            err,
                            CpError::Authorisation {
                                operation: ControlOp::TopologyCommit as u8,
                            }
                        );
                        assert_eq!(
                            cluster
                                .get_local(&src_id)
                                .expect("source rendezvous")
                                .expected_topology_ack(sid),
                            Ok(operands.ack(sid)),
                            "descriptor rejection must not clear source-side expected ACK",
                        );
                        assert_eq!(
                            cluster.distributed_topology_operands(sid),
                            Some(operands),
                            "rejected commit must leave distributed topology bookkeeping intact",
                        );
                        assert_eq!(
                            cluster
                                .get_local(&src_id)
                                .expect("source rendezvous")
                                .expected_topology_ack(sid),
                            Ok(operands.ack(sid)),
                            "rejected token must preserve source-side expected ACK",
                        );

                        let pending = cluster
                            .run_effect_step(src_id, CpCommand::topology_commit(sid, operands))
                            .expect("correct commit must still succeed after rejected token");
                        assert!(matches!(pending, PendingEffect::None));

                        unsafe {
                            drop_test_public_endpoint(cluster, src_id, src_handle);
                        }
                    });
                });
            },
        );
    }

    #[test]
    fn prepare_topology_operands_from_descriptor_decodes_typed_handle() {
        run_on_transient_compiled_test_stack(
            "prepare_topology_operands_from_descriptor_decodes_typed_handle",
            || {
                with_cluster_fixture_pair(|clock, src_cfg, dst_cfg| {
                    with_test_cluster(clock, |cluster| {
                        let src_id = cluster
                            .add_rendezvous_from_config(src_cfg, DummyTransport)
                            .expect("register src");
                        let dst_id = cluster
                            .add_rendezvous_from_config(dst_cfg, DummyTransport)
                            .expect("register dst");

                        let expected = TopologyOperands::new(
                            src_id,
                            dst_id,
                            Lane::new(3),
                            Lane::new(7),
                            Generation::new(11),
                            Generation::new(12),
                            13,
                            14,
                        );
                        let descriptor =
                            TopologyDescriptor::decode(topology_handle(expected).encode())
                                .expect("typed topology handle must decode");
                        let operands = cluster
                            .prepare_topology_operands_from_descriptor(
                                src_id,
                                Lane::new(3),
                                ControlDesc::new(
                                    EffIndex::MAX,
                                    ControlDesc::STATIC_POLICY_SITE,
                                    0x04A8,
                                    0x7B,
                                    TAG_TOPOLOGY_BEGIN_CONTROL,
                                    ControlOp::TopologyBegin,
                                    crate::global::const_dsl::ControlScopeKind::Topology,
                                    ControlPath::Wire,
                                    CapShot::One,
                                    true,
                                ),
                                descriptor,
                            )
                            .expect("typed topology descriptor must decode topology operands");

                        assert_eq!(operands, expected);
                    });
                });
            },
        );
    }

    #[test]
    fn prepare_topology_operands_from_descriptor_rejects_same_rendezvous() {
        run_on_transient_compiled_test_stack(
            "prepare_topology_operands_from_descriptor_rejects_same_rendezvous",
            || {
                with_cluster_fixture_pair(|clock, src_cfg, _dst_cfg| {
                    with_test_cluster(clock, |cluster| {
                        let rv_id = cluster
                            .add_rendezvous_from_config(src_cfg, DummyTransport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(22);

                        let operands = TopologyOperands::new(
                            rv_id,
                            rv_id,
                            Lane::new(3),
                            Lane::new(7),
                            Generation::new(11),
                            Generation::new(12),
                            13,
                            14,
                        );
                        let descriptor =
                            TopologyDescriptor::decode(topology_handle(operands).encode())
                                .expect("typed topology handle must decode");
                        let err = cluster
                            .prepare_topology_operands_from_descriptor(
                                rv_id,
                                Lane::new(3),
                                ControlDesc::new(
                                    EffIndex::MAX,
                                    ControlDesc::STATIC_POLICY_SITE,
                                    0x04A8,
                                    0x7B,
                                    TAG_TOPOLOGY_BEGIN_CONTROL,
                                    ControlOp::TopologyBegin,
                                    crate::global::const_dsl::ControlScopeKind::Topology,
                                    ControlPath::Wire,
                                    CapShot::One,
                                    true,
                                ),
                                descriptor,
                            )
                            .expect_err("typed topology descriptor must reject same-rendezvous");

                        assert_eq!(
                            err,
                            CpError::Authorisation {
                                operation: ControlOp::TopologyBegin as u8,
                            }
                        );
                        cluster.with_control_mut(|core| {
                            assert!(
                                core.topology_state.get(sid).is_none(),
                                "same-rendezvous topology descriptor must not create distributed topology state",
                            );
                        });
                    });
                });
            },
        );
    }

    #[test]
    fn validate_topology_operands_from_descriptor_rejects_ack_mismatch() {
        run_on_transient_compiled_test_stack(
            "validate_topology_operands_from_descriptor_rejects_ack_mismatch",
            || {
                with_cluster_fixture_pair(|clock, src_cfg, dst_cfg| {
                    with_test_cluster(clock, |cluster| {
                        let src_id = cluster
                            .add_rendezvous_from_config(src_cfg, DummyTransport)
                            .expect("register src");
                        let dst_id = cluster
                            .add_rendezvous_from_config(dst_cfg, DummyTransport)
                            .expect("register dst");

                        let operands = TopologyOperands::new(
                            src_id,
                            dst_id,
                            Lane::new(3),
                            Lane::new(7),
                            Generation::new(11),
                            Generation::new(12),
                            13,
                            14,
                        );

                        let mismatched = TopologyOperands::new(
                            src_id,
                            dst_id,
                            Lane::new(3),
                            Lane::new(7),
                            Generation::new(11),
                            Generation::new(13),
                            13,
                            14,
                        );
                        let descriptor =
                            TopologyDescriptor::decode(topology_handle(mismatched).encode())
                                .expect("typed topology handle must decode");
                        let err = cluster
                            .validate_topology_operands_from_descriptor(
                                dst_id,
                                Lane::new(7),
                                ControlDesc::new(
                                    EffIndex::MAX,
                                    ControlDesc::STATIC_POLICY_SITE,
                                    0x04A9,
                                    0x7C,
                                    0x7C,
                                    ControlOp::TopologyAck,
                                    crate::global::const_dsl::ControlScopeKind::Topology,
                                    ControlPath::Wire,
                                    CapShot::One,
                                    true,
                                ),
                                descriptor,
                                operands,
                            )
                            .expect_err("typed ack descriptor validation must reject mismatch");

                        assert_eq!(
                            err,
                            CpError::Authorisation {
                                operation: ControlOp::TopologyAck as u8,
                            }
                        );
                    });
                });
            },
        );
    }

    #[test]
    fn prepare_reroute_handle_from_policy_decodes_static_route_input() {
        run_on_transient_compiled_test_stack(
            "prepare_reroute_handle_from_policy_decodes_static_route_input",
            || {
                with_cluster_fixture_pair(|clock, src_cfg, dst_cfg| {
                    with_test_cluster(clock, |cluster| {
                        let src_id = cluster
                            .add_rendezvous_from_config(src_cfg, DummyTransport)
                            .expect("register src");
                        let dst_id = cluster
                            .add_rendezvous_from_config(dst_cfg, DummyTransport)
                            .expect("register dst");

                        let handle = cluster
                            .prepare_reroute_handle_from_policy(
                                src_id,
                                Lane::new(4),
                                EffIndex::new(10),
                                TAG_CAP_DELEGATE_CONTROL,
                                ControlOp::CapDelegate,
                                PolicyMode::Static,
                                [pack_u16_pair(dst_id.raw(), 8), 21, 22, 23],
                                &crate::transport::context::PolicyAttrs::EMPTY,
                            )
                            .expect("static route input must decode delegation handle");

                        assert_eq!(
                            handle,
                            DelegationHandle {
                                src_rv: src_id.raw(),
                                dst_rv: dst_id.raw(),
                                src_lane: 4,
                                dst_lane: 8,
                                seq_tx: 21,
                                seq_rx: 22,
                                shard: 23,
                                flags: 0,
                            }
                        );
                    });
                });
            },
        );
    }

    #[test]
    fn canonicalize_delegate_reads_validated_endpoint_header_fields() {
        let handle = crate::control::cap::mint::EndpointHandle::new(
            SessionId::new(0x0102_0304),
            Lane::new(1),
            9,
        );
        let mut header = [0u8; crate::control::cap::mint::CAP_HEADER_LEN];
        crate::control::cap::mint::CapHeader::new(
            handle.sid,
            handle.lane,
            handle.role,
            crate::control::cap::mint::EndpointResource::TAG,
            0,
            ControlOp::Fence,
            ControlPath::Local,
            crate::control::cap::mint::CapShot::One,
            crate::global::const_dsl::ControlScopeKind::None,
            0,
            0,
            0,
            crate::control::cap::mint::EndpointResource::encode_handle(&handle),
        )
        .encode(&mut header);

        let token = GenericCapToken::<crate::control::cap::mint::EndpointResource>::from_parts(
            [0xAB; crate::control::cap::mint::CAP_NONCE_LEN],
            header,
            [0; crate::control::cap::mint::CAP_TAG_LEN],
        );
        let command = CpCommand::new(ControlOp::CapDelegate).with_delegate(DelegateOperands {
            claim: false,
            token,
        });

        let canonical = command
            .canonicalize_delegate()
            .expect("valid endpoint header must canonicalize");
        assert_eq!(canonical.sid, Some(handle.sid));
        assert_eq!(canonical.lane, Some(handle.lane));
    }

    #[test]
    fn canonicalize_delegate_rejects_noncanonical_endpoint_headers() {
        fn endpoint_delegate_with_mutated_header(
            mutate: fn(&mut [u8; crate::control::cap::mint::CAP_HEADER_LEN]),
        ) -> CpCommand {
            let handle =
                crate::control::cap::mint::EndpointHandle::new(SessionId::new(7), Lane::new(1), 9);
            let mut header = [0u8; crate::control::cap::mint::CAP_HEADER_LEN];
            crate::control::cap::mint::CapHeader::new(
                handle.sid,
                handle.lane,
                handle.role,
                crate::control::cap::mint::EndpointResource::TAG,
                0,
                ControlOp::Fence,
                ControlPath::Local,
                crate::control::cap::mint::CapShot::One,
                crate::global::const_dsl::ControlScopeKind::None,
                0,
                0,
                0,
                crate::control::cap::mint::EndpointResource::encode_handle(&handle),
            )
            .encode(&mut header);
            mutate(&mut header);

            let token = GenericCapToken::<crate::control::cap::mint::EndpointResource>::from_parts(
                [0xAB; crate::control::cap::mint::CAP_NONCE_LEN],
                header,
                [0; crate::control::cap::mint::CAP_TAG_LEN],
            );

            CpCommand::new(ControlOp::CapDelegate).with_delegate(DelegateOperands {
                claim: false,
                token,
            })
        }

        fn mutate_tag(header: &mut [u8; crate::control::cap::mint::CAP_HEADER_LEN]) {
            header[7] = RouteDecisionKind::TAG;
        }

        fn mutate_label(header: &mut [u8; crate::control::cap::mint::CAP_HEADER_LEN]) {
            header[8] = 1;
        }

        fn mutate_op(header: &mut [u8; crate::control::cap::mint::CAP_HEADER_LEN]) {
            header[9] = ControlOp::TopologyBegin.as_u8();
        }

        fn mutate_path(header: &mut [u8; crate::control::cap::mint::CAP_HEADER_LEN]) {
            header[10] = ControlPath::Wire.as_u8();
        }

        fn mutate_shot(header: &mut [u8; crate::control::cap::mint::CAP_HEADER_LEN]) {
            header[11] = crate::control::cap::mint::CapShot::Many.as_u8();
        }

        fn mutate_scope_kind(header: &mut [u8; crate::control::cap::mint::CAP_HEADER_LEN]) {
            header[12] = crate::global::const_dsl::ControlScopeKind::Route as u8;
        }

        fn mutate_flags(header: &mut [u8; crate::control::cap::mint::CAP_HEADER_LEN]) {
            header[13] = 0x01;
        }

        fn mutate_scope_id(header: &mut [u8; crate::control::cap::mint::CAP_HEADER_LEN]) {
            header[14..16].copy_from_slice(&1u16.to_be_bytes());
        }

        fn mutate_epoch(header: &mut [u8; crate::control::cap::mint::CAP_HEADER_LEN]) {
            header[16..18].copy_from_slice(&1u16.to_be_bytes());
        }

        let cases: &[(
            &str,
            fn(&mut [u8; crate::control::cap::mint::CAP_HEADER_LEN]),
        )] = &[
            ("tag", mutate_tag),
            ("label", mutate_label),
            ("op", mutate_op),
            ("path", mutate_path),
            ("shot", mutate_shot),
            ("scope_kind", mutate_scope_kind),
            ("flags", mutate_flags),
            ("scope_id", mutate_scope_id),
            ("epoch", mutate_epoch),
        ];

        for (name, mutate) in cases {
            let err = endpoint_delegate_with_mutated_header(*mutate)
                .canonicalize_delegate()
                .expect_err("malformed endpoint header must be rejected");
            assert!(
                matches!(err, CpError::Delegation(DelegationError::InvalidToken)),
                "{name} mutation must be rejected as invalid delegate token, got {err:?}",
            );
        }
    }

    #[test]
    fn canonicalize_delegate_rejects_malformed_endpoint_handle_payloads() {
        fn endpoint_delegate_with_mutated_handle(
            mutate: fn(&mut [u8; crate::control::cap::mint::CAP_HANDLE_LEN]),
        ) -> CpCommand {
            let handle =
                crate::control::cap::mint::EndpointHandle::new(SessionId::new(7), Lane::new(1), 9);
            let mut header = [0u8; crate::control::cap::mint::CAP_HEADER_LEN];
            crate::control::cap::mint::CapHeader::new(
                handle.sid,
                handle.lane,
                handle.role,
                crate::control::cap::mint::EndpointResource::TAG,
                0,
                ControlOp::Fence,
                ControlPath::Local,
                crate::control::cap::mint::CapShot::One,
                crate::global::const_dsl::ControlScopeKind::None,
                0,
                0,
                0,
                crate::control::cap::mint::EndpointResource::encode_handle(&handle),
            )
            .encode(&mut header);

            let handle_bytes = &mut header[crate::control::cap::mint::CAP_CONTROL_HEADER_FIXED_LEN
                ..crate::control::cap::mint::CAP_CONTROL_HEADER_FIXED_LEN
                    + crate::control::cap::mint::CAP_HANDLE_LEN];
            let handle_bytes: &mut [u8; crate::control::cap::mint::CAP_HANDLE_LEN] = handle_bytes
                .try_into()
                .expect("endpoint handle payload must fit");
            mutate(handle_bytes);

            let token = GenericCapToken::<crate::control::cap::mint::EndpointResource>::from_parts(
                [0xAB; crate::control::cap::mint::CAP_NONCE_LEN],
                header,
                [0; crate::control::cap::mint::CAP_TAG_LEN],
            );

            CpCommand::new(ControlOp::CapDelegate).with_delegate(DelegateOperands {
                claim: false,
                token,
            })
        }

        fn mutate_sid(handle: &mut [u8; crate::control::cap::mint::CAP_HANDLE_LEN]) {
            handle[0] ^= 0x01;
        }

        fn mutate_lane(handle: &mut [u8; crate::control::cap::mint::CAP_HANDLE_LEN]) {
            handle[4] ^= 0x01;
        }

        fn mutate_role(handle: &mut [u8; crate::control::cap::mint::CAP_HANDLE_LEN]) {
            handle[5] ^= 0x01;
        }

        fn mutate_trailing_padding(handle: &mut [u8; crate::control::cap::mint::CAP_HANDLE_LEN]) {
            handle[6] = 0x7F;
        }

        let cases: &[(
            &str,
            fn(&mut [u8; crate::control::cap::mint::CAP_HANDLE_LEN]),
        )] = &[
            ("sid", mutate_sid),
            ("lane", mutate_lane),
            ("role", mutate_role),
            ("trailing_padding", mutate_trailing_padding),
        ];

        for (name, mutate) in cases {
            let err = endpoint_delegate_with_mutated_handle(*mutate)
                .canonicalize_delegate()
                .expect_err("malformed endpoint handle payload must be rejected");
            assert!(
                matches!(err, CpError::Delegation(DelegationError::InvalidToken)),
                "{name} mutation must be rejected as invalid delegate token, got {err:?}",
            );
        }
    }

    #[test]
    fn cached_topology_operands_shard_by_source_rv() {
        run_on_transient_compiled_test_stack("cached_topology_operands_shard_by_source_rv", || {
            with_cluster_fixture_pair(|clock, src_cfg, dst_cfg| {
                with_test_cluster(clock, |cluster| {
                    let src_id = cluster
                        .add_rendezvous_from_config(src_cfg, DummyTransport)
                        .expect("register src");
                    let dst_id = cluster
                        .add_rendezvous_from_config(dst_cfg, DummyTransport)
                        .expect("register dst");

                    let sid0 = SessionId::new(7);
                    let sid1 = SessionId::new(9);
                    let ops0 = TopologyOperands::new(
                        src_id,
                        dst_id,
                        Lane::new(0),
                        Lane::new(1),
                        Generation::new(0),
                        Generation::new(1),
                        0,
                        0,
                    );
                    let ops1 = TopologyOperands::new(
                        dst_id,
                        src_id,
                        Lane::new(1),
                        Lane::new(0),
                        Generation::new(2),
                        Generation::new(3),
                        1,
                        1,
                    );

                    cluster
                        .cache_topology_operands(sid0, ops0)
                        .expect("cache first shard");
                    cluster
                        .cache_topology_operands(sid1, ops1)
                        .expect("cache second shard");

                    assert_eq!(cluster.distributed_topology_operands(sid0), Some(ops0));
                    assert_eq!(cluster.distributed_topology_operands(sid1), Some(ops1));
                    assert_eq!(cluster.take_cached_topology_operands(sid0), Some(ops0));
                    assert_eq!(cluster.take_cached_topology_operands(sid1), Some(ops1));
                    assert!(cluster.distributed_topology_operands(sid0).is_none());
                    assert!(cluster.distributed_topology_operands(sid1).is_none());
                });
            });
        });
    }

    fn test_distributed_topology_entry(seq_tx: u32) -> DistributedEntry {
        DistributedEntry {
            operands: TopologyOperands::new(
                RendezvousId::new(1),
                RendezvousId::new(2),
                Lane::new(3),
                Lane::new(4),
                Generation::new(5),
                Generation::new(6),
                seq_tx,
                8,
            ),
            phase: DistributedPhase::Begin { txn: None },
        }
    }

    #[test]
    fn distributed_topology_bucket_accesses_untagged_entries() {
        let capacity = 2usize;
        let layout = std::alloc::Layout::from_size_align(
            DistributedTopologyBucket::storage_bytes(capacity),
            DistributedTopologyBucket::storage_align(),
        )
        .expect("bucket storage layout");
        let storage = unsafe { std::alloc::alloc(layout) };
        if storage.is_null() {
            std::alloc::handle_alloc_error(layout);
        }

        let mut bucket = DistributedTopologyBucket::empty();
        let reclaim_delta = 1usize;
        assert!(
            DistributedTopologyBucket::STORAGE_TAG_MASK >= reclaim_delta,
            "test requires a non-zero reclaim tag bit"
        );

        unsafe {
            bucket.bind_from_storage(storage, capacity, reclaim_delta);
        }

        let entries = bucket.entries_ptr();
        assert_ne!(bucket.raw_entries().addr(), entries.addr());

        let sid = SessionId::new(17);
        bucket
            .insert(sid, test_distributed_topology_entry(7))
            .expect("insert uses untagged storage");

        let stored = unsafe { (&*entries).as_ref() }.expect("entry stored at untagged base");
        assert_eq!(stored.sid, sid);
        assert_eq!(stored.entry.operands.seq_tx, 7);
        assert_eq!(bucket.occupied_len(), 1);
        assert!(bucket.contains_sid(sid));
        assert_eq!(bucket.get(sid).map(|entry| entry.operands.seq_tx), Some(7));

        let entry = bucket.get_mut(sid).expect("mutable entry");
        entry.operands.seq_tx = 9;
        assert_eq!(
            unsafe {
                (&*entries)
                    .as_ref()
                    .map(|stored| stored.entry.operands.seq_tx)
            },
            Some(9)
        );

        let removed = bucket.remove(sid).expect("remove entry");
        assert_eq!(removed.operands.seq_tx, 9);
        assert_eq!(bucket.occupied_len(), 0);
        assert!(!bucket.contains_sid(sid));
        assert!(bucket.get(sid).is_none());

        unsafe {
            std::alloc::dealloc(storage, layout);
        }
    }

    #[test]
    fn distributed_topology_state_binds_by_source_rv() {
        run_on_transient_compiled_test_stack(
            "distributed_topology_state_binds_by_source_rv",
            || {
                with_cluster_fixture_pair(|clock, src_cfg, dst_cfg| {
                    with_test_cluster(clock, |cluster| {
                        let src_id = cluster
                            .add_rendezvous_from_config(src_cfg, DummyTransport)
                            .expect("register src");
                        let dst_id = cluster
                            .add_rendezvous_from_config(dst_cfg, DummyTransport)
                            .expect("register dst");

                        let sid0 = SessionId::new(11);
                        let sid1 = SessionId::new(13);
                        let ops0 = TopologyOperands::new(
                            src_id,
                            dst_id,
                            Lane::new(0),
                            Lane::new(1),
                            Generation::new(0),
                            Generation::new(1),
                            0,
                            0,
                        );
                        let ops1 = TopologyOperands::new(
                            dst_id,
                            src_id,
                            Lane::new(1),
                            Lane::new(0),
                            Generation::new(2),
                            Generation::new(3),
                            1,
                            1,
                        );

                        cluster.with_control_mut(|core| {
                            assert!(
                                core.topology_state
                                    .bucket(src_id)
                                    .expect("src bucket")
                                    .storage_ptr()
                                    .is_null()
                            );
                            assert!(
                                core.topology_state
                                    .bucket(dst_id)
                                    .expect("dst bucket")
                                    .storage_ptr()
                                    .is_null()
                            );

                            core.ensure_distributed_topology_capacity(src_id, 1)
                                .expect("bind src bucket");
                            core.topology_state.begin(sid0, ops0).expect("begin src");
                            assert!(
                                !core
                                    .topology_state
                                    .bucket(src_id)
                                    .expect("src bucket bound")
                                    .storage_ptr()
                                    .is_null()
                            );
                            assert!(
                                core.topology_state
                                    .bucket(dst_id)
                                    .expect("dst bucket still unbound")
                                    .storage_ptr()
                                    .is_null()
                            );

                            core.ensure_distributed_topology_capacity(dst_id, 1)
                                .expect("bind dst bucket");
                            core.topology_state.begin(sid1, ops1).expect("begin dst");
                            assert!(
                                !core
                                    .topology_state
                                    .bucket(dst_id)
                                    .expect("dst bucket bound")
                                    .storage_ptr()
                                    .is_null()
                            );

                            let ack0 = core
                                .topology_state
                                .acknowledge(sid0, src_id)
                                .expect("ack src shard");
                            assert_eq!(ack0, ops0.ack(sid0));
                            assert_eq!(
                                core.topology_state
                                    .topology_commit(sid0, src_id, Some(ack0)),
                                Ok(ops0)
                            );
                            assert_eq!(core.topology_state.get(sid1).copied(), Some(ops1));
                        });

                        assert!(cluster.distributed_topology_operands(sid0).is_none());
                        assert_eq!(cluster.distributed_topology_operands(sid1), Some(ops1));
                    });
                });
            },
        );
    }

    #[test]
    fn distributed_topology_commit_mismatch_preserves_entry_for_retry() {
        run_on_transient_compiled_test_stack(
            "distributed_topology_commit_mismatch_preserves_entry_for_retry",
            || {
                with_cluster_fixture_pair(|clock, src_cfg, dst_cfg| {
                    with_test_cluster(clock, |cluster| {
                        let src_id = cluster
                            .add_rendezvous_from_config(src_cfg, DummyTransport)
                            .expect("register src");
                        let dst_id = cluster
                            .add_rendezvous_from_config(dst_cfg, DummyTransport)
                            .expect("register dst");

                        let sid = SessionId::new(29);
                        let ops = TopologyOperands::new(
                            src_id,
                            dst_id,
                            Lane::new(0),
                            Lane::new(1),
                            Generation::new(0),
                            Generation::new(1),
                            0,
                            0,
                        );

                        cluster.with_control_mut(|core| {
                            core.ensure_distributed_topology_capacity(src_id, 1)
                                .expect("bind src bucket");
                            let (_intent, ack) =
                                core.topology_state.begin(sid, ops).expect("begin topology");
                            assert_eq!(
                                core.topology_state.acknowledge(sid, src_id),
                                Ok(ack),
                                "begin entry must advance to acked phase before commit",
                            );

                            let mismatched_ack = TopologyAck::new(
                                ops.src_rv,
                                ops.dst_rv,
                                sid.raw(),
                                Generation::new(2),
                                ops.src_lane,
                                ops.dst_lane,
                                ops.seq_tx,
                                ops.seq_rx,
                            );
                            assert_eq!(
                                core.topology_state
                                    .topology_commit(sid, src_id, Some(mismatched_ack)),
                                Err(CpError::Topology(TopologyError::CommitFailed)),
                                "commit mismatch must fail closed without consuming the entry",
                            );
                            assert_eq!(
                                core.topology_state.get(sid).copied(),
                                Some(ops),
                                "failed commit must preserve the distributed topology owner for retry",
                            );
                            assert_eq!(
                                core.topology_state.topology_commit(sid, src_id, Some(ack)),
                                Ok(ops),
                                "correct commit must still succeed after the rejected attempt",
                            );
                        });
                    });
                });
            },
        );
    }

    #[test]
    fn cached_topology_operands_replace_same_session_across_rendezvous_shards() {
        run_on_transient_compiled_test_stack(
            "cached_topology_operands_replace_same_session_across_rendezvous_shards",
            || {
                with_cluster_fixture_pair(|clock, src_cfg, dst_cfg| {
                    with_test_cluster(clock, |cluster| {
                        let src_id = cluster
                            .add_rendezvous_from_config(src_cfg, DummyTransport)
                            .expect("register src");
                        let dst_id = cluster
                            .add_rendezvous_from_config(dst_cfg, DummyTransport)
                            .expect("register dst");

                        let sid = SessionId::new(23);
                        let ops0 = TopologyOperands::new(
                            src_id,
                            dst_id,
                            Lane::new(0),
                            Lane::new(1),
                            Generation::new(0),
                            Generation::new(1),
                            0,
                            0,
                        );
                        let ops1 = TopologyOperands::new(
                            dst_id,
                            src_id,
                            Lane::new(1),
                            Lane::new(0),
                            Generation::new(2),
                            Generation::new(3),
                            1,
                            1,
                        );

                        cluster
                            .cache_topology_operands(sid, ops0)
                            .expect("cache first shard");
                        assert_eq!(cluster.distributed_topology_operands(sid), Some(ops0));

                        cluster
                            .cache_topology_operands(sid, ops1)
                            .expect("replace cached operands on second shard");

                        assert_eq!(
                            cluster.distributed_topology_operands(sid),
                            Some(ops1),
                            "same-session cached topology operands must stay globally unique across rendezvous shards"
                        );
                        assert_eq!(cluster.take_cached_topology_operands(sid), Some(ops1));
                        assert!(cluster.distributed_topology_operands(sid).is_none());
                    });
                });
            },
        );
    }

    #[test]
    fn register_dynamic_resolver_rejects_topology_and_reroute_ops() {
        run_on_transient_compiled_test_stack(
            "register_dynamic_resolver_rejects_topology_and_reroute_ops",
            || {
                fn defer_resolution(
                    _ctx: ResolverContext,
                ) -> Result<DynamicResolution, ResolverError> {
                    Ok(DynamicResolution::Defer { retry_hint: 2 })
                }

                with_cluster_fixture(|clock, config| {
                    with_test_cluster(clock, |cluster| {
                        let rv_id = cluster
                            .add_rendezvous_from_config(config, DummyTransport)
                            .expect("register rendezvous");

                        let policy_id = 913u16;
                        let eff_index = EffIndex::new(7);
                        let policy = crate::global::const_dsl::PolicyMode::dynamic(policy_id);

                        cluster
                            .register_dynamic_policy_resolver(
                                rv_id,
                                eff_index,
                                TAG_TOPOLOGY_BEGIN_CONTROL,
                                policy,
                                TAG_TOPOLOGY_BEGIN_CONTROL,
                                ControlOp::TopologyBegin,
                                None,
                                ResolverRef::from_fn(defer_resolution),
                            )
                            .expect_err("topology resolver must be rejected");
                        cluster
                            .register_dynamic_policy_resolver(
                                rv_id,
                                eff_index,
                                TAG_CAP_DELEGATE_CONTROL,
                                policy,
                                TAG_CAP_DELEGATE_CONTROL,
                                ControlOp::CapDelegate,
                                None,
                                ResolverRef::from_fn(defer_resolution),
                            )
                            .expect_err("reroute resolver must be rejected");
                    });
                });
            },
        );
    }

    #[test]
    fn dynamic_resolver_rejects_cross_semantic_results() {
        run_on_transient_compiled_test_stack(
            "dynamic_resolver_rejects_cross_semantic_results",
            || {
                fn loop_resolution(
                    _ctx: ResolverContext,
                ) -> Result<DynamicResolution, ResolverError> {
                    Ok(DynamicResolution::Loop { decision: false })
                }

                fn route_resolution(
                    _ctx: ResolverContext,
                ) -> Result<DynamicResolution, ResolverError> {
                    Ok(DynamicResolution::RouteArm { arm: 0 })
                }

                fn non_binary_route_resolution(
                    _ctx: ResolverContext,
                ) -> Result<DynamicResolution, ResolverError> {
                    Ok(DynamicResolution::RouteArm { arm: 2 })
                }

                with_cluster_fixture(|clock, config| {
                    with_test_cluster(clock, |cluster| {
                        let rv_id = cluster
                            .add_rendezvous_from_config(config, DummyTransport)
                            .expect("register rendezvous");
                        let policy = crate::global::const_dsl::PolicyMode::dynamic(914)
                            .with_scope(ScopeId::route(1));
                        let eff_index = EffIndex::new(8);
                        let tag = crate::control::cap::resource_kinds::RouteDecisionKind::TAG;

                        cluster
                            .register_dynamic_policy_resolver(
                                rv_id,
                                eff_index,
                                tag,
                                policy,
                                tag,
                                ControlOp::RouteDecision,
                                None,
                                ResolverRef::from_fn(loop_resolution),
                            )
                            .expect("register route resolver");
                        assert!(
                            matches!(
                                cluster.resolve_dynamic_policy(
                                    rv_id,
                                    None,
                                    Lane::new(1),
                                    eff_index,
                                    tag,
                                    ControlOp::RouteDecision,
                                    [0; 4],
                                    &crate::transport::context::PolicyAttrs::EMPTY,
                                ),
                                Err(CpError::PolicyAbort { reason: 914 })
                            ),
                            "route decision must reject loop resolver vocabulary"
                        );

                        let loop_eff = EffIndex::new(9);
                        let loop_tag = crate::control::cap::resource_kinds::LoopContinueKind::TAG;
                        cluster
                            .register_dynamic_policy_resolver(
                                rv_id,
                                loop_eff,
                                loop_tag,
                                policy,
                                loop_tag,
                                ControlOp::LoopContinue,
                                None,
                                ResolverRef::from_fn(route_resolution),
                            )
                            .expect("register loop resolver");
                        assert!(
                            matches!(
                                cluster.resolve_dynamic_policy(
                                    rv_id,
                                    None,
                                    Lane::new(1),
                                    loop_eff,
                                    loop_tag,
                                    ControlOp::LoopContinue,
                                    [0; 4],
                                    &crate::transport::context::PolicyAttrs::EMPTY,
                                ),
                                Err(CpError::PolicyAbort { reason: 914 })
                            ),
                            "loop control must reject route-arm resolver vocabulary"
                        );

                        let non_binary_eff = EffIndex::new(10);
                        cluster
                            .register_dynamic_policy_resolver(
                                rv_id,
                                non_binary_eff,
                                tag,
                                policy,
                                tag,
                                ControlOp::RouteDecision,
                                None,
                                ResolverRef::from_fn(non_binary_route_resolution),
                            )
                            .expect("register non-binary route resolver");
                        assert!(
                            matches!(
                                cluster.resolve_dynamic_policy(
                                    rv_id,
                                    None,
                                    Lane::new(1),
                                    non_binary_eff,
                                    tag,
                                    ControlOp::RouteDecision,
                                    [0; 4],
                                    &crate::transport::context::PolicyAttrs::EMPTY,
                                ),
                                Err(CpError::PolicyAbort { reason: 914 })
                            ),
                            "route decision must reject non-binary route arms"
                        );
                    });
                });
            },
        );
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

    #[test]
    fn set_resolver_and_enter_materialize_transient_compiled_artifacts_each_time() {
        run_on_transient_compiled_test_stack(
            "set_resolver_and_enter_materialize_transient_compiled_artifacts_each_time",
            || {
                with_cluster_fixture(|clock, config| {
                    with_test_cluster(clock, |cluster| {
                        let route_policy_program_one = route_policy_program_one();
                        let route_policy_projected_one: SharedBorrowRoleProgram =
                            role_program::project(&route_policy_program_one);
                        let rv_id = cluster
                            .add_rendezvous_from_config(config, DummyTransport)
                            .expect("register rendezvous");

                        cluster
                            .set_resolver::<ROUTE_POLICY_ONE, 0>(
                                rv_id,
                                &route_policy_projected_one,
                                ResolverRef::from_fn(route_resolver),
                            )
                            .expect("register resolver");

                        cluster
                            .with_transient_compiled_role(
                                rv_id,
                                &route_policy_projected_one,
                                |_| Ok::<(), AttachError>(()),
                            )
                            .expect("materialize transient compiled role");

                        cluster
                            .with_transient_compiled_role(
                                rv_id,
                                &route_policy_projected_one,
                                |_| Ok::<(), AttachError>(()),
                            )
                            .expect("rematerialize transient compiled role");
                    });
                });
            },
        );
    }

    #[test]
    fn equivalent_borrowed_role_programs_reuse_shared_runtime_image() {
        run_on_transient_compiled_test_stack(
            "equivalent_borrowed_role_programs_reuse_shared_runtime_image",
            || {
                with_cluster_fixture(|clock, config| {
                    with_test_cluster(clock, |cluster| {
                        let shared_borrow_program_a = shared_borrow_program_a();
                        let shared_borrow_program_b = shared_borrow_program_b();
                        let shared_borrow_projected_a: SharedBorrowRoleProgram =
                            role_program::project(&shared_borrow_program_a);
                        let shared_borrow_projected_b: SharedBorrowRoleProgram =
                            role_program::project(&shared_borrow_program_b);
                        let rv_id = cluster
                            .add_rendezvous_from_config(config, DummyTransport)
                            .expect("register rendezvous");

                        assert_eq!(
                            shared_borrow_projected_a.stamp(),
                            shared_borrow_projected_b.stamp()
                        );
                        assert_eq!(
                            shared_borrow_projected_a.borrow_id(),
                            shared_borrow_projected_b.borrow_id(),
                            "equivalent thin RoleProgram values should borrow the same shared source owner"
                        );

                        cluster
                            .with_transient_compiled_role(rv_id, &shared_borrow_projected_a, |_| {
                                Ok::<(), AttachError>(())
                            })
                            .expect("materialize first borrowed program");

                        cluster
                            .with_transient_compiled_role(rv_id, &shared_borrow_projected_b, |_| {
                                Ok::<(), AttachError>(())
                            })
                            .expect("materialize second borrowed program");
                    });
                });
            },
        );
    }

    #[test]
    fn set_resolver_registers_dynamic_policy_sites_without_resident_cache() {
        run_on_transient_compiled_test_stack(
            "set_resolver_registers_dynamic_policy_sites_without_resident_cache",
            || {
                with_cluster_fixture(|clock, config| {
                    with_test_cluster(clock, |cluster| {
                        let route_policy_program_two = route_policy_program_two();
                        let route_policy_projected_two: SharedBorrowRoleProgram =
                            role_program::project(&route_policy_program_two);
                        let rv_id = cluster
                            .add_rendezvous_from_config(config, DummyTransport)
                            .expect("register rendezvous");

                        cluster
                            .set_resolver::<ROUTE_POLICY_TWO, 0>(
                                rv_id,
                                &route_policy_projected_two,
                                ResolverRef::from_fn(route_resolver),
                            )
                            .expect("register resolver without a free cache slot");

                        crate::global::compiled::materialize::with_compiled_program(
                            crate::global::lowering_input(&route_policy_projected_two),
                            |compiled| {
                                let site = compiled
                                    .dynamic_policy_sites_for(ROUTE_POLICY_TWO)
                                    .next()
                                    .expect("dynamic policy site");
                                assert!(
                                    cluster
                                        .dynamic_resolver(DynamicResolverKey::new(
                                            rv_id,
                                            site.eff_index(),
                                            site.op().expect("route policy op")
                                        ))
                                        .is_some(),
                                    "resolver registration must still succeed when the cache is saturated"
                                );
                            },
                        );
                    });
                });
            },
        );
    }
}
