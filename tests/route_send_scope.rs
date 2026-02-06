#![cfg(feature = "std")]
#![allow(dead_code)]

mod common;
mod support;

use common::TestTransport;
use hibana::NoBinding;
use hibana::control::{
    cap::{
        CapError, CapShot, CapsMask, EpochInit, GenericCapToken, ResourceKind, RouteDecisionHandle,
        SessionScopedKind, resource_kinds::RouteDecisionKind,
    },
    cluster::{DynamicResolution, ResolverContext},
    types::RendezvousId,
};
use hibana::endpoint::{ControlOutcome, CursorEndpoint};
use hibana::g::steps::{ProjectRole, SendStep, StepConcat, StepCons, StepNil};
use hibana::g::{self, MessageSpec, Msg, Role};
use hibana::global::const_dsl::{ControlScopeKind, DynamicMeta, HandlePlan, ScopeId};
use hibana::rendezvous::{Lane, Rendezvous, SessionId};
use hibana::runtime::{
    SessionCluster,
    config::{Config, CounterClock},
    consts::{DefaultLabelUniverse, LABEL_ROUTE_DECISION},
};
use support::{leak_clock, leak_slab, leak_tap_storage};

use hibana::observe::{ScopeTrace, normalise};
use std::{
    collections::BTreeMap,
    panic::{self, AssertUnwindSafe},
    sync::mpsc,
    thread,
    time::Duration,
};

type Controller = Role<0>;
type Worker = Role<1>;

const PRIMARY_ROUTE_POLICY_ID: u16 = 401;
const NESTED_OUTER_ROUTE_POLICY_ID: u16 = 402;
const NESTED_INNER_ROUTE_POLICY_ID: u16 = 403;
const SEND_ROUTE_POLICY_ID: u16 = 404;
const RETURN_ROUTE_POLICY_ID: u16 = 405;
const ROUTE_PLAN_META: DynamicMeta = DynamicMeta::new();

// Accept/Reject decision messages for nested routes - CanonicalControl self-send pattern
// Label 50 for Accept arm decision, Label 51 for Reject arm decision
type AcceptDecisionMsg =
    Msg<50, GenericCapToken<AcceptDecisionKind>, g::CanonicalControl<AcceptDecisionKind>>;
type RejectDecisionMsg =
    Msg<51, GenericCapToken<RejectDecisionKind>, g::CanonicalControl<RejectDecisionKind>>;

fn run_with_large_stack<F, Fut, R>(f: F) -> R
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = R>,
    R: Send + 'static,
{
    run_with_large_stack_inner_async(None, f)
}

fn run_with_large_stack_timeout<F, Fut, R>(timeout: Duration, f: F) -> R
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = R>,
    R: Send + 'static,
{
    run_with_large_stack_inner_async(Some(timeout), f)
}

fn run_with_large_stack_inner<F, R>(timeout: Option<Duration>, f: F) -> R
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    // Route/resolver fixtures build fairly large rendezvous/cluster structures; the closure body
    // needs more than the default 16 MiB when typestate metadata grows. Reserve a larger stack to
    // keep the regression tests stable.
    const STACK: usize = 64 * 1024 * 1024;
    let (tx, rx) = mpsc::channel();
    let handle = thread::Builder::new()
        .stack_size(STACK)
        .spawn(move || {
            let result = panic::catch_unwind(AssertUnwindSafe(f));
            let _ = tx.send(result);
        })
        .expect("spawn large-stack thread");
    let recv_result = match timeout {
        Some(deadline) => rx
            .recv_timeout(deadline)
            .unwrap_or_else(|_| panic!("large-stack thread timed out after {:?}", deadline)),
        None => rx
            .recv()
            .expect("large-stack thread dropped without result"),
    };
    handle
        .join()
        .expect("large-stack thread panicked before reporting result");
    match recv_result {
        Ok(value) => value,
        Err(panic) => panic::resume_unwind(panic),
    }
}

fn run_with_large_stack_inner_async<F, Fut, R>(timeout: Option<Duration>, f: F) -> R
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = R>,
    R: Send + 'static,
{
    const STACK: usize = 64 * 1024 * 1024;
    let (tx, rx) = mpsc::channel();
    let handle = thread::Builder::new()
        .stack_size(STACK)
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("build tokio runtime");
            let result = panic::catch_unwind(AssertUnwindSafe(|| rt.block_on(f())));
            let _ = tx.send(result);
        })
        .expect("spawn large-stack thread");
    let recv_result = match timeout {
        Some(deadline) => rx
            .recv_timeout(deadline)
            .unwrap_or_else(|_| panic!("large-stack thread timed out after {:?}", deadline)),
        None => rx
            .recv()
            .expect("large-stack thread dropped without result"),
    };
    handle
        .join()
        .expect("large-stack thread panicked before reporting result");
    match recv_result {
        Ok(value) => value,
        Err(panic) => panic::resume_unwind(panic),
    }
}

// ============================================================================
// Hibana Beauty: Self-send CanonicalControl + wire data (like nested_route_runtime.rs)
// ============================================================================
//
// This demonstrates hibana's route choreography where:
// 1. Controller sends self-send CanonicalControl (Controller → Controller) for route decision
// 2. Controller sends wire data to Worker
// 3. Worker uses offer() to discover which arm was selected
// 4. Type system ensures correct messages at each step
//
// route::<0, 1, _> means Controller=0 decides, Worker=1 receives data.
// CanonicalControl requires self-send (From == To) per hibana semantics.
// This matches the pattern in nested_route_runtime.rs.

// Control message for route decision (self-send: Controller → Controller)
type AcceptControlMsg = Msg<
    { LABEL_ROUTE_DECISION },
    GenericCapToken<RouteDecisionKind>,
    g::CanonicalControl<RouteDecisionKind>,
>;
type RejectControlMsg =
    Msg<11, GenericCapToken<RouteRightKind>, g::CanonicalControl<RouteRightKind>>;

// Data messages (wire: Controller → Worker)
type AcceptDataMsg = Msg<12, u32>;
type AcceptAckMsg = Msg<14, u32>; // Worker → Controller
type RejectDataMsg = Msg<13, u32>;
type FinalMsg = Msg<15, u32>;

// Accept arm: self-send control → data → ack
type AcceptControlStep = StepCons<SendStep<Controller, Controller, AcceptControlMsg>, StepNil>;
type AcceptDataStep = StepCons<SendStep<Controller, Worker, AcceptDataMsg>, StepNil>;
type AcceptAckStep = StepCons<SendStep<Worker, Controller, AcceptAckMsg>, StepNil>;
type AcceptArmSteps = <AcceptControlStep as StepConcat<
    <AcceptDataStep as StepConcat<AcceptAckStep>>::Output,
>>::Output;

