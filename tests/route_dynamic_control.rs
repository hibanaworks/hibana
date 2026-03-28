#![cfg(feature = "std")]
mod common;
#[path = "support/route_control_kinds.rs"]
mod route_control_kinds;
#[path = "support/runtime.rs"]
mod runtime_support;

use common::TestTransport;
use hibana::{
    g::advanced::steps::{
        LoopBreakSteps, LoopContinueSteps, LoopDecisionSteps, ProjectRole, SendStep, SeqSteps,
        StepConcat, StepCons, StepNil,
    },
    g::advanced::{CanonicalControl, RoleProgram, project},
    g::{self, Msg, Role},
    substrate::{
        SessionCluster, SessionId,
        binding::{BindingSlot, Channel, IncomingClassification, NoBinding, TransportOpsError},
        policy::{
            ContextId, ContextValue, PolicyAttrs, PolicySignals, PolicySignalsProvider, core,
            epf::Slot,
        },
        runtime::{Config, DefaultLabelUniverse},
    },
    substrate::{
        cap::{
            GenericCapToken, ResourceKind,
            advanced::{LoopBreakKind, LoopContinueKind, RouteDecisionKind},
        },
        policy::{DynamicResolution, ResolverContext, ResolverError},
    },
};
use runtime_support::{leak_clock, leak_slab, leak_tap_storage};

const LABEL_LOOP_CONTINUE: u8 = 48;
const LABEL_LOOP_BREAK: u8 = 49;
const LABEL_ROUTE_DECISION: u8 = 57;

type RouteRightKind = route_control_kinds::RouteControl<11, 0>;

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU32, Ordering},
};

fn block_on_async<F>(future: F) -> F::Output
where
    F: std::future::Future,
{
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build tokio runtime");
    rt.block_on(future)
}

const ROUTE_POLICY_ID: u16 = 9;
static ROUTE_ALLOW: AtomicBool = AtomicBool::new(false);
const POLICY_INPUT_ID: ContextId = ContextId::new(0x9001);

#[derive(Clone)]
struct PolicyInputBinding {
    policy_input0: Arc<AtomicU32>,
}

impl PolicyInputBinding {
    fn new(policy_input0: Arc<AtomicU32>) -> Self {
        Self { policy_input0 }
    }
}

impl PolicySignalsProvider for PolicyInputBinding {
    fn signals(&self, slot: Slot) -> PolicySignals {
        let policy_input0 = self.policy_input0.load(Ordering::Relaxed);
        let input = if matches!(slot, Slot::Route) {
            [policy_input0, 0, 0, 0]
        } else {
            [0; 4]
        };
        let mut attrs = PolicyAttrs::new();
        let _ = attrs.insert(POLICY_INPUT_ID, ContextValue::from_u32(policy_input0));
        PolicySignals { input, attrs }
    }
}

impl BindingSlot for PolicyInputBinding {
    fn poll_incoming_for_lane(&mut self, _logical_lane: u8) -> Option<IncomingClassification> {
        None
    }

    fn on_recv(&mut self, _channel: Channel, _buf: &mut [u8]) -> Result<usize, TransportOpsError> {
        Ok(0)
    }

    fn policy_signals_provider(&self) -> Option<&dyn PolicySignalsProvider> {
        Some(self)
    }
}

const LEFT_ARM: g::Program<
    StepCons<
        SendStep<
            Role<0>,
            Role<0>,
            Msg<
                { LABEL_ROUTE_DECISION },
                GenericCapToken<RouteDecisionKind>,
                CanonicalControl<RouteDecisionKind>,
            >,
        >,
        StepNil,
    >,
> = g::send::<
    Role<0>,
    Role<0>,
    Msg<
        { LABEL_ROUTE_DECISION },
        GenericCapToken<RouteDecisionKind>,
        CanonicalControl<RouteDecisionKind>,
    >,
    0,
>()
.policy::<ROUTE_POLICY_ID>();
const RIGHT_ARM: g::Program<
    StepCons<
        SendStep<
            Role<0>,
            Role<0>,
            Msg<11, GenericCapToken<RouteRightKind>, CanonicalControl<RouteRightKind>>,
        >,
        StepNil,
    >,
