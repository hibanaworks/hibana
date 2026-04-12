//! Compile-time unprojectable route test.
//!
//! If a passive role cannot merge arms and cannot build a functional
//! label→continuation dispatch, the route is unprojectable unless a
//! dynamic policy is provided. This test verifies the compile-time panic.

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
            StepCons<SendStep<g::Role<0>, g::Role<1>, g::Msg<42, ()>>, StepNil>,
            StepCons<SendStep<g::Role<0>, g::Role<1>, g::Msg<99, ()>>, StepNil>,
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
        g::send::<g::Role<0>, g::Role<1>, g::Msg<42, ()>, 0>(),
        g::send::<g::Role<0>, g::Role<1>, g::Msg<99, ()>, 0>(),
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
            StepCons<SendStep<g::Role<0>, g::Role<1>, g::Msg<42, ()>>, StepNil>,
            SeqSteps<
                StepCons<SendStep<g::Role<0>, g::Role<1>, g::Msg<99, ()>>, StepNil>,
                StepCons<SendStep<g::Role<0>, g::Role<1>, g::Msg<77, ()>>, StepNil>,
            >,
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
        g::send::<g::Role<0>, g::Role<1>, g::Msg<42, ()>, 0>(),
        g::seq(
            g::send::<g::Role<0>, g::Role<1>, g::Msg<99, ()>, 0>(),
            g::send::<g::Role<0>, g::Role<1>, g::Msg<77, ()>, 0>(),
        ),
    ),
);

// No dynamic policy here -> unprojectable for Passive.
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
            StepCons<SendStep<g::Role<0>, g::Role<1>, g::Msg<42, ()>>, StepNil>,
            StepCons<SendStep<g::Role<0>, g::Role<1>, g::Msg<99, ()>>, StepNil>,
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
            StepCons<SendStep<g::Role<0>, g::Role<1>, g::Msg<42, ()>>, StepNil>,
            SeqSteps<
                StepCons<SendStep<g::Role<0>, g::Role<1>, g::Msg<99, ()>>, StepNil>,
                StepCons<SendStep<g::Role<0>, g::Role<1>, g::Msg<77, ()>>, StepNil>,
            >,
        >,
    >,
>;

const ROUTE: g::Program<RouteProgramSteps> = g::route(ARM0, ARM1);

// Force evaluation by projecting to the passive role.
static PASSIVE_PROGRAM: RoleProgram<'static, 1, ProgramWitness<RouteProgramSteps>> =
    project(&ROUTE);

fn main() {
    let _ = &PASSIVE_PROGRAM;
}
