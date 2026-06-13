use hibana::{
    g::{self, Msg},
    runtime::program::{project, RoleProgram},
};

fn main() {
    let program = g::send::<0, 1, Msg<90, u8>>().resolver::<7>();
    let _: RoleProgram<0> = project(&program);
}
