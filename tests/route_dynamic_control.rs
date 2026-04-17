#![cfg(feature = "std")]
mod common;
#[path = "support/placement.rs"]
mod placement_support;
#[path = "support/route_control_kinds.rs"]
mod route_control_kinds;
#[path = "support/runtime.rs"]
mod runtime_support;
#[path = "support/tls_mut.rs"]
mod tls_mut_support;
#[path = "support/tls_ref.rs"]
mod tls_ref_support;

use ::core::{cell::UnsafeCell, mem::MaybeUninit};
use common::TestTransport;
use hibana::{
    g::advanced::steps::{PolicySteps, RouteSteps, SendStep, SeqSteps, StepCons, StepNil},
    g::advanced::{CanonicalControl, RoleProgram, project},
    g::{self, Msg, Role},
    substrate::{
        SessionId, SessionKit,
        binding::{BindingSlot, Channel, IncomingClassification, NoBinding, TransportOpsError},
        policy::{
            ContextId, ContextValue, PolicyAttrs, PolicySignals, PolicySignalsProvider, PolicySlot,
            core,
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
use placement_support::write_value;
use runtime_support::with_fixture;
use std::cell::Cell;
use tls_mut_support::with_tls_mut;
use tls_ref_support::with_tls_ref;

const LABEL_LOOP_CONTINUE: u8 = 48;
const LABEL_LOOP_BREAK: u8 = 49;
const LABEL_ROUTE_DECISION: u8 = 57;
const ROUTE_POLICY_ID: u16 = 9;
const LOOP_POLICY_ID: u16 = 10;
const POLICY_INPUT_ID: ContextId = ContextId::new(0x9001);

type RouteRightKind = route_control_kinds::RouteControl<11, 0>;
type RouteLeftHead = PolicySteps<
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
    ROUTE_POLICY_ID,
>;
type RouteRightHead = PolicySteps<
    StepCons<
        SendStep<
            Role<0>,
            Role<0>,
            Msg<11, GenericCapToken<RouteRightKind>, CanonicalControl<RouteRightKind>>,
        >,
        StepNil,
    >,
    ROUTE_POLICY_ID,
>;
type RouteProgramSteps = RouteSteps<RouteLeftHead, RouteRightHead>;

fn block_on_async<F>(future: F) -> F::Output
where
    F: std::future::Future,
{
    futures::executor::block_on(future)
}

type LoopContinueHead = PolicySteps<
    StepCons<
        SendStep<
            Role<0>,
            Role<0>,
            Msg<
                { LABEL_LOOP_CONTINUE },
                GenericCapToken<LoopContinueKind>,
                CanonicalControl<LoopContinueKind>,
            >,
        >,
        StepNil,
    >,
    LOOP_POLICY_ID,
>;
type LoopBreakHead = PolicySteps<
    StepCons<
        SendStep<
            Role<0>,
            Role<0>,
            Msg<
                { LABEL_LOOP_BREAK },
                GenericCapToken<LoopBreakKind>,
                CanonicalControl<LoopBreakKind>,
            >,
        >,
        StepNil,
    >,
    LOOP_POLICY_ID,
>;
type LoopContinueArmSteps = SeqSteps<LoopContinueHead, StepNil>;
type LoopProgramSteps = RouteSteps<LoopContinueArmSteps, LoopBreakHead>;
type OuterLoopContinueArmSteps = SeqSteps<LoopContinueArmSteps, LoopProgramSteps>;
type NestedLoopLeftSteps = SeqSteps<RouteLeftHead, OuterLoopContinueArmSteps>;
type NestedLoopRightSteps = SeqSteps<RouteRightHead, LoopBreakHead>;
type NestedLoopProgramSteps = RouteSteps<NestedLoopLeftSteps, NestedLoopRightSteps>;
type RouteTailLeftSteps = SeqSteps<RouteLeftHead, LoopContinueArmSteps>;
type RouteTailRightSteps = SeqSteps<RouteRightHead, LoopBreakHead>;
type RouteTailProgramSteps = RouteSteps<RouteTailLeftSteps, RouteTailRightSteps>;
type TestKit = SessionKit<
    'static,
    TestTransport,
    DefaultLabelUniverse,
    hibana::substrate::runtime::CounterClock,
    2,
>;
type ControllerEndpoint = hibana::Endpoint<'static, 0, TestKit>;
type WorkerEndpoint = hibana::Endpoint<'static, 1, TestKit>;

std::thread_local! {
    static SESSION_SLOT: UnsafeCell<MaybeUninit<TestKit>> = const {
        UnsafeCell::new(MaybeUninit::uninit())
    };
    static POLICY_INPUT_SLOT: UnsafeCell<MaybeUninit<Cell<u32>>> = const {
        UnsafeCell::new(MaybeUninit::uninit())
    };
    static POLICY_BINDING_SLOT: UnsafeCell<MaybeUninit<PolicyInputBinding>> = const {
        UnsafeCell::new(MaybeUninit::uninit())
    };
    static ROUTE_ALLOW: Cell<bool> = const { Cell::new(false) };
    static CONTROLLER_ENDPOINT_SLOT: UnsafeCell<MaybeUninit<ControllerEndpoint>> = const {
        UnsafeCell::new(MaybeUninit::uninit())
    };
    static WORKER_ENDPOINT_SLOT: UnsafeCell<MaybeUninit<WorkerEndpoint>> = const {
        UnsafeCell::new(MaybeUninit::uninit())
    };
}

fn route_allow() -> bool {
    ROUTE_ALLOW.with(Cell::get)
}

fn set_route_allow(value: bool) {
    ROUTE_ALLOW.with(|cell| cell.set(value));
}

#[derive(Clone)]
struct PolicyInputBinding {
    policy_input0: &'static Cell<u32>,
}

impl PolicyInputBinding {
    fn new(policy_input0: &'static Cell<u32>) -> Self {
        Self { policy_input0 }
    }
}

impl PolicySignalsProvider for PolicyInputBinding {
    fn signals(&self, slot: PolicySlot) -> PolicySignals<'_> {
        let policy_input0 = self.policy_input0.get();
        let input = if matches!(slot, PolicySlot::Route) {
            [policy_input0, 0, 0, 0]
        } else {
            [0; 4]
        };
        let mut attrs = PolicyAttrs::new();
        let _ = attrs.insert(POLICY_INPUT_ID, ContextValue::from_u32(policy_input0));
        PolicySignals::owned(input, attrs)
    }
}

impl BindingSlot for PolicyInputBinding {
    fn poll_incoming_for_lane(&mut self, _logical_lane: u8) -> Option<IncomingClassification> {
        None
    }

    fn on_recv<'a>(
        &'a mut self,
        _channel: Channel,
        _buf: &'a mut [u8],
    ) -> Result<hibana::substrate::wire::Payload<'a>, TransportOpsError> {
        Ok(hibana::substrate::wire::Payload::new(&[]))
    }

    fn policy_signals_provider(&self) -> Option<&dyn PolicySignalsProvider> {
        Some(self)
    }
}

