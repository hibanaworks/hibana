use hibana::g::{self, SendStep, StepConcat, StepCons, StepNil};

type R0 = g::Role<0>;
type R1 = g::Role<1>;
type R2 = g::Role<2>;

type LaneA = StepCons<SendStep<R0, R1, g::Msg<3, ()>>, StepNil>;
type LaneB = StepCons<SendStep<R0, R2, g::Msg<4, ()>>, StepNil>;
type Combined = <LaneA as StepConcat<LaneB>>::Output;

const _: () = {
    let lane_a: g::Program<LaneA> = g::send::<R0, R1, g::Msg<3, ()>>();
    let lane_b: g::Program<LaneB> = g::send::<R0, R2, g::Msg<4, ()>>();
    let builder = g::par_chain::<LaneA>(lane_a).and(lane_b);
    let _ = g::par::<Combined>(builder);
};

fn main() {}
