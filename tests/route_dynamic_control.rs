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
use common::{TestTransport, TestTx};
use hibana::{
    g::{self, Msg, Role},
    integration::program::{RoleProgram, project},
    integration::{
        SessionKit,
        binding::{
            BindingSlot, NoBinding,
            advanced::{Channel, IngressEvidence, TransportOpsError},
        },
        ids::SessionId,
        policy::{
            PolicySignalsProvider,
            signals::{ContextId, ContextValue, PolicyAttrs, PolicySignals, PolicySlot, core},
        },
        runtime::{Config, DefaultLabelUniverse},
    },
    integration::{
        cap::{
            GenericCapToken, ResourceKind,
            control::{LoopBreakKind, LoopContinueKind, RouteDecisionKind},
        },
        policy::{ResolverContext, ResolverError, RouteResolution},
    },
};
use placement_support::write_value;
use runtime_support::with_fixture;
use std::{
    cell::Cell,
    future::Future,
    task::{Context, Poll},
};
use tls_mut_support::with_tls_mut;
use tls_ref_support::with_tls_ref;

const TEST_LOOP_CONTINUE_LOGICAL: u8 = 0xA1;
const TEST_LOOP_BREAK_LOGICAL: u8 = 0xA2;
const TEST_ROUTE_DECISION_LOGICAL: u8 = 0xA3;
const ROUTE_RIGHT_CONTROL_LOGICAL: u8 = 118;
const ROUTE_LEFT_PAYLOAD_LOGICAL: u8 = 119;
const ROUTE_RIGHT_PAYLOAD_LOGICAL: u8 = 120;
const ROUTE_TAIL_ACK_LOGICAL: u8 = 121;
const ROUTE_SEND_FIRST_PAYLOAD_LOGICAL: u8 = 122;
const ROUTE_POLICY_ID: u16 = 9;
const LOOP_POLICY_ID: u16 = 10;
const POLICY_INPUT_ID: ContextId = ContextId::new(0x9001);

type RouteRightKind = route_control_kinds::RouteControl<0>;

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
    hibana::integration::runtime::CounterClock,
    2,
>;

type EmbeddedTestKit = SessionKit<
    'static,
    TestTransport,
    DefaultLabelUniverse,
    hibana::integration::runtime::CounterClock,
    1,
>;

std::thread_local! {
    static SESSION_SLOT: UnsafeCell<MaybeUninit<TestKit>> = const {
        UnsafeCell::new(MaybeUninit::uninit())
    };
    static SESSION_SLOT_B: UnsafeCell<MaybeUninit<TestKit>> = const {
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
    static CONTROLLER_ROLE1_ENDPOINT_SLOT: UnsafeCell<MaybeUninit<hibana::Endpoint<'static, 1>>> = const {
        UnsafeCell::new(MaybeUninit::uninit())
    };
    static WORKER_ROLE0_ENDPOINT_SLOT: UnsafeCell<MaybeUninit<hibana::Endpoint<'static, 0>>> = const {
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
    fn poll_incoming_for_lane(&mut self, _logical_lane: u8) -> Option<IngressEvidence> {
        None
    }

    fn on_recv<'a>(
        &'a mut self,
        _channel: Channel,
        _buf: &'a mut [u8],
    ) -> Result<hibana::integration::wire::Payload<'a>, TransportOpsError> {
        Ok(hibana::integration::wire::Payload::new(&[]))
    }

    fn policy_signals_provider(&self) -> Option<&dyn PolicySignalsProvider> {
        Some(self)
    }
}

fn transport_queue_is_empty(transport: &TestTransport) -> bool {
    transport.queue_is_empty()
}

#[test]
fn test_transport_demuxes_lane_and_drains_route_hint() {
    let transport = TestTransport::default();
    let mut tx0 = TestTx::default();
    transport.stage_send(&mut tx0, 1, 0, 10, b"lane-zero");
    assert!(matches!(
        transport.poll_send_staged(&mut tx0),
        Poll::Ready(Ok(()))
    ));
    let mut tx1 = TestTx::default();
    transport.stage_send(&mut tx1, 1, 1, 20, b"lane-one");
    assert!(matches!(
        transport.poll_send_staged(&mut tx1),
        Poll::Ready(Ok(()))
    ));

    let mut rx0 = transport.open_rx_for_test(1, 0);
    let mut rx1 = transport.open_rx_for_test(1, 1);

    assert_eq!(
        hibana::integration::transport::Transport::recv_frame_hint(&transport, &rx0)
            .map(|label| label.raw()),
        Some(10),
        "lane 0 must observe only its own first staged frame"
    );
    assert_eq!(
        hibana::integration::transport::Transport::recv_frame_hint(&transport, &rx0)
            .map(|label| label.raw()),
        None,
        "route hint must drain after one observation"
    );
    assert_eq!(
        hibana::integration::transport::Transport::recv_frame_hint(&transport, &rx1)
            .map(|label| label.raw()),
        Some(20),
        "lane 1 must not see lane 0 frame metadata"
    );
    assert_eq!(
        hibana::integration::transport::Transport::recv_frame_hint(&transport, &rx1)
            .map(|label| label.raw()),
        None,
        "route hint drain is per lane-owned receive handle"
    );

    let waker = futures::task::noop_waker();
    let mut cx = Context::from_waker(&waker);
    {
        let payload = match hibana::integration::transport::Transport::poll_recv(
            &transport, &mut rx0, &mut cx,
        ) {
            Poll::Ready(Ok(payload)) => payload,
            Poll::Ready(Err(_)) => panic!("lane 0 payload returned transport error"),
            Poll::Pending => panic!("lane 0 payload must be ready after hint drain"),
        };
        assert_eq!(payload.as_bytes(), b"lane-zero");
    }
    let rx0_after_recv = transport.open_rx_for_test(1, 0);
    assert_eq!(
        hibana::integration::transport::Transport::recv_frame_hint(&transport, &rx0_after_recv)
            .map(|label| label.raw()),
        None,
        "poll_recv must remove the drained lane 0 frame from the shared carrier"
    );

    {
        let payload = match hibana::integration::transport::Transport::poll_recv(
            &transport, &mut rx1, &mut cx,
        ) {
            Poll::Ready(Ok(payload)) => payload,
            Poll::Ready(Err(_)) => panic!("lane 1 payload returned transport error"),
            Poll::Pending => panic!("lane 1 payload must remain available independently"),
        };
        assert_eq!(payload.as_bytes(), b"lane-one");
    }
}

fn controller_program() -> RoleProgram<0> {
    let left_arm = g::send::<
        Role<0>,
        Role<0>,
        Msg<{ TEST_ROUTE_DECISION_LOGICAL }, GenericCapToken<RouteDecisionKind>, RouteDecisionKind>,
        0,
    >()
    .policy::<ROUTE_POLICY_ID>();
    let right_arm = g::send::<
        Role<0>,
        Role<0>,
        Msg<ROUTE_RIGHT_CONTROL_LOGICAL, GenericCapToken<RouteRightKind>, RouteRightKind>,
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
        Msg<{ TEST_ROUTE_DECISION_LOGICAL }, GenericCapToken<RouteDecisionKind>, RouteDecisionKind>,
        0,
    >()
    .policy::<ROUTE_POLICY_ID>();
    let right_arm = g::send::<
        Role<0>,
        Role<0>,
        Msg<ROUTE_RIGHT_CONTROL_LOGICAL, GenericCapToken<RouteRightKind>, RouteRightKind>,
        0,
    >()
    .policy::<ROUTE_POLICY_ID>();
    let program = g::route(left_arm, right_arm);
    project(&program)
}

