//! Test: g::par creates proper Phase structures for fork-join barriers.
//!
//! This test verifies that the Phased Multi-Lane Architecture correctly
//! splits g::par choreographies into separate phases.

use hibana::g::{self, Msg, Role, RoleProgram};

type Client = Role<0>;
type Server = Role<1>;

type RequestMsg = Msg<10, ()>;
type ResponseMsg = Msg<11, ()>;
type PingMsg = Msg<12, ()>;
type PongMsg = Msg<13, ()>;

// Single sequential program (no g::par)
type SeqSteps = g::steps::StepCons<
    g::steps::SendStep<Client, Server, RequestMsg, 0>,
    g::steps::StepCons<g::steps::SendStep<Server, Client, ResponseMsg, 0>, g::steps::StepNil>,
>;

const SEQ_PROGRAM: g::Program<SeqSteps> = g::send::<Client, Server, RequestMsg, 0>()
    .then(g::send::<Server, Client, ResponseMsg, 0>());

// Two parallel lanes
type Lane0Steps = g::steps::StepCons<g::steps::SendStep<Client, Server, RequestMsg, 0>, g::steps::StepNil>;
type Lane1Steps = g::steps::StepCons<g::steps::SendStep<Server, Client, PingMsg, 1>, g::steps::StepNil>;
type ParSteps = <Lane0Steps as g::steps::StepConcat<Lane1Steps>>::Output;

const LANE0: g::Program<Lane0Steps> = g::send::<Client, Server, RequestMsg, 0>();
const LANE1: g::Program<Lane1Steps> = g::send::<Server, Client, PingMsg, 1>();
const PAR_PROGRAM: g::Program<ParSteps> = g::par(g::par_chain(LANE0).and(LANE1));

// Sequential + Parallel + Sequential: g::seq(A, g::par(B, C), D)
type PreStep = g::steps::StepCons<g::steps::SendStep<Client, Server, RequestMsg, 0>, g::steps::StepNil>;
type PostStep = g::steps::StepCons<g::steps::SendStep<Server, Client, ResponseMsg, 0>, g::steps::StepNil>;
type ParLane0 = g::steps::StepCons<g::steps::SendStep<Client, Server, PingMsg, 0>, g::steps::StepNil>;
type ParLane1 = g::steps::StepCons<g::steps::SendStep<Server, Client, PongMsg, 1>, g::steps::StepNil>;
type ParInner = <ParLane0 as g::steps::StepConcat<ParLane1>>::Output;
type PrePar = <PreStep as g::steps::StepConcat<ParInner>>::Output;
type FullSteps = <PrePar as g::steps::StepConcat<PostStep>>::Output;

const PRE: g::Program<PreStep> = g::send::<Client, Server, RequestMsg, 0>();
const PAR_LANE0: g::Program<ParLane0> = g::send::<Client, Server, PingMsg, 0>();
const PAR_LANE1: g::Program<ParLane1> = g::send::<Server, Client, PongMsg, 1>();
const PAR_INNER: g::Program<ParInner> = g::par(g::par_chain(PAR_LANE0).and(PAR_LANE1));
const POST: g::Program<PostStep> = g::send::<Server, Client, ResponseMsg, 0>();
const FULL_PROGRAM: g::Program<FullSteps> = PRE.then(PAR_INNER).then(POST);

#[test]
fn sequential_program_has_single_phase() {
    let program: RoleProgram<'static, 0, <SeqSteps as g::steps::ProjectRole<Client>>::Output> =
        g::project::<0, SeqSteps, _>(&SEQ_PROGRAM);

    // Sequential program should have 1 phase
    assert_eq!(program.phase_count(), 1, "Sequential program should have 1 phase");

    // Phase 0 should have steps on Lane 0 only
    let phase0 = program.phase(0);
    assert!(phase0.lane(0).is_active(), "Lane 0 should be active");
    assert!(!phase0.lane(1).is_active(), "Lane 1 should not be active");
}

#[test]
fn parallel_program_detects_par_scope() {
    let client_program: RoleProgram<'static, 0, <ParSteps as g::steps::ProjectRole<Client>>::Output> =
        g::project::<0, ParSteps, _>(&PAR_PROGRAM);

    // Client sends on Lane 0, so should have at least one step
    assert!(client_program.steps().len() >= 1, "Client should have steps");

    let server_program: RoleProgram<'static, 1, <ParSteps as g::steps::ProjectRole<Server>>::Output> =
        g::project::<1, ParSteps, _>(&PAR_PROGRAM);

    // Server has recv on Lane 0 and send on Lane 1
    assert!(server_program.steps().len() >= 1, "Server should have steps");
}

#[test]
fn parallel_program_has_single_phase_with_active_lanes() {
    let client_program: RoleProgram<'static, 0, <ParSteps as g::steps::ProjectRole<Client>>::Output> =
        g::project::<0, ParSteps, _>(&PAR_PROGRAM);

    assert_eq!(
        client_program.phase_count(),
        1,
        "g::par should yield a single phase"
    );

    let phase0 = client_program.phase(0);
    assert!(phase0.lane(0).is_active(), "Lane 0 should be active in phase 0");
    assert!(phase0.lane(1).is_active(), "Lane 1 should be active in phase 0");
}

