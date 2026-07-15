mod common;
#[path = "support/large_choreography/fanout_program.rs"]
mod fanout_program;
#[path = "support/large_choreography/huge_program.rs"]
mod huge_program;
#[path = "support/large_choreography/linear_program.rs"]
mod linear_program;
#[path = "support/large_choreography/localside.rs"]
mod localside;
#[path = "support/large_choreography/route_localside.rs"]
mod route_localside;
#[path = "support/runtime.rs"]
mod runtime_support;

use common::TestTransport;
use hibana::{
    Endpoint, g,
    g::Msg,
    runtime::program::{RoleProgram, project},
    runtime::{SessionKitStorage, ids::SessionId},
};

type HugeKitStorage<'a> = SessionKitStorage<'a, TestTransport>;

fn drive<F: core::future::Future>(future: F) -> F::Output {
    let mut future = core::pin::pin!(future);
    drive_pinned(future.as_mut())
}

fn drive_pinned<F: core::future::Future>(mut future: core::pin::Pin<&mut F>) -> F::Output {
    use core::task::{Context, Poll, Waker};

    let mut cx = Context::from_waker(Waker::noop());
    loop {
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(output) => return output,
            Poll::Pending => std::thread::yield_now(),
        }
    }
}

macro_rules! ordered_message_block {
    (($first_label:literal, $second_label:literal) $(, ($next_first:literal, $next_second:literal))* $(,)?) => {{
        let program = g::seq(
            g::send::<0, 1, Msg<$first_label, u8>>(),
            g::send::<0, 1, Msg<$second_label, u8>>(),
        );
        $(
            let program = g::seq(program, g::send::<0, 1, Msg<$next_first, u8>>());
            let program = g::seq(program, g::send::<0, 1, Msg<$next_second, u8>>());
        )*
        program
    }};
}

macro_rules! ordered_message_block_48 {
    () => {
        ordered_message_block!(
            (58, 59),
            (60, 61),
            (62, 63),
            (64, 65),
            (66, 67),
            (68, 69),
            (70, 71),
            (72, 73),
            (74, 75),
            (76, 77),
            (78, 79),
            (80, 81),
            (82, 83),
            (84, 85),
            (86, 87),
            (88, 89),
            (90, 91),
            (92, 93),
            (94, 95),
            (96, 97),
            (98, 99),
            (100, 101),
            (102, 103),
            (104, 105),
        )
    };
}

macro_rules! over_256_one_way_program {
    () => {{
        let block_0 = ordered_message_block_48!();
        let block_1 = ordered_message_block_48!();
        let block_2 = ordered_message_block_48!();
        let block_3 = ordered_message_block_48!();
        let block_4 = ordered_message_block_48!();
        let block_5 = ordered_message_block_48!();
        g::seq(
            g::send::<0, 1, Msg<0, u8>>(),
            g::seq(
                g::seq(block_0, block_1),
                g::seq(g::seq(block_2, block_3), g::seq(block_4, block_5)),
            ),
        )
    }};
}

macro_rules! drive_over_256_one_way {
    ($controller:ident, $worker:ident, $(($first_label:literal, $second_label:literal)),+ $(,)?) => {{
        localside::controller_send_u8::<0>(&mut $controller, 0);
        assert_eq!(localside::worker_recv_u8::<0>(&mut $worker), 0);
        $(
            localside::controller_send_u8::<$first_label>(&mut $controller, $first_label as u8);
            assert_eq!(
                localside::worker_recv_u8::<$first_label>(&mut $worker),
                $first_label as u8,
                "linear >256 runtime payload must roundtrip through first label {}",
                $first_label,
            );
            localside::controller_send_u8::<$second_label>(&mut $controller, $second_label as u8);
            assert_eq!(
                localside::worker_recv_u8::<$second_label>(&mut $worker),
                $second_label as u8,
                "linear >256 runtime payload must roundtrip through second label {}",
                $second_label,
            );
        )+
    }};
}

