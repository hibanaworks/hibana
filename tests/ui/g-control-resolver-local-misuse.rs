//! Dynamic route resolver sites are not valid control authorities for state/txn local controls.

use hibana::g;
use hibana::integration::program::{project, RoleProgram};

fn main() {
    let route = g::route(
        g::send::<0, 0, g::ControlMsg<50, g::control::StateSnapshot>>(),
        g::send::<0, 0, g::ControlMsg<51, g::control::TxnCommit>>(),
    )
    .resolve::<7>();
    let _: RoleProgram<0> = project(&route);
}
