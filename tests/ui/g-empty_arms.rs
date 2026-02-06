use hibana::g::{self, SendStep, StepCons, StepNil};

type Controller = g::Role<0>;
type Target = g::Role<1>;

type ArmSteps = StepCons<SendStep<Controller, Target, g::Msg<3, ()>>, StepNil>;

// Attempting to materialise a route with a single arm must fail because
// `RouteChainBuilder::finish` enforces at least two branches.
const _: () = {
    let arm = g::send::<Controller, Target, g::Msg<3, ()>>();
    let builder = g::route_chain::<0, ArmSteps>(arm);
    let _ = g::route(builder);
};

fn main() {}
