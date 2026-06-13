use hibana::runtime::program::{RoleProgram, project};
use hibana::g::{self};

fn main() {
    let lane_a = g::send::<0, 1, g::Msg<10, ()>>();
    let lane_b = g::send::<2, 3, g::Msg<11, ()>>();
    let lane_c = g::send::<4, 5, g::Msg<12, ()>>();
    let parallel = g::par(g::par(lane_a, lane_b), lane_c);
    let program: RoleProgram<0> = project(&parallel);
    let _ = program;
}
