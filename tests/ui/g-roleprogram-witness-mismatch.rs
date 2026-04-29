#![allow(unreachable_code)]

use hibana::substrate::program::RoleProgram;

fn main() {
    core::hint::black_box(RoleProgram::<0> {
        image: unreachable!(),
    });
}
