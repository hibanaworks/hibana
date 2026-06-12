//! Dynamic route resolver sites are not valid control authorities for topology wire controls.

use hibana::g;
use hibana::integration::program::{project, RoleProgram};

fn main() {
    let route = g::route(
        g::send::<0, 1, g::ControlMsg<52, g::control::TopologyBegin>>(),
        g::send::<0, 1, g::Msg<53, ()>>(),
    )
    .resolve::<8>();
    let _: RoleProgram<0> = project(&route);
}
