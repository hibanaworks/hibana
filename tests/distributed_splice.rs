#![cfg(feature = "std")]

mod common;
mod support;

use common::TestTransport;
use hibana::global::const_dsl::HandlePlan;
use hibana::{
    NoBinding,
    control::{
        cap::{
            GenericCapToken, ResourceKind,
            resource_kinds::{SpliceAckKind, SpliceIntentKind},
        },
        cluster::{DynamicResolution, ResolverContext},
        types::{LaneId as CpLaneId, RendezvousId},
    },
    g::{
        self, Msg, Role, StepCons, StepNil,
        steps::{ProjectRole, SendStep},
    },
    global::const_dsl::DynamicMeta,
    rendezvous::{Lane, Rendezvous, SessionId as RendezvousSessionId},
    runtime::{SessionCluster, config::Config, consts::DefaultLabelUniverse},
    transport::TransportSnapshot,
};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use support::{leak_slab, leak_tap_storage};

fn run_with_large_stack<F, R>(f: F) -> R
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    const STACK: usize = 32 * 1024 * 1024;
    std::thread::Builder::new()
        .stack_size(STACK)
        .spawn(f)
        .expect("spawn large-stack thread")
        .join()
        .expect("join large-stack thread")
}

type Cluster = SessionCluster<
    'static,
    TestTransport,
    DefaultLabelUniverse,
    hibana::runtime::config::CounterClock,
    4,
>;

type Controller = Role<0>;
type Worker = Role<1>;

// ExternalControl allows cross-role communication (required for distributed splice)
type DelegateMsg = Msg<
    { hibana::runtime::consts::LABEL_SPLICE_INTENT },
    GenericCapToken<SpliceIntentKind>,
    hibana::g::ExternalControl<SpliceIntentKind>,
>;
type AcceptMsg = Msg<
    { hibana::runtime::consts::LABEL_SPLICE_ACK },
    GenericCapToken<SpliceAckKind>,
    hibana::g::ExternalControl<SpliceAckKind>,
>;

// Cross-role steps for ExternalControl (Worker -> Controller, Controller -> Worker)
type DelegateSteps = SendStep<Worker, Controller, DelegateMsg>;
type AcceptSteps = SendStep<Controller, Worker, AcceptMsg>;
type ProtocolSteps = StepCons<DelegateSteps, StepCons<AcceptSteps, StepNil>>;

const SPLICE_POLICY_ID: u16 = 7;
const SPLICE_META: DynamicMeta = DynamicMeta::new();
const QUEUE_ALERT_DEPTH: u32 = 64;
const LATENCY_ALERT_US: u64 = 250_000;

const PROGRAM: g::Program<ProtocolSteps> = g::seq(
    g::with_control_plan(
        g::send::<Worker, Controller, DelegateMsg, 0>(),
        HandlePlan::dynamic(SPLICE_POLICY_ID, SPLICE_META),
    ),
    g::with_control_plan(
        g::send::<Controller, Worker, AcceptMsg, 0>(),
        HandlePlan::dynamic(SPLICE_POLICY_ID, SPLICE_META),
    ),
);

static CONTROLLER_PROGRAM: g::RoleProgram<
    'static,
    0,
    <ProtocolSteps as ProjectRole<Controller>>::Output,
> = g::project::<0, ProtocolSteps, _>(&PROGRAM);

static WORKER_PROGRAM: g::RoleProgram<'static, 1, <ProtocolSteps as ProjectRole<Worker>>::Output> =
    g::project::<1, ProtocolSteps, _>(&PROGRAM);

static SPLICE_TARGET: OnceLock<Mutex<HashMap<RendezvousId, (RendezvousId, CpLaneId)>>> =
    OnceLock::new();

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
    // Handle both splice intent and splice ack tags
    if ctx.tag != SpliceIntentKind::TAG && ctx.tag != SpliceAckKind::TAG {
        return Err(());
    }
    // For intent, check congestion
    if ctx.tag == SpliceIntentKind::TAG && transport_congested(&ctx.metrics) {
        return Err(());
    }
    // Both intent and ack return Splice resolution with target info
    let guard = SPLICE_TARGET
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .expect("splice target mutex poisoned");
    let (dst_rv, dst_lane_cp) = guard.get(&ctx.rv_id).ok_or(())?;
    Ok(DynamicResolution::Splice {
        dst_rv: *dst_rv,
        dst_lane: hibana::control::types::LaneId::new(dst_lane_cp.raw()),
        fences: None,
    })
}