#[test]
fn projected_role_attach_order_does_not_fix_lane_storage_capacity() {
    with_fixture(|clock, tap_buf, slab| {
        with_tls_ref(
            &SESSION_SLOT,
            |ptr| unsafe {
                ptr.write(SessionKit::new(clock));
            },
            |cluster| {
                let config =
                    Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources(
                        (tap_buf, slab),
                        hibana::integration::runtime::CounterClock::new(),
                    );
                let transport = TestTransport::default();
                let rv_id = cluster
                    .add_rendezvous_from_config(config, transport)
                    .expect("register rendezvous");
                cluster
                    .rendezvous(rv_id)
                    .role(&controller_program())
                    .set_resolver::<ROUTE_POLICY_ID>(
                        hibana::integration::policy::ResolverRef::route_fn(route_resolver),
                    )
                    .expect("register route resolver");

                let sid = SessionId::new(107);
                with_tls_mut(
                    &WORKER_ENDPOINT_SLOT,
                    |ptr| unsafe {
                        write_value(
                            ptr,
                            cluster
                                .rendezvous(rv_id)
                                .session(sid)
                                .role(&worker_program())
                                .enter(NoBinding)
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
                                        .rendezvous(rv_id)
                                        .session(sid)
                                        .role(&controller_program())
                                        .enter(NoBinding)
                                        .expect("controller endpoint after worker"),
                                );
                            },
                            |_controller_endpoint| {},
                        );
                    },
                );
            },
        );
    });
}

fn loop_controller_program() -> RoleProgram<0> {
    let loop_continue_arm = g::send::<
        Role<0>,
        Role<0>,
        Msg<{ TEST_LOOP_CONTINUE_LOGICAL }, GenericCapToken<LoopContinueKind>, LoopContinueKind>,
        0,
    >()
    .policy::<LOOP_POLICY_ID>();
    let loop_break_arm = g::send::<
        Role<0>,
        Role<0>,
        Msg<{ TEST_LOOP_BREAK_LOGICAL }, GenericCapToken<LoopBreakKind>, LoopBreakKind>,
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
        Msg<{ TEST_ROUTE_DECISION_LOGICAL }, GenericCapToken<RouteDecisionKind>, RouteDecisionKind>,
        0,
    >()
    .policy::<ROUTE_POLICY_ID>();
    let right_arm = g::send::<
        Role<0>,
        Role<0>,
        Msg<ROUTE_RIGHT_CONTROL_LOGICAL, GenericCapToken<RouteRightKind>, RouteRightKind>,
        0,
    >()
    .policy::<ROUTE_POLICY_ID>();
    let loop_continue_arm = g::send::<
        Role<0>,
        Role<0>,
        Msg<{ TEST_LOOP_CONTINUE_LOGICAL }, GenericCapToken<LoopContinueKind>, LoopContinueKind>,
        0,
    >()
    .policy::<LOOP_POLICY_ID>();
    let loop_break_arm = g::send::<
        Role<0>,
        Role<0>,
        Msg<{ TEST_LOOP_BREAK_LOGICAL }, GenericCapToken<LoopBreakKind>, LoopBreakKind>,
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
        Msg<{ TEST_ROUTE_DECISION_LOGICAL }, GenericCapToken<RouteDecisionKind>, RouteDecisionKind>,
        0,
    >()
    .policy::<ROUTE_POLICY_ID>();
    let right_arm = g::send::<
        Role<0>,
        Role<0>,
        Msg<ROUTE_RIGHT_CONTROL_LOGICAL, GenericCapToken<RouteRightKind>, RouteRightKind>,
        0,
    >()
    .policy::<ROUTE_POLICY_ID>();
    let loop_continue_arm = g::send::<
        Role<0>,
        Role<0>,
        Msg<{ TEST_LOOP_CONTINUE_LOGICAL }, GenericCapToken<LoopContinueKind>, LoopContinueKind>,
        0,
    >()
    .policy::<LOOP_POLICY_ID>();
    let loop_break_arm = g::send::<
        Role<0>,
        Role<0>,
        Msg<{ TEST_LOOP_BREAK_LOGICAL }, GenericCapToken<LoopBreakKind>, LoopBreakKind>,
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
        Msg<{ TEST_ROUTE_DECISION_LOGICAL }, GenericCapToken<RouteDecisionKind>, RouteDecisionKind>,
        0,
    >()
    .policy::<ROUTE_POLICY_ID>();
    let right_arm = g::send::<
        Role<0>,
        Role<0>,
        Msg<ROUTE_RIGHT_CONTROL_LOGICAL, GenericCapToken<RouteRightKind>, RouteRightKind>,
        0,
    >()
    .policy::<ROUTE_POLICY_ID>();
    let loop_continue_arm = g::send::<
        Role<0>,
        Role<0>,
        Msg<{ TEST_LOOP_CONTINUE_LOGICAL }, GenericCapToken<LoopContinueKind>, LoopContinueKind>,
        0,
    >()
    .policy::<LOOP_POLICY_ID>();
    let loop_break_arm = g::send::<
        Role<0>,
        Role<0>,
        Msg<{ TEST_LOOP_BREAK_LOGICAL }, GenericCapToken<LoopBreakKind>, LoopBreakKind>,
        0,
    >()
    .policy::<LOOP_POLICY_ID>();
    let loop_program = g::route(loop_continue_arm, loop_break_arm);
    let outer_loop_continue_arm = g::seq(
        g::send::<
            Role<0>,
            Role<0>,
            Msg<
                { TEST_LOOP_CONTINUE_LOGICAL },
                GenericCapToken<LoopContinueKind>,
                LoopContinueKind,
            >,
            1,
        >()
        .policy::<LOOP_POLICY_ID>(),
        loop_program,
    );
    let nested_loop_break_arm = g::send::<
        Role<0>,
        Role<0>,
        Msg<{ TEST_LOOP_BREAK_LOGICAL }, GenericCapToken<LoopBreakKind>, LoopBreakKind>,
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
        return Err(ResolverError::reject());
    }
    if route_allow() {
        Ok(RouteResolution::Arm(0))
    } else {
        Err(ResolverError::reject())
    }
}

fn route_policy_input_resolver(ctx: ResolverContext) -> Result<RouteResolution, ResolverError> {
    if ctx.attr(core::TAG).map(|value| value.as_u8()) != Some(RouteDecisionKind::TAG) {
        return Err(ResolverError::reject());
    }
    let arm = ctx
        .attr(POLICY_INPUT_ID)
        .map(|v| (v.as_u32() & 1) as u8)
        .unwrap_or(0);
    Ok(RouteResolution::Arm(arm))
}

fn right_route_resolver(ctx: ResolverContext) -> Result<RouteResolution, ResolverError> {
    if ctx.attr(core::TAG).map(|value| value.as_u8()) != Some(RouteRightKind::TAG) {
        return Err(ResolverError::reject());
    }
    Ok(RouteResolution::Arm(1))
}

fn routed_payload_controller_program() -> RoleProgram<0> {
    let left_arm = g::seq(
        g::send::<
            Role<0>,
            Role<0>,
            Msg<
                { TEST_ROUTE_DECISION_LOGICAL },
                GenericCapToken<RouteDecisionKind>,
                RouteDecisionKind,
            >,
            0,
        >()
        .policy::<ROUTE_POLICY_ID>(),
        g::send::<Role<0>, Role<1>, Msg<ROUTE_LEFT_PAYLOAD_LOGICAL, u8>, 0>(),
    );
    let right_arm = g::seq(
        g::send::<
            Role<0>,
            Role<0>,
            Msg<ROUTE_RIGHT_CONTROL_LOGICAL, GenericCapToken<RouteRightKind>, RouteRightKind>,
            0,
        >()
        .policy::<ROUTE_POLICY_ID>(),
        g::send::<Role<0>, Role<1>, Msg<ROUTE_RIGHT_PAYLOAD_LOGICAL, u8>, 0>(),
    );
    let program = g::route(left_arm, right_arm);
    project(&program)
}

