//! Lease-first rendezvous table.
//!
//! This module centralizes rendezvous access behind an explicit, RAII-based
//! leasing API. `RendezvousTable::lease()` is the narrow entry point for mutating a
//! leased rendezvous entry; other paths use checked lookup helpers.
//!
//! The design goals are:
//! - **Centralized mutable access** — raw storage pointers stay inside the
//!   lease table, and active entries reject normal mutable lookups.
//! - **Affine lifecycle** — leases release themselves on drop, and cannot be
//!   cloned or duplicated.
//! - **Const-friendly session storage** — the lease layer stays allocation free
//!   and is ready for `no_std`.

use core::{marker::PhantomData, ptr, ptr::NonNull};

use crate::rendezvous::core::{EndpointLeaseId, EndpointResidentBudget, LaneRelease, Rendezvous};
use crate::session::types::{Lane, RendezvousId, SessionId};
use crate::transport::Transport;

mod registry_ops;
pub(crate) use registry_ops::RegisterRendezvousError;

/// Local rendezvous registry backed by nodes carved from each rendezvous slab.
pub(crate) struct RendezvousTable<'cfg, T: Transport> {
    head: Option<NonNull<RendezvousEntry<'cfg, T>>>,
    len: u16,
}

impl<'cfg, T> RendezvousTable<'cfg, T>
where
    T: Transport,
{
    /// Initialize an empty rendezvous registry in place.
    ///
    /// # Safety
    /// `dst` must point to valid, writable memory for `Self`.
    pub(crate) unsafe fn init_empty(dst: *mut Self) {
        /* SAFETY: `SessionKitStorage::init` provides the unpublished
        `RendezvousTable` slot. This initializer writes both registry fields
        before any table lookup can borrow the list head. */
        unsafe {
            core::ptr::addr_of_mut!((*dst).head).write(None);
            core::ptr::addr_of_mut!((*dst).len).write(0);
        }
    }

    fn entry_ref(&self, id: &RendezvousId) -> Option<&RendezvousEntry<'cfg, T>> {
        let mut current = self.head;
        while let Some(entry_ptr) = current {
            let entry = /* SAFETY: `self.head` links only initialized
            `RendezvousEntry` nodes carved from rendezvous slabs; shared lookup
            does not activate a lease and follows the pinned `next` chain. */ unsafe {
                entry_ptr.as_ref()
            };
            if entry.id == *id {
                return Some(entry);
            }
            current = entry.next;
        }
        None
    }

    fn entry_mut(&mut self, id: &RendezvousId) -> Option<&mut RendezvousEntry<'cfg, T>> {
        let mut current = self.head;
        while let Some(mut entry_ptr) = current {
            let entry = /* SAFETY: `&mut self` is the registry mutation token.
            The selected entry pointer belongs to the pinned intrusive list, and
            no rendezvous lease is created until the caller marks the entry. */ unsafe {
                entry_ptr.as_mut()
            };
            if entry.id == *id {
                return Some(entry);
            }
            current = entry.next;
        }
        None
    }

    /// Borrow a rendezvous by shared reference when no lease is active.
    pub(crate) fn get(&self, id: &RendezvousId) -> Option<&Rendezvous<'cfg, 'cfg, T>> {
        self.entry_ref(id).and_then(|entry| entry.rendezvous_ref())
    }

    /// Borrow a rendezvous by mutable reference when no lease is active.
    pub(crate) fn get_mut(&mut self, id: &RendezvousId) -> Option<&mut Rendezvous<'cfg, 'cfg, T>> {
        self.entry_mut(id).and_then(|entry| entry.rendezvous_mut())
    }

    /// Borrow a rendezvous mutably, keeping the distinction between an
    /// absent rendezvous and an active affine lease.
    pub(crate) fn get_mut_checked(
        &mut self,
        id: &RendezvousId,
    ) -> Result<&mut Rendezvous<'cfg, 'cfg, T>, LeaseError> {
        let slot = self
            .entry_mut(id)
            .ok_or(LeaseError::RendezvousUnregistered(*id))?;
        if slot.is_active() {
            return Err(LeaseError::AlreadyLeased(*id));
        }
        Ok(slot.rendezvous())
    }

    /// Obtain a lease for the rendezvous identified by `rv_id`.
    pub(crate) fn lease<'lease>(
        &'lease mut self,
        rv_id: RendezvousId,
    ) -> Result<RendezvousLease<'lease, 'cfg, T>, LeaseError>
    where
        'cfg: 'lease,
    {
        let slot = self
            .entry_mut(&rv_id)
            .ok_or(LeaseError::RendezvousUnregistered(rv_id))?;
        if slot.is_active() {
            return Err(LeaseError::AlreadyLeased(rv_id));
        }
        slot.mark_active();
        Ok(RendezvousLease::new(slot))
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

/// Resident rendezvous slot used by [`RendezvousTable`].
struct RendezvousEntry<'cfg, T>
where
    T: Transport,
{
    id: RendezvousId,
    rendezvous: NonNull<Rendezvous<'cfg, 'cfg, T>>,
    state: RendezvousEntryState,
    resolver_bucket: crate::session::cluster::core::ResolverBucket<'cfg>,
    next: Option<NonNull<RendezvousEntry<'cfg, T>>>,
    _marker: PhantomData<&'cfg mut Rendezvous<'cfg, 'cfg, T>>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum RendezvousEntryState {
    Available,
    Leased,
}

impl<'cfg, T> RendezvousEntry<'cfg, T>
where
    T: Transport,
{
    fn is_active(&self) -> bool {
        self.state == RendezvousEntryState::Leased
    }

    fn mark_active(&mut self) {
        self.state = RendezvousEntryState::Leased;
    }

    fn clear_active(&mut self) {
        self.state = RendezvousEntryState::Available;
    }

    fn rendezvous_ref(&self) -> Option<&Rendezvous<'cfg, 'cfg, T>> {
        match self.state {
            RendezvousEntryState::Available => Some(
                /* SAFETY: an `Available` entry has no active
                `RendezvousLease`; this shared borrow is tied to the registry
                lookup and reads the pinned rendezvous pointer stored at insert. */
                unsafe { self.rendezvous.as_ref() },
            ),
            RendezvousEntryState::Leased => None,
        }
    }

    fn rendezvous_mut(&mut self) -> Option<&mut Rendezvous<'cfg, 'cfg, T>> {
        match self.state {
            RendezvousEntryState::Available => Some(
                /* SAFETY: `&mut self` is the unique registry entry borrow and
                `Available` proves no affine lease currently owns the pinned
                rendezvous pointer. */
                unsafe { self.rendezvous.as_mut() },
            ),
            RendezvousEntryState::Leased => None,
        }
    }

    fn rendezvous(&mut self) -> &mut Rendezvous<'cfg, 'cfg, T> {
        /* SAFETY: `RendezvousLease::new` reaches this after marking the entry
        `Leased`; the lease owns the only mutable path to this pinned rendezvous
        until its Drop clears the state. */
        unsafe { self.rendezvous.as_mut() }
    }
}

impl<'cfg, T> RendezvousEntry<'cfg, T>
where
    T: Transport,
{
    unsafe fn init_from_parts(
        dst: *mut Self,
        rv_id: RendezvousId,
        rendezvous: NonNull<Rendezvous<'cfg, 'cfg, T>>,
        next: Option<NonNull<RendezvousEntry<'cfg, T>>>,
    ) {
        /* SAFETY: registry insertion passes an unpublished `RendezvousEntry`
        sidecar cell. Each field is written once, the resolver bucket receives
        its own empty initialization, and the entry is linked from `self.head`
        only after this initializer returns. */
        unsafe {
            core::ptr::addr_of_mut!((*dst).id).write(rv_id);
            core::ptr::addr_of_mut!((*dst).rendezvous).write(rendezvous);
            core::ptr::addr_of_mut!((*dst).state).write(RendezvousEntryState::Available);
            crate::session::cluster::core::ResolverBucket::init_empty(core::ptr::addr_of_mut!(
                (*dst).resolver_bucket
            ));
            core::ptr::addr_of_mut!((*dst).next).write(next);
            core::ptr::addr_of_mut!((*dst)._marker).write(PhantomData);
        }
    }
}

impl<'cfg, T> Drop for RendezvousEntry<'cfg, T>
where
    T: Transport,
{
    fn drop(&mut self) {
        /* SAFETY: dropping the registry entry owns the stored rendezvous slab
        node. Active leases borrow the entry and therefore cannot overlap entry
        drop; the pointer is dropped exactly once with the entry. */
        unsafe {
            ptr::drop_in_place(self.rendezvous.as_ptr());
        }
    }
}

/// RAII lease over a rendezvous slot.
///
/// The lease is affine: it cannot be cloned, and dropping it automatically marks
/// the underlying rendezvous as available again.
pub(crate) struct RendezvousLease<'lease, 'cfg, T: Transport>
where
    'cfg: 'lease,
{
    slot: Option<&'lease mut RendezvousEntry<'cfg, T>>,
}

impl<'lease, 'cfg, T> RendezvousLease<'lease, 'cfg, T>
where
    T: Transport,
    'cfg: 'lease,
{
    fn new(slot: &'lease mut RendezvousEntry<'cfg, T>) -> Self {
        Self { slot: Some(slot) }
    }

    #[inline]
    fn entry_mut(&mut self) -> &mut RendezvousEntry<'cfg, T> {
        match self.slot.as_mut() {
            Some(slot) => slot,
            None => crate::invariant(),
        }
    }

    #[inline]
    pub(crate) fn with_rendezvous<R>(
        &mut self,
        f: impl FnOnce(&mut Rendezvous<'cfg, 'cfg, T>) -> R,
    ) -> R {
        let entry = self.entry_mut();
        f(entry.rendezvous())
    }
    #[inline]
    pub(crate) fn brand(&mut self) -> crate::session::brand::Guard<'cfg> {
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
        if let Some(slot) = self.slot.take() {
            slot.clear_active();
        }
    }
}
