#![allow(dead_code)]

use hibana::{
    control::{
        cap::{
            resource_kinds::CancelKind,
            GenericCapToken,
        },
        lease::planner::{LeaseFacetNeeds, assert_program_covers_facets},
    },
    g::{
        self, CanonicalControl, Msg, Role,
        steps::{ProjectRole, SendStep, StepCons, StepNil},
    },
    runtime::consts::LABEL_CANCEL,
};

type Controller = Role<0>;
type Worker = Role<1>;

type CancelMsg = Msg<{ LABEL_CANCEL }, GenericCapToken<CancelKind>, CanonicalControl<CancelKind>>;
// CanonicalControl requires self-send (From == To)
type Steps = StepCons<SendStep<Controller, Controller, CancelMsg, 0>, StepNil>;

const PROGRAM: g::Program<Steps> = g::send::<Controller, Controller, CancelMsg, 0>();

type ControllerLocal = <Steps as ProjectRole<Controller>>::Output;

static CONTROLLER_PROGRAM: g::RoleProgram<'static, 0, ControllerLocal> =
    g::project::<0, Steps, _>(&PROGRAM);

const NEEDS: LeaseFacetNeeds = LeaseFacetNeeds::new().with_caps();

const _: () = assert_program_covers_facets(&CONTROLLER_PROGRAM, NEEDS);

fn main() {}
