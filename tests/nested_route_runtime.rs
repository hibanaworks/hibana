#![cfg(feature = "std")]

mod common;
#[path = "support/runtime.rs"]
mod runtime_support;

use common::TestTransport;
use hibana::g::advanced::steps::{ProjectRole, SendStep, SeqSteps, StepConcat, StepCons, StepNil};
use hibana::g::advanced::{CanonicalControl, RoleProgram, project};
use hibana::g::{self, Msg, Role};
use hibana::substrate::{
    RendezvousId,
    cap::{GenericCapToken, ResourceKind, advanced::RouteDecisionKind},
    policy::{DynamicResolution, ResolverContext, ResolverError, core},
};
use hibana::substrate::{
    SessionCluster, SessionId,
    binding::NoBinding,
    runtime::{Config, CounterClock, DefaultLabelUniverse},
};
use runtime_support::{leak_clock, leak_slab, leak_tap_storage};

const LABEL_ROUTE_DECISION: u8 = 57;

hibana::impl_control_resource!(
    RouteRightKind,
    handle: RouteDecision,
    name: "RouteRightDecision",
    label: 11,
);

// CanonicalControl requires self-send (From == To)
const OUTER_ROUTE_POLICY_ID: u16 = 310;
const INNER_ROUTE_POLICY_ID: u16 = 311;

fn nested_route_resolver(
    ctx: ResolverContext,
) -> Result<DynamicResolution, ResolverError> {
    let tag = ctx.attr(core::TAG).map(|value| value.as_u8());
    if tag != Some(RouteDecisionKind::TAG) && tag != Some(RouteRightKind::TAG) {
        return Err(ResolverError::Reject);
    }
    Ok(DynamicResolution::RouteArm { arm: 0 })
}

fn register_route_resolvers(
    cluster: &SessionCluster<'static, TestTransport, DefaultLabelUniverse, CounterClock, 4>,
    rv_id: RendezvousId,
) {
    cluster
        .set_resolver::<OUTER_ROUTE_POLICY_ID, 0, _, _>(
            rv_id,
            &CONTROLLER_PROGRAM,
            hibana::substrate::policy::ResolverRef::from_fn(nested_route_resolver),
        )
        .expect("register outer route resolver");
    cluster
        .set_resolver::<INNER_ROUTE_POLICY_ID, 0, _, _>(
            rv_id,
            &CONTROLLER_PROGRAM,
            hibana::substrate::policy::ResolverRef::from_fn(nested_route_resolver),
        )
        .expect("register inner route resolver");
}

const INNER_ROUTE: g::Program<
    <SeqSteps<
        StepCons<
            SendStep<
                Role<0>,
                Role<0>,
                Msg<
                    { LABEL_ROUTE_DECISION },
                    GenericCapToken<RouteDecisionKind>,
                    CanonicalControl<RouteDecisionKind>,
                >,
            >,
            StepNil,
        >,
        StepCons<SendStep<Role<0>, Role<1>, Msg<7, u32>>, StepNil>,
    > as StepConcat<
        SeqSteps<
            StepCons<
                SendStep<
                    Role<0>,
                    Role<0>,
                    Msg<11, GenericCapToken<RouteRightKind>, CanonicalControl<RouteRightKind>>,
                >,
                StepNil,
            >,
            StepCons<SendStep<Role<0>, Role<1>, Msg<8, u32>>, StepNil>,
        >,
    >>::Output,
> = g::route(
    g::seq(
        g::send::<
            Role<0>,
            Role<0>,
            Msg<
                { LABEL_ROUTE_DECISION },
                GenericCapToken<RouteDecisionKind>,
                CanonicalControl<RouteDecisionKind>,
            >,
            0,
        >()
        .policy::<INNER_ROUTE_POLICY_ID>(),
        g::send::<Role<0>, Role<1>, Msg<7, u32>, 0>(),
    ),
    g::seq(
        g::send::<
            Role<0>,
            Role<0>,
            Msg<11, GenericCapToken<RouteRightKind>, CanonicalControl<RouteRightKind>>,
            0,
        >()
        .policy::<INNER_ROUTE_POLICY_ID>(),
        g::send::<Role<0>, Role<1>, Msg<8, u32>, 0>(),
    ),
);

