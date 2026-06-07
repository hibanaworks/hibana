use hibana::integration::program::{RoleProgram, project};
use hibana::g::{self};

fn main() {
    let left = g::send::<0, 1, g::Msg<21, ()>>();
    let right = g::send::<1, 0, g::Msg<22, ()>>();
    let program = g::seq(left, right);
    let controller: RoleProgram<0> = project(&program);
    let worker: RoleProgram<1> = project(&program);
    let _ = (controller, worker);
}