// Reject arm: self-send control → data
type RejectControlStep = StepCons<SendStep<Controller, Controller, RejectControlMsg>, StepNil>;
type RejectDataStepOnly = StepCons<SendStep<Controller, Worker, RejectDataMsg>, StepNil>;
type RejectArmSteps = <RejectControlStep as StepConcat<RejectDataStepOnly>>::Output;

type RouteSteps = <AcceptArmSteps as StepConcat<RejectArmSteps>>::Output;

type FinalSteps = StepCons<SendStep<Worker, Controller, FinalMsg>, StepNil>;
type ProtocolSteps = <RouteSteps as StepConcat<FinalSteps>>::Output;

// Accept arm: self-send control with plan → data → ack
const ACCEPT_ARM: g::Program<AcceptArmSteps> = g::with_control_plan(
    g::send::<Controller, Controller, AcceptControlMsg, 0>(),
    HandlePlan::dynamic(PRIMARY_ROUTE_POLICY_ID, ROUTE_PLAN_META),
)
.then(g::send::<Controller, Worker, AcceptDataMsg, 0>())
.then(g::send::<Worker, Controller, AcceptAckMsg, 0>());

// Reject arm: self-send control with plan → data
const REJECT_ARM: g::Program<RejectArmSteps> = g::with_control_plan(
    g::send::<Controller, Controller, RejectControlMsg, 0>(),
    HandlePlan::dynamic(PRIMARY_ROUTE_POLICY_ID, ROUTE_PLAN_META),
)
.then(g::send::<Controller, Worker, RejectDataMsg, 0>());

// route::<0, _>: Controller=0 decides locally (self-send pattern like hibana-quic)
// Worker does NOT use offer() - directly receives DataMsg after Controller's local decision
const ROUTE: g::Program<RouteSteps> = g::route::<0, _>(
    g::route_chain::<0, AcceptArmSteps>(ACCEPT_ARM).and::<RejectArmSteps>(REJECT_ARM),
);

const PROGRAM: g::Program<ProtocolSteps> = ROUTE.then(g::send::<Worker, Controller, FinalMsg, 0>());

type NestedOuterLeftMsg = Msg<20, u32>;
type NestedInnerLeftMsg = Msg<21, u32>;
type NestedInnerRightMsg = Msg<22, u32>;
type NestedOuterRightMsg = Msg<23, u32>;
type NestedPostRouteMsg = Msg<24, u32>;

// Nested route steps: self-send for CanonicalControl, cross-role for data
type NestedInnerLeftSteps = StepCons<
    SendStep<Controller, Controller, AcceptDecisionMsg>,
    StepCons<SendStep<Controller, Worker, NestedInnerLeftMsg>, StepNil>,
>;
type NestedInnerRightSteps = StepCons<
    SendStep<Controller, Controller, RejectDecisionMsg>,
    StepCons<SendStep<Controller, Worker, NestedInnerRightMsg>, StepNil>,
>;
type NestedInnerRouteSteps = <NestedInnerLeftSteps as StepConcat<NestedInnerRightSteps>>::Output;

type NestedOuterLeftSteps = StepCons<
    SendStep<Controller, Controller, AcceptDecisionMsg>,
    StepCons<SendStep<Controller, Worker, NestedOuterLeftMsg>, NestedInnerRouteSteps>,
>;
type NestedOuterRightSteps = StepCons<
    SendStep<Controller, Controller, RejectDecisionMsg>,
    StepCons<SendStep<Controller, Worker, NestedOuterRightMsg>, StepNil>,
>;
type NestedRouteSteps = <NestedOuterLeftSteps as StepConcat<NestedOuterRightSteps>>::Output;
type NestedPostSteps = StepCons<SendStep<Controller, Worker, NestedPostRouteMsg>, StepNil>;
type NestedProtocolSteps = <NestedRouteSteps as StepConcat<NestedPostSteps>>::Output;

// Nested inner route: self-send for CanonicalControl
const NESTED_INNER_LEFT_CONTROL: g::Program<
    StepCons<SendStep<Controller, Controller, AcceptDecisionMsg, 0>, StepNil>,
> = g::with_control_plan(
    g::send::<Controller, Controller, AcceptDecisionMsg, 0>(),
    HandlePlan::dynamic(NESTED_INNER_ROUTE_POLICY_ID, ROUTE_PLAN_META),
);
const NESTED_INNER_LEFT: g::Program<NestedInnerLeftSteps> =
    NESTED_INNER_LEFT_CONTROL.then(g::send::<Controller, Worker, NestedInnerLeftMsg, 0>());
const NESTED_INNER_RIGHT_CONTROL: g::Program<
    StepCons<SendStep<Controller, Controller, RejectDecisionMsg, 0>, StepNil>,
> = g::with_control_plan(
    g::send::<Controller, Controller, RejectDecisionMsg, 0>(),
    HandlePlan::dynamic(NESTED_INNER_ROUTE_POLICY_ID, ROUTE_PLAN_META),
);
const NESTED_INNER_RIGHT: g::Program<NestedInnerRightSteps> =
    NESTED_INNER_RIGHT_CONTROL.then(g::send::<Controller, Worker, NestedInnerRightMsg, 0>());
const NESTED_INNER_ROUTE: g::Program<NestedInnerRouteSteps> = g::route::<0, _>(
    g::route_chain::<0, NestedInnerLeftSteps>(NESTED_INNER_LEFT)
        .and::<NestedInnerRightSteps>(NESTED_INNER_RIGHT),
);

// Nested outer route: self-send for CanonicalControl
const NESTED_OUTER_LEFT_CONTROL: g::Program<
    StepCons<SendStep<Controller, Controller, AcceptDecisionMsg, 0>, StepNil>,
> = g::with_control_plan(
    g::send::<Controller, Controller, AcceptDecisionMsg, 0>(),
    HandlePlan::dynamic(NESTED_OUTER_ROUTE_POLICY_ID, ROUTE_PLAN_META),
);
const NESTED_OUTER_LEFT: g::Program<NestedOuterLeftSteps> = NESTED_OUTER_LEFT_CONTROL
    .then(g::send::<Controller, Worker, NestedOuterLeftMsg, 0>())
    .then(NESTED_INNER_ROUTE);
const NESTED_OUTER_RIGHT_CONTROL: g::Program<
    StepCons<SendStep<Controller, Controller, RejectDecisionMsg, 0>, StepNil>,
> = g::with_control_plan(
    g::send::<Controller, Controller, RejectDecisionMsg, 0>(),
    HandlePlan::dynamic(NESTED_OUTER_ROUTE_POLICY_ID, ROUTE_PLAN_META),
);
const NESTED_OUTER_RIGHT: g::Program<NestedOuterRightSteps> =
    NESTED_OUTER_RIGHT_CONTROL.then(g::send::<Controller, Worker, NestedOuterRightMsg, 0>());
