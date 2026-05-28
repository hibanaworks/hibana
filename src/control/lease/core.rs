//! Lease-first control core.
//!
//! This module replaces ad-hoc interior mutability with an explicit, RAII-based
//! leasing API. `ControlCore::lease::<Spec>()` is the single entry point for
//! touching rendezvous state; everything else must be expressed as a typed
//! automaton that consumes the lease.
//!
//! The design goals are:
//! - **No hidden mutable access** — leases carry unique borrows, eliminating
//!   `UnsafeCell` gymnastics and raw pointers.
//! - **Facet-driven typing** — the lease exposes only the facets declared by
//!   the `RendezvousSpec`. Unsupported operations are a compile-time error.
//! - **Affine lifecycle** — leases release themselves on drop, and cannot be
//!   cloned or duplicated.
//! - **Const-friendly control** — the control layer stays allocation free and
//!   is ready for `no_std`.

use core::{marker::PhantomData, ptr, ptr::NonNull};

#[cfg(test)]
use crate::control::lease::graph::{LeaseGraph, LeaseSpec};
use crate::control::types::{Lane, RendezvousId, SessionId};
use crate::rendezvous::core::Rendezvous;
use crate::{
    control::lease::map::ArrayMap,
    runtime::{config::Clock, consts::LabelUniverse},
    transport::Transport,
};

/// Slot proof for a registered local rendezvous owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RendezvousOwnerProof {
    id: RendezvousId,
    slot: u16,
}

impl RendezvousOwnerProof {
    #[inline]
    const fn new(id: RendezvousId, slot: usize) -> Self {
        Self {
            id,
            slot: slot as u16,
        }
    }

    #[inline]
    pub(crate) const fn id(self) -> RendezvousId {
        self.id
    }

    #[inline]
    const fn slot(self) -> usize {
        self.slot as usize
    }
}

/// Fixed-size control core that owns rendezvous instances.
///
/// `ControlCore` is parameterised by the transport, label universe, clock and
/// epoch table used by the rendezvous layer. The `MAX_RV` const parameter fixes
/// the maximum number of rendezvous that can be registered in `no_alloc`
/// environments.
pub(crate) struct ControlCore<
    'cfg,
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
    const MAX_RV: usize,
> {
    entries: ArrayMap<RendezvousId, RendezvousEntry<'cfg, T, U, C, E>, MAX_RV>,
}

impl<'cfg, T, U, C, E, const MAX_RV: usize> Default for ControlCore<'cfg, T, U, C, E, MAX_RV>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<'cfg, T, U, C, E, const MAX_RV: usize> ControlCore<'cfg, T, U, C, E, MAX_RV>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
{
    /// Construct an empty control core.
    pub(crate) const fn new() -> Self {
        Self {
            entries: ArrayMap::new(),
        }
    }

    /// Initialize an empty control core in place without constructing the full
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

    /// Returns true if the rendezvous identifier is present, regardless of activity.
    #[cfg(test)]
    pub(crate) fn is_registered(&self, id: &RendezvousId) -> bool {
        self.entries.contains_key(id)
    }

    /// Borrow a rendezvous by shared reference when no lease is active.
    pub(crate) fn get(&self, id: &RendezvousId) -> Option<&Rendezvous<'cfg, 'cfg, T, U, C, E>> {
        self.entries
            .get(id)
            .and_then(|entry| entry.rendezvous_ref())
    }

    /// Borrow a rendezvous by mutable reference when no lease is active.
    pub(crate) fn get_mut(
        &mut self,
        id: &RendezvousId,
    ) -> Option<&mut Rendezvous<'cfg, 'cfg, T, U, C, E>> {
        self.entries
            .get_mut(id)
            .and_then(|entry| entry.rendezvous_mut())
    }

    /// Mint a compact proof for a registered local rendezvous owner.
    pub(crate) fn owner_proof(&self, id: RendezvousId) -> Result<RendezvousOwnerProof, LeaseError> {
        self.entries
            .index_of(&id)
            .map(|slot| RendezvousOwnerProof::new(id, slot))
            .ok_or(LeaseError::UnknownRendezvous(id))
    }

    /// Borrow the rendezvous identified by an owner proof.
    pub(crate) fn get_mut_by_proof(
        &mut self,
        proof: RendezvousOwnerProof,
    ) -> &mut Rendezvous<'cfg, 'cfg, T, U, C, E> {
        let (id, entry) = self
            .entries
            .get_index_mut(proof.slot())
            .expect("local rendezvous owner proof points outside registered storage");
        assert_eq!(
            *id,
            proof.id(),
            "local rendezvous owner proof slot changed owner"
        );
        entry
            .rendezvous_mut()
            .expect("local rendezvous owner proof points to an active lease")
    }

