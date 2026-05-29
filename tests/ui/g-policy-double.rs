use hibana::{
    g::{self, Msg, Role},
    integration::{cap::control::RouteDecisionKind, program::{project, RoleProgram}},
};

fn main() {
    let program = g::send::<Role<0>, Role<0>, Msg<91, (), RouteDecisionKind>, 0>()
        .policy::<7>()
        .policy::<8>();
    let _: RoleProgram<0> = project(&program);
}
