use hibana::g::{self, Msg};
use hibana::runtime::program::{RoleProgram, project};

fn main() {
    let left_arm = g::send::<0, 1, Msg<3, ()>>();
    let right_arm = g::send::<0, 1, Msg<4, ()>>();
    let route = g::route(left_arm, right_arm);
    let _: RoleProgram<0> = project(&route);
    let _: RoleProgram<1> = project(&route);
}
