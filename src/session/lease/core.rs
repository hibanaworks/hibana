//! Lease-first rendezvous table.
//!
//! This module centralizes rendezvous access behind an explicit, RAII-based
//! leasing API. `RendezvousTable::lease()` is the narrow entry point for marking a
//! rendezvous active while constructing its detached port and lane guard.
//!
//! The design goals are:
//! - **Centralized owner access** — published rendezvous state mutates only
//!   through its dedicated cells, and active entries reject conflicting access.
//! - **Affine lifecycle** — leases release themselves on drop, and cannot be
//!   cloned or duplicated.
//! - **Const-friendly session storage** — the lease layer stays allocation free
//!   and is ready for `no_std`.

use core::{cell::Cell, ptr::NonNull};

use crate::rendezvous::core::{EndpointLeaseId, EndpointResidentBudget, LaneRelease, Rendezvous};
use crate::session::types::{Lane, RendezvousId, SessionId};
use crate::transport::Transport;

mod registry_ops;
pub(crate) use registry_ops::{EndpointLeaseRequest, RegisterRendezvousError};

/// Local intrusive registry linked through each slab-resident rendezvous header.
pub(crate) struct RendezvousTable<'cfg, T: Transport> {
    head: Cell<Option<NonNull<Rendezvous<'cfg, 'cfg, T>>>>,
}

impl<'cfg, T> RendezvousTable<'cfg, T>
where
    T: Transport,
{
    pub(crate) const fn empty() -> Self {
        Self {
            head: Cell::new(None),
        }
    }

    fn node_ref(&self, id: &RendezvousId) -> Option<&Rendezvous<'cfg, 'cfg, T>> {
        let mut current = self.head.get();
        while let Some(rendezvous_ptr) = current {
            let rendezvous = /* SAFETY: `self.head` links initialized rendezvous
            headers pinned in caller slabs; shared lookup only follows their
            owner-local intrusive links. */ unsafe {
                rendezvous_ptr.as_ref()
            };
            if rendezvous.registry_id() == *id {
                return Some(rendezvous);
            }
            current = rendezvous.registry_next();
        }
        None
    }

    /// Borrow a rendezvous by shared reference when no lease is active.
    pub(crate) fn get(&self, id: &RendezvousId) -> Option<&Rendezvous<'cfg, 'cfg, T>> {
        self.node_ref(id)
            .filter(|rendezvous| !rendezvous.access_is_busy())
    }

    /// Borrow an available rendezvous, keeping absence distinct from an active
    /// affine lease.
    pub(crate) fn get_checked(
        &self,
        id: &RendezvousId,
    ) -> Result<&Rendezvous<'cfg, 'cfg, T>, LeaseError> {
        let rendezvous = self
            .node_ref(id)
            .ok_or(LeaseError::RendezvousUnregistered(*id))?;
        if rendezvous.access_is_busy() {
            return Err(LeaseError::AlreadyLeased(*id));
        }
        Ok(rendezvous)
    }

    /// Obtain a lease for the rendezvous identified by `rv_id`.
    pub(crate) fn lease<'lease>(
        &'lease self,
        rv_id: RendezvousId,
    ) -> Result<RendezvousLease<'lease, 'cfg, T>, LeaseError>
    where
        'cfg: 'lease,
    {
        let rendezvous = self
            .node_ref(&rv_id)
            .ok_or(LeaseError::RendezvousUnregistered(rv_id))?;
        if rendezvous.access_is_busy() {
            return Err(LeaseError::AlreadyLeased(rv_id));
        }
        rendezvous.acquire_registry_lease();
        Ok(RendezvousLease::new(rendezvous))
    }
}

/// Leasing failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LeaseError {
    /// No rendezvous with the requested identifier exists.
    RendezvousUnregistered(RendezvousId),
    /// The rendezvous is currently leased and cannot be borrowed again.
    AlreadyLeased(RendezvousId),
}

/// RAII lease over a rendezvous slot.
///
/// The lease is affine: it cannot be cloned, and dropping it automatically marks
/// the underlying rendezvous as available again.
pub(crate) struct RendezvousLease<'lease, 'cfg, T: Transport>
where
    'cfg: 'lease,
{
    rendezvous: Option<&'lease Rendezvous<'cfg, 'cfg, T>>,
}

impl<'lease, 'cfg, T> RendezvousLease<'lease, 'cfg, T>
where
    T: Transport,
    'cfg: 'lease,
{
    fn new(rendezvous: &'lease Rendezvous<'cfg, 'cfg, T>) -> Self {
        Self {
            rendezvous: Some(rendezvous),
        }
    }

    #[inline]
    fn rendezvous(&self) -> &Rendezvous<'cfg, 'cfg, T> {
        match self.rendezvous.as_ref() {
            Some(rendezvous) => rendezvous,
            None => crate::invariant(),
        }
    }

    #[inline]
    pub(crate) fn with_rendezvous<R>(&self, f: impl FnOnce(&Rendezvous<'cfg, 'cfg, T>) -> R) -> R {
        f(self.rendezvous())
    }
    #[inline]
    pub(crate) fn brand(&self) -> crate::session::brand::Guard<'cfg> {
        self.with_rendezvous(|rv| rv.brand())
    }

    #[inline]
    pub(crate) fn release_lane_with_tap(&mut self, sid: SessionId, lane: Lane) {
        self.with_rendezvous(|rv| match rv.release_lane(sid, lane) {
            LaneRelease::Released => {
                rv.emit_lane_release(sid, lane);
            }
            LaneRelease::StillHeld => {}
        });
    }
}

impl<'lease, 'cfg, T> Drop for RendezvousLease<'lease, 'cfg, T>
where
    T: Transport,
    'cfg: 'lease,
{
    fn drop(&mut self) {
        if let Some(rendezvous) = self.rendezvous.take() {
            rendezvous.release_registry_lease();
        }
    }
}
