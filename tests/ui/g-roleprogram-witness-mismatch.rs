#![allow(unreachable_code)]

use hibana::integration::program::RoleProgram;

fn main() {
    core::hint::black_box(RoleProgram::<0> {
        image: unreachable!(),
    });
}
