use hibana::g::{self, SendStep, StepCons, StepNil};
use hibana::runtime::consts::LABEL_CHECKPOINT;

type Controller = g::Role<0>;
type Target = g::Role<1>;

type Steps = StepCons<
    SendStep<Controller, Target, g::Msg<LABEL_CHECKPOINT, ()>>,
    StepNil,
>;

const BAD: g::Program<Steps> =
    g::send::<Controller, Target, g::Msg<LABEL_CHECKPOINT, ()>>();

fn main() {
    let _ = BAD;
}
