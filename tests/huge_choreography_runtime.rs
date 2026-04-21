#![cfg(feature = "std")]

mod common;
#[path = "../internal/pico_smoke/src/fanout_program.rs"]
mod fanout_program;
#[path = "../internal/pico_smoke/src/huge_program.rs"]
mod huge_program;
#[path = "../internal/pico_smoke/src/linear_program.rs"]
mod linear_program;
#[path = "../internal/pico_smoke/src/localside.rs"]
mod localside;
#[path = "../internal/pico_smoke/src/route_control_kinds.rs"]
mod route_control_kinds;
#[path = "../internal/pico_smoke/src/route_localside.rs"]
mod route_localside;
#[path = "support/runtime.rs"]
mod runtime_support;

use common::TestTransport;
use hibana::{
    Endpoint, g,
    g::advanced::{RoleProgram, project},
    g::{Msg, Role},
    substrate::{
        SessionId, SessionKit,
        binding::NoBinding,
        cap::GenericCapToken,
        runtime::{Config, CounterClock, DefaultLabelUniverse},
    },
};

type HugeKit = SessionKit<'static, TestTransport, DefaultLabelUniverse, CounterClock, 2>;

fn drive<F: core::future::Future>(future: F) -> F::Output {
    let mut future = core::pin::pin!(future);
    drive_pinned(future.as_mut())
}

fn drive_pinned<F: core::future::Future>(mut future: core::pin::Pin<&mut F>) -> F::Output {
    use core::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

    const VTABLE: RawWakerVTable = RawWakerVTable::new(
        |_| RawWaker::new(core::ptr::null(), &VTABLE),
        |_| {},
        |_| {},
        |_| {},
    );

    fn noop_waker() -> Waker {
        unsafe { Waker::from_raw(RawWaker::new(core::ptr::null(), &VTABLE)) }
    }

    let waker = noop_waker();
    let mut cx = Context::from_waker(&waker);
    loop {
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(output) => return output,
            Poll::Pending => std::thread::yield_now(),
        }
    }
}

const HIGH_LANE_LEFT_CTRL: u8 = 122;
const HIGH_LANE_RIGHT_CTRL: u8 = 123;
const HIGH_LANE_LEFT_LABEL: u8 = 82;
const HIGH_LANE_RIGHT_LABEL: u8 = 83;
const HIGH_LANE_LEFT_REPLY_LABEL: u8 = 84;
const HIGH_LANE_RIGHT_REPLY_LABEL: u8 = 85;
const HIGH_LANE_LEFT: u8 = 33;
const HIGH_LANE_RIGHT: u8 = 34;

type HighLaneLeftKind = route_control_kinds::RouteControl<HIGH_LANE_LEFT_CTRL, 0>;
type HighLaneRightKind = route_control_kinds::RouteControl<HIGH_LANE_RIGHT_CTRL, 1>;

fn high_lane_controller_program() -> RoleProgram<0> {
    let high_lane_left_program = {
        let program = g::send::<
            Role<0>,
            Role<0>,
            Msg<{ HIGH_LANE_LEFT_CTRL }, GenericCapToken<HighLaneLeftKind>, HighLaneLeftKind>,
            0,
        >();
        g::seq(
            program,
            g::seq(
                g::send::<Role<0>, Role<1>, Msg<{ HIGH_LANE_LEFT_LABEL }, u8>, HIGH_LANE_LEFT>(),
                g::send::<Role<1>, Role<0>, Msg<{ HIGH_LANE_LEFT_REPLY_LABEL }, u8>, HIGH_LANE_LEFT>(
                ),
            ),
        )
    };

    let high_lane_right_program = {
        let program = g::send::<
            Role<0>,
            Role<0>,
            Msg<{ HIGH_LANE_RIGHT_CTRL }, GenericCapToken<HighLaneRightKind>, HighLaneRightKind>,
            0,
        >();
        g::seq(
            program,
            g::seq(
                g::send::<Role<0>, Role<1>, Msg<{ HIGH_LANE_RIGHT_LABEL }, u8>, HIGH_LANE_RIGHT>(),
                g::send::<
                    Role<1>,
                    Role<0>,
                    Msg<{ HIGH_LANE_RIGHT_REPLY_LABEL }, u8>,
                    HIGH_LANE_RIGHT,
                >(),
            ),
        )
    };

    let program = g::route(high_lane_left_program, high_lane_right_program);
    project(&program)
}

