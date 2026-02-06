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

use core::{marker::PhantomData, mem::ManuallyDrop};

use crate::{
    control::lease::map::ArrayMap,
    rendezvous::{Rendezvous, RendezvousId},
    runtime::{config::Clock, consts::LabelUniverse},
    transport::Transport,
};

/// Fixed-size control core that owns rendezvous instances.
///
/// `ControlCore` is parameterised by the transport, label universe, clock and
/// epoch table used by the rendezvous layer. The `MAX_RV` const parameter fixes
/// the maximum number of rendezvous that can be registered in `no_alloc`
/// environments.
pub struct ControlCore<
    'cfg,
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::EpochTable,
    const MAX_RV: usize,
> {
    entries: ArrayMap<RendezvousId, RendezvousEntry<'cfg, T, U, C, E>, MAX_RV>,
}

impl<'cfg, T, U, C, E, const MAX_RV: usize> Default for ControlCore<'cfg, T, U, C, E, MAX_RV>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::EpochTable,
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
    E: crate::control::cap::EpochTable,
{
    /// Construct an empty control core.
    pub const fn new() -> Self {
        Self {
            entries: ArrayMap::new(),
        }
    }

    /// Register a local rendezvous with the core.
    ///
    /// The rendezvous is stored by value, preserving the strict drop-order:
    /// ControlCore → Rendezvous → in-flight leases.
    pub fn register_local(
        &mut self,
        rendezvous: Rendezvous<'cfg, 'cfg, T, U, C, E>,
    ) -> Result<RendezvousId, RegisterRendezvousError> {
        let id = rendezvous.id();
        if self.entries.contains_key(&id) {
            return Err(RegisterRendezvousError::Duplicate(id));
        }
        if self.entries.is_full() {
            return Err(RegisterRendezvousError::CapacityExceeded);
        }
        let entry = RendezvousEntry::new(rendezvous);
        self.entries.insert(id, entry).map_err(|entry| {
            let _ = entry;
            RegisterRendezvousError::CapacityExceeded
        })?;
        Ok(id)
    }

    /// Returns true if the rendezvous is registered and currently idle.
    pub fn contains(&self, id: &RendezvousId) -> bool {
        self.entries
            .get(id)
            .map(|entry| !entry.is_active())
            .unwrap_or(false)
    }

    /// Returns true if the rendezvous identifier is present, regardless of activity.
    pub fn is_registered(&self, id: &RendezvousId) -> bool {
        self.entries.contains_key(id)
    }

    /// Returns true if the rendezvous is currently leased.
    pub fn is_active(&self, id: &RendezvousId) -> bool {
        self.entries
            .get(id)
            .map(|entry| entry.is_active())
            .unwrap_or(false)
    }

    /// Borrow a rendezvous by shared reference when no lease is active.
    pub fn get(&self, id: &RendezvousId) -> Option<&Rendezvous<'cfg, 'cfg, T, U, C, E>> {
        self.entries
            .get(id)
            .and_then(|entry| entry.rendezvous_ref())
    }

    /// Borrow a rendezvous by mutable reference when no lease is active.
    pub fn get_mut(
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
    pub fn lease<'lease, Spec>(
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

    /// Number of registered rendezvous instances.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True when no rendezvous instances are registered.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Drive a control automaton against a rendezvous.
    ///
    /// This is a convenience method that obtains a lease and runs the automaton.
    ///
    /// ## Example
    ///
    /// ```ignore
    /// let result = core.drive::<MyAutomaton>(rv_id, seed)?;
    /// match result {
    ///     ControlStep::Complete(output) => { /* success */ }
    ///     ControlStep::Abort(error) => { /* failure */ }
    /// }
    /// ```
    #[allow(clippy::type_complexity)]
    pub fn drive<A>(
        &mut self,
        rv_id: RendezvousId,
        seed: A::Seed,
    ) -> Result<ControlStep<A::Output, A::Error>, DriveError<A::Error>>
    where
        A: ControlAutomaton<T, U, C, E>,
    {
        let mut lease = self.lease::<A::Spec>(rv_id).map_err(DriveError::Lease)?;
        Ok(A::run(&mut lease, seed))
    }

    /// Drive a delegation automaton with a LeaseGraph.
    ///
    /// This method obtains a lease for the root rendezvous and runs the automaton
    /// with the provided LeaseGraph managing child rendezvous.
    ///
    /// ## Example
    ///
    /// ```ignore
    /// let mut graph = LeaseGraph::new(root_id, Facet::default(), root_context);
    /// graph.add_child(root_id, child_id, Facet::default(), child_context)?;
    ///
    /// let result = core.drive_with_graph::<SpliceAutomaton, _>(
    ///     &mut graph,
    ///     root_id,
    ///     splice_intent,
    /// )?;
    ///
    /// match result {
    ///     ControlStep::Complete(ack) => { /* success */ }
    ///     ControlStep::Abort(err) => { /* automaton failed */ }
    /// }
    ///
    /// // Commit or rollback the graph
    /// graph.commit();
    /// ```
    ///
    /// ## Error Handling
    ///
    /// Returns `DelegationDriveError::Lease` if the lease cannot be obtained.
    /// Returns `DelegationDriveError::Graph` if LeaseGraph operations fail (future use).
    /// The automaton's own errors are returned via `ControlStep::Abort`.
    #[allow(clippy::type_complexity)]
    pub fn drive_with_graph<'graph, A>(
        &mut self,
        graph: &'graph mut LeaseGraph<'graph, A::GraphSpec>,
        root_id: RendezvousId,
        seed: A::Seed,
    ) -> Result<ControlStep<A::Output, A::Error>, DelegationDriveError<A::Error>>
    where
        A: ControlAutomaton<T, U, C, E>,
        A::GraphSpec: LeaseSpec,
    {
        let mut lease = self
            .lease::<A::Spec>(root_id)
            .map_err(DelegationDriveError::Lease)?;
        Ok(A::run_with_graph(graph, &mut lease, seed))
    }

    /// Visit all rendezvous that are currently not leased.
    pub fn for_each_available<F>(&self, mut f: F)
    where
        F: FnMut(RendezvousId, &Rendezvous<'cfg, 'cfg, T, U, C, E>),
    {
        self.entries.for_each(|id, entry| {
            if let Some(rv) = entry.rendezvous_ref() {
                f(*id, rv);
            }
        });
    }
}

