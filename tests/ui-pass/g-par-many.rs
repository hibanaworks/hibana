use hibana::g::{self};
use hibana::g::advanced::{RoleProgram, project};
use hibana::g::advanced::steps::{ParSteps, SendStep, StepCons, StepNil};
use hibana::substrate::cap::advanced::MintConfig;

const LANE_A: g::Program<
    StepCons<SendStep<g::Role<0>, g::Role<1>, g::Msg<10, ()>, 0>, StepNil>,
> = g::send::<g::Role<0>, g::Role<1>, g::Msg<10, ()>, 0>();
const LANE_B: g::Program<
    StepCons<SendStep<g::Role<2>, g::Role<3>, g::Msg<11, ()>, 0>, StepNil>,
> = g::send::<g::Role<2>, g::Role<3>, g::Msg<11, ()>, 0>();
const LANE_C: g::Program<
    StepCons<SendStep<g::Role<4>, g::Role<5>, g::Msg<12, ()>, 0>, StepNil>,
> = g::send::<g::Role<4>, g::Role<5>, g::Msg<12, ()>, 0>();

const PARALLEL: g::Program<
    ParSteps<
        ParSteps<
            StepCons<SendStep<g::Role<0>, g::Role<1>, g::Msg<10, ()>, 0>, StepNil>,
            StepCons<SendStep<g::Role<2>, g::Role<3>, g::Msg<11, ()>, 0>, StepNil>,
        >,
        StepCons<SendStep<g::Role<4>, g::Role<5>, g::Msg<12, ()>, 0>, StepNil>,
    >,
> = g::par(g::par(LANE_A, LANE_B), LANE_C);

fn main() {
    let frozen = PARALLEL;
    let program: RoleProgram<
        '_,
        0,
        ParSteps<
            ParSteps<
                StepCons<SendStep<g::Role<0>, g::Role<1>, g::Msg<10, ()>, 0>, StepNil>,
                StepCons<SendStep<g::Role<2>, g::Role<3>, g::Msg<11, ()>, 0>, StepNil>,
            >,
            StepCons<SendStep<g::Role<4>, g::Role<5>, g::Msg<12, ()>, 0>, StepNil>,
        >,
        MintConfig,
    > = project(&frozen);
    let _ = &program;
}