const NESTED_ROUTE: g::Program<NestedRouteSteps> = g::route::<0, _>(
    g::route_chain::<0, NestedOuterLeftSteps>(NESTED_OUTER_LEFT)
        .and::<NestedOuterRightSteps>(NESTED_OUTER_RIGHT),
);
const NESTED_PROGRAM: g::Program<NestedProtocolSteps> =
    NESTED_ROUTE.then(g::send::<Controller, Worker, NestedPostRouteMsg, 0>());

type SendRouteFirstLeftMsg = Msg<30, u32>;
type SendRouteFirstRightMsg = Msg<31, u32>;
type ReturnRouteLeftMsg = Msg<32, u32>;
type ReturnRouteRightMsg = Msg<33, u32>;

// Send route: Controller self-send control → Controller sends data to Worker
type SendRouteFirstLeftSteps = StepCons<
    SendStep<Controller, Controller, AcceptDecisionMsg>,
    StepCons<SendStep<Controller, Worker, SendRouteFirstLeftMsg>, StepNil>,
>;
type SendRouteFirstRightSteps = StepCons<
    SendStep<Controller, Controller, RejectDecisionMsg>,
    StepCons<SendStep<Controller, Worker, SendRouteFirstRightMsg>, StepNil>,
>;
type SendRouteSteps = <SendRouteFirstLeftSteps as StepConcat<SendRouteFirstRightSteps>>::Output;

// Return route: Worker self-send control → Worker sends data to Controller
type ReturnRouteLeftSteps = StepCons<
    SendStep<Worker, Worker, AcceptDecisionMsg>,
    StepCons<SendStep<Worker, Controller, ReturnRouteLeftMsg>, StepNil>,
>;
type ReturnRouteRightSteps = StepCons<
    SendStep<Worker, Worker, RejectDecisionMsg>,
    StepCons<SendStep<Worker, Controller, ReturnRouteRightMsg>, StepNil>,
>;
type ReturnRouteSteps = <ReturnRouteLeftSteps as StepConcat<ReturnRouteRightSteps>>::Output;

type SendThenRecvSteps = <SendRouteSteps as StepConcat<ReturnRouteSteps>>::Output;

const SEND_ROUTE_LEFT_CONTROL: g::Program<
    StepCons<SendStep<Controller, Controller, AcceptDecisionMsg, 0>, StepNil>,
> = g::with_control_plan(
    g::send::<Controller, Controller, AcceptDecisionMsg, 0>(),
    HandlePlan::dynamic(SEND_ROUTE_POLICY_ID, ROUTE_PLAN_META),
);
const SEND_ROUTE_LEFT: g::Program<SendRouteFirstLeftSteps> =
    SEND_ROUTE_LEFT_CONTROL.then(g::send::<Controller, Worker, SendRouteFirstLeftMsg, 0>());
const SEND_ROUTE_RIGHT_CONTROL: g::Program<
    StepCons<SendStep<Controller, Controller, RejectDecisionMsg, 0>, StepNil>,
> = g::with_control_plan(
    g::send::<Controller, Controller, RejectDecisionMsg, 0>(),
    HandlePlan::dynamic(SEND_ROUTE_POLICY_ID, ROUTE_PLAN_META),
);
const SEND_ROUTE_RIGHT: g::Program<SendRouteFirstRightSteps> =
    SEND_ROUTE_RIGHT_CONTROL.then(g::send::<Controller, Worker, SendRouteFirstRightMsg, 0>());
const SEND_ROUTE_FIRST: g::Program<SendRouteSteps> = g::route::<0, _>(
    g::route_chain::<0, SendRouteFirstLeftSteps>(SEND_ROUTE_LEFT)
        .and::<SendRouteFirstRightSteps>(SEND_ROUTE_RIGHT),
);

const RETURN_ROUTE_LEFT_CONTROL: g::Program<
    StepCons<SendStep<Worker, Worker, AcceptDecisionMsg, 0>, StepNil>,
> = g::with_control_plan(
    g::send::<Worker, Worker, AcceptDecisionMsg, 0>(),
    HandlePlan::dynamic(RETURN_ROUTE_POLICY_ID, ROUTE_PLAN_META),
);
const RETURN_ROUTE_LEFT: g::Program<ReturnRouteLeftSteps> =
    RETURN_ROUTE_LEFT_CONTROL.then(g::send::<Worker, Controller, ReturnRouteLeftMsg, 0>());
const RETURN_ROUTE_RIGHT_CONTROL: g::Program<
    StepCons<SendStep<Worker, Worker, RejectDecisionMsg, 0>, StepNil>,
> = g::with_control_plan(
    g::send::<Worker, Worker, RejectDecisionMsg, 0>(),
    HandlePlan::dynamic(RETURN_ROUTE_POLICY_ID, ROUTE_PLAN_META),
);
const RETURN_ROUTE_RIGHT: g::Program<ReturnRouteRightSteps> =
    RETURN_ROUTE_RIGHT_CONTROL.then(g::send::<Worker, Controller, ReturnRouteRightMsg, 0>());
const RETURN_ROUTE: g::Program<ReturnRouteSteps> = g::route::<1, _>(
    g::route_chain::<1, ReturnRouteLeftSteps>(RETURN_ROUTE_LEFT)
        .and::<ReturnRouteRightSteps>(RETURN_ROUTE_RIGHT),
);

const SEND_THEN_RECV_PROGRAM: g::Program<SendThenRecvSteps> = SEND_ROUTE_FIRST.then(RETURN_ROUTE);

static CONTROLLER_PROGRAM: g::RoleProgram<
    'static,
    0,
    <ProtocolSteps as ProjectRole<Controller>>::Output,
> = g::project::<0, ProtocolSteps, _>(&PROGRAM);

static WORKER_PROGRAM: g::RoleProgram<'static, 1, <ProtocolSteps as ProjectRole<Worker>>::Output> =
    g::project::<1, ProtocolSteps, _>(&PROGRAM);

static NESTED_CONTROLLER_PROGRAM: g::RoleProgram<
    'static,
    0,
    <NestedProtocolSteps as ProjectRole<Controller>>::Output,
> = g::project::<0, NestedProtocolSteps, _>(&NESTED_PROGRAM);

static NESTED_WORKER_PROGRAM: g::RoleProgram<
    'static,
    1,
    <NestedProtocolSteps as ProjectRole<Worker>>::Output,
> = g::project::<1, NestedProtocolSteps, _>(&NESTED_PROGRAM);

static SEND_RECV_CONTROLLER_PROGRAM: g::RoleProgram<
    'static,
    0,
    <SendThenRecvSteps as ProjectRole<Controller>>::Output,
> = g::project::<0, SendThenRecvSteps, _>(&SEND_THEN_RECV_PROGRAM);

static SEND_RECV_WORKER_PROGRAM: g::RoleProgram<
    'static,
    1,
    <SendThenRecvSteps as ProjectRole<Worker>>::Output,
