use hibana::g::{self};
use hibana::g::advanced::{RoleProgram, project};
use hibana::g::advanced::steps::{ProjectRole, SendStep, StepConcat, StepCons, StepNil};
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
    <<
        StepCons<SendStep<g::Role<0>, g::Role<1>, g::Msg<10, ()>, 0>, StepNil> as StepConcat<
            StepCons<SendStep<g::Role<2>, g::Role<3>, g::Msg<11, ()>, 0>, StepNil>,
        >
    >::Output as StepConcat<
        StepCons<SendStep<g::Role<4>, g::Role<5>, g::Msg<12, ()>, 0>, StepNil>,
    >>::Output,
> = g::par(g::par(LANE_A, LANE_B), LANE_C);

fn main() {
    let program: RoleProgram<
        '_,
        0,
        <<<StepCons<
            SendStep<g::Role<0>, g::Role<1>, g::Msg<10, ()>, 0>,
            StepNil,
        > as StepConcat<
            StepCons<SendStep<g::Role<2>, g::Role<3>, g::Msg<11, ()>, 0>, StepNil>,
        >>::Output as StepConcat<
            StepCons<SendStep<g::Role<4>, g::Role<5>, g::Msg<12, ()>, 0>, StepNil>,
        >>::Output as ProjectRole<g::Role<0>>>::Output,
        MintConfig,
    > = project(&PARALLEL);
    assert_eq!(program.eff_list().len(), 3);
}
