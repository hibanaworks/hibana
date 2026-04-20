#[path = "../support/control_kinds.rs"]
mod control_kinds;

use hibana::g::advanced::{CanonicalControl, RoleProgram, project};
use hibana::g::{self, Msg, Role};
use hibana::substrate::cap::GenericCapToken;

type ArmWithPolicyKind = control_kinds::UnitControl<0x95, 5, 7, 0x0400>;
type ArmWithoutPolicyKind = control_kinds::UnitControl<0x96, 6, 7, 0x0400>;

fn main() {
    let arm_with_policy = g::send::<
        Role<0>,
        Role<0>,
        Msg<5, GenericCapToken<ArmWithPolicyKind>, CanonicalControl<ArmWithPolicyKind>>,
        0,
    >()
    .policy::<9>();
    let arm_without_policy = g::send::<
        Role<0>,
        Role<0>,
        Msg<6, GenericCapToken<ArmWithoutPolicyKind>, CanonicalControl<ArmWithoutPolicyKind>>,
        0,
    >()
    .policy::<10>();
    let route = g::route(arm_with_policy, arm_without_policy);
    let _: RoleProgram<0> = project(&route);
}
