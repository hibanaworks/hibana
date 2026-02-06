#![cfg(feature = "std")]

mod common;
mod support;

use common::TestTransport;
use hibana::NoBinding;
use hibana::control::{
    cap::{
        CapError, CapShot, CapsMask, EpochInit, GenericCapToken, ResourceKind, RouteDecisionHandle,
        SessionScopedKind, resource_kinds::RouteDecisionKind,
    },
    cluster::{DynamicResolution, ResolverContext},
    types::RendezvousId,
};
use hibana::endpoint::{ControlOutcome, CursorEndpoint};
use hibana::g::steps::{ProjectRole, SendStep, StepConcat, StepCons, StepNil};
use hibana::g::{self, Msg, Role};
use hibana::global::const_dsl::{ControlScopeKind, DynamicMeta, HandlePlan, ScopeId};
use hibana::rendezvous::{Rendezvous, SessionId};
use hibana::runtime::{
    SessionCluster,
    config::{Config, CounterClock},
    consts::DefaultLabelUniverse,
};
use support::{leak_clock, leak_slab, leak_tap_storage};

type Controller = Role<0>;
type Worker = Role<1>;

// CanonicalControl requires self-send (From == To)
type OuterLeftControl = Msg<
    { hibana::runtime::consts::LABEL_ROUTE_DECISION },
    GenericCapToken<RouteDecisionKind>,
    g::CanonicalControl<RouteDecisionKind>,
>;
type OuterRightControl =
    Msg<11, GenericCapToken<RouteRightKind>, g::CanonicalControl<RouteRightKind>>;
type OuterLeftData = Msg<5, u32>;
type OuterRightData = Msg<6, u32>;
type InnerLeftControl = OuterLeftControl;
type InnerRightControl = OuterRightControl;
type InnerLeftData = Msg<7, u32>;
type InnerRightData = Msg<8, u32>;

// Self-send steps for CanonicalControl, data steps remain cross-role
type InnerLeftSteps = StepCons<
    SendStep<Controller, Controller, InnerLeftControl>,
    StepCons<SendStep<Controller, Worker, InnerLeftData>, StepNil>,
>;
type InnerRightSteps = StepCons<
    SendStep<Controller, Controller, InnerRightControl>,
    StepCons<SendStep<Controller, Worker, InnerRightData>, StepNil>,
>;
type InnerRouteSteps = <InnerLeftSteps as StepConcat<InnerRightSteps>>::Output;

type OuterLeftSteps = StepCons<
    SendStep<Controller, Controller, OuterLeftControl>,
    StepCons<SendStep<Controller, Worker, OuterLeftData>, InnerRouteSteps>,
>;
type OuterRightSteps = StepCons<
    SendStep<Controller, Controller, OuterRightControl>,
    StepCons<SendStep<Controller, Worker, OuterRightData>, StepNil>,
>;
type ProtocolSteps = <OuterLeftSteps as StepConcat<OuterRightSteps>>::Output;

const OUTER_ROUTE_POLICY_ID: u16 = 310;
const INNER_ROUTE_POLICY_ID: u16 = 311;
const ROUTE_PLAN_META: DynamicMeta = DynamicMeta::new();

fn nested_route_resolver(
    _cluster: &Cluster,
    _meta: &DynamicMeta,
    ctx: ResolverContext,
) -> Result<DynamicResolution, ()> {
    if ctx.tag != RouteDecisionKind::TAG && ctx.tag != RouteRightKind::TAG {
        return Err(());
    }
    Ok(DynamicResolution::RouteArm { arm: 0 })
}

fn register_route_resolvers(cluster: &Cluster, rv_id: RendezvousId) {
    for info in CONTROLLER_PROGRAM.control_plans() {
        if info.plan.is_dynamic() {
            cluster
                .register_control_plan_resolver(rv_id, &info, nested_route_resolver)
                .expect("register route resolver");
        }
    }
}

