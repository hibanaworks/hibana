//! Route projection test (compile-pass).
//!
//! Static passive routing is allowed when route authority is carried by
//! explicit RouteDecision control messages.

#[path = "../support/control_kinds.rs"]
mod control_kinds;

use hibana::g::advanced::{CanonicalControl, RoleProgram, project};
use hibana::g::{self};
use hibana::substrate::cap::GenericCapToken;

type RouteArm100Kind = control_kinds::RouteControl<100, 0>;
type RouteArm101Kind = control_kinds::RouteControl<101, 0>;

fn main() {
    let arm0 = g::seq(
        g::send::<
            g::Role<0>,
            g::Role<0>,
            g::Msg<100, GenericCapToken<RouteArm100Kind>, CanonicalControl<RouteArm100Kind>>,
            0,
        >(),
        g::send::<g::Role<0>, g::Role<1>, g::Msg<10, ()>, 0>(),
    );
    let arm1 = g::seq(
        g::send::<
            g::Role<0>,
            g::Role<0>,
            g::Msg<101, GenericCapToken<RouteArm101Kind>, CanonicalControl<RouteArm101Kind>>,
            0,
        >(),
        g::send::<g::Role<0>, g::Role<1>, g::Msg<20, ()>, 0>(),
    );
    let route = g::route(arm0, arm1);
    let passive_program: RoleProgram<1> = project(&route);
    let _ = passive_program;
}
