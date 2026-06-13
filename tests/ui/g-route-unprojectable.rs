//! Compile-time unprojectable route test.
//!
//! Role 1 cannot observe the first visible route action and the later role-1
//! suffixes are not mergeable, so projection must reject the route.

use hibana::g::{self};
use hibana::runtime::program::{RoleProgram, project};

fn main() {
    let arm0 = g::seq(
        g::send::<0, 2, g::Msg<118, ()>>(),
        g::seq(
            g::send::<0, 1, g::Msg<42, ()>>(),
            g::send::<0, 1, g::Msg<99, ()>>(),
        ),
    );
    let arm1 = g::seq(
        g::send::<0, 2, g::Msg<119, ()>>(),
        g::seq(
            g::send::<0, 1, g::Msg<42, ()>>(),
            g::seq(
                g::send::<0, 1, g::Msg<99, ()>>(),
                g::send::<0, 1, g::Msg<77, ()>>(),
            ),
        ),
    );
    let route = g::route(arm0, arm1);
    let passive_program: RoleProgram<1> = project(&route);
    let _ = &passive_program;
}