> = g::project::<1, SendThenRecvSteps, _>(&SEND_THEN_RECV_PROGRAM);

type Cluster = SessionCluster<'static, TestTransport, DefaultLabelUniverse, CounterClock, 4>;

type ControllerEndpoint =
    CursorEndpoint<'static, 0, TestTransport, DefaultLabelUniverse, CounterClock, EpochInit, 4>;
type WorkerEndpoint =
    CursorEndpoint<'static, 1, TestTransport, DefaultLabelUniverse, CounterClock, EpochInit, 4>;

// ============================================================================
// Hibana Beauty Test: Self-send control + wire data (Accept arm)
// ============================================================================
//
// This test demonstrates hibana's route choreography design:
// 1. Controller uses local::<AcceptControlMsg>().apply() for route decision (self-send)
// 2. Controller sends AcceptDataMsg to Worker (wire data)
// 3. Worker uses offer() to discover the selected arm
// 4. Worker acks back to Controller
// 5. Final message after route scope
//
// Key insight: CanonicalControl is self-send (Controller → Controller), processed locally.
// Worker's offer() receives the first wire message (AcceptDataMsg) after control skip.
// This matches the pattern in nested_route_runtime.rs.
#[tokio::test]
async fn controller_advances_after_route_branch() {
    let tap_buf = leak_tap_storage();
    let slab = leak_slab(2048);
    let config = Config::new(tap_buf, slab);
    let transport = TestTransport::default();
    let rendezvous: Rendezvous<'_, '_, TestTransport, DefaultLabelUniverse, CounterClock> =
        Rendezvous::from_config(config, transport.clone());

    let cluster: &mut Cluster = Box::leak(Box::new(SessionCluster::new(leak_clock())));
    let rv_id = cluster.add_rendezvous(rendezvous).expect("register rv");

    // Register resolver for both roles (arm=0 for Accept)
    register_route_resolvers_for_program(&*cluster, rv_id, &CONTROLLER_PROGRAM);
    register_route_resolvers_for_program(&*cluster, rv_id, &WORKER_PROGRAM);

    let sid = SessionId::new(99);

    let mut controller: ControllerEndpoint = cluster
        .attach_cursor::<0, _, _, _>(rv_id, sid, &CONTROLLER_PROGRAM, NoBinding)
        .expect("attach controller");
    let mut worker: WorkerEndpoint = cluster
        .attach_cursor::<1, _, _, _>(rv_id, sid, &WORKER_PROGRAM, NoBinding)
        .expect("attach worker");

    // =========================================================================
    // Step 1: Controller sends self-send control via flow().send(()) (route decision)
    // =========================================================================
    let (controller_after_control, outcome) = controller
        .flow::<AcceptControlMsg>()
        .expect("accept control flow")
        .send(())
        .await
        .expect("send accept control");
    assert!(matches!(outcome, ControlOutcome::Canonical(_)));
    controller = controller_after_control;

    // =========================================================================
    // Step 2: Controller sends AcceptDataMsg (wire data)
    // =========================================================================
    let (controller_after_data, _outcome) = controller
        .flow::<AcceptDataMsg>()
        .expect("data flow")
        .send(&1001)
        .await
        .expect("send accept data");
    controller = controller_after_data;

    // =========================================================================
    // Step 3: Worker offers route arm, then decodes AcceptDataMsg
    // =========================================================================
    let worker_branch = worker.offer().await.expect("offer accept arm");
    assert_eq!(
        worker_branch.label(),
        <AcceptDataMsg as MessageSpec>::LABEL,
        "accept arm exposes AcceptDataMsg"
    );
    let (worker_after_data, data_value) = worker_branch
        .decode::<AcceptDataMsg>()
        .await
        .expect("decode accept data");
    assert_eq!(data_value, 1001);
    worker = worker_after_data;

    // =========================================================================
    // Step 4: Worker acks back to Controller
    // =========================================================================
    let (worker_after_ack, _) =
        worker.flow::<AcceptAckMsg>().expect("ack flow").send(&777).await.expect("send ack");
    worker = worker_after_ack;

    // =========================================================================
    // Step 5: Controller receives ack
    // =========================================================================
    let (controller_after_ack, ack) =
        controller.recv::<AcceptAckMsg>().await.expect("recv ack");
    assert_eq!(ack, 777);
    controller = controller_after_ack;

    // =========================================================================
    // Step 6: Final message (after route scope ends)
    // =========================================================================
    let (_worker_after_final, _) =
        worker.flow::<FinalMsg>().expect("final flow").send(&888).await.expect("send final");

    let (_, final_value) = controller.recv::<FinalMsg>().await.expect("recv final");
    assert_eq!(final_value, 888);
}

// ============================================================================
// Route Reject Arm Test: route::<0, 0> passive observer limitation
// ============================================================================
//
// NOTE: With route::<0, 0>, Worker's typestate projection is computed at compile time.
// The projection contains recv indices only for arms that are statically reachable.
// For passive observers (roles that are neither controller nor target), the projection
// typically contains only arm=0 recv indices.
//
// To dynamically select different arms at runtime, use route::<0, 1> where Worker
// is the target and can discover arm selection via control messages.
//
// This test validates the fix for passive observer route arm tracking.
// The typestate now correctly tracks arm boundaries from other roles' self-send
// messages, and route_scope_arm_end_index computes arm count by scanning
// typestate when route_recv_len == 0 (for passive observers).
#[test]
fn route_reject_arm_typestate() {
    run_with_large_stack_timeout(Duration::from_secs(30), || async {
        let tap_buf = leak_tap_storage();
        let slab = leak_slab(2048);
        let config = Config::new(tap_buf, slab);
        let transport = TestTransport::default();
        let rendezvous: Rendezvous<'_, '_, TestTransport, DefaultLabelUniverse, CounterClock> =
            Rendezvous::from_config(config, transport.clone());

        let cluster: &mut Cluster = Box::leak(Box::new(SessionCluster::new(leak_clock())));
        let rv_id = cluster.add_rendezvous(rendezvous).expect("register rv");

        // Register resolver that returns arm=1 (Reject arm) for both roles
        for info in CONTROLLER_PROGRAM.control_plans() {
            if info.plan.is_dynamic() {
                cluster
                    .register_control_plan_resolver(rv_id, &info, always_right_route_resolver)
                    .expect("register reject resolver for controller");
            }
        }
        for info in WORKER_PROGRAM.control_plans() {
            if info.plan.is_dynamic() {
                cluster
                    .register_control_plan_resolver(rv_id, &info, always_right_route_resolver)
                    .expect("register reject resolver for worker");
            }
        }

        let sid = SessionId::new(100);

        let mut controller: ControllerEndpoint = cluster
            .attach_cursor::<0, _, _, _>(rv_id, sid, &CONTROLLER_PROGRAM, NoBinding)
            .expect("attach controller");
        let mut worker: WorkerEndpoint = cluster
            .attach_cursor::<1, _, _, _>(rv_id, sid, &WORKER_PROGRAM, NoBinding)
            .expect("attach worker");

        // =========================================================================
        // Step 1: Controller sends self-send control via flow().send(()) (route decision for Reject)
        // =========================================================================
        let (controller_after_control, outcome) = controller
            .flow::<RejectControlMsg>()
            .expect("reject control flow")
            .send(())
            .await
            .expect("send reject control");
        assert!(matches!(outcome, ControlOutcome::Canonical(_)));
        controller = controller_after_control;

        // =========================================================================
        // Step 2: Controller sends RejectDataMsg (wire data)
        // =========================================================================
        let (controller_after_data, _outcome) = controller
            .flow::<RejectDataMsg>()
            .expect("reject data flow")
            .send(&2002)
            .await
            .expect("send reject data");
        controller = controller_after_data;

        // =========================================================================
    // Step 3: Worker offers reject arm, then decodes RejectDataMsg
    // =========================================================================
    let worker_branch = worker.offer().await.expect("offer reject arm");
    assert_eq!(
        worker_branch.label(),
        <RejectDataMsg as MessageSpec>::LABEL,
        "reject arm exposes RejectDataMsg"
    );
    let (worker_after_data, data_value) = worker_branch
        .decode::<RejectDataMsg>()
        .await
        .expect("decode reject data");
        assert_eq!(data_value, 2002);
        worker = worker_after_data;

        // =========================================================================
        // Step 4: Final message (Reject arm has no ack step)
        // =========================================================================
        let (_worker_after_final, _) =
            worker.flow::<FinalMsg>().expect("final flow").send(&999).await.expect("send final");

        let (_, final_value) = controller.recv::<FinalMsg>().await.expect("recv final");
        assert_eq!(final_value, 999);
    });
}

