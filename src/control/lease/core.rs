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

use crate::control::types::{Lane, RendezvousId, SessionId};
use crate::rendezvous::core::Rendezvous;
use crate::{
    control::lease::map::ArrayMap,
    runtime::{config::Clock, consts::LabelUniverse},
    transport::Transport,
};

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
        unsafe {
            ArrayMap::init_empty(core::ptr::addr_of_mut!((*dst).entries));
        }
    }

    /// Returns true if the rendezvous identifier is present, regardless of activity.
    pub(crate) fn is_registered(&self, id: &RendezvousId) -> bool {
        self.entries.contains_key(id)
    }

    /// Returns true if the rendezvous is currently leased.
    pub(crate) fn is_active(&self, id: &RendezvousId) -> bool {
        self.entries
            .get(id)
            .map(|entry| entry.is_active())
            .unwrap_or(false)
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

    /// Register a local rendezvous by constructing it directly inside the
    /// fixed-capacity owner slot instead of materialising a large temporary on
    /// the caller stack first.
    #[cfg(test)]
    pub(crate) fn register_local_from_config(
        &mut self,
        config: crate::runtime::config::Config<'cfg, U, C>,
        transport: T,
        endpoint_slots: usize,
    ) -> Result<RendezvousId, RegisterRendezvousError> {
        if self.entries.is_full() {
            return Err(RegisterRendezvousError::CapacityExceeded);
        }
        let id = self
            .next_available_rendezvous_id()
            .ok_or(RegisterRendezvousError::CapacityExceeded)?;

        self.entries.try_push_with(|slot| unsafe {
            let entry = slot.as_mut_ptr();
            core::ptr::addr_of_mut!((*entry).0).write(id);
            RendezvousEntry::init_from_config(
                core::ptr::addr_of_mut!((*entry).1),
                id,
                config,
                transport,
                endpoint_slots,
            )
        })?;
        Ok(id)
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
        self.entries.try_push_with(|slot| unsafe {
            let entry = slot.as_mut_ptr();
            core::ptr::addr_of_mut!((*entry).0).write(id);
            RendezvousEntry::init_from_config_auto(
                core::ptr::addr_of_mut!((*entry).1),
                id,
                config,
                transport,
            )
        })?;
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
            Some(unsafe { self.rendezvous.as_ref() })
        }
    }

    fn rendezvous_mut(&mut self) -> Option<&mut Rendezvous<'cfg, 'cfg, T, U, C, E>> {
        if self.active {
            None
        } else {
            Some(unsafe { self.rendezvous.as_mut() })
        }
    }

    fn rendezvous(&mut self) -> &mut Rendezvous<'cfg, 'cfg, T, U, C, E> {
        unsafe { self.rendezvous.as_mut() }
    }
}

impl<'cfg, T, U, C> RendezvousEntry<'cfg, T, U, C, crate::control::cap::mint::EpochTbl>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
{
    #[cfg(test)]
    unsafe fn init_from_config(
        dst: *mut Self,
        rv_id: RendezvousId,
        config: crate::runtime::config::Config<'cfg, U, C>,
        transport: T,
        endpoint_slots: usize,
    ) -> Result<(), RegisterRendezvousError> {
        let rendezvous = unsafe {
            Rendezvous::init_in_slab(rv_id, config, transport, endpoint_slots)
                .ok_or(RegisterRendezvousError::StorageExhausted)?
        };
        unsafe {
            core::ptr::addr_of_mut!((*dst).rendezvous).write(NonNull::new_unchecked(rendezvous));
            core::ptr::addr_of_mut!((*dst).active).write(false);
            core::ptr::addr_of_mut!((*dst)._marker).write(PhantomData);
        }
        Ok(())
    }

