use hibana::{
    g::{self, Msg},
    runtime::program::{RoleProgram, project},
};

fn main() {
    let program = g::send::<0, 1, Msg<7, u16>>();
    let role: RoleProgram<0> = project(&program);
    drop(program);
    let _ = role;
}
