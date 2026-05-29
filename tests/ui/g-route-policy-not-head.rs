use hibana::{
    g::{self, Msg, Role},
    integration::{cap::control::RouteDecisionKind, program::{project, RoleProgram}},
};

fn main() {
    let left = g::seq(
        g::send::<Role<0>, Role<0>, Msg<92, (), RouteDecisionKind>, 0>(),
        g::send::<Role<0>, Role<0>, Msg<93, (), RouteDecisionKind>, 0>().policy::<7>(),
    );
    let right = g::seq(
        g::send::<Role<0>, Role<0>, Msg<94, (), RouteDecisionKind>, 0>(),
        g::send::<Role<0>, Role<0>, Msg<95, (), RouteDecisionKind>, 0>().policy::<7>(),
    );
    let program = g::route(left, right);
    let _: RoleProgram<0> = project(&program);
}