fn always_right_route_resolver(
    _cluster: &Cluster,
    _meta: &DynamicMeta,
    _ctx: ResolverContext,
) -> Result<DynamicResolution, ()> {
    Ok(DynamicResolution::RouteArm { arm: 1 })
}

// Test that tap events include scope metadata with route::<0, 0> pattern
#[test]
fn endpoint_tap_events_include_scope_metadata() {
    run_with_large_stack(|| async {
        let tap_buf = leak_tap_storage();
        let slab = leak_slab(2048);
        let config = Config::new(tap_buf, slab);
        let transport = TestTransport::default();
        let rendezvous: Rendezvous<'_, '_, TestTransport, DefaultLabelUniverse, CounterClock> =
            Rendezvous::from_config(config, transport.clone());

        let cluster: &mut Cluster = Box::leak(Box::new(SessionCluster::new(leak_clock())));
        let rv_id = cluster.add_rendezvous(rendezvous).expect("register rv");

        register_route_resolvers_for_program(&*cluster, rv_id, &CONTROLLER_PROGRAM);
        register_route_resolvers_for_program(&*cluster, rv_id, &WORKER_PROGRAM);

        let sid = SessionId::new(120);

        let mut controller_ep: ControllerEndpoint = cluster
            .attach_cursor::<0, _, _, _>(rv_id, sid, &CONTROLLER_PROGRAM, NoBinding)
            .expect("attach controller");
        let worker_ep: WorkerEndpoint = cluster
            .attach_cursor::<1, _, _, _>(rv_id, sid, &WORKER_PROGRAM, NoBinding)
            .expect("attach worker");

        #[cfg(feature = "test-utils")]
        let cursor = controller_ep.phase_cursor();
        #[cfg(feature = "test-utils")]
        let scope_region = cursor.scope_region().expect("route scope region");
        #[cfg(feature = "test-utils")]
        let expected_range = scope_region.range;
        #[cfg(feature = "test-utils")]
        let expected_nest = scope_region.nest;

        let start_head = cluster
            .get_local(&rv_id)
            .expect("rendezvous ref")
            .tap()
            .head();

        // Controller: local decision (self-send) using flow().send(())
        let (controller_after_control, outcome) = controller_ep
            .flow::<AcceptControlMsg>()
            .expect("accept control flow")
            .send(())
            .await
            .expect("send accept control");
        assert!(matches!(outcome, ControlOutcome::Canonical(_)));
        controller_ep = controller_after_control;

        // Controller: send AcceptDataMsg (wire data)
        let (_controller_after_data, _outcome) = controller_ep
            .flow::<AcceptDataMsg>()
            .expect("data flow")
            .send(&1001)
            .await
            .expect("send accept data");

        // Worker: offer route arm, then decode AcceptDataMsg
        let worker_branch = worker_ep.offer().await.expect("offer accept arm");
        assert_eq!(
            worker_branch.label(),
            <AcceptDataMsg as MessageSpec>::LABEL,
            "accept arm exposes AcceptDataMsg"
        );
        let (worker_after_data, data_value) = worker_branch
            .decode::<AcceptDataMsg>()
            .await
            .expect("decode accept data");
        assert_eq!(data_value, 1001);
        let _worker_ep = worker_after_data;

        let tap_ring = cluster.get_local(&rv_id).expect("rendezvous ref").tap();
        let end_head = tap_ring.head();
        let storage = tap_ring.as_slice();
        let endpoint_events = normalise::endpoint_trace(storage, start_head, end_head);
        let (policy_lane, failures) = normalise::policy_lane_trace(storage, start_head, end_head);
        assert!(
            failures.is_empty(),
            "route decision trace should not emit local action failures: {:?}",
            failures
        );
        let atlas: Vec<_> = CONTROLLER_PROGRAM.scope_regions().collect();
        let _correlated = normalise::correlate_scope_traces_with_atlas(
            &endpoint_events,
            &policy_lane,
            &[],
            &atlas,
        );
        #[cfg(feature = "test-utils")]
        {
            let expected_trace = ScopeTrace::new(expected_range, expected_nest);
            let annotated = correlated
                .get(&expected_trace)
                .expect("correlated entry for controller route scope");
            assert_eq!(
                annotated.region.as_ref().map(|region| region.scope_id),
                Some(scope_region.scope_id),
                "correlated scope region should match typestate scope id"
            );
            // With route::<0, 0>, the control message is local (self-send), so we check for
            // the AcceptControlMsg label in endpoint trace
            assert!(
                annotated.traces.endpoint.iter().any(|event| matches!(
                    event,
                    normalise::EndpointEvent::Control { sid: event_sid, label, .. }
                        if *event_sid == sid.raw() && *label == <AcceptControlMsg as MessageSpec>::LABEL
                )),
                "expected control send event to be tracked in endpoint trace"
            );
        }
    });
}

