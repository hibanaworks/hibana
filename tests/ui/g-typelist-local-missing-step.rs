use hibana::g::{self};
use hibana::g::advanced::{RoleProgram, project};
use hibana::g::advanced::steps::{SendStep, StepCons, StepNil};

const PROGRAM: g::Program<StepCons<SendStep<g::Role<0>, g::Role<1>, g::Msg<7, u16>, 0>, StepNil>> =
    g::send::<g::Role<0>, g::Role<1>, g::Msg<7, u16>, 0>();

fn expect_missing(program: &g::Program<StepNil>) {
    let _: RoleProgram<'_, 0> = project(program);
}

fn main() {
    expect_missing(&PROGRAM);
}
