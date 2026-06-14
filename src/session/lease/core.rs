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

use crate::rendezvous::core::{LaneRelease, Rendezvous};
use crate::session::types::{Lane, RendezvousId, SessionId};
use crate::{runtime_core::config::Clock, session::lease::map::ArrayMap, transport::Transport};

/// Fixed-size rendezvous table.
///
/// `RendezvousTable` is parameterised by the transport and clock used by the
/// rendezvous layer. The `MAX_RV` const parameter fixes the maximum number of
/// rendezvous that can be registered in `no_alloc` environments.
pub(crate) struct RendezvousTable<'cfg, T: Transport, C: Clock, const MAX_RV: usize> {
    entries: ArrayMap<RendezvousId, RendezvousEntry<'cfg, T, C>, MAX_RV>,
}

impl<'cfg, T, C, const MAX_RV: usize> RendezvousTable<'cfg, T, C, MAX_RV>
where
    T: Transport,
    C: Clock,
{
    /// Initialize an empty rendezvous table in place without constructing the full
    /// fixed-capacity storage on the caller's stack first.
    ///
    /// # Safety
    /// `dst` must point to valid, writable memory for `Self`.
    pub(crate) unsafe fn init_empty(dst: *mut Self) {
        /* SAFETY: the caller supplies exclusive uninitialized storage and this initializer writes all exposed fields before return. */
        unsafe {
            ArrayMap::init_empty(core::ptr::addr_of_mut!((*dst).entries));
        }
    }

    /// Borrow a rendezvous by shared reference when no lease is active.
    pub(crate) fn get(&self, id: &RendezvousId) -> Option<&Rendezvous<'cfg, 'cfg, T, C>> {
        self.entries
            .get(id)
            .and_then(|entry| entry.rendezvous_ref())
    }

    /// Borrow a rendezvous by mutable reference when no lease is active.
    pub(crate) fn get_mut(
        &mut self,
        id: &RendezvousId,
    ) -> Option<&mut Rendezvous<'cfg, 'cfg, T, C>> {
        self.entries
            .get_mut(id)
            .and_then(|entry| entry.rendezvous_mut())
    }

    /// Borrow a rendezvous mutably, keeping the distinction between an
    /// absent rendezvous and an active affine lease.
    pub(crate) fn get_mut_checked(
        &mut self,
        id: &RendezvousId,
    ) -> Result<&mut Rendezvous<'cfg, 'cfg, T, C>, LeaseError> {
        let slot = self
            .entries
            .get_mut(id)
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
    ) -> Result<RendezvousLease<'lease, 'cfg, T, C>, LeaseError>
    where
        'cfg: 'lease,
    {
        let slot = self
            .entries
            .get_mut(&rv_id)
            .ok_or(LeaseError::RendezvousUnregistered(rv_id))?;
        if slot.is_active() {
            return Err(LeaseError::AlreadyLeased(rv_id));
        }
        slot.mark_active();
        Ok(RendezvousLease::new(slot))
    }
}

impl<'cfg, T, C, const MAX_RV: usize> RendezvousTable<'cfg, T, C, MAX_RV>
where
    T: Transport,
    C: Clock,
{
    fn next_available_rendezvous_id(&self) -> Option<RendezvousId> {
        let mut raw = 1u16;
        loop {
            let id = RendezvousId::new(raw);
            if !self.entries.contains_key(&id) {
                return Some(id);
            }
            raw = raw.wrapping_add(1);
            if raw == 0 {
                return None;
            }
        }
    }

    pub(crate) fn register_local_from_config_auto(
        &mut self,
        config: crate::runtime_core::config::Config<'cfg, C>,
        transport: T,
    ) -> Result<RendezvousId, RegisterRendezvousError> {
        if self.entries.is_full() {
            return Err(RegisterRendezvousError::CapacityExceeded);
        }
        let id = self
            .next_available_rendezvous_id()
            .ok_or(RegisterRendezvousError::CapacityExceeded)?;
        // SAFETY: The key written before entry initialization is `RendezvousId: Copy`
        // and leaves no droppable state on failure. `init_from_config_auto`
        // returns `Err` before writing `RendezvousEntry` fields, or writes the
        // complete entry before returning `Ok(())`.
        /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
        unsafe {
            self.entries
                .try_push_with(RegisterRendezvousError::CapacityExceeded, |slot| {
                    let entry = slot.as_mut_ptr();
                    core::ptr::addr_of_mut!((*entry).0).write(id);
                    RendezvousEntry::init_from_config_auto(
                        core::ptr::addr_of_mut!((*entry).1),
                        id,
                        config,
                        transport,
                    )
                })?;
        }
        Ok(id)
    }
}

/// Failure modes for rendezvous registration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RegisterRendezvousError {
    /// Attempted to register more rendezvous than the fixed capacity allows.
    CapacityExceeded,
    /// Caller-provided slab cannot fit the rendezvous resident header.
    StorageExhausted,
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
struct RendezvousEntry<'cfg, T, C>
where
    T: Transport,
    C: Clock,
{
    rendezvous: NonNull<Rendezvous<'cfg, 'cfg, T, C>>,
    state: RendezvousEntryState,
    _marker: PhantomData<&'cfg mut Rendezvous<'cfg, 'cfg, T, C>>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum RendezvousEntryState {
    Available,
    Leased,
}

