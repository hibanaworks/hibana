//! Test: g::par with g::route on different lanes.
//!
//! This test verifies that the Lane-Local Route Stacks correctly handle
//! independent route scopes within parallel composition.

use hibana::control::cap::GenericCapToken;
use hibana::control::cap::resource_kinds::{LoopBreakKind, LoopContinueKind};
use hibana::g::{self, Msg, Role, RoleProgram};
use hibana::global::const_dsl::{DynamicMeta, HandlePlan};
use hibana::runtime::consts::{LABEL_LOOP_BREAK, LABEL_LOOP_CONTINUE};

type Client = Role<0>;
type Server = Role<1>;
type Controller = Role<2>;

// Lane 0 messages
type RequestMsg = Msg<10, ()>;
type ResponseMsg = Msg<11, ()>;

// Lane 1 messages (Controller's route)
type ContinueMsg = Msg<
    { LABEL_LOOP_CONTINUE },
    GenericCapToken<LoopContinueKind>,
    g::CanonicalControl<LoopContinueKind>,
>;
type BreakMsg =
    Msg<{ LABEL_LOOP_BREAK }, GenericCapToken<LoopBreakKind>, g::CanonicalControl<LoopBreakKind>>;
type TickMsg = Msg<20, ()>;

// Lane 0: Client → Server → Client (simple request-response)
type Lane0Steps = g::steps::StepCons<
    g::steps::SendStep<Client, Server, RequestMsg, 0>,
    g::steps::StepCons<g::steps::SendStep<Server, Client, ResponseMsg, 0>, g::steps::StepNil>,
>;

// Lane 1: Controller loop with TickMsg
// Note: LoopContinueSteps/LoopBreakSteps use LANE=0 for the control message
type TickStep =
    g::steps::StepCons<g::steps::SendStep<Controller, Server, TickMsg, 1>, g::steps::StepNil>;
type ContinueArm = g::steps::StepCons<
    g::steps::SendStep<Controller, Controller, ContinueMsg, 1>,
    TickStep,
>;
type BreakArm = g::steps::StepCons<
    g::steps::SendStep<Controller, Controller, BreakMsg, 1>,
    g::steps::StepNil,
>;
type Lane1Steps = <ContinueArm as g::steps::StepConcat<BreakArm>>::Output;

// Combined parallel steps
type ParSteps = <Lane0Steps as g::steps::StepConcat<Lane1Steps>>::Output;

// Constants
const LANE0: g::Program<Lane0Steps> = g::send::<Client, Server, RequestMsg, 0>()
    .then(g::send::<Server, Client, ResponseMsg, 0>());

const TICK: g::Program<TickStep> = g::send::<Controller, Server, TickMsg, 1>();
const CONTINUE_ARM: g::Program<ContinueArm> = g::with_control_plan(
    g::send::<Controller, Controller, ContinueMsg, 1>(),
    HandlePlan::dynamic(100, DynamicMeta::new()),
)
.then(TICK);
const BREAK_ARM: g::Program<BreakArm> = g::with_control_plan(
    g::send::<Controller, Controller, BreakMsg, 1>(),
    HandlePlan::dynamic(100, DynamicMeta::new()),
);
const LANE1: g::Program<Lane1Steps> =
    g::route::<2, _>(g::route_chain::<2, ContinueArm>(CONTINUE_ARM).and::<BreakArm>(BREAK_ARM));

const PAR_PROGRAM: g::Program<ParSteps> =
    g::par(g::par_chain::<Lane0Steps>(LANE0).and::<Lane1Steps>(LANE1));

#[test]
fn par_with_route_on_different_lanes_compiles() {
    // Verify Client projection
    let client_program: RoleProgram<'static, 0, <ParSteps as g::steps::ProjectRole<Client>>::Output> =
        g::project::<0, ParSteps, _>(&PAR_PROGRAM);

    // Client: send on Lane 0, recv on Lane 0
    let steps = client_program.steps();
    assert!(steps.len() >= 2, "Client should have at least 2 steps");

    // Verify Server projection
    let server_program: RoleProgram<'static, 1, <ParSteps as g::steps::ProjectRole<Server>>::Output> =
        g::project::<1, ParSteps, _>(&PAR_PROGRAM);

    // Server: recv on Lane 0, send on Lane 0, recv on Lane 1 (TickMsg)
    let steps = server_program.steps();
    assert!(steps.len() >= 2, "Server should have at least 2 steps");

    // Verify Controller projection
    let controller_program: RoleProgram<'static, 2, <ParSteps as g::steps::ProjectRole<Controller>>::Output> =
        g::project::<2, ParSteps, _>(&PAR_PROGRAM);

    // Controller: route control on Lane 1
    let steps = controller_program.steps();
    assert!(steps.len() >= 1, "Controller should have at least 1 step");
}

#[test]
fn lane_assignments_are_preserved_in_projection() {
    let server_program: RoleProgram<'static, 1, <ParSteps as g::steps::ProjectRole<Server>>::Output> =
        g::project::<1, ParSteps, _>(&PAR_PROGRAM);

    let steps = server_program.steps();

    // Check lane distribution
    let lane0_steps: Vec<_> = steps.iter().filter(|s| s.lane() == 0).collect();
    let lane1_steps: Vec<_> = steps.iter().filter(|s| s.lane() == 1).collect();

    // Server should have steps on both lanes
    assert!(!lane0_steps.is_empty(), "Server should have steps on Lane 0 (request-response)");
    assert!(!lane1_steps.is_empty(), "Server should have steps on Lane 1 (tick recv)");
}

#[test]
fn scope_markers_exist_for_route_in_par() {
    let eff_list = PAR_PROGRAM.eff_list();
    let scope_markers = eff_list.scope_markers();

    // Should have Parallel scope markers
    let parallel_markers: Vec<_> = scope_markers
        .iter()
        .filter(|m| matches!(m.scope_kind, hibana::global::const_dsl::ScopeKind::Parallel))
        .collect();
    assert!(!parallel_markers.is_empty(), "Should have Parallel scope markers");

    // Should have Route scope markers (from the loop on Lane 1)
    let route_markers: Vec<_> = scope_markers
        .iter()
        .filter(|m| matches!(m.scope_kind, hibana::global::const_dsl::ScopeKind::Route))
        .collect();
    assert!(!route_markers.is_empty(), "Should have Route scope markers from the loop");
}