fn high_lane_worker_program() -> RoleProgram<1> {
    let high_lane_left_program = {
        let program = g::send::<
            Role<0>,
            Role<0>,
            Msg<{ HIGH_LANE_LEFT_CTRL }, GenericCapToken<HighLaneLeftKind>, HighLaneLeftKind>,
            0,
        >();
        g::seq(
            program,
            g::seq(
                g::send::<Role<0>, Role<1>, Msg<{ HIGH_LANE_LEFT_LABEL }, u8>, HIGH_LANE_LEFT>(),
                g::send::<Role<1>, Role<0>, Msg<{ HIGH_LANE_LEFT_REPLY_LABEL }, u8>, HIGH_LANE_LEFT>(
                ),
            ),
        )
    };

    let high_lane_right_program = {
        let program = g::send::<
            Role<0>,
            Role<0>,
            Msg<{ HIGH_LANE_RIGHT_CTRL }, GenericCapToken<HighLaneRightKind>, HighLaneRightKind>,
            0,
        >();
        g::seq(
            program,
            g::seq(
                g::send::<Role<0>, Role<1>, Msg<{ HIGH_LANE_RIGHT_LABEL }, u8>, HIGH_LANE_RIGHT>(),
                g::send::<
                    Role<1>,
                    Role<0>,
                    Msg<{ HIGH_LANE_RIGHT_REPLY_LABEL }, u8>,
                    HIGH_LANE_RIGHT,
                >(),
            ),
        )
    };

    let program = g::route(high_lane_left_program, high_lane_right_program);
    project(&program)
}

#[inline(never)]
fn run_attached_sample(
    controller_program: &hibana::g::advanced::RoleProgram<0>,
    worker_program: &hibana::g::advanced::RoleProgram<1>,
    route_scope_count: usize,
    expected_branch_labels: &'static [u8],
    expected_acks: &'static [u8],
    run: fn(&mut Endpoint<'_, 0>, &mut Endpoint<'_, 1>),
) {
    assert_eq!(route_scope_count, expected_branch_labels.len());
    assert_eq!(route_scope_count, expected_acks.len());

    runtime_support::with_fixture(|clock, tap_buf, slab| {
        let transport = TestTransport::default();
        let kit = HugeKit::new(clock);
        let rv_id = kit
            .add_rendezvous_from_config(Config::new(tap_buf, slab), transport.clone())
            .expect("register rendezvous");
        let sid = SessionId::new(0x6000);
        let mut controller = kit
            .enter(rv_id, sid, controller_program, NoBinding)
            .expect("enter controller");
        let mut worker = kit
            .enter(rv_id, sid, worker_program, NoBinding)
            .expect("enter worker");

        run(&mut controller, &mut worker);
        assert!(
            transport.queue_is_empty(),
            "huge choreography runtime must drain every transport frame"
        );
    });
}

#[inline(never)]
fn run_on_small_stack(name: &'static str, f: impl FnOnce() + Send + 'static) {
    let handle = std::thread::Builder::new()
        .name(name.into())
        .stack_size(32 * 1024)
        .spawn(f)
        .expect("spawn huge choreography runtime thread");
    handle.join().expect("small-stack huge choreography thread");
}