macro_rules! over_256_label_pairs {
    ($macro_name:ident $(, $arg:ident)* $(,)?) => {
        $macro_name!(
            $($arg,)*
            (58, 59), (60, 61), (62, 63), (64, 65), (66, 67), (68, 69),
            (70, 71), (72, 73), (74, 75), (76, 77), (78, 79), (80, 81),
            (82, 83), (84, 85), (86, 87), (88, 89), (90, 91), (92, 93),
            (94, 95), (96, 97), (98, 99), (100, 101), (102, 103), (104, 105),
            (58, 59), (60, 61), (62, 63), (64, 65), (66, 67), (68, 69),
            (70, 71), (72, 73), (74, 75), (76, 77), (78, 79), (80, 81),
            (82, 83), (84, 85), (86, 87), (88, 89), (90, 91), (92, 93),
            (94, 95), (96, 97), (98, 99), (100, 101), (102, 103), (104, 105),
            (58, 59), (60, 61), (62, 63), (64, 65), (66, 67), (68, 69),
            (70, 71), (72, 73), (74, 75), (76, 77), (78, 79), (80, 81),
            (82, 83), (84, 85), (86, 87), (88, 89), (90, 91), (92, 93),
            (94, 95), (96, 97), (98, 99), (100, 101), (102, 103), (104, 105),
            (58, 59), (60, 61), (62, 63), (64, 65), (66, 67), (68, 69),
            (70, 71), (72, 73), (74, 75), (76, 77), (78, 79), (80, 81),
            (82, 83), (84, 85), (86, 87), (88, 89), (90, 91), (92, 93),
            (94, 95), (96, 97), (98, 99), (100, 101), (102, 103), (104, 105),
            (58, 59), (60, 61), (62, 63), (64, 65), (66, 67), (68, 69),
            (70, 71), (72, 73), (74, 75), (76, 77), (78, 79), (80, 81),
            (82, 83), (84, 85), (86, 87), (88, 89), (90, 91), (92, 93),
            (94, 95), (96, 97), (98, 99), (100, 101), (102, 103), (104, 105),
            (58, 59), (60, 61), (62, 63), (64, 65), (66, 67), (68, 69),
            (70, 71), (72, 73), (74, 75), (76, 77), (78, 79), (80, 81),
            (82, 83), (84, 85), (86, 87), (88, 89), (90, 91), (92, 93),
            (94, 95), (96, 97), (98, 99), (100, 101), (102, 103), (104, 105),
        )
    };
}

macro_rules! concurrent_edge_program {
    ($from:literal, $to:literal; $first:literal $(, $rest:literal)* $(,)?) => {{
        let program = g::send::<$from, $to, Msg<$first, ()>>();
        $(
            let program = g::par(g::send::<$from, $to, Msg<$rest, ()>>(), program);
        )*
        program
    }};
}

const HIGH_LANE_LEFT_LABEL: u8 = 82;
const HIGH_LANE_RIGHT_LABEL: u8 = 83;
const HIGH_LANE_LEFT_REPLY_LABEL: u8 = 84;
const HIGH_LANE_RIGHT_REPLY_LABEL: u8 = 85;
const EDGE_LANE_LABEL: u8 = 86;
const EDGE_LANE_REPLY_LABEL: u8 = 87;

fn high_lane_controller_program() -> RoleProgram<0> {
    let high_lane_left_program = g::seq(
        g::send::<0, 1, Msg<{ HIGH_LANE_LEFT_LABEL }, u8>>(),
        g::send::<1, 0, Msg<{ HIGH_LANE_LEFT_REPLY_LABEL }, u8>>(),
    );

    let high_lane_right_program = g::seq(
        g::send::<0, 1, Msg<{ HIGH_LANE_RIGHT_LABEL }, u8>>(),
        g::send::<1, 0, Msg<{ HIGH_LANE_RIGHT_REPLY_LABEL }, u8>>(),
    );

    let program = g::route(high_lane_left_program, high_lane_right_program);
    project(&program)
}

fn high_lane_worker_program() -> RoleProgram<1> {
    let high_lane_left_program = g::seq(
        g::send::<0, 1, Msg<{ HIGH_LANE_LEFT_LABEL }, u8>>(),
        g::send::<1, 0, Msg<{ HIGH_LANE_LEFT_REPLY_LABEL }, u8>>(),
    );

    let high_lane_right_program = g::seq(
        g::send::<0, 1, Msg<{ HIGH_LANE_RIGHT_LABEL }, u8>>(),
        g::send::<1, 0, Msg<{ HIGH_LANE_RIGHT_REPLY_LABEL }, u8>>(),
    );

    let program = g::route(high_lane_left_program, high_lane_right_program);
    project(&program)
}

fn edge_lane_controller_program() -> RoleProgram<0> {
    let program = g::seq(
        g::send::<0, 1, Msg<{ EDGE_LANE_LABEL }, u8>>(),
        g::send::<1, 0, Msg<{ EDGE_LANE_REPLY_LABEL }, u8>>(),
    );
    project(&program)
}

