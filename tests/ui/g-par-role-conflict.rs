use hibana::g::{self};
use hibana::integration::program::{RoleProgram, project};

fn main() {
    let lane_a: g::Program<_> = g::send::<g::Role<0>, g::Role<1>, g::Msg<3, ()>, 0>();
    let lane_b: g::Program<_> = g::send::<g::Role<0>, g::Role<2>, g::Msg<4, ()>, 0>();
    let parallel = g::par(lane_a, lane_b);
    let _: RoleProgram<0> = project(&parallel);
}
