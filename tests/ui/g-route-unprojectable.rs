//! Compile-time unprojectable route test.
//!
//! If a passive role cannot merge arms and cannot build a functional
//! label→continuation dispatch, the route is unprojectable unless a
//! dynamic policy is provided. This test verifies the compile-time panic.

#[path = "../support/control_kinds.rs"]
mod control_kinds;

use hibana::substrate::program::{RoleProgram, project};
use hibana::g::{self};
use hibana::substrate::cap::GenericCapToken;

const ROUTE_ARM_LEFT_LABEL: u8 = 118;
const ROUTE_ARM_RIGHT_LABEL: u8 = 119;

type RouteArm100Kind = control_kinds::RouteControl<ROUTE_ARM_LEFT_LABEL, 0>;
type RouteArm101Kind = control_kinds::RouteControl<ROUTE_ARM_RIGHT_LABEL, 0>;

fn main() {
    let arm0 = g::seq(
        g::send::<
            g::Role<0>,
            g::Role<0>,
            g::Msg<ROUTE_ARM_LEFT_LABEL, GenericCapToken<RouteArm100Kind>, RouteArm100Kind>,
            0,
        >(),
        g::seq(
            g::send::<g::Role<0>, g::Role<1>, g::Msg<42, ()>, 0>(),
            g::send::<g::Role<0>, g::Role<1>, g::Msg<99, ()>, 0>(),
        ),
    );
    let arm1 = g::seq(
        g::send::<
            g::Role<0>,
            g::Role<0>,
            g::Msg<ROUTE_ARM_RIGHT_LABEL, GenericCapToken<RouteArm101Kind>, RouteArm101Kind>,
            0,
        >(),
        g::seq(
            g::send::<g::Role<0>, g::Role<1>, g::Msg<42, ()>, 0>(),
            g::seq(
                g::send::<g::Role<0>, g::Role<1>, g::Msg<99, ()>, 0>(),
                g::send::<g::Role<0>, g::Role<1>, g::Msg<77, ()>, 0>(),
            ),
        ),
    );
    let route = g::route(arm0, arm1);
    let passive_program: RoleProgram<1> = project(&route);
    let _ = &passive_program;
}
