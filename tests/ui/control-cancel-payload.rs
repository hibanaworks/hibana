use hibana::g::{self, SendStep, StepCons, StepNil};
use hibana::runtime::consts::LABEL_CANCEL;

type Controller = g::Role<0>;
type Target = g::Role<1>;

type Steps = StepCons<
    SendStep<Controller, Target, g::Msg<LABEL_CANCEL, ()>>,
    StepNil,
>;

const BAD: g::Program<Steps> =
    g::send::<Controller, Target, g::Msg<LABEL_CANCEL, ()>>();

fn main() {
    let _ = BAD;
}