// ============================================================================
// Nested Route Test: self-send control + wire data (route::<0, 0> pattern)
// ============================================================================
//
// This test demonstrates nested routes with hibana's route::<0, 0> pattern:
// 1. Controller sends outer route decision (self-send via flow().send())
// 2. Controller sends outer data to Worker (wire data)
// 3. Worker directly receives outer data - resolver determines arm
// 4. Controller sends inner route decision (self-send via flow().send())
// 5. Controller sends inner data to Worker (wire data)
// 6. Worker directly receives inner data - resolver determines arm
// 7. Controller sends post-route message
//
// Key insight: With route::<0, 0>, Worker uses resolver to determine arm, not wire label.
// This is the hibana way - typestate + resolver, not wire-level labels.
#[test]
fn nested_route_stack_tracks_scope() {
    run_with_large_stack(|| async {
        let tap_buf = leak_tap_storage();
        let slab = leak_slab(2048);
        let config = Config::new(tap_buf, slab);
        let transport = TestTransport::default();
        let rendezvous: Rendezvous<'_, '_, TestTransport, DefaultLabelUniverse, CounterClock> =
            Rendezvous::from_config(config, transport.clone());

        let cluster: &mut Cluster = Box::leak(Box::new(SessionCluster::new(leak_clock())));
        let rv_id = cluster.add_rendezvous(rendezvous).expect("register rv");

        register_route_resolvers_for_program(&*cluster, rv_id, &NESTED_CONTROLLER_PROGRAM);
        register_route_resolvers_for_program(&*cluster, rv_id, &NESTED_WORKER_PROGRAM);

        let sid = SessionId::new(100);

        let mut controller: ControllerEndpoint = cluster
            .attach_cursor::<0, _, _, _>(rv_id, sid, &NESTED_CONTROLLER_PROGRAM, NoBinding)
            .expect("attach controller");
        let mut worker: WorkerEndpoint = cluster
            .attach_cursor::<1, _, _, _>(rv_id, sid, &NESTED_WORKER_PROGRAM, NoBinding)
            .expect("attach worker");

        let start_head = cluster
            .get_local(&rv_id)
            .expect("rendezvous ref")
            .tap()
            .head();
        #[cfg(feature = "test-utils")]
        let outer_scope_region = controller
            .phase_cursor()
            .scope_region()
            .expect("outer route scope region");

        // =========================================================================
        // Outer route: Controller self-send control (flow().send(()))
        // =========================================================================
        let (controller_after_outer_ctrl, outcome) = controller
            .flow::<AcceptDecisionMsg>()
            .expect("outer-left control flow")
            .send(())
            .await
            .expect("send outer-left control");
        assert!(matches!(outcome, ControlOutcome::Canonical(_)));
        controller = controller_after_outer_ctrl;

        // =========================================================================
        // Outer route: Controller sends wire data to Worker
        // =========================================================================
        let (controller_after_outer, _outcome) = controller
            .flow::<NestedOuterLeftMsg>()
            .expect("outer-left data flow")
            .send(&111)
            .await
            .expect("send outer-left data");
        controller = controller_after_outer;

        // =========================================================================
        // Outer route: Worker offers route arm, then decodes outer data
        // =========================================================================
        let worker_branch = worker.offer().await.expect("offer outer route");
        assert_eq!(
            worker_branch.label(),
            <NestedOuterLeftMsg as MessageSpec>::LABEL,
            "outer route exposes NestedOuterLeftMsg"
        );
        let (worker_after_outer, observed_outer) = worker_branch
            .decode::<NestedOuterLeftMsg>()
            .await
            .expect("decode outer data");
        assert_eq!(observed_outer, 111);
        worker = worker_after_outer;

        #[cfg(feature = "test-utils")]
        let inner_scope_region = controller
            .phase_cursor()
            .scope_region()
            .expect("inner route scope region");

        // =========================================================================
        // Inner route: Controller self-send control (flow().send(()))
        // =========================================================================
        let (controller_after_inner_ctrl, outcome) = controller
            .flow::<AcceptDecisionMsg>()
            .expect("inner-left control flow")
            .send(())
            .await
            .expect("send inner-left control");
        assert!(matches!(outcome, ControlOutcome::Canonical(_)));
        controller = controller_after_inner_ctrl;

        // =========================================================================
        // Inner route: Controller sends wire data to Worker
        // =========================================================================
        let (controller_after_inner, _outcome) = controller
            .flow::<NestedInnerLeftMsg>()
            .expect("inner-left data flow")
            .send(&222)
            .await
            .expect("send inner-left data");
        controller = controller_after_inner;

        // =========================================================================
        // Inner route: Worker offers route arm, then decodes inner data
        // =========================================================================
        let worker_branch = worker.offer().await.expect("offer inner route");
        assert_eq!(
            worker_branch.label(),
            <NestedInnerLeftMsg as MessageSpec>::LABEL,
            "inner route exposes NestedInnerLeftMsg"
        );
        let (worker_after_inner, observed_inner) = worker_branch
            .decode::<NestedInnerLeftMsg>()
            .await
            .expect("decode inner data");
        assert_eq!(observed_inner, 222);
        worker = worker_after_inner;

        // =========================================================================
        // Post-route: Controller sends final message
        // =========================================================================
        let (controller_after_post, _outcome) = controller
            .flow::<NestedPostRouteMsg>()
            .expect("post-route flow")
            .send(&333)
            .await
            .expect("send post-route");
        drop(controller_after_post);

        let (_, post_payload) =
            worker.recv::<NestedPostRouteMsg>().await.expect("recv post-route");
        assert_eq!(post_payload, 333);

        let tap_ring = cluster.get_local(&rv_id).expect("rendezvous ref").tap();
        let end_head = tap_ring.head();
        let storage = tap_ring.as_slice();
        let endpoint_events = normalise::endpoint_trace(storage, start_head, end_head);
        let (policy_lane, failures) = normalise::policy_lane_trace(storage, start_head, end_head);
        assert!(
            failures.is_empty(),
            "nested route trace should not emit local action failures: {:?}",
            failures
        );
        let mut atlas: Vec<_> = NESTED_CONTROLLER_PROGRAM.scope_regions().collect();
        atlas.extend(NESTED_WORKER_PROGRAM.scope_regions());
        let _correlated = normalise::correlate_scope_traces_with_atlas(
            &endpoint_events,
            &policy_lane,
            &[],
            &atlas,
        );

        #[cfg(feature = "test-utils")]
        {
            // Verify outer route scope is correlated
            let outer_trace = ScopeTrace::new(outer_scope_region.range, outer_scope_region.nest);
            let outer_entry = correlated
                .get(&outer_trace)
                .expect("missing scope trace for outer route");
            assert_eq!(
                outer_entry.region.as_ref().map(|region| region.scope_id),
                Some(outer_scope_region.scope_id),
                "outer route correlated scope id mismatch"
            );

            // Verify inner route scope is correlated
            let inner_trace = ScopeTrace::new(inner_scope_region.range, inner_scope_region.nest);
            let inner_entry = correlated
                .get(&inner_trace)
                .expect("missing scope trace for inner route");
            assert_eq!(
                inner_entry.region.as_ref().map(|region| region.scope_id),
                Some(inner_scope_region.scope_id),
                "inner route correlated scope id mismatch"
            );
        }
    });
}

