use hibana::g::{self, Msg, Role};
use hibana::g::advanced::{ProgramWitness, RoleProgram, project};
use hibana::g::advanced::steps::{SendStep, StepCons, StepNil};

type ProgramASteps = StepCons<SendStep<Role<0>, Role<1>, Msg<1, ()>, 0>, StepNil>;
type ProgramBSteps = StepCons<SendStep<Role<0>, Role<1>, Msg<2, ()>, 0>, StepNil>;

const PROGRAM_A: g::Program<ProgramASteps> = g::send::<Role<0>, Role<1>, Msg<1, ()>, 0>();
const PROGRAM_B: g::Program<ProgramBSteps> = g::send::<Role<0>, Role<1>, Msg<2, ()>, 0>();

fn expect_program_a(_program: RoleProgram<'static, 0, ProgramWitness<ProgramASteps>>) {}

fn main() {
    let _expected: RoleProgram<'static, 0, ProgramWitness<ProgramASteps>> = project(&PROGRAM_A);
    let wrong: RoleProgram<'static, 0, ProgramWitness<ProgramBSteps>> = project(&PROGRAM_B);
    expect_program_a(wrong);
}
