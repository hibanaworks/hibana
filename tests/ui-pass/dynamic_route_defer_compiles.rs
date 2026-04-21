//! Dynamic route + defer surface should compile.

#[path = "../support/control_kinds.rs"]
mod control_kinds;

use hibana::g::advanced::{RoleProgram, project};
use hibana::g::{self};
use hibana::substrate::cap::GenericCapToken;
use hibana::substrate::policy::DynamicResolution;

const ROUTE_ARM_LEFT_LABEL: u8 = 118;
const ROUTE_ARM_RIGHT_LABEL: u8 = 119;

type RouteArm100Kind = control_kinds::RouteControl<ROUTE_ARM_LEFT_LABEL, 0>;
type RouteArm101Kind = control_kinds::RouteControl<ROUTE_ARM_RIGHT_LABEL, 0>;

const POLICY_ID: u16 = 77;

fn main() {
    let arm0 = g::seq(
        g::send::<
            g::Role<0>,
            g::Role<0>,
            g::Msg<ROUTE_ARM_LEFT_LABEL, GenericCapToken<RouteArm100Kind>, RouteArm100Kind>,
            0,
        >()
        .policy::<POLICY_ID>(),
        g::send::<g::Role<0>, g::Role<1>, g::Msg<10, ()>, 0>(),
    );
    let arm1 = g::seq(
        g::send::<
            g::Role<0>,
            g::Role<0>,
            g::Msg<ROUTE_ARM_RIGHT_LABEL, GenericCapToken<RouteArm101Kind>, RouteArm101Kind>,
            0,
        >()
        .policy::<POLICY_ID>(),
        g::send::<g::Role<0>, g::Role<1>, g::Msg<20, ()>, 0>(),
    );
    let route = g::route(arm0, arm1);
    let passive_program: RoleProgram<1> = project(&route);
    let _ = passive_program;
    let _ = DynamicResolution::Defer { retry_hint: 1 };
}
