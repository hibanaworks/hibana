use hibana::substrate::cap::{
    GenericCapToken,
};
use hibana::g::{self, Msg, Role};
use hibana::g::advanced::{CanonicalControl, RoleProgram, project};
use hibana::g::advanced::steps::{ProjectRole, SendStep, StepConcat, StepCons, StepNil};

hibana::impl_control_resource!(
    ArmWithPolicyKind,
    handle: Unit,
    tag: 0x95,
    name: "RouteArmWithPolicy",
    label: 5,
    scope: Route,
    tap_id: 0x0400,
    handling: Canonical,
);

hibana::impl_control_resource!(
    ArmWithoutPolicyKind,
    handle: Unit,
    tag: 0x96,
    name: "RouteArmWithoutPolicy",
    label: 6,
    scope: Route,
    tap_id: 0x0400,
    handling: Canonical,
);

const ARM_WITH_POLICY: g::Program<
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
const ARM_WITHOUT_POLICY: g::Program<
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

const ROUTE: g::Program<
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
    <<StepCons<
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
    >>::Output as ProjectRole<Role<0>>>::Output,
> = project(&ROUTE);

fn main() {}
