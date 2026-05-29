//! Rendezvous (control plane) primitives.
//!
//! The rendezvous component owns the association tables that map session
//! identifiers to transport lanes. It also owns resident generation state,
//! capability ledgers, topology reservations, endpoint leases, and lane release
//! side effects for one rendezvous image.
//!
//! # Unsafe Owner Contract
//!
//! This module owns resident rendezvous images and endpoint lease tables.
//! Unsafe blocks here may initialize or migrate pinned rendezvous storage, but
//! must preserve the association table, generation table, and endpoint lease
//! lifetimes before returning safe ports or endpoints.

use core::{cell::Cell, marker::PhantomData, ops::Range, task::Waker};

use super::{
    association::AssocTable,
    capability::{CapReleaseCtx, CapTable},
    error::{
        GenError, GenerationRecord, RendezvousError, StateRestoreError, TopologyError,
        TxAbortError, TxCommitError,
    },
    port::{Port, PortInit},
    tables::{
        GenTable, LoopTable, PolicyTable, PreparedSnapshotFinalization, PreparedSnapshotRecord,
        RouteTable, SnapshotFinalization, SnapshotFinalizeTarget, StateSnapshotTable,
    },
    topology::{PendingTopology, TopologyStateTable},
};
use crate::{
    control::{
        automaton::txn::{NoopTap, Txn},
        brand::{self, Guard},
        cap::mint::{ControlOp, NonceSeed},
        cluster::{
            core::TopologyOperands,
            error::{CpError, ResourceScope},
        },
        types::{IncreasingGen, One},
    },
    eff::EffIndex,
    endpoint::affine::LaneGuard,
    global::const_dsl::{ControlScopeKind, PolicyMode},
    observe::core::{TapEvent, TapRing, emit},
    observe::events::{LaneRelease, RawEvent, StateRestoreOk},
    runtime::config::{Clock, Config, CounterClock},
    runtime::consts::{DefaultLabelUniverse, LabelUniverse},
    transport::Transport,
};

use super::topology::{LocalTopologyInvariant, TopologyLeaseState, TopologySessionState};
pub(crate) use super::topology::{
    PreparedDestinationTopologyCommit as ReservedDestinationTopologyCommitProof,
    PreparedSourceTopologyCommit as ReservedSourceTopologyCommitProof,
};
use crate::control::automaton::distributed::{TopologyAck, TopologyIntent};
use crate::control::cluster::effects::control_op_tap_event_id;
use crate::control::types::{Generation, Lane, RendezvousId, SessionId};

type EpochPort<'a, T> = Port<'a, T, crate::control::cap::mint::EpochTbl>;
type EpochPortGuard<'a, T, U, C> = (EpochPort<'a, T>, LaneGuard<'a, T, U, C>);
type BrandedEpochPortGuard<'a, 'cfg, T, U, C> = (
    EpochPort<'a, T>,
    LaneGuard<'a, T, U, C>,
    crate::control::brand::Guard<'cfg>,
);

#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct EndpointLeaseId(u16);

impl EndpointLeaseId {
    pub(crate) const ZERO: Self = Self(0);
}

impl From<u8> for EndpointLeaseId {
    #[inline]
    fn from(value: u8) -> Self {
        Self(value.into())
    }
}

impl From<u16> for EndpointLeaseId {
    #[inline]
    fn from(value: u16) -> Self {
        Self(value)
    }
}

impl From<EndpointLeaseId> for u16 {
    #[inline]
    fn from(value: EndpointLeaseId) -> Self {
        value.0
    }
}

impl From<EndpointLeaseId> for u32 {
    #[inline]
    fn from(value: EndpointLeaseId) -> Self {
        value.0.into()
    }
}

impl From<EndpointLeaseId> for usize {
    #[inline]
    fn from(value: EndpointLeaseId) -> Self {
        value.0.into()
    }
}

impl TryFrom<usize> for EndpointLeaseId {
    type Error = core::num::TryFromIntError;

