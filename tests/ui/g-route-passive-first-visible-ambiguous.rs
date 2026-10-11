use hibana::g;
use hibana::runtime::program::{RoleProgram, project};

fn main() {
    let left = g::send::<0, 1, g::Msg<9, ()>>();
    let right = g::send::<0, 1, g::Msg<9, ()>>();
    let route = g::route(left, right);
    let _: RoleProgram<1> = project(&route);
}
