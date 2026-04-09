#[path = "../support/control_kinds.rs"]
mod control_kinds;

use hibana::substrate::cap::{
    GenericCapToken,
};
use hibana::g::{self, Msg, Role};
use hibana::g::advanced::{CanonicalControl, RoleProgram, project};
use hibana::g::advanced::steps::{SendStep, StepConcat, StepCons, StepNil};

type ArmWithPolicyKind = control_kinds::UnitControl<0x95, 5, 7, 0x0400>;
type ArmWithoutPolicyKind = control_kinds::UnitControl<0x96, 6, 7, 0x0400>;

const ARM_WITH_POLICY: g::ProgramSource<
    StepCons<
        SendStep<
            Role<0>,
            Role<0>,
            Msg<5, GenericCapToken<ArmWithPolicyKind>, CanonicalControl<ArmWithPolicyKind>>,
            0,
        >,
        StepNil,
    >,
> =
    g::send::<
        Role<0>,
        Role<0>,
        Msg<5, GenericCapToken<ArmWithPolicyKind>, CanonicalControl<ArmWithPolicyKind>>,
        0,
    >()
    .policy::<9>();
const ARM_WITHOUT_POLICY: g::ProgramSource<
    StepCons<
        SendStep<
            Role<0>,
            Role<0>,
            Msg<6, GenericCapToken<ArmWithoutPolicyKind>, CanonicalControl<ArmWithoutPolicyKind>>,
            0,
        >,
        StepNil,
    >,
> =
    g::send::<
        Role<0>,
        Role<0>,
        Msg<6, GenericCapToken<ArmWithoutPolicyKind>, CanonicalControl<ArmWithoutPolicyKind>>,
        0,
    >()
    .policy::<10>();

const ROUTE: g::ProgramSource<
    <StepCons<
        SendStep<
            Role<0>,
            Role<0>,
            Msg<5, GenericCapToken<ArmWithPolicyKind>, CanonicalControl<ArmWithPolicyKind>>,
            0,
        >,
        StepNil,
    > as StepConcat<
        StepCons<
            SendStep<
                Role<0>,
                Role<0>,
                Msg<6, GenericCapToken<ArmWithoutPolicyKind>, CanonicalControl<ArmWithoutPolicyKind>>,
                0,
            >,
            StepNil,
        >,
    >>::Output,
> = g::route(ARM_WITH_POLICY, ARM_WITHOUT_POLICY);

const CONTROLLER: RoleProgram<
    'static,
    0,
    <StepCons<
        SendStep<
            Role<0>,
            Role<0>,
            Msg<5, GenericCapToken<ArmWithPolicyKind>, CanonicalControl<ArmWithPolicyKind>>,
            0,
        >,
        StepNil,
    > as StepConcat<
        StepCons<
            SendStep<
                Role<0>,
                Role<0>,
                Msg<6, GenericCapToken<ArmWithoutPolicyKind>, CanonicalControl<ArmWithoutPolicyKind>>,
                0,
            >,
            StepNil,
        >,
    >>::Output,
> = project(&g::freeze(&ROUTE));

fn main() {}
