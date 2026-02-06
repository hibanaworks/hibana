#![cfg(feature = "std")]

mod common;
mod support;

use common::TestTransport;
use hibana::{
    NoBinding,
    control::cluster::{CpError, SpliceOperands, error::SpliceError as CpSpliceError},
    control::{
        cap::{
            GenericCapToken, ResourceKind,
            resource_kinds::SpliceIntentKind,
        },
        cluster::{
            ControlPlaneAdapter, ControlPlaneSlot, CpCommand, CpEffect, DynamicResolution,
            EffectExecutor, ResolverContext, SessionCluster,
        },
        types::{LaneId as CpLaneId, RendezvousId, UniverseId},
    },
    endpoint::{ControlOutcome, SendError},
    g::{
        self, Msg, Role, StepCons, StepNil,
        steps::{ProjectRole, SendStep},
    },
    global::const_dsl::{DynamicMeta, HandlePlan},
    rendezvous::{Lane, Rendezvous, SessionId as RendezvousSessionId},
    runtime::{
        config::{Config, CounterClock},
        consts::{DefaultLabelUniverse, LABEL_SPLICE_INTENT, LabelUniverse},
    },
    transport::TransportSnapshot,
};
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};

use support::{leak_clock, leak_slab, leak_tap_storage, run_with_large_stack};

type Cluster = SessionCluster<'static, TestTransport, DefaultLabelUniverse, CounterClock, 4>;
type RendezvousHandle = Rendezvous<
    'static,
    'static,
    TestTransport,
    DefaultLabelUniverse,
    CounterClock,
    hibana::control::cap::EpochInit,
>;

// -----------------------------------------------------------------------------
// Global program definitions used by the harness
// -----------------------------------------------------------------------------
type Controller = Role<0>;
type Worker = Role<1>;

// ExternalControl allows cross-role communication (required for distributed splice)
type DelegateMsg = Msg<
    { LABEL_SPLICE_INTENT },
    GenericCapToken<SpliceIntentKind>,
    hibana::g::ExternalControl<SpliceIntentKind>,
>;
// AcceptMsg is a simple acknowledgment - the splice is already complete after DelegateMsg.
// We don't use SpliceAckKind here because that would trigger dispatch_splice_ack_with_view,
// which expects to be called from the destination cluster. The adapter already completed
// the splice via SpliceCommit during DelegateMsg processing.
// Use label 20 which is not in the reserved control label range (40-58).
const LABEL_ACCEPT: u8 = 20;
type AcceptMsg = Msg<{ LABEL_ACCEPT }, u64>;
type ResumeMsg = Msg<42, u64>;

// Cross-role steps for ExternalControl (Worker -> Controller, Controller -> Worker)
type DelegateSteps = SendStep<Worker, Controller, DelegateMsg>;
type AcceptSteps = SendStep<Controller, Worker, AcceptMsg>;
type ProtocolSteps = StepCons<DelegateSteps, StepCons<AcceptSteps, StepNil>>;
type ResumeSteps = StepCons<SendStep<Controller, Worker, ResumeMsg>, StepNil>;

const PROGRAM: g::Program<ProtocolSteps> = g::seq(
    g::with_control_plan(
        g::send::<Worker, Controller, DelegateMsg, 0>(),
        HandlePlan::dynamic(SPLICE_POLICY_ID, SPLICE_META),
    ),
    // AcceptMsg is a simple acknowledgment - no control plan needed
    g::send::<Controller, Worker, AcceptMsg, 0>(),
);
const RESUME_PROGRAM: g::Program<ResumeSteps> = g::send::<Controller, Worker, ResumeMsg, 0>();

static CONTROLLER_PROGRAM: g::RoleProgram<
    'static,
    0,
    <ProtocolSteps as ProjectRole<Controller>>::Output,
> = g::project::<0, ProtocolSteps, _>(&PROGRAM);
static WORKER_PROGRAM: g::RoleProgram<'static, 1, <ProtocolSteps as ProjectRole<Worker>>::Output> =
    g::project::<1, ProtocolSteps, _>(&PROGRAM);

static CONTROLLER_RESUME_PROGRAM: g::RoleProgram<
    'static,
    0,
    <ResumeSteps as ProjectRole<Controller>>::Output,
