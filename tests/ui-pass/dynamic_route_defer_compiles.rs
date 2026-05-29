//! Dynamic route + defer surface should compile.

use hibana::integration::program::{RoleProgram, project};
use hibana::g::{self};
use hibana::integration::cap::control::RouteDecisionKind;
use hibana::integration::policy::DecisionResolution;

const ROUTE_ARM_LEFT_LABEL: u8 = 118;
const ROUTE_ARM_RIGHT_LABEL: u8 = 119;

const POLICY_ID: u16 = 77;

fn main() {
    let arm0 = g::seq(
        g::send::<
            g::Role<0>,
            g::Role<0>,
            g::Msg<ROUTE_ARM_LEFT_LABEL, (), RouteDecisionKind>,
            0,
        >()
        .policy::<POLICY_ID>(),
        g::send::<g::Role<0>, g::Role<1>, g::Msg<10, ()>, 0>(),
    );
    let arm1 = g::seq(
        g::send::<
            g::Role<0>,
            g::Role<0>,
            g::Msg<ROUTE_ARM_RIGHT_LABEL, (), RouteDecisionKind>,
            0,
        >()
        .policy::<POLICY_ID>(),
        g::send::<g::Role<0>, g::Role<1>, g::Msg<20, ()>, 0>(),
    );
    let route = g::route(arm0, arm1);
    let passive_program: RoleProgram<1> = project(&route);
    let _ = passive_program;
    let _ = DecisionResolution::Defer;
}
