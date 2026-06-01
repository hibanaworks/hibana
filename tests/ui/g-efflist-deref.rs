use hibana::g::{self, Msg};
use hibana::integration::program::{RoleProgram, project};

fn main() {
    let protocol = g::send::<0, 1, Msg<1, ()>, 0>();
    let program: RoleProgram<0> = project(&protocol);
    let _ = program.eff_list();
}
