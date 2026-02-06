use hibana::g::{self, SendStep, StepConcat, StepCons, StepNil};

type R0 = g::Role<0>;
type R1 = g::Role<1>;
type R2 = g::Role<2>;
type R3 = g::Role<3>;
type R4 = g::Role<4>;
type R5 = g::Role<5>;

type LaneA = StepCons<SendStep<R0, R1, g::Msg<10, ()>, 0>, StepNil>;
type LaneB = StepCons<SendStep<R2, R3, g::Msg<11, ()>, 0>, StepNil>;
type LaneC = StepCons<SendStep<R4, R5, g::Msg<12, ()>, 0>, StepNil>;

type StepsAB = <LaneA as StepConcat<LaneB>>::Output;
type Steps = <StepsAB as StepConcat<LaneC>>::Output;

const LANE_A: g::Program<LaneA> = g::send::<R0, R1, g::Msg<10, ()>, 0>();
const LANE_B: g::Program<LaneB> = g::send::<R2, R3, g::Msg<11, ()>, 0>();
const LANE_C: g::Program<LaneC> = g::send::<R4, R5, g::Msg<12, ()>, 0>();

const PARALLEL: g::Program<Steps> = {
    let builder = g::par_chain(LANE_A).and(LANE_B).and(LANE_C);
    g::par(builder)
};

fn main() {
    assert_eq!(PARALLEL.eff_list().len(), 3);
}
