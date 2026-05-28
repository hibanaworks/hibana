#[path = "../support/control_kinds.rs"]
mod control_kinds;

use hibana::g;
use hibana::integration::program::{RoleProgram, project};

type RouteControlKind = control_kinds::RouteControl<0>;

fn main() {
    let arm0 = g::seq(
        g::send::<g::Role<0>, g::Role<0>, g::Msg<118, (), RouteControlKind>, 0>()
            .policy::<{ u16::MAX }>(),
        g::send::<g::Role<0>, g::Role<1>, g::Msg<10, ()>, 0>(),
    );
    let arm1 = g::seq(
        g::send::<g::Role<0>, g::Role<0>, g::Msg<119, (), RouteControlKind>, 0>().policy::<7>(),
        g::send::<g::Role<0>, g::Role<1>, g::Msg<20, ()>, 0>(),
    );
    let route = g::route(arm0, arm1);
    let _: RoleProgram<1> = project(&route);
}
