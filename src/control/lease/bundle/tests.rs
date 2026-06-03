use super::*;
use core::{cell::UnsafeCell, mem::MaybeUninit, ptr};
use std::thread_local;

use crate::{
    control::lease::graph::{LeaseGraph, LeaseSpec},
    control::types::RendezvousId,
    observe::core::TapEvent,
    runtime::{
        config::{Config, CounterClock},
        consts::{DefaultLabelUniverse, RING_EVENTS},
    },
    transport::{Transport, TransportError},
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

    fn open<'a>(&'a self, _port: crate::transport::PortOpen) -> (Self::Tx<'a>, Self::Rx<'a>) {
        ((), ())
    }

    fn poll_send<'a, 'f>(
        &self,
        _tx: &'a mut Self::Tx<'a>,
        _outgoing: crate::transport::Outgoing<'f>,
        _cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Result<(), Self::Error>>
    where
        'a: 'f,
    {
        core::task::Poll::Ready(Ok(()))
    }

    fn poll_recv<'a>(
        &'a self,
        _rx: &'a mut Self::Rx<'a>,
        _cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Result<crate::transport::Incoming<'a>, Self::Error>> {
        core::task::Poll::Ready(Err(TransportError::Offline))
    }

    fn cancel_send<'a>(&self, _tx: &'a mut Self::Tx<'a>) {}

    // Rollback contract exemption: this transport never exercises endpoint rollback.
    fn requeue<'a>(&self, _rx: &mut Self::Rx<'a>) -> Result<(), Self::Error> {
        unreachable!("this fixture never exercises endpoint rollback")
    }
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
    type ChildStorage = crate::control::lease::graph::InlineLeaseChildStorage<RendezvousId, 3>;
    type NodeStorage<'graph>
        = crate::control::lease::graph::InlineLeaseNodeStorage<'graph, Self, 4>
    where
        Self: 'graph;
    const MAX_NODES: usize = 4;
    const MAX_CHILDREN: usize = 3;
}

// Keep bundle fixture slabs above the current rendezvous resident floor so
// these tests exercise lease wiring rather than stale tiny-slab assumptions.
const TEST_SLAB_CAPACITY: usize = 8 * 1024;

type TestControlCore = ControlCore<
    'static,
    DummyTransport,
    DefaultLabelUniverse,
    CounterClock,
    crate::control::cap::mint::EpochTbl,
    4,
>;

type TestRendezvous = crate::rendezvous::core::Rendezvous<
    'static,
    'static,
    DummyTransport,
    DefaultLabelUniverse,
    CounterClock,
    crate::control::cap::mint::EpochTbl,
>;

struct BundleRuntimeGuard {
    tap0: *mut [TapEvent; RING_EVENTS],
    tap1: *mut [TapEvent; RING_EVENTS],
    slab0: *mut [u8; TEST_SLAB_CAPACITY],
    slab1: *mut [u8; TEST_SLAB_CAPACITY],
}

thread_local! {
    static BUNDLE_TAP0: UnsafeCell<[TapEvent; RING_EVENTS]> =
        const { UnsafeCell::new([TapEvent::zero(); RING_EVENTS]) };
    static BUNDLE_TAP1: UnsafeCell<[TapEvent; RING_EVENTS]> =
        const { UnsafeCell::new([TapEvent::zero(); RING_EVENTS]) };
    static BUNDLE_SLAB0: UnsafeCell<[u8; TEST_SLAB_CAPACITY]> =
        const { UnsafeCell::new([0u8; TEST_SLAB_CAPACITY]) };
    static BUNDLE_SLAB1: UnsafeCell<[u8; TEST_SLAB_CAPACITY]> =
        const { UnsafeCell::new([0u8; TEST_SLAB_CAPACITY]) };
    static BUNDLE_CONTROL_CORE: UnsafeCell<MaybeUninit<TestControlCore>> =
        const { UnsafeCell::new(MaybeUninit::uninit()) };
    static BUNDLE_RENDEZVOUS: UnsafeCell<MaybeUninit<TestRendezvous>> =
        const { UnsafeCell::new(MaybeUninit::uninit()) };
}

fn with_bundle_runtime<R>(f: impl FnOnce(&mut BundleRuntimeGuard) -> R) -> R {
    BUNDLE_TAP0.with(|tap0| {
            BUNDLE_TAP1.with(|tap1| {
                BUNDLE_SLAB0.with(|slab0| {
                    BUNDLE_SLAB1.with(|slab1| /* SAFETY: the pointer comes from pinned owner storage and this path holds unique mutable access for the borrow. */ unsafe {
                        let tap0 = &mut *tap0.get();
                        tap0.fill(TapEvent::zero());
                        let tap1 = &mut *tap1.get();
                        tap1.fill(TapEvent::zero());
                        let slab0 = &mut *slab0.get();
                        slab0.fill(0);
                        let slab1 = &mut *slab1.get();
                        slab1.fill(0);
                        let mut runtime = BundleRuntimeGuard {
                            tap0,
                            tap1,
                            slab0,
                            slab1,
                        };
                        f(&mut runtime)
                    })
                })
            })
        })
}