    /// Borrow two distinct rendezvous identified by owner proofs.
    pub(crate) fn get_pair_mut_by_proof(
        &mut self,
        left: RendezvousOwnerProof,
        right: RendezvousOwnerProof,
    ) -> (
        &mut Rendezvous<'cfg, 'cfg, T, U, C, E>,
        &mut Rendezvous<'cfg, 'cfg, T, U, C, E>,
    ) {
        assert_ne!(
            left.id(),
            right.id(),
            "topology commit local owner proofs must be distinct"
        );
        let ((left_id, left_entry), (right_id, right_entry)) = self
            .entries
            .get_pair_index_mut(left.slot(), right.slot())
            .expect("local rendezvous owner proofs point outside registered storage");
        assert_eq!(
            *left_id,
            left.id(),
            "left local rendezvous owner proof slot changed owner"
        );
        assert_eq!(
            *right_id,
            right.id(),
            "right local rendezvous owner proof slot changed owner"
        );
        let left = left_entry
            .rendezvous_mut()
            .expect("left local rendezvous owner proof points to an active lease");
        let right = right_entry
            .rendezvous_mut()
            .expect("right local rendezvous owner proof points to an active lease");
        (left, right)
    }

    /// Borrow a rendezvous mutably, preserving the distinction between an
    /// absent rendezvous and an active affine lease.
    pub(crate) fn get_mut_checked(
        &mut self,
        id: &RendezvousId,
    ) -> Result<&mut Rendezvous<'cfg, 'cfg, T, U, C, E>, LeaseError> {
        let slot = self
            .entries
            .get_mut(id)
            .ok_or(LeaseError::UnknownRendezvous(*id))?;
        if slot.is_active() {
            return Err(LeaseError::AlreadyLeased(*id));
        }
        Ok(slot.rendezvous())
    }

    /// Obtain a lease for the rendezvous identified by `rv_id`.
    ///
    /// The lease carries a type parameter `Spec` that determines which facets
    /// of the rendezvous state may be accessed.
    pub(crate) fn lease<'lease, Spec>(
        &'lease mut self,
        rv_id: RendezvousId,
    ) -> Result<RendezvousLease<'lease, 'cfg, T, U, C, E, Spec>, LeaseError>
    where
        Spec: RendezvousSpec<T, U, C, E>,
        'cfg: 'lease,
    {
        let slot = self
            .entries
            .get_mut(&rv_id)
            .ok_or(LeaseError::UnknownRendezvous(rv_id))?;
        if slot.is_active() {
            return Err(LeaseError::AlreadyLeased(rv_id));
        }
        slot.mark_active();
        Ok(RendezvousLease::new(slot))
    }
}

impl<'cfg, T, U, C, const MAX_RV: usize>
    ControlCore<'cfg, T, U, C, crate::control::cap::mint::EpochTbl, MAX_RV>
where
    T: Transport,
    U: LabelUniverse,
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
        config: crate::runtime::config::Config<'cfg, U, C>,
        transport: T,
    ) -> Result<RendezvousId, RegisterRendezvousError> {
        if self.entries.is_full() {
            return Err(RegisterRendezvousError::CapacityExceeded);
        }
        let id = self
            .next_available_rendezvous_id()
            .ok_or(RegisterRendezvousError::CapacityExceeded)?;
        // SAFETY: The key written before delegation is `RendezvousId: Copy`
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
    /// Borrowed runtime storage cannot fit the rendezvous resident header.
    StorageExhausted,
}

/// Leasing failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LeaseError {
    /// No rendezvous with the requested identifier exists.
    UnknownRendezvous(RendezvousId),
    /// The rendezvous is currently leased and cannot be borrowed again.
    AlreadyLeased(RendezvousId),
}

/// Internal rendezvous slot used by [`ControlCore`].
struct RendezvousEntry<'cfg, T, U, C, E>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
{
    rendezvous: NonNull<Rendezvous<'cfg, 'cfg, T, U, C, E>>,
    active: bool,
    _marker: PhantomData<&'cfg mut Rendezvous<'cfg, 'cfg, T, U, C, E>>,
}

impl<'cfg, T, U, C, E> RendezvousEntry<'cfg, T, U, C, E>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
{
    fn is_active(&self) -> bool {
        self.active
    }

    fn mark_active(&mut self) {
        self.active = true;
    }

    fn clear_active(&mut self) {
        self.active = false;
    }