const LEFT_ARM: g::Program<RouteLeftHead> = g::send::<
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
const RIGHT_ARM: g::Program<RouteRightHead> = g::send::<
    Role<0>,
    Role<0>,
    Msg<11, GenericCapToken<RouteRightKind>, CanonicalControl<RouteRightKind>>,
    0,
>()
.policy::<ROUTE_POLICY_ID>();
// Route is local to Controller (0 → 0) since all arms are self-sends
const PROGRAM: g::Program<RouteProgramSteps> = g::route(LEFT_ARM, RIGHT_ARM);

static CONTROLLER_PROGRAM: RoleProgram<'static, 0> = project(&PROGRAM);
static WORKER_PROGRAM: RoleProgram<'static, 1> = project(&PROGRAM);

fn transport_queue_is_empty(transport: &TestTransport) -> bool {
    transport.queue_is_empty()
}

// Self-send for CanonicalControl: Controller → Controller
const LOOP_CONTINUE_ARM: g::Program<LoopContinueArmSteps> = g::seq(
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
const LOOP_BREAK_ARM: g::Program<LoopBreakHead> = g::send::<
    Role<0>,
    Role<0>,
    Msg<{ LABEL_LOOP_BREAK }, GenericCapToken<LoopBreakKind>, CanonicalControl<LoopBreakKind>>,
    0,
>()
.policy::<LOOP_POLICY_ID>();
// Route is local to Controller (0 → 0)
const LOOP_PROGRAM: g::Program<LoopProgramSteps> = g::route(LOOP_CONTINUE_ARM, LOOP_BREAK_ARM);

static LOOP_CONTROLLER_PROGRAM: RoleProgram<'static, 0> = project(&LOOP_PROGRAM);

const ROUTE_TAIL_LEFT_ARM: g::Program<RouteTailLeftSteps> = g::seq(LEFT_ARM, LOOP_CONTINUE_ARM);
const ROUTE_TAIL_RIGHT_ARM: g::Program<RouteTailRightSteps> = g::seq(RIGHT_ARM, LOOP_BREAK_ARM);
const ROUTE_TAIL_PROGRAM: g::Program<RouteTailProgramSteps> =
    g::route(ROUTE_TAIL_LEFT_ARM, ROUTE_TAIL_RIGHT_ARM);

static ROUTE_TAIL_CONTROLLER_PROGRAM: RoleProgram<'static, 0> = project(&ROUTE_TAIL_PROGRAM);
static ROUTE_TAIL_WORKER_PROGRAM: RoleProgram<'static, 1> = project(&ROUTE_TAIL_PROGRAM);

const OUTER_LOOP_CONTINUE_ARM: g::Program<OuterLoopContinueArmSteps> =
    g::seq(LOOP_CONTINUE_ARM, LOOP_PROGRAM);
const NESTED_LOOP_LEFT_ARM: g::Program<NestedLoopLeftSteps> =
    g::seq(LEFT_ARM, OUTER_LOOP_CONTINUE_ARM);
const NESTED_LOOP_RIGHT_ARM: g::Program<NestedLoopRightSteps> = g::seq(RIGHT_ARM, LOOP_BREAK_ARM);
// Route is local to Controller (0 → 0)
const NESTED_LOOP_PROGRAM: g::Program<NestedLoopProgramSteps> =
    g::route(NESTED_LOOP_LEFT_ARM, NESTED_LOOP_RIGHT_ARM);

static NESTED_LOOP_CONTROLLER_PROGRAM: RoleProgram<'static, 0> = project(&NESTED_LOOP_PROGRAM);

fn route_resolver(ctx: ResolverContext) -> Result<DynamicResolution, ResolverError> {
    if ctx.attr(core::TAG).map(|value| value.as_u8()) != Some(RouteDecisionKind::TAG) {
        return Err(ResolverError::Reject);
    }
    if route_allow() {
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
    with_fixture(|clock, tap_buf, slab| {
        with_tls_ref(
            &SESSION_SLOT,
            |ptr| unsafe {
                ptr.write(SessionKit::new(clock));
            },
            |cluster| {
                let config = Config::new(tap_buf, slab);
                let transport = TestTransport::default();

                let rv_id = cluster
                    .add_rendezvous_from_config(config, transport.clone())
                    .expect("register rendezvous");
                cluster
                    .set_resolver::<ROUTE_POLICY_ID, 0, _>(
                        rv_id,
                        &CONTROLLER_PROGRAM,
                        hibana::substrate::policy::ResolverRef::from_fn(route_resolver),
                    )
                    .expect("register route resolver");

                let sid = SessionId::new(7);

                with_tls_mut(
                    &WORKER_ENDPOINT_SLOT,
                    |ptr| unsafe {
                        write_value(
                            ptr,
                            cluster
                                .enter(rv_id, sid, &WORKER_PROGRAM, NoBinding)
                                .expect("worker endpoint"),
                        );
                    },
                    |_worker_endpoint| {
                        with_tls_mut(
                            &CONTROLLER_ENDPOINT_SLOT,
                            |ptr| unsafe {
                                write_value(
                                    ptr,
                                    cluster
                                        .enter(rv_id, sid, &CONTROLLER_PROGRAM, NoBinding)
                                        .expect("controller endpoint"),
                                );
                            },
                            |controller_cursor| {
                                set_route_allow(false);
                                block_on_async(async {
                                    let first_flow = controller_cursor
                                        .flow::<Msg<
                                            { LABEL_ROUTE_DECISION },
                                            GenericCapToken<RouteDecisionKind>,
                                            CanonicalControl<RouteDecisionKind>,
                                        >>()
                                        .expect("self-send route flow should be available");
                                    let first_outcome = first_flow
                                        .send(())
                                        .await
                                        .expect("self-send route should not re-evaluate disallowed resolver");
                                    assert!(first_outcome.is_canonical());
                                });
                            },
                        );
                    },
                );

                set_route_allow(true);

                let sid2 = SessionId::new(8);

                with_tls_mut(
                    &WORKER_ENDPOINT_SLOT,
                    |ptr| unsafe {
                        write_value(
                            ptr,
                            cluster
                                .enter(rv_id, sid2, &WORKER_PROGRAM, NoBinding)
                                .expect("worker endpoint (retry)"),
                        );
                    },
                    |_worker_endpoint| {
                        with_tls_mut(
                            &CONTROLLER_ENDPOINT_SLOT,
                            |ptr| unsafe {
                                write_value(
                                    ptr,
                                    cluster
                                        .enter(rv_id, sid2, &CONTROLLER_PROGRAM, NoBinding)
                                        .expect("controller endpoint (retry)"),
                                );
                            },
                            |controller_cursor| {
                                block_on_async(async {
                                    let send_flow = controller_cursor
                                        .flow::<Msg<
                                            { LABEL_ROUTE_DECISION },
                                            GenericCapToken<RouteDecisionKind>,
                                            CanonicalControl<RouteDecisionKind>,
                                        >>()
                                        .expect("route should proceed when allowed");

                                    let outcome =
                                        send_flow.send(()).await.expect("send route decision");
                                    assert!(outcome.is_canonical());
                                });
                            },
                        );
                    },
                );

                assert!(transport_queue_is_empty(&transport));
            },
        );
    });
}

#[test]
fn route_head_policy_ignores_later_arm_dynamic_controls_on_enter() {
    with_fixture(|clock, tap_buf, slab| {
        with_tls_ref(
            &SESSION_SLOT,
            |ptr| unsafe {
                ptr.write(SessionKit::new(clock));
            },
            |cluster| {
                let config = Config::new(tap_buf, slab);
                let transport = TestTransport::default();

                let rv_id = cluster
                    .add_rendezvous_from_config(config, transport.clone())
                    .expect("register rendezvous");
                cluster
                    .set_resolver::<ROUTE_POLICY_ID, 0, _>(
                        rv_id,
                        &ROUTE_TAIL_CONTROLLER_PROGRAM,
                        hibana::substrate::policy::ResolverRef::from_fn(route_resolver),
                    )
                    .expect("register route resolver");
                set_route_allow(true);

                let sid = SessionId::new(10);
                with_tls_mut(
                    &WORKER_ENDPOINT_SLOT,
                    |ptr| unsafe {
                        write_value(
                            ptr,
                            cluster
                                .enter(rv_id, sid, &ROUTE_TAIL_WORKER_PROGRAM, NoBinding)
                                .expect("worker endpoint"),
                        );
                    },
                    |_worker| {
                        with_tls_mut(
                            &CONTROLLER_ENDPOINT_SLOT,
                            |ptr| unsafe {
                                write_value(
                                    ptr,
                                    cluster
                                        .enter(
                                            rv_id,
                                            sid,
                                            &ROUTE_TAIL_CONTROLLER_PROGRAM,
                                            NoBinding,
                                        )
                                        .expect("controller endpoint"),
                                );
                            },
                            |controller| {
                                let _route_flow = controller
                                    .flow::<Msg<
                                        { LABEL_ROUTE_DECISION },
                                        GenericCapToken<RouteDecisionKind>,
                                        CanonicalControl<RouteDecisionKind>,
                                    >>()
                                    .expect("route flow should remain available after enter");
                            },
                        );
                    },
                );

                set_route_allow(false);
                assert!(transport_queue_is_empty(&transport));
            },
        );
    });
}

#[test]
fn route_token_arm_matches_offer_when_policy_input_changes_before_send() {
    with_fixture(|clock, tap_buf, slab| {
        with_tls_ref(
            &SESSION_SLOT,
            |ptr| unsafe {
                ptr.write(SessionKit::new(clock));
            },
            |cluster| {
                with_tls_mut(
                    &POLICY_INPUT_SLOT,
                    |ptr: *mut Cell<u32>| unsafe { ptr.write(Cell::new(0)) },
                    |policy_input0| {
                        let policy_input: &'static Cell<u32> = policy_input0;
                        with_tls_mut(
                            &POLICY_BINDING_SLOT,
                            |ptr: *mut PolicyInputBinding| unsafe {
                                ptr.write(PolicyInputBinding::new(policy_input))
                            },
                            |controller_binding| {
                                let config = Config::new(tap_buf, slab);
                                let transport = TestTransport::default();
                                let rv_id = cluster
                                    .add_rendezvous_from_config(config, transport.clone())
                                    .expect("register rendezvous");

                                cluster
                                    .set_resolver::<ROUTE_POLICY_ID, 0, _>(
                                        rv_id,
                                        &CONTROLLER_PROGRAM,
                                        hibana::substrate::policy::ResolverRef::from_fn(
                                            route_policy_input_resolver,
                                        ),
                                    )
                                    .expect("register route resolver");

                                let sid = SessionId::new(9);
                                with_tls_mut(
                                    &WORKER_ENDPOINT_SLOT,
                                    |ptr| unsafe {
                                        write_value(
                                            ptr,
                                            cluster
                                                .enter(rv_id, sid, &WORKER_PROGRAM, NoBinding)
                                                .expect("worker endpoint"),
                                        );
                                    },
                                    |_worker| {
                                        with_tls_mut(
                                            &CONTROLLER_ENDPOINT_SLOT,
                                            |ptr| unsafe {
                                                write_value(
                                                    ptr,
                                                    cluster
                                                        .enter(
                                                            rv_id,
                                                            sid,
                                                            &CONTROLLER_PROGRAM,
                                                            controller_binding,
                                                        )
                                                        .expect("controller endpoint"),
                                                );
                                            },
                                            |controller| {
                                                block_on_async(async {
                                                    let send_flow = controller
                                                        .flow::<Msg<
                                                            { LABEL_ROUTE_DECISION },
                                                            GenericCapToken<RouteDecisionKind>,
                                                            CanonicalControl<RouteDecisionKind>,
                                                        >>(
                                                        )
                                                        .expect("route should select left arm");

                                                    policy_input.set(1);

                                                    let outcome = send_flow
                                                        .send(())
                                                        .await
                                                        .expect("send route decision");
                                                    let handle = outcome
                                                        .into_canonical()
                                                        .expect(
                                                            "expected canonical control token",
                                                        )
                                                        .as_generic()
                                                        .decode_handle()
                                                        .expect(
                                                            "decode canonical route decision handle",
                                                        );

                                                    assert_eq!(
                                                        handle.arm, 0,
                                                        "token arm must remain equal to offer-selected arm"
                                                    );
                                                    assert!(
                                                        !handle.scope.is_none(),
                                                        "canonical route decision handle must carry a materialized scope"
                                                    );
                                                });
                                            },
                                        );
                                    },
                                );
                                assert!(transport_queue_is_empty(&transport));
                            },
                        )
                    },
                );
            },
        );
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
    let _controller_program = &LOOP_CONTROLLER_PROGRAM;
}

/// Test nested routes with flow().send(()) pattern.
///
/// With self-send CanonicalControl (Controller → Controller), all route decisions
/// are local to the Controller role. Worker doesn't participate in route control.
#[test]
fn nested_loop_dynamic_send_and_offer() {
    with_fixture(|clock, tap_buf, slab| {
        let config = Config::new(tap_buf, slab);
        let transport = TestTransport::default();
        with_tls_ref(
            &SESSION_SLOT,
            |ptr| unsafe {
                ptr.write(SessionKit::new(clock));
            },
            |cluster| {
                let _rv_id = cluster
                    .add_rendezvous_from_config(config, transport.clone())
                    .expect("register rendezvous");

                let _controller_program = &NESTED_LOOP_CONTROLLER_PROGRAM;
            },
        );

        assert!(transport_queue_is_empty(&transport));
    });
}
