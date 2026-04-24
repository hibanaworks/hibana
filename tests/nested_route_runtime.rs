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

use ::core::{
    cell::UnsafeCell,
    mem::{MaybeUninit, size_of, size_of_val},
};

use common::TestTransport;
use hibana::g::{self, Msg, Role};
use hibana::substrate::program::{RoleProgram, project};
use hibana::substrate::{
    SessionKit,
    binding::NoBinding,
    ids::SessionId,
    runtime::{Config, CounterClock, DefaultLabelUniverse},
};
use hibana::substrate::{
    cap::{GenericCapToken, ResourceKind, advanced::RouteDecisionKind},
    ids::RendezvousId,
    policy::{ResolverContext, ResolverError, RouteResolution, core},
};
use placement_support::write_value;
use runtime_support::with_fixture;
use tls_mut_support::with_tls_mut;
use tls_ref_support::with_tls_ref;

const LABEL_ROUTE_DECISION: u8 = 57;
const LABEL_ROUTE_RIGHT_CONTROL: u8 = 118;

type RouteRightKind = route_control_kinds::RouteControl<LABEL_ROUTE_RIGHT_CONTROL, 0>;
const OUTER_ROUTE_POLICY_ID: u16 = 310;
const INNER_ROUTE_POLICY_ID: u16 = 311;
type TestKit = SessionKit<'static, TestTransport, DefaultLabelUniverse, CounterClock, 2>;
const ROUTE_BRANCH_BYTES_MAX: usize = 32;
const OFFER_FUTURE_BYTES_MAX: usize = 48;
const DECODE_FUTURE_BYTES_MAX: usize = 48;

std::thread_local! {
    static SESSION_SLOT: UnsafeCell<MaybeUninit<TestKit>> = const {
        UnsafeCell::new(MaybeUninit::uninit())
    };
    static CONTROLLER_ENDPOINT_SLOT: UnsafeCell<MaybeUninit<hibana::Endpoint<'static, 0>>> = const {
        UnsafeCell::new(MaybeUninit::uninit())
    };
    static WORKER_ENDPOINT_SLOT: UnsafeCell<MaybeUninit<hibana::Endpoint<'static, 1>>> = const {
        UnsafeCell::new(MaybeUninit::uninit())
    };
}

fn nested_route_resolver(ctx: ResolverContext) -> Result<RouteResolution, ResolverError> {
    let tag = ctx.attr(core::TAG).map(|value| value.as_u8());
    if tag != Some(RouteDecisionKind::TAG) && tag != Some(RouteRightKind::TAG) {
        return Err(ResolverError::Reject);
    }
    Ok(RouteResolution::Arm(0))
}

fn controller_program() -> RoleProgram<0> {
    let inner_route = g::route(
        g::seq(
            g::send::<
                Role<0>,
                Role<0>,
                Msg<
                    { LABEL_ROUTE_DECISION },
                    GenericCapToken<RouteDecisionKind>,
                    RouteDecisionKind,
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
                Msg<LABEL_ROUTE_RIGHT_CONTROL, GenericCapToken<RouteRightKind>, RouteRightKind>,
                0,
            >()
            .policy::<INNER_ROUTE_POLICY_ID>(),
            g::send::<Role<0>, Role<1>, Msg<8, u32>, 0>(),
        ),
    );

    let outer_left = g::seq(
        g::send::<
            Role<0>,
            Role<0>,
            Msg<{ LABEL_ROUTE_DECISION }, GenericCapToken<RouteDecisionKind>, RouteDecisionKind>,
            0,
        >()
        .policy::<OUTER_ROUTE_POLICY_ID>(),
        g::seq(g::send::<Role<0>, Role<1>, Msg<5, u32>, 0>(), inner_route),
    );

    let outer_right = g::seq(
        g::send::<
            Role<0>,
            Role<0>,
            Msg<LABEL_ROUTE_RIGHT_CONTROL, GenericCapToken<RouteRightKind>, RouteRightKind>,
            0,
        >()
        .policy::<OUTER_ROUTE_POLICY_ID>(),
        g::send::<Role<0>, Role<1>, Msg<6, u32>, 0>(),
    );

    let program = g::route(outer_left, outer_right);
    project(&program)
}

fn worker_program() -> RoleProgram<1> {
    let inner_route = g::route(
        g::seq(
            g::send::<
                Role<0>,
                Role<0>,
                Msg<
                    { LABEL_ROUTE_DECISION },
                    GenericCapToken<RouteDecisionKind>,
                    RouteDecisionKind,
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
                Msg<LABEL_ROUTE_RIGHT_CONTROL, GenericCapToken<RouteRightKind>, RouteRightKind>,
                0,
            >()
            .policy::<INNER_ROUTE_POLICY_ID>(),
            g::send::<Role<0>, Role<1>, Msg<8, u32>, 0>(),
        ),
    );

    let outer_left = g::seq(
        g::send::<
            Role<0>,
            Role<0>,
            Msg<{ LABEL_ROUTE_DECISION }, GenericCapToken<RouteDecisionKind>, RouteDecisionKind>,
            0,
        >()
        .policy::<OUTER_ROUTE_POLICY_ID>(),
        g::seq(g::send::<Role<0>, Role<1>, Msg<5, u32>, 0>(), inner_route),
    );

    let outer_right = g::seq(
        g::send::<
            Role<0>,
            Role<0>,
            Msg<LABEL_ROUTE_RIGHT_CONTROL, GenericCapToken<RouteRightKind>, RouteRightKind>,
            0,
        >()
        .policy::<OUTER_ROUTE_POLICY_ID>(),
        g::send::<Role<0>, Role<1>, Msg<6, u32>, 0>(),
    );

    let program = g::route(outer_left, outer_right);
    project(&program)
}

