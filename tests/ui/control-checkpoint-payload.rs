use hibana::g::advanced::steps::{SendStep, StepCons, StepNil};
use hibana::g::{self};

const LABEL_CHECKPOINT: u8 = 61;

const BAD: g::Program<
    StepCons<SendStep<g::Role<0>, g::Role<1>, g::Msg<LABEL_CHECKPOINT, ()>, 0>, StepNil>,
> =
    g::send::<g::Role<0>, g::Role<1>, g::Msg<LABEL_CHECKPOINT, ()>, 0>();

fn main() {
    let _ = BAD;
}
