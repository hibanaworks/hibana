#![cfg(feature = "std")]
mod common;
#[path = "support/placement.rs"]
mod placement_support;
#[path = "support/runtime.rs"]
mod runtime_support;
#[path = "support/tls_mut.rs"]
mod tls_mut_support;
#[path = "support/tls_ref.rs"]
mod tls_ref_support;
use ::core::{cell::UnsafeCell, mem::MaybeUninit};
use common::{TestRx, TestTransport, TestTransportError, TestTx};
use hibana::{
    g::{self, Msg},
    integration::program::{RoleProgram, project},
    integration::{
        SessionKitStorage,
        binding::{BindingError, Channel, EndpointSlot, IngressEvidence},
        ids::SessionId,
        runtime::{Config, DefaultLabelUniverse},
    },
    integration::{
        cap::control::{LoopBreakKind, LoopContinueKind, RouteDecisionKind},
        policy::{DecisionArm, DecisionResolution, ResolverError},
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

#[derive(Clone)]
struct PayloadOnlyTransport(TestTransport);

impl Default for PayloadOnlyTransport {
    fn default() -> Self {
        Self(TestTransport::default())
    }
}

impl PayloadOnlyTransport {
    fn queue_is_empty(&self) -> bool {
        self.0.queue_is_empty()
    }
}

impl hibana::integration::transport::Transport for PayloadOnlyTransport {
    type Error = TestTransportError;
    type Tx<'a>
        = TestTx
    where
        Self: 'a;
    type Rx<'a>
        = TestRx<'a>
    where
        Self: 'a;

    fn open<'a>(
        &'a self,
        port: hibana::integration::transport::PortOpen,
    ) -> (Self::Tx<'a>, Self::Rx<'a>) {
        self.0.open(port)
    }

    fn poll_send<'a, 'f>(
        &self,
        tx: &'a mut Self::Tx<'a>,
        outgoing: hibana::integration::transport::Outgoing<'f>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>>
    where
        'a: 'f,
    {
        self.0.poll_send(tx, outgoing, cx)
    }

    fn poll_recv<'a>(
        &'a self,
        rx: &'a mut Self::Rx<'a>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<hibana::integration::transport::Incoming<'a>, Self::Error>> {
        self.0.poll_recv(rx, cx)
    }

    fn cancel_send<'a>(&self, tx: &'a mut Self::Tx<'a>) {
        self.0.cancel_send(tx);
    }

    fn requeue<'a>(&self, rx: &mut Self::Rx<'a>) -> Result<(), Self::Error> {
        self.0.requeue(rx)
    }
}

fn block_on_async<F>(future: F) -> F::Output
where
    F: std::future::Future,
{
    futures::executor::block_on(future)
}

type TestKitStorage = SessionKitStorage<
    'static,
    TestTransport,
    DefaultLabelUniverse,
    hibana::integration::runtime::CounterClock,
    2,
>;

std::thread_local! {
    static SESSION_SLOT: UnsafeCell<TestKitStorage> = const {
        UnsafeCell::new(SessionKitStorage::uninit())
    };
    static SESSION_SLOT_B: UnsafeCell<TestKitStorage> = const {
        UnsafeCell::new(SessionKitStorage::uninit())
    };
    static POLICY_INPUT_SLOT: UnsafeCell<MaybeUninit<Cell<u32>>> = const {
        UnsafeCell::new(MaybeUninit::uninit())
    };
    static EMPTY_BINDING_SLOT: UnsafeCell<MaybeUninit<EmptyEndpointBinding>> = const {
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
struct EmptyEndpointBinding;

impl EmptyEndpointBinding {
    fn new() -> Self {
        Self
    }
}

impl EndpointSlot for EmptyEndpointBinding {
    fn poll_incoming_for_lane(&mut self, _logical_lane: u8) -> Option<IngressEvidence> {
        None
    }

    fn on_recv<'a>(
        &'a mut self,
        _channel: Channel,
        _buf: &'a mut [u8],
    ) -> Result<hibana::integration::wire::Payload<'a>, BindingError> {
        Ok(hibana::integration::wire::Payload::new(&[]))
    }
}

fn transport_queue_is_empty(transport: &TestTransport) -> bool {
    transport.queue_is_empty()
}

