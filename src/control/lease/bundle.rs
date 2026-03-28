//! LeaseGraph facet bundle combining caps/splice contexts plus tap hooks.

use core::{marker::PhantomData, ptr::NonNull};

use crate::{
    control::types::{Lane, RendezvousId},
    control::{
        automaton::splice::SpliceGraphContext,
        cap::mint::CapsMask,
        lease::{
            core::{ControlCore, LeaseObserve},
            graph::{LeaseFacet, LeaseGraph, LeaseGraphError, LeaseSpec},
            planner::LeaseFacetNeeds,
        },
    },
    epf::vm::Slot,
    observe::core::TapEvent,
    rendezvous::{capability::CapTable, core::Rendezvous, slots::SlotArena},
    runtime::{config::Clock, consts::LabelUniverse},
    transport::Transport,
};

const CAP_LOG_CAPACITY: usize = 4;
const CAP_MASK_LOG_CAPACITY: usize = 4;
const SLOT_LOG_CAPACITY: usize = 4;

/// Error returned when a lease bundle handle runs out of tracking capacity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LeaseBundleError {
    Capacity,
}

#[derive(Clone, Copy)]
struct CapsMintRecord {
    nonce: [u8; crate::control::cap::mint::CAP_NONCE_LEN],
}

#[derive(Clone, Copy)]
struct CapsMaskRecord {
    lane: Lane,
    mask: CapsMask,
}

/// Handle that records minted capabilities and capability mask adjustments so rollback can purge them.
pub(crate) struct CapsBundleHandle<'ctx, 'cfg, T, U, C, E>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
{
    rendezvous: NonNull<Rendezvous<'ctx, 'cfg, T, U, C, E>>,
    table: NonNull<CapTable>,
    pending: [Option<CapsMintRecord>; CAP_LOG_CAPACITY],
    masks: [Option<CapsMaskRecord>; CAP_MASK_LOG_CAPACITY],
    _marker: PhantomData<&'ctx crate::observe::core::TapRing<'cfg>>,
}

impl<'ctx, 'cfg, T, U, C, E> CapsBundleHandle<'ctx, 'cfg, T, U, C, E>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
{
    pub(crate) const fn new(
        rendezvous: NonNull<Rendezvous<'ctx, 'cfg, T, U, C, E>>,
        table: NonNull<CapTable>,
    ) -> Self {
        Self {
            rendezvous,
            table,
            pending: [None; CAP_LOG_CAPACITY],
            masks: [None; CAP_MASK_LOG_CAPACITY],
            _marker: PhantomData,
        }
    }

    #[inline]
    pub(crate) fn track_mint(
        &mut self,
        nonce: [u8; crate::control::cap::mint::CAP_NONCE_LEN],
    ) -> Result<(), LeaseBundleError> {
        if self
            .pending
            .iter()
            .any(|entry| entry.is_some_and(|rec| rec.nonce == nonce))
        {
            return Ok(());
        }
        if let Some(slot) = self.pending.iter_mut().find(|entry| entry.is_none()) {
            *slot = Some(CapsMintRecord { nonce });
            Ok(())
        } else {
            Err(LeaseBundleError::Capacity)
        }
    }

    #[inline]
    pub(crate) fn track_mask(
        &mut self,
        lane: Lane,
        previous: CapsMask,
    ) -> Result<(), LeaseBundleError> {
        if self
            .masks
            .iter()
            .any(|entry| entry.is_some_and(|rec| rec.lane == lane))
        {
            return Ok(());
        }
        if let Some(slot) = self.masks.iter_mut().find(|entry| entry.is_none()) {
            *slot = Some(CapsMaskRecord {
                lane,
                mask: previous,
            });
            Ok(())
        } else {
            Err(LeaseBundleError::Capacity)
        }
    }

    #[inline]
    fn on_commit(&mut self) {
        self.pending.fill(None);
        self.masks.fill(None);
    }

