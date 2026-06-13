use hibana::g::{self};
use hibana::runtime::program::{RoleProgram, project};

fn main() {
    let left = g::send::<0, 1, g::Msg<3, ()>>();
    let right = g::send::<0, 2, g::Msg<4, ()>>();
    let parallel = g::par(left, right);
    let _: RoleProgram<0> = project(&parallel);
}
