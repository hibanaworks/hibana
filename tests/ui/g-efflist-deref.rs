use hibana::g::{self, Msg};
use hibana::g::advanced::{RoleProgram, project};

fn main() {
    let protocol = g::send::<g::Role<0>, g::Role<1>, Msg<1, ()>, 0>();
    let program: RoleProgram<0> = project(&protocol);
    let _ = program.eff_list();
}