    fn on_rollback(&mut self) {
        for entry in self.masks.iter_mut() {
            if let Some(record) = entry.take() {
                // SAFETY: rendezvous pointer originates from an exclusive lease.
                unsafe {
                    self.rendezvous
                        .as_mut()
                        .set_caps_mask_for_lane(record.lane, record.mask);
                }
            }
        }
        for entry in self.pending.iter_mut() {
            if let Some(record) = entry.take() {
                unsafe {
                    self.table.as_ref().release_by_nonce(&record.nonce);
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
struct SlotStageRecord {
    slot: Slot,
}

/// Handle that records slot staging so rollback can scrub temporary buffers.
pub(crate) struct SlotBundleHandle<'ctx, 'cfg> {
    arena: NonNull<SlotArena>,
    stages: [Option<SlotStageRecord>; SLOT_LOG_CAPACITY],
    _marker: PhantomData<&'ctx crate::observe::core::TapRing<'cfg>>,
}

impl<'ctx, 'cfg> SlotBundleHandle<'ctx, 'cfg> {
    pub(crate) const fn new(arena: NonNull<SlotArena>) -> Self {
        Self {
            arena,
            stages: [None; SLOT_LOG_CAPACITY],
            _marker: PhantomData,
        }
    }

    #[inline]
    pub(crate) fn track_stage(&mut self, slot: Slot) -> Result<(), LeaseBundleError> {
        if self
            .stages
            .iter()
            .any(|entry| entry.is_some_and(|rec| rec.slot == slot))
        {
            return Ok(());
        }
        if let Some(entry) = self.stages.iter_mut().find(|entry| entry.is_none()) {
            *entry = Some(SlotStageRecord { slot });
            Ok(())
        } else {
            Err(LeaseBundleError::Capacity)
        }
    }

    #[inline]
    fn on_commit(&mut self) {
        self.stages.fill(None);
    }

    fn on_rollback(&mut self) {
        for entry in self.stages.iter_mut() {
            if let Some(record) = entry.take() {
                unsafe {
                    let arena = self.arena.as_mut();
                    let storage = arena.storage_mut(record.slot);
                    storage.staging_mut().fill(0);
                }
            }
        }
    }
}

/// Facet marker used by LeaseGraph nodes that require bundling.
#[allow(clippy::type_complexity)]
pub(crate) struct LeaseBundleFacet<T, U, C, E>(PhantomData<fn() -> (T, U, C, E)>)
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable;

impl<T, U, C, E> Copy for LeaseBundleFacet<T, U, C, E>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
{
}

impl<T, U, C, E> Clone for LeaseBundleFacet<T, U, C, E>
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

impl<T, U, C, E> Default for LeaseBundleFacet<T, U, C, E>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
{
    fn default() -> Self {
        Self(PhantomData)
    }
}

/// Per-node bundle stored in LeaseGraph when using [`LeaseBundleFacet`].
#[allow(clippy::type_complexity)]
pub(crate) struct LeaseBundleContext<'ctx, 'cfg, T, U, C, E>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
{
    observe: Option<LeaseObserve<'ctx, 'cfg>>,
    splice: Option<SpliceGraphContext>,
    caps: Option<CapsBundleHandle<'ctx, 'cfg, T, U, C, E>>,
    slots: Option<SlotBundleHandle<'ctx, 'cfg>>,
    commit_event: Option<TapEvent>,
    rollback_event: Option<TapEvent>,
    _marker: PhantomData<fn() -> (T, U, C, E)>,
}

impl<'ctx, 'cfg, T, U, C, E> Default for LeaseBundleContext<'ctx, 'cfg, T, U, C, E>
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

impl<'ctx, 'cfg, T, U, C, E> LeaseBundleContext<'ctx, 'cfg, T, U, C, E>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
{
    #[inline]
    pub(crate) const fn new() -> Self {
        Self {
            observe: None,
            splice: None,
            caps: None,
            slots: None,
            commit_event: None,
            rollback_event: None,
            _marker: PhantomData,
        }
    }

    #[inline]
    pub(crate) fn set_observe(&mut self, observe: LeaseObserve<'ctx, 'cfg>) {
        self.observe = Some(observe);
    }

    #[inline]
    #[cfg(test)]
    pub(crate) fn observe(&self) -> Option<LeaseObserve<'ctx, 'cfg>> {
        self.observe
    }

    #[inline]
    pub(crate) fn set_splice(&mut self, ctx: SpliceGraphContext) {
        self.splice = Some(ctx);
    }

    #[inline]
    pub(crate) fn set_caps(&mut self, handle: CapsBundleHandle<'ctx, 'cfg, T, U, C, E>) {
        self.caps = Some(handle);
    }

    #[inline]
    pub(crate) fn set_slot_bundle(&mut self, handle: SlotBundleHandle<'ctx, 'cfg>) {
        self.slots = Some(handle);
    }

    #[inline]
    pub(crate) fn caps_mut(&mut self) -> Option<&mut CapsBundleHandle<'ctx, 'cfg, T, U, C, E>> {
        self.caps.as_mut()
    }

    #[inline]
    pub(crate) fn slots_mut(&mut self) -> Option<&mut SlotBundleHandle<'ctx, 'cfg>> {
        self.slots.as_mut()
    }

    #[inline]
    pub(crate) fn splice(&mut self) -> Option<&mut SpliceGraphContext> {
        self.splice.as_mut()
    }

    #[inline]
    #[cfg(test)]
    pub(crate) fn register_commit_tap(&mut self, event: TapEvent) {
        self.commit_event = Some(event);
    }

    #[inline]
    pub(crate) fn populate_local_with_needs(
        &mut self,
        rendezvous: &mut Rendezvous<'ctx, 'cfg, T, U, C, E>,
        needs: LeaseFacetNeeds,
    ) where
        'cfg: 'ctx,
    {
        let observe = rendezvous.observe_facet();
        self.set_observe(LeaseObserve::new(core::ptr::from_ref(observe.tap())));

        if needs.requires_caps() || needs.requires_delegation() || needs.requires_splice() {
            let rendezvous_ptr = NonNull::from(&mut *rendezvous);
            let caps_ptr = NonNull::from(rendezvous.caps());
            self.set_caps(CapsBundleHandle::new(rendezvous_ptr, caps_ptr));
        }

        if needs.requires_slots() {
            let mut bundle = rendezvous.slot_bundle();
            let arena_ptr = NonNull::from(bundle.arena());
            self.set_slot_bundle(SlotBundleHandle::new(arena_ptr));
        }
    }

    #[inline]
    #[cfg(test)]
    pub(crate) fn populate_local(&mut self, rendezvous: &mut Rendezvous<'ctx, 'cfg, T, U, C, E>)
    where
        'cfg: 'ctx,
    {
        self.populate_local_with_needs(rendezvous, LeaseFacetNeeds::all());
    }

    #[inline]
    pub(crate) fn on_commit(&mut self) {
        if let Some(handle) = self.caps.as_mut() {
            handle.on_commit();
        }
        if let Some(handle) = self.slots.as_mut() {
            handle.on_commit();
        }
        if let Some(ctx) = self.splice.as_mut() {
            ctx.clear();
        }
        if let (Some(observe), Some(event)) = (self.observe, self.commit_event.take()) {
            observe.emit(event);
        }
    }

    #[inline]
    pub(crate) fn on_rollback(&mut self) {
        if let Some(handle) = self.caps.as_mut() {
            handle.on_rollback();
        }
        if let Some(handle) = self.slots.as_mut() {
            handle.on_rollback();
        }
        if let Some(ctx) = self.splice.as_mut() {
            ctx.clear();
        }
        if let (Some(observe), Some(event)) = (self.observe, self.rollback_event.take()) {
            observe.emit(event);
        }
    }
}

impl<'cfg, T, U, C, E> LeaseBundleContext<'cfg, 'cfg, T, U, C, E>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
{
    #[inline]
    pub(crate) fn from_control_core_with_needs<const MAX_RV: usize>(
        core: &mut ControlCore<'cfg, T, U, C, E, MAX_RV>,
        rv_id: RendezvousId,
        needs: LeaseFacetNeeds,
    ) -> Option<Self> {
        let mut ctx = Self::new();
        if let Some(rendezvous) = core.get_mut(&rv_id) {
            ctx.populate_local_with_needs(rendezvous, needs);
            Some(ctx)
        } else {
            None
        }
    }

    #[inline]
    #[cfg(test)]
    pub(crate) fn from_control_core<const MAX_RV: usize>(
        core: &mut ControlCore<'cfg, T, U, C, E, MAX_RV>,
        rv_id: RendezvousId,
    ) -> Option<Self> {
        Self::from_control_core_with_needs(core, rv_id, LeaseFacetNeeds::all())
    }

    #[inline]
    pub(crate) fn from_control_core_or_default<const MAX_RV: usize>(
        core: &mut ControlCore<'cfg, T, U, C, E, MAX_RV>,
        rv_id: RendezvousId,
    ) -> Self {
        Self::from_control_core_with_needs(core, rv_id, LeaseFacetNeeds::all())
            .unwrap_or_else(Self::new)
    }
}

pub(crate) trait LeaseGraphBundleExt<'graph, T, U, C, E, const MAX_RV: usize>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
{
    #[cfg(test)]
    fn add_child_with_bundle(
        &mut self,
        core: &mut ControlCore<'graph, T, U, C, E, MAX_RV>,
        parent: RendezvousId,
        child: RendezvousId,
    ) -> Result<(), LeaseGraphError>;

    fn add_child_with_bundle_config<F>(
        &mut self,
        core: &mut ControlCore<'graph, T, U, C, E, MAX_RV>,
        parent: RendezvousId,
        child: RendezvousId,
        configure: F,
    ) -> Result<(), LeaseGraphError>
    where
        F: FnOnce(&mut LeaseBundleContext<'graph, 'graph, T, U, C, E>);
}

impl<'graph, T, U, C, E, S, const MAX_RV: usize> LeaseGraphBundleExt<'graph, T, U, C, E, MAX_RV>
    for LeaseGraph<'graph, S>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
    S: LeaseSpec<NodeId = RendezvousId, Facet = LeaseBundleFacet<T, U, C, E>>,
{
    #[inline]
    #[cfg(test)]
    fn add_child_with_bundle(
        &mut self,
        core: &mut ControlCore<'graph, T, U, C, E, MAX_RV>,
        parent: RendezvousId,
        child: RendezvousId,
    ) -> Result<(), LeaseGraphError> {
        let context = LeaseBundleContext::from_control_core_or_default::<MAX_RV>(core, child);
        self.add_child(parent, child, S::Facet::default(), context)
    }

    #[inline]
    fn add_child_with_bundle_config<F>(
        &mut self,
        core: &mut ControlCore<'graph, T, U, C, E, MAX_RV>,
        parent: RendezvousId,
        child: RendezvousId,
        configure: F,
    ) -> Result<(), LeaseGraphError>
    where
        F: FnOnce(&mut LeaseBundleContext<'graph, 'graph, T, U, C, E>),
    {
        let mut context = LeaseBundleContext::from_control_core_or_default::<MAX_RV>(core, child);
        configure(&mut context);
        self.add_child(parent, child, S::Facet::default(), context)
    }
}

impl<T, U, C, E> LeaseFacet for LeaseBundleFacet<T, U, C, E>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
{
    type Context<'ctx> = LeaseBundleContext<'ctx, 'ctx, T, U, C, E>;

    #[inline]
    fn on_commit<'ctx>(&self, ctx: &mut Self::Context<'ctx>) {
        ctx.on_commit();
    }

    #[inline]
    fn on_rollback<'ctx>(&self, ctx: &mut Self::Context<'ctx>) {
        ctx.on_rollback();
    }
}

#[cfg(test)]
mod tests {
    use super::LeaseGraphBundleExt;
    use super::*;
    use core::ptr::NonNull;
    use std::boxed::Box;
    use std::vec;

    use crate::{
        control::cap::mint::{CapShot, CapsMask, EndpointResource, ResourceKind},
        control::cluster::effects::CpEffect,
        control::types::{Lane, RendezvousId, SessionId},
        observe::core::TapRing,
        observe::{self},
        rendezvous::{capability::CapEntry, core::Rendezvous},
        runtime::{
            config::{Config, CounterClock},
            consts::{DefaultLabelUniverse, RING_EVENTS},
        },
        transport::{Transport, TransportError, wire::Payload},
    };

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
        type Send<'a>
            = core::future::Ready<Result<(), Self::Error>>
        where
            Self: 'a;
        type Recv<'a>
            = core::future::Ready<Result<Payload<'a>, Self::Error>>
        where
            Self: 'a;
        type Metrics = ();

        fn open<'a>(&'a self, _local_role: u8, _session_id: u32) -> (Self::Tx<'a>, Self::Rx<'a>) {
            ((), ())
        }

        fn send<'a, 'f>(
            &'a self,
            _tx: &'a mut Self::Tx<'a>,
            _outgoing: crate::transport::Outgoing<'f>,
        ) -> Self::Send<'a>
        where
            'a: 'f,
        {
            core::future::ready(Ok(()))
        }

        fn recv<'a>(&'a self, _rx: &'a mut Self::Rx<'a>) -> Self::Recv<'a> {
            core::future::ready(Err(TransportError::Offline))
        }

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

