//! Dynamic route + defer surface should compile.

#[path = "../support/control_kinds.rs"]
mod control_kinds;

use hibana::substrate::cap::GenericCapToken;
use hibana::substrate::policy::DynamicResolution;
use hibana::g::{self};
use hibana::g::advanced::{CanonicalControl, RoleProgram, project};
use hibana::g::advanced::steps::{ProjectRole, SendStep, SeqSteps, StepConcat, StepCons, StepNil};

type RouteArm100Kind = control_kinds::RouteControl<100, 0>;
type RouteArm101Kind = control_kinds::RouteControl<101, 0>;

const POLICY_ID: u16 = 77;

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
        StepCons<SendStep<g::Role<0>, g::Role<1>, g::Msg<10, ()>>, StepNil>,
    >,
> =
    g::seq(
        g::send::<
            g::Role<0>,
            g::Role<0>,
            g::Msg<100, GenericCapToken<RouteArm100Kind>, CanonicalControl<RouteArm100Kind>>,
            0,
        >()
        .policy::<POLICY_ID>(),
        g::send::<g::Role<0>, g::Role<1>, g::Msg<10, ()>, 0>(),
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
        StepCons<SendStep<g::Role<0>, g::Role<1>, g::Msg<20, ()>>, StepNil>,
    >,
> =
    g::seq(
        g::send::<
            g::Role<0>,
            g::Role<0>,
            g::Msg<101, GenericCapToken<RouteArm101Kind>, CanonicalControl<RouteArm101Kind>>,
            0,
        >()
        .policy::<POLICY_ID>(),
        g::send::<g::Role<0>, g::Role<1>, g::Msg<20, ()>, 0>(),
    );

const ROUTE: g::Program<
    <SeqSteps<
        StepCons<
            SendStep<
                g::Role<0>,
                g::Role<0>,
                g::Msg<100, GenericCapToken<RouteArm100Kind>, CanonicalControl<RouteArm100Kind>>,
            >,
            StepNil,
        >,
        StepCons<SendStep<g::Role<0>, g::Role<1>, g::Msg<10, ()>>, StepNil>,
    > as StepConcat<
        SeqSteps<
            StepCons<
                SendStep<
                    g::Role<0>,
                    g::Role<0>,
                    g::Msg<101, GenericCapToken<RouteArm101Kind>, CanonicalControl<RouteArm101Kind>>,
                >,
                StepNil,
            >,
            StepCons<SendStep<g::Role<0>, g::Role<1>, g::Msg<20, ()>>, StepNil>,
        >,
    >>::Output,
> = g::route(ARM0, ARM1);

static PASSIVE_PROGRAM: RoleProgram<
    'static,
    1,
    <<SeqSteps<
        StepCons<
            SendStep<
                g::Role<0>,
                g::Role<0>,
                g::Msg<100, GenericCapToken<RouteArm100Kind>, CanonicalControl<RouteArm100Kind>>,
            >,
            StepNil,
        >,
        StepCons<SendStep<g::Role<0>, g::Role<1>, g::Msg<10, ()>>, StepNil>,
    > as StepConcat<
        SeqSteps<
            StepCons<
                SendStep<
                    g::Role<0>,
                    g::Role<0>,
                    g::Msg<101, GenericCapToken<RouteArm101Kind>, CanonicalControl<RouteArm101Kind>>,
                >,
                StepNil,
            >,
            StepCons<SendStep<g::Role<0>, g::Role<1>, g::Msg<20, ()>>, StepNil>,
        >,
    >>::Output as ProjectRole<g::Role<1>>>::Output,
> = project(&ROUTE);

fn main() {
    let _ = &PASSIVE_PROGRAM;
    let _ = DynamicResolution::Defer { retry_hint: 1 };
}