fn edge_lane_worker_program() -> RoleProgram<1> {
    let program = g::seq(
        g::send::<0, 1, Msg<{ EDGE_LANE_LABEL }, u8>>(),
        g::send::<1, 0, Msg<{ EDGE_LANE_REPLY_LABEL }, u8>>(),
    );
    project(&program)
}

#[inline(never)]
fn run_attached_sample(
    controller_program: &hibana::runtime::program::RoleProgram<0>,
    worker_program: &hibana::runtime::program::RoleProgram<1>,
    route_scope_count: usize,
    expected_branch_labels: &'static [u8],
    expected_acks: &'static [u8],
    run: fn(&mut Endpoint<'_, 0>, &mut Endpoint<'_, 1>),
) {
    assert_eq!(route_scope_count, expected_branch_labels.len());
    assert_eq!(route_scope_count, expected_acks.len());

    runtime_support::with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        let mut kit_storage = HugeKitStorage::uninit();
        let kit = kit_storage.init();
        let rv = kit
            .rendezvous(slab, transport.clone())
            .expect("register rendezvous");
        let sid = SessionId::new(0x6000);
        let mut controller = rv.enter(sid, controller_program).expect("enter controller");
        let mut worker = rv.enter(sid, worker_program).expect("enter worker");

        run(&mut controller, &mut worker);
        assert!(
            transport.queue_is_empty(),
            "huge choreography runtime must drain every transport frame"
        );
    });
}

fn retain_large_choreography_symbols() {
    let _ = fanout_program::ROUTE_SCOPE_COUNT;
    let _ = fanout_program::EXPECTED_WORKER_BRANCH_LABELS;
    let _ = fanout_program::ACK_LABELS;
    let _ = huge_program::ROUTE_SCOPE_COUNT;
    let _ = huge_program::EXPECTED_WORKER_BRANCH_LABELS;
    let _ = huge_program::ACK_LABELS;
    let _ = linear_program::ROUTE_SCOPE_COUNT;
    let _ = linear_program::EXPECTED_WORKER_BRANCH_LABELS;
    let _ = linear_program::ACK_LABELS;
    let _ = huge_program::run
        as fn(&mut localside::ControllerEndpoint<'_>, &mut localside::WorkerEndpoint<'_>);
    let _ = linear_program::run
        as fn(&mut localside::ControllerEndpoint<'_>, &mut localside::WorkerEndpoint<'_>);
    let _ = fanout_program::run
        as fn(&mut localside::ControllerEndpoint<'_>, &mut localside::WorkerEndpoint<'_>);
    let _ = localside::worker_offer_recv_u8::<0> as fn(&mut localside::WorkerEndpoint<'_>) -> u8;
}

#[test]
fn huge_programs_stay_on_direct_program_values() {
    retain_large_choreography_symbols();
    assert_eq!(huge_program::ROUTE_SCOPE_COUNT, 4);
    assert_eq!(
        huge_program::EXPECTED_WORKER_BRANCH_LABELS,
        [84, 85, 84, 85]
    );
    assert_eq!(huge_program::ACK_LABELS, [2, 2, 2, 2]);
    assert_eq!(linear_program::ROUTE_SCOPE_COUNT, 0);
    assert_eq!(
        fanout_program::EXPECTED_WORKER_BRANCH_LABELS,
        [81, 84, 85, 88, 89, 92, 93, 96]
    );
    assert_eq!(
        fanout_program::ACK_LABELS,
        [97, 98, 99, 100, 101, 102, 103, 104]
    );
}

#[test]
fn huge_program_shape_matrix_projects_both_roles() {
    retain_large_choreography_symbols();
    let route_heavy_controller: RoleProgram<0> = huge_program::controller_program();
    let route_heavy_worker: RoleProgram<1> = huge_program::worker_program();
    let linear_heavy_controller: RoleProgram<0> = linear_program::controller_program();
    let linear_heavy_worker: RoleProgram<1> = linear_program::worker_program();
    let fanout_heavy_controller: RoleProgram<0> = fanout_program::controller_program();
    let fanout_heavy_worker: RoleProgram<1> = fanout_program::worker_program();

    let _ = (
        &route_heavy_controller,
        &route_heavy_worker,
        &linear_heavy_controller,
        &linear_heavy_worker,
        &fanout_heavy_controller,
        &fanout_heavy_worker,
    );
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
fn over_256_ordered_inbound_occurrences_reuse_frame_labels_and_run() {
    let program = over_256_one_way_program!();
    let controller_program: RoleProgram<0> = project(&program);
    let worker_program: RoleProgram<1> = project(&program);

    runtime_support::with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        let mut kit_storage = HugeKitStorage::uninit();
        let kit = kit_storage.init();
        let rv = kit
            .rendezvous(slab, transport.clone())
            .expect("register rendezvous");
        let sid = SessionId::new(0x6300);
        let mut controller = rv
            .enter(sid, &controller_program)
            .expect("enter >256 controller");
        let mut worker = rv.enter(sid, &worker_program).expect("enter >256 worker");

        over_256_label_pairs!(drive_over_256_one_way, controller, worker);

        assert!(
            transport.queue_is_empty(),
            ">256 ordered inbound occurrences must drain every transport frame"
        );
    });
}