    struct TestSpec;

    impl LeaseSpec for TestSpec {
        type NodeId = RendezvousId;
        type Facet = LeaseBundleFacet<
            DummyTransport,
            DefaultLabelUniverse,
            CounterClock,
            crate::control::cap::mint::EpochTbl,
        >;
        const MAX_NODES: usize = 4;
        const MAX_CHILDREN: usize = 3;
    }

    fn leak_empty_control_core<const MAX_RV: usize>() -> &'static mut ControlCore<
        'static,
        DummyTransport,
        DefaultLabelUniverse,
        CounterClock,
        crate::control::cap::mint::EpochTbl,
        MAX_RV,
    > {
        let mut core = Box::<
            ControlCore<
                'static,
                DummyTransport,
                DefaultLabelUniverse,
                CounterClock,
                crate::control::cap::mint::EpochTbl,
                MAX_RV,
            >,
        >::new_uninit();
        unsafe {
            ControlCore::init_empty(core.as_mut_ptr());
            Box::leak(core.assume_init())
        }
    }

    fn leak_tap_storage() -> &'static mut [TapEvent; RING_EVENTS] {
        let storage: Box<[TapEvent]> = vec![TapEvent::default(); RING_EVENTS].into_boxed_slice();
        let storage: Box<[TapEvent; RING_EVENTS]> = storage.try_into().expect("ring events length");
        Box::leak(storage)
    }

    fn leak_slab(size: usize) -> &'static mut [u8] {
        Box::leak(vec![0u8; size].into_boxed_slice())
    }

    #[test]
    fn populate_local_sets_handles() {
        let tap_storage = leak_tap_storage();
        let slab = leak_slab(512);
        let config = Config::new(tap_storage, slab);
        let mut rendezvous = Rendezvous::from_config(config, DummyTransport);

        let mut ctx: LeaseBundleContext<
            'static,
            'static,
            DummyTransport,
            DefaultLabelUniverse,
            CounterClock,
            crate::control::cap::mint::EpochTbl,
        > = LeaseBundleContext::new();
        ctx.populate_local(&mut rendezvous);

        assert!(ctx.observe().is_some(), "observe facet seeded");
        assert!(ctx.caps_mut().is_some(), "caps bundle seeded");
        assert!(ctx.slots_mut().is_some(), "slot bundle seeded");
    }

    #[test]
    fn control_core_builder_returns_context() {
        const MAX_RV: usize = 4;

        let tap_storage = leak_tap_storage();
        let slab = leak_slab(512);
        let config = Config::new(tap_storage, slab);
        let rendezvous = Rendezvous::from_config(config, DummyTransport);
        let rv_id = rendezvous.id();

        let core = leak_empty_control_core::<MAX_RV>();
        core.register_local(rendezvous)
            .expect("register rendezvous succeeds");

        let mut ctx = LeaseBundleContext::from_control_core::<MAX_RV>(core, rv_id)
            .expect("context available for local rendezvous");

        assert!(ctx.observe().is_some());
        assert!(ctx.caps_mut().is_some());
        assert!(ctx.slots_mut().is_some());
    }

    #[test]
    fn lease_graph_bundle_adds_child_with_handles() {
        const MAX_RV: usize = 4;

        let tap_storage_root = leak_tap_storage();
        let slab_root = leak_slab(512);
        let config_root = Config::new(tap_storage_root, slab_root);
        let rendezvous_root = Rendezvous::from_config(config_root, DummyTransport);
        let root_id = rendezvous_root.id();

        let tap_storage_child = leak_tap_storage();
        let slab_child = leak_slab(512);
        let config_child = Config::new(tap_storage_child, slab_child);
        let rendezvous_child = Rendezvous::from_config(config_child, DummyTransport);
        let child_id = rendezvous_child.id();

        let core = leak_empty_control_core::<MAX_RV>();
        core.register_local(rendezvous_root)
            .expect("register root rendezvous");
        core.register_local(rendezvous_child)
            .expect("register child rendezvous");

        let root_ctx = LeaseBundleContext::from_control_core::<MAX_RV>(core, root_id)
            .expect("root context available");
        let mut graph = LeaseGraph::<TestSpec>::new(
            root_id,
            LeaseBundleFacet::<
                DummyTransport,
                DefaultLabelUniverse,
                CounterClock,
                crate::control::cap::mint::EpochTbl,
            >::default(),
            root_ctx,
        );
        graph
            .add_child_with_bundle(core, root_id, child_id)
            .expect("child added");

        let mut child_handle = graph.handle_mut(child_id).expect("child handle");
        let ctx = child_handle.context();
        assert!(ctx.caps_mut().is_some(), "caps bundle seeded in child");
        assert!(ctx.slots_mut().is_some(), "slot bundle seeded in child");
    }

    #[test]
    fn commit_emits_registered_tap() {
        let ring = TapRing::from_storage(leak_tap_storage());
        let static_ring = unsafe { ring.assume_static() };

        let mut ctx: LeaseBundleContext<
            'static,
            'static,
            DummyTransport,
            DefaultLabelUniverse,
            CounterClock,
            crate::control::cap::mint::EpochTbl,
        > = LeaseBundleContext::new();
        ctx.set_observe(LeaseObserve::new(core::ptr::from_ref(static_ring)));
        let event = observe::events::DelegSplice::new(7, 1, 2);
        ctx.register_commit_tap(event);

        let facet = LeaseBundleFacet::<
            DummyTransport,
            DefaultLabelUniverse,
            CounterClock,
            crate::control::cap::mint::EpochTbl,
        >::default();
        facet.on_commit(&mut ctx);

        assert_eq!(ring.head(), 1);
        let recorded = ring.as_slice()[0];
        assert_eq!(recorded.id, event.id);
        assert_eq!(recorded.arg0, event.arg0);
        assert_eq!(recorded.arg1, event.arg1);
    }

    #[test]
    fn caps_mint_released_on_rollback() {
        use crate::control::cap::mint::{CAP_HANDLE_LEN, CAP_NONCE_LEN, CapsMask};
        use crate::rendezvous::error::CapError;

        let tap_storage = leak_tap_storage();
        let slab = leak_slab(256);
        let config = Config::new(tap_storage, slab);
        let mut rendezvous = Rendezvous::from_config(config, DummyTransport);
        let rv_ptr = NonNull::from(&mut rendezvous);
        let cap_ptr = NonNull::from(rendezvous.caps());
        let cap_table = rendezvous.caps();
        let mut ctx: LeaseBundleContext<
            'static,
            'static,
            DummyTransport,
            DefaultLabelUniverse,
            CounterClock,
            crate::control::cap::mint::EpochTbl,
        > = LeaseBundleContext::new();
        ctx.set_caps(CapsBundleHandle::new(rv_ptr, cap_ptr));

        let sid = SessionId::new(1);
        let lane = Lane::new(2);
        let nonce = [0xAB; CAP_NONCE_LEN];
        let entry = CapEntry {
            sid,
            lane: Lane::new(lane.raw()),
            kind_tag: EndpointResource::TAG,
            shot: CapShot::Many,
            role: 7,
            consumed: false,
            nonce,
            caps_mask: CapsMask::allow_all(),
            handle: [0u8; CAP_HANDLE_LEN],
        };
        cap_table.insert_entry(entry).expect("insert succeeds");

        ctx.caps_mut()
            .expect("caps handle present")
            .track_mint(nonce)
            .expect("log mint");

        ctx.on_rollback();

        let claim = cap_table.claim_by_nonce(
            &nonce,
            SessionId::new(1),
            Lane::new(2),
            EndpointResource::TAG,
            7,
            CapShot::Many,
            CapsMask::allow_all(),
        );
        assert!(matches!(claim, Err(CapError::UnknownToken)));
    }

    #[test]
    fn slot_staging_cleared_on_rollback() {
        use crate::rendezvous::slots::SlotArena;

        let mut arena = SlotArena::new();
        let arena_ptr = NonNull::from(&mut arena);
        let slot = Slot::Forward;
        {
            let storage = arena.storage_mut(slot);
            storage.staging_mut()[0] = 0xAA;
        }

        let mut ctx: LeaseBundleContext<
            'static,
            'static,
            DummyTransport,
            DefaultLabelUniverse,
            CounterClock,
            crate::control::cap::mint::EpochTbl,
        > = LeaseBundleContext::new();
        ctx.set_slot_bundle(SlotBundleHandle::new(arena_ptr));

        {
            let slots = ctx.slots_mut().expect("slot handle present");
            slots.track_stage(slot).expect("log stage");
        }

        ctx.on_commit();
        assert_eq!(arena.storage(slot).staging()[0], 0xAA);

        {
            let storage = arena.storage_mut(slot);
            storage.staging_mut()[0] = 0xBB;
        }

        {
            let slots = ctx.slots_mut().expect("slot handle present");
            slots.track_stage(slot).expect("log stage");
        }

        ctx.on_rollback();

        assert!(arena.storage(slot).staging().iter().all(|byte| *byte == 0));
    }

    #[test]
    fn caps_mask_restored_on_rollback() {
        let tap_storage = leak_tap_storage();
        let slab = leak_slab(256);
        let config = Config::new(tap_storage, slab);
        let mut rendezvous = Rendezvous::from_config(config, DummyTransport);
        let original = rendezvous.caps_mask_for_lane(Lane::new(0));
        let rv_ptr = NonNull::from(&mut rendezvous);
        let cap_ptr = NonNull::from(rendezvous.caps());

        let mut ctx: LeaseBundleContext<
            'static,
            'static,
            DummyTransport,
            DefaultLabelUniverse,
            CounterClock,
            crate::control::cap::mint::EpochTbl,
        > = LeaseBundleContext::new();
        ctx.set_caps(CapsBundleHandle::new(rv_ptr, cap_ptr));

        {
            let caps = ctx.caps_mut().expect("caps handle present");
            caps.track_mask(Lane::new(0), original).expect("log mask");
        }

        let updated = original.union(CapsMask::empty().with(CpEffect::SpliceBegin));
        rendezvous.set_caps_mask_for_lane(Lane::new(0), updated);

        ctx.on_rollback();

        assert_eq!(
            rendezvous.caps_mask_for_lane(Lane::new(0)).bits(),
            original.bits()
        );
    }
}
