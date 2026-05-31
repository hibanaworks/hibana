use hibana::integration::cap::control::RouteDecisionKind;
use hibana::integration::program::{RoleProgram, project};
use hibana::g::{self, Msg};

const ROUTE_ARM_WITH_POLICY_LABEL: u8 = 120;
const ROUTE_ARM_WITHOUT_POLICY_LABEL: u8 = 121;

fn main() {
    let arm_with_policy = g::send::<
        0,
        0,
        Msg<ROUTE_ARM_WITH_POLICY_LABEL, (), RouteDecisionKind>,
        0,
    >()
    .policy::<9>();
    let arm_without_policy = g::send::<
        0,
        0,
        Msg<
            ROUTE_ARM_WITHOUT_POLICY_LABEL,
            (),
            RouteDecisionKind,
        >,
        0,
    >()
    .policy::<10>();
    let route = g::route(arm_with_policy, arm_without_policy);
    let _: RoleProgram<0> = project(&route);
}