fn register_route_resolvers<const MAX_RV: usize>(
    cluster: &SessionKit<'_, TestTransport, DefaultLabelUniverse, CounterClock, MAX_RV>,
    rv_id: RendezvousId,
) {
    let controller_program = controller_program();
    cluster
        .set_resolver::<OUTER_ROUTE_POLICY_ID, 0>(
            rv_id,
            &controller_program,
            hibana::substrate::policy::ResolverRef::route_fn(nested_route_resolver),
        )
        .expect("register outer route resolver");
    cluster
        .set_resolver::<INNER_ROUTE_POLICY_ID, 0>(
            rv_id,
            &controller_program,
            hibana::substrate::policy::ResolverRef::route_fn(nested_route_resolver),
        )
        .expect("register inner route resolver");
}

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
                let controller_program = controller_program();
                let worker_program = worker_program();

                with_tls_mut(
                    &CONTROLLER_ENDPOINT_SLOT,
                    |ptr| unsafe {
                        write_value(
                            ptr,
                            cluster
                                .enter(rv_id, sid, &controller_program, NoBinding)
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
                                        .enter(rv_id, sid, &worker_program, NoBinding)
                                        .expect("attach worker"),
                                );
                            },
                            |worker| {
                                futures::executor::block_on(async {
                                    // =========================================================================
                                    // Outer route: Controller self-send control via flow().send(())
                                    // =========================================================================
                                    let _outer_token = controller
                                        .flow::<Msg<
                                            { LABEL_ROUTE_DECISION },
                                            GenericCapToken<RouteDecisionKind>,
                                            RouteDecisionKind,
                                        >>()
                                        .expect("outer left control flow")
                                        .send(())
                                        .await
                                        .expect("apply outer left control");

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
                                    let _inner_token = controller
                                        .flow::<Msg<
                                            { LABEL_ROUTE_DECISION },
                                            GenericCapToken<RouteDecisionKind>,
                                            RouteDecisionKind,
                                        >>()
                                        .expect("inner left control flow")
                                        .send(())
                                        .await
                                        .expect("apply inner left control");

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

#[test]
fn localside_offer_decode_sizes_stay_compact() {
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
                    .add_rendezvous_from_config(config, transport)
                    .expect("register rv");
                register_route_resolvers(cluster, rv_id);

                let sid = SessionId::new(78);
                let controller_program = controller_program();
                let worker_program = worker_program();

                with_tls_mut(
                    &CONTROLLER_ENDPOINT_SLOT,
                    |ptr| unsafe {
                        write_value(
                            ptr,
                            cluster
                                .enter(rv_id, sid, &controller_program, NoBinding)
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
                                        .enter(rv_id, sid, &worker_program, NoBinding)
                                        .expect("attach worker"),
                                );
                            },
                            |worker| {
                                let offer = worker.offer();
                                let offer_bytes = size_of_val(&offer);
                                drop(offer);

                                let _route_token = futures::executor::block_on(
                                    controller
                                        .flow::<Msg<
                                            { LABEL_ROUTE_DECISION },
                                            GenericCapToken<RouteDecisionKind>,
                                            RouteDecisionKind,
                                        >>()
                                        .expect("outer left control flow")
                                        .send(()),
                                )
                                .expect("apply outer left control");

                                let _outcome = futures::executor::block_on(
                                    controller
                                        .flow::<Msg<5, u32>>()
                                        .expect("outer left data flow")
                                        .send(&1234),
                                )
                                .expect("send outer left data");

                                let branch = futures::executor::block_on(worker.offer())
                                    .expect("offer route");
                                let branch_bytes =
                                    size_of::<hibana::RouteBranch<'static, 'static, 1>>();
                                assert_eq!(branch.label(), 5, "route should expose outer left arm");

                                let decode = branch.decode::<Msg<5, u32>>();
                                let decode_bytes = size_of_val(&decode);
                                drop(decode);

                                assert!(
                                    branch_bytes <= ROUTE_BRANCH_BYTES_MAX,
                                    "route branch handle regressed: {branch_bytes} > {ROUTE_BRANCH_BYTES_MAX}"
                                );
                                assert!(
                                    offer_bytes <= OFFER_FUTURE_BYTES_MAX,
                                    "offer future regressed: {offer_bytes} > {OFFER_FUTURE_BYTES_MAX}"
                                );
                                assert!(
                                    decode_bytes <= DECODE_FUTURE_BYTES_MAX,
                                    "decode future regressed: {decode_bytes} > {DECODE_FUTURE_BYTES_MAX}"
                                );
                            },
                        );
                    },
                );
            },
        );
    });
}
