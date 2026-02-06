use hibana::control::cap::{
    CapError, CapShot, CapsMask, ControlResourceKind, GenericCapToken, ResourceKind,
    SessionScopedKind,
};
use hibana::g::{self, Msg, Role, SendStep, StepCons, StepNil};
use hibana::global::const_dsl::{DynamicMeta, ControlScopeKind};

type Controller = Role<0>;
type Target = Role<1>;

#[derive(Clone, Copy, Debug)]
struct ArmKind<const LABEL: u8>;

impl<const LABEL: u8> ResourceKind for ArmKind<LABEL> {
    type Handle = ();
    const TAG: u8 = 0x90 + LABEL;
    const NAME: &'static str = "RouteArm";

    fn encode_handle(_handle: &Self::Handle) -> [u8; hibana::control::cap::CAP_HANDLE_LEN] {
        [0u8; hibana::control::cap::CAP_HANDLE_LEN]
    }

    fn decode_handle(
        _data: [u8; hibana::control::cap::CAP_HANDLE_LEN],
    ) -> Result<Self::Handle, CapError> {
        Ok(())
    }

    fn zeroize(_handle: &mut Self::Handle) {}

    fn caps_mask(_handle: &Self::Handle) -> CapsMask {
        CapsMask::empty()
    }
}

impl<const LABEL: u8> SessionScopedKind for ArmKind<LABEL> {
    fn handle_for_session(
        _sid: hibana::control::types::SessionId,
        _lane: hibana::rendezvous::Lane,
    ) -> Self::Handle {
        ()
    }
}

impl<const LABEL: u8> ControlResourceKind for ArmKind<LABEL> {
    const LABEL: u8 = LABEL;
    const SCOPE: ControlScopeKind = ControlScopeKind::Route;
    const TAP_ID: u16 = 0x0400;
    const SHOT: CapShot = CapShot::One;
    const HANDLING: hibana::g::ControlHandling = hibana::g::ControlHandling::Canonical;
}

type WithPlanKind = ArmKind<5>;
type WithoutPlanKind = ArmKind<6>;

type RouteMsgWithPlan = Msg<
    5,
    GenericCapToken<WithPlanKind>,
    g::CanonicalControl<WithPlanKind>,
>;
type RouteMsgWithoutPlan = Msg<
    6,
    GenericCapToken<WithoutPlanKind>,
    g::CanonicalControl<WithoutPlanKind>,
>;

type WithPlanSteps = StepCons<SendStep<Controller, Target, RouteMsgWithPlan>, StepNil>;
type WithoutPlanSteps = StepCons<SendStep<Controller, Target, RouteMsgWithoutPlan>, StepNil>;

const ROUTE_PLAN: g::HandlePlan =
    g::HandlePlan::dynamic(9, DynamicMeta::new().with_static_weight(1));

const ARM_WITH_PLAN: g::Program<WithPlanSteps> = g::with_control_plan(
    g::send::<Controller, Target, RouteMsgWithPlan>(),
    ROUTE_PLAN,
);
const ARM_WITHOUT_PLAN: g::Program<WithoutPlanSteps> =
    g::send::<Controller, Target, RouteMsgWithoutPlan>();

const _: () = {
    let builder = g::route_chain::<0, WithPlanSteps>(ARM_WITH_PLAN).and::<WithoutPlanSteps>(ARM_WITHOUT_PLAN);
    let _ = g::route::<0, 1, _>(builder);
};

fn main() {}