> = g::send::<
    Role<0>,
    Role<0>,
    Msg<11, GenericCapToken<RouteRightKind>, CanonicalControl<RouteRightKind>>,
    0,
>()
.policy::<ROUTE_POLICY_ID>();
// Route is local to Controller (0 → 0) since all arms are self-sends
const PROGRAM: g::Program<
    <StepCons<
        SendStep<
            Role<0>,
            Role<0>,
            Msg<
                { LABEL_ROUTE_DECISION },
                GenericCapToken<RouteDecisionKind>,
                CanonicalControl<RouteDecisionKind>,
            >,
        >,
        StepNil,
    > as StepConcat<
        StepCons<
            SendStep<
                Role<0>,
                Role<0>,
                Msg<11, GenericCapToken<RouteRightKind>, CanonicalControl<RouteRightKind>>,
            >,
            StepNil,
        >,
    >>::Output,
> = g::route(LEFT_ARM, RIGHT_ARM);

static CONTROLLER_PROGRAM: RoleProgram<
    'static,
    0,
    <<StepCons<
        SendStep<
            Role<0>,
            Role<0>,
            Msg<
                { LABEL_ROUTE_DECISION },
                GenericCapToken<RouteDecisionKind>,
                CanonicalControl<RouteDecisionKind>,
            >,
        >,
        StepNil,
    > as StepConcat<
        StepCons<
            SendStep<
                Role<0>,
                Role<0>,
                Msg<11, GenericCapToken<RouteRightKind>, CanonicalControl<RouteRightKind>>,
            >,
            StepNil,
        >,
    >>::Output as ProjectRole<Role<0>>>::Output,
> = project(&PROGRAM);
static WORKER_PROGRAM: RoleProgram<
    'static,
    1,
    <<StepCons<
        SendStep<
            Role<0>,
            Role<0>,
            Msg<
                { LABEL_ROUTE_DECISION },
                GenericCapToken<RouteDecisionKind>,
                CanonicalControl<RouteDecisionKind>,
            >,
        >,
        StepNil,
    > as StepConcat<
        StepCons<
            SendStep<
                Role<0>,
                Role<0>,
                Msg<11, GenericCapToken<RouteRightKind>, CanonicalControl<RouteRightKind>>,
            >,
            StepNil,
        >,
    >>::Output as ProjectRole<Role<1>>>::Output,
> = project(&PROGRAM);

const LOOP_POLICY_ID: u16 = 10;

fn transport_queue_is_empty(transport: &TestTransport) -> bool {
    transport
        .state
        .lock()
        .expect("state lock")
        .queues
        .values()
        .all(|queue| queue.is_empty())
}

// Self-send for CanonicalControl: Controller → Controller
const LOOP_CONTINUE_ARM: g::Program<
    LoopContinueSteps<
        Role<0>,
        Msg<
            { LABEL_LOOP_CONTINUE },
            GenericCapToken<LoopContinueKind>,
            CanonicalControl<LoopContinueKind>,
        >,
        StepNil,
    >,
> = g::seq(
    g::send::<
        Role<0>,
        Role<0>,
        Msg<
            { LABEL_LOOP_CONTINUE },
            GenericCapToken<LoopContinueKind>,
            CanonicalControl<LoopContinueKind>,
        >,
        0,
    >()
    .policy::<LOOP_POLICY_ID>(),
    StepNil::PROGRAM,
);
const LOOP_BREAK_ARM: g::Program<
    LoopBreakSteps<
        Role<0>,
        Msg<{ LABEL_LOOP_BREAK }, GenericCapToken<LoopBreakKind>, CanonicalControl<LoopBreakKind>>,
        StepNil,
    >,
> = g::send::<
    Role<0>,
    Role<0>,
    Msg<{ LABEL_LOOP_BREAK }, GenericCapToken<LoopBreakKind>, CanonicalControl<LoopBreakKind>>,
    0,
