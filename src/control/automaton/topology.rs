//! TopologyAutomaton — lease-first distributed topology.
//!
//! This module provides ControlAutomaton implementations for source-side topology operations.
//! It replaces the procedural DistributedTopology::begin/commit with typed automatons
//! that use RendezvousLease to access TopologyFacet.
//!
//! ## Design
//!
//! - **Source-only**: These automatons handle source RV operations only
//! - **Destination handling**: Destination RV ack is handled by existing Rendezvous::process_topology_intent
//! - **No LeaseGraph**: Single source RV doesn't need ownership tracking
//! - **Lease-first**: Uses ControlAutomaton pattern with RendezvousLease
//!
//! ## Lifecycle
//!
//! 1. **Begin** — Source RV calls `TopologyFacet::begin`, generates TopologyIntent
//! 2. **Wait Ack** — (External: destination processes intent via Rendezvous::process_topology_intent)
//! 3. **Commit** — SessionCluster finalizes source+destination topology as one protocol step

use core::marker::PhantomData;

use crate::{
    control::automaton::distributed::TopologyIntent,
    control::cluster::error::TopologyError,
    control::{
        lease::{
            bundle::LeaseBundleFacet,
            core::{ControlAutomaton, ControlStep, RendezvousLease, TopologySpec},
            graph::{InlineLeaseChildStorage, InlineLeaseNodeStorage, LeaseSpec},
        },
        types::RendezvousId,
    },
    runtime::{config::Clock, consts::LabelUniverse},
    transport::Transport,
};

#[cfg(test)]
use crate::control::{
    automaton::distributed::TopologyAck,
    types::{Lane, SessionId},
};

#[derive(Debug, Default)]
pub(crate) struct TopologyGraphContext {
    pub(crate) last_intent: Option<TopologyIntent>,
}

impl TopologyGraphContext {
    pub(crate) fn new(last_intent: Option<TopologyIntent>) -> Self {
        Self { last_intent }
    }

    #[inline]
    pub(crate) fn clear(&mut self) {
        self.last_intent = None;
    }
}

/// Maximum node capacity for [`TopologyLeaseSpec`].
pub(crate) const TOPOLOGY_LEASE_MAX_NODES: usize = 3;
/// Maximum child capacity for [`TopologyLeaseSpec`].
pub(crate) const TOPOLOGY_LEASE_MAX_CHILDREN: usize = 2;

/// LeaseGraph specification for topology orchestration.
pub(crate) struct TopologyLeaseSpec<T, U, C, E>(PhantomData<(T, U, C, E)>);

impl<T, U, C, E> LeaseSpec for TopologyLeaseSpec<T, U, C, E>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
{
    type NodeId = RendezvousId;
    type Facet = LeaseBundleFacet<T, U, C, E>;
    type ChildStorage = InlineLeaseChildStorage<RendezvousId, TOPOLOGY_LEASE_MAX_CHILDREN>;
    type NodeStorage<'graph>
        = InlineLeaseNodeStorage<'graph, Self, TOPOLOGY_LEASE_MAX_NODES>
    where
        Self: 'graph;
    const MAX_NODES: usize = TOPOLOGY_LEASE_MAX_NODES;
    const MAX_CHILDREN: usize = TOPOLOGY_LEASE_MAX_CHILDREN;
}

/// Begin automaton for distributed topology.
///
/// This automaton receives a TopologyIntent, validates it, and calls
/// TopologyFacet::begin on the source rendezvous. On success, it returns
/// the TopologyIntent to be sent to the destination.
///
/// ## Usage
///
/// ```ignore
/// let intent = TopologyIntent { src_rv, dst_rv, sid, old_gen, new_gen, ... };
/// let mut lease = core.lease::<TopologySpec>(src_rv)?;
/// let result = TopologyBeginAutomaton::run(&mut lease, intent);
/// match result {
///     ControlStep::Complete(intent_msg) => {
///         // Send intent_msg to destination
///     }
///     ControlStep::Abort(err) => {
///         // Handle error
///     }
/// }
/// ```
pub(crate) struct TopologyBeginAutomaton;