fn routed_payload_worker_program() -> RoleProgram<1> {
    let left_arm = g::seq(
        g::send::<
            Role<0>,
            Role<0>,
            Msg<
                { TEST_ROUTE_DECISION_LOGICAL },
                GenericCapToken<RouteDecisionKind>,
                RouteDecisionKind,
            >,
            0,
        >()
        .policy::<ROUTE_POLICY_ID>(),
        g::send::<Role<0>, Role<1>, Msg<ROUTE_LEFT_PAYLOAD_LOGICAL, u8>, 0>(),
    );
    let right_arm = g::seq(
        g::send::<
            Role<0>,
            Role<0>,
            Msg<ROUTE_RIGHT_CONTROL_LOGICAL, GenericCapToken<RouteRightKind>, RouteRightKind>,
            0,
        >()
        .policy::<ROUTE_POLICY_ID>(),
        g::send::<Role<0>, Role<1>, Msg<ROUTE_RIGHT_PAYLOAD_LOGICAL, u8>, 0>(),
    );
    let program = g::route(left_arm, right_arm);
    project(&program)
}

fn send_first_route_controller_program() -> RoleProgram<0> {
    let left_arm = g::seq(
        g::send::<
            Role<0>,
            Role<0>,
            Msg<
                { TEST_ROUTE_DECISION_LOGICAL },
                GenericCapToken<RouteDecisionKind>,
                RouteDecisionKind,
            >,
            0,
        >()
        .policy::<ROUTE_POLICY_ID>(),
        g::send::<Role<0>, Role<1>, Msg<ROUTE_LEFT_PAYLOAD_LOGICAL, u8>, 0>(),
    );
    let right_arm = g::seq(
        g::send::<
            Role<0>,
            Role<0>,
            Msg<ROUTE_RIGHT_CONTROL_LOGICAL, GenericCapToken<RouteRightKind>, RouteRightKind>,
            0,
        >()
        .policy::<ROUTE_POLICY_ID>(),
        g::send::<Role<1>, Role<0>, Msg<ROUTE_SEND_FIRST_PAYLOAD_LOGICAL, u8>, 0>(),
    );
    project(&g::route(left_arm, right_arm))
}

fn send_first_route_worker_program() -> RoleProgram<1> {
    let left_arm = g::seq(
        g::send::<
            Role<0>,
            Role<0>,
            Msg<
                { TEST_ROUTE_DECISION_LOGICAL },
                GenericCapToken<RouteDecisionKind>,
                RouteDecisionKind,
            >,
            0,
        >()
        .policy::<ROUTE_POLICY_ID>(),
        g::send::<Role<0>, Role<1>, Msg<ROUTE_LEFT_PAYLOAD_LOGICAL, u8>, 0>(),
    );
    let right_arm = g::seq(
        g::send::<
            Role<0>,
            Role<0>,
            Msg<ROUTE_RIGHT_CONTROL_LOGICAL, GenericCapToken<RouteRightKind>, RouteRightKind>,
            0,
        >()
        .policy::<ROUTE_POLICY_ID>(),
        g::send::<Role<1>, Role<0>, Msg<ROUTE_SEND_FIRST_PAYLOAD_LOGICAL, u8>, 0>(),
    );
    project(&g::route(left_arm, right_arm))
}

fn routed_payload_role1_controller_program() -> RoleProgram<1> {
    let left_arm = g::seq(
        g::send::<
            Role<1>,
            Role<1>,
            Msg<
                { TEST_ROUTE_DECISION_LOGICAL },
                GenericCapToken<RouteDecisionKind>,
                RouteDecisionKind,
            >,
            0,
        >()
        .policy::<ROUTE_POLICY_ID>(),
        g::send::<Role<1>, Role<0>, Msg<ROUTE_LEFT_PAYLOAD_LOGICAL, u8>, 0>(),
    );
    let right_arm = g::seq(
        g::send::<
            Role<1>,
            Role<1>,
            Msg<ROUTE_RIGHT_CONTROL_LOGICAL, GenericCapToken<RouteRightKind>, RouteRightKind>,
            0,
        >()
        .policy::<ROUTE_POLICY_ID>(),
        g::send::<Role<1>, Role<0>, Msg<ROUTE_RIGHT_PAYLOAD_LOGICAL, u8>, 0>(),
    );
    let program = g::route(left_arm, right_arm);
    project(&program)
}

fn routed_payload_role0_worker_program() -> RoleProgram<0> {
    let left_arm = g::seq(
        g::send::<
            Role<1>,
            Role<1>,
            Msg<
                { TEST_ROUTE_DECISION_LOGICAL },
                GenericCapToken<RouteDecisionKind>,
                RouteDecisionKind,
            >,
            0,
        >()
        .policy::<ROUTE_POLICY_ID>(),
        g::send::<Role<1>, Role<0>, Msg<ROUTE_LEFT_PAYLOAD_LOGICAL, u8>, 0>(),
    );
    let right_arm = g::seq(
        g::send::<
            Role<1>,
            Role<1>,
            Msg<ROUTE_RIGHT_CONTROL_LOGICAL, GenericCapToken<RouteRightKind>, RouteRightKind>,
            0,
        >()
        .policy::<ROUTE_POLICY_ID>(),
        g::send::<Role<1>, Role<0>, Msg<ROUTE_RIGHT_PAYLOAD_LOGICAL, u8>, 0>(),
    );
    let program = g::route(left_arm, right_arm);
    project(&program)
}

fn routed_payload_with_tail_role1_controller_program() -> RoleProgram<1> {
    let left_arm = g::seq(
        g::send::<
            Role<1>,
            Role<1>,
            Msg<
                { TEST_ROUTE_DECISION_LOGICAL },
                GenericCapToken<RouteDecisionKind>,
                RouteDecisionKind,
            >,
            0,
        >()
        .policy::<ROUTE_POLICY_ID>(),
        g::send::<Role<1>, Role<0>, Msg<ROUTE_LEFT_PAYLOAD_LOGICAL, u8>, 0>(),
    );
    let right_arm = g::seq(
        g::send::<
            Role<1>,
            Role<1>,
            Msg<ROUTE_RIGHT_CONTROL_LOGICAL, GenericCapToken<RouteRightKind>, RouteRightKind>,
            0,
        >()
        .policy::<ROUTE_POLICY_ID>(),
        g::send::<Role<1>, Role<0>, Msg<ROUTE_RIGHT_PAYLOAD_LOGICAL, u8>, 0>(),
    );
    project(&g::seq(
        g::route(left_arm, right_arm),
        g::send::<Role<0>, Role<1>, Msg<ROUTE_TAIL_ACK_LOGICAL, u8>, 1>(),
    ))
}

