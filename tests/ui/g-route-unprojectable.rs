//! Compile-time unprojectable route test.
//!
//! Role 1 is not the intrinsic route controller, and after a shared outbound
//! prefix only the right arm asks role 1 to send again. There is no inbound
//! evidence that can make that outbound continuation deterministic.

use hibana::g::{self};
use hibana::runtime::program::{RoleProgram, project};

fn main() {
    let arm0 = g::seq(
        g::send::<0, 3, g::Msg<118, ()>>(),
        g::send::<1, 2, g::Msg<42, ()>>(),
    );
    let arm1 = g::seq(
        g::send::<0, 3, g::Msg<119, ()>>(),
        g::seq(
            g::send::<1, 2, g::Msg<42, ()>>(),
            g::send::<1, 2, g::Msg<77, ()>>(),
        ),
    );
    let route = g::route(arm0, arm1);
    let passive_program: RoleProgram<1> = project(&route);
    let _ = &passive_program;
}