const OUTER_LEFT: g::Program<
    SeqSteps<
        StepCons<
            SendStep<
                Role<0>,
                Role<0>,
                Msg<
                    { LABEL_ROUTE_DECISION },
                    GenericCapToken<RouteDecisionKind>,
                    CanonicalControl<RouteDecisionKind>,
                >,
            >,
            StepNil,
        >,
        SeqSteps<
            StepCons<SendStep<Role<0>, Role<1>, Msg<5, u32>>, StepNil>,
            <SeqSteps<
                StepCons<
                    SendStep<
                        Role<0>,
                        Role<0>,
                        Msg<
                            { LABEL_ROUTE_DECISION },
                            GenericCapToken<RouteDecisionKind>,
                            CanonicalControl<RouteDecisionKind>,
                        >,
                    >,
                    StepNil,
                >,
                StepCons<SendStep<Role<0>, Role<1>, Msg<7, u32>>, StepNil>,
            > as StepConcat<
                SeqSteps<
                    StepCons<
                        SendStep<
                            Role<0>,
                            Role<0>,
                            Msg<
                                11,
                                GenericCapToken<RouteRightKind>,
                                CanonicalControl<RouteRightKind>,
                            >,
                        >,
                        StepNil,
                    >,
                    StepCons<SendStep<Role<0>, Role<1>, Msg<8, u32>>, StepNil>,
                >,
            >>::Output,
        >,
    >,
> = g::seq(
    g::send::<
        Role<0>,
        Role<0>,
        Msg<
            { LABEL_ROUTE_DECISION },
            GenericCapToken<RouteDecisionKind>,
            CanonicalControl<RouteDecisionKind>,
        >,
        0,
    >()
    .policy::<OUTER_ROUTE_POLICY_ID>(),
    g::seq(g::send::<Role<0>, Role<1>, Msg<5, u32>, 0>(), INNER_ROUTE),
);

const OUTER_RIGHT: g::Program<
    SeqSteps<
        StepCons<
            SendStep<
                Role<0>,
                Role<0>,
                Msg<11, GenericCapToken<RouteRightKind>, CanonicalControl<RouteRightKind>>,
            >,
            StepNil,
        >,
        StepCons<SendStep<Role<0>, Role<1>, Msg<6, u32>>, StepNil>,
    >,
> = g::seq(
    g::send::<
        Role<0>,
        Role<0>,
        Msg<11, GenericCapToken<RouteRightKind>, CanonicalControl<RouteRightKind>>,
        0,
    >()
    .policy::<OUTER_ROUTE_POLICY_ID>(),
    g::send::<Role<0>, Role<1>, Msg<6, u32>, 0>(),
);