impl<'cfg, T, C> RendezvousEntry<'cfg, T, C>
where
    T: Transport,
    C: Clock,
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

    fn rendezvous_ref(&self) -> Option<&Rendezvous<'cfg, 'cfg, T, C>> {
        match self.state {
            RendezvousEntryState::Available => Some(
                /* SAFETY: the lease owner stores pinned rendezvous/tap/slab pointers and borrows them through one lease path at a time. */
                unsafe { self.rendezvous.as_ref() },
            ),
            RendezvousEntryState::Leased => None,
        }
    }

    fn rendezvous_mut(&mut self) -> Option<&mut Rendezvous<'cfg, 'cfg, T, C>> {
        match self.state {
            RendezvousEntryState::Available => Some(
                /* SAFETY: the lease owner stores pinned rendezvous/tap/slab pointers and borrows them through one lease path at a time. */
                unsafe { self.rendezvous.as_mut() },
            ),
            RendezvousEntryState::Leased => None,
        }
    }

    fn rendezvous(&mut self) -> &mut Rendezvous<'cfg, 'cfg, T, C> {
        /* SAFETY: the lease owner stores pinned rendezvous/tap/slab pointers and borrows them through one lease path at a time. */
        unsafe { self.rendezvous.as_mut() }
    }
}

impl<'cfg, T, C> RendezvousEntry<'cfg, T, C>
where
    T: Transport,
    C: Clock,
{
    unsafe fn init_from_config_auto(
        dst: *mut Self,
        rv_id: RendezvousId,
        config: crate::runtime_core::config::Config<'cfg, C>,
        transport: T,
    ) -> Result<(), RegisterRendezvousError> {
        let rendezvous = /* SAFETY: the lease owner stores pinned rendezvous/tap/slab pointers and borrows them through one lease path at a time. */ unsafe {
            Rendezvous::init_in_slab_auto(rv_id, config, transport)
                .ok_or(RegisterRendezvousError::StorageExhausted)?
        };
        /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
        unsafe {
            core::ptr::addr_of_mut!((*dst).rendezvous).write(NonNull::new_unchecked(rendezvous));
            core::ptr::addr_of_mut!((*dst).state).write(RendezvousEntryState::Available);
            core::ptr::addr_of_mut!((*dst)._marker).write(PhantomData);
        }
        Ok(())
    }
}

impl<'cfg, T, C> Drop for RendezvousEntry<'cfg, T, C>
where
    T: Transport,
    C: Clock,
{
    fn drop(&mut self) {
        /* SAFETY: the lease owner stores pinned rendezvous/tap/slab pointers and borrows them through one lease path at a time. */
        unsafe {
            ptr::drop_in_place(self.rendezvous.as_ptr());
        }
    }
}

/// RAII lease over a rendezvous slot.
///
/// The lease is affine: it cannot be cloned, and dropping it automatically marks
/// the underlying rendezvous as available again.
pub(crate) struct RendezvousLease<'lease, 'cfg, T: Transport, C: Clock>
where
    'cfg: 'lease,
{
    slot: Option<&'lease mut RendezvousEntry<'cfg, T, C>>,
}

impl<'lease, 'cfg, T, C> RendezvousLease<'lease, 'cfg, T, C>
where
    T: Transport,
    C: Clock,
    'cfg: 'lease,
{
    fn new(slot: &'lease mut RendezvousEntry<'cfg, T, C>) -> Self {
        Self { slot: Some(slot) }
    }

    #[inline]
    fn entry_mut(&mut self) -> &mut RendezvousEntry<'cfg, T, C> {
        match self.slot.as_mut() {
            Some(slot) => slot,
            None => crate::invariant(),
        }
    }

    #[inline]
    pub(crate) fn with_rendezvous<R>(
        &mut self,
        f: impl FnOnce(&mut Rendezvous<'cfg, 'cfg, T, C>) -> R,
    ) -> R {
        let entry = self.entry_mut();
        f(entry.rendezvous())
    }
    #[inline]
    pub(crate) fn brand(&mut self) -> crate::session::brand::Guard<'cfg> {
        self.with_rendezvous(|rv| rv.brand())
    }

    #[inline]
    pub(crate) fn emit_lane_acquire(
        &mut self,
        rv_id: crate::session::types::RendezvousId,
        sid: SessionId,
        lane: Lane,
    ) {
        self.with_rendezvous(|rv| {
            crate::observe::core::emit(
                rv.tap(),
                crate::observe::events::lane_acquire(
                    rv.now32(),
                    rv_id.raw() as u32,
                    sid.raw(),
                    lane.raw() as u16,
                ),
            );
        });
    }

    #[inline]
    pub(crate) fn release_lane_with_tap(&mut self, lane: Lane) {
        self.with_rendezvous(|rv| match rv.release_lane(lane) {
            LaneRelease::Released(sid) => {
                rv.emit_lane_release(sid, lane);
            }
            LaneRelease::StillHeld => {}
        });
    }
}

impl<'lease, 'cfg, T, C> Drop for RendezvousLease<'lease, 'cfg, T, C>
where
    T: Transport,
    C: Clock,
    'cfg: 'lease,
{
    fn drop(&mut self) {
        if let Some(slot) = self.slot.take() {
            slot.clear_active();
        }
    }
}