#[test]
fn huge_choreography_shape_matrix_runs_to_completion_on_small_stack() {
    run_on_small_stack("huge-choreography-route-heavy", || {
        let controller_program = huge_program::controller_program();
        let worker_program = huge_program::worker_program();
        run_attached_sample(
            &controller_program,
            &worker_program,
            huge_program::ROUTE_SCOPE_COUNT,
            &huge_program::EXPECTED_WORKER_BRANCH_LABELS,
            &huge_program::ACK_LABELS,
            huge_program::run,
        );
    });

    run_on_small_stack("huge-choreography-linear-heavy", || {
        let controller_program = linear_program::controller_program();
        let worker_program = linear_program::worker_program();
        run_attached_sample(
            &controller_program,
            &worker_program,
            linear_program::ROUTE_SCOPE_COUNT,
            &linear_program::EXPECTED_WORKER_BRANCH_LABELS,
            &linear_program::ACK_LABELS,
            linear_program::run,
        );
    });

    run_on_small_stack("huge-choreography-fanout-heavy", || {
        let controller_program = fanout_program::controller_program();
        let worker_program = fanout_program::worker_program();
        run_attached_sample(
            &controller_program,
            &worker_program,
            fanout_program::ROUTE_SCOPE_COUNT,
            &fanout_program::EXPECTED_WORKER_BRANCH_LABELS,
            &fanout_program::ACK_LABELS,
            fanout_program::run,
        );
    });
}

#[test]
fn high_lane_route_runs_to_completion_on_actual_localside() {
    runtime_support::with_fixture(|clock, tap_buf, slab| {
        let transport = TestTransport::default();
        let kit = HugeKit::new(clock);
        let rv_id = kit
            .add_rendezvous_from_config(
                Config::new(tap_buf, slab).with_lane_range(0..35),
                transport.clone(),
            )
            .expect("register rendezvous");

        let mut controller = kit
            .enter(
                rv_id,
                SessionId::new(0x6100),
                &high_lane_controller_program(),
                NoBinding,
            )
            .expect("enter controller-left");
        let mut worker = kit
            .enter(
                rv_id,
                SessionId::new(0x6100),
                &high_lane_worker_program(),
                NoBinding,
            )
            .expect("enter worker-left");
        route_localside::controller_select::<HighLaneLeftKind>(&mut controller);
        localside::controller_send_u8::<{ HIGH_LANE_LEFT_LABEL }>(&mut controller, 7);
        assert_eq!(
            localside::worker_offer_decode_u8::<{ HIGH_LANE_LEFT_LABEL }>(&mut worker,),
            7,
            "lane 33 route payload must survive exact lane-set runtime selection"
        );
        localside::worker_send_u8::<{ HIGH_LANE_LEFT_REPLY_LABEL }>(&mut worker, 17);
        assert_eq!(
            localside::controller_recv_u8::<{ HIGH_LANE_LEFT_REPLY_LABEL }>(&mut controller),
            17,
            "lane 33 reply payload must roundtrip through SessionKit localside"
        );
        drop(worker);
        drop(controller);

        let mut controller = kit
            .enter(
                rv_id,
                SessionId::new(0x6101),
                &high_lane_controller_program(),
                NoBinding,
            )
            .expect("enter controller-right");
        let mut worker = kit
            .enter(
                rv_id,
                SessionId::new(0x6101),
                &high_lane_worker_program(),
                NoBinding,
            )
            .expect("enter worker-right");
        route_localside::controller_select::<HighLaneRightKind>(&mut controller);
        localside::controller_send_u8::<{ HIGH_LANE_RIGHT_LABEL }>(&mut controller, 9);
        assert_eq!(
            localside::worker_offer_decode_u8::<{ HIGH_LANE_RIGHT_LABEL }>(&mut worker,),
            9,
            "lane 34 route payload must survive exact lane-set runtime selection"
        );
        localside::worker_send_u8::<{ HIGH_LANE_RIGHT_REPLY_LABEL }>(&mut worker, 19);
        assert_eq!(
            localside::controller_recv_u8::<{ HIGH_LANE_RIGHT_REPLY_LABEL }>(&mut controller),
            19,
            "lane 34 reply payload must roundtrip through SessionKit localside"
        );

        assert!(
            transport.queue_is_empty(),
            "high-lane localside route test must drain every transport frame"
        );
    });
}
