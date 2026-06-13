use hibana::runtime::program::{RoleProgram, project};
use hibana::g::{self};

fn generic_send<const ROLE: u8>() {
    let _ = g::send::<ROLE, 1, g::Msg<21, ()>>();
}

fn project_generic_role<const ROLE: u8>() {
    let left = g::send::<0, 1, g::Msg<21, ()>>();
    let right = g::send::<1, 0, g::Msg<22, ()>>();
    let program = g::seq(left, right);
    let projected: RoleProgram<ROLE> = project(&program);
    let _ = projected;
}

fn main() {
    generic_send::<0>();
    generic_send::<1>();
    project_generic_role::<0>();
    project_generic_role::<1>();
}
