use hibana::substrate::program::{RoleProgram, project};
use hibana::g::{self};

fn main() {
    let request = g::send::<g::Role<0>, g::Role<1>, g::Msg<10, u32>, 0>();
    let reply = g::send::<g::Role<1>, g::Role<0>, g::Msg<11, u32>, 0>();
    let program = g::seq(request, reply);
    let projected: RoleProgram<0> = project(&program);
    let _ = projected;
}