// ============================================================================
// Send Route followed by Recv Route Test: route::<0, 0> then route::<1, 1>
// ============================================================================
//
// This test demonstrates two consecutive routes with different decision-makers:
// 1. Controller uses flow().send(()) for send route decision (route::<0, 0>)
// 2. Controller sends data to Worker (wire data)
// 3. Worker directly receives (no offer() needed)
// 4. Worker uses flow().send(()) for return route decision (route::<1, 1>)
// 5. Worker sends data to Controller (wire data)
// 6. Controller directly receives (no offer() needed)
//
// Key insight: Each role uses flow().send(()) for their own route decision.
#[test]
fn send_route_followed_by_recv_route() {
    run_with_large_stack(|| async {
        let tap_buf = leak_tap_storage();
        let slab = leak_slab(2048);
        let config = Config::new(tap_buf, slab);
        let transport = TestTransport::default();
        let rendezvous: Rendezvous<'_, '_, TestTransport, DefaultLabelUniverse, CounterClock> =
            Rendezvous::from_config(config, transport.clone());

        let cluster: &mut Cluster = Box::leak(Box::new(SessionCluster::new(leak_clock())));
        let rv_id = cluster.add_rendezvous(rendezvous).expect("register rv");

        register_route_resolvers_for_program(&*cluster, rv_id, &SEND_RECV_CONTROLLER_PROGRAM);
        register_route_resolvers_for_program(&*cluster, rv_id, &SEND_RECV_WORKER_PROGRAM);

        let sid = SessionId::new(101);

        let mut controller: ControllerEndpoint = cluster
            .attach_cursor::<0, _, _, _>(rv_id, sid, &SEND_RECV_CONTROLLER_PROGRAM, NoBinding)
            .expect("attach controller");
        let mut worker: WorkerEndpoint = cluster
            .attach_cursor::<1, _, _, _>(rv_id, sid, &SEND_RECV_WORKER_PROGRAM, NoBinding)
            .expect("attach worker");

        // =========================================================================
        // Send route: Controller self-send control (flow().send(()))
        // =========================================================================
        let (controller_after_route_ctrl, outcome) = controller
            .flow::<AcceptDecisionMsg>()
            .expect("send-route control flow")
            .send(())
            .await
            .expect("send send-route control");
        assert!(matches!(outcome, ControlOutcome::Canonical(_)));
        controller = controller_after_route_ctrl;

        // =========================================================================
        // Send route: Controller sends wire data to Worker
        // =========================================================================
        let (controller_after_route, _outcome) = controller
            .flow::<SendRouteFirstLeftMsg>()
            .expect("send-route flow")
            .send(&7)
            .await
            .expect("send route left");
        controller = controller_after_route;

        // =========================================================================
        // Send route: Worker offers route arm, then decodes data
        // =========================================================================
        let worker_branch = worker.offer().await.expect("offer send route");
        assert_eq!(
            worker_branch.label(),
            <SendRouteFirstLeftMsg as MessageSpec>::LABEL,
            "send route exposes SendRouteFirstLeftMsg"
        );
        let (worker_after_recv, value) = worker_branch
            .decode::<SendRouteFirstLeftMsg>()
            .await
            .expect("decode route left");
        assert_eq!(value, 7);
        worker = worker_after_recv;

        // =========================================================================
        // Return route: Worker self-send control (flow().send(()))
        // =========================================================================
        let (worker_after_return_ctrl, outcome) = worker
            .flow::<AcceptDecisionMsg>()
            .expect("return-route control flow")
            .send(())
            .await
            .expect("send return-route control");
        assert!(matches!(outcome, ControlOutcome::Canonical(_)));
        worker = worker_after_return_ctrl;

        // =========================================================================
        // Return route: Worker sends wire data to Controller
        // =========================================================================
        let (_, _outcome) = worker
            .flow::<ReturnRouteLeftMsg>()
            .expect("return-route flow")
            .send(&99)
            .await
            .expect("worker send return route");

        // =========================================================================
        // Return route: Controller offers route arm, then decodes data
        // =========================================================================
        let controller_branch = controller.offer().await.expect("offer return route");
        assert_eq!(
            controller_branch.label(),
            <ReturnRouteLeftMsg as MessageSpec>::LABEL,
            "return route exposes ReturnRouteLeftMsg"
        );
        let (controller_after_recv, ret) = controller_branch
            .decode::<ReturnRouteLeftMsg>()
            .await
            .expect("decode return route");
        assert_eq!(ret, 99);
        let _controller = controller_after_recv;

        // ensure controller typestate advanced past the route without manual intervention
        #[cfg(feature = "test-utils")]
        assert_ne!(
            controller.phase_cursor().label(),
            Some(<ReturnRouteLeftMsg as hibana::g::MessageSpec>::LABEL)
        );
    });
}

fn assert_control_scope_correlated(
    correlated: &BTreeMap<ScopeTrace, normalise::ScopeAnnotatedCorrelatedTraces>,
    trace: ScopeTrace,
    expected_scope: ScopeId,
    sid: SessionId,
    label: u8,
    context: &str,
) {
    let entry = correlated
        .get(&trace)
        .unwrap_or_else(|| panic!("missing scope trace for {}", context));
    assert_eq!(
        entry.region.as_ref().map(|region| region.scope_id),
        Some(expected_scope),
        "{context} correlated scope id mismatch"
    );
    assert!(
        entry.traces.endpoint.iter().any(|event| matches!(
            event,
            normalise::EndpointEvent::Control {
                sid: event_sid,
                label: event_label,
                ..
            } if *event_sid == sid.raw() && *event_label == label
        )),
        "{context} missing control endpoint event for label {}",
        label
    );
}

fn register_route_resolvers_for_program<const ROLE: u8, Steps>(
    cluster: &Cluster,
    rv_id: RendezvousId,
    program: &g::RoleProgram<'static, ROLE, Steps>,
) {
    for info in program.control_plans() {
        if info.plan.is_dynamic() {
            cluster
                .register_control_plan_resolver(rv_id, &info, always_left_route_resolver)
                .expect("register route resolver");
        }
    }
}

fn register_route_resolvers_for_policy<const ROLE: u8, Steps>(
    cluster: &Cluster,
    rv_id: RendezvousId,
    program: &g::RoleProgram<'static, ROLE, Steps>,
    policy_id: u16,
) {
    for info in program.control_plans() {
        if info.plan.is_dynamic() {
            if info.plan.dynamic_components().map(|(pid, _)| pid) == Some(policy_id) {
                cluster
                    .register_control_plan_resolver(rv_id, &info, always_left_route_resolver)
                    .expect("register route resolver");
            }
        }
    }
}