const INNER_ROUTE: g::Program<InnerRouteSteps> = g::route::<0, _>(
    g::route_chain::<0, InnerLeftSteps>(
        g::with_control_plan(
            g::send::<Controller, Controller, InnerLeftControl, 0>(),
            HandlePlan::dynamic(INNER_ROUTE_POLICY_ID, ROUTE_PLAN_META),
        )
        .then(g::send::<Controller, Worker, InnerLeftData, 0>()),
    )
    .and::<InnerRightSteps>(
        g::with_control_plan(
            g::send::<Controller, Controller, InnerRightControl, 0>(),
            HandlePlan::dynamic(INNER_ROUTE_POLICY_ID, ROUTE_PLAN_META),
        )
        .then(g::send::<Controller, Worker, InnerRightData, 0>()),
    ),
);

const OUTER_LEFT: g::Program<OuterLeftSteps> = g::with_control_plan(
    g::send::<Controller, Controller, OuterLeftControl, 0>(),
    HandlePlan::dynamic(OUTER_ROUTE_POLICY_ID, ROUTE_PLAN_META),
)
.then(g::send::<Controller, Worker, OuterLeftData, 0>())
.then(INNER_ROUTE);

const OUTER_RIGHT: g::Program<OuterRightSteps> = g::with_control_plan(
    g::send::<Controller, Controller, OuterRightControl, 0>(),
    HandlePlan::dynamic(OUTER_ROUTE_POLICY_ID, ROUTE_PLAN_META),
)
.then(g::send::<Controller, Worker, OuterRightData, 0>());

const PROGRAM: g::Program<ProtocolSteps> = g::route::<0, _>(
    g::route_chain::<0, OuterLeftSteps>(OUTER_LEFT).and::<OuterRightSteps>(OUTER_RIGHT),
);

static CONTROLLER_PROGRAM: g::RoleProgram<
    'static,
    0,
    <ProtocolSteps as ProjectRole<Controller>>::Output,
> = g::project::<0, ProtocolSteps, _>(&PROGRAM);

static WORKER_PROGRAM: g::RoleProgram<'static, 1, <ProtocolSteps as ProjectRole<Worker>>::Output> =
    g::project::<1, ProtocolSteps, _>(&PROGRAM);

type Cluster = SessionCluster<'static, TestTransport, DefaultLabelUniverse, CounterClock, 4>;

type ControllerEndpoint =
    CursorEndpoint<'static, 0, TestTransport, DefaultLabelUniverse, CounterClock, EpochInit, 4>;
type WorkerEndpoint =
    CursorEndpoint<'static, 1, TestTransport, DefaultLabelUniverse, CounterClock, EpochInit, 4>;

