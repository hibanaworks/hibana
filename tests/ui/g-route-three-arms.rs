use hibana::control::cap::{
    CapError, CapShot, CapsMask, GenericCapToken, ResourceKind, RouteDecisionHandle,
    SessionScopedKind, resource_kinds::RouteDecisionKind,
};
use hibana::control::types::SessionId;
use hibana::g::{self, CanonicalControl, ControlHandling, SendStep, StepConcat, StepCons, StepNil};
use hibana::global::const_dsl::{ControlScopeKind, DynamicMeta, HandlePlan, ScopeId};
use hibana::rendezvous::Lane;

type Controller = g::Role<0>;

const ROUTE_POLICY_ID: u16 = 511;
const ROUTE_PLAN_META: DynamicMeta = DynamicMeta::new();

type Arm1Msg = g::Msg<3, GenericCapToken<RouteArmKind<3>>, CanonicalControl<RouteArmKind<3>>>;
type Arm2Msg = g::Msg<4, GenericCapToken<RouteArmKind<4>>, CanonicalControl<RouteArmKind<4>>>;
type Arm3Msg = g::Msg<5, GenericCapToken<RouteArmKind<5>>, CanonicalControl<RouteArmKind<5>>>;

type Arm1 = StepCons<SendStep<Controller, Controller, Arm1Msg>, StepNil>;
type Arm2 = StepCons<SendStep<Controller, Controller, Arm2Msg>, StepNil>;
type Arm3 = StepCons<SendStep<Controller, Controller, Arm3Msg>, StepNil>;

type Arms12 = <Arm1 as StepConcat<Arm2>>::Output;
type Steps = <Arms12 as StepConcat<Arm3>>::Output;

const ARM1: g::Program<Arm1> = g::with_control_plan(
    g::send::<Controller, Controller, Arm1Msg, 0>(),
    HandlePlan::dynamic(ROUTE_POLICY_ID, ROUTE_PLAN_META),
);
const ARM2: g::Program<Arm2> = g::with_control_plan(
    g::send::<Controller, Controller, Arm2Msg, 0>(),
    HandlePlan::dynamic(ROUTE_POLICY_ID, ROUTE_PLAN_META),
);
const ARM3: g::Program<Arm3> = g::with_control_plan(
    g::send::<Controller, Controller, Arm3Msg, 0>(),
    HandlePlan::dynamic(ROUTE_POLICY_ID, ROUTE_PLAN_META),
);

// 3-arm routes are not allowed - use nested binary routes instead
const ROUTE: g::Program<Steps> = {
    let builder = g::route_chain::<0, Arm1>(ARM1).and(ARM2).and(ARM3);
    g::route(builder)
};

fn main() {}

#[derive(Clone, Copy, Debug)]
struct RouteArmKind<const LABEL: u8>;

impl<const LABEL: u8> ResourceKind for RouteArmKind<LABEL> {
    type Handle = RouteDecisionHandle;
    const TAG: u8 = RouteDecisionKind::TAG;
    const NAME: &'static str = RouteDecisionKind::NAME;

    fn encode_handle(handle: &Self::Handle) -> [u8; hibana::control::cap::CAP_HANDLE_LEN] {
        handle.encode()
    }

    fn decode_handle(
        data: [u8; hibana::control::cap::CAP_HANDLE_LEN],
    ) -> Result<Self::Handle, CapError> {
        RouteDecisionHandle::decode(data)
    }

    fn zeroize(handle: &mut Self::Handle) {
        *handle = RouteDecisionHandle::new(ScopeId::none(), 0);
    }

    fn caps_mask(_handle: &Self::Handle) -> CapsMask {
        CapsMask::empty()
    }
}

impl<const LABEL: u8> SessionScopedKind for RouteArmKind<LABEL> {
    fn handle_for_session(_sid: SessionId, _lane: Lane) -> Self::Handle {
        RouteDecisionHandle::new(ScopeId::none(), 0)
    }
}

impl<const LABEL: u8> hibana::control::cap::ControlResourceKind for RouteArmKind<LABEL> {
    const LABEL: u8 = LABEL;
    const SCOPE: ControlScopeKind = ControlScopeKind::Route;
    const TAP_ID: u16 = <RouteDecisionKind as hibana::control::cap::ControlResourceKind>::TAP_ID;
    const SHOT: CapShot = CapShot::One;
    const HANDLING: ControlHandling = ControlHandling::Canonical;
}
