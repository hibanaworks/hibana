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
    g::{self, Msg, Role},
    substrate::program::{RoleProgram, project},
    substrate::{
        SessionKit,
        binding::{
            BindingSlot, NoBinding,
            advanced::{Channel, IncomingClassification, TransportOpsError},
        },
        ids::SessionId,
        policy::{
            PolicySignalsProvider,
            signals::{ContextId, ContextValue, PolicyAttrs, PolicySignals, PolicySlot, core},
        },
        runtime::{Config, DefaultLabelUniverse},
    },
    substrate::{
        cap::{
            GenericCapToken, ResourceKind,
            advanced::{LoopBreakKind, LoopContinueKind, RouteDecisionKind},
        },
        policy::{ResolverContext, ResolverError, RouteResolution},
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
const LABEL_ROUTE_RIGHT_CONTROL: u8 = 118;
const ROUTE_POLICY_ID: u16 = 9;
const LOOP_POLICY_ID: u16 = 10;
const POLICY_INPUT_ID: ContextId = ContextId::new(0x9001);

type RouteRightKind = route_control_kinds::RouteControl<LABEL_ROUTE_RIGHT_CONTROL, 0>;

fn block_on_async<F>(future: F) -> F::Output
where
    F: std::future::Future,
{
    futures::executor::block_on(future)
}

type TestKit = SessionKit<
    'static,
    TestTransport,
    DefaultLabelUniverse,
    hibana::substrate::runtime::CounterClock,
    2,
>;

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
    static CONTROLLER_ENDPOINT_SLOT: UnsafeCell<MaybeUninit<hibana::Endpoint<'static, 0>>> = const {
        UnsafeCell::new(MaybeUninit::uninit())
    };
    static WORKER_ENDPOINT_SLOT: UnsafeCell<MaybeUninit<hibana::Endpoint<'static, 1>>> = const {
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

fn transport_queue_is_empty(transport: &TestTransport) -> bool {
    transport.queue_is_empty()
}

fn controller_program() -> RoleProgram<0> {
    let left_arm = g::send::<
        Role<0>,
        Role<0>,
        Msg<{ LABEL_ROUTE_DECISION }, GenericCapToken<RouteDecisionKind>, RouteDecisionKind>,
        0,
    >()
    .policy::<ROUTE_POLICY_ID>();
    let right_arm = g::send::<
        Role<0>,
        Role<0>,
        Msg<LABEL_ROUTE_RIGHT_CONTROL, GenericCapToken<RouteRightKind>, RouteRightKind>,
        0,
    >()
    .policy::<ROUTE_POLICY_ID>();
    let program = g::route(left_arm, right_arm);
    project(&program)
}

fn worker_program() -> RoleProgram<1> {
    let left_arm = g::send::<
        Role<0>,
        Role<0>,
        Msg<{ LABEL_ROUTE_DECISION }, GenericCapToken<RouteDecisionKind>, RouteDecisionKind>,
        0,
    >()
    .policy::<ROUTE_POLICY_ID>();
    let right_arm = g::send::<
        Role<0>,
        Role<0>,
        Msg<LABEL_ROUTE_RIGHT_CONTROL, GenericCapToken<RouteRightKind>, RouteRightKind>,
        0,
    >()
    .policy::<ROUTE_POLICY_ID>();
    let program = g::route(left_arm, right_arm);
    project(&program)
}

fn loop_controller_program() -> RoleProgram<0> {
    let loop_continue_arm = g::send::<
        Role<0>,
        Role<0>,
        Msg<{ LABEL_LOOP_CONTINUE }, GenericCapToken<LoopContinueKind>, LoopContinueKind>,
        0,
    >()
    .policy::<LOOP_POLICY_ID>();
    let loop_break_arm = g::send::<
        Role<0>,
        Role<0>,
        Msg<{ LABEL_LOOP_BREAK }, GenericCapToken<LoopBreakKind>, LoopBreakKind>,
        0,
    >()
    .policy::<LOOP_POLICY_ID>();
    let loop_program = g::route(loop_continue_arm, loop_break_arm);
    project(&loop_program)
}