// Test nested routes with self-send control pattern via flow().send().
// Controller uses flow().send(()) for control decisions, Worker uses direct recv().
#[tokio::test]
async fn nested_branch_commit_stack() {
    let tap_buf = leak_tap_storage();
    let slab = leak_slab(2048);
    let config = Config::new(tap_buf, slab);
    let transport = TestTransport::default();
    let rendezvous: Rendezvous<'_, '_, TestTransport, DefaultLabelUniverse, CounterClock> =
        Rendezvous::from_config(config, transport.clone());

    let cluster: &mut Cluster = Box::leak(Box::new(SessionCluster::new(leak_clock())));
    let rv_id = cluster.add_rendezvous(rendezvous).expect("register rv");
    register_route_resolvers(&*cluster, rv_id);

    let sid = SessionId::new(77);

    let mut controller: ControllerEndpoint = cluster
        .attach_cursor::<0, _, _, _>(rv_id, sid, &CONTROLLER_PROGRAM, NoBinding)
        .expect("attach controller");
    let mut worker: WorkerEndpoint = cluster
        .attach_cursor::<1, _, _, _>(rv_id, sid, &WORKER_PROGRAM, NoBinding)
        .expect("attach worker");

    // =========================================================================
    // Outer route: Controller self-send control via flow().send(())
    // =========================================================================
    let (controller_after_outer_ctrl, outer_outcome) = controller
        .flow::<OuterLeftControl>()
        .expect("outer left control flow")
        .send(())
        .await
        .expect("apply outer left control");
    assert!(matches!(outer_outcome, ControlOutcome::Canonical(_)));
    controller = controller_after_outer_ctrl;

    // =========================================================================
    // Outer route: Controller sends wire data to Worker
    // =========================================================================
    let (controller_after_outer, _outcome) = controller
        .flow::<OuterLeftData>()
        .expect("outer left data flow")
        .send(&1234)
        .await
        .expect("send outer left data");
    controller = controller_after_outer;

    // =========================================================================
    // Outer route: Worker offers route arm, then decodes selected data
    // =========================================================================
    let outer_branch = worker.offer().await.expect("offer outer route");
    assert_eq!(
        outer_branch.label(),
        5,
        "outer route should expose OuterLeftData"
    );
    let (worker_after_outer, observed_outer) = outer_branch
        .decode::<OuterLeftData>()
        .await
        .expect("decode outer left data");
    assert_eq!(observed_outer, 1234);
    worker = worker_after_outer;

    // =========================================================================
    // Inner route: Controller self-send control via flow().send(())
    // =========================================================================
    let (controller_after_inner_ctrl, inner_outcome) = controller
        .flow::<InnerLeftControl>()
        .expect("inner left control flow")
        .send(())
        .await
        .expect("apply inner left control");
    assert!(matches!(inner_outcome, ControlOutcome::Canonical(_)));
    controller = controller_after_inner_ctrl;

    // =========================================================================
    // Inner route: Controller sends wire data to Worker
    // =========================================================================
    let (controller_after_inner, _outcome) = controller
        .flow::<InnerLeftData>()
        .expect("inner left data flow")
        .send(&5678)
        .await
        .expect("send inner left data");
    let _controller = controller_after_inner;

    // =========================================================================
    // Inner route: Worker offers route arm, then decodes selected data
    // =========================================================================
    let inner_branch = worker.offer().await.expect("offer inner route");
    assert_eq!(
        inner_branch.label(),
        7,
        "inner route should expose InnerLeftData"
    );
    let (worker_after_inner, observed_inner) = inner_branch
        .decode::<InnerLeftData>()
        .await
        .expect("decode inner left data");
    assert_eq!(observed_inner, 5678);
    let _worker = worker_after_inner;

    // Touch cursors so the final assignments are observed.
    #[cfg(feature = "test-utils")]
    let _ = controller.phase_cursor();
    #[cfg(feature = "test-utils")]
    let _ = worker.phase_cursor();
}

#[derive(Clone, Copy, Debug)]
struct RouteRightKind;

impl ResourceKind for RouteRightKind {
    type Handle = RouteDecisionHandle;
    const TAG: u8 = RouteDecisionKind::TAG;
    const NAME: &'static str = "RouteRightDecision";

    fn encode_handle(handle: &Self::Handle) -> [u8; hibana::control::cap::CAP_HANDLE_LEN] {
        handle.encode()
    }

    fn decode_handle(
        data: [u8; hibana::control::cap::CAP_HANDLE_LEN],
    ) -> Result<Self::Handle, CapError> {
        RouteDecisionHandle::decode(data)
    }

    fn zeroize(handle: &mut Self::Handle) {
        handle.arm = 0;
        handle.scope = ScopeId::generic(0);
    }

    fn caps_mask(_handle: &Self::Handle) -> CapsMask {
        CapsMask::empty()
    }
}

impl SessionScopedKind for RouteRightKind {
    fn handle_for_session(
        _sid: hibana::control::types::SessionId,
        _lane: hibana::rendezvous::Lane,
    ) -> Self::Handle {
        RouteDecisionHandle::default()
    }
}

impl hibana::control::cap::ControlResourceKind for RouteRightKind {
    const LABEL: u8 = 11;
    const SCOPE: ControlScopeKind = ControlScopeKind::Route;
    const TAP_ID: u16 = <RouteDecisionKind as hibana::control::cap::ControlResourceKind>::TAP_ID;
    const SHOT: CapShot = CapShot::One;
    const HANDLING: hibana::g::ControlHandling = hibana::g::ControlHandling::Canonical;
}

impl hibana::control::cap::ControlMint for RouteRightKind {
    fn mint_handle(
        _sid: hibana::rendezvous::SessionId,
        _lane: hibana::rendezvous::Lane,
        scope: hibana::global::const_dsl::ScopeId,
    ) -> Self::Handle {
        RouteDecisionHandle::new(scope, 0)
    }
}
