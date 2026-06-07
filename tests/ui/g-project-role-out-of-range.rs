use hibana::{
    g,
    integration::program::{RoleProgram, project},
};

fn main() {
    let program = g::send::<0, 1, g::Msg<1, ()>>();
    let _: RoleProgram<16> = project(&program);
}
