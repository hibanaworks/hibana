//! Route projection test with shared local prefix (compile-pass).
//!
//! Shared local prefixes remain valid when static route authority is carried by
//! explicit RouteDecision control messages.

#[path = "../support/control_kinds.rs"]
mod control_kinds;

use hibana::substrate::cap::GenericCapToken;
use hibana::g::{self};
use hibana::g::advanced::{CanonicalControl, ProgramWitness, RoleProgram, project};
use hibana::g::advanced::steps::{RouteSteps, SendStep, SeqSteps, StepCons, StepNil};

type RouteArm100Kind = control_kinds::RouteControl<100, 0>;
type RouteArm101Kind = control_kinds::RouteControl<101, 0>;

const ARM0: g::Program<
    SeqSteps<
        StepCons<
            SendStep<
                g::Role<0>,
                g::Role<0>,
                g::Msg<100, GenericCapToken<RouteArm100Kind>, CanonicalControl<RouteArm100Kind>>,
            >,
            StepNil,
        >,
        SeqSteps<
            StepCons<SendStep<g::Role<1>, g::Role<1>, g::Msg<7, ()>>, StepNil>,
            StepCons<SendStep<g::Role<0>, g::Role<1>, g::Msg<10, ()>>, StepNil>,
        >,
    >,
> = g::seq(
    g::send::<
        g::Role<0>,
        g::Role<0>,
        g::Msg<100, GenericCapToken<RouteArm100Kind>, CanonicalControl<RouteArm100Kind>>,
        0,
    >(),
    g::seq(
        g::send::<g::Role<1>, g::Role<1>, g::Msg<7, ()>, 0>(),
        g::send::<g::Role<0>, g::Role<1>, g::Msg<10, ()>, 0>(),
    ),
);

const ARM1: g::Program<
    SeqSteps<
        StepCons<
            SendStep<
                g::Role<0>,
                g::Role<0>,
                g::Msg<101, GenericCapToken<RouteArm101Kind>, CanonicalControl<RouteArm101Kind>>,
            >,
            StepNil,
        >,
        SeqSteps<
            StepCons<SendStep<g::Role<1>, g::Role<1>, g::Msg<7, ()>>, StepNil>,
            StepCons<SendStep<g::Role<0>, g::Role<1>, g::Msg<20, ()>>, StepNil>,
        >,
    >,
> = g::seq(
    g::send::<
        g::Role<0>,
        g::Role<0>,
        g::Msg<101, GenericCapToken<RouteArm101Kind>, CanonicalControl<RouteArm101Kind>>,
        0,
    >(),
    g::seq(
        g::send::<g::Role<1>, g::Role<1>, g::Msg<7, ()>, 0>(),
        g::send::<g::Role<0>, g::Role<1>, g::Msg<20, ()>, 0>(),
    ),
);

type RouteProgramSteps = RouteSteps<
    SeqSteps<
        StepCons<
            SendStep<
                g::Role<0>,
                g::Role<0>,
                g::Msg<100, GenericCapToken<RouteArm100Kind>, CanonicalControl<RouteArm100Kind>>,
            >,
            StepNil,
        >,
        SeqSteps<
            StepCons<SendStep<g::Role<1>, g::Role<1>, g::Msg<7, ()>>, StepNil>,
            StepCons<SendStep<g::Role<0>, g::Role<1>, g::Msg<10, ()>>, StepNil>,
        >,
    >,
    SeqSteps<
        StepCons<
            SendStep<
                g::Role<0>,
                g::Role<0>,
                g::Msg<101, GenericCapToken<RouteArm101Kind>, CanonicalControl<RouteArm101Kind>>,
            >,
            StepNil,
        >,
        SeqSteps<
            StepCons<SendStep<g::Role<1>, g::Role<1>, g::Msg<7, ()>>, StepNil>,
            StepCons<SendStep<g::Role<0>, g::Role<1>, g::Msg<20, ()>>, StepNil>,
        >,
    >,
>;

const ROUTE: g::Program<RouteProgramSteps> = g::route(ARM0, ARM1);

static PASSIVE_PROGRAM: RoleProgram<'static, 1, ProgramWitness<RouteProgramSteps>> =
    project(&ROUTE);

fn main() {
    let _ = &PASSIVE_PROGRAM;
}

// RouteArmKind boilerplate (same pattern as g-route-three-arms.rs)