async fn distributed_splice_moves_lane_between_rendezvous_inner()
-> Result<(), Box<dyn std::error::Error>> {
    // Construct control-plane clusters and rendezvous instances.
    let cluster: &'static Cluster = Box::leak(Box::new(SessionCluster::new(support::leak_clock())));

    let transport_primary = TestTransport::default();
    let transport_secondary = TestTransport::default();

    let rendezvous_primary: Rendezvous<
        '_,
        '_,
        TestTransport,
        DefaultLabelUniverse,
        hibana::runtime::config::CounterClock,
    > = Rendezvous::from_config(
        Config::new(leak_tap_storage(), leak_slab(2048)),
        transport_primary.clone(),
    );
    let rendezvous_secondary: Rendezvous<
        '_,
        '_,
        TestTransport,
        DefaultLabelUniverse,
        hibana::runtime::config::CounterClock,
    > = Rendezvous::from_config(
        Config::new(leak_tap_storage(), leak_slab(2048)),
        transport_secondary.clone(),
    );

    let src_rv = cluster
        .add_rendezvous(rendezvous_primary)
        .expect("register primary rendezvous");
    let dst_rv = cluster
        .add_rendezvous(rendezvous_secondary)
        .expect("register secondary rendezvous");

    // Seed session state on the primary rendezvous.
    let sid_rv = RendezvousSessionId::new(99);
    let src_lane_rv = Lane::new(0);
    let dst_lane_rv = Lane::new(2);

    let intent_plan = WORKER_PROGRAM
        .control_plans()
        .find(|info| info.label == hibana::runtime::consts::LABEL_SPLICE_INTENT)
        .expect("splice intent control plan");
    SPLICE_TARGET
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .expect("splice target mutex poisoned")
        .insert(src_rv, (dst_rv, CpLaneId::new(dst_lane_rv.raw())));
    cluster
        .register_control_plan_resolver(src_rv, &intent_plan, static_splice_resolver)
        .expect("register splice intent resolver");

    let ack_plan = CONTROLLER_PROGRAM
        .control_plans()
        .find(|info| info.label == hibana::runtime::consts::LABEL_SPLICE_ACK)
        .expect("splice ack control plan");
    cluster
        .register_control_plan_resolver(src_rv, &ack_plan, static_splice_resolver)
        .expect("register splice ack resolver");

    cluster
        .get_local(&src_rv)
        .expect("primary rendezvous registered")
        .port(sid_rv, src_lane_rv, 0)
        .expect("seed session on source rendezvous");

    assert!(
        cluster
            .get_local(&src_rv)
            .unwrap()
            .association(sid_rv)
            .is_some(),
        "session should be registered before splice"
    );

    let mut controller = cluster
        .attach_cursor::<0, _, _, _>(src_rv, sid_rv, &CONTROLLER_PROGRAM, NoBinding)
        .expect("controller attach");
    let mut worker = cluster
        .attach_cursor::<1, _, _, _>(src_rv, sid_rv, &WORKER_PROGRAM, NoBinding)
        .expect("worker attach");

    // ExternalControl: Worker sends DelegateMsg to Controller (cross-role)
    let delegate_token = GenericCapToken::<SpliceIntentKind>::AUTO;
    let (next_worker, _) = worker
        .flow::<DelegateMsg>()
        .expect("delegate flow")
        .send(&delegate_token)
        .await
        .expect("send delegate");
    worker = next_worker;

    // Controller receives DelegateMsg from Worker
    let (next_controller, _delegate_payload) = controller
        .recv::<DelegateMsg>()
        .await
        .expect("recv delegate");
    controller = next_controller;

    // Controller sends AcceptMsg back to Worker (cross-role)
    let accept_token = GenericCapToken::<SpliceAckKind>::AUTO;
    let (next_controller, _) = controller
        .flow::<AcceptMsg>()
        .expect("accept flow")
        .send(&accept_token)
        .await
        .expect("send accept");
    controller = next_controller;

    // Worker receives AcceptMsg from Controller
    let (next_worker, _accept_payload) = worker.recv::<AcceptMsg>().await.expect("recv accept");
    worker = next_worker;

    // Cross-role communication complete - splice effect should now be executed

    drop(worker);
    drop(controller);

    // Source rendezvous releases the lane.
    assert!(
        cluster
            .get_local(&src_rv)
            .unwrap()
            .association(sid_rv)
            .is_none(),
        "source rendezvous still tracks the session after splice"
    );

    // Destination rendezvous now owns the association.
    let secondary_assoc = cluster
        .get_local(&dst_rv)
        .unwrap()
        .association(sid_rv)
        .expect("destination rendezvous should own the session");
    assert_eq!(secondary_assoc.lane, dst_lane_rv);
    assert_eq!(secondary_assoc.sid, sid_rv);

    let leftover = cluster
        .get_local(&src_rv)
        .expect("source rendezvous still registered")
        .take_cached_distributed_intent(sid_rv, dst_rv);
    assert!(
        leftover.is_none(),
        "distributed splice intent should be drained into splice graph context"
    );

    let _ = controller;
    let _ = worker;

    Ok(())
}