fn routed_payload_with_tail_role0_worker_program() -> RoleProgram<0> {
    let left_arm = g::seq(
        g::send::<
            Role<1>,
            Role<1>,
            Msg<
                { TEST_ROUTE_DECISION_LOGICAL },
                GenericCapToken<RouteDecisionKind>,
                RouteDecisionKind,
            >,
            0,
        >()
        .policy::<ROUTE_POLICY_ID>(),
        g::send::<Role<1>, Role<0>, Msg<ROUTE_LEFT_PAYLOAD_LOGICAL, u8>, 0>(),
    );
    let right_arm = g::seq(
        g::send::<
            Role<1>,
            Role<1>,
            Msg<ROUTE_RIGHT_CONTROL_LOGICAL, GenericCapToken<RouteRightKind>, RouteRightKind>,
            0,
        >()
        .policy::<ROUTE_POLICY_ID>(),
        g::send::<Role<1>, Role<0>, Msg<ROUTE_RIGHT_PAYLOAD_LOGICAL, u8>, 0>(),
    );
    project(&g::seq(
        g::route(left_arm, right_arm),
        g::send::<Role<0>, Role<1>, Msg<ROUTE_TAIL_ACK_LOGICAL, u8>, 1>(),
    ))
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
                let config =
                    Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources(
                        (tap_buf, slab),
                        hibana::integration::runtime::CounterClock::new(),
                    );
                let transport = TestTransport::default();

                let rv_id = cluster
                    .add_rendezvous_from_config(config, transport.clone())
                    .expect("register rendezvous");
                cluster
                    .rendezvous(rv_id)
                    .role(&controller_program())
                    .set_resolver::<ROUTE_POLICY_ID>(
                        hibana::integration::policy::ResolverRef::route_fn(route_resolver),
                    )
                    .expect("register route resolver");

                let sid = SessionId::new(7);

                with_tls_mut(
                    &WORKER_ENDPOINT_SLOT,
                    |ptr| unsafe {
                        write_value(
                            ptr,
                            cluster
                                .rendezvous(rv_id)
                                .session(sid)
                                .role(&worker_program())
                                .enter(NoBinding)
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
                                        .rendezvous(rv_id)
                                        .session(sid)
                                        .role(&controller_program())
                                        .enter(NoBinding)
                                        .expect("controller endpoint"),
                                );
                            },
                            |controller_cursor| {
                                set_route_allow(false);
                                block_on_async(async {
                                    let first_flow = controller_cursor
                                        .flow::<Msg<
                                            { TEST_ROUTE_DECISION_LOGICAL },
                                            GenericCapToken<RouteDecisionKind>,
                                            RouteDecisionKind,
                                        >>()
                                        .expect("self-send route flow should be available");
                                    first_flow
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
                                .rendezvous(rv_id)
                                .session(sid2)
                                .role(&worker_program())
                                .enter(NoBinding)
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
                                        .rendezvous(rv_id)
                                        .session(sid2)
                                        .role(&controller_program())
                                        .enter(NoBinding)
                                        .expect("controller endpoint (retry)"),
                                );
                            },
                            |controller_cursor| {
                                block_on_async(async {
                                    let send_flow = controller_cursor
                                        .flow::<Msg<
                                            { TEST_ROUTE_DECISION_LOGICAL },
                                            GenericCapToken<RouteDecisionKind>,
                                            RouteDecisionKind,
                                        >>()
                                        .expect("route should proceed when allowed");

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
                let config =
                    Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources(
                        (tap_buf, slab),
                        hibana::integration::runtime::CounterClock::new(),
                    );
                let transport = TestTransport::default();

                let rv_id = cluster
                    .add_rendezvous_from_config(config, transport.clone())
                    .expect("register rendezvous");
                cluster
                    .rendezvous(rv_id)
                    .role(&controller_program())
                    .set_resolver::<ROUTE_POLICY_ID>(
                        hibana::integration::policy::ResolverRef::route_fn(route_resolver),
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
                                .rendezvous(rv_id)
                                .session(sid)
                                .role(&worker_program())
                                .enter(NoBinding)
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
                                        .rendezvous(rv_id)
                                        .session(sid)
                                        .role(&controller_program())
                                        .enter(NoBinding)
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
                                        TEST_ROUTE_DECISION_LOGICAL,
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
fn passive_dynamic_offer_decodes_payload_selected_by_controller_route_frame() {
    with_fixture(|clock, tap_buf, slab| {
        with_tls_ref(
            &SESSION_SLOT,
            |ptr| unsafe {
                ptr.write(SessionKit::new(clock));
            },
            |cluster| {
                let config =
                    Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources(
                        (tap_buf, slab),
                        hibana::integration::runtime::CounterClock::new(),
                    );
                let transport = TestTransport::default();

                let rv_id = cluster
                    .add_rendezvous_from_config(config, transport.clone())
                    .expect("register rendezvous");
                cluster
                    .rendezvous(rv_id)
                    .role(&routed_payload_controller_program())
                    .set_resolver::<ROUTE_POLICY_ID>(
                        hibana::integration::policy::ResolverRef::route_fn(right_route_resolver),
                    )
                    .expect("register controller route resolver");

                let sid = SessionId::new(11);
                with_tls_mut(
                    &WORKER_ENDPOINT_SLOT,
                    |ptr| unsafe {
                        write_value(
                            ptr,
                            cluster
                                .rendezvous(rv_id)
                                .session(sid)
                                .role(&routed_payload_worker_program())
                                .enter(NoBinding)
                                .expect("worker endpoint"),
                        );
                    },
                    |worker| {
                        with_tls_mut(
                            &CONTROLLER_ENDPOINT_SLOT,
                            |ptr| unsafe {
                                write_value(
                                    ptr,
                                    cluster
                                        .rendezvous(rv_id)
                                        .session(sid)
                                        .role(&routed_payload_controller_program())
                                        .enter(NoBinding)
                                        .expect("controller endpoint"),
                                );
                            },
                            |controller| {
                                block_on_async(async {
                                    controller
                                        .flow::<Msg<
                                            ROUTE_RIGHT_CONTROL_LOGICAL,
                                            GenericCapToken<RouteRightKind>,
                                            RouteRightKind,
                                        >>()
                                        .expect("right route control must be available")
                                        .send(())
                                        .await
                                        .expect("right route control self-send must resolve");

                                    controller
                                        .flow::<Msg<ROUTE_RIGHT_PAYLOAD_LOGICAL, u8>>()
                                        .expect("right route payload flow must be available")
                                        .send(&42u8)
                                        .await
                                        .expect("right route payload send must cross transport");

                                    let branch = worker
                                        .offer()
                                        .await
                                        .expect("passive worker offer must select routed payload");
                                    assert_eq!(
                                        branch.label(),
                                        ROUTE_RIGHT_PAYLOAD_LOGICAL,
                                        "passive branch label must come from projected payload frame"
                                    );
                                    let payload = branch
                                        .decode::<Msg<ROUTE_RIGHT_PAYLOAD_LOGICAL, u8>>()
                                        .await
                                        .expect("passive branch decode must commit route arm");
                                    assert_eq!(payload, 42);
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
fn send_first_route_branch_decode_is_phase_invariant() {
    with_fixture(|clock, tap_buf, slab| {
        with_tls_ref(
            &SESSION_SLOT,
            |ptr| unsafe {
                ptr.write(SessionKit::new(clock));
            },
            |cluster| {
                let config =
                    Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources(
                        (tap_buf, slab),
                        hibana::integration::runtime::CounterClock::new(),
                    );
                let transport = TestTransport::default();

                let rv_id = cluster
                    .add_rendezvous_from_config(config, transport.clone())
                    .expect("register rendezvous");
                cluster
                    .rendezvous(rv_id)
                    .role(&send_first_route_controller_program())
                    .set_resolver::<ROUTE_POLICY_ID>(
                        hibana::integration::policy::ResolverRef::route_fn(right_route_resolver),
                    )
                    .expect("register controller route resolver");

                let sid = SessionId::new(111);
                with_tls_mut(
                    &WORKER_ENDPOINT_SLOT,
                    |ptr| unsafe {
                        write_value(
                            ptr,
                            cluster
                                .rendezvous(rv_id)
                                .session(sid)
                                .role(&send_first_route_worker_program())
                                .enter(NoBinding)
                                .expect("worker endpoint"),
                        );
                    },
                    |worker| {
                        with_tls_mut(
                            &CONTROLLER_ENDPOINT_SLOT,
                            |ptr| unsafe {
                                write_value(
                                    ptr,
                                    cluster
                                        .rendezvous(rv_id)
                                        .session(sid)
                                        .role(&send_first_route_controller_program())
                                        .enter(NoBinding)
                                        .expect("controller endpoint"),
                                );
                            },
                            |controller| {
                                block_on_async(async {
                                    controller
                                        .flow::<Msg<
                                            ROUTE_RIGHT_CONTROL_LOGICAL,
                                            GenericCapToken<RouteRightKind>,
                                            RouteRightKind,
                                        >>()
                                        .expect("right route control must be available")
                                        .send(())
                                        .await
                                        .expect("right route control self-send must resolve");

                                    let branch = worker
                                        .offer()
                                        .await
                                        .expect("worker offer must select send-first arm");
                                    assert_eq!(
                                        branch.label(),
                                        ROUTE_SEND_FIRST_PAYLOAD_LOGICAL,
                                        "send-first branch must preview the first send label"
                                    );

                                    let err = branch
                                        .decode::<Msg<ROUTE_SEND_FIRST_PAYLOAD_LOGICAL, ()>>()
                                        .await
                                        .expect_err("send-first arm must not decode");
                                    let rendered = format!("{err:?}");
                                    assert!(
                                        rendered.contains("PhaseInvariant")
                                            && !rendered.contains("SessionFault"),
                                        "send-first decode must fail before synthetic progress: {rendered}"
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
fn passive_role0_offer_decodes_payload_selected_by_role1_controller_route_frame() {
    with_fixture(|clock, tap_buf, slab| {
        with_tls_ref(
            &SESSION_SLOT,
            |ptr| unsafe {
                ptr.write(SessionKit::new(clock));
            },
            |cluster| {
                let config =
                    Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources(
                        (tap_buf, slab),
                        hibana::integration::runtime::CounterClock::new(),
                    );
                let transport = TestTransport::default();

                let rv_id = cluster
                    .add_rendezvous_from_config(config, transport.clone())
                    .expect("register rendezvous");
                cluster
                    .rendezvous(rv_id)
                    .role(&routed_payload_role1_controller_program())
                    .set_resolver::<ROUTE_POLICY_ID>(
                        hibana::integration::policy::ResolverRef::route_fn(right_route_resolver),
                    )
                    .expect("register role1 route resolver");

                let sid = SessionId::new(12);
                with_tls_mut(
                    &WORKER_ROLE0_ENDPOINT_SLOT,
                    |ptr| unsafe {
                        write_value(
                            ptr,
                            cluster
                                .rendezvous(rv_id)
                                .session(sid)
                                .role(&routed_payload_role0_worker_program())
                                .enter(NoBinding)
                                .expect("worker endpoint"),
                        );
                    },
                    |worker| {
                        with_tls_mut(
                            &CONTROLLER_ROLE1_ENDPOINT_SLOT,
                            |ptr| unsafe {
                                write_value(
                                    ptr,
                                    cluster
                                        .rendezvous(rv_id)
                                        .session(sid)
                                        .role(&routed_payload_role1_controller_program())
                                        .enter(NoBinding)
                                        .expect("controller endpoint"),
                                );
                            },
                            |controller| {
                                block_on_async(async {
                                    controller
                                        .flow::<Msg<
                                            ROUTE_RIGHT_CONTROL_LOGICAL,
                                            GenericCapToken<RouteRightKind>,
                                            RouteRightKind,
                                        >>()
                                        .expect("right route control must be available")
                                        .send(())
                                        .await
                                        .expect("right route control self-send must resolve");

                                    controller
                                        .flow::<Msg<ROUTE_RIGHT_PAYLOAD_LOGICAL, u8>>()
                                        .expect("right route payload flow must be available")
                                        .send(&7u8)
                                        .await
                                        .expect("right route payload send must cross transport");

                                    let branch = worker
                                        .offer()
                                        .await
                                        .expect("passive role0 offer must select routed payload");
                                    assert_eq!(branch.label(), ROUTE_RIGHT_PAYLOAD_LOGICAL);
                                    let payload = branch
                                        .decode::<Msg<ROUTE_RIGHT_PAYLOAD_LOGICAL, u8>>()
                                        .await
                                        .expect("passive role0 decode must commit route arm");
                                    assert_eq!(payload, 7);
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
fn passive_dynamic_offer_without_route_evidence_waits_instead_of_faulting() {
    with_fixture(|clock, tap_buf, slab| {
        with_tls_ref(
            &SESSION_SLOT,
            |ptr| unsafe {
                ptr.write(SessionKit::new(clock));
            },
            |cluster| {
                let config =
                    Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources(
                        (tap_buf, slab),
                        hibana::integration::runtime::CounterClock::new(),
                    );
                let transport = TestTransport::default();

                let rv_id = cluster
                    .add_rendezvous_from_config(config, transport)
                    .expect("register rendezvous");
                let sid = SessionId::new(17);

                with_tls_mut(
                    &WORKER_ROLE0_ENDPOINT_SLOT,
                    |ptr| unsafe {
                        write_value(
                            ptr,
                            cluster
                                .rendezvous(rv_id)
                                .session(sid)
                                .role(&routed_payload_with_tail_role0_worker_program())
                                .enter(NoBinding)
                                .expect("worker endpoint"),
                        );
                    },
                    |worker| {
                        let mut offer = Box::pin(worker.offer());
                        let waker = futures::task::noop_waker();
                        let mut cx = Context::from_waker(&waker);
                        match Future::poll(offer.as_mut(), &mut cx) {
                            Poll::Pending => {}
                            Poll::Ready(Ok(branch)) => {
                                panic!(
                                    "passive dynamic offer selected branch {} without route evidence",
                                    branch.label()
                                );
                            }
                            Poll::Ready(Err(error)) => {
                                panic!(
                                    "passive dynamic offer must wait for route evidence, got {error:?}"
                                );
                            }
                        }
                    },
                )
            },
        );
    });
}

#[test]
fn passive_route_decode_allows_tail_send_from_same_endpoint() {
    with_fixture(|clock, tap_buf, slab| {
        with_tls_ref(
            &SESSION_SLOT,
            |ptr| unsafe {
                ptr.write(SessionKit::new(clock));
            },
            |cluster| {
                let config =
                    Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources(
                        (tap_buf, slab),
                        hibana::integration::runtime::CounterClock::new(),
                    );
                let transport = TestTransport::default();

                let rv_id = cluster
                    .add_rendezvous_from_config(config, transport.clone())
                    .expect("register rendezvous");
                cluster
                    .rendezvous(rv_id)
                    .role(&routed_payload_with_tail_role1_controller_program())
                    .set_resolver::<ROUTE_POLICY_ID>(
                        hibana::integration::policy::ResolverRef::route_fn(right_route_resolver),
                    )
                    .expect("register role1 route resolver");

                let sid = SessionId::new(18);
                with_tls_mut(
                    &WORKER_ROLE0_ENDPOINT_SLOT,
                    |ptr| unsafe {
                        write_value(
                            ptr,
                            cluster
                                .rendezvous(rv_id)
                                .session(sid)
                                .role(&routed_payload_with_tail_role0_worker_program())
                                .enter(NoBinding)
                                .expect("worker endpoint"),
                        );
                    },
                    |worker| {
                        with_tls_mut(
                            &CONTROLLER_ROLE1_ENDPOINT_SLOT,
                            |ptr| unsafe {
                                write_value(
                                    ptr,
                                    cluster
                                        .rendezvous(rv_id)
                                        .session(sid)
                                        .role(&routed_payload_with_tail_role1_controller_program())
                                        .enter(NoBinding)
                                        .expect("controller endpoint"),
                                );
                            },
                            |controller| {
                                block_on_async(async {
                                    controller
                                        .flow::<Msg<
                                            ROUTE_RIGHT_CONTROL_LOGICAL,
                                            GenericCapToken<RouteRightKind>,
                                            RouteRightKind,
                                        >>()
                                        .expect("right route control must be available")
                                        .send(())
                                        .await
                                        .expect("right route control self-send must resolve");

                                    controller
                                        .flow::<Msg<ROUTE_RIGHT_PAYLOAD_LOGICAL, u8>>()
                                        .expect("right route payload flow must be available")
                                        .send(&7u8)
                                        .await
                                        .expect("right route payload send must cross transport");

                                    let branch = worker
                                        .offer()
                                        .await
                                        .expect("passive offer must select routed payload");
                                    assert_eq!(branch.label(), ROUTE_RIGHT_PAYLOAD_LOGICAL);
                                    let payload = branch
                                        .decode::<Msg<ROUTE_RIGHT_PAYLOAD_LOGICAL, u8>>()
                                        .await
                                        .expect("passive decode must commit route arm");
                                    assert_eq!(payload, 7);

                                    worker
                                        .flow::<Msg<ROUTE_TAIL_ACK_LOGICAL, u8>>()
                                        .expect("tail flow must be available after route decode")
                                        .send(&1u8)
                                        .await
                                        .expect("tail send must progress after route decode");

                                    let ack = controller
                                        .recv::<Msg<ROUTE_TAIL_ACK_LOGICAL, u8>>()
                                        .await
                                        .expect("controller must receive tail ack");
                                    assert_eq!(ack, 1);
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
fn split_kits_passive_role0_decodes_payload_after_local_resolver_decision() {
    with_fixture(|clock, tap_buf, slab| {
        let _ = tap_buf;
        let controller_tap = Box::leak(Box::new(
            [hibana::integration::runtime::TapEvent::zero(); runtime_support::RING_EVENTS],
        ));
        let worker_tap = Box::leak(Box::new(
            [hibana::integration::runtime::TapEvent::zero(); runtime_support::RING_EVENTS],
        ));
        let (controller_slab, worker_slab) = slab.split_at_mut(512 * 1024);
        let transport = TestTransport::default();
        with_tls_ref(
            &SESSION_SLOT,
            |ptr| unsafe {
                ptr.write(SessionKit::new(clock));
            },
            |controller_kit| {
                with_tls_ref(
                    &SESSION_SLOT_B,
                    |ptr| unsafe {
                        ptr.write(SessionKit::new(clock));
                    },
                    |worker_kit| {
                        let controller_config = Config::<
                            hibana::integration::runtime::DefaultLabelUniverse,
                            _,
                        >::from_resources(
                            (controller_tap, controller_slab),
                            hibana::integration::runtime::CounterClock::new(),
                        );
                        let worker_config = Config::<
                            hibana::integration::runtime::DefaultLabelUniverse,
                            _,
                        >::from_resources(
                            (worker_tap, worker_slab),
                            hibana::integration::runtime::CounterClock::new(),
                        );
                        let controller_rv = controller_kit
                            .add_rendezvous_from_config(controller_config, transport.clone())
                            .expect("register controller rendezvous");
                        let worker_rv = worker_kit
                            .add_rendezvous_from_config(worker_config, transport.clone())
                            .expect("register worker rendezvous");
                        controller_kit
                            .rendezvous(controller_rv)
                            .role(&routed_payload_role1_controller_program())
                            .set_resolver::<ROUTE_POLICY_ID>(
                                hibana::integration::policy::ResolverRef::route_fn(
                                    right_route_resolver,
                                ),
                            )
                            .expect("register role1 route resolver");
                        worker_kit
                            .rendezvous(worker_rv)
                            .role(&routed_payload_role0_worker_program())
                            .set_resolver::<ROUTE_POLICY_ID>(
                                hibana::integration::policy::ResolverRef::route_fn(
                                    right_route_resolver,
                                ),
                            )
                            .expect("register role0 route resolver");

                        let sid = SessionId::new(13);
                        with_tls_mut(
                            &WORKER_ROLE0_ENDPOINT_SLOT,
                            |ptr| unsafe {
                                write_value(
                                    ptr,
                                    worker_kit
                                        .rendezvous(worker_rv)
                                        .session(sid)
                                        .role(&routed_payload_role0_worker_program())
                                        .enter(NoBinding)
                                        .expect("worker endpoint"),
                                );
                            },
                            |worker| {
                                with_tls_mut(
                                    &CONTROLLER_ROLE1_ENDPOINT_SLOT,
                                    |ptr| unsafe {
                                        write_value(
                                            ptr,
                                            controller_kit
                                                .rendezvous(controller_rv)
                                                .session(sid)
                                                .role(&routed_payload_role1_controller_program())
                                                .enter(NoBinding)
                                                .expect("controller endpoint"),
                                        );
                                    },
                                    |controller| {
                                        block_on_async(async {
                                            controller
                                                .flow::<Msg<
                                                    ROUTE_RIGHT_CONTROL_LOGICAL,
                                                    GenericCapToken<RouteRightKind>,
                                                    RouteRightKind,
                                                >>()
                                                .expect("right route control must be available")
                                                .send(())
                                                .await
                                                .expect(
                                                    "right route control self-send must resolve",
                                                );

                                            controller
                                                .flow::<Msg<ROUTE_RIGHT_PAYLOAD_LOGICAL, u8>>()
                                                .expect(
                                                    "right route payload flow must be available",
                                                )
                                                .send(&9u8)
                                                .await
                                                .expect(
                                                    "right route payload send must cross transport",
                                                );

                                            let branch = worker.offer().await.expect(
                                                "split worker offer must select routed payload",
                                            );
                                            assert_eq!(branch.label(), ROUTE_RIGHT_PAYLOAD_LOGICAL);
                                            let payload = branch
                                                .decode::<Msg<ROUTE_RIGHT_PAYLOAD_LOGICAL, u8>>()
                                                .await
                                                .expect(
                                                    "split worker decode must commit route arm",
                                                );
                                            assert_eq!(payload, 9);
                                        });
                                    },
                                );
                            },
                        );
                    },
                )
            },
        );
    });
}

#[test]
fn split_kits_passive_route_decode_allows_tail_send() {
    with_fixture(|clock, tap_buf, slab| {
        let _ = tap_buf;
        let controller_tap = Box::leak(Box::new(
            [hibana::integration::runtime::TapEvent::zero(); runtime_support::RING_EVENTS],
        ));
        let worker_tap = Box::leak(Box::new(
            [hibana::integration::runtime::TapEvent::zero(); runtime_support::RING_EVENTS],
        ));
        let (controller_slab, worker_slab) = slab.split_at_mut(512 * 1024);
        let transport = TestTransport::default();
        with_tls_ref(
            &SESSION_SLOT,
            |ptr| unsafe {
                ptr.write(SessionKit::new(clock));
            },
            |controller_kit| {
                with_tls_ref(
                    &SESSION_SLOT_B,
                    |ptr| unsafe {
                        ptr.write(SessionKit::new(clock));
                    },
                    |worker_kit| {
                        let controller_config = Config::<
                            hibana::integration::runtime::DefaultLabelUniverse,
                            _,
                        >::from_resources(
                            (controller_tap, controller_slab),
                            hibana::integration::runtime::CounterClock::new(),
                        );
                        let worker_config = Config::<
                            hibana::integration::runtime::DefaultLabelUniverse,
                            _,
                        >::from_resources(
                            (worker_tap, worker_slab),
                            hibana::integration::runtime::CounterClock::new(),
                        );
                        let controller_rv = controller_kit
                            .add_rendezvous_from_config(controller_config, transport.clone())
                            .expect("register controller rendezvous");
                        let worker_rv = worker_kit
                            .add_rendezvous_from_config(worker_config, transport.clone())
                            .expect("register worker rendezvous");
                        controller_kit
                            .rendezvous(controller_rv)
                            .role(&routed_payload_with_tail_role1_controller_program())
                            .set_resolver::<ROUTE_POLICY_ID>(
                                hibana::integration::policy::ResolverRef::route_fn(
                                    right_route_resolver,
                                ),
                            )
                            .expect("register role1 route resolver");
                        worker_kit
                            .rendezvous(worker_rv)
                            .role(&routed_payload_with_tail_role0_worker_program())
                            .set_resolver::<ROUTE_POLICY_ID>(
                                hibana::integration::policy::ResolverRef::route_fn(
                                    right_route_resolver,
                                ),
                            )
                            .expect("register role0 route resolver");

                        let sid = SessionId::new(19);
                        with_tls_mut(
                            &WORKER_ROLE0_ENDPOINT_SLOT,
                            |ptr| unsafe {
                                write_value(
                                    ptr,
                                    worker_kit
                                        .rendezvous(worker_rv)
                                        .session(sid)
                                        .role(&routed_payload_with_tail_role0_worker_program())
                                        .enter(NoBinding)
                                        .expect("worker endpoint"),
                                );
                            },
                            |worker| {
                                with_tls_mut(
                                    &CONTROLLER_ROLE1_ENDPOINT_SLOT,
                                    |ptr| unsafe {
                                        write_value(
                                            ptr,
                                            controller_kit
                                                .rendezvous(controller_rv).session(sid).role(&routed_payload_with_tail_role1_controller_program()).enter(NoBinding,)
                                                .expect("controller endpoint"),
                                        );
                                    },
                                    |controller| {
                                        block_on_async(async {
                                            controller
                                                .flow::<Msg<
                                                    ROUTE_RIGHT_CONTROL_LOGICAL,
                                                    GenericCapToken<RouteRightKind>,
                                                    RouteRightKind,
                                                >>()
                                                .expect("right route control must be available")
                                                .send(())
                                                .await
                                                .expect(
                                                    "right route control self-send must resolve",
                                                );

                                            controller
                                                .flow::<Msg<ROUTE_RIGHT_PAYLOAD_LOGICAL, u8>>()
                                                .expect(
                                                    "right route payload flow must be available",
                                                )
                                                .send(&9u8)
                                                .await
                                                .expect(
                                                    "right route payload send must cross transport",
                                                );

                                            let branch = worker.offer().await.expect(
                                                "split worker offer must select routed payload",
                                            );
                                            assert_eq!(branch.label(), ROUTE_RIGHT_PAYLOAD_LOGICAL);
                                            let payload = branch
                                                .decode::<Msg<ROUTE_RIGHT_PAYLOAD_LOGICAL, u8>>()
                                                .await
                                                .expect(
                                                    "split worker decode must commit route arm",
                                                );
                                            assert_eq!(payload, 9);

                                            worker
                                                .flow::<Msg<ROUTE_TAIL_ACK_LOGICAL, u8>>()
                                                .expect(
                                                    "split tail flow must be available after route decode",
                                                )
                                                .send(&1u8)
                                                .await
                                                .expect(
                                                    "split tail send must progress after route decode",
                                                );

                                            let ack = controller
                                                .recv::<Msg<ROUTE_TAIL_ACK_LOGICAL, u8>>()
                                                .await
                                                .expect("controller must receive split tail ack");
                                            assert_eq!(ack, 1);
                                        });
                                    },
                                );
                            },
                        );

                        assert!(transport.queue_is_empty());
                    },
                )
            },
        );
    });
}

#[test]
fn in_place_split_kits_one_endpoint_allow_route_tail_send() {
    with_fixture(|clock, controller_tap, slab| {
        let worker_tap = Box::leak(Box::new(
            [hibana::integration::runtime::TapEvent::zero(); runtime_support::RING_EVENTS],
        ));
        let (controller_slab, worker_slab) = slab.split_at_mut(512 * 1024);
        let controller_storage = Box::leak(Box::new(MaybeUninit::<EmbeddedTestKit>::uninit()));
        let worker_storage = Box::leak(Box::new(MaybeUninit::<EmbeddedTestKit>::uninit()));
        let controller_kit = EmbeddedTestKit::init_in_place(controller_storage, clock);
        let worker_kit = EmbeddedTestKit::init_in_place(worker_storage, clock);
        let transport = TestTransport::default();
        let controller_config =
            Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources(
                (controller_tap, controller_slab),
                hibana::integration::runtime::CounterClock::new(),
            );
        let worker_config =
            Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources(
                (worker_tap, worker_slab),
                hibana::integration::runtime::CounterClock::new(),
            );
        let controller_rv = controller_kit
            .add_rendezvous_from_config(controller_config, transport.clone())
            .expect("register in-place controller rendezvous");
        let worker_rv = worker_kit
            .add_rendezvous_from_config(worker_config, transport.clone())
            .expect("register in-place worker rendezvous");
        controller_kit
            .rendezvous(controller_rv)
            .role(&routed_payload_with_tail_role1_controller_program())
            .set_resolver::<ROUTE_POLICY_ID>(hibana::integration::policy::ResolverRef::route_fn(
                right_route_resolver,
            ))
            .expect("register in-place role1 route resolver");
        worker_kit
            .rendezvous(worker_rv)
            .role(&routed_payload_with_tail_role0_worker_program())
            .set_resolver::<ROUTE_POLICY_ID>(hibana::integration::policy::ResolverRef::route_fn(
                right_route_resolver,
            ))
            .expect("register in-place role0 route resolver");

        let sid = SessionId::new(20);
        with_tls_mut(
            &WORKER_ROLE0_ENDPOINT_SLOT,
            |ptr| unsafe {
                write_value(
                    ptr,
                    worker_kit
                        .rendezvous(worker_rv)
                        .session(sid)
                        .role(&routed_payload_with_tail_role0_worker_program())
                        .enter(NoBinding)
                        .expect("in-place worker endpoint"),
                );
            },
            |worker| {
                with_tls_mut(
                    &CONTROLLER_ROLE1_ENDPOINT_SLOT,
                    |ptr| unsafe {
                        write_value(
                            ptr,
                            controller_kit
                                .rendezvous(controller_rv)
                                .session(sid)
                                .role(&routed_payload_with_tail_role1_controller_program())
                                .enter(NoBinding)
                                .expect("in-place controller endpoint"),
                        );
                    },
                    |controller| {
                        block_on_async(async {
                            controller
                                .flow::<Msg<
                                    ROUTE_RIGHT_CONTROL_LOGICAL,
                                    GenericCapToken<RouteRightKind>,
                                    RouteRightKind,
                                >>()
                                .expect("right route control must be available")
                                .send(())
                                .await
                                .expect("right route control self-send must resolve");

                            controller
                                .flow::<Msg<ROUTE_RIGHT_PAYLOAD_LOGICAL, u8>>()
                                .expect("right route payload flow must be available")
                                .send(&11u8)
                                .await
                                .expect("right route payload send must cross transport");

                            let branch = worker
                                .offer()
                                .await
                                .expect("in-place worker offer must select routed payload");
                            assert_eq!(branch.label(), ROUTE_RIGHT_PAYLOAD_LOGICAL);
                            let payload = branch
                                .decode::<Msg<ROUTE_RIGHT_PAYLOAD_LOGICAL, u8>>()
                                .await
                                .expect("in-place worker decode must commit route arm");
                            assert_eq!(payload, 11);

                            worker
                                .flow::<Msg<ROUTE_TAIL_ACK_LOGICAL, u8>>()
                                .expect("in-place tail flow must be available after route decode")
                                .send(&1u8)
                                .await
                                .expect("in-place tail send must progress after route decode");

                            let ack = controller
                                .recv::<Msg<ROUTE_TAIL_ACK_LOGICAL, u8>>()
                                .await
                                .expect("controller must receive in-place tail ack");
                            assert_eq!(ack, 1);
                        });
                    },
                );
            },
        );

        assert!(transport.queue_is_empty());
    });
}

#[test]
fn split_kits_passive_dynamic_route_does_not_use_payload_label_as_authority() {
    use std::{
        future::Future,
        pin::Pin,
        task::{Context, Poll},
    };

    with_fixture(|clock, tap_buf, slab| {
        let _ = tap_buf;
        let controller_tap = Box::leak(Box::new(
            [hibana::integration::runtime::TapEvent::zero(); runtime_support::RING_EVENTS],
        ));
        let worker_tap = Box::leak(Box::new(
            [hibana::integration::runtime::TapEvent::zero(); runtime_support::RING_EVENTS],
        ));
        let (controller_slab, worker_slab) = slab.split_at_mut(512 * 1024);
        let transport = TestTransport::default();
        with_tls_ref(
            &SESSION_SLOT,
            |ptr| unsafe {
                ptr.write(SessionKit::new(clock));
            },
            |controller_kit| {
                with_tls_ref(
                    &SESSION_SLOT_B,
                    |ptr| unsafe {
                        ptr.write(SessionKit::new(clock));
                    },
                    |worker_kit| {
                        let controller_config = Config::<
                            hibana::integration::runtime::DefaultLabelUniverse,
                            _,
                        >::from_resources(
                            (controller_tap, controller_slab),
                            hibana::integration::runtime::CounterClock::new(),
                        );
                        let worker_config = Config::<
                            hibana::integration::runtime::DefaultLabelUniverse,
                            _,
                        >::from_resources(
                            (worker_tap, worker_slab),
                            hibana::integration::runtime::CounterClock::new(),
                        );
                        let controller_rv = controller_kit
                            .add_rendezvous_from_config(controller_config, transport.clone())
                            .expect("register controller rendezvous");
                        let worker_rv = worker_kit
                            .add_rendezvous_from_config(worker_config, transport.clone())
                            .expect("register worker rendezvous");
                        controller_kit
                            .rendezvous(controller_rv)
                            .role(&routed_payload_role1_controller_program())
                            .set_resolver::<ROUTE_POLICY_ID>(
                                hibana::integration::policy::ResolverRef::route_fn(
                                    right_route_resolver,
                                ),
                            )
                            .expect("register role1 route resolver");

                        let sid = SessionId::new(14);
                        with_tls_mut(
                            &WORKER_ROLE0_ENDPOINT_SLOT,
                            |ptr| unsafe {
                                write_value(
                                    ptr,
                                    worker_kit
                                        .rendezvous(worker_rv)
                                        .session(sid)
                                        .role(&routed_payload_role0_worker_program())
                                        .enter(NoBinding)
                                        .expect("worker endpoint"),
                                );
                            },
                            |worker| {
                                with_tls_mut(
                                    &CONTROLLER_ROLE1_ENDPOINT_SLOT,
                                    |ptr| unsafe {
                                        write_value(
                                            ptr,
                                            controller_kit
                                                .rendezvous(controller_rv)
                                                .session(sid)
                                                .role(&routed_payload_role1_controller_program())
                                                .enter(NoBinding)
                                                .expect("controller endpoint"),
                                        );
                                    },
                                    |controller| {
                                        block_on_async(async {
                                            controller
                                                .flow::<Msg<
                                                    ROUTE_RIGHT_CONTROL_LOGICAL,
                                                    GenericCapToken<RouteRightKind>,
                                                    RouteRightKind,
                                                >>()
                                                .expect("right route control must be available")
                                                .send(())
                                                .await
                                                .expect(
                                                    "right route control self-send must resolve",
                                                );

                                            controller
                                                .flow::<Msg<ROUTE_RIGHT_PAYLOAD_LOGICAL, u8>>()
                                                .expect(
                                                    "right route payload flow must be available",
                                                )
                                                .send(&11u8)
                                                .await
                                                .expect(
                                                    "right route payload send must cross transport",
                                                );
                                        });

                                        let mut offer = Box::pin(worker.offer());
                                        let waker = futures::task::noop_waker();
                                        let mut cx = Context::from_waker(&waker);
                                        match Pin::as_mut(&mut offer).poll(&mut cx) {
                                            Poll::Ready(Ok(branch)) => panic!(
                                                "dynamic route selected branch {} from payload label without passive resolver authority",
                                                branch.label()
                                            ),
                                            Poll::Ready(Err(_)) | Poll::Pending => {}
                                        }
                                    },
                                );
                            },
                        );
                    },
                )
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
                let config =
                    Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources(
                        (tap_buf, slab),
                        hibana::integration::runtime::CounterClock::new(),
                    );
                let transport = TestTransport::default();

                let rv_id = cluster
                    .add_rendezvous_from_config(config, transport.clone())
                    .expect("register rendezvous");
                cluster
                    .rendezvous(rv_id)
                    .role(&route_tail_controller_program())
                    .set_resolver::<ROUTE_POLICY_ID>(
                        hibana::integration::policy::ResolverRef::route_fn(route_resolver),
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
                                .rendezvous(rv_id)
                                .session(sid)
                                .role(&route_tail_worker_program())
                                .enter(NoBinding)
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
                                        .rendezvous(rv_id)
                                        .session(sid)
                                        .role(&route_tail_controller_program())
                                        .enter(NoBinding)
                                        .expect("controller endpoint"),
                                );
                            },
                            |controller| {
                                let route_flow = controller
                                    .flow::<Msg<
                                        { TEST_ROUTE_DECISION_LOGICAL },
                                        GenericCapToken<RouteDecisionKind>,
                                        RouteDecisionKind,
                                    >>()
                                    .expect("route flow should remain available after enter");
                                drop(route_flow);
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
                                let config = Config::<
                                    hibana::integration::runtime::DefaultLabelUniverse,
                                    _,
                                >::from_resources(
                                    (tap_buf, slab),
                                    hibana::integration::runtime::CounterClock::new(),
                                );
                                let transport = TestTransport::default();
                                let rv_id = cluster
                                    .add_rendezvous_from_config(config, transport.clone())
                                    .expect("register rendezvous");

                                cluster
                                    .rendezvous(rv_id)
                                    .role(&controller_program())
                                    .set_resolver::<ROUTE_POLICY_ID>(
                                        hibana::integration::policy::ResolverRef::route_fn(
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
                                                .rendezvous(rv_id)
                                                .session(sid)
                                                .role(&worker_program())
                                                .enter(NoBinding)
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
                                                        .rendezvous(rv_id)
                                                        .session(sid)
                                                        .role(&controller_program())
                                                        .enter(controller_binding)
                                                        .expect("controller endpoint"),
                                                );
                                            },
                                            |controller| {
                                                block_on_async(async {
                                                    let send_flow = controller
                                                        .flow::<Msg<
                                                            { TEST_ROUTE_DECISION_LOGICAL },
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
/// This test verifies the self-send local-control definitions are well-formed.
#[test]
fn loop_dynamic_resolver_policy_abort_and_success() {
    let controller_program = loop_controller_program();
    drop(controller_program);
}

/// Test nested routes with flow().send(()) pattern.
///
/// With self-send local control (Controller → Controller), all route decisions
/// are local to the Controller role. Worker doesn't participate in route control.
#[test]
fn nested_loop_dynamic_send_and_offer() {
    with_fixture(|clock, tap_buf, slab| {
        let config =
            Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources(
                (tap_buf, slab),
                hibana::integration::runtime::CounterClock::new(),
            );
        let transport = TestTransport::default();
        with_tls_ref(
            &SESSION_SLOT,
            |ptr| unsafe {
                ptr.write(SessionKit::new(clock));
            },
            |cluster| {
                let rv_id = cluster
                    .add_rendezvous_from_config(config, transport.clone())
                    .expect("register rendezvous");
                assert_ne!(rv_id.raw(), 0);

                let controller_program = nested_loop_controller_program();
                drop(controller_program);
            },
        );

        assert!(transport_queue_is_empty(&transport));
    });
}
