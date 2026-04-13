#![cfg(feature = "std")]

mod common;
#[path = "support/placement.rs"]
mod placement_support;
#[path = "support/route_control_kinds.rs"]
mod route_control_kinds;
#[path = "support/runtime.rs"]
mod runtime_support;
#[path = "support/tls_mut.rs"]
mod tls_mut_support;
#[path = "support/tls_ref.rs"]
mod tls_ref_support;

use ::core::{cell::UnsafeCell, mem::MaybeUninit};

use common::TestTransport;
use hibana::g::advanced::steps::{PolicySteps, RouteSteps, SendStep, SeqSteps, StepCons, StepNil};
use hibana::g::advanced::{CanonicalControl, ProgramWitness, RoleProgram, project};
use hibana::g::{self, Msg, Role};
use hibana::substrate::{
    RendezvousId,
    cap::{GenericCapToken, ResourceKind, advanced::RouteDecisionKind},
    policy::{DynamicResolution, ResolverContext, ResolverError, core},
};
use hibana::substrate::{
    SessionId, SessionKit,
    binding::NoBinding,
    runtime::{Config, CounterClock, DefaultLabelUniverse},
};
use placement_support::write_value;
use runtime_support::with_fixture;
use tls_mut_support::with_tls_mut;
use tls_ref_support::with_tls_ref;

const LABEL_ROUTE_DECISION: u8 = 57;

type RouteRightKind = route_control_kinds::RouteControl<11, 0>;
type InnerLeftHead = PolicySteps<
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
    INNER_ROUTE_POLICY_ID,
>;
type InnerRightHead = PolicySteps<
    StepCons<
        SendStep<
            Role<0>,
            Role<0>,
            Msg<11, GenericCapToken<RouteRightKind>, CanonicalControl<RouteRightKind>>,
        >,
        StepNil,
    >,
    INNER_ROUTE_POLICY_ID,
>;
type InnerLeftArmSteps =
    SeqSteps<InnerLeftHead, StepCons<SendStep<Role<0>, Role<1>, Msg<7, u32>>, StepNil>>;
type InnerRightArmSteps =
    SeqSteps<InnerRightHead, StepCons<SendStep<Role<0>, Role<1>, Msg<8, u32>>, StepNil>>;
type InnerRouteProgramSteps = RouteSteps<InnerLeftArmSteps, InnerRightArmSteps>;
type OuterLeftHead = PolicySteps<
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
    OUTER_ROUTE_POLICY_ID,
>;
type OuterRightHead = PolicySteps<
    StepCons<
        SendStep<
            Role<0>,
            Role<0>,
            Msg<11, GenericCapToken<RouteRightKind>, CanonicalControl<RouteRightKind>>,
        >,
        StepNil,
    >,
    OUTER_ROUTE_POLICY_ID,
>;
type OuterLeftArmSteps = SeqSteps<
    OuterLeftHead,
    SeqSteps<StepCons<SendStep<Role<0>, Role<1>, Msg<5, u32>>, StepNil>, InnerRouteProgramSteps>,
>;
type OuterRightArmSteps =
    SeqSteps<OuterRightHead, StepCons<SendStep<Role<0>, Role<1>, Msg<6, u32>>, StepNil>>;
type ProgramSteps = RouteSteps<OuterLeftArmSteps, OuterRightArmSteps>;

// CanonicalControl requires self-send (From == To)
const OUTER_ROUTE_POLICY_ID: u16 = 310;
const INNER_ROUTE_POLICY_ID: u16 = 311;
type TestKit = SessionKit<'static, TestTransport, DefaultLabelUniverse, CounterClock, 2>;
type ControllerEndpoint = hibana::Endpoint<'static, 0, TestKit>;
type WorkerEndpoint = hibana::Endpoint<'static, 1, TestKit>;

std::thread_local! {
    static SESSION_SLOT: UnsafeCell<MaybeUninit<TestKit>> = const {
        UnsafeCell::new(MaybeUninit::uninit())
    };
    static CONTROLLER_ENDPOINT_SLOT: UnsafeCell<MaybeUninit<ControllerEndpoint>> = const {
        UnsafeCell::new(MaybeUninit::uninit())
    };
    static WORKER_ENDPOINT_SLOT: UnsafeCell<MaybeUninit<WorkerEndpoint>> = const {
        UnsafeCell::new(MaybeUninit::uninit())
    };
}

fn nested_route_resolver(ctx: ResolverContext) -> Result<DynamicResolution, ResolverError> {
    let tag = ctx.attr(core::TAG).map(|value| value.as_u8());
    if tag != Some(RouteDecisionKind::TAG) && tag != Some(RouteRightKind::TAG) {
        return Err(ResolverError::Reject);
    }
    Ok(DynamicResolution::RouteArm { arm: 0 })
}

