#![cfg(feature = "std")]
#![recursion_limit = "512"]

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
    g::{Msg, Role},
    substrate::program::{RoleProgram, project},
    substrate::{
        SessionKit,
        binding::NoBinding,
        cap::GenericCapToken,
        ids::SessionId,
        runtime::{Config, CounterClock, DefaultLabelUniverse},
    },
};

type HugeKit = SessionKit<'static, TestTransport, DefaultLabelUniverse, CounterClock, 2>;
type DeepScopeKit = SessionKit<'static, TestTransport, DefaultLabelUniverse, CounterClock, 4>;

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

macro_rules! over_256_linear_program {
    ($($label:literal),+ $(,)?) => {{
        let program = g::send::<Role<0>, Role<1>, Msg<0, u8>, 0>();
        $(
            let program = g::seq(
                program,
                g::send::<Role<0>, Role<1>, Msg<$label, u8>, 0>(),
            );
        )+
        program
    }};
}

macro_rules! drive_over_256_linear {
    ($controller:ident, $worker:ident, $($label:literal),+ $(,)?) => {{
        localside::controller_send_u8::<0>(&mut $controller, 0);
        assert_eq!(localside::worker_recv_u8::<0>(&mut $worker), 0);
        $(
            localside::controller_send_u8::<$label>(&mut $controller, $label as u8);
            assert_eq!(
                localside::worker_recv_u8::<$label>(&mut $worker),
                $label as u8,
                "linear >256 runtime payload must roundtrip through label {}",
                $label,
            );
        )+
    }};
}

macro_rules! over_256_labels {
    ($macro_name:ident $(, $arg:ident)* $(,)?) => {
        $macro_name!(
            $($arg,)*
            58, 59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 69,
            70, 71, 72, 73, 74, 75, 76, 77, 78, 79, 80, 81,
            82, 83, 84, 85, 86, 87, 88, 89, 90, 91, 92, 93,
            94, 95, 96, 97, 98, 99, 100, 101, 102, 103, 104, 105,
            58, 59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 69,
            70, 71, 72, 73, 74, 75, 76, 77, 78, 79, 80, 81,
            82, 83, 84, 85, 86, 87, 88, 89, 90, 91, 92, 93,
            94, 95, 96, 97, 98, 99, 100, 101, 102, 103, 104, 105,
            58, 59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 69,
            70, 71, 72, 73, 74, 75, 76, 77, 78, 79, 80, 81,
            82, 83, 84, 85, 86, 87, 88, 89, 90, 91, 92, 93,
            94, 95, 96, 97, 98, 99, 100, 101, 102, 103, 104, 105,
            58, 59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 69,
            70, 71, 72, 73, 74, 75, 76, 77, 78, 79, 80, 81,
            82, 83, 84, 85, 86, 87, 88, 89, 90, 91, 92, 93,
            94, 95, 96, 97, 98, 99, 100, 101, 102, 103, 104, 105,
            58, 59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 69,
            70, 71, 72, 73, 74, 75, 76, 77, 78, 79, 80, 81,
            82, 83, 84, 85, 86, 87, 88, 89, 90, 91, 92, 93,
            94, 95, 96, 97, 98, 99, 100, 101, 102, 103, 104, 105,
            58, 59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 69,
            70, 71, 72, 73, 74, 75, 76, 77, 78, 79, 80, 81,
            82, 83, 84, 85, 86, 87, 88, 89, 90, 91, 92, 93,
            94, 95, 96, 97, 98, 99, 100, 101, 102, 103, 104, 105,
        )
    };
}

macro_rules! deep_nested_par_scope_program {
    () => {
        g::send::<Role<0>, Role<1>, Msg<90, ()>, 0>()
    };
    ($lane:literal $($tail:literal)*) => {{
        let left = g::send::<Role<2>, Role<3>, Msg<91, ()>, $lane>();
        let right = deep_nested_par_scope_program!($($tail)*);
        g::par(left, right)
    }};
}

const HIGH_LANE_LEFT_CTRL: u8 = 122;
const HIGH_LANE_RIGHT_CTRL: u8 = 123;
const HIGH_LANE_LEFT_LABEL: u8 = 82;
const HIGH_LANE_RIGHT_LABEL: u8 = 83;
const HIGH_LANE_LEFT_REPLY_LABEL: u8 = 84;
const HIGH_LANE_RIGHT_REPLY_LABEL: u8 = 85;
const HIGH_LANE_LEFT: u8 = 33;
const HIGH_LANE_RIGHT: u8 = 34;
const EDGE_LANE: u8 = 255;
const EDGE_LANE_LABEL: u8 = 86;
const EDGE_LANE_REPLY_LABEL: u8 = 87;

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

fn edge_lane_controller_program() -> RoleProgram<0> {
    let program = g::seq(
        g::send::<Role<0>, Role<1>, Msg<{ EDGE_LANE_LABEL }, u8>, EDGE_LANE>(),
        g::send::<Role<1>, Role<0>, Msg<{ EDGE_LANE_REPLY_LABEL }, u8>, EDGE_LANE>(),
    );
    project(&program)
}

