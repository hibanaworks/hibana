use hibana::g::{self};
// The arms disagree on their controller self-send, so binary route construction
// must fail before const evaluation.
const _: () = {
    let bad_arm = g::send::<g::Role<2>, g::Role<2>, g::Msg<5, ()>, 0>();
    let good_arm = g::send::<g::Role<0>, g::Role<0>, g::Msg<6, ()>, 0>();
    let _ = g::route(bad_arm, good_arm);
};

fn main() {}
