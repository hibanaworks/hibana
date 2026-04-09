#![cfg(feature = "std")]

#[path = "../internal/pico_smoke/src/fanout_program.rs"]
mod fanout_program;
#[path = "../internal/pico_smoke/src/huge_program.rs"]
mod huge_program;
#[path = "../internal/pico_smoke/src/linear_program.rs"]
mod linear_program;
#[path = "../internal/pico_smoke/src/route_control_kinds.rs"]
mod route_control_kinds;
#[path = "../internal/pico_smoke/src/scenario.rs"]
mod scenario;

use core::mem::size_of;

use hibana::{
    g,
    g::advanced::{RoleProgram, project},
    substrate::cap::advanced::MintConfig,
};

const ROUTE_HEAVY_PROGRAM: g::Program<huge_program::ProgramSteps> =
    g::freeze(&huge_program::PROGRAM);
const LINEAR_HEAVY_PROGRAM: g::Program<linear_program::ProgramSteps> =
    g::freeze(&linear_program::PROGRAM);
const FANOUT_HEAVY_PROGRAM: g::Program<fanout_program::ProgramSteps> =
    g::freeze(&fanout_program::PROGRAM);

fn retain_pico_smoke_fixture_symbols() {
    let _ = fanout_program::ROUTE_SCOPE_COUNT;
    let _ = fanout_program::EXPECTED_WORKER_BRANCH_LABELS;
    let _ = fanout_program::ACK_LABELS;
    let _ = fanout_program::run::<scenario::FixtureHarness>;
    let _ = huge_program::ROUTE_SCOPE_COUNT;
    let _ = huge_program::EXPECTED_WORKER_BRANCH_LABELS;
    let _ = huge_program::ACK_LABELS;
    let _ = huge_program::run::<scenario::FixtureHarness>;
    let _ = linear_program::ROUTE_SCOPE_COUNT;
    let _ = linear_program::EXPECTED_WORKER_BRANCH_LABELS;
    let _ = linear_program::ACK_LABELS;
    let _ = linear_program::run::<scenario::FixtureHarness>;
}

#[test]
fn pico_smoke_fixture_symbols_are_reachable() {
    retain_pico_smoke_fixture_symbols();
}

#[test]
fn huge_programs_freeze_into_thin_tokens() {
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

    assert!(
        size_of::<g::Program<huge_program::ProgramSteps>>() <= 2 * size_of::<usize>(),
        "route-heavy frozen Program token must stay thin even for huge choreography sources"
    );
    assert!(
        size_of::<g::Program<linear_program::ProgramSteps>>() <= 2 * size_of::<usize>(),
        "linear-heavy frozen Program token must stay thin even for huge choreography sources"
    );
    assert!(
        size_of::<g::Program<fanout_program::ProgramSteps>>() <= 2 * size_of::<usize>(),
        "fanout-heavy frozen Program token must stay thin even for huge choreography sources"
    );
}

#[test]
fn huge_program_shape_matrix_projects_both_roles() {
    retain_pico_smoke_fixture_symbols();
    let route_heavy_controller: RoleProgram<'_, 0, _, MintConfig> = project(&ROUTE_HEAVY_PROGRAM);
    let route_heavy_worker: RoleProgram<'_, 1, _, MintConfig> = project(&ROUTE_HEAVY_PROGRAM);
    let linear_heavy_controller: RoleProgram<'_, 0, _, MintConfig> = project(&LINEAR_HEAVY_PROGRAM);
    let linear_heavy_worker: RoleProgram<'_, 1, _, MintConfig> = project(&LINEAR_HEAVY_PROGRAM);
    let fanout_heavy_controller: RoleProgram<'_, 0, _, MintConfig> = project(&FANOUT_HEAVY_PROGRAM);
    let fanout_heavy_worker: RoleProgram<'_, 1, _, MintConfig> = project(&FANOUT_HEAVY_PROGRAM);

    let _ = (
        &route_heavy_controller,
        &route_heavy_worker,
        &linear_heavy_controller,
        &linear_heavy_worker,
        &fanout_heavy_controller,
        &fanout_heavy_worker,
    );
}
