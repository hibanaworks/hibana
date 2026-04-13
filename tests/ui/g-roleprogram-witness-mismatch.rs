use hibana::g::{self, Msg};
use hibana::g::advanced::{RoleProgram, project};
use hibana::g::advanced::steps::{SendStep, StepCons, StepNil};

const PROGRAM: g::Program<
    StepCons<SendStep<g::Role<0>, g::Role<1>, Msg<1, ()>, 0>, StepNil>,
> = g::send::<g::Role<0>, g::Role<1>, Msg<1, ()>, 0>();

const CLIENT: RoleProgram<'static, 0, StepNil> = project(&PROGRAM);

fn main() {
    let _ = CLIENT;
}
