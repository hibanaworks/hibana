use hibana::g::advanced::steps::{SendStep, StepCons, StepNil};
use hibana::g::{self};

const LABEL_CANCEL: u8 = 60;

const BAD: g::ProgramSource<
    StepCons<SendStep<g::Role<0>, g::Role<1>, g::Msg<LABEL_CANCEL, ()>, 0>, StepNil>,
> = g::send::<g::Role<0>, g::Role<1>, g::Msg<LABEL_CANCEL, ()>, 0>();

fn main() {
    let _ = BAD;
}
