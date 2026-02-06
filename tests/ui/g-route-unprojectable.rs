//! Compile-time unprojectable route test.
//!
//! If a passive role cannot merge arms and cannot build a functional
//! label→continuation dispatch, the route is unprojectable unless a
//! dynamic plan is provided. This test verifies the compile-time panic.

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

// Data messages
// Both arms start with the same recv labels for Passive, but arm1 has an extra recv.
type DataMsg = g::Msg<42, ()>;
type SameMsg = g::Msg<99, ()>;
type ExtraMsg = g::Msg<77, ()>;

// Arm0: self-send control, then send DataMsg + SameMsg to Passive
// Passive projection: [Recv(42), Recv(99)]
type Arm0Steps = StepCons<
    SendStep<Controller, Controller, ControlArm0>,
    StepCons<SendStep<Controller, Passive, DataMsg>, StepCons<SendStep<Controller, Passive, SameMsg>, StepNil>>,
>;

// Arm1: self-send control, then send DataMsg + SameMsg + ExtraMsg to Passive
// Passive projection: [Recv(42), Recv(99), Recv(77)]
type Arm1Steps = StepCons<
    SendStep<Controller, Controller, ControlArm1>,
    StepCons<
        SendStep<Controller, Passive, DataMsg>,
        StepCons<SendStep<Controller, Passive, SameMsg>, StepCons<SendStep<Controller, Passive, ExtraMsg>, StepNil>>,
    >,
>;

type Steps = <Arm0Steps as StepConcat<Arm1Steps>>::Output;

const ARM0: g::Program<Arm0Steps> = g::seq(
    g::send::<Controller, Controller, ControlArm0, 0>(),
    g::send::<Controller, Passive, DataMsg, 0>().then(g::send::<Controller, Passive, SameMsg, 0>()),
);

const ARM1: g::Program<Arm1Steps> = g::seq(
    g::send::<Controller, Controller, ControlArm1, 0>(),
    g::send::<Controller, Passive, DataMsg, 0>()
        .then(g::send::<Controller, Passive, SameMsg, 0>())
        .then(g::send::<Controller, Passive, ExtraMsg, 0>()),
);

// No HandlePlan::dynamic here → unprojectable for Passive
const ROUTE: g::Program<Steps> = g::route::<0, _>(
    g::route_chain::<0, Arm0Steps>(ARM0).and::<Arm1Steps>(ARM1),
);

// Force evaluation by projecting to the passive role
type PassiveSteps = <Steps as ProjectRole<Passive>>::Output;

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