    unsafe fn init_from_config_auto(
        dst: *mut Self,
        rv_id: RendezvousId,
        config: crate::runtime::config::Config<'cfg, U, C>,
        transport: T,
    ) -> Result<(), RegisterRendezvousError> {
        let rendezvous = unsafe {
            Rendezvous::init_in_slab_auto(rv_id, config, transport)
                .ok_or(RegisterRendezvousError::StorageExhausted)?
        };
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

    /// Obtain an observation lease for the underlying rendezvous.
    pub(crate) fn observe(&mut self) -> LeaseObserve<'_, 'cfg> {
        let tap = self.with_rendezvous(|rv| rv.tap() as *const crate::observe::core::TapRing<'cfg>);
        LeaseObserve::new(tap)
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
        timestamp: u32,
        rv_id: crate::control::types::RendezvousId,
        sid: SessionId,
        lane: Lane,
    ) {
        let observe = self.observe();
        observe.emit(crate::observe::events::LaneAcquire::new(
            timestamp,
            rv_id.raw() as u32,
            sid.raw(),
            lane.raw() as u16,
        ));
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

/// Spec that exposes only splice operations.
pub(crate) struct SpliceSpec;

/// Spec that exposes only delegation operations.
pub(crate) struct DelegationSpec;

impl<T, U, C, E> RendezvousSpec<T, U, C, E> for SpliceSpec
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
{
}

impl<T, U, C, E> RendezvousSpec<T, U, C, E> for DelegationSpec
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
{
}

/// Lease-backed access to rendezvous observation events.
#[derive(Clone, Copy)]
pub(crate) struct LeaseObserve<'lease, 'cfg> {
    tap: *const crate::observe::core::TapRing<'cfg>,
    _marker: PhantomData<&'lease crate::observe::core::TapRing<'cfg>>,
}

impl<'lease, 'cfg> LeaseObserve<'lease, 'cfg> {
    #[inline]
    pub(crate) const fn new(tap: *const crate::observe::core::TapRing<'cfg>) -> Self {
        Self {
            tap,
            _marker: PhantomData,
        }
    }

    #[inline]
    fn ring(&self) -> &crate::observe::core::TapRing<'cfg> {
        unsafe { &*self.tap }
    }

    /// Emit an already constructed tap event.
    #[inline]
    pub(crate) fn emit(&self, event: crate::observe::core::TapEvent) {
        crate::observe::core::emit(self.ring(), event);
    }
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

    /// Execute the automaton against the provided lease without additional
    /// LeaseGraph context.
    fn run<'lease, 'cfg>(
        lease: &mut RendezvousLease<'lease, 'cfg, T, U, C, E, Self::Spec>,
        seed: Self::Seed,
    ) -> ControlStep<Self::Output, Self::Error>
    where
        'cfg: 'lease;

    /// Execute the automaton using a LeaseGraph for ownership tracking. By
    /// default this forwards to [`ControlAutomaton::run`]; automatons that rely
    /// on LeaseGraph should override this method.
    fn run_with_graph<'lease, 'cfg, 'graph>(
        graph: &'graph mut LeaseGraph<'graph, Self::GraphSpec>,
        lease: &mut RendezvousLease<'lease, 'cfg, T, U, C, E, Self::Spec>,
        seed: Self::Seed,
    ) -> ControlStep<Self::Output, Self::Error>
    where
        'cfg: 'lease,
    {
        let _ = graph;
        Self::run(lease, seed)
    }
}

/// Result of running a control automaton step.
pub(crate) enum ControlStep<O, E> {
    /// Automaton finished successfully.
    Complete(O),
    /// Automaton failed.
    Abort(E),
}

use crate::control::lease::graph::{LeaseGraph, LeaseGraphError, LeaseSpec};

/// Error when running a LeaseGraph-enabled automaton.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DelegationDriveError<E> {
    /// Failed to obtain a rendezvous lease.
    Lease(LeaseError),
    /// LeaseGraph operation failed.
    Graph(LeaseGraphError),
    /// The automaton aborted.
    Automaton(E),
}

#[cfg(test)]
mod automaton_tests {
    use super::*;
    use crate::control::lease::graph::LeaseFacet;
    use crate::control::types::RendezvousId;

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

        let mut graph =
            LeaseGraph::<RvLeaseSpec>::new(root_id, TestFacet, TestContext { value: 10 });
        graph
            .add_child(root_id, child_id, TestFacet, TestContext { value: 20 })
            .unwrap();

        let sum = graph.handle_mut(root_id).unwrap().with(|_, ctx| ctx.value)
            + graph.handle_mut(child_id).unwrap().with(|_, ctx| ctx.value);
        assert_eq!(sum, 30);

        graph.commit();
    }
}