fn register_route_resolvers<const MAX_RV: usize>(
    cluster: &SessionKit<'_, TestTransport, DefaultLabelUniverse, CounterClock, MAX_RV>,
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

const INNER_ROUTE: g::Program<InnerRouteProgramSteps> = g::route(
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

const OUTER_LEFT: g::Program<OuterLeftArmSteps> = g::seq(
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

const OUTER_RIGHT: g::Program<OuterRightArmSteps> = g::seq(
    g::send::<
        Role<0>,
        Role<0>,
        Msg<11, GenericCapToken<RouteRightKind>, CanonicalControl<RouteRightKind>>,
        0,
    >()
    .policy::<OUTER_ROUTE_POLICY_ID>(),
    g::send::<Role<0>, Role<1>, Msg<6, u32>, 0>(),
);

const PROGRAM: g::Program<ProgramSteps> = g::route(OUTER_LEFT, OUTER_RIGHT);

static CONTROLLER_PROGRAM: RoleProgram<'static, 0, ProgramWitness<ProgramSteps>> =
    project(&PROGRAM);

static WORKER_PROGRAM: RoleProgram<'static, 1, ProgramWitness<ProgramSteps>> = project(&PROGRAM);

// Test nested routes with self-send control pattern via flow().send().
// Controller uses flow().send(()) for control decisions, Worker uses direct recv().
#[test]
fn nested_branch_commit_stack() {
    with_fixture(|clock, tap_buf, slab| {
        with_tls_ref(
            &SESSION_SLOT,
            |ptr| unsafe {
                ptr.write(SessionKit::new(clock));
            },
            |cluster| {
                let config = Config::new(tap_buf, slab);
                let transport = TestTransport::default();
                let rv_id = cluster
                    .add_rendezvous_from_config(config, transport.clone())
                    .expect("register rv");
                register_route_resolvers(cluster, rv_id);

                let sid = SessionId::new(77);

                with_tls_mut(
                    &CONTROLLER_ENDPOINT_SLOT,
                    |ptr| unsafe {
                        write_value(
                            ptr,
                            cluster
                                .enter(rv_id, sid, &CONTROLLER_PROGRAM, NoBinding)
                                .expect("attach controller"),
                        );
                    },
                    |controller| {
                        with_tls_mut(
                            &WORKER_ENDPOINT_SLOT,
                            |ptr| unsafe {
                                write_value(
                                    ptr,
                                    cluster
                                        .enter(rv_id, sid, &WORKER_PROGRAM, NoBinding)
                                        .expect("attach worker"),
                                );
                            },
                            |worker| {
                                futures::executor::block_on(async {
                                    // =========================================================================
                                    // Outer route: Controller self-send control via flow().send(())
                                    // =========================================================================
                                    let outer_outcome = controller
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

                                    // =========================================================================
                                    // Outer route: Controller sends wire data to Worker
                                    // =========================================================================
                                    let _outcome = controller
                                        .flow::<Msg<5, u32>>()
                                        .expect("outer left data flow")
                                        .send(&1234)
                                        .await
                                        .expect("send outer left data");

                                    // =========================================================================
                                    // Outer route: Worker offers route arm, then decodes selected data
                                    // =========================================================================
                                    let outer_branch =
                                        worker.offer().await.expect("offer outer route");
                                    assert_eq!(
                                        outer_branch.label(),
                                        5,
                                        "outer route should expose OuterLeftData"
                                    );
                                    let observed_outer = outer_branch
                                        .decode::<Msg<5, u32>>()
                                        .await
                                        .expect("decode outer left data");
                                    assert_eq!(observed_outer, 1234);

                                    // =========================================================================
                                    // Inner route: Controller self-send control via flow().send(())
                                    // =========================================================================
                                    let inner_outcome = controller
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

                                    // =========================================================================
                                    // Inner route: Controller sends wire data to Worker
                                    // =========================================================================
                                    let _outcome = controller
                                        .flow::<Msg<7, u32>>()
                                        .expect("inner left data flow")
                                        .send(&5678)
                                        .await
                                        .expect("send inner left data");

                                    // =========================================================================
                                    // Inner route: Worker offers route arm, then decodes selected data
                                    // =========================================================================
                                    let inner_branch =
                                        worker.offer().await.expect("offer inner route");
                                    assert_eq!(
                                        inner_branch.label(),
                                        7,
                                        "inner route should expose InnerLeftData"
                                    );
                                    let observed_inner = inner_branch
                                        .decode::<Msg<7, u32>>()
                                        .await
                                        .expect("decode inner left data");
                                    assert_eq!(observed_inner, 5678);
                                })
                            },
                        );
                    },
                );
            },
        );
    });
}
