use hibana::g::{self, SendStep, StepCons, StepNil};

type Controller = g::Role<0>;
type Target = g::Role<1>;

type ArmSteps = StepCons<SendStep<Controller, Target, g::Msg<5, ()>>, StepNil>;

const ARM_ONE: g::Program<ArmSteps> = g::send::<Controller, Target, g::Msg<5, ()>>();
const ARM_TWO: g::Program<ArmSteps> = g::send::<Controller, Target, g::Msg<5, ()>>();

// Duplicate labels inside a typed `route` must be rejected during const evaluation.
const _: () = {
    let builder = g::route_chain::<0, ArmSteps>(ARM_ONE).and(ARM_TWO);
    let _ = g::route(builder);
};

fn main() {}