fn deep_active_scope_controller_program() -> RoleProgram<0> {
    let program = deep_nested_par_scope_program!(
        0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15
        16 17 18 19 20 21 22 23 24 25 26 27 28 29 30 31
        32 33 34 35 36 37 38 39 40 41 42 43 44 45 46 47
        48 49 50 51 52 53 54 55 56 57 58 59 60 61 62 63
        64 65 66 67 68 69 70 71 72 73 74 75 76 77 78 79
        80 81 82 83 84 85 86 87 88 89 90 91 92 93 94 95
        96 97 98 99 100 101 102 103 104 105 106 107 108 109 110 111
        112 113 114 115 116 117 118 119 120 121 122 123 124 125 126 127
        128
    );
    project(&program)
}

fn edge_lane_worker_program() -> RoleProgram<1> {
    let program = g::seq(
        g::send::<Role<0>, Role<1>, Msg<{ EDGE_LANE_LABEL }, u8>, EDGE_LANE>(),
        g::send::<Role<1>, Role<0>, Msg<{ EDGE_LANE_REPLY_LABEL }, u8>, EDGE_LANE>(),
    );
    project(&program)
}

#[inline(never)]
fn run_attached_sample(
    controller_program: &hibana::substrate::program::RoleProgram<0>,
    worker_program: &hibana::substrate::program::RoleProgram<1>,
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

#[test]
fn huge_choreography_shape_matrix_runs_to_completion_on_actual_localside() {
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
}

#[test]
fn program_over_256_effects_projects_and_runs_through_segment_2() {
    let program = over_256_labels!(over_256_linear_program);
    let controller_program: RoleProgram<0> = project(&program);
    let worker_program: RoleProgram<1> = project(&program);

    runtime_support::with_fixture(|clock, tap_buf, slab| {
        let transport = TestTransport::default();
        let kit = HugeKit::new(clock);
        let rv_id = kit
            .add_rendezvous_from_config(Config::new(tap_buf, slab), transport.clone())
            .expect("register rendezvous");
        let sid = SessionId::new(0x6300);
        let mut controller = kit
            .enter(rv_id, sid, &controller_program, NoBinding)
            .expect("enter >256 controller");
        let mut worker = kit
            .enter(rv_id, sid, &worker_program, NoBinding)
            .expect("enter >256 worker");

        over_256_labels!(drive_over_256_linear, controller, worker);

        assert!(
            transport.queue_is_empty(),
            ">256 effect runtime proof must drain every transport frame"
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

#[test]
fn active_scope_depth_above_128_enters_public_sessionkit_path() {
    runtime_support::with_fixture(|clock, tap_buf, slab| {
        let transport = TestTransport::default();
        let kit = DeepScopeKit::new(clock);
        let rv_id = kit
            .add_rendezvous_from_config(
                Config::new(tap_buf, slab).with_lane_range(0..256),
                transport.clone(),
            )
            .expect("register deep-scope rendezvous");

        let _controller = kit
            .enter(
                rv_id,
                SessionId::new(0x6210),
                &deep_active_scope_controller_program(),
                NoBinding,
            )
            .expect("enter role with >128 active nested scopes");
    });
}

#[test]
fn lane_255_runs_to_completion_on_public_sessionkit_path() {
    runtime_support::with_fixture(|clock, tap_buf, slab| {
        let transport = TestTransport::default();
        let kit = HugeKit::new(clock);
        let rv_id = kit
            .add_rendezvous_from_config(
                Config::new(tap_buf, slab).with_lane_range(0..256),
                transport.clone(),
            )
            .expect("register rendezvous with the full wire lane domain");

        let mut controller = kit
            .enter(
                rv_id,
                SessionId::new(0x6200),
                &edge_lane_controller_program(),
                NoBinding,
            )
            .expect("enter lane-255 controller");
        let mut worker = kit
            .enter(
                rv_id,
                SessionId::new(0x6200),
                &edge_lane_worker_program(),
                NoBinding,
            )
            .expect("enter lane-255 worker");

        localside::controller_send_u8::<{ EDGE_LANE_LABEL }>(&mut controller, 11);
        assert_eq!(
            localside::worker_recv_u8::<{ EDGE_LANE_LABEL }>(&mut worker),
            11,
            "lane 255 payload must be reachable through public SessionKit config"
        );
        localside::worker_send_u8::<{ EDGE_LANE_REPLY_LABEL }>(&mut worker, 29);
        assert_eq!(
            localside::controller_recv_u8::<{ EDGE_LANE_REPLY_LABEL }>(&mut controller),
            29,
            "lane 255 reply must roundtrip through the real localside runtime"
        );

        assert!(
            transport.queue_is_empty(),
            "lane-255 localside test must drain every transport frame"
        );
    });
}
