//! A dynamic resolver is not branch evidence for another role's first output.

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
    let route = g::route(left, right).resolve::<7>();
    let _: RoleProgram<1> = project(&route);
}