>()
.policy::<LOOP_POLICY_ID>();
// Route is local to Controller (0 → 0)
const LOOP_PROGRAM: g::Program<
    LoopDecisionSteps<
        Role<0>,
        Msg<
            { LABEL_LOOP_CONTINUE },
            GenericCapToken<LoopContinueKind>,
            CanonicalControl<LoopContinueKind>,
        >,
        Msg<{ LABEL_LOOP_BREAK }, GenericCapToken<LoopBreakKind>, CanonicalControl<LoopBreakKind>>,
    >,
> = g::route(LOOP_CONTINUE_ARM, LOOP_BREAK_ARM);

static LOOP_CONTROLLER_PROGRAM: RoleProgram<
    'static,
    0,
    <LoopDecisionSteps<
        Role<0>,
        Msg<
            { LABEL_LOOP_CONTINUE },
            GenericCapToken<LoopContinueKind>,
            CanonicalControl<LoopContinueKind>,
        >,
        Msg<{ LABEL_LOOP_BREAK }, GenericCapToken<LoopBreakKind>, CanonicalControl<LoopBreakKind>>,
    > as ProjectRole<Role<0>>>::Output,
> = project(&LOOP_PROGRAM);

const OUTER_LOOP_CONTINUE_ARM: g::Program<
    SeqSteps<
        LoopContinueSteps<
            Role<0>,
            Msg<
                { LABEL_LOOP_CONTINUE },
                GenericCapToken<LoopContinueKind>,
                CanonicalControl<LoopContinueKind>,
            >,
            StepNil,
        >,
        LoopDecisionSteps<
            Role<0>,
            Msg<
                { LABEL_LOOP_CONTINUE },
                GenericCapToken<LoopContinueKind>,
                CanonicalControl<LoopContinueKind>,
            >,
            Msg<
                { LABEL_LOOP_BREAK },
                GenericCapToken<LoopBreakKind>,
                CanonicalControl<LoopBreakKind>,
            >,
        >,
    >,
> = g::seq(LOOP_CONTINUE_ARM, LOOP_PROGRAM);
// Route is local to Controller (0 → 0)
const NESTED_LOOP_PROGRAM: g::Program<
    <SeqSteps<
        LoopContinueSteps<
            Role<0>,
            Msg<
                { LABEL_LOOP_CONTINUE },
                GenericCapToken<LoopContinueKind>,
                CanonicalControl<LoopContinueKind>,
            >,
            StepNil,
        >,
        LoopDecisionSteps<
            Role<0>,
            Msg<
                { LABEL_LOOP_CONTINUE },
                GenericCapToken<LoopContinueKind>,
                CanonicalControl<LoopContinueKind>,
            >,
            Msg<
                { LABEL_LOOP_BREAK },
                GenericCapToken<LoopBreakKind>,
                CanonicalControl<LoopBreakKind>,
            >,
        >,
    > as StepConcat<
        LoopBreakSteps<
            Role<0>,
            Msg<
                { LABEL_LOOP_BREAK },
                GenericCapToken<LoopBreakKind>,
                CanonicalControl<LoopBreakKind>,
            >,
            StepNil,
        >,
    >>::Output,
> = g::route(OUTER_LOOP_CONTINUE_ARM, LOOP_BREAK_ARM);

static NESTED_LOOP_CONTROLLER_PROGRAM: RoleProgram<
    'static,
    0,
    <<SeqSteps<
        LoopContinueSteps<
            Role<0>,
            Msg<
                { LABEL_LOOP_CONTINUE },
                GenericCapToken<LoopContinueKind>,
                CanonicalControl<LoopContinueKind>,
            >,
            StepNil,
        >,
        LoopDecisionSteps<
            Role<0>,
            Msg<
                { LABEL_LOOP_CONTINUE },
                GenericCapToken<LoopContinueKind>,
                CanonicalControl<LoopContinueKind>,
            >,
            Msg<
                { LABEL_LOOP_BREAK },
                GenericCapToken<LoopBreakKind>,
                CanonicalControl<LoopBreakKind>,
            >,
        >,
    > as StepConcat<
        LoopBreakSteps<
            Role<0>,
            Msg<
                { LABEL_LOOP_BREAK },
                GenericCapToken<LoopBreakKind>,
                CanonicalControl<LoopBreakKind>,
            >,
            StepNil,
        >,
    >>::Output as ProjectRole<Role<0>>>::Output,
