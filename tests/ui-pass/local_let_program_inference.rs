use hibana::runtime::program::{RoleProgram, project};
use hibana::g::{self};

fn main() {
    let request = g::send::<0, 1, g::Msg<10, u32>>();
    let reply = g::send::<1, 0, g::Msg<11, u32>>();
    let program = g::seq(request, reply);
    let projected: RoleProgram<0> = project(&program);
    let _ = projected;
}
