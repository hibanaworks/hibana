#![cfg(feature = "std")]

//! Test that loop control (self-send) operates via flow().send() pattern.
//!
//! Per AGENTS.md Branch Patterns:
//! - Pattern A (Explicit Decision): Controller uses flow().send() for loop Continue/Break
//! - The self-send records the decision in RouteTable
//! - Target (passive observer) uses offer() to observe the arm via cross-role messages

mod common;
#[path = "support/placement.rs"]
mod placement_support;
#[path = "support/runtime.rs"]
mod runtime_support;
#[path = "support/tls_mut.rs"]
mod tls_mut_support;
#[path = "support/tls_ref.rs"]
mod tls_ref_support;

use core::{cell::UnsafeCell, mem::MaybeUninit};

use common::TestTransport;
use hibana::{
    g::{self, Msg, Role},
    substrate::program::{RoleProgram, project},
    substrate::{
        SessionKit,
        binding::NoBinding,
        ids::SessionId,
        runtime::{Config, CounterClock, DefaultLabelUniverse},
        tap::TapEvent,
    },
    substrate::{
        cap::{
            GenericCapToken,
            advanced::{LoopBreakKind, LoopContinueKind},
        },
        ids::RendezvousId,
        policy::{LoopResolution, ResolverContext, ResolverError},
    },
};
use placement_support::write_value;
use runtime_support::with_fixture;
use tls_mut_support::with_tls_mut;
use tls_ref_support::with_tls_ref;

const TEST_LOOP_CONTINUE_LOGICAL: u8 = 0xA1;
const TEST_LOOP_BREAK_LOGICAL: u8 = 0xA2;
const LOOP_POLICY_ID: u16 = 99;
type TestKit = SessionKit<'static, TestTransport, DefaultLabelUniverse, CounterClock, 2>;

std::thread_local! {
    static LOOP_DECISION_INDEX: core::cell::Cell<usize> = const { core::cell::Cell::new(0) };
    static SESSION_SLOT: UnsafeCell<MaybeUninit<TestKit>> = const {
        UnsafeCell::new(MaybeUninit::uninit())
    };
    static CONTROLLER_SLOT: UnsafeCell<MaybeUninit<hibana::Endpoint<'static, 0>>> = const {
        UnsafeCell::new(MaybeUninit::uninit())
    };
    static TARGET_SLOT: UnsafeCell<MaybeUninit<hibana::Endpoint<'static, 1>>> = const {
        UnsafeCell::new(MaybeUninit::uninit())
    };
}

fn loop_decision_index() -> usize {
    LOOP_DECISION_INDEX.with(core::cell::Cell::get)
}

fn set_loop_decision_index(value: usize) {
    LOOP_DECISION_INDEX.with(|cell| cell.set(value));
}

fn loop_lane_resolver(_ctx: ResolverContext) -> Result<LoopResolution, ResolverError> {
    let decision = loop_decision_index() == 0;
    set_loop_decision_index(loop_decision_index() + 1);
    Ok(if decision {
        LoopResolution::Continue
    } else {
        LoopResolution::Break
    })
}

fn register_loop_lane_resolvers<const MAX_RV: usize>(
    cluster: &SessionKit<'_, TestTransport, DefaultLabelUniverse, CounterClock, MAX_RV>,
    rv_id: RendezvousId,
) {
    let controller_program = controller_program();
    cluster
        .set_resolver::<LOOP_POLICY_ID, 0>(
            rv_id,
            &controller_program,
            hibana::substrate::policy::ResolverRef::loop_fn(loop_lane_resolver),
        )
        .expect("register loop resolver");
}

fn controller_program() -> RoleProgram<0> {
    let loop_body = g::send::<Role<0>, Role<1>, Msg<7, u32>, 0>();
    let loop_exit = g::send::<Role<1>, Role<0>, Msg<8, i32>, 0>();
    let loop_continue_arm = g::seq(
        g::send::<
            Role<0>,
            Role<0>,
            Msg<
                { TEST_LOOP_CONTINUE_LOGICAL },
                GenericCapToken<LoopContinueKind>,
                LoopContinueKind,
            >,
            0,
        >()
        .policy::<LOOP_POLICY_ID>(),
        loop_body,
    );
    let loop_break_arm = g::seq(
        g::send::<
            Role<0>,
            Role<0>,
            Msg<{ TEST_LOOP_BREAK_LOGICAL }, GenericCapToken<LoopBreakKind>, LoopBreakKind>,
            0,
        >()
        .policy::<LOOP_POLICY_ID>(),
        loop_exit,
    );
    let loop_segment = g::route(loop_continue_arm, loop_break_arm);
    let protocol = g::seq(g::send::<Role<0>, Role<1>, Msg<10, ()>, 0>(), loop_segment);
    project(&protocol)
}

