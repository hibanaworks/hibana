use hibana::g;
use hibana::integration::cap::control::RouteDecisionKind;
use hibana::integration::program::{RoleProgram, project};

fn main() {
    let arm0 = g::seq(
        g::send::<0, 0, g::Msg<118, (), RouteDecisionKind>, 0>()
            .policy::<{ u16::MAX }>(),
        g::send::<0, 1, g::Msg<10, ()>, 0>(),
    );
    let arm1 = g::seq(
        g::send::<0, 0, g::Msg<119, (), RouteDecisionKind>, 0>().policy::<7>(),
        g::send::<0, 1, g::Msg<20, ()>, 0>(),
    );
    let route = g::route(arm0, arm1);
    let _: RoleProgram<1> = project(&route);
}
