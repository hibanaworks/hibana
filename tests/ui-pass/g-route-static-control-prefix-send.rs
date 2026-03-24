//! Route projection test with shared send prefix (compile-pass).
//!
//! Shared send prefixes remain valid when static route authority is carried by
//! explicit RouteDecision control messages.

use hibana::substrate::cap::GenericCapToken;
use hibana::g::{self};
use hibana::g::advanced::{CanonicalControl, RoleProgram, project};
use hibana::g::advanced::steps::{ProjectRole, SendStep, SeqSteps, StepConcat, StepCons, StepNil};

hibana::impl_control_resource!(
    RouteArm100Kind,
    handle: RouteDecision,
    name: "RouteArm100",
    label: 100,
);

hibana::impl_control_resource!(
    RouteArm101Kind,
    handle: RouteDecision,
    name: "RouteArm101",
    label: 101,
);

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
            StepCons<SendStep<g::Role<1>, g::Role<0>, g::Msg<7, ()>>, StepNil>,
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
        g::send::<g::Role<1>, g::Role<0>, g::Msg<7, ()>, 0>(),
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
            StepCons<SendStep<g::Role<1>, g::Role<0>, g::Msg<7, ()>>, StepNil>,
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
        g::send::<g::Role<1>, g::Role<0>, g::Msg<7, ()>, 0>(),
        g::send::<g::Role<0>, g::Role<1>, g::Msg<20, ()>, 0>(),
    ),
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
        SeqSteps<
            StepCons<SendStep<g::Role<1>, g::Role<0>, g::Msg<7, ()>>, StepNil>,
            StepCons<SendStep<g::Role<0>, g::Role<1>, g::Msg<10, ()>>, StepNil>,
        >,
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
            SeqSteps<
                StepCons<SendStep<g::Role<1>, g::Role<0>, g::Msg<7, ()>>, StepNil>,
                StepCons<SendStep<g::Role<0>, g::Role<1>, g::Msg<20, ()>>, StepNil>,
            >,
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
        SeqSteps<
            StepCons<SendStep<g::Role<1>, g::Role<0>, g::Msg<7, ()>>, StepNil>,
            StepCons<SendStep<g::Role<0>, g::Role<1>, g::Msg<10, ()>>, StepNil>,
        >,
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
            SeqSteps<
                StepCons<SendStep<g::Role<1>, g::Role<0>, g::Msg<7, ()>>, StepNil>,
                StepCons<SendStep<g::Role<0>, g::Role<1>, g::Msg<20, ()>>, StepNil>,
            >,
        >,
    >>::Output as ProjectRole<g::Role<1>>>::Output,
> = project(&ROUTE);

fn main() {
    let _ = PASSIVE_PROGRAM.eff_list();
}

// RouteArmKind boilerplate (same pattern as g-route-three-arms.rs)
