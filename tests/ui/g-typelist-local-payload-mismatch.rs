use hibana::g::{self};
use hibana::g::advanced::{RoleProgram, project};
use hibana::g::advanced::steps::{LocalSend, SendStep, StepCons, StepNil};

const PROGRAM: g::ProgramSource<StepCons<SendStep<g::Role<0>, g::Role<1>, g::Msg<7, u16>, 0>, StepNil>> =
    g::send::<g::Role<0>, g::Role<1>, g::Msg<7, u16>, 0>();
const PROGRAM_TOKEN: g::Program<StepCons<SendStep<g::Role<0>, g::Role<1>, g::Msg<7, u16>, 0>, StepNil>> =
    g::freeze(&PROGRAM);

// Intentionally declare an incorrect local typelist for the client role. The payload type
// (`u8`) mismatches the actual projection (`u16`), so this must fail during compilation.
const CLIENT: RoleProgram<
    'static,
    0,
    StepCons<LocalSend<g::Role<1>, g::Msg<7, u8>>, StepNil>,
> = project(&PROGRAM_TOKEN);

fn main() {
    let _ = CLIENT;
}
