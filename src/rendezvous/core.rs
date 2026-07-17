//! Rendezvous session primitives.
//!
//! The rendezvous component owns the association tables that map session
//! identifiers to transport lanes. It also owns route state, endpoint leases,
//! resolver storage, and lane release side effects for one rendezvous image.
//!
//! # Unsafe Owner Contract
//!
//! This module owns resident rendezvous images and endpoint lease tables.
//! Unsafe blocks here may initialize or migrate pinned rendezvous storage, but
//! must preserve association, resolver, and endpoint-lease owner roots
//! before returning safe ports or endpoints.

use core::{
    cell::{Cell, UnsafeCell},
    marker::PhantomData,
    ptr::NonNull,
};

use super::{
    association::AssocTable,
    error::RendezvousError,
    port::{Port, PortInit},
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
mod access_state;
pub(crate) use access_state::{EndpointOperationLease, RendezvousAccessState};

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct EndpointLeaseId(u16);

impl EndpointLeaseId {
    #[inline]
    pub(in crate::rendezvous::core) const fn slot_count_is_representable(
        slot_count: usize,
    ) -> bool {
        match slot_count.checked_sub(1) {
            None => true,
            Some(last_slot) => last_slot <= u16::MAX as usize,
        }
    }
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
    Released,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EndpointResidentBudget {
    pub(crate) frontier_workspace_bytes: u16,
}

impl EndpointResidentBudget {
    pub(crate) const ZERO: Self = Self {
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
    pub(crate) const fn with_frontier_workspace(frontier_workspace_bytes: usize) -> Self {
        Self {
            frontier_workspace_bytes: Self::compact_u16(frontier_workspace_bytes),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EndpointLeaseSlot {
    pub(crate) generation: u32,
    pub(crate) sid: SessionId,
    pub(crate) offset: u32,
    pub(crate) len: u32,
    pub(crate) resident_budget: EndpointResidentBudget,
    pub(crate) role: u8,
    pub(crate) state: EndpointLeaseState,
}

impl EndpointLeaseSlot {
    const EMPTY: Self = Self {
        generation: 0,
        sid: SessionId::new(0),
        offset: 0,
        len: 0,
        resident_budget: EndpointResidentBudget::ZERO,
        role: 0,
        state: EndpointLeaseState::Vacant,
    };

    #[inline]
    pub(crate) const fn is_occupied(&self) -> bool {
        self.state.is_occupied()
    }

    #[inline]
    pub(crate) const fn is_published(&self) -> bool {
        self.state.is_published()
    }
}

pub(crate) struct EndpointLeaseRecord {
    slot: Cell<EndpointLeaseSlot>,
    waiter: endpoint_waiter::EndpointWaiter,
}

impl EndpointLeaseRecord {
    #[inline]
    const fn empty() -> Self {
        Self {
            slot: Cell::new(EndpointLeaseSlot::EMPTY),
            waiter: endpoint_waiter::EndpointWaiter::empty(),
        }
    }

    #[inline]
    pub(crate) const fn slot(&self) -> EndpointLeaseSlot {
        self.slot.get()
    }

    #[inline]
    pub(crate) fn set_slot(&self, slot: EndpointLeaseSlot) {
        self.slot.set(slot);
    }

    #[inline]
    pub(crate) fn replace_waiter(&self, waker: core::task::Waker) -> Option<core::task::Waker> {
        self.waiter.replace(waker)
    }

    #[inline]
    pub(crate) fn take_waiter(&self) -> Option<core::task::Waker> {
        self.waiter.take()
    }

    #[inline]
    pub(crate) fn waiter_is_empty(&self) -> bool {
        self.waiter.is_empty()
    }

    #[inline]
    pub(crate) fn storage_slot_count(storage: Sidecar<Self>) -> usize {
        if storage.is_empty() {
            return 0;
        }
        let slot_bytes = core::mem::size_of::<Self>();
        if slot_bytes == 0 || !storage.bytes().is_multiple_of(slot_bytes) {
            crate::invariant();
        }
        let slots = storage.bytes() / slot_bytes;
        if !EndpointLeaseId::slot_count_is_representable(slots) {
            crate::invariant();
        }
        slots
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EndpointLeaseState {
    Vacant = 0,
    Reserved = 1,
    Published = 2,
    MembershipSealed = 3,
}

impl EndpointLeaseState {
    #[inline]
    const fn is_occupied(self) -> bool {
        !matches!(self, Self::Vacant)
    }

    #[inline]
    const fn is_published(self) -> bool {
        matches!(self, Self::Published | Self::MembershipSealed)
    }

    #[inline]
    const fn is_membership_sealed(self) -> bool {
        matches!(self, Self::MembershipSealed)
    }

    #[inline]
    const fn seal_membership(self) -> Option<Self> {
        match self {
            Self::Published | Self::MembershipSealed => Some(Self::MembershipSealed),
            Self::Vacant | Self::Reserved => None,
        }
    }
}

pub(crate) struct Rendezvous<'rv, 'cfg, T: Transport>
where
    'cfg: 'rv,
{
    brand_marker: PhantomData<brand::Brand<'rv>>,
    id: RendezvousId,
    access_state: Cell<RendezvousAccessState>,
    registry_next: Cell<Option<NonNull<Rendezvous<'rv, 'cfg, T>>>>,
    resolver_bucket: UnsafeCell<crate::session::cluster::core::ResolverBucket<'cfg>>,
    tap: TapRing<'cfg>,
    slab_ptr: *mut u8,
    slab_len: usize,
    slab_marker: PhantomData<&'cfg mut [u8]>,
    image_frontier: Cell<u32>,
    frontier_workspace_bytes: Cell<u32>,
    endpoint_lease_generation: Cell<u32>,
    endpoint_lease_storage: Cell<Sidecar<EndpointLeaseRecord>>,
    lane_base: Cell<u32>,
    lane_end: Cell<u32>,
    transport: T,
    assoc_storage: Cell<Sidecar<u8>>,
    assoc: AssocTable,
}

impl<'rv, 'cfg, T: Transport> Rendezvous<'rv, 'cfg, T>
where
    'cfg: 'rv,
{
    #[inline]
    pub(crate) const fn registry_id(&self) -> RendezvousId {
        self.id
    }

    #[inline]
    pub(crate) fn registry_next(&self) -> Option<NonNull<Self>> {
        self.registry_next.get()
    }

    #[inline]
    pub(crate) fn link_registry_next(&self, next: Option<NonNull<Self>>) {
        if self.registry_next.get().is_some() {
            crate::invariant();
        }
        self.registry_next.set(next);
    }

    #[inline]
    pub(crate) fn access_is_busy(&self) -> bool {
        self.access_state.get() != RendezvousAccessState::Available
    }

    #[inline]
    pub(crate) fn try_endpoint_operation_lease(&self) -> Option<EndpointOperationLease<'_>> {
        let next = self.access_state.get().begin_endpoint_operation()?;
        self.access_state.set(next);
        Some(EndpointOperationLease::new(&self.access_state))
    }

    #[inline]
    pub(crate) fn acquire_registry_lease(&self) {
        if self.access_state.get() != RendezvousAccessState::Available {
            crate::invariant();
        }
        self.access_state.set(RendezvousAccessState::RegistryLease);
    }

    #[inline]
    pub(crate) fn release_registry_lease(&self) {
        if self.access_state.get() != RendezvousAccessState::RegistryLease {
            crate::invariant();
        }
        self.access_state.set(RendezvousAccessState::Available);
    }

    #[inline]
    pub(crate) fn resolver_storage_sidecar(&self) -> Sidecar<u8> {
        /* SAFETY: shared sidecar-root inspection copies the resolver descriptor
        without borrowing any resolver entry. */
        unsafe { (&*self.resolver_bucket.get()).erased_storage_sidecar() }
    }

    pub(crate) fn insert_dynamic_resolver(
        &self,
        registration: crate::session::cluster::core::ResolverRegistrationKey,
        resolver_ref: crate::session::cluster::core::ErasedResolverRef<'cfg>,
    ) -> Result<(), crate::session::cluster::error::ClusterError> {
        /* SAFETY: registry access rejects an active affine lease; this
        local-only rendezvous solely mutates the initialized resolver bucket. */
        unsafe { (&mut *self.resolver_bucket.get()).insert(registration, resolver_ref) }
    }

    pub(crate) fn dynamic_resolver(
        &self,
        registration: crate::session::cluster::core::ResolverRegistrationKey,
    ) -> Option<crate::session::cluster::core::ErasedResolverRef<'cfg>> {
        /* SAFETY: shared lookup copies the initialized resolver entry before
        any callback can run. */
        unsafe { (&*self.resolver_bucket.get()).get(registration) }
    }
}

mod endpoint_leases;
mod endpoint_waiter;
mod lane_lifecycle;
mod storage_layout;
mod storage_runtime_budget;

/// RAII witness for lane access through a leased rendezvous entry.
///
/// Construction succeeds only after the rendezvous has recorded the matching
/// `(SessionId, Lane)` claim. Consuming the lease transfers that release
/// authority into a `LaneGuard`; dropping it before conversion releases the
/// claim directly. The registry lease remains affine throughout construction.
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
        active_leases: &'lease core::cell::Cell<u32>,
        brand: crate::session::brand::Guard<'cfg>,
    ) -> Self {
        Self {
            lease: Some(lease),
            sid,
            lane,
            role,
            active_leases: Some(active_leases),
            brand,
        }
    }

    pub(crate) fn into_port_guard(
        mut self,
    ) -> Result<LanePortAccess<'lease, 'cfg, T>, RendezvousError> {
        let opened = {
            let lease = crate::invariant_some(self.lease.as_mut());
            let rv_ptr: *const Rendezvous<'cfg, 'cfg, T> =
                lease.with_rendezvous(core::ptr::from_ref);
            let rv: &'lease Rendezvous<'cfg, 'cfg, T> =
                /* SAFETY: the active `RendezvousLease` keeps the pinned registry
                entry alive for `'lease`; published rendezvous mutation uses only
                interior owner cells. */ unsafe { &*rv_ptr };
            let active_leases = *crate::invariant_some(self.active_leases.as_ref());
            rv.open_port_guard(self.sid, self.lane, self.role, active_leases)
        };
        let (port, guard) = opened?;
        self.lease = None;
        self.active_leases = None;
        Ok(LanePortAccess {
            port,
            lane_guard: guard,
            brand: self.brand,
        })
    }
}

impl<'lease, 'cfg, T> Drop for LaneLease<'lease, 'cfg, T>
where
    T: Transport,
    'cfg: 'lease,
{
    fn drop(&mut self) {
        if let Some(mut lease) = self.lease.take() {
            lease.release_lane_with_tap(self.sid, self.lane);
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
