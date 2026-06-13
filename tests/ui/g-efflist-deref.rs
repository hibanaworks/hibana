use hibana::g::{self, Msg};
use hibana::runtime::program::{RoleProgram, project};

fn main() {
    let protocol = g::send::<0, 1, Msg<1, ()>>();
    let program: RoleProgram<0> = project(&protocol);
    let _ = program.eff_list();
}
