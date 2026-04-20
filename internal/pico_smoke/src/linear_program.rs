use hibana::g::advanced::{RoleProgram, project};
use hibana::g::{self, Msg, Role};
use super::localside;

pub const ROUTE_SCOPE_COUNT: usize = 0;
pub const EXPECTED_WORKER_BRANCH_LABELS: [u8; ROUTE_SCOPE_COUNT] = [];
pub const ACK_LABELS: [u8; ROUTE_SCOPE_COUNT] = [];

pub fn controller_program() -> RoleProgram<0> {
    let segment_a = || {
        let program = g::send::<Role<0>, Role<1>, Msg<1, u8>, 0>();
        let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<2, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<3, u8>, 0>());
        let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<4, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<5, u8>, 0>());
        let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<6, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<7, u8>, 0>());
        g::seq(program, g::send::<Role<1>, Role<0>, Msg<8, u8>, 0>())
    };

    let segment_b = || {
        let program = g::send::<Role<0>, Role<1>, Msg<9, u8>, 0>();
        let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<10, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<11, u8>, 0>());
        let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<12, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<13, u8>, 0>());
        let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<14, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<15, u8>, 0>());
        g::seq(program, g::send::<Role<1>, Role<0>, Msg<16, u8>, 0>())
    };

    let segment_c = || {
        let program = g::send::<Role<0>, Role<1>, Msg<17, u8>, 0>();
        let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<18, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<19, u8>, 0>());
        let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<20, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<21, u8>, 0>());
        let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<22, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<23, u8>, 0>());
        g::seq(program, g::send::<Role<1>, Role<0>, Msg<24, u8>, 0>())
    };

    let segment_d = || {
        let program = g::send::<Role<0>, Role<1>, Msg<81, u8>, 0>();
        let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<82, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<83, u8>, 0>());
        let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<84, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<85, u8>, 0>());
        let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<86, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<87, u8>, 0>());
        g::seq(program, g::send::<Role<1>, Role<0>, Msg<88, u8>, 0>())
    };

    let segment_e = || {
        let program = g::send::<Role<0>, Role<1>, Msg<89, u8>, 0>();
        let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<90, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<91, u8>, 0>());
        let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<92, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<93, u8>, 0>());
        let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<94, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<95, u8>, 0>());
        g::seq(program, g::send::<Role<1>, Role<0>, Msg<96, u8>, 0>())
    };

    let segment_f = || {
        let program = g::send::<Role<0>, Role<1>, Msg<97, u8>, 0>();
        let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<98, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<99, u8>, 0>());
        let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<100, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<101, u8>, 0>());
        let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<102, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<103, u8>, 0>());
        g::seq(program, g::send::<Role<1>, Role<0>, Msg<104, u8>, 0>())
    };

    let program = g::seq(
        segment_a(),
        g::seq(
            segment_b(),
            g::seq(
                segment_c(),
                g::seq(segment_d(), g::seq(segment_e(), segment_f())),
            ),
        ),
    );

    let projected: RoleProgram<0> = project(&program);
    projected
}

pub fn worker_program() -> RoleProgram<1> {
    let segment_a = || {
        let program = g::send::<Role<0>, Role<1>, Msg<1, u8>, 0>();
        let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<2, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<3, u8>, 0>());
        let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<4, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<5, u8>, 0>());
        let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<6, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<7, u8>, 0>());
        g::seq(program, g::send::<Role<1>, Role<0>, Msg<8, u8>, 0>())
    };

    let segment_b = || {
        let program = g::send::<Role<0>, Role<1>, Msg<9, u8>, 0>();
        let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<10, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<11, u8>, 0>());
        let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<12, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<13, u8>, 0>());
        let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<14, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<15, u8>, 0>());
        g::seq(program, g::send::<Role<1>, Role<0>, Msg<16, u8>, 0>())
    };

    let segment_c = || {
        let program = g::send::<Role<0>, Role<1>, Msg<17, u8>, 0>();
        let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<18, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<19, u8>, 0>());
        let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<20, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<21, u8>, 0>());
        let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<22, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<23, u8>, 0>());
        g::seq(program, g::send::<Role<1>, Role<0>, Msg<24, u8>, 0>())
    };

    let segment_d = || {
        let program = g::send::<Role<0>, Role<1>, Msg<81, u8>, 0>();
        let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<82, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<83, u8>, 0>());
        let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<84, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<85, u8>, 0>());
        let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<86, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<87, u8>, 0>());
        g::seq(program, g::send::<Role<1>, Role<0>, Msg<88, u8>, 0>())
    };

    let segment_e = || {
        let program = g::send::<Role<0>, Role<1>, Msg<89, u8>, 0>();
        let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<90, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<91, u8>, 0>());
        let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<92, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<93, u8>, 0>());
        let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<94, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<95, u8>, 0>());
        g::seq(program, g::send::<Role<1>, Role<0>, Msg<96, u8>, 0>())
    };

    let segment_f = || {
        let program = g::send::<Role<0>, Role<1>, Msg<97, u8>, 0>();
        let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<98, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<99, u8>, 0>());
        let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<100, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<101, u8>, 0>());
        let program = g::seq(program, g::send::<Role<1>, Role<0>, Msg<102, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<103, u8>, 0>());
        g::seq(program, g::send::<Role<1>, Role<0>, Msg<104, u8>, 0>())
    };

    let program = g::seq(
        segment_a(),
        g::seq(
            segment_b(),
            g::seq(
                segment_c(),
                g::seq(segment_d(), g::seq(segment_e(), segment_f())),
            ),
        ),
    );

    let projected: RoleProgram<1> = project(&program);
    projected
}

