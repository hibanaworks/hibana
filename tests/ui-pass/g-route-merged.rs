//! Route projection merge test (compile-pass).
//!
//! Both arms are identical for the passive role, so the route is mergeable
//! and should compile without requiring a resolver.

use hibana::control::cap::{
    CapError, CapShot, CapsMask, GenericCapToken, ResourceKind, RouteDecisionHandle,
    SessionScopedKind, resource_kinds::RouteDecisionKind,
};
use hibana::control::types::SessionId;
use hibana::g::{self, CanonicalControl, ControlHandling, SendStep, StepConcat, StepCons, StepNil};
use hibana::g::steps::ProjectRole;
use hibana::global::const_dsl::{ControlScopeKind, ScopeId};
use hibana::rendezvous::Lane;

type Controller = g::Role<0>;
type Passive = g::Role<1>;

// Control messages for self-send (different labels, so controller can distinguish)
type ControlArm0 = g::Msg<100, GenericCapToken<RouteArmKind<100>>, CanonicalControl<RouteArmKind<100>>>;
type ControlArm1 = g::Msg<101, GenericCapToken<RouteArmKind<101>>, CanonicalControl<RouteArmKind<101>>>;

// Data message (same label in both arms)
type DataMsg = g::Msg<42, ()>;

// Arm0: self-send control, then send DataMsg to Passive
type Arm0Steps = StepCons<
    SendStep<Controller, Controller, ControlArm0>,
    StepCons<SendStep<Controller, Passive, DataMsg>, StepNil>,
>;

// Arm1: same sequence as Arm0
// Passive projection is identical → mergeable
// (no resolver needed)
type Arm1Steps = StepCons<
    SendStep<Controller, Controller, ControlArm1>,
    StepCons<SendStep<Controller, Passive, DataMsg>, StepNil>,
>;

type Steps = <Arm0Steps as StepConcat<Arm1Steps>>::Output;

type PassiveSteps = <Steps as ProjectRole<Passive>>::Output;

const ARM0: g::Program<Arm0Steps> = g::seq(
    g::send::<Controller, Controller, ControlArm0, 0>(),
    g::send::<Controller, Passive, DataMsg, 0>(),
);

const ARM1: g::Program<Arm1Steps> = g::seq(
    g::send::<Controller, Controller, ControlArm1, 0>(),
    g::send::<Controller, Passive, DataMsg, 0>(),
);

const ROUTE: g::Program<Steps> = g::route::<0, _>(
    g::route_chain::<0, Arm0Steps>(ARM0).and::<Arm1Steps>(ARM1),
);

static PASSIVE_PROGRAM: hibana::g::RoleProgram<'static, 1, PassiveSteps> =
    g::project::<1, Steps, _>(&ROUTE);

fn main() {
    let _ = &*PASSIVE_PROGRAM;
}

// RouteArmKind boilerplate (same pattern as g-route-three-arms.rs)
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
