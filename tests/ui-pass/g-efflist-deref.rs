use hibana::g::{self, Msg};
use hibana::g::advanced::{RoleProgram, project};
use hibana::g::advanced::steps::{ProjectRole, SendStep, StepCons, StepNil};
use hibana::substrate::cap::advanced::MintConfig;

const PROGRAM: g::Program<
    StepCons<SendStep<g::Role<0>, g::Role<1>, Msg<1, ()>, 0>, StepNil>,
> = g::send::<g::Role<0>, g::Role<1>, Msg<1, ()>, 0>();

fn main() {
    let program: RoleProgram<
        '_,
        0,
        <StepCons<SendStep<g::Role<0>, g::Role<1>, Msg<1, ()>, 0>, StepNil> as ProjectRole<g::Role<0>>>::Output,
        MintConfig,
    > = project(&PROGRAM);
        let eff_list = program.eff_list();
    let slice = eff_list.as_slice();
    let _first = &slice[0];

    let _also_slice = eff_list.as_ref();
    let _static_slice = eff_list.as_static_slice();

    assert_eq!(eff_list.len(), 1);
}