> = g::project::<0, ResumeSteps, _>(&RESUME_PROGRAM);
static WORKER_RESUME_PROGRAM: g::RoleProgram<
    'static,
    1,
    <ResumeSteps as ProjectRole<Worker>>::Output,
> = g::project::<1, ResumeSteps, _>(&RESUME_PROGRAM);

const SPLICE_POLICY_ID: u16 = 7;
const SPLICE_META: DynamicMeta = DynamicMeta::new();
const QUEUE_ALERT_DEPTH: u32 = 64;
const LATENCY_ALERT_US: u64 = 250_000;

static SPLICE_TARGET: OnceLock<Mutex<HashMap<RendezvousId, (RendezvousId, CpLaneId)>>> =
    OnceLock::new();
static TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn splice_target_map() -> &'static Mutex<HashMap<RendezvousId, (RendezvousId, CpLaneId)>> {
    SPLICE_TARGET.get_or_init(|| Mutex::new(HashMap::new()))
}

fn global_test_lock() -> &'static Mutex<()> {
    TEST_LOCK.get_or_init(|| Mutex::new(()))
}

fn transport_congested(snapshot: &TransportSnapshot) -> bool {
    snapshot
        .queue_depth
        .map(|depth| depth >= QUEUE_ALERT_DEPTH)
        .unwrap_or(false)
        || snapshot
            .latency_us
            .map(|lat| lat >= LATENCY_ALERT_US)
            .unwrap_or(false)
}

fn static_splice_resolver(
    _cluster: &Cluster,
    _meta: &DynamicMeta,
    ctx: ResolverContext,
) -> Result<DynamicResolution, ()> {
    if ctx.tag != SpliceIntentKind::TAG {
        return Err(());
    }
    if transport_congested(&ctx.metrics) {
        return Err(());
    }
    let guard = splice_target_map()
        .lock()
        .expect("splice target mutex poisoned");
    let (dst_rv, dst_lane) = guard.get(&ctx.rv_id).ok_or(())?;
    Ok(DynamicResolution::Splice {
        dst_rv: *dst_rv,
        dst_lane: hibana::control::types::LaneId::new(dst_lane.raw()),
        fences: None,
    })
}

// -----------------------------------------------------------------------------
// Harness wiring two clusters together through loopback adapters
// -----------------------------------------------------------------------------
struct MultiNodeHarness {
    cluster_a: &'static Cluster,
    cluster_b: &'static Cluster,
    rv_a: RendezvousId,
    rv_b: RendezvousId,
    adapter_a_to_b: &'static LoopbackAdapter<'static>,
    adapter_b_to_a: &'static LoopbackAdapter<'static>,
    transport_a: TestTransport,
    universe: UniverseId,
}

impl MultiNodeHarness {
    fn new() -> Self {
        let universe = UniverseId::new(0x5441_5021);
        let cluster_a: &'static Cluster = Box::leak(Box::new(SessionCluster::new(leak_clock())));
        let cluster_b: &'static Cluster = Box::leak(Box::new(SessionCluster::new(leak_clock())));

        let transport_a = TestTransport::default();
        let transport_b = TestTransport::default();

        let rv_a = cluster_a
            .add_rendezvous(Self::build_rendezvous(transport_a.clone()))
            .expect("register rendezvous A");
        let rv_b = cluster_b
            .add_rendezvous(Self::build_rendezvous(transport_b.clone()))
            .expect("register rendezvous B");

        let executor_a = cluster_a.get_local(&rv_a).expect("rv A registered");
        let executor_b = cluster_b.get_local(&rv_b).expect("rv B registered");
        let adapter_a_to_b = Box::leak(Box::new(LoopbackAdapter::new(
            executor_a,
            executor_b,
            rv_b,
            universe,
            <DefaultLabelUniverse as LabelUniverse>::MAX_LABEL,
        )));
        let adapter_b_to_a = Box::leak(Box::new(LoopbackAdapter::new(
            executor_b,
            executor_a,
            rv_a,
            universe,
            <DefaultLabelUniverse as LabelUniverse>::MAX_LABEL,
        )));

        Self {
            cluster_a,
            cluster_b,
            rv_a,
            rv_b,
            adapter_a_to_b,
            adapter_b_to_a,
            transport_a,
            universe,
        }
    }