impl<T, U, C, E> ControlAutomaton<T, U, C, E> for TopologyBeginAutomaton
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
{
    type Spec = TopologySpec;
    type Seed = TopologyIntent;
    type Output = TopologyIntent;
    type Error = TopologyError;
    type GraphSpec = TopologyLeaseSpec<T, U, C, E>;

    fn run<'lease, 'cfg>(
        lease: &mut RendezvousLease<'lease, 'cfg, T, U, C, E, Self::Spec>,
        intent: Self::Seed,
    ) -> ControlStep<Self::Output, Self::Error>
    where
        'cfg: 'lease,
    {
        // Validate generation ordering
        if intent.new_gen.raw() <= intent.old_gen.raw() {
            return ControlStep::Abort(TopologyError::GenerationMismatch);
        }

        // Begin topology transition at source
        match lease.with_rendezvous(|rv| {
            let facet = rv.topology_facet();
            facet.begin_from_intent(rv, intent)
        }) {
            Ok(()) => ControlStep::Complete(intent),
            Err(ra_err) => ControlStep::Abort(ra_err.into()),
        }
    }

    fn run_with_graph<'lease, 'cfg, 'graph>(
        graph: &'graph mut crate::control::lease::graph::LeaseGraph<
            'graph,
            TopologyLeaseSpec<T, U, C, E>,
        >,
        root_lease: &mut RendezvousLease<'lease, 'cfg, T, U, C, E, Self::Spec>,
        intent: Self::Seed,
    ) -> ControlStep<Self::Output, Self::Error>
    where
        'cfg: 'lease,
    {
        let root_id = graph.root_id();
        if intent.src_rv != root_id {
            return ControlStep::Abort(TopologyError::RendezvousIdMismatch);
        }

        let _ = graph;

        match <Self as ControlAutomaton<T, U, C, E>>::run(root_lease, intent) {
            ControlStep::Complete(intent) => {
                {
                    let mut handle = graph.root_handle_mut();
                    if let Some(topology) = handle.context().topology() {
                        topology.last_intent = Some(intent);
                    }
                }
                ControlStep::Complete(intent)
            }
            ControlStep::Abort(err) => {
                {
                    let mut handle = graph.root_handle_mut();
                    if let Some(topology) = handle.context().topology() {
                        topology.clear();
                    }
                }
                ControlStep::Abort(err)
            }
        }
    }
}

#[cfg(test)]
/// Commit automaton for distributed topology transitions.
///
/// Distributed topology commit is cluster-owned because it must retire the
/// source rendezvous, finalize the destination rendezvous, and close the
/// cluster-wide bookkeeping entry as one protocol step. A single-rendezvous
/// lease cannot own that full transition safely, so direct automaton commit
/// entrypoints fail closed.
///
/// Use [`SessionCluster::run_effect_step`](crate::control::cluster::core::SessionCluster::run_effect_step)
/// for distributed commit orchestration.
pub(crate) struct TopologyCommitAutomaton;

