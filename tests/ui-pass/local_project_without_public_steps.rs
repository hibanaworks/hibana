use hibana::g::advanced::{RoleProgram, project};
use hibana::g::{self};

fn main() {
    let left = g::send::<g::Role<0>, g::Role<1>, g::Msg<21, ()>, 0>();
    let right = g::send::<g::Role<1>, g::Role<0>, g::Msg<22, ()>, 0>();
    let program = g::seq(left, right);
    let controller: RoleProgram<0> = project(&program);
    let worker: RoleProgram<1> = project(&program);
    let _ = (controller, worker);
}
