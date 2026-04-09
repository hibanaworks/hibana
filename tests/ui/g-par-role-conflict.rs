use hibana::g::{self};
use hibana::g::advanced::steps::{SendStep, StepCons, StepNil};

const _: () = {
    let lane_a: g::ProgramSource<StepCons<SendStep<g::Role<0>, g::Role<1>, g::Msg<3, ()>, 0>, StepNil>> =
        g::send::<g::Role<0>, g::Role<1>, g::Msg<3, ()>, 0>();
    let lane_b: g::ProgramSource<StepCons<SendStep<g::Role<0>, g::Role<2>, g::Msg<4, ()>, 0>, StepNil>> =
        g::send::<g::Role<0>, g::Role<2>, g::Msg<4, ()>, 0>();
    let _ = g::par(lane_a, lane_b);
};

fn main() {}