#[cfg(test)]
impl<T, U, C, E> ControlAutomaton<T, U, C, E> for TopologyCommitAutomaton
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
{
    type Spec = TopologySpec;
    type Seed = TopologyAck;
    type Output = TopologyAck;
    type Error = TopologyError;
    type GraphSpec = TopologyLeaseSpec<T, U, C, E>;

    fn run<'lease, 'cfg>(
        _lease: &mut RendezvousLease<'lease, 'cfg, T, U, C, E, Self::Spec>,
        _ack: Self::Seed,
    ) -> ControlStep<Self::Output, Self::Error>
    where
        'cfg: 'lease,
    {
        ControlStep::Abort(TopologyError::InvalidState)
    }

    fn run_with_graph<'lease, 'cfg, 'graph>(
        _graph: &'graph mut crate::control::lease::graph::LeaseGraph<
            'graph,
            TopologyLeaseSpec<T, U, C, E>,
        >,
        _root_lease: &mut RendezvousLease<'lease, 'cfg, T, U, C, E, Self::Spec>,
        _ack: Self::Seed,
    ) -> ControlStep<Self::Output, Self::Error>
    where
        'cfg: 'lease,
    {
        ControlStep::Abort(TopologyError::InvalidState)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        control::lease::{
            bundle::{LeaseBundleContext, LeaseBundleFacet, LeaseGraphBundleExt},
            core::ControlCore,
            graph::LeaseGraph,
        },
        control::types::Generation,
        observe::core::TapEvent,
        runtime::{
            config::{Config, CounterClock},
            consts::{DefaultLabelUniverse, RING_EVENTS},
        },
        transport::{TransportError, wire::Payload},
    };
    use core::{cell::UnsafeCell, mem::MaybeUninit};
    use std::{boxed::Box, thread_local};

    const MAX_RV: usize = 4;
    const TEST_SLAB_CAPACITY: usize = 8 * 1024;

    struct DummyTransport;

    impl crate::transport::Transport for DummyTransport {
        type Error = TransportError;
        type Tx<'a>
            = ()
        where
            Self: 'a;
        type Rx<'a>
            = ()
        where
            Self: 'a;
        type Metrics = ();

        fn open<'a>(&'a self, _local_role: u8, _session_id: u32) -> (Self::Tx<'a>, Self::Rx<'a>) {
            ((), ())
        }

        fn poll_send<'a, 'f>(
            &'a self,
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
        ) -> core::task::Poll<Result<Payload<'a>, Self::Error>> {
            core::task::Poll::Ready(Err(TransportError::Offline))
        }

        fn cancel_send<'a>(&'a self, _tx: &'a mut Self::Tx<'a>) {}

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

    type TestControlCore = ControlCore<
        'static,
        DummyTransport,
        DefaultLabelUniverse,
        CounterClock,
        crate::control::cap::mint::EpochTbl,
        MAX_RV,
    >;

    thread_local! {
        static TOPOLOGY_GRAPH: UnsafeCell<MaybeUninit<LeaseGraph<
            'static,
            TopologyLeaseSpec<
                DummyTransport,
                DefaultLabelUniverse,
                CounterClock,
                crate::control::cap::mint::EpochTbl,
            >,
        >>> = const { UnsafeCell::new(MaybeUninit::uninit()) };
    }

    fn test_config() -> Config<'static, DefaultLabelUniverse, CounterClock> {
        let tap = Box::leak(Box::new([TapEvent::zero(); RING_EVENTS]));
        let slab = Box::leak(Box::new([0u8; TEST_SLAB_CAPACITY]));
        Config::new(tap, slab).with_lane_range(0..3)
    }

    fn new_test_core() -> (TestControlCore, RendezvousId, RendezvousId) {
        let mut core = TestControlCore::new();
        let src_id = core
            .register_local_from_config(test_config(), DummyTransport, 0)
            .expect("register source rendezvous");
        let dst_id = core
            .register_local_from_config(test_config(), DummyTransport, 0)
            .expect("register destination rendezvous");
        (core, src_id, dst_id)
    }

    fn prepare_source_lane(
        core: &mut TestControlCore,
        src_id: RendezvousId,
        sid: SessionId,
        lane: Lane,
    ) {
        core.get_mut(&src_id)
            .expect("source rendezvous present")
            .prepare_topology_control_scope(lane)
            .expect("source topology storage must be available");
        core.get(&src_id)
            .expect("source rendezvous shared access")
            .activate_lane_attachment(sid, lane)
            .expect("source lane must attach for release-path validation");
    }

    #[test]
    fn topology_intent_validation() {
        let intent = TopologyIntent {
            src_rv: RendezvousId::new(1),
            dst_rv: RendezvousId::new(2),
            sid: 42,
            old_gen: Generation::new(1),
            new_gen: Generation::new(2),
            seq_tx: 0,
            seq_rx: 0,
            src_lane: Lane::new(0),
            dst_lane: Lane::new(1),
        };

        assert!(intent.new_gen.raw() > intent.old_gen.raw());
    }

    #[test]
    fn topology_ack_from_intent() {
        let intent = TopologyIntent {
            src_rv: RendezvousId::new(1),
            dst_rv: RendezvousId::new(2),
            sid: 42,
            old_gen: Generation::new(1),
            new_gen: Generation::new(2),
            seq_tx: 100,
            seq_rx: 200,
            src_lane: Lane::new(0),
            dst_lane: Lane::new(1),
        };

        let ack = TopologyAck::from_intent(&intent);

        assert_eq!(ack.src_rv, intent.src_rv);
        assert_eq!(ack.dst_rv, intent.dst_rv);
        assert_eq!(ack.sid, intent.sid);
        assert_eq!(ack.new_gen, intent.new_gen);
        assert_eq!(ack.src_lane, intent.src_lane);
        assert_eq!(ack.new_lane, intent.dst_lane);
        assert_eq!(ack.seq_tx, intent.seq_tx);
        assert_eq!(ack.seq_rx, intent.seq_rx);
    }

    #[test]
    fn begin_run_rejects_mismatched_source_rendezvous() {
        let (mut core, src_id, dst_id) = new_test_core();
        let intent = TopologyIntent {
            src_rv: dst_id,
            dst_rv: src_id,
            sid: 42,
            old_gen: Generation::ZERO,
            new_gen: Generation::new(1),
            seq_tx: 0,
            seq_rx: 0,
            src_lane: Lane::new(0),
            dst_lane: Lane::new(1),
        };

        let mut lease = core
            .lease::<TopologySpec>(src_id)
            .expect("lease source rendezvous");
        assert!(matches!(
            TopologyBeginAutomaton::run(&mut lease, intent),
            ControlStep::Abort(TopologyError::RendezvousIdMismatch)
        ));
    }

    #[test]
    fn commit_run_is_cluster_owned_and_does_not_mutate_source_lane() {
        let (mut core, src_id, dst_id) = new_test_core();
        let sid = SessionId::new(7);
        let src_lane = Lane::new(0);
        let dst_lane = Lane::new(1);
        let intent = TopologyIntent {
            src_rv: src_id,
            dst_rv: dst_id,
            sid: sid.raw(),
            old_gen: Generation::ZERO,
            new_gen: Generation::new(1),
            seq_tx: 17,
            seq_rx: 23,
            src_lane: src_lane,
            dst_lane: dst_lane,
        };

        prepare_source_lane(&mut core, src_id, sid, src_lane);

        {
            let mut lease = core
                .lease::<TopologySpec>(src_id)
                .expect("lease source rendezvous");
            assert!(matches!(
                TopologyBeginAutomaton::run(&mut lease, intent),
                ControlStep::Complete(_)
            ));
        }

        let mut mismatched_ack = TopologyAck::from_intent(&intent);
        mismatched_ack.src_lane = Lane::new(2);

        {
            let mut lease = core
                .lease::<TopologySpec>(src_id)
                .expect("lease source rendezvous for mismatched ack");
            assert!(matches!(
                TopologyCommitAutomaton::run(&mut lease, mismatched_ack),
                ControlStep::Abort(TopologyError::InvalidState)
            ));
        }

        assert_eq!(
            core.get(&src_id)
                .expect("source rendezvous shared access")
                .session_lane(sid),
            Some(src_lane),
            "mismatched ack must not release the source lane"
        );

        {
            let mut lease = core
                .lease::<TopologySpec>(src_id)
                .expect("lease source rendezvous for successful commit");
            assert!(matches!(
                TopologyCommitAutomaton::run(&mut lease, TopologyAck::from_intent(&intent)),
                ControlStep::Abort(TopologyError::InvalidState)
            ));
        }

        assert_eq!(
            core.get(&src_id)
                .expect("source rendezvous shared access")
                .session_lane(sid),
            Some(src_lane),
            "direct automaton commit must not bypass the cluster-owned topology commit path"
        );
    }

    #[test]
    fn commit_run_with_graph_is_cluster_owned() {
        let (mut core, src_id, dst_id) = new_test_core();
        let sid = SessionId::new(9);
        let src_lane = Lane::new(0);
        let dst_lane = Lane::new(1);
        let intent = TopologyIntent {
            src_rv: src_id,
            dst_rv: dst_id,
            sid: sid.raw(),
            old_gen: Generation::ZERO,
            new_gen: Generation::new(1),
            seq_tx: 31,
            seq_rx: 37,
            src_lane: src_lane,
            dst_lane: dst_lane,
        };

        prepare_source_lane(&mut core, src_id, sid, src_lane);

        {
            let mut lease = core
                .lease::<TopologySpec>(src_id)
                .expect("lease source rendezvous");
            assert!(matches!(
                TopologyBeginAutomaton::run(&mut lease, intent),
                ControlStep::Complete(_)
            ));
        }

        let mut root_ctx = LeaseBundleContext::from_control_core::<MAX_RV>(&mut core, src_id)
            .expect("root bundle context");
        root_ctx.set_topology(TopologyGraphContext::new(Some(intent)));
        TOPOLOGY_GRAPH.with(|graph_storage| unsafe {
            let graph_storage = &mut *graph_storage.get();
            LeaseGraph::<
                TopologyLeaseSpec<
                    DummyTransport,
                    DefaultLabelUniverse,
                    CounterClock,
                    crate::control::cap::mint::EpochTbl,
                >,
            >::init_new(
                graph_storage.as_mut_ptr(),
                src_id,
                LeaseBundleFacet::<
                    DummyTransport,
                    DefaultLabelUniverse,
                    CounterClock,
                    crate::control::cap::mint::EpochTbl,
                >::default(),
                root_ctx,
            );
            let graph = graph_storage.assume_init_mut();
            graph
                .add_child_with_bundle(&mut core, src_id, dst_id)
                .expect("graph child added");

            let mut mismatched_ack = TopologyAck::from_intent(&intent);
            mismatched_ack.src_lane = Lane::new(2);

            let mut lease = core
                .lease::<TopologySpec>(src_id)
                .expect("lease source rendezvous for graph commit");
            assert!(matches!(
                TopologyCommitAutomaton::run_with_graph(graph, &mut lease, mismatched_ack),
                ControlStep::Abort(TopologyError::InvalidState)
            ));
        });
    }
}