impl BundleRuntimeGuard {
    fn config0<const N: usize>(&mut self) -> Config<'static, DefaultLabelUniverse, CounterClock> {
        assert!(N <= TEST_SLAB_CAPACITY, "fixture slab 0 too small");
        let tap = /* SAFETY: the pointer comes from pinned owner storage and this path holds the unique mutable access for the borrow. */ unsafe { &mut *self.tap0 };
        let slab = /* SAFETY: the pointer comes from pinned owner storage and this path holds the unique mutable access for the borrow. */ unsafe { &mut *self.slab0 };
        Config::from_resources((tap, slab), CounterClock::new())
    }

    fn config1<const N: usize>(&mut self) -> Config<'static, DefaultLabelUniverse, CounterClock> {
        assert!(N <= TEST_SLAB_CAPACITY, "fixture slab 1 too small");
        let tap = /* SAFETY: the pointer comes from pinned owner storage and this path holds the unique mutable access for the borrow. */ unsafe { &mut *self.tap1 };
        let slab = /* SAFETY: the pointer comes from pinned owner storage and this path holds the unique mutable access for the borrow. */ unsafe { &mut *self.slab1 };
        Config::from_resources((tap, slab), CounterClock::new())
    }
}

fn with_bundle_control_core<R>(f: impl FnOnce(&mut TestControlCore) -> R) -> R {
    BUNDLE_CONTROL_CORE.with(|value| /* SAFETY: the caller supplies exclusive uninitialized storage and this initializer writes all exposed fields before return. */ unsafe {
            let ptr = (*value.get()).as_mut_ptr();
            TestControlCore::init_empty(ptr);
            let result = f(&mut *ptr);
            ptr::drop_in_place(ptr);
            result
        })
}

fn with_bundle_rendezvous<R>(
    config: Config<'static, DefaultLabelUniverse, CounterClock>,
    f: impl FnOnce(&mut TestRendezvous) -> R,
) -> R {
    BUNDLE_RENDEZVOUS.with(|value| /* SAFETY: the caller supplies exclusive uninitialized storage and this initializer writes all exposed fields before return. */ unsafe {
            let ptr = (*value.get()).as_mut_ptr();
            let rv_id = RendezvousId::new(1);
            TestRendezvous::init_from_config(ptr, rv_id, config, DummyTransport);
            let result = f(&mut *ptr);
            ptr::drop_in_place(ptr);
            result
        })
}

#[test]
fn populate_local_sets_handles() {
    with_bundle_runtime(|fixture| {
        let config = fixture.config0::<512>();
        with_bundle_rendezvous(config, |rendezvous| {
            let mut ctx: LeaseBundleContext<
                'static,
                'static,
                DummyTransport,
                DefaultLabelUniverse,
                CounterClock,
                crate::control::cap::mint::EpochTbl,
            > = LeaseBundleContext::new();
            ctx.populate_local(rendezvous);

            assert!(ctx.topology().is_some(), "topology bundle seeded");
        })
    });
}

#[test]
fn control_core_builder_returns_context() {
    const MAX_RV: usize = 4;

    with_bundle_runtime(|fixture| {
        let config = fixture.config0::<512>();
        with_bundle_control_core(|core| {
            let rv_id = core
                .register_local_from_config_auto(config, DummyTransport)
                .expect("register rendezvous succeeds");

            let mut ctx = LeaseBundleContext::from_control_core::<MAX_RV>(core, rv_id)
                .expect("context available for local rendezvous");

            assert!(ctx.topology().is_some());
        })
    });
}

#[test]
fn lease_graph_bundle_adds_child_with_handles() {
    const MAX_RV: usize = 4;

    with_bundle_runtime(|fixture| {
        let config_root = fixture.config0::<512>();
        let config_child = fixture.config1::<512>();
        with_bundle_control_core(|core| {
            let root_id = core
                .register_local_from_config_auto(config_root, DummyTransport)
                .expect("register root rendezvous");
            let child_id = core
                .register_local_from_config_auto(config_child, DummyTransport)
                .expect("register child rendezvous");

            let root_ctx = LeaseBundleContext::from_control_core::<MAX_RV>(core, root_id)
                .expect("root context available");
            let mut graph_storage = MaybeUninit::<LeaseGraph<'_, TestSpec>>::uninit();
            let mut graph = /* SAFETY: the caller supplies exclusive uninitialized storage and this initializer writes all exposed fields before return. */ unsafe {
                    LeaseGraph::<TestSpec>::init_new(
                        graph_storage.as_mut_ptr(),
                        root_id,
                        LeaseBundleFacet::<
                            DummyTransport,
                            DefaultLabelUniverse,
                            CounterClock,
                            crate::control::cap::mint::EpochTbl,
                        >::default(),
                        root_ctx,
                    );
                    graph_storage.assume_init()
                };
            let child_ctx =
                LeaseBundleContext::from_control_core_or_default::<MAX_RV>(core, child_id);
            graph
                .add_child(
                    root_id,
                    child_id,
                    LeaseBundleFacet::<
                        DummyTransport,
                        DefaultLabelUniverse,
                        CounterClock,
                        crate::control::cap::mint::EpochTbl,
                    >::default(),
                    child_ctx,
                )
                .expect("child added");

            let mut child_handle = graph.handle_mut(child_id).expect("child handle");
            let ctx = child_handle.context();
            assert!(ctx.topology().is_some(), "topology bundle seeded in child");
        })
    });
}