#[test]
fn test_transport_demuxes_lane_and_returns_frame_header_with_payload() {
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

    let waker = futures::task::noop_waker();
    let mut cx = Context::from_waker(&waker);
    {
        let incoming = match hibana::integration::transport::Transport::poll_recv(
            &transport, &mut rx0, &mut cx,
        ) {
            Poll::Ready(Ok(incoming)) => incoming,
            Poll::Ready(Err(_)) => panic!("lane 0 payload returned transport error"),
            Poll::Pending => panic!("lane 0 payload must be ready after hint drain"),
        };
        let header = incoming.header().expect("lane 0 frame header");
        assert_eq!(header.label().raw(), 10);
        assert_eq!(header.lane().as_wire(), 0);
        assert_eq!(incoming.payload().as_bytes(), b"lane-zero");
    }

    {
        let incoming = match hibana::integration::transport::Transport::poll_recv(
            &transport, &mut rx1, &mut cx,
        ) {
            Poll::Ready(Ok(incoming)) => incoming,
            Poll::Ready(Err(_)) => panic!("lane 1 payload returned transport error"),
            Poll::Pending => panic!("lane 1 payload must remain available independently"),
        };
        let header = incoming.header().expect("lane 1 frame header");
        assert_eq!(header.label().raw(), 20);
        assert_eq!(header.lane().as_wire(), 1);
        assert_eq!(incoming.payload().as_bytes(), b"lane-one");
    }
}

fn controller_program() -> RoleProgram<0> {
    let left_arm =
        g::send::<0, 0, Msg<{ TEST_ROUTE_DECISION_LOGICAL }, (), RouteDecisionKind>, 0>()
            .policy::<ROUTE_POLICY_ID>();
    let right_arm = g::send::<0, 0, Msg<ROUTE_RIGHT_CONTROL_LOGICAL, (), RouteDecisionKind>, 0>()
        .policy::<ROUTE_POLICY_ID>();
    let program = g::route(left_arm, right_arm);
    project(&program)
}

fn worker_program() -> RoleProgram<1> {
    let left_arm =
        g::send::<0, 0, Msg<{ TEST_ROUTE_DECISION_LOGICAL }, (), RouteDecisionKind>, 0>()
            .policy::<ROUTE_POLICY_ID>();
    let right_arm = g::send::<0, 0, Msg<ROUTE_RIGHT_CONTROL_LOGICAL, (), RouteDecisionKind>, 0>()
        .policy::<ROUTE_POLICY_ID>();
    let program = g::route(left_arm, right_arm);
    project(&program)
}

fn duplicate_policy_site_controller_program() -> RoleProgram<0> {
    let first = g::route(
        g::send::<0, 0, Msg<201, (), RouteDecisionKind>, 0>().policy::<ROUTE_POLICY_ID>(),
        g::send::<0, 0, Msg<202, (), RouteDecisionKind>, 0>().policy::<ROUTE_POLICY_ID>(),
    );
    let second = g::route(
        g::send::<0, 0, Msg<203, (), RouteDecisionKind>, 0>().policy::<ROUTE_POLICY_ID>(),
        g::send::<0, 0, Msg<204, (), RouteDecisionKind>, 0>().policy::<ROUTE_POLICY_ID>(),
    );
    project(&g::seq(first, second))
}

#[test]
fn resolver_policy_id_identifies_one_decision_site() {
    with_fixture(|_clock, tap_buf, slab| {
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let config =
                Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources(
                    (tap_buf, slab),
                    hibana::integration::runtime::CounterClock::new(),
                );
            let rv = cluster
                .rendezvous(config, TestTransport::default())
                .expect("register rendezvous");
            let result = rv
                .role(&duplicate_policy_site_controller_program())
                .set_resolver::<ROUTE_POLICY_ID>(
                    hibana::integration::policy::ResolverRef::decision_fn(route_resolver),
                );
            assert!(
                result.is_err(),
                "a resolver without site context must not bind one policy id to multiple decision scopes"
            );
        });
    });
}

