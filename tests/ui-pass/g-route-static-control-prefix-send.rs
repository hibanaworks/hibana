//! Route projection test with shared send prefix (compile-pass).

use hibana::g::{self};
use hibana::integration::program::{RoleProgram, project};

fn main() {
    let prefix = g::send::<1, 0, g::Msg<7, ()>>();
    let route = g::seq(
        prefix,
        g::route(
            g::send::<0, 1, g::Msg<10, ()>>(),
            g::send::<0, 1, g::Msg<20, ()>>(),
        ),
    );
    let passive_program: RoleProgram<1> = project(&route);
    let _ = passive_program;
}
