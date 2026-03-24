use hibana::g::{self};
use hibana::g::advanced::steps::StepNil;

// Attempting to materialise a route with a single arm must fail because
// binary `route(left, right)` requires both arms to satisfy the route-arm
// shape contract.
const _: () = {
    let arm = g::send::<g::Role<0>, g::Role<0>, g::Msg<3, ()>, 0>();
    let _ = g::route(arm, StepNil::PROGRAM);
};

fn main() {}