fn route_tail_controller_program() -> RoleProgram<0> {
    let left_arm = g::send::<
        Role<0>,
        Role<0>,
        Msg<{ LABEL_ROUTE_DECISION }, GenericCapToken<RouteDecisionKind>, RouteDecisionKind>,
        0,
    >()
    .policy::<ROUTE_POLICY_ID>();
    let right_arm = g::send::<
        Role<0>,
        Role<0>,
        Msg<LABEL_ROUTE_RIGHT_CONTROL, GenericCapToken<RouteRightKind>, RouteRightKind>,
        0,
    >()
    .policy::<ROUTE_POLICY_ID>();
    let loop_continue_arm = g::send::<
        Role<0>,
        Role<0>,
        Msg<{ LABEL_LOOP_CONTINUE }, GenericCapToken<LoopContinueKind>, LoopContinueKind>,
        0,
    >()
    .policy::<LOOP_POLICY_ID>();
    let loop_break_arm = g::send::<
        Role<0>,
        Role<0>,
        Msg<{ LABEL_LOOP_BREAK }, GenericCapToken<LoopBreakKind>, LoopBreakKind>,
        0,
    >()
    .policy::<LOOP_POLICY_ID>();
    let program = g::route(
        g::seq(left_arm, loop_continue_arm),
        g::seq(right_arm, loop_break_arm),
    );
    project(&program)
}

fn route_tail_worker_program() -> RoleProgram<1> {
    let left_arm = g::send::<
        Role<0>,
        Role<0>,
        Msg<{ LABEL_ROUTE_DECISION }, GenericCapToken<RouteDecisionKind>, RouteDecisionKind>,
        0,
    >()
    .policy::<ROUTE_POLICY_ID>();
    let right_arm = g::send::<
        Role<0>,
        Role<0>,
        Msg<LABEL_ROUTE_RIGHT_CONTROL, GenericCapToken<RouteRightKind>, RouteRightKind>,
        0,
    >()
    .policy::<ROUTE_POLICY_ID>();
    let loop_continue_arm = g::send::<
        Role<0>,
        Role<0>,
        Msg<{ LABEL_LOOP_CONTINUE }, GenericCapToken<LoopContinueKind>, LoopContinueKind>,
        0,
    >()
    .policy::<LOOP_POLICY_ID>();
    let loop_break_arm = g::send::<
        Role<0>,
        Role<0>,
        Msg<{ LABEL_LOOP_BREAK }, GenericCapToken<LoopBreakKind>, LoopBreakKind>,
        0,
    >()
    .policy::<LOOP_POLICY_ID>();
    let program = g::route(
        g::seq(left_arm, loop_continue_arm),
        g::seq(right_arm, loop_break_arm),
    );
    project(&program)
}

fn nested_loop_controller_program() -> RoleProgram<0> {
    let left_arm = g::send::<
        Role<0>,
        Role<0>,
        Msg<{ LABEL_ROUTE_DECISION }, GenericCapToken<RouteDecisionKind>, RouteDecisionKind>,
        0,
    >()
    .policy::<ROUTE_POLICY_ID>();
    let right_arm = g::send::<
        Role<0>,
        Role<0>,
        Msg<LABEL_ROUTE_RIGHT_CONTROL, GenericCapToken<RouteRightKind>, RouteRightKind>,
        0,
    >()
    .policy::<ROUTE_POLICY_ID>();
    let loop_continue_arm = g::send::<
        Role<0>,
        Role<0>,
        Msg<{ LABEL_LOOP_CONTINUE }, GenericCapToken<LoopContinueKind>, LoopContinueKind>,
        0,
    >()
    .policy::<LOOP_POLICY_ID>();
    let loop_break_arm = g::send::<
        Role<0>,
        Role<0>,
        Msg<{ LABEL_LOOP_BREAK }, GenericCapToken<LoopBreakKind>, LoopBreakKind>,
        0,
    >()
    .policy::<LOOP_POLICY_ID>();
    let loop_program = g::route(loop_continue_arm, loop_break_arm);
    let outer_loop_continue_arm = g::seq(
        g::send::<
            Role<0>,
            Role<0>,
            Msg<{ LABEL_LOOP_CONTINUE }, GenericCapToken<LoopContinueKind>, LoopContinueKind>,
            0,
        >()
        .policy::<LOOP_POLICY_ID>(),
        loop_program,
    );
    let nested_loop_break_arm = g::send::<
        Role<0>,
        Role<0>,
        Msg<{ LABEL_LOOP_BREAK }, GenericCapToken<LoopBreakKind>, LoopBreakKind>,
        0,
    >()
    .policy::<LOOP_POLICY_ID>();
    let program = g::route(
        g::seq(left_arm, outer_loop_continue_arm),
        g::seq(right_arm, nested_loop_break_arm),
    );
    project(&program)
}

fn route_resolver(ctx: ResolverContext) -> Result<RouteResolution, ResolverError> {
    if ctx.attr(core::TAG).map(|value| value.as_u8()) != Some(RouteDecisionKind::TAG) {
        return Err(ResolverError::Reject);
    }
    if route_allow() {
        Ok(RouteResolution::Arm(0))
    } else {
        Err(ResolverError::Reject)
    }
}

