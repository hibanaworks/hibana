//! Rendezvous session primitives.
//!
//! The rendezvous component owns the association tables that map session
//! identifiers to transport lanes. It also owns resident generation state,
//! endpoint leases, and lane release side effects for one rendezvous image.
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
    error::RendezvousError,
    port::{Port, PortInit},
    tables::RouteTable,
};
use crate::session::types::{Lane, RendezvousId, SessionId};
use crate::{
    endpoint::affine::LaneGuard,
    observe::{
        core::{TapRing, emit},
        events,
    },
    runtime_core::resources::RuntimeResources,
    session::{
        brand::{self, Guard},
        cluster::error::ResourceScope,
    },
    transport::Transport,
};
pub(crate) use storage_layout::Sidecar;

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
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

pub(crate) struct LanePortAccess<'lease, 'cfg, T: Transport> {
    pub(crate) port: Port<'lease, T>,
    pub(crate) lane_guard: LaneGuard<'lease, T>,
    pub(crate) brand: Guard<'cfg>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LaneRelease {
    StillHeld,
    Released(SessionId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EndpointResidentBudget {
    pub(crate) route_frame_slots: u16,
    pub(crate) route_lane_slots: u8,
    pub(crate) frontier_workspace_bytes: u16,
}

impl EndpointResidentBudget {
    pub(crate) const ZERO: Self = Self {
        route_frame_slots: 0,
        route_lane_slots: 0,
        frontier_workspace_bytes: 0,
    };

    #[inline]
    const fn compact_u16(value: usize) -> u16 {
        if value > u16::MAX as usize {
            crate::invariant();
        }
        value as u16
    }

    #[inline]
    const fn compact_u8(value: usize) -> u8 {
        if value > u8::MAX as usize {
            crate::invariant();
        }
        value as u8
    }

    #[inline]
    pub(crate) const fn with_route_storage(
        route_frame_slots: usize,
        route_lane_slots: usize,
        frontier_workspace_bytes: usize,
    ) -> Self {
        Self {
            route_frame_slots: Self::compact_u16(route_frame_slots),
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
    pub(crate) state: EndpointLeaseState,
}

impl EndpointLeaseSlot {
    const EMPTY: Self = Self {
        generation: 0,
        offset: 0,
        len: 0,
        resident_budget: EndpointResidentBudget::ZERO,
        state: EndpointLeaseState::Vacant,
    };

    #[inline]
    pub(crate) const fn is_live(&self) -> bool {
        self.state.is_live()
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EndpointLeaseState {
    Vacant = 0,
    Live = 1,
}

impl EndpointLeaseState {
    #[inline]
    const fn is_live(self) -> bool {
        matches!(self, Self::Live)
    }
}

const FREE_REGION_CAPACITY: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FreeRegion {
    offset: u32,
    len: u32,
    state: FreeRegionState,
}

impl FreeRegion {
    const EMPTY: Self = Self {
        offset: 0,
        len: 0,
        state: FreeRegionState::Vacant,
    };

    #[inline]
    const fn recorded(offset: u32, len: u32) -> Self {
        Self {
            offset,
            len,
            state: FreeRegionState::Recorded,
        }
    }

    #[inline]
    const fn is_recorded(&self) -> bool {
        self.state.is_recorded()
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FreeRegionState {
    Vacant = 0,
    Recorded = 1,
}

impl FreeRegionState {
    #[inline]
    const fn is_recorded(self) -> bool {
        matches!(self, Self::Recorded)
    }
}

pub(crate) struct Rendezvous<'rv, 'cfg, T: Transport>
where
    'cfg: 'rv,
{
    brand_marker: PhantomData<brand::Brand<'rv>>,
    id: RendezvousId,
    tap: TapRing<'cfg>,
    tap_counter: Cell<u32>,
    slab: *mut [u8],
    slab_marker: PhantomData<&'cfg mut [u8]>,
    image_frontier: u32,
    frontier_workspace_bytes: u32,
    endpoint_lease_storage: Sidecar<EndpointLeaseSlot>,
    endpoint_lease_capacity: EndpointLeaseId,
    free_regions: [FreeRegion; FREE_REGION_CAPACITY],
    lane_range: Range<u32>,
    transport: T,
    assoc_storage: Sidecar<u8>,
    route_storage: Sidecar<u8>,
    assoc: AssocTable,
    routes: RouteTable,
}

mod endpoint_leases;
mod lane_lifecycle;
mod storage_layout;
mod storage_runtime_budget;

/// RAII witness for lane access through a leased rendezvous entry.
///
/// `LaneLease<'lease, 'cfg, ...>` owns a `RendezvousLease` and is consumed by
/// value when it is converted into a port. The lease core marks the rendezvous
/// entry active while the lease is alive, so the normal rendezvous lookup paths
/// reject a second mutable borrow of that entry.
///
/// ```text
/// Drop order:
///   LaneLease<'lease, ...> -> Port<'lease, ...> -> rendezvous borrow expires
/// ```
///
/// The borrow lifetime is independent of the rendezvous storage lifetime. This
/// permits short scopes where a lane lease is dropped before the rendezvous
/// storage owner itself:
///
/// ```text
/// let mut rv = /* some Rendezvous owner */; // 'rv starts
/// {
///     let lease = rv.lease_port(...);     // 'a: shorter borrow
/// }                                        // 'a ends, lease dropped
///                                          // rv can now be moved/dropped
/// ```
///
/// # Owner Guarantees
///
/// 1. The lease is affine; it is not cloneable.
/// 2. The lease stores the rendezvous entry lease that marked the entry active.
/// 3. Dropping the lease releases the lane and clears the active entry marker.
///
/// # Example
///
/// ```ignore
/// let mut rv = /* some Rendezvous owner */;
/// {
///     let lease = rv.lease_port(sid, lane, role)?;
///     let port = lease.port();
///     // ... use port
/// } // lease dropped here, lane released, borrow expires
/// // rv can now be safely dropped or moved
/// ```
///
/// # Visibility
///
/// This type is crate-private lane ownership machinery. Public API users obtain
/// endpoints via the
/// `SessionKit::rendezvous(...).enter(sid, &role_program)` path.
///
/// # Cluster Ownership Model
///
/// `LaneLease` owns the rendezvous lease outright. This ties the borrow
/// lifetime `'lease` to the leased rendezvous entry:
/// Cluster -> RendezvousLease -> LaneLease.
///
/// # Safety Invariants
///
/// 1. The stored `RendezvousLease` remains alive until the lane lease is
///    consumed or dropped.
/// 2. Normal cluster lookup paths refuse a rendezvous entry while its lease is
///    active.
/// 3. Callers must not reach storage through raw owner pointers without the active-entry marker.
///
/// # Observable Properties
///
/// - LANE_ACQUIRE tap event when a session/lane association count moves 0->1
/// - LANE_RELEASE tap event when that association count moves 1->0
/// - Streaming checker verifies association acquire/release pairs match
pub(crate) struct LaneLease<'lease, 'cfg, T>
where
    T: Transport,
    'cfg: 'lease,
{
    /// Borrow-bound lease over the parent rendezvous.
    lease: Option<crate::session::lease::core::RendezvousLease<'lease, 'cfg, T>>,
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
    brand: crate::session::brand::Guard<'cfg>,
}

impl<'lease, 'cfg, T> LaneLease<'lease, 'cfg, T>
where
    T: Transport,
    'cfg: 'lease,
{
    /// Constructs a rendezvous entry that has already been marked
    /// active by the lease table.
    pub(crate) fn new(
        lease: crate::session::lease::core::RendezvousLease<'lease, 'cfg, T>,
        sid: SessionId,
        lane: Lane,
        role: u8,
        role_count: u8,
        active_leases: &'lease core::cell::Cell<u32>,
        brand: crate::session::brand::Guard<'cfg>,
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

    pub(crate) fn into_port_guard(mut self) -> LanePortAccess<'lease, 'cfg, T> {
        let (port, guard) = {
            let lease = crate::invariant_some(self.lease.as_mut());
            // SAFETY: `RendezvousLease<'lease, 'cfg, ...>` holds the unique mutable
            // entry borrow for `'lease`, so reborrowing the rendezvous as shared for
            // the same `'lease` lifetime is sound as long as we do not use the lease
            // mutably while the shared reference is live.
            let rv_ptr: *mut Rendezvous<'cfg, 'cfg, T> = lease.with_rendezvous(core::ptr::from_mut);
            let rv: &'lease Rendezvous<'cfg, 'cfg, T> =
                /* SAFETY: the active `RendezvousLease` owns the pinned entry
                for `'lease`; this shared borrow is used only while the lease is
                not mutably accessed. */ unsafe { &*rv_ptr };
            let active_leases = *crate::invariant_some(self.active_leases.as_ref());
            rv.open_port_guard(
                self.sid,
                self.lane,
                self.role,
                self.role_count,
                active_leases,
            )
        };
        self.lease = None;
        self.active_leases = None;
        LanePortAccess {
            port,
            lane_guard: guard,
            brand: self.brand,
        }
    }

    #[inline]
    pub(crate) fn with_rendezvous_mut<R>(
        &mut self,
        f: impl FnOnce(&mut Rendezvous<'cfg, 'cfg, T>) -> R,
    ) -> R {
        let lease = crate::invariant_some(self.lease.as_mut());
        lease.with_rendezvous(f)
    }
}

impl<'lease, 'cfg, T> Drop for LaneLease<'lease, 'cfg, T>
where
    T: Transport,
    'cfg: 'lease,
{
    fn drop(&mut self) {
        if let Some(mut lease) = self.lease.take() {
            lease.release_lane_with_tap(self.lane);
        }
        if let Some(active_leases) = self.active_leases.take() {
            let current = active_leases.get();
            if current == 0 {
                crate::invariant();
            }
            active_leases.set(current - 1);
        }
    }
}

mod access_port;
