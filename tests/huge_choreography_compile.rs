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

use hibana::g::advanced::RoleProgram;

fn drive<F: core::future::Future>(future: F) -> F::Output {
    futures::executor::block_on(future)
}

fn retain_pico_smoke_fixture_symbols() {
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
    let _ = localside::worker_offer_decode_u8::<0> as fn(&mut localside::WorkerEndpoint<'_>) -> u8;
}

#[test]
fn pico_smoke_fixture_symbols_are_reachable() {
    retain_pico_smoke_fixture_symbols();
}

#[test]
fn huge_programs_stay_on_direct_program_values() {
    retain_pico_smoke_fixture_symbols();
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
    retain_pico_smoke_fixture_symbols();
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
