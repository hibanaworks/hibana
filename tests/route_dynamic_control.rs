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
use tls_ref_support::with_resident_tls_ref;

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
    with_fixture(|_clock, tap_buf, slab| {
        with_resident_tls_ref(
            &SESSION_SLOT,
            |storage| unsafe { SessionKit::init_in_place(storage) },
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
#[path = "route_dynamic_control/dynamic_offer.rs"]
mod dynamic_offer;
#[path = "route_dynamic_control/split_policy.rs"]
mod split_policy;
#[path = "route_dynamic_control/tail_decode.rs"]
mod tail_decode;