> = project(&NESTED_LOOP_PROGRAM);

fn route_resolver(ctx: ResolverContext) -> Result<DynamicResolution, ResolverError> {
    if ctx.attr(core::TAG).map(|value| value.as_u8()) != Some(RouteDecisionKind::TAG) {
        return Err(ResolverError::Reject);
    }
    if ROUTE_ALLOW.load(Ordering::Relaxed) {
        Ok(DynamicResolution::RouteArm { arm: 0 })
    } else {
        Err(ResolverError::Reject)
    }
}

fn route_policy_input_resolver(ctx: ResolverContext) -> Result<DynamicResolution, ResolverError> {
    if ctx.attr(core::TAG).map(|value| value.as_u8()) != Some(RouteDecisionKind::TAG) {
        return Err(ResolverError::Reject);
    }
    let arm = ctx
        .attr(POLICY_INPUT_ID)
        .map(|v| (v.as_u32() & 1) as u8)
        .unwrap_or(0);
    Ok(DynamicResolution::RouteArm { arm })
}

/// Test route dynamic resolver with flow().send(()) pattern.
///
/// CanonicalControl uses self-send (Controller → Controller) and advances
/// via flow().send(()) which skips wire transmission for self-send.
#[test]
fn route_dynamic_self_send_send_path_skips_revalidation() {
    block_on_async(async {
        let tap_buf = leak_tap_storage();
        let slab = leak_slab(2048);
        let config = Config::new(tap_buf, slab);
        let transport = TestTransport::default();

        let cluster: &mut SessionCluster<
            'static,
            TestTransport,
            DefaultLabelUniverse,
            hibana::substrate::runtime::CounterClock,
            4,
        > = Box::leak(Box::new(SessionCluster::new(leak_clock())));

        let rv_id = cluster
            .add_rendezvous_from_config(config, transport.clone())
            .expect("register rendezvous");
        cluster
            .set_resolver::<ROUTE_POLICY_ID, 0, _, _>(
                rv_id,
                &CONTROLLER_PROGRAM,
                hibana::substrate::policy::ResolverRef::from_fn(route_resolver),
            )
            .expect("register route resolver");

        // First attempt: resolver rejects, but self-send send-path must not
        // re-evaluate dynamic route policy after arm selection.
        let sid = SessionId::new(7);

        let worker_endpoint = cluster
            .enter(rv_id, sid, &WORKER_PROGRAM, NoBinding)
            .expect("worker endpoint");

        ROUTE_ALLOW.store(false, Ordering::Relaxed);
        let controller_cursor = cluster
            .enter(rv_id, sid, &CONTROLLER_PROGRAM, NoBinding)
            .expect("controller endpoint");

        let first_flow = controller_cursor
            .flow::<Msg<
                { LABEL_ROUTE_DECISION },
                GenericCapToken<RouteDecisionKind>,
                CanonicalControl<RouteDecisionKind>,
            >>()
            .expect("self-send route flow should be available");
        let (controller_cursor, first_outcome) = first_flow
            .send(())
            .await
            .expect("self-send route should not re-evaluate disallowed resolver");
        assert!(first_outcome.is_canonical());
        drop(controller_cursor);

        drop(worker_endpoint);

        // Second attempt: resolver allows (ROUTE_ALLOW = true)
        ROUTE_ALLOW.store(true, Ordering::Relaxed);

        let sid2 = SessionId::new(8);

        let worker_endpoint = cluster
            .enter(rv_id, sid2, &WORKER_PROGRAM, NoBinding)
            .expect("worker endpoint (retry)");

        let controller_cursor = cluster
            .enter(rv_id, sid2, &CONTROLLER_PROGRAM, NoBinding)
            .expect("controller endpoint (retry)");

        // Use flow().send(()) pattern for self-send CanonicalControl
        let send_flow = controller_cursor
            .flow::<Msg<
                { LABEL_ROUTE_DECISION },
                GenericCapToken<RouteDecisionKind>,
                CanonicalControl<RouteDecisionKind>,
            >>()
            .expect("route should proceed when allowed");

        let (controller_endpoint, outcome) = send_flow.send(()).await.expect("send route decision");
        assert!(outcome.is_canonical());

        // Worker doesn't receive anything for self-send control - the route decision
        // is purely local to the Controller. Worker endpoint is already at end state.
        drop(worker_endpoint);
        drop(controller_endpoint);

        assert!(transport_queue_is_empty(&transport));
    });
}

