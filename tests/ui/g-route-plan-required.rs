#![allow(dead_code)]

use hibana::g::steps::{SendStep, StepConcat, StepCons, StepNil};
use hibana::g::{self, Msg, Role};

type Controller = Role<0>;
type Worker = Role<1>;

type LeftMsg = Msg<3, ()>;
type RightMsg = Msg<4, ()>;

type LeftSteps = StepCons<SendStep<Controller, Worker, LeftMsg>, StepNil>;
type RightSteps = StepCons<SendStep<Controller, Worker, RightMsg>, StepNil>;
type RouteSteps = <LeftSteps as StepConcat<RightSteps>>::Output;

const LEFT_ARM: g::Program<LeftSteps> = g::send::<Controller, Worker, LeftMsg>();
const RIGHT_ARM: g::Program<RightSteps> = g::send::<Controller, Worker, RightMsg>();

const _: g::Program<RouteSteps> = g::route::<0, 1, _>(
    g::route_chain::<0, LeftSteps>(LEFT_ARM).and::<RightSteps>(RIGHT_ARM),
);

fn main() {}
