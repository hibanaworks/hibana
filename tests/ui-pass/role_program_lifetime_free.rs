use hibana::{
    g::{self, Msg, Role},
    substrate::program::{RoleProgram, project},
};

fn main() {
    let program = g::send::<Role<0>, Role<1>, Msg<7, u16>, 0>();
    let role: RoleProgram<0> = project(&program);
    drop(program);
    let _ = role;
}
