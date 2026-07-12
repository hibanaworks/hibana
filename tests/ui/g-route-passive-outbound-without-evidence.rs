//! Shared output is not intrinsic branch evidence for another role.

use hibana::g;
use hibana::runtime::program::{RoleProgram, project};

fn main() {
    let left = g::seq(
        g::send::<0, 2, g::Msg<1, ()>>(),
        g::send::<1, 3, g::Msg<5, ()>>(),
    );
    let right = g::seq(
        g::send::<0, 2, g::Msg<2, ()>>(),
        g::send::<1, 3, g::Msg<5, ()>>(),
    );
    let route = g::route(left, right);
    let _: RoleProgram<1> = project(&route);
}
