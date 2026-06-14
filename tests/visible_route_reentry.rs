#![cfg(feature = "std")]

mod common;
#[path = "support/runtime.rs"]
mod runtime_support;
#[path = "support/tls_ref.rs"]
mod tls_ref_support;

use core::cell::UnsafeCell;

use common::TestTransport;
use hibana::g::{self, Message, Msg};
use hibana::runtime::program::{RoleProgram, project};
use hibana::runtime::{Config, CounterClock, SessionKitStorage, ids::SessionId};
use runtime_support::with_runtime_workspace;
use tls_ref_support::with_resident_tls_ref;

type TestKitStorage = SessionKitStorage<'static, TestTransport, CounterClock, 2>;

const TOP_BODY_REQ: u8 = 151;
const TOP_BODY_ACK: u8 = 152;
const TOP_EXIT: u8 = 153;

const PAR_LEFT: u8 = 156;
const PAR_RIGHT: u8 = 157;
const PAR_EXIT: u8 = 158;

const OUTER_OPEN: u8 = 161;
const INNER_BODY: u8 = 162;
const INNER_EXIT: u8 = 163;
const OUTER_ACK: u8 = 164;
const OUTER_EXIT: u8 = 165;

const NESTED_BODY: u8 = 171;
const NESTED_OTHER: u8 = 172;
const NESTED_TAIL: u8 = 173;
const NESTED_EXIT: u8 = 174;

std::thread_local! {
    static SESSION_SLOT: UnsafeCell<TestKitStorage> = const {
        UnsafeCell::new(SessionKitStorage::uninit())
    };
}

fn visible_reentry_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let body = g::seq(
        g::send::<0, 1, Msg<TOP_BODY_REQ, u8>>(),
        g::send::<1, 0, Msg<TOP_BODY_ACK, u8>>(),
    )
    .roll();
    project(&g::seq(body, g::send::<0, 1, Msg<TOP_EXIT, u8>>()))
}

fn visible_parallel_reentry_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let body = g::par(
        g::send::<0, 1, Msg<PAR_LEFT, u8>>(),
        g::send::<0, 1, Msg<PAR_RIGHT, u8>>(),
    )
    .roll();
    project(&g::seq(body, g::send::<0, 1, Msg<PAR_EXIT, u8>>()))
}

fn nested_visible_reentry_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let inner = g::route(
        g::send::<0, 1, Msg<INNER_BODY, u8>>(),
        g::send::<0, 1, Msg<INNER_EXIT, u8>>(),
    )
    .roll();
    let outer_body = g::seq(
        g::send::<0, 1, Msg<OUTER_OPEN, u8>>(),
        g::seq(inner, g::send::<1, 0, Msg<OUTER_ACK, u8>>()),
    );
    project(&g::route(outer_body, g::send::<0, 1, Msg<OUTER_EXIT, u8>>()).roll())
}

fn nested_seq_roll_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let inner = g::seq(
        g::send::<0, 1, Msg<NESTED_BODY, u8>>(),
        g::send::<1, 0, Msg<NESTED_OTHER, u8>>(),
    )
    .roll();
    let outer = g::seq(inner, g::send::<0, 1, Msg<NESTED_TAIL, u8>>()).roll();
    project(&g::seq(outer, g::send::<0, 1, Msg<NESTED_EXIT, u8>>()))
}

fn with_visible_reentry_workspace(
    sid: u32,
    controller_program: RoleProgram<0>,
    worker_program: RoleProgram<1>,
    run: impl FnOnce(&mut hibana::Endpoint<'static, 0>, &mut hibana::Endpoint<'static, 1>),
) {
    with_runtime_workspace(|_clock, tap_buf, slab| {
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let config =
                Config::from_resources((tap_buf, slab), hibana::runtime::CounterClock::zero());
            let transport = TestTransport::new();
            let rv = cluster
                .rendezvous(config, transport)
                .expect("register rendezvous");
            let sid = SessionId::new(sid);
            let mut controller = rv
                .session(sid)
                .role(&controller_program)
                .enter()
                .expect("attach controller");
            let mut worker = rv
                .session(sid)
                .role(&worker_program)
                .enter()
                .expect("attach worker");
            run(&mut controller, &mut worker);
        });
    });
}