fn always_left_route_resolver(
    _cluster: &Cluster,
    _meta: &DynamicMeta,
    _ctx: ResolverContext,
) -> Result<DynamicResolution, ()> {
    Ok(DynamicResolution::RouteArm { arm: 0 })
}

#[derive(Clone, Copy, Debug)]
struct RouteRightKind;

impl ResourceKind for RouteRightKind {
    type Handle = RouteDecisionHandle;
    const TAG: u8 = RouteDecisionKind::TAG;
    const NAME: &'static str = "RouteRightDecision";

    fn encode_handle(handle: &Self::Handle) -> [u8; hibana::control::cap::CAP_HANDLE_LEN] {
        handle.encode()
    }

    fn decode_handle(
        data: [u8; hibana::control::cap::CAP_HANDLE_LEN],
    ) -> Result<Self::Handle, CapError> {
        RouteDecisionHandle::decode(data)
    }

    fn zeroize(handle: &mut Self::Handle) {
        handle.arm = 0;
        handle.scope = ScopeId::generic(0);
    }

    fn caps_mask(_handle: &Self::Handle) -> CapsMask {
        CapsMask::empty()
    }
}

impl SessionScopedKind for RouteRightKind {
    fn handle_for_session(_sid: hibana::control::types::SessionId, _lane: Lane) -> Self::Handle {
        RouteDecisionHandle::default()
    }
}

impl hibana::control::cap::ControlResourceKind for RouteRightKind {
    const LABEL: u8 = 11;
    const SCOPE: ControlScopeKind = ControlScopeKind::Route;
    const TAP_ID: u16 = <RouteDecisionKind as hibana::control::cap::ControlResourceKind>::TAP_ID;
    const SHOT: CapShot = CapShot::One;
    const HANDLING: hibana::g::ControlHandling = hibana::g::ControlHandling::Canonical;
}

impl hibana::control::cap::ControlMint for RouteRightKind {
    fn mint_handle(
        _sid: hibana::rendezvous::SessionId,
        _lane: Lane,
        scope: ScopeId,
    ) -> Self::Handle {
        RouteDecisionHandle::new(scope, 0)
    }
}

// ============================================================================
// Accept/Reject Decision Kinds - for nested route self-send control
// ============================================================================

#[derive(Clone, Copy, Debug)]
struct AcceptDecisionKind;

impl ResourceKind for AcceptDecisionKind {
    type Handle = RouteDecisionHandle;
    const TAG: u8 = RouteDecisionKind::TAG;
    const NAME: &'static str = "AcceptDecision";

    fn encode_handle(handle: &Self::Handle) -> [u8; hibana::control::cap::CAP_HANDLE_LEN] {
        handle.encode()
    }

    fn decode_handle(
        data: [u8; hibana::control::cap::CAP_HANDLE_LEN],
    ) -> Result<Self::Handle, CapError> {
        RouteDecisionHandle::decode(data)
    }

    fn zeroize(handle: &mut Self::Handle) {
        handle.arm = 0;
        handle.scope = ScopeId::generic(0);
    }

    fn caps_mask(_handle: &Self::Handle) -> CapsMask {
        CapsMask::empty()
    }
}

impl SessionScopedKind for AcceptDecisionKind {
    fn handle_for_session(_sid: hibana::control::types::SessionId, _lane: Lane) -> Self::Handle {
        RouteDecisionHandle::default()
    }
}

impl hibana::control::cap::ControlResourceKind for AcceptDecisionKind {
    const LABEL: u8 = 50;
    const SCOPE: ControlScopeKind = ControlScopeKind::Route;
    const TAP_ID: u16 = <RouteDecisionKind as hibana::control::cap::ControlResourceKind>::TAP_ID;
    const SHOT: CapShot = CapShot::One;
    const HANDLING: hibana::g::ControlHandling = hibana::g::ControlHandling::Canonical;
}

impl hibana::control::cap::ControlMint for AcceptDecisionKind {
    fn mint_handle(
        _sid: hibana::rendezvous::SessionId,
        _lane: Lane,
        scope: ScopeId,
    ) -> Self::Handle {
        RouteDecisionHandle::new(scope, 0)
    }
}

#[derive(Clone, Copy, Debug)]
struct RejectDecisionKind;

impl ResourceKind for RejectDecisionKind {
    type Handle = RouteDecisionHandle;
    const TAG: u8 = RouteDecisionKind::TAG;
    const NAME: &'static str = "RejectDecision";

    fn encode_handle(handle: &Self::Handle) -> [u8; hibana::control::cap::CAP_HANDLE_LEN] {
        handle.encode()
    }

    fn decode_handle(
        data: [u8; hibana::control::cap::CAP_HANDLE_LEN],
    ) -> Result<Self::Handle, CapError> {
        RouteDecisionHandle::decode(data)
    }

    fn zeroize(handle: &mut Self::Handle) {
        handle.arm = 0;
        handle.scope = ScopeId::generic(0);
    }

    fn caps_mask(_handle: &Self::Handle) -> CapsMask {
        CapsMask::empty()
    }
}

impl SessionScopedKind for RejectDecisionKind {
    fn handle_for_session(_sid: hibana::control::types::SessionId, _lane: Lane) -> Self::Handle {
        RouteDecisionHandle::default()
    }
}

impl hibana::control::cap::ControlResourceKind for RejectDecisionKind {
    const LABEL: u8 = 51;
    const SCOPE: ControlScopeKind = ControlScopeKind::Route;
    const TAP_ID: u16 = <RouteDecisionKind as hibana::control::cap::ControlResourceKind>::TAP_ID;
    const SHOT: CapShot = CapShot::One;
    const HANDLING: hibana::g::ControlHandling = hibana::g::ControlHandling::Canonical;
}

impl hibana::control::cap::ControlMint for RejectDecisionKind {
    fn mint_handle(
        _sid: hibana::rendezvous::SessionId,
        _lane: Lane,
        scope: ScopeId,
    ) -> Self::Handle {
        RouteDecisionHandle::new(scope, 0)
    }
}

/// Verify that Worker typestate has route_arm set on recv nodes.
#[cfg(feature = "test-utils")]
#[test]
fn nested_worker_has_route_arm_on_recv() {
    let cursor = NESTED_WORKER_PROGRAM.phase_cursor();
    let mut idx = 0;
    let mut found_any_route_arm = false;

    // Scan up to 10 nodes (typestate is finite)
    while idx < 10 {
        let node = cursor.typestate_node(idx);
        let action = node.action();

        if action.is_recv() && node.route_arm().is_some() {
            found_any_route_arm = true;
            break;
        }
        if matches!(action, hibana::global::typestate::LocalAction::Terminate) {
            break;
        }
        idx += 1;
    }

    assert!(
        found_any_route_arm,
        "Worker typestate should have at least one recv with route_arm set"
    );
}
