use hibana::g::{self};
// Duplicate labels inside a typed `route` must be rejected during const evaluation.
const _: () = {
    let arm_one = g::send::<g::Role<0>, g::Role<0>, g::Msg<5, ()>, 0>();
    let arm_two = g::send::<g::Role<0>, g::Role<0>, g::Msg<5, ()>, 0>();
    let _ = g::route(arm_one, arm_two);
};

fn main() {}
