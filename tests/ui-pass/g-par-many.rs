use hibana::substrate::program::{RoleProgram, project};
use hibana::g::{self};

fn main() {
    let lane_a = g::send::<g::Role<0>, g::Role<1>, g::Msg<10, ()>, 0>();
    let lane_b = g::send::<g::Role<2>, g::Role<3>, g::Msg<11, ()>, 0>();
    let lane_c = g::send::<g::Role<4>, g::Role<5>, g::Msg<12, ()>, 0>();
    let parallel = g::par(g::par(lane_a, lane_b), lane_c);
    let program: RoleProgram<0> = project(&parallel);
    let _ = program;
}
