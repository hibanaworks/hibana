use hibana::g::{self, Msg};
use hibana::integration::program::{RoleProgram, project};

fn main() {
    let left_arm = g::send::<0, 1, Msg<3, ()>, 0>();
    let right_arm = g::send::<0, 1, Msg<4, ()>, 0>();
    let route = g::route(left_arm, right_arm);
    let _: RoleProgram<0> = project(&route);
}