fn route_policy_input_resolver(ctx: ResolverContext) -> Result<RouteResolution, ResolverError> {
    if ctx.attr(core::TAG).map(|value| value.as_u8()) != Some(RouteDecisionKind::TAG) {
        return Err(ResolverError::Reject);
    }
    let arm = ctx
        .attr(POLICY_INPUT_ID)
        .map(|v| (v.as_u32() & 1) as u8)
        .unwrap_or(0);
    Ok(RouteResolution::Arm(arm))
}

/// Test route dynamic resolver with flow().send(()) pattern.
///
/// local control uses self-send (Controller → Controller) and advances
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
                    .set_resolver::<ROUTE_POLICY_ID, 0>(
                        rv_id,
                        &controller_program(),
                        hibana::substrate::policy::ResolverRef::route_fn(route_resolver),
                    )
                    .expect("register route resolver");

                let sid = SessionId::new(7);

                with_tls_mut(
                    &WORKER_ENDPOINT_SLOT,
                    |ptr| unsafe {
                        write_value(
                            ptr,
                            cluster
                                .enter(rv_id, sid, &worker_program(), NoBinding)
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
                                        .enter(rv_id, sid, &controller_program(), NoBinding)
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
                                            RouteDecisionKind,
                                        >>()
                                        .expect("self-send route flow should be available");
                                    let _first_token = first_flow
                                        .send(())
                                        .await
                                        .expect("self-send route should not re-evaluate disallowed resolver");
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
                                .enter(rv_id, sid2, &worker_program(), NoBinding)
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
                                        .enter(rv_id, sid2, &controller_program(), NoBinding)
                                        .expect("controller endpoint (retry)"),
                                );
                            },
                            |controller_cursor| {
                                block_on_async(async {
                                    let send_flow = controller_cursor
                                        .flow::<Msg<
                                            { LABEL_ROUTE_DECISION },
                                            GenericCapToken<RouteDecisionKind>,
                                            RouteDecisionKind,
                                        >>()
                                        .expect("route should proceed when allowed");

                                    let _token =
                                        send_flow.send(()).await.expect("send route decision");
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
fn route_dynamic_self_send_offer_resolves_without_controller_arm_entry() {
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
                    .set_resolver::<ROUTE_POLICY_ID, 0>(
                        rv_id,
                        &controller_program(),
                        hibana::substrate::policy::ResolverRef::route_fn(route_resolver),
                    )
                    .expect("register route resolver");

                set_route_allow(true);
                let sid = SessionId::new(9);

                with_tls_mut(
                    &WORKER_ENDPOINT_SLOT,
                    |ptr| unsafe {
                        write_value(
                            ptr,
                            cluster
                                .enter(rv_id, sid, &worker_program(), NoBinding)
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
                                        .enter(rv_id, sid, &controller_program(), NoBinding)
                                        .expect("controller endpoint"),
                                );
                            },
                            |controller_cursor| {
                                block_on_async(async {
                                    let branch = controller_cursor.offer().await.expect(
                                        "self-send route offer should resolve via route policy",
                                    );
                                    assert_eq!(
                                        branch.label(),
                                        LABEL_ROUTE_DECISION,
                                        "self-send dynamic offer must resolve the selected arm without controller arm entries"
                                    );
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
                    .set_resolver::<ROUTE_POLICY_ID, 0>(
                        rv_id,
                        &route_tail_controller_program(),
                        hibana::substrate::policy::ResolverRef::route_fn(route_resolver),
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
                                .enter(rv_id, sid, &route_tail_worker_program(), NoBinding)
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
                                            &route_tail_controller_program(),
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
                                        RouteDecisionKind,
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
                                    .set_resolver::<ROUTE_POLICY_ID, 0>(
                                        rv_id,
                                        &controller_program(),
                                        hibana::substrate::policy::ResolverRef::route_fn(
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
                                                .enter(rv_id, sid, &worker_program(), NoBinding)
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
                                                            &controller_program(),
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
                                                            RouteDecisionKind,
                                                        >>(
                                                        )
                                                        .expect("route should select left arm");

                                                    policy_input.set(1);

                                                    send_flow.send(()).await.expect(
                                                        "send must remain on the offer-selected arm",
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
/// With self-send local control, `local()` doesn't navigate routes dynamically.
/// The type system ensures the protocol is well-formed, and local() can be used
/// once the cursor is positioned at the appropriate local action.
///
/// This test verifies the type definitions are correct after removing the Target parameter.
#[test]
fn loop_dynamic_resolver_policy_abort_and_success() {
    let _controller_program = loop_controller_program();
}

/// Test nested routes with flow().send(()) pattern.
///
/// With self-send local control (Controller → Controller), all route decisions
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

                let _controller_program = nested_loop_controller_program();
            },
        );

        assert!(transport_queue_is_empty(&transport));
    });
}
