use hibana::g::{self};
use hibana::g::advanced::{RoleProgram, project};
use hibana::g::advanced::steps::{LocalSend, SendStep, StepCons, StepNil};

const PROGRAM: g::Program<StepCons<SendStep<g::Role<0>, g::Role<1>, g::Msg<7, u16>, 0>, StepNil>> =
    g::send::<g::Role<0>, g::Role<1>, g::Msg<7, u16>, 0>();
const PROGRAM_TOKEN: g::Program<
    StepCons<LocalSend<g::Role<1>, g::Msg<7, u8>>, StepNil>,
> = PROGRAM;

const CLIENT: RoleProgram<'static, 0> =
    project(&PROGRAM_TOKEN);

fn main() {
    let _ = CLIENT;
}