pub fn run(
    controller: &mut localside::ControllerEndpoint<'_>,
    worker: &mut localside::WorkerEndpoint<'_>,
) {
    run_segment_a(controller, worker);
    run_segment_b(controller, worker);
    run_segment_c(controller, worker);
    run_segment_d(controller, worker);
    run_segment_e(controller, worker);
    run_segment_f(controller, worker);
}

fn run_segment_a(
    controller: &mut localside::ControllerEndpoint<'_>,
    worker: &mut localside::WorkerEndpoint<'_>,
) {
    localside::controller_send_u8::<1>(controller, 1);
    assert_eq!(localside::worker_recv_u8::<1>(worker), 1);
    localside::worker_send_u8::<2>(worker, 2);
    assert_eq!(localside::controller_recv_u8::<2>(controller), 2);
    localside::controller_send_u8::<3>(controller, 3);
    assert_eq!(localside::worker_recv_u8::<3>(worker), 3);
    localside::worker_send_u8::<4>(worker, 4);
    assert_eq!(localside::controller_recv_u8::<4>(controller), 4);
    localside::controller_send_u8::<5>(controller, 5);
    assert_eq!(localside::worker_recv_u8::<5>(worker), 5);
    localside::worker_send_u8::<6>(worker, 6);
    assert_eq!(localside::controller_recv_u8::<6>(controller), 6);
    localside::controller_send_u8::<7>(controller, 7);
    assert_eq!(localside::worker_recv_u8::<7>(worker), 7);
    localside::worker_send_u8::<8>(worker, 8);
    assert_eq!(localside::controller_recv_u8::<8>(controller), 8);
}

fn run_segment_b(
    controller: &mut localside::ControllerEndpoint<'_>,
    worker: &mut localside::WorkerEndpoint<'_>,
) {
    localside::controller_send_u8::<9>(controller, 9);
    assert_eq!(localside::worker_recv_u8::<9>(worker), 9);
    localside::worker_send_u8::<10>(worker, 10);
    assert_eq!(localside::controller_recv_u8::<10>(controller), 10);
    localside::controller_send_u8::<11>(controller, 11);
    assert_eq!(localside::worker_recv_u8::<11>(worker), 11);
    localside::worker_send_u8::<12>(worker, 12);
    assert_eq!(localside::controller_recv_u8::<12>(controller), 12);
    localside::controller_send_u8::<13>(controller, 13);
    assert_eq!(localside::worker_recv_u8::<13>(worker), 13);
    localside::worker_send_u8::<14>(worker, 14);
    assert_eq!(localside::controller_recv_u8::<14>(controller), 14);
    localside::controller_send_u8::<15>(controller, 15);
    assert_eq!(localside::worker_recv_u8::<15>(worker), 15);
    localside::worker_send_u8::<16>(worker, 16);
    assert_eq!(localside::controller_recv_u8::<16>(controller), 16);
}