    #[inline]
    fn try_from(value: usize) -> Result<Self, Self::Error> {
        u16::try_from(value).map(Self)
    }
}

impl core::fmt::Display for EndpointLeaseId {
    #[inline]
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EndpointResidentBudget {
    pub(crate) route_frame_slots: u16,
    pub(crate) loop_slots: u16,
    pub(crate) cap_entries: u16,
    pub(crate) route_lane_slots: u8,
    pub(crate) frontier_workspace_bytes: u16,
}

impl EndpointResidentBudget {
    pub(crate) const ZERO: Self = Self {
        route_frame_slots: 0,
        loop_slots: 0,
        cap_entries: 0,
        route_lane_slots: 0,
        frontier_workspace_bytes: 0,
    };

    #[inline]
    const fn compact_u16(value: usize) -> u16 {
        if value > u16::MAX as usize {
            panic!("endpoint resident budget u16 overflow");
        }
        value as u16
    }

    #[inline]
    const fn compact_u8(value: usize) -> u8 {
        if value > u8::MAX as usize {
            panic!("endpoint resident budget u8 overflow");
        }
        value as u8
    }

    #[inline]
    pub(crate) const fn with_route_storage(
        route_frame_slots: usize,
        route_lane_slots: usize,
        loop_slots: usize,
        cap_entries: usize,
        frontier_workspace_bytes: usize,
    ) -> Self {
        Self {
            route_frame_slots: Self::compact_u16(route_frame_slots),
            loop_slots: Self::compact_u16(loop_slots),
            cap_entries: Self::compact_u16(cap_entries),
            route_lane_slots: Self::compact_u8(route_lane_slots),
            frontier_workspace_bytes: Self::compact_u16(frontier_workspace_bytes),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct EndpointLeaseSlot {
    pub(crate) generation: u32,
    pub(crate) offset: u32,
    pub(crate) len: u32,
    pub(crate) resident_budget: EndpointResidentBudget,
    pub(crate) public_endpoint: bool,
    pub(crate) occupied: bool,
}

impl EndpointLeaseSlot {
    const EMPTY: Self = Self {
        generation: 0,
        offset: 0,
        len: 0,
        resident_budget: EndpointResidentBudget::ZERO,
        public_endpoint: false,
        occupied: false,
    };
}

const FREE_REGION_CAPACITY: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FreeRegion {
    offset: u32,
    len: u32,
    occupied: bool,
}

impl FreeRegion {
    const EMPTY: Self = Self {
        offset: 0,
        len: 0,
        occupied: false,
    };
}

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
    image_frontier: u32,
    frontier_workspace_bytes: u32,
    endpoint_leases: *mut EndpointLeaseSlot,
    endpoint_lease_capacity: EndpointLeaseId,
    endpoint_lease_reclaim_delta: u16,
    runtime_frontier: u32,
    free_regions: [FreeRegion; FREE_REGION_CAPACITY],
    lane_range: Range<u32>,
    universe_marker: PhantomData<U>,
    transport: T,
    r#gen: GenTable,
    assoc: AssocTable,
    state_snapshots: StateSnapshotTable,
    topology: TopologyStateTable,
    cap_nonce: Cell<u64>,
    cap_revision: Cell<u64>,
    caps: CapTable,
    loops: LoopTable,
    routes: RouteTable,
    policies: PolicyTable,
    clock: C,
    offer_progress_policy: crate::runtime::config::OfferProgressPolicy,
    _epoch_marker: PhantomData<E>,
}

mod effects;
mod endpoint_leases;
mod lane_lifecycle;
mod prepared_effects;
mod storage_layout;
mod storage_runtime_budget;
pub(crate) use prepared_effects::{
    PreparedAbortAckEffect, PreparedAbortBeginEffect, PreparedStateRestoreEffect,
    PreparedStateSnapshotEffect, PreparedTxAbortEffect, PreparedTxCommitEffect,
};

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
/// let mut rv = /* some Rendezvous owner */; // 'rv starts
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
/// let mut rv = /* some Rendezvous owner */;
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
/// accessible to integration tests. Public API users obtain endpoints via the
/// `SessionKit::rendezvous(...).session(...).role(...).enter(...)` witness chain.
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
/// 1. `cluster_ptr` always points to a valid `SessionKit` during `'lease`
/// 2. Only `LaneLease::Drop` calls back into the cluster to release the lane
/// 3. SessionKit guarantees: no duplicate leases for same lane
/// 4. SessionKit guarantees: no Rendezvous write access while lease held
/// 5. Cluster must not move while lease is alive (enforced by the PhantomData borrow)
///
/// # Observable Properties
///
/// - LANE_ACQUIRE tap event on lease creation (via `SessionKit::lease_port`)
/// - LANE_RELEASE tap event on Drop
/// - Streaming checker verifies acquire/release pairs match (similar to cancel begin/ack)
pub(crate) struct LaneLease<'lease, 'cfg, T, U, C, const MAX_RV: usize>
where
    T: Transport,
    U: LabelUniverse + 'cfg,
    C: Clock + 'cfg,
    'cfg: 'lease,
{
    /// Borrow-bound lease over the parent rendezvous.
    lease: Option<
        crate::control::lease::core::RendezvousLease<
            'lease,
            'cfg,
            T,
            U,
            C,
            crate::control::cap::mint::EpochTbl,
            crate::control::lease::core::FullSpec,
        >,
    >,
    /// Session identifier.
    sid: SessionId,
    /// Lane identifier.
    lane: Lane,
    /// Role for the port.
    role: u8,
    /// Number of global roles participating in the attached program.
    role_count: u8,
    /// Active lease counter borrowed from the parent cluster.
    active_leases: Option<&'lease core::cell::Cell<u32>>,
    /// Rendezvous brand for typed owner construction.
    brand: crate::control::brand::Guard<'cfg>,
}

impl<'lease, 'cfg, T, U, C, const MAX_RV: usize> LaneLease<'lease, 'cfg, T, U, C, MAX_RV>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    'cfg: 'lease,
{
    /// Internal constructor (called by `SessionKit::lease_port`).
    /// The caller must ensure no duplicate leases for the same `(rv_id, lane)` pair.
    pub(crate) fn new(
        lease: crate::control::lease::core::RendezvousLease<
            'lease,
            'cfg,
            T,
            U,
            C,
            crate::control::cap::mint::EpochTbl,
            crate::control::lease::core::FullSpec,
        >,
        sid: SessionId,
        lane: Lane,
        role: u8,
        role_count: u8,
        active_leases: &'lease core::cell::Cell<u32>,
        brand: crate::control::brand::Guard<'cfg>,
    ) -> Self {
        Self {
            lease: Some(lease),
            sid,
            lane,
            role,
            role_count,
            active_leases: Some(active_leases),
            brand,
        }
    }

    pub(crate) fn into_port_guard(
        mut self,
    ) -> Result<BrandedEpochPortGuard<'lease, 'cfg, T, U, C>, RendezvousError> {
        let (port, guard) = {
            let lease = self
                .lease
                .as_mut()
                .expect("lane lease retains rendezvous lease");
            // SAFETY: `RendezvousLease<'lease, 'cfg, ...>` holds the unique mutable
            // entry borrow for `'lease`, so reborrowing the rendezvous as shared for
            // the same `'lease` lifetime is sound as long as we do not use the lease
            // mutably while the shared reference is live.
            let rv_ptr: *mut Rendezvous<'cfg, 'cfg, T, U, C, crate::control::cap::mint::EpochTbl> =
                lease.with_rendezvous(core::ptr::from_mut);
            let rv: &'lease Rendezvous<'cfg, 'cfg, T, U, C, crate::control::cap::mint::EpochTbl> =
                /* SAFETY: the pointer comes from pinned owner storage and this path only creates a shared borrow. */ unsafe { &*rv_ptr };
            let active_leases = *self
                .active_leases
                .as_ref()
                .expect("lane lease retains active lease counter");
            rv.open_port_guard(
                self.sid,
                self.lane,
                self.role,
                self.role_count,
                active_leases,
            )?
        };
        drop(self.lease.take());
        let _ = self.active_leases.take();
        Ok((port, guard, self.brand))
    }

    #[inline]
    pub(crate) fn with_rendezvous_mut<R>(
        &mut self,
        f: impl FnOnce(&mut Rendezvous<'cfg, 'cfg, T, U, C, crate::control::cap::mint::EpochTbl>) -> R,
    ) -> R {
        let lease = self
            .lease
            .as_mut()
            .expect("lane lease retains rendezvous lease");
        lease.with_rendezvous(f)
    }
}

impl<'lease, 'cfg, T, U, C, const MAX_RV: usize> Drop for LaneLease<'lease, 'cfg, T, U, C, MAX_RV>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    'cfg: 'lease,
{
    fn drop(&mut self) {
        if let Some(mut lease) = self.lease.take() {
            lease.release_lane_with_tap(self.lane);
        }
        if let Some(active_leases) = self.active_leases.take() {
            let current = active_leases.get();
            debug_assert!(current > 0, "lane_release underflow");
            active_leases.set(current.saturating_sub(1));
        }
    }
}

mod access_port;
mod cap_ledger;
mod topology_process;
pub(crate) use topology_process::PreparedDestinationTopologyAck;

#[inline]
fn classify_topology_ack_mismatch(expected: TopologyAck, got: TopologyAck) -> TopologyError {
    if got.sid != expected.sid {
        TopologyError::UnknownSession {
            sid: SessionId::new(got.sid),
        }
    } else if got.src_rv != expected.src_rv {
        TopologyError::RendezvousIdMismatch {
            expected: expected.src_rv,
            got: got.src_rv,
        }
    } else if got.dst_rv != expected.dst_rv {
        TopologyError::RendezvousIdMismatch {
            expected: expected.dst_rv,
            got: got.dst_rv,
        }
    } else if got.src_lane != expected.src_lane {
        TopologyError::LaneMismatch {
            expected: expected.src_lane,
            provided: got.src_lane,
        }
    } else if got.new_lane != expected.new_lane {
        TopologyError::LaneMismatch {
            expected: expected.new_lane,
            provided: got.new_lane,
        }
    } else if got.new_gen != expected.new_gen {
        TopologyError::StaleGeneration {
            lane: expected.new_lane,
            last: expected.new_gen,
            new: got.new_gen,
        }
    } else if got.seq_tx != expected.seq_tx || got.seq_rx != expected.seq_rx {
        TopologyError::SeqnoMismatch {
            seq_tx: got.seq_tx,
            seq_rx: got.seq_rx,
        }
    } else {
        TopologyError::NoPending {
            lane: expected.src_lane,
        }
    }
}

mod local_topology;

#[cfg(all(test, hibana_repo_tests))]
#[path = "core/tests.rs"]
mod epf_tests;

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
    /// Borrow topology coordination state as a constrained facet.
    #[cfg(test)]
    pub(crate) fn topology_facet(&mut self) -> TopologyFacet<T, U, C, E> {
        TopologyFacet::new()
    }
}

/// Topology-focused facet that exposes only topology coordination operations.
#[cfg(test)]
#[derive(Default)]
pub(crate) struct TopologyFacet<T, U, C, E>(PhantomData<(T, U, C, E)>)
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable;

#[cfg(test)]
impl<T, U, C, E> Copy for TopologyFacet<T, U, C, E>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
{
}

#[cfg(test)]
impl<T, U, C, E> Clone for TopologyFacet<T, U, C, E>
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

#[cfg(test)]
impl<T, U, C, E> TopologyFacet<T, U, C, E>
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

    pub(crate) fn begin_from_intent(
        self,
        rendezvous: &mut Rendezvous<'_, '_, T, U, C, E>,
        intent: TopologyIntent,
    ) -> Result<(), super::error::TopologyError> {
        rendezvous.topology_begin_from_intent(intent)
    }
}
