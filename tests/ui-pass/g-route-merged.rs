//! Route projection merge test (compile-pass).
//!
//! Both arms are identical for the passive role, so the route is mergeable
//! and should compile without requiring a resolver.

use hibana::integration::cap::control::RouteDecisionKind;
use hibana::integration::program::{RoleProgram, project};
use hibana::g::{self};

const ROUTE_ARM_LEFT_LABEL: u8 = 118;
const ROUTE_ARM_RIGHT_LABEL: u8 = 119;


fn main() {
    let arm0 = g::seq(
        g::send::<
            g::Role<0>,
            g::Role<0>,
            g::Msg<ROUTE_ARM_LEFT_LABEL, (), RouteDecisionKind>,
            0,
        >(),
        g::send::<g::Role<0>, g::Role<1>, g::Msg<42, ()>, 0>(),
    );
    let arm1 = g::seq(
        g::send::<
            g::Role<0>,
            g::Role<0>,
            g::Msg<ROUTE_ARM_RIGHT_LABEL, (), RouteDecisionKind>,
            0,
        >(),
        g::send::<g::Role<0>, g::Role<1>, g::Msg<42, ()>, 0>(),
    );
    let route = g::route(arm0, arm1);
    let passive_program: RoleProgram<1> = project(&route);
    let _ = &passive_program;
}
