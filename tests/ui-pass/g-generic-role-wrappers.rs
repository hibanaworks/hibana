use hibana::g::advanced::{RoleProgram, project};
use hibana::g::{self};

fn generic_send<const ROLE: u8>() {
    let _ = g::send::<g::Role<ROLE>, g::Role<1>, g::Msg<21, ()>, 0>();
}

fn project_generic_role<const ROLE: u8>() {
    let left = g::send::<g::Role<0>, g::Role<1>, g::Msg<21, ()>, 0>();
    let right = g::send::<g::Role<1>, g::Role<0>, g::Msg<22, ()>, 0>();
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