async fn send_from_controller<const MSG: u8>(
    controller: &mut hibana::Endpoint<'static, 0>,
    value: u8,
) {
    controller
        .flow::<Msg<MSG, u8>>()
        .unwrap_or_else(|err| panic!("controller flow label {MSG}: {err:?}"))
        .send(&value)
        .await
        .unwrap_or_else(|err| panic!("controller send label {MSG}: {err:?}"));
}

async fn send_from_worker<const MSG: u8>(worker: &mut hibana::Endpoint<'static, 1>, value: u8) {
    worker
        .flow::<Msg<MSG, u8>>()
        .expect("worker flow")
        .send(&value)
        .await
        .expect("worker send");
}

async fn offer_worker<const MSG: u8>(worker: &mut hibana::Endpoint<'static, 1>) -> u8 {
    let branch = worker.offer().await.expect("worker offer");
    assert_eq!(branch.label(), <Msg<MSG, u8> as Message>::LOGICAL_LABEL);
    branch
        .decode::<Msg<MSG, u8>>()
        .await
        .expect("worker decode")
}

async fn recv_worker<const MSG: u8>(worker: &mut hibana::Endpoint<'static, 1>) -> u8 {
    worker.recv::<Msg<MSG, u8>>().await.expect("worker recv")
}

async fn recv_controller<const MSG: u8>(controller: &mut hibana::Endpoint<'static, 0>) -> u8 {
    controller
        .recv::<Msg<MSG, u8>>()
        .await
        .expect("controller recv")
}

fn assert_controller_flow_blocked<const MSG: u8>(controller: &mut hibana::Endpoint<'static, 0>) {
    let err = match controller.flow::<Msg<MSG, u8>>() {
        Ok(_) => panic!("controller flow label {MSG} must be blocked"),
        Err(err) => err,
    };
    let rendered = format!("{err:?}");
    assert!(
        rendered.contains("LabelMismatch") || rendered.contains("PhaseInvariant"),
        "controller flow label {MSG} must remain blocked by roll/par progress: {rendered}"
    );
}

#[test]
fn rolled_seq_reenters_by_repeated_head_without_loop_control() {
    with_visible_reentry_workspace(
        960,
        visible_reentry_program::<0>(),
        visible_reentry_program::<1>(),
        |controller, worker| {
            futures::executor::block_on(async {
                send_from_controller::<TOP_BODY_REQ>(controller, 10).await;
                assert_eq!(recv_worker::<TOP_BODY_REQ>(worker).await, 10);
                send_from_worker::<TOP_BODY_ACK>(worker, 11).await;
                assert_eq!(
                    controller
                        .recv::<Msg<TOP_BODY_ACK, u8>>()
                        .await
                        .expect("controller recv first ack"),
                    11
                );

                send_from_controller::<TOP_BODY_REQ>(controller, 20).await;
                assert_eq!(recv_worker::<TOP_BODY_REQ>(worker).await, 20);
                send_from_worker::<TOP_BODY_ACK>(worker, 21).await;
                assert_eq!(
                    controller
                        .recv::<Msg<TOP_BODY_ACK, u8>>()
                        .await
                        .expect("controller recv second ack"),
                    21
                );

                send_from_controller::<TOP_EXIT>(controller, 99).await;
                assert_eq!(recv_worker::<TOP_EXIT>(worker).await, 99);
            });
        },
    );
}

