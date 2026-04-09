use hibana::g::{self, Msg, Role};
use hibana::g::advanced::steps::{SendStep, StepConcat, StepCons, StepNil};

const LEFT_ARM: g::ProgramSource<StepCons<SendStep<Role<0>, Role<1>, Msg<3, ()>, 0>, StepNil>> =
    g::send::<Role<0>, Role<1>, Msg<3, ()>, 0>();
const RIGHT_ARM: g::ProgramSource<StepCons<SendStep<Role<0>, Role<1>, Msg<4, ()>, 0>, StepNil>> =
    g::send::<Role<0>, Role<1>, Msg<4, ()>, 0>();

const _: g::ProgramSource<
    <StepCons<SendStep<Role<0>, Role<1>, Msg<3, ()>, 0>, StepNil> as StepConcat<
        StepCons<SendStep<Role<0>, Role<1>, Msg<4, ()>, 0>, StepNil>,
    >>::Output,
> = g::route(LEFT_ARM, RIGHT_ARM);

fn main() {}
