use hibana::g::{self, Msg, SendStep, StepCons, StepNil};

type Steps = StepCons<
    SendStep<g::Role<0>, g::Role<1>, Msg<1, ()>, 0>,
    StepNil,
>;

const PROGRAM: g::Program<Steps> = g::send::<g::Role<0>, g::Role<1>, Msg<1, ()>, 0>();

fn main() {
    let eff_list = PROGRAM.eff_list();
    let slice: &[hibana::eff::EffStruct] = eff_list.as_slice();
    let _first = &slice[0];

    let _also_slice: &[hibana::eff::EffStruct] = eff_list.as_ref();
    let _static_slice: &'static [hibana::eff::EffStruct] = eff_list.as_static_slice();

    assert_eq!(eff_list.len(), 1);
}