/// Failure modes for rendezvous registration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegisterRendezvousError {
    /// Attempted to register more rendezvous than the fixed capacity allows.
    CapacityExceeded,
    /// The rendezvous identifier is already present in the table.
    Duplicate(RendezvousId),
}

/// Leasing failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaseError {
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
    E: crate::control::cap::EpochTable,
{
    rendezvous: Rendezvous<'cfg, 'cfg, T, U, C, E>,
    active: bool,
}

impl<'cfg, T, U, C, E> RendezvousEntry<'cfg, T, U, C, E>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::EpochTable,
{
    fn new(rendezvous: Rendezvous<'cfg, 'cfg, T, U, C, E>) -> Self {
        Self {
            rendezvous,
            active: false,
        }
    }

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
            Some(&self.rendezvous)
        }
    }

    fn rendezvous_mut(&mut self) -> Option<&mut Rendezvous<'cfg, 'cfg, T, U, C, E>> {
        if self.active {
            None
        } else {
            Some(&mut self.rendezvous)
        }
    }

    fn rendezvous(&mut self) -> &mut Rendezvous<'cfg, 'cfg, T, U, C, E> {
        &mut self.rendezvous
    }
}

/// RAII lease over a rendezvous slot.
///
/// The lease is affine: it cannot be cloned, and dropping it automatically marks
/// the underlying rendezvous as available again. Access to rendezvous facets is
/// mediated through the `Spec` type parameter.
pub struct RendezvousLease<
    'lease,
    'cfg,
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::EpochTable,
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
    E: crate::control::cap::EpochTable,
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
    fn ensure_active(&self) {
        if self.slot.is_none() {
            panic!("rendezvous lease has already been consumed");
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
    pub fn observe(&mut self) -> LeaseObserve<'_, 'cfg> {
        let tap = self.with_rendezvous(|rv| rv.tap() as *const crate::observe::TapRing<'cfg>);
        LeaseObserve::new(tap)
    }

    /// Borrow the rendezvous facets declared by `Spec`.
    pub fn access(&mut self) -> Spec::Access<'_, 'cfg> {
        self.ensure_active();
        Spec::access()
    }

    /// Convert this lease into another specialisation.
    pub fn rebind<Other>(self) -> RendezvousLease<'lease, 'cfg, T, U, C, E, Other>
    where
        Other: RendezvousSpec<T, U, C, E>,
    {
        let mut this = ManuallyDrop::new(self);
        let slot = this.slot.take().expect("lease already consumed");
        RendezvousLease {
            slot: Some(slot),
            _spec: PhantomData,
        }
    }
}

impl<'lease, 'cfg, T, U, C, E> RendezvousLease<'lease, 'cfg, T, U, C, E, FullSpec>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::EpochTable,
    'cfg: 'lease,
{
    #[inline]
    pub fn with_full<R>(
        &mut self,
        f: impl FnOnce(&mut Rendezvous<'cfg, 'cfg, T, U, C, E>) -> R,
    ) -> R {
        self.with_rendezvous(f)
    }

    #[inline]
    pub fn lane_key(
        &mut self,
        lane: crate::rendezvous::Lane,
    ) -> crate::control::cap::LaneKey<'cfg> {
        self.with_rendezvous(|rv| crate::control::cap::LaneKey::new(rv.brand(), lane))
    }

    #[inline]
    pub fn emit_lane_acquire(
        &mut self,
        timestamp: u32,
        rv_id: crate::control::RendezvousId,
        sid: crate::rendezvous::SessionId,
        lane: crate::rendezvous::Lane,
    ) {
        let observe = self.observe();
        observe.emit(crate::observe::LaneAcquire::new(
            timestamp,
            rv_id.raw() as u32,
            sid.raw(),
            lane.raw() as u16,
        ));
    }

    #[inline]
    pub fn release_lane_with_tap(&mut self, lane: crate::rendezvous::Lane) -> bool {
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
    E: crate::control::cap::EpochTable,
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
pub trait RendezvousSpec<T, U, C, E>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::EpochTable,
{
    /// Facet bundle exposed by the spec.
    type Access<'lease, 'cfg>
    where
        'cfg: 'lease,
        T: 'lease,
        U: 'lease,
        C: 'lease,
        E: 'lease;

    /// Construct the facet bundle from a rendezvous entry.
    fn access<'lease, 'cfg>() -> Self::Access<'lease, 'cfg>
    where
        'cfg: 'lease,
        T: 'lease,
        U: 'lease,
        C: 'lease,
        E: 'lease;
}

/// Default spec exposing full mutable access to the rendezvous.
pub struct FullSpec;

/// Spec that exposes only slot storage operations.
pub struct SlotSpec;

/// Spec that exposes only capability table operations.
pub struct CapsSpec;

/// Spec that exposes only splice operations.
pub struct SpliceSpec;

/// Spec that exposes only delegation operations.
pub struct DelegationSpec;

impl<T, U, C, E> RendezvousSpec<T, U, C, E> for SlotSpec
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::EpochTable,
{
    type Access<'lease, 'cfg>
        = crate::rendezvous::SlotFacet<T, U, C, E>
    where
        'cfg: 'lease,
        T: 'lease,
        U: 'lease,
        C: 'lease,
        E: 'lease;

    fn access<'lease, 'cfg>() -> Self::Access<'lease, 'cfg>
    where
        'cfg: 'lease,
        T: 'lease,
        U: 'lease,
        C: 'lease,
        E: 'lease,
    {
        crate::rendezvous::SlotFacet::new()
    }
}

impl<T, U, C, E> RendezvousSpec<T, U, C, E> for CapsSpec
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::EpochTable,
{
    type Access<'lease, 'cfg>
        = crate::rendezvous::CapsFacet<T, U, C, E>
    where
        'cfg: 'lease,
        T: 'lease,
        U: 'lease,
        C: 'lease,
        E: 'lease;

    fn access<'lease, 'cfg>() -> Self::Access<'lease, 'cfg>
    where
        'cfg: 'lease,
        T: 'lease,
        U: 'lease,
        C: 'lease,
        E: 'lease,
    {
        crate::rendezvous::CapsFacet::new()
    }
}

impl<T, U, C, E> RendezvousSpec<T, U, C, E> for SpliceSpec
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::EpochTable,
{
    type Access<'lease, 'cfg>
        = crate::rendezvous::SpliceFacet<T, U, C, E>
    where
        'cfg: 'lease,
        T: 'lease,
        U: 'lease,
        C: 'lease,
        E: 'lease;

    fn access<'lease, 'cfg>() -> Self::Access<'lease, 'cfg>
    where
        'cfg: 'lease,
        T: 'lease,
        U: 'lease,
        C: 'lease,
        E: 'lease,
    {
        crate::rendezvous::SpliceFacet::new()
    }
}

impl<T, U, C, E> RendezvousSpec<T, U, C, E> for DelegationSpec
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::EpochTable,
{
    type Access<'lease, 'cfg>
        = crate::rendezvous::DelegationFacet<T, U, C, E>
    where
        'cfg: 'lease,
        T: 'lease,
        U: 'lease,
        C: 'lease,
        E: 'lease;

    fn access<'lease, 'cfg>() -> Self::Access<'lease, 'cfg>
    where
        'cfg: 'lease,
        T: 'lease,
        U: 'lease,
        C: 'lease,
        E: 'lease,
    {
        crate::rendezvous::DelegationFacet::new()
    }
}

/// Lease-backed access to rendezvous observation events.
#[derive(Clone, Copy)]
pub struct LeaseObserve<'lease, 'cfg> {
    tap: *const crate::observe::TapRing<'cfg>,
    _marker: PhantomData<&'lease crate::observe::TapRing<'cfg>>,
}

impl<'lease, 'cfg> LeaseObserve<'lease, 'cfg> {
    #[inline]
    pub(crate) const fn new(tap: *const crate::observe::TapRing<'cfg>) -> Self {
        Self {
            tap,
            _marker: PhantomData,
        }
    }

    #[inline]
    fn ring(&self) -> &crate::observe::TapRing<'cfg> {
        unsafe { &*self.tap }
    }

    /// Emit an already constructed tap event.
    #[inline]
    pub fn emit(&self, event: crate::observe::TapEvent) {
        crate::observe::emit(self.ring(), event);
    }

    /// Emit a tap event from individual fields.
    #[inline]
    pub fn emit_fields(&self, ts: u32, id: u16, arg0: u32, arg1: u32) {
        crate::observe::emit(
            self.ring(),
            crate::observe::RawEvent::new(ts, id, arg0, arg1),
        );
    }
}

/// Facet bundle returned by [`FullSpec`].
#[derive(Clone, Copy)]
pub struct FullAccess<'lease, 'cfg, T, U, C, E>(
    PhantomData<&'lease mut Rendezvous<'cfg, 'cfg, T, U, C, E>>,
)
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::EpochTable;

impl<'lease, 'cfg, T, U, C, E> FullAccess<'lease, 'cfg, T, U, C, E>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::EpochTable,
{
    #[inline]
    pub fn rendezvous(
        &self,
        lease: &'lease mut RendezvousLease<'lease, 'cfg, T, U, C, E, FullSpec>,
    ) -> &'lease mut Rendezvous<'cfg, 'cfg, T, U, C, E> {
        lease.entry_mut().rendezvous()
    }
}

impl<'lease, 'cfg, T, U, C, E> Default for FullAccess<'lease, 'cfg, T, U, C, E>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::EpochTable,
{
    fn default() -> Self {
        Self(PhantomData)
    }
}

impl<T, U, C, E> RendezvousSpec<T, U, C, E> for FullSpec
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::EpochTable,
{
    type Access<'lease, 'cfg>
        = FullAccess<'lease, 'cfg, T, U, C, E>
    where
        'cfg: 'lease,
        T: 'lease,
        U: 'lease,
        C: 'lease,
        E: 'lease;

    fn access<'lease, 'cfg>() -> Self::Access<'lease, 'cfg>
    where
        'cfg: 'lease,
        T: 'lease,
        U: 'lease,
        C: 'lease,
        E: 'lease,
    {
        FullAccess::default()
    }
}

/// Control automaton executed against a rendezvous lease.
pub trait ControlAutomaton<T, U, C, E>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::EpochTable,
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
    /// coordination. Automatons that do not depend on LeaseGraph should select
    /// [`crate::control::lease::graph::NullLeaseSpec`].
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
pub enum ControlStep<O, E> {
    /// Automaton finished successfully.
    Complete(O),
    /// Automaton failed.
    Abort(E),
}

/// Error type for `SessionCluster::drive` style execution helpers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriveError<E> {
    /// Failed to obtain a rendezvous lease.
    Lease(LeaseError),
    /// The automaton aborted.
    Automaton(E),
}

// ===== LeaseGraph integration for delegation/splice =====

use crate::control::lease::graph::{LeaseGraph, LeaseGraphError, LeaseSpec};

/// Error when running a LeaseGraph-enabled automaton.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DelegationDriveError<E> {
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
    use crate::control::RendezvousId;
    use crate::control::lease::graph::LeaseFacet;

    #[derive(Clone, Copy, Default)]
    struct TestFacet;

    #[derive(Clone, Copy)]
    struct TestContext {
        value: u32,
    }

    impl LeaseFacet for TestFacet {
        type Context<'ctx> = TestContext;
    }

    struct RvLeaseSpec;
    impl LeaseSpec for RvLeaseSpec {
        type NodeId = RendezvousId;
        type Facet = TestFacet;
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