#[test]
fn projected_role_attach_order_does_not_fix_lane_storage_capacity() {
    with_fixture(|_clock, tap_buf, slab| {
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let config =
                Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources(
                    (tap_buf, slab),
                    hibana::integration::runtime::CounterClock::new(),
                );
            let transport = TestTransport::default();
            let rv = cluster
                .rendezvous(config, transport)
                .expect("register rendezvous");
            rv.role(&controller_program())
                .set_resolver::<ROUTE_POLICY_ID>(
                    hibana::integration::policy::ResolverRef::decision_fn(route_resolver),
                )
                .expect("register decision resolver");

            let sid = SessionId::new(107);
            with_tls_mut(
                &WORKER_ENDPOINT_SLOT,
                |ptr| unsafe {
                    write_value(
                        ptr,
                        rv.session(sid)
                            .role(&worker_program())
                            .enter()
                            .expect("worker endpoint"),
                    );
                },
                |_worker_endpoint| {
                    with_tls_mut(
                        &CONTROLLER_ENDPOINT_SLOT,
                        |ptr| unsafe {
                            write_value(
                                ptr,
                                rv.session(sid)
                                    .role(&controller_program())
                                    .enter()
                                    .expect("controller endpoint after worker"),
                            );
                        },
                        |_controller_endpoint| {},
                    );
                },
            );
        });
    });
}

fn loop_controller_program() -> RoleProgram<0> {
    let loop_continue_arm =
        g::send::<0, 0, Msg<{ TEST_LOOP_CONTINUE_LOGICAL }, (), LoopContinueKind>, 0>()
            .policy::<LOOP_POLICY_ID>();
    let loop_break_arm = g::send::<0, 0, Msg<{ TEST_LOOP_BREAK_LOGICAL }, (), LoopBreakKind>, 0>()
        .policy::<LOOP_POLICY_ID>();
    let loop_program = g::route(loop_continue_arm, loop_break_arm);
    project(&loop_program)
}

fn route_tail_controller_program() -> RoleProgram<0> {
    let left_arm =
        g::send::<0, 0, Msg<{ TEST_ROUTE_DECISION_LOGICAL }, (), RouteDecisionKind>, 0>()
            .policy::<ROUTE_POLICY_ID>();
    let right_arm = g::send::<0, 0, Msg<ROUTE_RIGHT_CONTROL_LOGICAL, (), RouteDecisionKind>, 0>()
        .policy::<ROUTE_POLICY_ID>();
    let loop_continue_arm =
        g::send::<0, 0, Msg<{ TEST_LOOP_CONTINUE_LOGICAL }, (), LoopContinueKind>, 0>();
    let loop_break_arm = g::send::<0, 0, Msg<{ TEST_LOOP_BREAK_LOGICAL }, (), LoopBreakKind>, 0>();
    let program = g::route(
        g::seq(left_arm, loop_continue_arm),
        g::seq(right_arm, loop_break_arm),
    );
    project(&program)
}

fn route_tail_worker_program() -> RoleProgram<1> {
    let left_arm =
        g::send::<0, 0, Msg<{ TEST_ROUTE_DECISION_LOGICAL }, (), RouteDecisionKind>, 0>()
            .policy::<ROUTE_POLICY_ID>();
    let right_arm = g::send::<0, 0, Msg<ROUTE_RIGHT_CONTROL_LOGICAL, (), RouteDecisionKind>, 0>()
        .policy::<ROUTE_POLICY_ID>();
    let loop_continue_arm =
        g::send::<0, 0, Msg<{ TEST_LOOP_CONTINUE_LOGICAL }, (), LoopContinueKind>, 0>();
    let loop_break_arm = g::send::<0, 0, Msg<{ TEST_LOOP_BREAK_LOGICAL }, (), LoopBreakKind>, 0>();
    let program = g::route(
        g::seq(left_arm, loop_continue_arm),
        g::seq(right_arm, loop_break_arm),
    );
    project(&program)
}

fn nested_loop_controller_program() -> RoleProgram<0> {
    let left_arm =
        g::send::<0, 0, Msg<{ TEST_ROUTE_DECISION_LOGICAL }, (), RouteDecisionKind>, 0>()
            .policy::<ROUTE_POLICY_ID>();
    let right_arm = g::send::<0, 0, Msg<ROUTE_RIGHT_CONTROL_LOGICAL, (), RouteDecisionKind>, 0>()
        .policy::<ROUTE_POLICY_ID>();
    let loop_continue_arm =
        g::send::<0, 0, Msg<{ TEST_LOOP_CONTINUE_LOGICAL }, (), LoopContinueKind>, 0>()
            .policy::<LOOP_POLICY_ID>();
    let loop_break_arm = g::send::<0, 0, Msg<{ TEST_LOOP_BREAK_LOGICAL }, (), LoopBreakKind>, 0>()
        .policy::<LOOP_POLICY_ID>();
    let loop_program = g::route(loop_continue_arm, loop_break_arm);
    let outer_loop_continue_arm = loop_program;
    let nested_loop_break_arm =
        g::send::<0, 0, Msg<{ TEST_LOOP_BREAK_LOGICAL }, (), LoopBreakKind>, 0>();
    let program = g::route(
        g::seq(left_arm, outer_loop_continue_arm),
        g::seq(right_arm, nested_loop_break_arm),
    );
    project(&program)
}