    fn build_rendezvous(transport: TestTransport) -> RendezvousHandle {
        let tap = leak_tap_storage();
        let slab = leak_slab(2048);
        let config = Config::new(tap, slab);
        Rendezvous::from_config(config, transport)
    }

    fn register_remotes(&self) -> Result<(), CpError> {
        self.cluster_a
            .register_remote(ControlPlaneSlot::new(self.adapter_a_to_b))?;
        self.cluster_b
            .register_remote(ControlPlaneSlot::new(self.adapter_b_to_a))
    }

    fn hello_for(&self, rv: RendezvousId) -> Hello {
        Hello::new(
            rv,
            self.universe,
            <DefaultLabelUniverse as LabelUniverse>::MAX_LABEL,
        )
    }

    fn cluster_ids(&self) -> (RendezvousId, RendezvousId) {
        (self.rv_a, self.rv_b)
    }
}

struct LoopbackAdapter<'a> {
    source_executor: &'a dyn EffectExecutor,
    peer_executor: &'a dyn EffectExecutor,
    peer: RendezvousId,
    universe: UniverseId,
    max_label: u8,
}

impl<'a> LoopbackAdapter<'a> {
    fn new(
        source_executor: &'a dyn EffectExecutor,
        peer_executor: &'a dyn EffectExecutor,
        peer: RendezvousId,
        universe: UniverseId,
        max_label: u8,
    ) -> Self {
        Self {
            source_executor,
            peer_executor,
            peer,
            universe,
            max_label,
        }
    }
}

impl<'a> ControlPlaneAdapter for LoopbackAdapter<'a> {
    fn peer(&self) -> RendezvousId {
        self.peer
    }

    fn handshake(&self, _hello: &Hello) -> Result<Hello, CpError> {
        Ok(Hello::new(self.peer, self.universe, self.max_label))
    }

    fn run(&self, envelope: CpCommand) -> Result<(), CpError> {
        match envelope.effect {
            CpEffect::SpliceAck => {
                // Distributed splice protocol:
                // 1. Source sends SpliceAck command (containing intent) to destination
                // 2. Destination processes the intent via process_splice_intent()
                // 3. Destination sends SpliceCommit back to source
                //
                // This completes the splice in a single cross-cluster round-trip.
                let sid = envelope
                    .sid
                    .ok_or(CpError::Splice(CpSpliceError::InvalidSession))?;
                let ack = envelope
                    .ack
                    .ok_or(CpError::Splice(CpSpliceError::InvalidState))?;
                let intent = envelope
                    .intent
                    .ok_or(CpError::Splice(CpSpliceError::InvalidState))?;
                let operands = SpliceOperands::from_intent(&intent);

                // Process SpliceAck on destination (this handles the intent internally)
                let ack_result = self.peer_executor.run_effect(envelope);
                ack_result?;

                // Send SpliceCommit back to source to complete the splice
                let commit_cmd = CpCommand::splice_commit(sid, operands).with_ack(ack);
                let commit_result = self.source_executor.run_effect(commit_cmd);
                commit_result
            }
            CpEffect::SpliceCommit => self.source_executor.run_effect(envelope),
            _ => self.peer_executor.run_effect(envelope),
        }
    }
}

type Hello = hibana::control::cluster::ffi::Hello;

// -----------------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------------
#[test]
fn loopback_adapter_handshake_roundtrips() {
    let harness = MultiNodeHarness::new();
    harness
        .register_remotes()
        .expect("register remote adapters");

    let hello = harness.hello_for(harness.rv_a);
    let response = harness
        .adapter_b_to_a
        .handshake(&hello)
        .expect("handshake succeeds");

    let rv: RendezvousId = response.rv_id.into();
    let universe: UniverseId = response.universe_id.into();
    assert_eq!(rv, harness.rv_a, "adapter returns peer rendezvous id");
    assert_eq!(universe, harness.universe, "adapter preserves universe id");
    assert_eq!(
        response.version,
        hibana::control::cluster::ffi::ProtocolVersion::V1_0
    );
    assert_eq!(
        response.max_label,
        <DefaultLabelUniverse as LabelUniverse>::MAX_LABEL
    );
}

