use hibana::{
    g::{self, Msg, Role},
    integration::program::{project, RoleProgram},
};

fn main() {
    let program = g::send::<Role<0>, Role<1>, Msg<90, u8>, 0>().policy::<7>();
    let _: RoleProgram<0> = project(&program);
}