    fn rendezvous_ref(&self) -> Option<&Rendezvous<'cfg, 'cfg, T, U, C, E>> {
        if self.active {
            None
        } else {
            Some(
                /* SAFETY: the lease owner stores pinned rendezvous/tap/slab pointers and borrows them through one lease path at a time. */
                unsafe { self.rendezvous.as_ref() },
            )
        }
    }

    fn rendezvous_mut(&mut self) -> Option<&mut Rendezvous<'cfg, 'cfg, T, U, C, E>> {
        if self.active {
            None
        } else {
            Some(
                /* SAFETY: the lease owner stores pinned rendezvous/tap/slab pointers and borrows them through one lease path at a time. */
                unsafe { self.rendezvous.as_mut() },
            )
        }
    }

    fn rendezvous(&mut self) -> &mut Rendezvous<'cfg, 'cfg, T, U, C, E> {
        /* SAFETY: the lease owner stores pinned rendezvous/tap/slab pointers and borrows them through one lease path at a time. */
        unsafe { self.rendezvous.as_mut() }
    }
}

impl<'cfg, T, U, C> RendezvousEntry<'cfg, T, U, C, crate::control::cap::mint::EpochTbl>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
{
    unsafe fn init_from_config_auto(
        dst: *mut Self,
        rv_id: RendezvousId,
        config: crate::runtime::config::Config<'cfg, U, C>,
        transport: T,
    ) -> Result<(), RegisterRendezvousError> {
        let rendezvous = /* SAFETY: the lease owner stores pinned rendezvous/tap/slab pointers and borrows them through one lease path at a time. */ unsafe {
            Rendezvous::init_in_slab_auto(rv_id, config, transport)
                .ok_or(RegisterRendezvousError::StorageExhausted)?
        };
        /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
        unsafe {
            core::ptr::addr_of_mut!((*dst).rendezvous).write(NonNull::new_unchecked(rendezvous));
            core::ptr::addr_of_mut!((*dst).active).write(false);
            core::ptr::addr_of_mut!((*dst)._marker).write(PhantomData);
        }
        Ok(())
    }
}

impl<'cfg, T, U, C, E> Drop for RendezvousEntry<'cfg, T, U, C, E>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
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
/// the underlying rendezvous as available again. Access to rendezvous facets is
/// mediated through the `Spec` type parameter.
pub(crate) struct RendezvousLease<
    'lease,
    'cfg,
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
    Spec,
> where
    Spec: RendezvousSpec<T, U, C, E>,
    'cfg: 'lease,
{
    slot: Option<&'lease mut RendezvousEntry<'cfg, T, U, C, E>>,
    _spec: PhantomData<Spec>,
}

impl<'lease, 'cfg, T, U, C, E, Spec> RendezvousLease<'lease, 'cfg, T, U, C, E, Spec>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
    Spec: RendezvousSpec<T, U, C, E>,
    'cfg: 'lease,
{
    fn new(slot: &'lease mut RendezvousEntry<'cfg, T, U, C, E>) -> Self {
        Self {
            slot: Some(slot),
            _spec: PhantomData,
        }
    }

    #[inline]
    fn entry_mut(&mut self) -> &mut RendezvousEntry<'cfg, T, U, C, E> {
        self.slot
            .as_mut()
            .expect("rendezvous lease has already been consumed")
    }

    #[inline]
    pub(crate) fn with_rendezvous<R>(
        &mut self,
        f: impl FnOnce(&mut Rendezvous<'cfg, 'cfg, T, U, C, E>) -> R,
    ) -> R {
        let entry = self.entry_mut();
        f(entry.rendezvous())
    }
}

impl<'lease, 'cfg, T, U, C, E> RendezvousLease<'lease, 'cfg, T, U, C, E, FullSpec>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
    'cfg: 'lease,
{
    #[inline]
    pub(crate) fn brand(&mut self) -> crate::control::brand::Guard<'cfg> {
        self.with_rendezvous(|rv| rv.brand())
    }

    #[inline]
    pub(crate) fn emit_lane_acquire(
        &mut self,
        rv_id: crate::control::types::RendezvousId,
        sid: SessionId,
        lane: Lane,
    ) {
        self.with_rendezvous(|rv| {
            crate::observe::core::emit(
                rv.tap(),
                crate::observe::events::LaneAcquire::new(
                    rv.now32(),
                    rv_id.raw() as u32,
                    sid.raw(),
                    lane.raw() as u16,
                ),
            );
        });
    }

    #[inline]
    pub(crate) fn release_lane_with_tap(&mut self, lane: Lane) -> bool {
        self.with_rendezvous(|rv| {
            if let Some(sid) = rv.release_lane(lane) {
                rv.emit_lane_release(sid, lane);
                true
            } else {
                false
            }
        })
    }
}

