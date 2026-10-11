//! Roll reentry is not branch authority for a passive sender.

use hibana::g;
use hibana::runtime::program::{RoleProgram, project};

fn main() {
    let left = g::send::<0, 2, g::Msg<1, ()>>();
    let right = g::seq(
        g::send::<0, 2, g::Msg<2, ()>>(),
        g::send::<1, 2, g::Msg<3, ()>>(),
    );
    let route = g::route(left, right).roll();
    let _: RoleProgram<1> = project(&route);
}