// ExternalControl version: cross-role communication triggers splice effect.
#[test]
fn distributed_splice_moves_lane_between_rendezvous() {
    run_with_large_stack(|| {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime")
            .block_on(async {
                distributed_splice_moves_lane_between_rendezvous_inner()
                    .await
                    .expect("distributed splice lane move");
            });
    });
}

async fn distributed_splice_aborts_when_transport_congested_inner()
-> Result<(), Box<dyn std::error::Error>> {
    let cluster: &'static Cluster = Box::leak(Box::new(SessionCluster::new(support::leak_clock())));

    let transport_primary = TestTransport::default();
    transport_primary.set_metrics(TransportSnapshot::new(
        Some(LATENCY_ALERT_US + 1),
        Some(QUEUE_ALERT_DEPTH),
    ));
    let transport_secondary = TestTransport::default();

    let rendezvous_primary: Rendezvous<
        '_,
        '_,
        TestTransport,
        DefaultLabelUniverse,
        hibana::runtime::config::CounterClock,
    > = Rendezvous::from_config(
        Config::new(leak_tap_storage(), leak_slab(2048)),
        transport_primary.clone(),
    );
    let rendezvous_secondary: Rendezvous<
        '_,
        '_,
        TestTransport,
        DefaultLabelUniverse,
        hibana::runtime::config::CounterClock,
    > = Rendezvous::from_config(
        Config::new(leak_tap_storage(), leak_slab(2048)),
        transport_secondary.clone(),
    );

    let src_rv = cluster
        .add_rendezvous(rendezvous_primary)
        .expect("register primary rendezvous");
    let dst_rv = cluster
        .add_rendezvous(rendezvous_secondary)
        .expect("register secondary rendezvous");

    let sid_rv = RendezvousSessionId::new(7);
    let src_lane_rv = Lane::new(0);
    let dst_lane_rv = Lane::new(2);

    let intent_plan = WORKER_PROGRAM
        .control_plans()
        .find(|info| info.label == hibana::runtime::consts::LABEL_SPLICE_INTENT)
        .expect("splice intent control plan");
    SPLICE_TARGET
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .expect("splice target mutex poisoned")
        .insert(src_rv, (dst_rv, CpLaneId::new(dst_lane_rv.raw())));
    cluster
        .register_control_plan_resolver(src_rv, &intent_plan, static_splice_resolver)
        .expect("register splice intent resolver");

    let ack_plan = CONTROLLER_PROGRAM
        .control_plans()
        .find(|info| info.label == hibana::runtime::consts::LABEL_SPLICE_ACK)
        .expect("splice ack control plan");
    cluster
        .register_control_plan_resolver(src_rv, &ack_plan, static_splice_resolver)
        .expect("register splice ack resolver");

    cluster
        .get_local(&src_rv)
        .expect("primary rendezvous registered")
        .port(sid_rv, src_lane_rv, 0)
        .expect("seed session on source rendezvous");

    let _controller = cluster
        .attach_cursor::<0, _, _, _>(src_rv, sid_rv, &CONTROLLER_PROGRAM, NoBinding)
        .expect("controller attach");
    let worker = cluster
        .attach_cursor::<1, _, _, _>(src_rv, sid_rv, &WORKER_PROGRAM, NoBinding)
        .expect("worker attach");

    // ExternalControl: send() should fail when resolver rejects due to transport congestion
    // (Token is auto-minted during send(), which invokes the resolver)
    let delegate_token = GenericCapToken::<SpliceIntentKind>::AUTO;
    let flow = worker.flow::<DelegateMsg>().expect("flow should succeed");
    let send_err = flow
        .send(&delegate_token)
        .await
        .err()
        .expect("send should fail when transport is congested");
    // SendError::PhaseInvariant (wrapping PolicyAbort from resolver)
    assert!(
        format!("{:?}", send_err).contains("PhaseInvariant"),
        "expected PhaseInvariant from resolver rejection, got: {:?}",
        send_err
    );

    assert!(
        cluster
            .get_local(&src_rv)
            .unwrap()
            .association(sid_rv)
            .is_some(),
        "session should remain on the source rendezvous after abort"
    );
    assert!(
        cluster
            .get_local(&dst_rv)
            .unwrap()
            .association(sid_rv)
            .is_none(),
        "destination rendezvous must not take ownership when policy aborts"
    );

    Ok(())
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
                distributed_splice_aborts_when_transport_congested_inner()
                    .await
                    .expect("distributed splice abort");
            });
    });
}