#[test]
fn over_256_globally_parallel_events_reuse_lanes_across_disjoint_roles() {
    let left_low = concurrent_edge_program!(
        0, 1;
        0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,
        16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31,
        32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47,
        48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63,
        64,
    );
    let left_high = concurrent_edge_program!(
        0, 1;
        65, 66, 67, 68, 69, 70, 71, 72, 73, 74, 75, 76, 77, 78, 79,
        80, 81, 82, 83, 84, 85, 86, 87, 88, 89, 90, 91, 92, 93, 94, 95,
        96, 97, 98, 99, 100, 101, 102, 103, 104, 105, 106, 107, 108, 109, 110, 111,
        112, 113, 114, 115, 116, 117, 118, 119, 120, 121, 122, 123, 124, 125, 126, 127,
        128,
    );
    let right_low = concurrent_edge_program!(
        2, 3;
        0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,
        16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31,
        32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47,
        48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63,
        64,
    );
    let right_high = concurrent_edge_program!(
        2, 3;
        65, 66, 67, 68, 69, 70, 71, 72, 73, 74, 75, 76, 77, 78, 79,
        80, 81, 82, 83, 84, 85, 86, 87, 88, 89, 90, 91, 92, 93, 94, 95,
        96, 97, 98, 99, 100, 101, 102, 103, 104, 105, 106, 107, 108, 109, 110, 111,
        112, 113, 114, 115, 116, 117, 118, 119, 120, 121, 122, 123, 124, 125, 126, 127,
        128,
    );
    let left = g::par(left_low, left_high);
    let right = g::par(right_low, right_high);
    let program = g::par(left, right);

    let _: RoleProgram<0> = project(&program);
    let _: RoleProgram<1> = project(&program);
    let _: RoleProgram<2> = project(&program);
    let _: RoleProgram<3> = project(&program);
}

#[test]
fn high_lane_route_runs_to_completion_on_actual_localside() {
    runtime_support::with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        let mut kit_storage = HugeKitStorage::uninit();
        let kit = kit_storage.init();
        let rv = kit
            .rendezvous(slab, transport.clone())
            .expect("register rendezvous");

        let mut controller = rv
            .enter(SessionId::new(0x6100), &high_lane_controller_program())
            .expect("enter controller-left");
        let mut worker = rv
            .enter(SessionId::new(0x6100), &high_lane_worker_program())
            .expect("enter worker-left");
        localside::controller_send_u8::<{ HIGH_LANE_LEFT_LABEL }>(&mut controller, 7);
        assert_eq!(
            localside::worker_offer_recv_u8::<{ HIGH_LANE_LEFT_LABEL }>(&mut worker,),
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

        let mut controller = rv
            .enter(SessionId::new(0x6101), &high_lane_controller_program())
            .expect("enter controller-right");
        let mut worker = rv
            .enter(SessionId::new(0x6101), &high_lane_worker_program())
            .expect("enter worker-right");
        localside::controller_send_u8::<{ HIGH_LANE_RIGHT_LABEL }>(&mut controller, 9);
        assert_eq!(
            localside::worker_offer_recv_u8::<{ HIGH_LANE_RIGHT_LABEL }>(&mut worker,),
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
fn lane_255_runs_to_completion_on_public_sessionkit_path() {
    runtime_support::with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        let mut kit_storage = HugeKitStorage::uninit();
        let kit = kit_storage.init();
        let rv = kit
            .rendezvous(slab, transport.clone())
            .expect("register rendezvous with the full wire lane domain");

        let mut controller = rv
            .enter(SessionId::new(0x6200), &edge_lane_controller_program())
            .expect("enter lane-255 controller");
        let mut worker = rv
            .enter(SessionId::new(0x6200), &edge_lane_worker_program())
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
