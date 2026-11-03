mod common;
#[path = "support/large_choreography/fanout_program.rs"]
mod fanout_program;
#[path = "support/large_choreography/huge_program.rs"]
mod huge_program;
#[path = "../examples/support/in_process.rs"]
pub mod in_process;
#[path = "support/large_choreography/linear_program.rs"]
mod linear_program;

use common::TestTransport;
use hibana::{
    g,
    g::Msg,
    runtime::program::{RoleProgram, project},
    runtime::{SessionKitStorage, ids::SessionId},
};

type HugeKitStorage<'a> = SessionKitStorage<'a, TestTransport>;

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
        $controller.send::<Msg<0,u8>>(&(0)).await.expect("send data");
        assert_eq!($worker.recv::<Msg<0,u8>>().await.expect("receive data"), 0);
        $(
            $controller.send::<Msg<$first_label,u8>>(&($first_label as u8)).await.expect("send data");
            assert_eq!(
                $worker.recv::<Msg<$first_label,u8>>().await.expect("receive data"),
                $first_label as u8,
                "linear >256 runtime payload must roundtrip through first label {}",
                $first_label,
            );
            $controller.send::<Msg<$second_label,u8>>(&($second_label as u8)).await.expect("send data");
            assert_eq!(
                $worker.recv::<Msg<$second_label,u8>>().await.expect("receive data"),
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
    let route_heavy_controller: RoleProgram<0> = huge_program::program::<0>();
    let route_heavy_worker: RoleProgram<1> = huge_program::program::<1>();
    let linear_heavy_controller: RoleProgram<0> = linear_program::program::<0>();
    let linear_heavy_worker: RoleProgram<1> = linear_program::program::<1>();
    let fanout_heavy_controller: RoleProgram<0> = fanout_program::program::<0>();
    let fanout_heavy_worker: RoleProgram<1> = fanout_program::program::<1>();

    let _ = (
        &route_heavy_controller,
        &route_heavy_worker,
        &linear_heavy_controller,
        &linear_heavy_worker,
        &fanout_heavy_controller,
        &fanout_heavy_worker,
    );
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn huge_choreography_shape_matrix_runs_to_completion_on_actual_localside() {
    let controller_program = huge_program::program::<0>();
    let worker_program = huge_program::program::<1>();
    {
        assert_eq!(
            huge_program::ROUTE_SCOPE_COUNT,
            huge_program::EXPECTED_WORKER_BRANCH_LABELS.len()
        );
        assert_eq!(
            huge_program::ROUTE_SCOPE_COUNT,
            huge_program::ACK_LABELS.len()
        );
        let mut slab = [0; 64 * 1024];
        let transport = TestTransport::new();
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, transport.clone())
            .expect("register rendezvous");
        let sid = SessionId::new(0x6000);
        let mut controller = rv
            .enter(sid, &controller_program)
            .expect("enter controller");
        let mut worker = rv.enter(sid, &worker_program).expect("enter worker");

        {
            {
                let controller_value: u8 = 1;
                let worker_value: u8 = 2;
                controller
                    .send::<Msg<1, u8>>(&(controller_value))
                    .await
                    .expect("send data");
                assert_eq!(
                    worker.recv::<Msg<1, u8>>().await.expect("receive data"),
                    controller_value
                );
                worker
                    .send::<Msg<2, u8>>(&(worker_value))
                    .await
                    .expect("send data");
                assert_eq!(
                    controller.recv::<Msg<2, u8>>().await.expect("receive data"),
                    worker_value
                );
            }
            {
                let controller_value: u8 = 3;
                let worker_value: u8 = 4;
                controller
                    .send::<Msg<1, u8>>(&(controller_value))
                    .await
                    .expect("send data");
                assert_eq!(
                    worker.recv::<Msg<1, u8>>().await.expect("receive data"),
                    controller_value
                );
                worker
                    .send::<Msg<2, u8>>(&(worker_value))
                    .await
                    .expect("send data");
                assert_eq!(
                    controller.recv::<Msg<2, u8>>().await.expect("receive data"),
                    worker_value
                );
            }
            {
                let controller_value: u8 = 5;
                let worker_value: u8 = 6;
                controller
                    .send::<Msg<1, u8>>(&(controller_value))
                    .await
                    .expect("send data");
                assert_eq!(
                    worker.recv::<Msg<1, u8>>().await.expect("receive data"),
                    controller_value
                );
                worker
                    .send::<Msg<2, u8>>(&(worker_value))
                    .await
                    .expect("send data");
                assert_eq!(
                    controller.recv::<Msg<2, u8>>().await.expect("receive data"),
                    worker_value
                );
            }
            {
                let controller_value: u8 = 7;
                let worker_value: u8 = 8;
                controller
                    .send::<Msg<1, u8>>(&(controller_value))
                    .await
                    .expect("send data");
                assert_eq!(
                    worker.recv::<Msg<1, u8>>().await.expect("receive data"),
                    controller_value
                );
                worker
                    .send::<Msg<2, u8>>(&(worker_value))
                    .await
                    .expect("send data");
                assert_eq!(
                    controller.recv::<Msg<2, u8>>().await.expect("receive data"),
                    worker_value
                );
            }
            {
                let controller_value: u8 = 9;
                let worker_value: u8 = 10;
                controller
                    .send::<Msg<1, u8>>(&(controller_value))
                    .await
                    .expect("send data");
                assert_eq!(
                    worker.recv::<Msg<1, u8>>().await.expect("receive data"),
                    controller_value
                );
                worker
                    .send::<Msg<2, u8>>(&(worker_value))
                    .await
                    .expect("send data");
                assert_eq!(
                    controller.recv::<Msg<2, u8>>().await.expect("receive data"),
                    worker_value
                );
            }
            {
                let controller_value: u8 = 11;
                let worker_value: u8 = 12;
                controller
                    .send::<Msg<1, u8>>(&(controller_value))
                    .await
                    .expect("send data");
                assert_eq!(
                    worker.recv::<Msg<1, u8>>().await.expect("receive data"),
                    controller_value
                );
                worker
                    .send::<Msg<2, u8>>(&(worker_value))
                    .await
                    .expect("send data");
                assert_eq!(
                    controller.recv::<Msg<2, u8>>().await.expect("receive data"),
                    worker_value
                );
            }
            {
                let controller_value: u8 = 13;
                let worker_value: u8 = 14;
                controller
                    .send::<Msg<1, u8>>(&(controller_value))
                    .await
                    .expect("send data");
                assert_eq!(
                    worker.recv::<Msg<1, u8>>().await.expect("receive data"),
                    controller_value
                );
                worker
                    .send::<Msg<2, u8>>(&(worker_value))
                    .await
                    .expect("send data");
                assert_eq!(
                    controller.recv::<Msg<2, u8>>().await.expect("receive data"),
                    worker_value
                );
            }
            {
                let controller_value: u8 = 15;
                let worker_value: u8 = 16;
                controller
                    .send::<Msg<1, u8>>(&(controller_value))
                    .await
                    .expect("send data");
                assert_eq!(
                    worker.recv::<Msg<1, u8>>().await.expect("receive data"),
                    controller_value
                );
                worker
                    .send::<Msg<2, u8>>(&(worker_value))
                    .await
                    .expect("send data");
                assert_eq!(
                    controller.recv::<Msg<2, u8>>().await.expect("receive data"),
                    worker_value
                );
            }
            {
                let controller_value: u8 = 17;
                let worker_value: u8 = 18;
                controller
                    .send::<Msg<1, u8>>(&(controller_value))
                    .await
                    .expect("send data");
                assert_eq!(
                    worker.recv::<Msg<1, u8>>().await.expect("receive data"),
                    controller_value
                );
                worker
                    .send::<Msg<2, u8>>(&(worker_value))
                    .await
                    .expect("send data");
                assert_eq!(
                    controller.recv::<Msg<2, u8>>().await.expect("receive data"),
                    worker_value
                );
            }
            {
                let controller_value: u8 = 19;
                let worker_value: u8 = 20;
                controller
                    .send::<Msg<1, u8>>(&(controller_value))
                    .await
                    .expect("send data");
                assert_eq!(
                    worker.recv::<Msg<1, u8>>().await.expect("receive data"),
                    controller_value
                );
                worker
                    .send::<Msg<2, u8>>(&(worker_value))
                    .await
                    .expect("send data");
                assert_eq!(
                    controller.recv::<Msg<2, u8>>().await.expect("receive data"),
                    worker_value
                );
            }
            {
                let controller_value: u8 = 21;
                let worker_value: u8 = 22;
                controller
                    .send::<Msg<1, u8>>(&(controller_value))
                    .await
                    .expect("send data");
                assert_eq!(
                    worker.recv::<Msg<1, u8>>().await.expect("receive data"),
                    controller_value
                );
                worker
                    .send::<Msg<2, u8>>(&(worker_value))
                    .await
                    .expect("send data");
                assert_eq!(
                    controller.recv::<Msg<2, u8>>().await.expect("receive data"),
                    worker_value
                );
            }
            {
                let controller_value: u8 = 23;
                let worker_value: u8 = 24;
                controller
                    .send::<Msg<1, u8>>(&(controller_value))
                    .await
                    .expect("send data");
                assert_eq!(
                    worker.recv::<Msg<1, u8>>().await.expect("receive data"),
                    controller_value
                );
                worker
                    .send::<Msg<2, u8>>(&(worker_value))
                    .await
                    .expect("send data");
                assert_eq!(
                    controller.recv::<Msg<2, u8>>().await.expect("receive data"),
                    worker_value
                );
            }
            {
                let controller_value: u8 = 25;
                let worker_value: u8 = 26;
                controller
                    .send::<Msg<1, u8>>(&(controller_value))
                    .await
                    .expect("send data");
                assert_eq!(
                    worker.recv::<Msg<1, u8>>().await.expect("receive data"),
                    controller_value
                );
                worker
                    .send::<Msg<2, u8>>(&(worker_value))
                    .await
                    .expect("send data");
                assert_eq!(
                    controller.recv::<Msg<2, u8>>().await.expect("receive data"),
                    worker_value
                );
            }
            {
                let controller_value: u8 = 27;
                let worker_value: u8 = 28;
                controller
                    .send::<Msg<1, u8>>(&(controller_value))
                    .await
                    .expect("send data");
                assert_eq!(
                    worker.recv::<Msg<1, u8>>().await.expect("receive data"),
                    controller_value
                );
                worker
                    .send::<Msg<2, u8>>(&(worker_value))
                    .await
                    .expect("send data");
                assert_eq!(
                    controller.recv::<Msg<2, u8>>().await.expect("receive data"),
                    worker_value
                );
            }
        }
        {
            {
                controller
                    .send::<Msg<84, u32>>(&(0))
                    .await
                    .expect("send data");
                assert_eq!(
                    {
                        let branch = worker.offer().await.expect("worker offer");
                        assert_eq!(branch.label(), 84);
                        branch.recv::<Msg<84, u32>>().await.expect("worker receive")
                    },
                    0
                );
            }
            worker.send::<Msg<2, u8>>(&(92)).await.expect("send data");
            assert_eq!(
                controller.recv::<Msg<2, u8>>().await.expect("receive data"),
                92
            );
            {
                controller
                    .send::<Msg<85, u32>>(&(0))
                    .await
                    .expect("send data");
                assert_eq!(
                    {
                        let branch = worker.offer().await.expect("worker offer");
                        assert_eq!(branch.label(), 85);
                        branch.recv::<Msg<85, u32>>().await.expect("worker receive")
                    },
                    0
                );
            }
            worker.send::<Msg<2, u8>>(&(93)).await.expect("send data");
            assert_eq!(
                controller.recv::<Msg<2, u8>>().await.expect("receive data"),
                93
            );
            {
                controller
                    .send::<Msg<84, u32>>(&(0))
                    .await
                    .expect("send data");
                assert_eq!(
                    {
                        let branch = worker.offer().await.expect("worker offer");
                        assert_eq!(branch.label(), 84);
                        branch.recv::<Msg<84, u32>>().await.expect("worker receive")
                    },
                    0
                );
            }
            worker.send::<Msg<2, u8>>(&(94)).await.expect("send data");
            assert_eq!(
                controller.recv::<Msg<2, u8>>().await.expect("receive data"),
                94
            );
            {
                controller
                    .send::<Msg<85, u32>>(&(0))
                    .await
                    .expect("send data");
                assert_eq!(
                    {
                        let branch = worker.offer().await.expect("worker offer");
                        assert_eq!(branch.label(), 85);
                        branch.recv::<Msg<85, u32>>().await.expect("worker receive")
                    },
                    0
                );
            }
            worker.send::<Msg<2, u8>>(&(95)).await.expect("send data");
            assert_eq!(
                controller.recv::<Msg<2, u8>>().await.expect("receive data"),
                95
            );
        }
        {
            {
                let controller_value: u8 = 96;
                let worker_value: u8 = 97;
                controller
                    .send::<Msg<1, u8>>(&(controller_value))
                    .await
                    .expect("send data");
                assert_eq!(
                    worker.recv::<Msg<1, u8>>().await.expect("receive data"),
                    controller_value
                );
                worker
                    .send::<Msg<2, u8>>(&(worker_value))
                    .await
                    .expect("send data");
                assert_eq!(
                    controller.recv::<Msg<2, u8>>().await.expect("receive data"),
                    worker_value
                );
            }
            {
                let controller_value: u8 = 98;
                let worker_value: u8 = 99;
                controller
                    .send::<Msg<1, u8>>(&(controller_value))
                    .await
                    .expect("send data");
                assert_eq!(
                    worker.recv::<Msg<1, u8>>().await.expect("receive data"),
                    controller_value
                );
                worker
                    .send::<Msg<2, u8>>(&(worker_value))
                    .await
                    .expect("send data");
                assert_eq!(
                    controller.recv::<Msg<2, u8>>().await.expect("receive data"),
                    worker_value
                );
            }
            {
                let controller_value: u8 = 100;
                let worker_value: u8 = 101;
                controller
                    .send::<Msg<1, u8>>(&(controller_value))
                    .await
                    .expect("send data");
                assert_eq!(
                    worker.recv::<Msg<1, u8>>().await.expect("receive data"),
                    controller_value
                );
                worker
                    .send::<Msg<2, u8>>(&(worker_value))
                    .await
                    .expect("send data");
                assert_eq!(
                    controller.recv::<Msg<2, u8>>().await.expect("receive data"),
                    worker_value
                );
            }
            {
                let controller_value: u8 = 102;
                let worker_value: u8 = 103;
                controller
                    .send::<Msg<1, u8>>(&(controller_value))
                    .await
                    .expect("send data");
                assert_eq!(
                    worker.recv::<Msg<1, u8>>().await.expect("receive data"),
                    controller_value
                );
                worker
                    .send::<Msg<2, u8>>(&(worker_value))
                    .await
                    .expect("send data");
                assert_eq!(
                    controller.recv::<Msg<2, u8>>().await.expect("receive data"),
                    worker_value
                );
            }
            {
                let controller_value: u8 = 104;
                let worker_value: u8 = 105;
                controller
                    .send::<Msg<1, u8>>(&(controller_value))
                    .await
                    .expect("send data");
                assert_eq!(
                    worker.recv::<Msg<1, u8>>().await.expect("receive data"),
                    controller_value
                );
                worker
                    .send::<Msg<2, u8>>(&(worker_value))
                    .await
                    .expect("send data");
                assert_eq!(
                    controller.recv::<Msg<2, u8>>().await.expect("receive data"),
                    worker_value
                );
            }
            {
                let controller_value: u8 = 106;
                let worker_value: u8 = 107;
                controller
                    .send::<Msg<1, u8>>(&(controller_value))
                    .await
                    .expect("send data");
                assert_eq!(
                    worker.recv::<Msg<1, u8>>().await.expect("receive data"),
                    controller_value
                );
                worker
                    .send::<Msg<2, u8>>(&(worker_value))
                    .await
                    .expect("send data");
                assert_eq!(
                    controller.recv::<Msg<2, u8>>().await.expect("receive data"),
                    worker_value
                );
            }
            {
                let controller_value: u8 = 108;
                let worker_value: u8 = 109;
                controller
                    .send::<Msg<1, u8>>(&(controller_value))
                    .await
                    .expect("send data");
                assert_eq!(
                    worker.recv::<Msg<1, u8>>().await.expect("receive data"),
                    controller_value
                );
                worker
                    .send::<Msg<2, u8>>(&(worker_value))
                    .await
                    .expect("send data");
                assert_eq!(
                    controller.recv::<Msg<2, u8>>().await.expect("receive data"),
                    worker_value
                );
            }
            {
                let controller_value: u8 = 110;
                let worker_value: u8 = 111;
                controller
                    .send::<Msg<1, u8>>(&(controller_value))
                    .await
                    .expect("send data");
                assert_eq!(
                    worker.recv::<Msg<1, u8>>().await.expect("receive data"),
                    controller_value
                );
                worker
                    .send::<Msg<2, u8>>(&(worker_value))
                    .await
                    .expect("send data");
                assert_eq!(
                    controller.recv::<Msg<2, u8>>().await.expect("receive data"),
                    worker_value
                );
            }
        }

        assert!(
            transport.queue_is_empty(),
            "huge choreography must drain every frame"
        );
    }

    let controller_program = linear_program::program::<0>();
    let worker_program = linear_program::program::<1>();
    {
        assert_eq!(
            linear_program::ROUTE_SCOPE_COUNT,
            linear_program::EXPECTED_WORKER_BRANCH_LABELS.len()
        );
        assert_eq!(
            linear_program::ROUTE_SCOPE_COUNT,
            linear_program::ACK_LABELS.len()
        );
        let mut slab = [0; 64 * 1024];
        let transport = TestTransport::new();
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, transport.clone())
            .expect("register rendezvous");
        let sid = SessionId::new(0x6000);
        let mut controller = rv
            .enter(sid, &controller_program)
            .expect("enter controller");
        let mut worker = rv.enter(sid, &worker_program).expect("enter worker");

        {
            controller
                .send::<Msg<1, u8>>(&(1))
                .await
                .expect("send data");
            assert_eq!(worker.recv::<Msg<1, u8>>().await.expect("receive data"), 1);
            worker.send::<Msg<2, u8>>(&(2)).await.expect("send data");
            assert_eq!(
                controller.recv::<Msg<2, u8>>().await.expect("receive data"),
                2
            );
            controller
                .send::<Msg<3, u8>>(&(3))
                .await
                .expect("send data");
            assert_eq!(worker.recv::<Msg<3, u8>>().await.expect("receive data"), 3);
            worker.send::<Msg<4, u8>>(&(4)).await.expect("send data");
            assert_eq!(
                controller.recv::<Msg<4, u8>>().await.expect("receive data"),
                4
            );
            controller
                .send::<Msg<5, u8>>(&(5))
                .await
                .expect("send data");
            assert_eq!(worker.recv::<Msg<5, u8>>().await.expect("receive data"), 5);
            worker.send::<Msg<6, u8>>(&(6)).await.expect("send data");
            assert_eq!(
                controller.recv::<Msg<6, u8>>().await.expect("receive data"),
                6
            );
            controller
                .send::<Msg<7, u8>>(&(7))
                .await
                .expect("send data");
            assert_eq!(worker.recv::<Msg<7, u8>>().await.expect("receive data"), 7);
            worker.send::<Msg<8, u8>>(&(8)).await.expect("send data");
            assert_eq!(
                controller.recv::<Msg<8, u8>>().await.expect("receive data"),
                8
            );
        }
        {
            controller
                .send::<Msg<9, u8>>(&(9))
                .await
                .expect("send data");
            assert_eq!(worker.recv::<Msg<9, u8>>().await.expect("receive data"), 9);
            worker.send::<Msg<10, u8>>(&(10)).await.expect("send data");
            assert_eq!(
                controller
                    .recv::<Msg<10, u8>>()
                    .await
                    .expect("receive data"),
                10
            );
            controller
                .send::<Msg<11, u8>>(&(11))
                .await
                .expect("send data");
            assert_eq!(
                worker.recv::<Msg<11, u8>>().await.expect("receive data"),
                11
            );
            worker.send::<Msg<12, u8>>(&(12)).await.expect("send data");
            assert_eq!(
                controller
                    .recv::<Msg<12, u8>>()
                    .await
                    .expect("receive data"),
                12
            );
            controller
                .send::<Msg<13, u8>>(&(13))
                .await
                .expect("send data");
            assert_eq!(
                worker.recv::<Msg<13, u8>>().await.expect("receive data"),
                13
            );
            worker.send::<Msg<14, u8>>(&(14)).await.expect("send data");
            assert_eq!(
                controller
                    .recv::<Msg<14, u8>>()
                    .await
                    .expect("receive data"),
                14
            );
            controller
                .send::<Msg<15, u8>>(&(15))
                .await
                .expect("send data");
            assert_eq!(
                worker.recv::<Msg<15, u8>>().await.expect("receive data"),
                15
            );
            worker.send::<Msg<16, u8>>(&(16)).await.expect("send data");
            assert_eq!(
                controller
                    .recv::<Msg<16, u8>>()
                    .await
                    .expect("receive data"),
                16
            );
        }
        {
            controller
                .send::<Msg<17, u8>>(&(17))
                .await
                .expect("send data");
            assert_eq!(
                worker.recv::<Msg<17, u8>>().await.expect("receive data"),
                17
            );
            worker.send::<Msg<18, u8>>(&(18)).await.expect("send data");
            assert_eq!(
                controller
                    .recv::<Msg<18, u8>>()
                    .await
                    .expect("receive data"),
                18
            );
            controller
                .send::<Msg<19, u8>>(&(19))
                .await
                .expect("send data");
            assert_eq!(
                worker.recv::<Msg<19, u8>>().await.expect("receive data"),
                19
            );
            worker.send::<Msg<20, u8>>(&(20)).await.expect("send data");
            assert_eq!(
                controller
                    .recv::<Msg<20, u8>>()
                    .await
                    .expect("receive data"),
                20
            );
            controller
                .send::<Msg<21, u8>>(&(21))
                .await
                .expect("send data");
            assert_eq!(
                worker.recv::<Msg<21, u8>>().await.expect("receive data"),
                21
            );
            worker.send::<Msg<22, u8>>(&(22)).await.expect("send data");
            assert_eq!(
                controller
                    .recv::<Msg<22, u8>>()
                    .await
                    .expect("receive data"),
                22
            );
            controller
                .send::<Msg<23, u8>>(&(23))
                .await
                .expect("send data");
            assert_eq!(
                worker.recv::<Msg<23, u8>>().await.expect("receive data"),
                23
            );
            worker.send::<Msg<24, u8>>(&(24)).await.expect("send data");
            assert_eq!(
                controller
                    .recv::<Msg<24, u8>>()
                    .await
                    .expect("receive data"),
                24
            );
        }
        {
            controller
                .send::<Msg<81, u8>>(&(81))
                .await
                .expect("send data");
            assert_eq!(
                worker.recv::<Msg<81, u8>>().await.expect("receive data"),
                81
            );
            worker.send::<Msg<82, u8>>(&(82)).await.expect("send data");
            assert_eq!(
                controller
                    .recv::<Msg<82, u8>>()
                    .await
                    .expect("receive data"),
                82
            );
            controller
                .send::<Msg<83, u8>>(&(83))
                .await
                .expect("send data");
            assert_eq!(
                worker.recv::<Msg<83, u8>>().await.expect("receive data"),
                83
            );
            worker.send::<Msg<84, u8>>(&(84)).await.expect("send data");
            assert_eq!(
                controller
                    .recv::<Msg<84, u8>>()
                    .await
                    .expect("receive data"),
                84
            );
            controller
                .send::<Msg<85, u8>>(&(85))
                .await
                .expect("send data");
            assert_eq!(
                worker.recv::<Msg<85, u8>>().await.expect("receive data"),
                85
            );
            worker.send::<Msg<86, u8>>(&(86)).await.expect("send data");
            assert_eq!(
                controller
                    .recv::<Msg<86, u8>>()
                    .await
                    .expect("receive data"),
                86
            );
            controller
                .send::<Msg<87, u8>>(&(87))
                .await
                .expect("send data");
            assert_eq!(
                worker.recv::<Msg<87, u8>>().await.expect("receive data"),
                87
            );
            worker.send::<Msg<88, u8>>(&(88)).await.expect("send data");
            assert_eq!(
                controller
                    .recv::<Msg<88, u8>>()
                    .await
                    .expect("receive data"),
                88
            );
        }
        {
            controller
                .send::<Msg<89, u8>>(&(89))
                .await
                .expect("send data");
            assert_eq!(
                worker.recv::<Msg<89, u8>>().await.expect("receive data"),
                89
            );
            worker.send::<Msg<90, u8>>(&(90)).await.expect("send data");
            assert_eq!(
                controller
                    .recv::<Msg<90, u8>>()
                    .await
                    .expect("receive data"),
                90
            );
            controller
                .send::<Msg<91, u8>>(&(91))
                .await
                .expect("send data");
            assert_eq!(
                worker.recv::<Msg<91, u8>>().await.expect("receive data"),
                91
            );
            worker.send::<Msg<92, u8>>(&(92)).await.expect("send data");
            assert_eq!(
                controller
                    .recv::<Msg<92, u8>>()
                    .await
                    .expect("receive data"),
                92
            );
            controller
                .send::<Msg<93, u8>>(&(93))
                .await
                .expect("send data");
            assert_eq!(
                worker.recv::<Msg<93, u8>>().await.expect("receive data"),
                93
            );
            worker.send::<Msg<94, u8>>(&(94)).await.expect("send data");
            assert_eq!(
                controller
                    .recv::<Msg<94, u8>>()
                    .await
                    .expect("receive data"),
                94
            );
            controller
                .send::<Msg<95, u8>>(&(95))
                .await
                .expect("send data");
            assert_eq!(
                worker.recv::<Msg<95, u8>>().await.expect("receive data"),
                95
            );
            worker.send::<Msg<96, u8>>(&(96)).await.expect("send data");
            assert_eq!(
                controller
                    .recv::<Msg<96, u8>>()
                    .await
                    .expect("receive data"),
                96
            );
        }
        {
            controller
                .send::<Msg<97, u8>>(&(97))
                .await
                .expect("send data");
            assert_eq!(
                worker.recv::<Msg<97, u8>>().await.expect("receive data"),
                97
            );
            worker.send::<Msg<98, u8>>(&(98)).await.expect("send data");
            assert_eq!(
                controller
                    .recv::<Msg<98, u8>>()
                    .await
                    .expect("receive data"),
                98
            );
            controller
                .send::<Msg<99, u8>>(&(99))
                .await
                .expect("send data");
            assert_eq!(
                worker.recv::<Msg<99, u8>>().await.expect("receive data"),
                99
            );
            worker
                .send::<Msg<100, u8>>(&(100))
                .await
                .expect("send data");
            assert_eq!(
                controller
                    .recv::<Msg<100, u8>>()
                    .await
                    .expect("receive data"),
                100
            );
            controller
                .send::<Msg<101, u8>>(&(101))
                .await
                .expect("send data");
            assert_eq!(
                worker.recv::<Msg<101, u8>>().await.expect("receive data"),
                101
            );
            worker
                .send::<Msg<102, u8>>(&(102))
                .await
                .expect("send data");
            assert_eq!(
                controller
                    .recv::<Msg<102, u8>>()
                    .await
                    .expect("receive data"),
                102
            );
            controller
                .send::<Msg<103, u8>>(&(103))
                .await
                .expect("send data");
            assert_eq!(
                worker.recv::<Msg<103, u8>>().await.expect("receive data"),
                103
            );
            worker
                .send::<Msg<104, u8>>(&(104))
                .await
                .expect("send data");
            assert_eq!(
                controller
                    .recv::<Msg<104, u8>>()
                    .await
                    .expect("receive data"),
                104
            );
        }

        assert!(
            transport.queue_is_empty(),
            "huge choreography must drain every frame"
        );
    }

    let controller_program = fanout_program::program::<0>();
    let worker_program = fanout_program::program::<1>();
    {
        assert_eq!(
            fanout_program::ROUTE_SCOPE_COUNT,
            fanout_program::EXPECTED_WORKER_BRANCH_LABELS.len()
        );
        assert_eq!(
            fanout_program::ROUTE_SCOPE_COUNT,
            fanout_program::ACK_LABELS.len()
        );
        let mut slab = [0; 64 * 1024];
        let transport = TestTransport::new();
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, transport.clone())
            .expect("register rendezvous");
        let sid = SessionId::new(0x6000);
        let mut controller = rv
            .enter(sid, &controller_program)
            .expect("enter controller");
        let mut worker = rv.enter(sid, &worker_program).expect("enter worker");

        {
            controller
                .send::<Msg<1, u8>>(&(1))
                .await
                .expect("send data");
            assert_eq!(worker.recv::<Msg<1, u8>>().await.expect("receive data"), 1);
            worker.send::<Msg<2, u8>>(&(2)).await.expect("send data");
            assert_eq!(
                controller.recv::<Msg<2, u8>>().await.expect("receive data"),
                2
            );
            controller
                .send::<Msg<3, u8>>(&(3))
                .await
                .expect("send data");
            assert_eq!(worker.recv::<Msg<3, u8>>().await.expect("receive data"), 3);
            worker.send::<Msg<4, u8>>(&(4)).await.expect("send data");
            assert_eq!(
                controller.recv::<Msg<4, u8>>().await.expect("receive data"),
                4
            );
            controller
                .send::<Msg<5, u8>>(&(5))
                .await
                .expect("send data");
            assert_eq!(worker.recv::<Msg<5, u8>>().await.expect("receive data"), 5);
            worker.send::<Msg<6, u8>>(&(6)).await.expect("send data");
            assert_eq!(
                controller.recv::<Msg<6, u8>>().await.expect("receive data"),
                6
            );
            controller
                .send::<Msg<7, u8>>(&(7))
                .await
                .expect("send data");
            assert_eq!(worker.recv::<Msg<7, u8>>().await.expect("receive data"), 7);
            worker.send::<Msg<8, u8>>(&(8)).await.expect("send data");
            assert_eq!(
                controller.recv::<Msg<8, u8>>().await.expect("receive data"),
                8
            );
        }
        {
            {
                controller
                    .send::<Msg<81, u32>>(&(0))
                    .await
                    .expect("send data");
                assert_eq!(
                    {
                        let branch = worker.offer().await.expect("worker offer");
                        assert_eq!(branch.label(), 81);
                        branch.recv::<Msg<81, u32>>().await.expect("worker receive")
                    },
                    0
                );
                worker.send::<Msg<97, u8>>(&(97)).await.expect("send data");
                assert_eq!(
                    controller
                        .recv::<Msg<97, u8>>()
                        .await
                        .expect("receive data"),
                    97
                );
                controller
                    .send::<Msg<84, u32>>(&(0))
                    .await
                    .expect("send data");
                assert_eq!(
                    {
                        let branch = worker.offer().await.expect("worker offer");
                        assert_eq!(branch.label(), 84);
                        branch.recv::<Msg<84, u32>>().await.expect("worker receive")
                    },
                    0
                );
                worker.send::<Msg<98, u8>>(&(98)).await.expect("send data");
                assert_eq!(
                    controller
                        .recv::<Msg<98, u8>>()
                        .await
                        .expect("receive data"),
                    98
                );
            }
            {
                controller
                    .send::<Msg<85, u32>>(&(0))
                    .await
                    .expect("send data");
                assert_eq!(
                    {
                        let branch = worker.offer().await.expect("worker offer");
                        assert_eq!(branch.label(), 85);
                        branch.recv::<Msg<85, u32>>().await.expect("worker receive")
                    },
                    0
                );
                worker.send::<Msg<99, u8>>(&(99)).await.expect("send data");
                assert_eq!(
                    controller
                        .recv::<Msg<99, u8>>()
                        .await
                        .expect("receive data"),
                    99
                );
                controller
                    .send::<Msg<88, u32>>(&(0))
                    .await
                    .expect("send data");
                assert_eq!(
                    {
                        let branch = worker.offer().await.expect("worker offer");
                        assert_eq!(branch.label(), 88);
                        branch.recv::<Msg<88, u32>>().await.expect("worker receive")
                    },
                    0
                );
                worker
                    .send::<Msg<100, u8>>(&(100))
                    .await
                    .expect("send data");
                assert_eq!(
                    controller
                        .recv::<Msg<100, u8>>()
                        .await
                        .expect("receive data"),
                    100
                );
            }
            {
                controller
                    .send::<Msg<89, u32>>(&(0))
                    .await
                    .expect("send data");
                assert_eq!(
                    {
                        let branch = worker.offer().await.expect("worker offer");
                        assert_eq!(branch.label(), 89);
                        branch.recv::<Msg<89, u32>>().await.expect("worker receive")
                    },
                    0
                );
                worker
                    .send::<Msg<101, u8>>(&(101))
                    .await
                    .expect("send data");
                assert_eq!(
                    controller
                        .recv::<Msg<101, u8>>()
                        .await
                        .expect("receive data"),
                    101
                );
                controller
                    .send::<Msg<92, u32>>(&(0))
                    .await
                    .expect("send data");
                assert_eq!(
                    {
                        let branch = worker.offer().await.expect("worker offer");
                        assert_eq!(branch.label(), 92);
                        branch.recv::<Msg<92, u32>>().await.expect("worker receive")
                    },
                    0
                );
                worker
                    .send::<Msg<102, u8>>(&(102))
                    .await
                    .expect("send data");
                assert_eq!(
                    controller
                        .recv::<Msg<102, u8>>()
                        .await
                        .expect("receive data"),
                    102
                );
            }
            {
                controller
                    .send::<Msg<93, u32>>(&(0))
                    .await
                    .expect("send data");
                assert_eq!(
                    {
                        let branch = worker.offer().await.expect("worker offer");
                        assert_eq!(branch.label(), 93);
                        branch.recv::<Msg<93, u32>>().await.expect("worker receive")
                    },
                    0
                );
                worker
                    .send::<Msg<103, u8>>(&(103))
                    .await
                    .expect("send data");
                assert_eq!(
                    controller
                        .recv::<Msg<103, u8>>()
                        .await
                        .expect("receive data"),
                    103
                );
                controller
                    .send::<Msg<96, u32>>(&(0))
                    .await
                    .expect("send data");
                assert_eq!(
                    {
                        let branch = worker.offer().await.expect("worker offer");
                        assert_eq!(branch.label(), 96);
                        branch.recv::<Msg<96, u32>>().await.expect("worker receive")
                    },
                    0
                );
                worker
                    .send::<Msg<104, u8>>(&(104))
                    .await
                    .expect("send data");
                assert_eq!(
                    controller
                        .recv::<Msg<104, u8>>()
                        .await
                        .expect("receive data"),
                    104
                );
            }
        }
        {
            controller
                .send::<Msg<85, u8>>(&(85))
                .await
                .expect("send data");
            assert_eq!(
                worker.recv::<Msg<85, u8>>().await.expect("receive data"),
                85
            );
            worker.send::<Msg<86, u8>>(&(86)).await.expect("send data");
            assert_eq!(
                controller
                    .recv::<Msg<86, u8>>()
                    .await
                    .expect("receive data"),
                86
            );
            controller
                .send::<Msg<87, u8>>(&(87))
                .await
                .expect("send data");
            assert_eq!(
                worker.recv::<Msg<87, u8>>().await.expect("receive data"),
                87
            );
            worker.send::<Msg<88, u8>>(&(88)).await.expect("send data");
            assert_eq!(
                controller
                    .recv::<Msg<88, u8>>()
                    .await
                    .expect("receive data"),
                88
            );
            controller
                .send::<Msg<89, u8>>(&(89))
                .await
                .expect("send data");
            assert_eq!(
                worker.recv::<Msg<89, u8>>().await.expect("receive data"),
                89
            );
            worker.send::<Msg<90, u8>>(&(90)).await.expect("send data");
            assert_eq!(
                controller
                    .recv::<Msg<90, u8>>()
                    .await
                    .expect("receive data"),
                90
            );
            controller
                .send::<Msg<91, u8>>(&(91))
                .await
                .expect("send data");
            assert_eq!(
                worker.recv::<Msg<91, u8>>().await.expect("receive data"),
                91
            );
            worker.send::<Msg<92, u8>>(&(92)).await.expect("send data");
            assert_eq!(
                controller
                    .recv::<Msg<92, u8>>()
                    .await
                    .expect("receive data"),
                92
            );
        }

        assert!(
            transport.queue_is_empty(),
            "huge choreography must drain every frame"
        );
    }
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn over_256_ordered_inbound_occurrences_reuse_frame_labels_and_run() {
    let program = over_256_one_way_program!();
    let controller_program: RoleProgram<0> = project(&program);
    let worker_program: RoleProgram<1> = project(&program);

    {
        let mut slab = [0; 64 * 1024];
        let transport = TestTransport::new();
        let mut kit_storage = HugeKitStorage::uninit();
        let kit = kit_storage.init();
        let rv = kit
            .rendezvous(&mut slab, transport.clone())
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
    };
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

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn high_lane_route_runs_to_completion_on_actual_localside() {
    {
        let mut slab = [0; 64 * 1024];
        let transport = TestTransport::new();
        let mut kit_storage = HugeKitStorage::uninit();
        let kit = kit_storage.init();
        let rv = kit
            .rendezvous(&mut slab, transport.clone())
            .expect("register rendezvous");

        let mut controller = rv
            .enter(SessionId::new(0x6100), &high_lane_controller_program())
            .expect("enter controller-left");
        let mut worker = rv
            .enter(SessionId::new(0x6100), &high_lane_worker_program())
            .expect("enter worker-left");
        controller
            .send::<Msg<{ HIGH_LANE_LEFT_LABEL }, u8>>(&(7))
            .await
            .expect("send data");
        assert_eq!(
            {
                let branch = worker.offer().await.expect("worker offer");
                assert_eq!(branch.label(), { HIGH_LANE_LEFT_LABEL });
                branch
                    .recv::<Msg<{ HIGH_LANE_LEFT_LABEL }, u8>>()
                    .await
                    .expect("worker receive")
            },
            7,
            "lane 33 route payload must survive exact lane-set runtime selection"
        );
        worker
            .send::<Msg<{ HIGH_LANE_LEFT_REPLY_LABEL }, u8>>(&(17))
            .await
            .expect("send data");
        assert_eq!(
            controller
                .recv::<Msg<{ HIGH_LANE_LEFT_REPLY_LABEL }, u8>>()
                .await
                .expect("receive data"),
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
        controller
            .send::<Msg<{ HIGH_LANE_RIGHT_LABEL }, u8>>(&(9))
            .await
            .expect("send data");
        assert_eq!(
            {
                let branch = worker.offer().await.expect("worker offer");
                assert_eq!(branch.label(), { HIGH_LANE_RIGHT_LABEL });
                branch
                    .recv::<Msg<{ HIGH_LANE_RIGHT_LABEL }, u8>>()
                    .await
                    .expect("worker receive")
            },
            9,
            "lane 34 route payload must survive exact lane-set runtime selection"
        );
        worker
            .send::<Msg<{ HIGH_LANE_RIGHT_REPLY_LABEL }, u8>>(&(19))
            .await
            .expect("send data");
        assert_eq!(
            controller
                .recv::<Msg<{ HIGH_LANE_RIGHT_REPLY_LABEL }, u8>>()
                .await
                .expect("receive data"),
            19,
            "lane 34 reply payload must roundtrip through SessionKit localside"
        );

        assert!(
            transport.queue_is_empty(),
            "high-lane localside route test must drain every transport frame"
        );
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn lane_255_runs_to_completion_on_public_sessionkit_path() {
    {
        let mut slab = [0; 64 * 1024];
        let transport = TestTransport::new();
        let mut kit_storage = HugeKitStorage::uninit();
        let kit = kit_storage.init();
        let rv = kit
            .rendezvous(&mut slab, transport.clone())
            .expect("register rendezvous with the full wire lane domain");

        let mut controller = rv
            .enter(SessionId::new(0x6200), &edge_lane_controller_program())
            .expect("enter lane-255 controller");
        let mut worker = rv
            .enter(SessionId::new(0x6200), &edge_lane_worker_program())
            .expect("enter lane-255 worker");

        controller
            .send::<Msg<{ EDGE_LANE_LABEL }, u8>>(&(11))
            .await
            .expect("send data");
        assert_eq!(
            worker
                .recv::<Msg<{ EDGE_LANE_LABEL }, u8>>()
                .await
                .expect("receive data"),
            11,
            "lane 255 payload must be reachable through public SessionKit config"
        );
        worker
            .send::<Msg<{ EDGE_LANE_REPLY_LABEL }, u8>>(&(29))
            .await
            .expect("send data");
        assert_eq!(
            controller
                .recv::<Msg<{ EDGE_LANE_REPLY_LABEL }, u8>>()
                .await
                .expect("receive data"),
            29,
            "lane 255 reply must roundtrip through the real localside runtime"
        );

        assert!(
            transport.queue_is_empty(),
            "lane-255 localside test must drain every transport frame"
        );
    };
}
