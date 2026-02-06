use hibana::g::{self, SendStep, StepCons, StepNil};

type Controller = g::Role<0>;
type Target = g::Role<1>;
type Wrong = g::Role<2>;

type BadArm = StepCons<SendStep<Wrong, Controller, g::Msg<5, ()>>, StepNil>;
type GoodArm = StepCons<SendStep<Controller, Target, g::Msg<6, ()>>, StepNil>;

const BAD_ARM: g::Program<BadArm> = g::send::<Wrong, Controller, g::Msg<5, ()>>();
const GOOD_ARM: g::Program<GoodArm> = g::send::<Controller, Target, g::Msg<6, ()>>();

// The first arm is not initiated by the declared controller, so the trait
// bound `RouteArm<CONTROLLER, TARGET>` must reject the construction.
const _: () = {
    let builder = g::route_chain::<0, BadArm>(BAD_ARM).and(GOOD_ARM);
    let _ = g::route(builder);
};

fn main() {}
