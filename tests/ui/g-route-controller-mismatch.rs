use hibana::g::{self};
use hibana::runtime::program::{RoleProgram, project};

// The arms disagree on their first visible controller, so projection must reject
// the route before it can become a projected role program.
fn main() {
    let bad_arm = g::send::<2, 2, g::Msg<5, ()>>();
    let good_arm = g::send::<0, 0, g::Msg<6, ()>>();
    let route = g::route(bad_arm, good_arm);
    let _: RoleProgram<0> = project(&route);
}
