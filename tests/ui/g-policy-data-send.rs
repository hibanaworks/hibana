use hibana::{
    g::{self, Msg},
    integration::program::{project, RoleProgram},
};

fn main() {
    let program = g::send::<0, 1, Msg<90, u8>>().policy::<7>();
    let _: RoleProgram<0> = project(&program);
}