fn run_segment_c(
    controller: &mut localside::ControllerEndpoint<'_>,
    worker: &mut localside::WorkerEndpoint<'_>,
) {
    localside::controller_send_u8::<17>(controller, 17);
    assert_eq!(localside::worker_recv_u8::<17>(worker), 17);
    localside::worker_send_u8::<18>(worker, 18);
    assert_eq!(localside::controller_recv_u8::<18>(controller), 18);
    localside::controller_send_u8::<19>(controller, 19);
    assert_eq!(localside::worker_recv_u8::<19>(worker), 19);
    localside::worker_send_u8::<20>(worker, 20);
    assert_eq!(localside::controller_recv_u8::<20>(controller), 20);
    localside::controller_send_u8::<21>(controller, 21);
    assert_eq!(localside::worker_recv_u8::<21>(worker), 21);
    localside::worker_send_u8::<22>(worker, 22);
    assert_eq!(localside::controller_recv_u8::<22>(controller), 22);
    localside::controller_send_u8::<23>(controller, 23);
    assert_eq!(localside::worker_recv_u8::<23>(worker), 23);
    localside::worker_send_u8::<24>(worker, 24);
    assert_eq!(localside::controller_recv_u8::<24>(controller), 24);
}

fn run_segment_d(
    controller: &mut localside::ControllerEndpoint<'_>,
    worker: &mut localside::WorkerEndpoint<'_>,
) {
    localside::controller_send_u8::<81>(controller, 81);
    assert_eq!(localside::worker_recv_u8::<81>(worker), 81);
    localside::worker_send_u8::<82>(worker, 82);
    assert_eq!(localside::controller_recv_u8::<82>(controller), 82);
    localside::controller_send_u8::<83>(controller, 83);
    assert_eq!(localside::worker_recv_u8::<83>(worker), 83);
    localside::worker_send_u8::<84>(worker, 84);
    assert_eq!(localside::controller_recv_u8::<84>(controller), 84);
    localside::controller_send_u8::<85>(controller, 85);
    assert_eq!(localside::worker_recv_u8::<85>(worker), 85);
    localside::worker_send_u8::<86>(worker, 86);
    assert_eq!(localside::controller_recv_u8::<86>(controller), 86);
    localside::controller_send_u8::<87>(controller, 87);
    assert_eq!(localside::worker_recv_u8::<87>(worker), 87);
    localside::worker_send_u8::<88>(worker, 88);
    assert_eq!(localside::controller_recv_u8::<88>(controller), 88);
}

fn run_segment_e(
    controller: &mut localside::ControllerEndpoint<'_>,
    worker: &mut localside::WorkerEndpoint<'_>,
) {
    localside::controller_send_u8::<89>(controller, 89);
    assert_eq!(localside::worker_recv_u8::<89>(worker), 89);
    localside::worker_send_u8::<90>(worker, 90);
    assert_eq!(localside::controller_recv_u8::<90>(controller), 90);
    localside::controller_send_u8::<91>(controller, 91);
    assert_eq!(localside::worker_recv_u8::<91>(worker), 91);
    localside::worker_send_u8::<92>(worker, 92);
    assert_eq!(localside::controller_recv_u8::<92>(controller), 92);
    localside::controller_send_u8::<93>(controller, 93);
    assert_eq!(localside::worker_recv_u8::<93>(worker), 93);
    localside::worker_send_u8::<94>(worker, 94);
    assert_eq!(localside::controller_recv_u8::<94>(controller), 94);
    localside::controller_send_u8::<95>(controller, 95);
    assert_eq!(localside::worker_recv_u8::<95>(worker), 95);
    localside::worker_send_u8::<96>(worker, 96);
    assert_eq!(localside::controller_recv_u8::<96>(controller), 96);
}

fn run_segment_f(
    controller: &mut localside::ControllerEndpoint<'_>,
    worker: &mut localside::WorkerEndpoint<'_>,
) {
    localside::controller_send_u8::<97>(controller, 97);
    assert_eq!(localside::worker_recv_u8::<97>(worker), 97);
    localside::worker_send_u8::<98>(worker, 98);
    assert_eq!(localside::controller_recv_u8::<98>(controller), 98);
    localside::controller_send_u8::<99>(controller, 99);
    assert_eq!(localside::worker_recv_u8::<99>(worker), 99);
    localside::worker_send_u8::<100>(worker, 100);
    assert_eq!(localside::controller_recv_u8::<100>(controller), 100);
    localside::controller_send_u8::<101>(controller, 101);
    assert_eq!(localside::worker_recv_u8::<101>(worker), 101);
    localside::worker_send_u8::<102>(worker, 102);
    assert_eq!(localside::controller_recv_u8::<102>(controller), 102);
    localside::controller_send_u8::<103>(controller, 103);
    assert_eq!(localside::worker_recv_u8::<103>(worker), 103);
    localside::worker_send_u8::<104>(worker, 104);
    assert_eq!(localside::controller_recv_u8::<104>(controller), 104);
}
