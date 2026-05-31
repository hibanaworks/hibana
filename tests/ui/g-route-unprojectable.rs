//! Compile-time unprojectable route test.
//!
//! If a passive role cannot merge arms and cannot build a functional
//! frame-label continuation dispatch, the route is unprojectable unless a
//! dynamic policy is provided. This test verifies the compile-time panic.

use hibana::integration::cap::control::RouteDecisionKind;
use hibana::integration::program::{RoleProgram, project};
use hibana::g::{self};

const ROUTE_ARM_LEFT_LABEL: u8 = 118;
const ROUTE_ARM_RIGHT_LABEL: u8 = 119;


fn main() {
    let arm0 = g::seq(
        g::send::<
            0,
            0,
            g::Msg<ROUTE_ARM_LEFT_LABEL, (), RouteDecisionKind>,
            0,
        >(),
        g::seq(
            g::send::<0, 1, g::Msg<42, ()>, 0>(),
            g::send::<0, 1, g::Msg<99, ()>, 0>(),
        ),
    );
    let arm1 = g::seq(
        g::send::<
            0,
            0,
            g::Msg<ROUTE_ARM_RIGHT_LABEL, (), RouteDecisionKind>,
            0,
        >(),
        g::seq(
            g::send::<0, 1, g::Msg<42, ()>, 0>(),
            g::seq(
                g::send::<0, 1, g::Msg<99, ()>, 0>(),
                g::send::<0, 1, g::Msg<77, ()>, 0>(),
            ),
        ),
    );
    let route = g::route(arm0, arm1);
    let passive_program: RoleProgram<1> = project(&route);
    let _ = &passive_program;
}