fn target_program() -> RoleProgram<1> {
    let loop_body = g::send::<Role<0>, Role<1>, Msg<7, u32>, 0>();
    let loop_exit = g::send::<Role<1>, Role<0>, Msg<8, i32>, 0>();
    let loop_continue_arm = g::seq(
        g::send::<
            Role<0>,
            Role<0>,
            Msg<
                { TEST_LOOP_CONTINUE_LOGICAL },
                GenericCapToken<LoopContinueKind>,
                LoopContinueKind,
            >,
            0,
        >()
        .policy::<LOOP_POLICY_ID>(),
        loop_body,
    );
    let loop_break_arm = g::seq(
        g::send::<
            Role<0>,
            Role<0>,
            Msg<{ TEST_LOOP_BREAK_LOGICAL }, GenericCapToken<LoopBreakKind>, LoopBreakKind>,
            0,
        >()
        .policy::<LOOP_POLICY_ID>(),
        loop_exit,
    );
    let loop_segment = g::route(loop_continue_arm, loop_break_arm);
    let protocol = g::seq(g::send::<Role<0>, Role<1>, Msg<10, ()>, 0>(), loop_segment);
    project(&protocol)
}

fn transport_queue_is_empty(transport: &TestTransport) -> bool {
    transport.queue_is_empty()
}

fn controller_send_handshake(controller: &mut hibana::Endpoint<'_, 0>) {
    futures::executor::block_on(
        controller
            .flow::<Msg<10, ()>>()
            .expect("handshake flow")
            .send(&()),
    )
    .expect("handshake send");
}

fn target_recv_handshake(target: &mut hibana::Endpoint<'_, 1>) {
    let () = futures::executor::block_on(target.recv::<Msg<10, ()>>()).expect("handshake recv");
}

fn controller_send_continue(controller: &mut hibana::Endpoint<'_, 0>) {
    futures::executor::block_on(
        controller
            .flow::<Msg<
                { TEST_LOOP_CONTINUE_LOGICAL },
                GenericCapToken<LoopContinueKind>,
                LoopContinueKind,
            >>()
            .expect("continue flow")
            .send(()),
    )
    .expect("continue send");
}

fn controller_send_body(controller: &mut hibana::Endpoint<'_, 0>) {
    futures::executor::block_on(
        controller
            .flow::<Msg<7, u32>>()
            .expect("loop body flow")
            .send(&1),
    )
    .expect("loop body send");
}

fn target_recv_body(target: &mut hibana::Endpoint<'_, 1>) {
    let branch = futures::executor::block_on(target.offer()).expect("target offer iteration 1");
    assert_eq!(
        branch.label(),
        7,
        "continue arm exposes BodyMsg recv to passive observer"
    );
    let first_body = futures::executor::block_on(branch.decode::<Msg<7, u32>>())
        .expect("decode body in continue arm");
    assert_eq!(first_body, 1);
}

fn controller_send_break(controller: &mut hibana::Endpoint<'_, 0>) {
    futures::executor::block_on(
        controller
            .flow::<Msg<{ TEST_LOOP_BREAK_LOGICAL }, GenericCapToken<LoopBreakKind>, LoopBreakKind>>()
            .expect("break flow")
            .send(()),
    )
    .expect("break send");
}

fn target_send_exit(target: &mut hibana::Endpoint<'_, 1>) {
    futures::executor::block_on(
        target
            .flow::<Msg<8, i32>>()
            .expect("exit marker flow")
            .send(&0),
    )
    .expect("exit marker send");
}

fn controller_recv_exit(controller: &mut hibana::Endpoint<'_, 0>) -> i32 {
    futures::executor::block_on(controller.recv::<Msg<8, i32>>()).expect("exit recv")
}

fn run_loop_lane_share(
    cluster: &'static TestKit,
    tap_buf: &'static mut [TapEvent; runtime_support::RING_EVENTS],
    slab: &'static mut [u8],
    transport: &TestTransport,
) {
    let config = Config::new(tap_buf, slab);
    let rv_id = cluster
        .add_rendezvous_from_config(config, transport.clone())
        .expect("register rendezvous");
    set_loop_decision_index(0);
    register_loop_lane_resolvers(cluster, rv_id);

    let sid = SessionId::new(9);
    let controller_program = controller_program();
    let target_program = target_program();
    with_tls_mut(
        &CONTROLLER_SLOT,
        |ptr| unsafe {
            write_value(
                ptr,
                cluster
                    .enter(rv_id, sid, &controller_program, NoBinding)
                    .expect("controller attach"),
            );
        },
        |controller| {
            with_tls_mut(
                &TARGET_SLOT,
                |ptr| unsafe {
                    write_value(
                        ptr,
                        cluster
                            .enter(rv_id, sid, &target_program, NoBinding)
                            .expect("target attach"),
                    );
                },
                |target| {
                    controller_send_handshake(controller);
                    target_recv_handshake(target);
                    controller_send_continue(controller);
                    controller_send_body(controller);
                    target_recv_body(target);
                    controller_send_break(controller);
                    target_send_exit(target);
                    let exit_value = controller_recv_exit(controller);
                    assert_eq!(exit_value, 0);

                    assert!(transport_queue_is_empty(transport));
                },
            );
        },
    );
}

/// Test that loop control operates via flow().send() pattern (Pattern A).
///
/// Per AGENTS.md Branch Patterns:
/// - Controller uses flow().send() to explicitly decide Continue/Break
/// - Target (passive observer) uses offer() to observe the selected arm
#[test]
fn loop_and_control_plane_tokens_share_lane() {
    with_fixture(|clock, tap_buf, slab| {
        let transport = TestTransport::default();
        with_tls_ref(
            &SESSION_SLOT,
            |ptr| unsafe {
                ptr.write(SessionKit::new(clock));
            },
            |cluster| run_loop_lane_share(cluster, tap_buf, slab, &transport),
        );
    });
}
