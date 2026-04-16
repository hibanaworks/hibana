use hibana::g::{self, Msg};
use hibana::g::advanced::{RoleProgram, project};
use hibana::g::advanced::steps::{SendStep, StepCons, StepNil};
use hibana::substrate::cap::advanced::MintConfig;

const PROGRAM: g::Program<
    StepCons<SendStep<g::Role<0>, g::Role<1>, Msg<1, ()>, 0>, StepNil>,
> = g::send::<g::Role<0>, g::Role<1>, Msg<1, ()>, 0>();

fn main() {
    let program: RoleProgram<'_, 0, MintConfig> = project(&PROGRAM);
    let _ = program.eff_list();
}
