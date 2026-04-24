#[path = "../support/control_kinds.rs"]
mod control_kinds;

use hibana::substrate::program::{RoleProgram, project};
use hibana::g::{self, Msg, Role};
use hibana::substrate::cap::GenericCapToken;

const ROUTE_ARM_WITH_POLICY_LABEL: u8 = 120;
const ROUTE_ARM_WITHOUT_POLICY_LABEL: u8 = 121;

type ArmWithPolicyKind =
    control_kinds::UnitControl<0x95, ROUTE_ARM_WITH_POLICY_LABEL, 7, 0x0400>;
type ArmWithoutPolicyKind =
    control_kinds::UnitControl<0x96, ROUTE_ARM_WITHOUT_POLICY_LABEL, 7, 0x0400>;

fn main() {
    let arm_with_policy = g::send::<
        Role<0>,
        Role<0>,
        Msg<ROUTE_ARM_WITH_POLICY_LABEL, GenericCapToken<ArmWithPolicyKind>, ArmWithPolicyKind>,
        0,
    >()
    .policy::<9>();
    let arm_without_policy = g::send::<
        Role<0>,
        Role<0>,
        Msg<
            ROUTE_ARM_WITHOUT_POLICY_LABEL,
            GenericCapToken<ArmWithoutPolicyKind>,
            ArmWithoutPolicyKind,
        >,
        0,
    >()
    .policy::<10>();
    let route = g::route(arm_with_policy, arm_without_policy);
    let _: RoleProgram<0> = project(&route);
}
