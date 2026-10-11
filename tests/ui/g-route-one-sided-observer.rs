//! An uninvolved arm is not branch evidence for a passive role.

use hibana::g;
use hibana::runtime::program::{RoleProgram, project};

fn main() {
    let left = g::send::<0, 2, g::Msg<1, ()>>();
    let right = g::send::<0, 1, g::Msg<2, ()>>();
    let route = g::route(left, right);
    let _: RoleProgram<1> = project(&route);
}
