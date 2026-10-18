use hibana::{
    g::{self, Msg},
    runtime::program::{RoleProgram, project},
};

fn main() {
    let role: RoleProgram<0> = {
        let program = g::send::<0, 1, Msg<7, u16>>();
        project(&program)
    };
    std::hint::black_box(role);
}
