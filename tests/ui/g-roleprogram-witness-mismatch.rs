#![allow(unreachable_code)]

use hibana::substrate::program::RoleProgram;

fn main() {
    let _forged: RoleProgram<0> = RoleProgram::<0> {
        _seal: unreachable!(),
        image: unreachable!(),
    };
}
