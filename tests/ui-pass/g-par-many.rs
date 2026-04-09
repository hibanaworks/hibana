use hibana::g::{self};
use hibana::g::advanced::{RoleProgram, project};
use hibana::g::advanced::steps::{SendStep, StepConcat, StepCons, StepNil};
use hibana::substrate::cap::advanced::MintConfig;

const LANE_A: g::ProgramSource<
    StepCons<SendStep<g::Role<0>, g::Role<1>, g::Msg<10, ()>, 0>, StepNil>,
> = g::send::<g::Role<0>, g::Role<1>, g::Msg<10, ()>, 0>();
const LANE_B: g::ProgramSource<
    StepCons<SendStep<g::Role<2>, g::Role<3>, g::Msg<11, ()>, 0>, StepNil>,
> = g::send::<g::Role<2>, g::Role<3>, g::Msg<11, ()>, 0>();
const LANE_C: g::ProgramSource<
    StepCons<SendStep<g::Role<4>, g::Role<5>, g::Msg<12, ()>, 0>, StepNil>,
> = g::send::<g::Role<4>, g::Role<5>, g::Msg<12, ()>, 0>();

const PARALLEL: g::ProgramSource<
    <<
        StepCons<SendStep<g::Role<0>, g::Role<1>, g::Msg<10, ()>, 0>, StepNil> as StepConcat<
            StepCons<SendStep<g::Role<2>, g::Role<3>, g::Msg<11, ()>, 0>, StepNil>,
        >
    >::Output as StepConcat<
        StepCons<SendStep<g::Role<4>, g::Role<5>, g::Msg<12, ()>, 0>, StepNil>,
    >>::Output,
> = g::par(g::par(LANE_A, LANE_B), LANE_C);

fn main() {
    let frozen = g::freeze(&PARALLEL);
    let program: RoleProgram<
        '_,
        0,
        <<StepCons<
            SendStep<g::Role<0>, g::Role<1>, g::Msg<10, ()>, 0>,
            StepNil,
        > as StepConcat<
            StepCons<SendStep<g::Role<2>, g::Role<3>, g::Msg<11, ()>, 0>, StepNil>,
        >>::Output as StepConcat<
            StepCons<SendStep<g::Role<4>, g::Role<5>, g::Msg<12, ()>, 0>, StepNil>,
        >>::Output,
        MintConfig,
    > = project(&frozen);
    let _ = &program;
}
