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

use hibana::{
    g,
    g::advanced::{RoleProgram, project},
    substrate::{
        cap::advanced::MintConfig,
        runtime::{CounterClock, DefaultLabelUniverse},
    },
};

static ROUTE_HEAVY_PROGRAM: g::Program<huge_program::ProgramSteps> = huge_program::PROGRAM;
static LINEAR_HEAVY_PROGRAM: g::Program<linear_program::ProgramSteps> = linear_program::PROGRAM;
static FANOUT_HEAVY_PROGRAM: g::Program<fanout_program::ProgramSteps> = fanout_program::PROGRAM;

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
    let _ = huge_program::run::<common::TestTransport, DefaultLabelUniverse, CounterClock, 2>
        as fn(
            &mut localside::ControllerEndpoint<
                '_,
                common::TestTransport,
                DefaultLabelUniverse,
                CounterClock,
                2,
            >,
            &mut localside::WorkerEndpoint<
                '_,
                common::TestTransport,
                DefaultLabelUniverse,
                CounterClock,
                2,
            >,
        );
    let _ = linear_program::run::<common::TestTransport, DefaultLabelUniverse, CounterClock, 2>
        as fn(
            &mut localside::ControllerEndpoint<
                '_,
                common::TestTransport,
                DefaultLabelUniverse,
                CounterClock,
                2,
            >,
            &mut localside::WorkerEndpoint<
                '_,
                common::TestTransport,
                DefaultLabelUniverse,
                CounterClock,
                2,
            >,
        );
    let _ = fanout_program::run::<common::TestTransport, DefaultLabelUniverse, CounterClock, 2>
        as fn(
            &mut localside::ControllerEndpoint<
                '_,
                common::TestTransport,
                DefaultLabelUniverse,
                CounterClock,
                2,
            >,
            &mut localside::WorkerEndpoint<
                '_,
                common::TestTransport,
                DefaultLabelUniverse,
                CounterClock,
                2,
            >,
        );
    let _ = localside::worker_offer_decode_u8::<
        0,
        common::TestTransport,
        DefaultLabelUniverse,
        CounterClock,
        2,
    >
        as fn(
            &mut localside::WorkerEndpoint<
                '_,
                common::TestTransport,
                DefaultLabelUniverse,
                CounterClock,
                2,
            >,
        ) -> u8;
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
    let route_heavy_controller: RoleProgram<'_, 0, MintConfig> = project(&ROUTE_HEAVY_PROGRAM);
    let route_heavy_worker: RoleProgram<'_, 1, MintConfig> = project(&ROUTE_HEAVY_PROGRAM);
    let linear_heavy_controller: RoleProgram<'_, 0, MintConfig> = project(&LINEAR_HEAVY_PROGRAM);
    let linear_heavy_worker: RoleProgram<'_, 1, MintConfig> = project(&LINEAR_HEAVY_PROGRAM);
    let fanout_heavy_controller: RoleProgram<'_, 0, MintConfig> = project(&FANOUT_HEAVY_PROGRAM);
    let fanout_heavy_worker: RoleProgram<'_, 1, MintConfig> = project(&FANOUT_HEAVY_PROGRAM);

    let _ = (
        &route_heavy_controller,
        &route_heavy_worker,
        &linear_heavy_controller,
        &linear_heavy_worker,
        &fanout_heavy_controller,
        &fanout_heavy_worker,
    );
}
