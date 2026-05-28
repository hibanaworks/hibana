use hibana::g::{self};
use hibana::integration::program::{RoleProgram, project};

// The arms disagree on their controller self-send, so projection must reject
// the route before it can become a resident role program.
fn main() {
    let bad_arm = g::send::<g::Role<2>, g::Role<2>, g::Msg<5, ()>, 0>();
    let good_arm = g::send::<g::Role<0>, g::Role<0>, g::Msg<6, ()>, 0>();
    let route = g::route(bad_arm, good_arm);
    let _: RoleProgram<0> = project(&route);
}