const PROGRAM: g::Program<
    <SeqSteps<
        StepCons<
            SendStep<
                Role<0>,
                Role<0>,
                Msg<
                    { LABEL_ROUTE_DECISION },
                    GenericCapToken<RouteDecisionKind>,
                    CanonicalControl<RouteDecisionKind>,
                >,
            >,
            StepNil,
        >,
        SeqSteps<
            StepCons<SendStep<Role<0>, Role<1>, Msg<5, u32>>, StepNil>,
            <SeqSteps<
                StepCons<
                    SendStep<
                        Role<0>,
                        Role<0>,
                        Msg<
                            { LABEL_ROUTE_DECISION },
                            GenericCapToken<RouteDecisionKind>,
                            CanonicalControl<RouteDecisionKind>,
                        >,
                    >,
                    StepNil,
                >,
                StepCons<SendStep<Role<0>, Role<1>, Msg<7, u32>>, StepNil>,
            > as StepConcat<
                SeqSteps<
                    StepCons<
                        SendStep<
                            Role<0>,
                            Role<0>,
                            Msg<
                                11,
                                GenericCapToken<RouteRightKind>,
                                CanonicalControl<RouteRightKind>,
                            >,
                        >,
                        StepNil,
                    >,
                    StepCons<SendStep<Role<0>, Role<1>, Msg<8, u32>>, StepNil>,
                >,
            >>::Output,
        >,
    > as StepConcat<
        SeqSteps<
            StepCons<
                SendStep<
                    Role<0>,
                    Role<0>,
                    Msg<11, GenericCapToken<RouteRightKind>, CanonicalControl<RouteRightKind>>,
                >,
                StepNil,
            >,
            StepCons<SendStep<Role<0>, Role<1>, Msg<6, u32>>, StepNil>,
        >,
    >>::Output,
> = g::route(OUTER_LEFT, OUTER_RIGHT);

static CONTROLLER_PROGRAM: RoleProgram<
    'static,
    0,
    <<SeqSteps<
        StepCons<
            SendStep<
                Role<0>,
                Role<0>,
                Msg<
                    { LABEL_ROUTE_DECISION },
                    GenericCapToken<RouteDecisionKind>,
                    CanonicalControl<RouteDecisionKind>,
                >,
            >,
            StepNil,
        >,
        SeqSteps<
            StepCons<SendStep<Role<0>, Role<1>, Msg<5, u32>>, StepNil>,
            <SeqSteps<
                StepCons<
                    SendStep<
                        Role<0>,
                        Role<0>,
                        Msg<
                            { LABEL_ROUTE_DECISION },
                            GenericCapToken<RouteDecisionKind>,
                            CanonicalControl<RouteDecisionKind>,
                        >,
                    >,
                    StepNil,
                >,
                StepCons<SendStep<Role<0>, Role<1>, Msg<7, u32>>, StepNil>,
            > as StepConcat<
                SeqSteps<
                    StepCons<
                        SendStep<
                            Role<0>,
                            Role<0>,
                            Msg<
                                11,
                                GenericCapToken<RouteRightKind>,
                                CanonicalControl<RouteRightKind>,
                            >,
                        >,
                        StepNil,
                    >,
                    StepCons<SendStep<Role<0>, Role<1>, Msg<8, u32>>, StepNil>,
                >,
            >>::Output,
        >,
    > as StepConcat<
        SeqSteps<
            StepCons<
                SendStep<
                    Role<0>,
                    Role<0>,
                    Msg<11, GenericCapToken<RouteRightKind>, CanonicalControl<RouteRightKind>>,
                >,
                StepNil,
            >,
            StepCons<SendStep<Role<0>, Role<1>, Msg<6, u32>>, StepNil>,
        >,
    >>::Output as ProjectRole<Role<0>>>::Output,
> = project(&PROGRAM);

static WORKER_PROGRAM: RoleProgram<
    'static,
    1,
    <<SeqSteps<
        StepCons<
            SendStep<
                Role<0>,
                Role<0>,
                Msg<
                    { LABEL_ROUTE_DECISION },
                    GenericCapToken<RouteDecisionKind>,
                    CanonicalControl<RouteDecisionKind>,
                >,
            >,
            StepNil,
        >,
        SeqSteps<
            StepCons<SendStep<Role<0>, Role<1>, Msg<5, u32>>, StepNil>,
            <SeqSteps<
                StepCons<
                    SendStep<
                        Role<0>,
                        Role<0>,
                        Msg<
                            { LABEL_ROUTE_DECISION },
                            GenericCapToken<RouteDecisionKind>,
                            CanonicalControl<RouteDecisionKind>,
                        >,
                    >,
                    StepNil,
                >,
                StepCons<SendStep<Role<0>, Role<1>, Msg<7, u32>>, StepNil>,
            > as StepConcat<
                SeqSteps<
                    StepCons<
                        SendStep<
                            Role<0>,
                            Role<0>,
                            Msg<
                                11,
                                GenericCapToken<RouteRightKind>,
                                CanonicalControl<RouteRightKind>,
                            >,
                        >,
                        StepNil,
                    >,
                    StepCons<SendStep<Role<0>, Role<1>, Msg<8, u32>>, StepNil>,
                >,
            >>::Output,
        >,
    > as StepConcat<
        SeqSteps<
            StepCons<
                SendStep<
                    Role<0>,
                    Role<0>,
                    Msg<11, GenericCapToken<RouteRightKind>, CanonicalControl<RouteRightKind>>,
                >,
                StepNil,
            >,
            StepCons<SendStep<Role<0>, Role<1>, Msg<6, u32>>, StepNil>,
        >,
    >>::Output as ProjectRole<Role<1>>>::Output,