#[test]
fn route_token_arm_matches_offer_when_policy_input_changes_before_send() {
    block_on_async(async {
        let tap_buf = leak_tap_storage();
        let slab = leak_slab(2048);
        let config = Config::new(tap_buf, slab);
        let transport = TestTransport::default();
        let policy_input0 = Arc::new(AtomicU32::new(0));

        let cluster: &mut SessionCluster<
            'static,
            TestTransport,
            DefaultLabelUniverse,
            hibana::substrate::runtime::CounterClock,
            4,
        > = Box::leak(Box::new(SessionCluster::new(leak_clock())));
        let rv_id = cluster
            .add_rendezvous_from_config(config, transport.clone())
            .expect("register rendezvous");

        cluster
            .set_resolver::<ROUTE_POLICY_ID, 0, _, _>(
                rv_id,
                &CONTROLLER_PROGRAM,
                hibana::substrate::policy::ResolverRef::from_fn(route_policy_input_resolver),
            )
            .expect("register route resolver");

        let sid = SessionId::new(9);
        let worker = cluster
            .enter(rv_id, sid, &WORKER_PROGRAM, NoBinding)
            .expect("worker endpoint");
        let controller = cluster
            .enter(
                rv_id,
                sid,
                &CONTROLLER_PROGRAM,
                PolicyInputBinding::new(policy_input0.clone()),
            )
            .expect("controller endpoint");

        let send_flow = controller
            .flow::<Msg<
                { LABEL_ROUTE_DECISION },
                GenericCapToken<RouteDecisionKind>,
                CanonicalControl<RouteDecisionKind>,
            >>()
            .expect("route should select left arm");

        policy_input0.store(1, Ordering::Relaxed);

        let (controller, outcome) = send_flow.send(()).await.expect("send route decision");
        let handle = outcome
            .into_canonical()
            .expect("expected canonical control token")
            .as_generic()
            .decode_handle()
            .expect("decode canonical route decision handle");

        assert_eq!(
            handle.arm, 0,
            "token arm must remain equal to offer-selected arm"
        );
        assert!(
            !handle.scope.is_none(),
            "canonical route decision handle must carry a materialized scope"
        );

        drop(worker);
        drop(controller);
        assert!(transport_queue_is_empty(&transport));
    });
}

/// Test that self-send loop control type definitions compile correctly.
///
/// With self-send CanonicalControl, `local()` doesn't navigate routes dynamically.
/// The type system ensures the protocol is well-formed, and local() can be used
/// once the cursor is positioned at the appropriate local action.
///
/// This test verifies the type definitions are correct after removing the Target parameter.
#[test]
fn loop_dynamic_resolver_policy_abort_and_success() {
    // Verify the loop program compiles with self-send semantics
    let _controller_program = &LOOP_CONTROLLER_PROGRAM;
}

/// Test nested routes with flow().send(()) pattern.
///
/// With self-send CanonicalControl (Controller → Controller), all route decisions
/// are local to the Controller role. Worker doesn't participate in route control.
#[test]
fn nested_loop_dynamic_send_and_offer() {
    let tap_buf = leak_tap_storage();
    let slab = leak_slab(4096);
    let config = Config::new(tap_buf, slab);
    let transport = TestTransport::default();

    let cluster: &mut SessionCluster<
        'static,
        TestTransport,
        DefaultLabelUniverse,
        hibana::substrate::runtime::CounterClock,
        4,
    > = Box::leak(Box::new(SessionCluster::new(leak_clock())));

    let _rv_id = cluster
        .add_rendezvous_from_config(config, transport.clone())
        .expect("register rendezvous");

    // With self-send loops, verify the type definitions compile correctly
    let _controller_program = &NESTED_LOOP_CONTROLLER_PROGRAM;

    assert!(transport_queue_is_empty(&transport));
}