fn route_resolver() -> Result<DecisionResolution, ResolverError> {
    if route_allow() {
        Ok(DecisionResolution::Arm(DecisionArm::Left))
    } else {
        Err(ResolverError::reject())
    }
}

fn decision_policy_input_resolver(
    policy_input: &Cell<u32>,
) -> Result<DecisionResolution, ResolverError> {
    let arm = if policy_input.get() & 1 == 0 {
        DecisionArm::Left
    } else {
        DecisionArm::Right
    };
    Ok(DecisionResolution::Arm(arm))
}

fn right_route_resolver() -> Result<DecisionResolution, ResolverError> {
    Ok(DecisionResolution::Arm(DecisionArm::Right))
}

fn routed_payload_controller_program() -> RoleProgram<0> {
    let left_arm = g::seq(
        g::send::<0, 0, Msg<{ TEST_ROUTE_DECISION_LOGICAL }, (), RouteDecisionKind>, 0>()
            .policy::<ROUTE_POLICY_ID>(),
        g::send::<0, 1, Msg<ROUTE_LEFT_PAYLOAD_LOGICAL, u8>, 0>(),
    );
    let right_arm = g::seq(
        g::send::<0, 0, Msg<ROUTE_RIGHT_CONTROL_LOGICAL, (), RouteDecisionKind>, 0>()
            .policy::<ROUTE_POLICY_ID>(),
        g::send::<0, 1, Msg<ROUTE_RIGHT_PAYLOAD_LOGICAL, u8>, 0>(),
    );
    let program = g::route(left_arm, right_arm);
    project(&program)
}

fn routed_payload_worker_program() -> RoleProgram<1> {
    let left_arm = g::seq(
        g::send::<0, 0, Msg<{ TEST_ROUTE_DECISION_LOGICAL }, (), RouteDecisionKind>, 0>()
            .policy::<ROUTE_POLICY_ID>(),
        g::send::<0, 1, Msg<ROUTE_LEFT_PAYLOAD_LOGICAL, u8>, 0>(),
    );
    let right_arm = g::seq(
        g::send::<0, 0, Msg<ROUTE_RIGHT_CONTROL_LOGICAL, (), RouteDecisionKind>, 0>()
            .policy::<ROUTE_POLICY_ID>(),
        g::send::<0, 1, Msg<ROUTE_RIGHT_PAYLOAD_LOGICAL, u8>, 0>(),
    );
    let program = g::route(left_arm, right_arm);
    project(&program)
}

fn send_first_route_controller_program() -> RoleProgram<0> {
    let left_arm = g::seq(
        g::send::<0, 0, Msg<{ TEST_ROUTE_DECISION_LOGICAL }, (), RouteDecisionKind>, 0>()
            .policy::<ROUTE_POLICY_ID>(),
        g::send::<0, 1, Msg<ROUTE_LEFT_PAYLOAD_LOGICAL, u8>, 0>(),
    );
    let right_arm = g::seq(
        g::send::<0, 0, Msg<ROUTE_RIGHT_CONTROL_LOGICAL, (), RouteDecisionKind>, 0>()
            .policy::<ROUTE_POLICY_ID>(),
        g::send::<1, 0, Msg<ROUTE_SEND_FIRST_PAYLOAD_LOGICAL, u8>, 0>(),
    );
    project(&g::route(left_arm, right_arm))
}

fn send_first_route_worker_program() -> RoleProgram<1> {
    let left_arm = g::seq(
        g::send::<0, 0, Msg<{ TEST_ROUTE_DECISION_LOGICAL }, (), RouteDecisionKind>, 0>()
            .policy::<ROUTE_POLICY_ID>(),
        g::send::<0, 1, Msg<ROUTE_LEFT_PAYLOAD_LOGICAL, u8>, 0>(),
    );
    let right_arm = g::seq(
        g::send::<0, 0, Msg<ROUTE_RIGHT_CONTROL_LOGICAL, (), RouteDecisionKind>, 0>()
            .policy::<ROUTE_POLICY_ID>(),
        g::send::<1, 0, Msg<ROUTE_SEND_FIRST_PAYLOAD_LOGICAL, u8>, 0>(),
    );
    project(&g::route(left_arm, right_arm))
}