#[test]
fn clusters_register_loopback_adapters() {
    let harness = MultiNodeHarness::new();
    harness
        .register_remotes()
        .expect("register remote adapters");

    let (rv_a, rv_b) = harness.cluster_ids();
    let slot_a = harness
        .cluster_a
        .get_remote(&rv_b)
        .expect("cluster A stores remote B");
    assert_eq!(slot_a.adapter().peer(), rv_b);

    let slot_b = harness
        .cluster_b
        .get_remote(&rv_a)
        .expect("cluster B stores remote A");
    assert_eq!(slot_b.adapter().peer(), rv_a);

    // Establish explicit handshake both ways.
    let hello_a = harness.hello_for(rv_a);
    let response_ab = harness
        .cluster_a
        .handshake(rv_b, &hello_a)
        .expect("cluster A handshake succeeds");
    assert_eq!(RendezvousId::from(response_ab.rv_id), rv_b);

    let hello_b = harness.hello_for(rv_b);
    let response_ba = harness
        .cluster_b
        .handshake(rv_a, &hello_b)
        .expect("cluster B handshake succeeds");
    assert_eq!(RendezvousId::from(response_ba.rv_id), rv_a);
}

// ExternalControl version: cross-role communication triggers splice effect.
#[test]
fn distributed_splice_moves_lane_between_clusters() {
    run_with_large_stack(|| {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime")
            .block_on(async {
                let _test_guard = global_test_lock().lock().expect("test mutex");
                let harness = MultiNodeHarness::new();
                harness
                    .register_remotes()
                    .expect("register remote adapters");

                let (rv_a, rv_b) = harness.cluster_ids();
                {
                    let mut guard = splice_target_map().lock().expect("splice target mutex");
                    guard.clear();
                    // Splice to lane 0 to match RESUME_PROGRAM's lane specification
                    guard.insert(rv_a, (rv_b, CpLaneId::new(0)));
                }

                harness
                    .cluster_a
                    .handshake(rv_b, &harness.hello_for(rv_a))
                    .expect("cluster A handshake succeeds");
                harness
                    .cluster_b
                    .handshake(rv_a, &harness.hello_for(rv_b))
                    .expect("cluster B handshake succeeds");

                let intent_plan = WORKER_PROGRAM
                    .control_plans()
                    .find(|info| info.label == LABEL_SPLICE_INTENT)
                    .expect("splice intent plan");
                harness
                    .cluster_a
                    .register_control_plan_resolver(rv_a, &intent_plan, static_splice_resolver)
                    .expect("register intent resolver");
                // AcceptMsg is a simple acknowledgment - no control plan registration needed

                let sid_rv = RendezvousSessionId::new(99);
                let src_lane_rv = Lane::new(0);
                harness
                    .cluster_a
                    .get_local(&rv_a)
                    .expect("source rendezvous registered")
                    .port(sid_rv, src_lane_rv, 0)
                    .expect("seed session on source rendezvous");

                let mut controller = harness
                    .cluster_a
                    .attach_cursor::<0, _, _, _>(rv_a, sid_rv, &CONTROLLER_PROGRAM, NoBinding)
                    .expect("controller attach");
                let mut worker = harness
                    .cluster_a
                    .attach_cursor::<1, _, _, _>(rv_a, sid_rv, &WORKER_PROGRAM, NoBinding)
                    .expect("worker attach");

                #[cfg(feature = "test-utils")]
                assert!(
                    worker.phase_cursor().is_send(),
                    "worker cursor not in send state"
                );

                let delegate_token = GenericCapToken::<SpliceIntentKind>::AUTO;
                let (next_worker, delegate_outcome) = worker
                    .flow::<DelegateMsg>()
                    .unwrap()
                    .send(&delegate_token)
                    .await
                    .expect("delegate send");
                assert!(matches!(delegate_outcome, ControlOutcome::External(_)));
                worker = next_worker;
                let (next_controller, _intent) = controller
                    .recv::<DelegateMsg>()
                    .await
                    .expect("delegate recv");
                controller = next_controller;

                // AcceptMsg is a simple acknowledgment (u64 payload)
                let (next_controller, accept_outcome) = controller
                    .flow::<AcceptMsg>()
                    .unwrap()
                    .send(&1u64)  // Simple ack value
                    .await
                    .expect("accept send");
                assert!(matches!(accept_outcome, ControlOutcome::None));
                controller = next_controller;
                let (next_worker, _ack) = worker.recv::<AcceptMsg>().await.expect("accept recv");
                worker = next_worker;

                drop(worker);
                drop(controller);

                assert!(
                    harness
                        .cluster_a
                        .get_local(&rv_a)
                        .unwrap()
                        .association(sid_rv)
                        .is_none(),
                    "cluster A still tracks the session after splice"
                );
                assert!(
                    harness
                        .cluster_b
                        .get_local(&rv_b)
                        .unwrap()
                        .association(sid_rv)
                        .is_some(),
                    "cluster B did not take ownership of the session after splice"
                );

                let controller = harness
                    .cluster_b
                    .attach_cursor::<0, _, _, _>(rv_b, sid_rv, &CONTROLLER_RESUME_PROGRAM, NoBinding)
                    .expect("controller attach (resume)");
                let worker = harness
                    .cluster_b
                    .attach_cursor::<1, _, _, _>(rv_b, sid_rv, &WORKER_RESUME_PROGRAM, NoBinding)
                    .expect("worker attach (resume)");

                let (controller_next, outcome) = controller
                    .flow::<ResumeMsg>()
                    .unwrap()
                    .send(&777)
                    .await
                    .expect("resume send");
                assert!(matches!(outcome, ControlOutcome::None));
                drop(controller_next);
                let (worker_next, payload) = worker.recv::<ResumeMsg>().await.expect("resume recv");
                drop(worker_next);
                assert_eq!(payload, 777);
            });
    });
}