#[test]
fn rolled_par_reenters_only_after_both_lanes_settle() {
    with_visible_reentry_workspace(
        962,
        visible_parallel_reentry_program::<0>(),
        visible_parallel_reentry_program::<1>(),
        |controller, worker| {
            futures::executor::block_on(async {
                send_from_controller::<PAR_LEFT>(controller, 10).await;
                assert_controller_flow_blocked::<PAR_LEFT>(controller);
                assert_controller_flow_blocked::<PAR_EXIT>(controller);

                send_from_controller::<PAR_RIGHT>(controller, 11).await;
                send_from_controller::<PAR_LEFT>(controller, 20).await;
                assert_controller_flow_blocked::<PAR_EXIT>(controller);

                send_from_controller::<PAR_RIGHT>(controller, 21).await;
                send_from_controller::<PAR_EXIT>(controller, 99).await;

                assert_eq!(recv_worker::<PAR_LEFT>(worker).await, 10);
                assert_eq!(recv_worker::<PAR_RIGHT>(worker).await, 11);
                assert_eq!(recv_worker::<PAR_LEFT>(worker).await, 20);
                assert_eq!(recv_worker::<PAR_RIGHT>(worker).await, 21);
                assert_eq!(recv_worker::<PAR_EXIT>(worker).await, 99);
            });
        },
    );
}

#[test]
fn nested_rolled_route_reenters_before_outer_body_continues() {
    with_visible_reentry_workspace(
        961,
        nested_visible_reentry_program::<0>(),
        nested_visible_reentry_program::<1>(),
        |controller, worker| {
            futures::executor::block_on(async {
                send_from_controller::<OUTER_OPEN>(controller, 1).await;
                assert_eq!(offer_worker::<OUTER_OPEN>(worker).await, 1);

                send_from_controller::<INNER_BODY>(controller, 2).await;
                assert_eq!(offer_worker::<INNER_BODY>(worker).await, 2);

                send_from_controller::<INNER_BODY>(controller, 3).await;
                assert_eq!(offer_worker::<INNER_BODY>(worker).await, 3);

                send_from_controller::<INNER_EXIT>(controller, 4).await;
                assert_eq!(offer_worker::<INNER_EXIT>(worker).await, 4);

                send_from_worker::<OUTER_ACK>(worker, 5).await;
                assert_eq!(
                    controller
                        .recv::<Msg<OUTER_ACK, u8>>()
                        .await
                        .expect("controller recv outer ack"),
                    5
                );

                send_from_controller::<OUTER_EXIT>(controller, 6).await;
                assert_eq!(offer_worker::<OUTER_EXIT>(worker).await, 6);
            });
        },
    );
}

#[test]
fn nested_roll_scopes_reenter_inner_until_outer_scope_completes() {
    with_visible_reentry_workspace(
        963,
        nested_seq_roll_program::<0>(),
        nested_seq_roll_program::<1>(),
        |controller, worker| {
            futures::executor::block_on(async {
                send_from_controller::<NESTED_BODY>(controller, 10).await;
                assert_eq!(recv_worker::<NESTED_BODY>(worker).await, 10);
                send_from_worker::<NESTED_OTHER>(worker, 11).await;
                assert_eq!(recv_controller::<NESTED_OTHER>(controller).await, 11);

                send_from_controller::<NESTED_BODY>(controller, 20).await;
                assert_eq!(recv_worker::<NESTED_BODY>(worker).await, 20);
                send_from_worker::<NESTED_OTHER>(worker, 21).await;
                assert_eq!(recv_controller::<NESTED_OTHER>(controller).await, 21);

                send_from_controller::<NESTED_TAIL>(controller, 30).await;
                assert_eq!(recv_worker::<NESTED_TAIL>(worker).await, 30);

                send_from_controller::<NESTED_BODY>(controller, 40).await;
                assert_eq!(recv_worker::<NESTED_BODY>(worker).await, 40);
                send_from_worker::<NESTED_OTHER>(worker, 41).await;
                assert_eq!(recv_controller::<NESTED_OTHER>(controller).await, 41);
                assert_controller_flow_blocked::<NESTED_EXIT>(controller);

                send_from_controller::<NESTED_TAIL>(controller, 50).await;
                assert_eq!(recv_worker::<NESTED_TAIL>(worker).await, 50);
                send_from_controller::<NESTED_EXIT>(controller, 60).await;
                assert_eq!(recv_worker::<NESTED_EXIT>(worker).await, 60);
            });
        },
    );
}
