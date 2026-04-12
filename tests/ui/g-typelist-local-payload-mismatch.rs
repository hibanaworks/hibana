use hibana::g::{self};
use hibana::g::advanced::{ProgramWitness, RoleProgram, project};
use hibana::g::advanced::steps::{LocalSend, SendStep, StepCons, StepNil};

const PROGRAM: g::Program<StepCons<SendStep<g::Role<0>, g::Role<1>, g::Msg<7, u16>, 0>, StepNil>> =
    g::send::<g::Role<0>, g::Role<1>, g::Msg<7, u16>, 0>();
const PROGRAM_TOKEN: g::Program<StepCons<SendStep<g::Role<0>, g::Role<1>, g::Msg<7, u16>, 0>, StepNil>> =
    PROGRAM;

// Explicit local typelists are no longer part of the public projection identity.
const CLIENT: RoleProgram<
    'static,
    0,
    ProgramWitness<StepCons<LocalSend<g::Role<1>, g::Msg<7, u8>>, StepNil>>,
> = project(&PROGRAM_TOKEN);

fn main() {
    let _ = CLIENT;
}
