use hibana::g::{self, Msg};
use hibana::g::advanced::{RoleProgram, project};
use hibana::g::advanced::steps::{SendStep, StepCons, StepNil};
use hibana::substrate::cap::advanced::MintConfig;

const PROGRAM: g::ProgramSource<
    StepCons<SendStep<g::Role<0>, g::Role<1>, Msg<1, ()>, 0>, StepNil>,
> = g::send::<g::Role<0>, g::Role<1>, Msg<1, ()>, 0>();

fn main() {
    let program: RoleProgram<
        '_,
        0,
        StepCons<SendStep<g::Role<0>, g::Role<1>, Msg<1, ()>, 0>, StepNil>,
        MintConfig,
    > = project(&g::freeze(&PROGRAM));
    let _ = program.eff_list();
}