fn routed_payload_role1_controller_program() -> RoleProgram<1> {
    let left_arm = g::seq(
        g::send::<1, 1, Msg<{ TEST_ROUTE_DECISION_LOGICAL }, (), RouteDecisionKind>, 0>()
            .policy::<ROUTE_POLICY_ID>(),
        g::send::<1, 0, Msg<ROUTE_LEFT_PAYLOAD_LOGICAL, u8>, 0>(),
    );
    let right_arm = g::seq(
        g::send::<1, 1, Msg<ROUTE_RIGHT_CONTROL_LOGICAL, (), RouteDecisionKind>, 0>()
            .policy::<ROUTE_POLICY_ID>(),
        g::send::<1, 0, Msg<ROUTE_RIGHT_PAYLOAD_LOGICAL, u8>, 0>(),
    );
    let program = g::route(left_arm, right_arm);
    project(&program)
}

fn routed_payload_role0_worker_program() -> RoleProgram<0> {
    let left_arm = g::seq(
        g::send::<1, 1, Msg<{ TEST_ROUTE_DECISION_LOGICAL }, (), RouteDecisionKind>, 0>()
            .policy::<ROUTE_POLICY_ID>(),
        g::send::<1, 0, Msg<ROUTE_LEFT_PAYLOAD_LOGICAL, u8>, 0>(),
    );
    let right_arm = g::seq(
        g::send::<1, 1, Msg<ROUTE_RIGHT_CONTROL_LOGICAL, (), RouteDecisionKind>, 0>()
            .policy::<ROUTE_POLICY_ID>(),
        g::send::<1, 0, Msg<ROUTE_RIGHT_PAYLOAD_LOGICAL, u8>, 0>(),
    );
    let program = g::route(left_arm, right_arm);
    project(&program)
}

fn routed_payload_with_tail_role1_controller_program() -> RoleProgram<1> {
    let left_arm = g::seq(
        g::send::<1, 1, Msg<{ TEST_ROUTE_DECISION_LOGICAL }, (), RouteDecisionKind>, 0>()
            .policy::<ROUTE_POLICY_ID>(),
        g::send::<1, 0, Msg<ROUTE_LEFT_PAYLOAD_LOGICAL, u8>, 0>(),
    );
    let right_arm = g::seq(
        g::send::<1, 1, Msg<ROUTE_RIGHT_CONTROL_LOGICAL, (), RouteDecisionKind>, 0>()
            .policy::<ROUTE_POLICY_ID>(),
        g::send::<1, 0, Msg<ROUTE_RIGHT_PAYLOAD_LOGICAL, u8>, 0>(),
    );
    project(&g::seq(
        g::route(left_arm, right_arm),
        g::send::<0, 1, Msg<ROUTE_TAIL_ACK_LOGICAL, u8>, 1>(),
    ))
}

fn routed_payload_with_tail_role0_worker_program() -> RoleProgram<0> {
    let left_arm = g::seq(
        g::send::<1, 1, Msg<{ TEST_ROUTE_DECISION_LOGICAL }, (), RouteDecisionKind>, 0>()
            .policy::<ROUTE_POLICY_ID>(),
        g::send::<1, 0, Msg<ROUTE_LEFT_PAYLOAD_LOGICAL, u8>, 0>(),
    );
    let right_arm = g::seq(
        g::send::<1, 1, Msg<ROUTE_RIGHT_CONTROL_LOGICAL, (), RouteDecisionKind>, 0>()
            .policy::<ROUTE_POLICY_ID>(),
        g::send::<1, 0, Msg<ROUTE_RIGHT_PAYLOAD_LOGICAL, u8>, 0>(),
    );
    project(&g::seq(
        g::route(left_arm, right_arm),
        g::send::<0, 1, Msg<ROUTE_TAIL_ACK_LOGICAL, u8>, 1>(),
    ))
}

/// Test route dynamic resolver with flow().send(&()) pattern.
///
/// local control uses self-send (Controller → Controller) and advances
/// via flow().send(&()) which skips wire transmission for self-send.
#[path = "route_dynamic_control/dynamic_offer.rs"]
mod dynamic_offer;
#[path = "route_dynamic_control/split_policy.rs"]
mod split_policy;
#[path = "route_dynamic_control/tail_decode.rs"]
mod tail_decode;
