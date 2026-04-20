use hibana::g::{self};

const _: () = {
    let lane_a: g::Program<_> = g::send::<g::Role<0>, g::Role<1>, g::Msg<3, ()>, 0>();
    let lane_b: g::Program<_> = g::send::<g::Role<0>, g::Role<2>, g::Msg<4, ()>, 0>();
    let _ = g::par(lane_a, lane_b);
};

fn main() {}