> = project(&PROGRAM);

// Test nested routes with self-send control pattern via flow().send().
// Controller uses flow().send(()) for control decisions, Worker uses direct recv().
#[tokio::test]
async fn nested_branch_commit_stack() {
    let tap_buf = leak_tap_storage();
    let slab = leak_slab(2048);
    let config = Config::new(tap_buf, slab);
    let transport = TestTransport::default();

    let cluster: &mut SessionCluster<
        'static,
        TestTransport,
        DefaultLabelUniverse,
        CounterClock,
        4,
    > = Box::leak(Box::new(SessionCluster::new(leak_clock())));
    let rv_id = cluster
        .add_rendezvous_from_config(config, transport.clone())
        .expect("register rv");
    register_route_resolvers(&*cluster, rv_id);

    let sid = SessionId::new(77);

    let mut controller = cluster
        .enter::<0, _, _, _>(rv_id, sid, &CONTROLLER_PROGRAM, NoBinding)
        .expect("attach controller");
    let mut worker = cluster
        .enter::<1, _, _, _>(rv_id, sid, &WORKER_PROGRAM, NoBinding)
        .expect("attach worker");

    // =========================================================================
    // Outer route: Controller self-send control via flow().send(())
    // =========================================================================
    let (controller_after_outer_ctrl, outer_outcome) = controller
        .flow::<Msg<
            { LABEL_ROUTE_DECISION },
            GenericCapToken<RouteDecisionKind>,
            CanonicalControl<RouteDecisionKind>,
        >>()
        .expect("outer left control flow")
        .send(())
        .await
        .expect("apply outer left control");
    assert!(outer_outcome.is_canonical());
    controller = controller_after_outer_ctrl;

    // =========================================================================
    // Outer route: Controller sends wire data to Worker
    // =========================================================================
    let (controller_after_outer, _outcome) = controller
        .flow::<Msg<5, u32>>()
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
        .decode::<Msg<5, u32>>()
        .await
        .expect("decode outer left data");
    assert_eq!(observed_outer, 1234);
    worker = worker_after_outer;

    // =========================================================================
    // Inner route: Controller self-send control via flow().send(())
    // =========================================================================
    let (controller_after_inner_ctrl, inner_outcome) = controller
        .flow::<Msg<
            { LABEL_ROUTE_DECISION },
            GenericCapToken<RouteDecisionKind>,
            CanonicalControl<RouteDecisionKind>,
        >>()
        .expect("inner left control flow")
        .send(())
        .await
        .expect("apply inner left control");
    assert!(inner_outcome.is_canonical());
    controller = controller_after_inner_ctrl;

    // =========================================================================
    // Inner route: Controller sends wire data to Worker
    // =========================================================================
    let (controller_after_inner, _outcome) = controller
        .flow::<Msg<7, u32>>()
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
        .decode::<Msg<7, u32>>()
        .await
        .expect("decode inner left data");
    assert_eq!(observed_inner, 5678);
    let _worker = worker_after_inner;
}