impl<'lease, 'cfg, T, U, C, E, Spec> Drop for RendezvousLease<'lease, 'cfg, T, U, C, E, Spec>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
    Spec: RendezvousSpec<T, U, C, E>,
    'cfg: 'lease,
{
    fn drop(&mut self) {
        if let Some(slot) = self.slot.take() {
            slot.clear_active();
        }
    }
}

/// Trait implemented by rendezvous lease specifications.
///
/// A spec declares a set of facets accessible through a lease. Simple specs may
/// return a mutable reference to the rendezvous itself, while more focused specs
/// can expose narrow capability objects.
pub(crate) trait RendezvousSpec<T, U, C, E>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
{
}

/// Default spec exposing full mutable access to the rendezvous.
pub(crate) struct FullSpec;

/// Spec that exposes only topology operations.
#[cfg(test)]
pub(crate) struct TopologySpec;

#[cfg(test)]
impl<T, U, C, E> RendezvousSpec<T, U, C, E> for TopologySpec
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
{
}

impl<T, U, C, E> RendezvousSpec<T, U, C, E> for FullSpec
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
{
}

/// Control automaton executed against a rendezvous lease.
#[cfg(test)]
pub(crate) trait ControlAutomaton<T, U, C, E>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
{
    /// Lease specialisation required by the automaton.
    type Spec: RendezvousSpec<T, U, C, E>;
    /// Initial input value.
    type Seed;
    /// Result produced on success.
    type Output;
    /// Error reported on failure.
    type Error;
    /// LeaseGraph specialisation used when the automaton requires cross-rendezvous
    /// coordination. Automatons that do not depend on additional graph state can
    /// provide a degenerate spec.
    type GraphSpec: LeaseSpec;

    /// Execute the automaton using a LeaseGraph for ownership tracking.
    fn run_with_graph<'lease, 'cfg, 'graph>(
        graph: &'graph mut LeaseGraph<'graph, Self::GraphSpec>,
        lease: &mut RendezvousLease<'lease, 'cfg, T, U, C, E, Self::Spec>,
        seed: Self::Seed,
    ) -> ControlStep<Self::Output, Self::Error>
    where
        'cfg: 'lease;
}

/// Result of running a control automaton step.
#[cfg(test)]
pub(crate) enum ControlStep<O, E> {
    /// Automaton finished successfully.
    Complete(O),
    /// Automaton failed.
    Abort(E),
}

#[cfg(test)]
mod automaton_tests {
    use super::*;
    use crate::control::lease::graph::LeaseFacet;
    use crate::control::types::RendezvousId;
    use core::mem::MaybeUninit;

    #[derive(Clone, Copy, Default)]
    struct TestFacet;

    #[derive(Clone, Copy)]
    struct TestContext {
        value: u32,
    }

    impl LeaseFacet for TestFacet {
        type Context<'ctx> = TestContext;

        fn on_commit<'ctx>(&self, _context: &mut Self::Context<'ctx>) {}

        fn on_rollback<'ctx>(&self, _context: &mut Self::Context<'ctx>) {}
    }

    struct RvLeaseSpec;
    impl LeaseSpec for RvLeaseSpec {
        type NodeId = RendezvousId;
        type Facet = TestFacet;
        type ChildStorage = crate::control::lease::graph::InlineLeaseChildStorage<RendezvousId, 3>;
        type NodeStorage<'graph>
            = crate::control::lease::graph::InlineLeaseNodeStorage<'graph, Self, 4>
        where
            Self: 'graph;
        const MAX_NODES: usize = 4;
        const MAX_CHILDREN: usize = 3;
    }

    #[test]
    fn test_lease_graph_operations() {
        let root_id = RendezvousId::new(1);
        let child_id = RendezvousId::new(2);

        let mut graph_storage = MaybeUninit::<LeaseGraph<'_, RvLeaseSpec>>::uninit();
        let mut graph = /* SAFETY: the caller supplies exclusive uninitialized storage and this initializer writes all exposed fields before return. */ unsafe {
            LeaseGraph::<RvLeaseSpec>::init_new(
                graph_storage.as_mut_ptr(),
                root_id,
                TestFacet,
                TestContext { value: 10 },
            );
            graph_storage.assume_init()
        };
        graph
            .add_child(root_id, child_id, TestFacet, TestContext { value: 20 })
            .unwrap();

        let sum = graph.handle_mut(root_id).unwrap().with(|_, ctx| ctx.value)
            + graph.handle_mut(child_id).unwrap().with(|_, ctx| ctx.value);
        assert_eq!(sum, 30);

        graph.commit();
    }
}
