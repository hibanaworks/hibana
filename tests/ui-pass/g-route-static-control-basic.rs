//! Route projection test (compile-pass).

use hibana::g::{self};
use hibana::integration::program::{RoleProgram, project};

fn main() {
    let route = g::route(
        g::send::<0, 1, g::Msg<10, ()>>(),
        g::send::<0, 1, g::Msg<20, ()>>(),
    );
    let passive_program: RoleProgram<1> = project(&route);
    let _ = passive_program;
}