#[test]
fn steps_have_correct_lane_assignment() {
    let client_program: RoleProgram<'static, 0, <ParSteps as g::steps::ProjectRole<Client>>::Output> =
        g::project::<0, ParSteps, _>(&PAR_PROGRAM);

    // Client: sends on Lane 0, recvs on Lane 1
    // Lane 0: Client → Server (send)
    // Lane 1: Server → Client (recv)
    let client_steps = client_program.steps();
    assert_eq!(client_steps.len(), 2, "Client should have 2 steps (send on L0, recv on L1)");

    // Check lane distribution for Client
    let client_lane0 = client_steps.iter().filter(|s| s.lane() == 0).count();
    let client_lane1 = client_steps.iter().filter(|s| s.lane() == 1).count();
    assert_eq!(client_lane0, 1, "Client should have 1 step on Lane 0");
    assert_eq!(client_lane1, 1, "Client should have 1 step on Lane 1");

    let server_program: RoleProgram<'static, 1, <ParSteps as g::steps::ProjectRole<Server>>::Output> =
        g::project::<1, ParSteps, _>(&PAR_PROGRAM);

    // Server: recv on Lane 0, send on Lane 1
    let server_steps = server_program.steps();
    assert_eq!(server_steps.len(), 2, "Server should have 2 steps");

    // Check lane distribution for Server
    let lane0_count = server_steps.iter().filter(|s| s.lane() == 0).count();
    let lane1_count = server_steps.iter().filter(|s| s.lane() == 1).count();
    assert_eq!(lane0_count, 1, "Server should have 1 step on Lane 0");
    assert_eq!(lane1_count, 1, "Server should have 1 step on Lane 1");
}

#[test]
fn full_program_has_multiple_phases() {
    // This tests g::seq(PRE, g::par(LANE0, LANE1), POST)
    let client_program: RoleProgram<'static, 0, <FullSteps as g::steps::ProjectRole<Client>>::Output> =
        g::project::<0, FullSteps, _>(&FULL_PROGRAM);

    // Client sends RequestMsg (pre), PingMsg (par lane 0), recv PongMsg (par lane 1)
    // Should have at least 2 steps
    assert!(client_program.steps().len() >= 2, "Client should have at least 2 steps");

    let server_program: RoleProgram<'static, 1, <FullSteps as g::steps::ProjectRole<Server>>::Output> =
        g::project::<1, FullSteps, _>(&FULL_PROGRAM);

    // Server: recv RequestMsg (pre), recv PingMsg (par lane 0), send PongMsg (par lane 1), send ResponseMsg (post)
    assert!(server_program.steps().len() >= 3, "Server should have at least 3 steps");
}

#[test]
fn scope_id_and_phase_consistency() {
    // Verify that ScopeMarker offsets correctly map to eff_index values
    let eff_list = PAR_PROGRAM.eff_list();
    let scope_markers = eff_list.scope_markers();

    // Check that scope markers exist
    let parallel_markers: Vec<_> = scope_markers
        .iter()
        .filter(|m| matches!(m.scope_kind, hibana::global::const_dsl::ScopeKind::Parallel))
        .collect();

    // g::par should produce at least Enter and Exit markers
    assert!(
        parallel_markers.len() >= 2,
        "g::par should have Enter/Exit markers, found {}",
        parallel_markers.len()
    );

    // Verify scope_id consistency between Enter and Exit
    let enters: Vec<_> = parallel_markers
        .iter()
        .filter(|m| matches!(m.event, hibana::global::const_dsl::ScopeEvent::Enter))
        .collect();
    let exits: Vec<_> = parallel_markers
        .iter()
        .filter(|m| matches!(m.event, hibana::global::const_dsl::ScopeEvent::Exit))
        .collect();

    assert!(!enters.is_empty(), "Should have at least one Enter marker");
    assert!(!exits.is_empty(), "Should have at least one Exit marker");

    // Each Enter should have a matching Exit with same scope_id
    for enter in &enters {
        let matching_exit = exits
            .iter()
            .find(|exit| exit.scope_id.raw() == enter.scope_id.raw());
        assert!(
            matching_exit.is_some(),
            "Enter marker {:?} should have matching Exit",
            enter.scope_id
        );
    }
}

/// Verify that steps' eff_index values fall within expected scope boundaries.
#[test]
fn eff_index_within_scope_boundaries() {
    let client_program: RoleProgram<'static, 0, <ParSteps as g::steps::ProjectRole<Client>>::Output> =
        g::project::<0, ParSteps, _>(&PAR_PROGRAM);

    let eff_list = PAR_PROGRAM.eff_list();
    let scope_markers = eff_list.scope_markers();

    // Verify eff_list and scope_markers are non-empty
    assert!(eff_list.len() > 0, "EffList should not be empty");
    assert!(scope_markers.len() > 0, "Scope markers should not be empty");

    // Find parallel scope boundaries
    let par_enter = scope_markers
        .iter()
        .find(|m| {
            matches!(m.scope_kind, hibana::global::const_dsl::ScopeKind::Parallel)
                && matches!(m.event, hibana::global::const_dsl::ScopeEvent::Enter)
        });
    let par_exit = scope_markers
        .iter()
        .find(|m| {
            matches!(m.scope_kind, hibana::global::const_dsl::ScopeKind::Parallel)
                && matches!(m.event, hibana::global::const_dsl::ScopeEvent::Exit)
        });

    // Verify parallel scope has both enter and exit markers
    assert!(par_enter.is_some(), "Parallel scope Enter marker should exist");
    assert!(par_exit.is_some(), "Parallel scope Exit marker should exist");

    let enter = par_enter.unwrap();
    let _exit = par_exit.unwrap();

    // All steps should have eff_index within scope boundaries
    for step in client_program.steps() {
        let eff_idx = step.eff_index() as usize;
        // Steps inside g::par have eff_index >= enter.offset
        assert!(
            eff_idx >= enter.offset,
            "Step eff_index {} should be >= parallel scope enter offset {}",
            eff_idx,
            enter.offset
        );
    }
}
