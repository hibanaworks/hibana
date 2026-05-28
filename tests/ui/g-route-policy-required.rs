use hibana::g::{self, Msg, Role};
use hibana::integration::program::{RoleProgram, project};

fn main() {
    let left_arm = g::send::<Role<0>, Role<1>, Msg<3, ()>, 0>();
    let right_arm = g::send::<Role<0>, Role<1>, Msg<4, ()>, 0>();
    let route = g::route(left_arm, right_arm);
    let _: RoleProgram<0> = project(&route);
}