// ExternalControl version: resolver rejects when transport is congested.
#[test]
fn distributed_splice_aborts_when_transport_congested() {
    run_with_large_stack(|| {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime")
            .block_on(async {
                let _test_guard = global_test_lock().lock().expect("test mutex");
                let harness = MultiNodeHarness::new();
                harness
                    .register_remotes()
                    .expect("register remote adapters");

                let (rv_a, rv_b) = harness.cluster_ids();
                {
                    let mut guard = splice_target_map().lock().expect("splice target mutex");
                    guard.clear();
                    // Splice to lane 0 to match RESUME_PROGRAM's lane specification
                    guard.insert(rv_a, (rv_b, CpLaneId::new(0)));
                }

                harness
                    .cluster_a
                    .handshake(rv_b, &harness.hello_for(rv_a))
                    .expect("cluster A handshake succeeds");
                harness
                    .cluster_b
                    .handshake(rv_a, &harness.hello_for(rv_b))
                    .expect("cluster B handshake succeeds");

                harness.transport_a.set_metrics(TransportSnapshot::new(
                    Some(LATENCY_ALERT_US + 1),
                    Some(QUEUE_ALERT_DEPTH),
                ));

                let intent_plan = WORKER_PROGRAM
                    .control_plans()
                    .find(|info| info.label == LABEL_SPLICE_INTENT)
                    .expect("splice intent plan");
                harness
                    .cluster_a
                    .register_control_plan_resolver(rv_a, &intent_plan, static_splice_resolver)
                    .expect("register intent resolver");
                // AcceptMsg is a simple acknowledgment - no control plan registration needed

                let sid_rv = RendezvousSessionId::new(7);
                let src_lane_rv = Lane::new(0);
                harness
                    .cluster_a
                    .get_local(&rv_a)
                    .expect("source rendezvous registered")
                    .port(sid_rv, src_lane_rv, 0)
                    .expect("seed session on source rendezvous");

                let worker = harness
                    .cluster_a
                    .attach_cursor::<1, _, _, _>(rv_a, sid_rv, &WORKER_PROGRAM, NoBinding)
                    .expect("worker attach");

                let delegate_token = GenericCapToken::<SpliceIntentKind>::AUTO;
                let err = worker
                    .flow::<DelegateMsg>()
                    .unwrap()
                    .send(&delegate_token)
                    .await
                    .err()
                    .expect("policy abort when transport is congested");
                assert!(
                    matches!(err, SendError::PolicyAbort { .. })
                        || matches!(err, SendError::PhaseInvariant)
                );

                assert!(
                    harness
                        .cluster_a
                        .get_local(&rv_a)
                        .unwrap()
                        .association(sid_rv)
                        .is_some(),
                    "session should remain on source rendezvous after abort"
                );
                assert!(
                    harness
                        .cluster_b
                        .get_local(&rv_b)
                        .unwrap()
                        .association(sid_rv)
                        .is_none(),
                    "destination rendezvous must not take ownership when policy aborts"
                );
            });
    });
}
