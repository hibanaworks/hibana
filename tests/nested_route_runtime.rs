#![cfg(feature = "std")]

mod common;
#[path = "support/placement.rs"]
mod placement_support;
#[path = "support/runtime.rs"]
mod runtime_support;
#[path = "support/tls_mut.rs"]
mod tls_mut_support;
#[path = "support/tls_ref.rs"]
mod tls_ref_support;

use ::core::{
    cell::UnsafeCell,
    future::Future,
    mem::{MaybeUninit, size_of, size_of_val},
    task::{Context, Poll},
};

use common::TestTransport;
use hibana::g::{self, Msg};
use hibana::runtime::program::{RoleProgram, project};
use hibana::runtime::{Config, SessionKitStorage, ids::SessionId};
use placement_support::write_value;
use runtime_support::with_runtime_workspace;
use tls_mut_support::with_tls_mut;
use tls_ref_support::with_resident_tls_ref;

type TestKitStorage = SessionKitStorage<'static, TestTransport>;
const ROUTE_BRANCH_BYTES_MAX: usize = 32;
const OFFER_FUTURE_BYTES_MAX: usize = 48;
const DECODE_FUTURE_BYTES_MAX: usize = 48;

std::thread_local! {
    static SESSION_SLOT: UnsafeCell<TestKitStorage> = const {
        UnsafeCell::new(SessionKitStorage::uninit())
    };
    static CONTROLLER_ENDPOINT_SLOT: UnsafeCell<MaybeUninit<hibana::Endpoint<'static, 0>>> = const {
        UnsafeCell::new(MaybeUninit::uninit())
    };
    static WORKER_ENDPOINT_SLOT: UnsafeCell<MaybeUninit<hibana::Endpoint<'static, 1>>> = const {
        UnsafeCell::new(MaybeUninit::uninit())
    };
}

fn controller_program() -> RoleProgram<0> {
    let inner_route = g::route(
        g::send::<0, 1, Msg<7, u32>>(),
        g::send::<0, 1, Msg<8, u32>>(),
    );

    let outer_left = g::seq(g::send::<0, 1, Msg<5, u32>>(), inner_route);

    let outer_right = g::send::<0, 1, Msg<6, u32>>();

    let program = g::route(outer_left, outer_right);
    project(&program)
}

fn worker_program() -> RoleProgram<1> {
    let inner_route = g::route(
        g::send::<0, 1, Msg<7, u32>>(),
        g::send::<0, 1, Msg<8, u32>>(),
    );

    let outer_left = g::seq(g::send::<0, 1, Msg<5, u32>>(), inner_route);

    let outer_right = g::send::<0, 1, Msg<6, u32>>();

    let program = g::route(outer_left, outer_right);
    project(&program)
}

// Test nested first-visible routes.
#[test]
fn nested_branch_commit_stack() {
    with_runtime_workspace(|slab| {
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let config = Config::from_resources(slab);
            let transport = TestTransport::new();
            let rv = cluster
                .rendezvous(config, transport.clone())
                .expect("register rv");

            let sid = SessionId::new(77);
            let controller_program = controller_program();
            let worker_program = worker_program();

            with_tls_mut(
                &CONTROLLER_ENDPOINT_SLOT,
                |ptr| unsafe {
                    write_value(
                        ptr,
                        rv.session(sid)
                            .role(&controller_program)
                            .enter()
                            .expect("attach controller"),
                    );
                },
                |controller| {
                    with_tls_mut(
                        &WORKER_ENDPOINT_SLOT,
                        |ptr| unsafe {
                            write_value(
                                ptr,
                                rv.session(sid)
                                    .role(&worker_program)
                                    .enter()
                                    .expect("attach worker"),
                            );
                        },
                        |worker| {
                            futures::executor::block_on(async {
                                // =========================================================================
                                // Outer route: Controller sends wire data to Worker
                                // =========================================================================
                                controller
                                    .send::<Msg<5, u32>>(&1234)
                                    .await
                                    .expect("send outer left data");

                                // =========================================================================
                                // Outer route: Worker offers route arm, then decodes selected data
                                // =========================================================================
                                let outer_branch = worker.offer().await.expect("offer outer route");
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
                                // Inner route: Controller sends wire data to Worker
                                // =========================================================================
                                controller
                                    .send::<Msg<7, u32>>(&5678)
                                    .await
                                    .expect("send inner left data");

                                // =========================================================================
                                // Inner route: Worker offers route arm, then decodes selected data
                                // =========================================================================
                                let inner_branch = worker.offer().await.expect("offer inner route");
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
        });
    });
}

#[test]
fn forgotten_started_offer_future_leaves_endpoint_fail_closed() {
    with_runtime_workspace(|slab| {
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let config = Config::from_resources(slab);
            let transport = TestTransport::new();
            let rv = cluster.rendezvous(config, transport).expect("register rv");

            let sid = SessionId::new(79);
            let controller_program = controller_program();
            let worker_program = worker_program();

            with_tls_mut(
                &CONTROLLER_ENDPOINT_SLOT,
                |ptr| unsafe {
                    write_value(
                        ptr,
                        rv.session(sid)
                            .role(&controller_program)
                            .enter()
                            .expect("attach controller"),
                    );
                },
                |controller| {
                    core::hint::black_box(controller);
                    with_tls_mut(
                        &WORKER_ENDPOINT_SLOT,
                        |ptr| unsafe {
                            write_value(
                                ptr,
                                rv.session(sid)
                                    .role(&worker_program)
                                    .enter()
                                    .expect("attach worker"),
                            );
                        },
                        |worker| {
                            let mut offer = Box::pin(worker.offer());
                            let waker = futures::task::noop_waker();
                            let mut cx = Context::from_waker(&waker);
                            if let Poll::Ready(result) = Future::poll(offer.as_mut(), &mut cx) {
                                match result {
                                    Ok(_) => {
                                        panic!("offer should wait before the route decision")
                                    }
                                    Err(error) => {
                                        panic!("offer failed before the route decision: {error:?}")
                                    }
                                }
                            }
                            core::mem::forget(offer);

                            let error = match futures::executor::block_on(worker.offer()) {
                                Ok(_) => panic!(
                                    "forgotten pending offer must leave endpoint fail-closed"
                                ),
                                Err(error) => error,
                            };
                            assert!(format!("{error:?}").contains("operation: \"offer\""));
                            let rendered = format!("{error:?}");
                            assert!(
                                rendered.contains("PhaseInvariant")
                                    || rendered.contains("ProgressInvariantViolated")
                                    || rendered.contains("SessionFault"),
                                "busy endpoint must preserve terminal progress evidence: {rendered}"
                            );
                        },
                    )
                },
            );
        });
    });
}

#[test]
fn localside_offer_decode_sizes_stay_compact() {
    with_runtime_workspace(|slab| {
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let config = Config::from_resources(slab);
            let transport = TestTransport::new();
            let rv = cluster.rendezvous(config, transport).expect("register rv");

            let sid = SessionId::new(78);
            let controller_program = controller_program();
            let worker_program = worker_program();

            with_tls_mut(
                &CONTROLLER_ENDPOINT_SLOT,
                |ptr| unsafe {
                    write_value(
                        ptr,
                        rv.session(sid)
                            .role(&controller_program)
                            .enter()
                            .expect("attach controller"),
                    );
                },
                |controller| {
                    with_tls_mut(
                        &WORKER_ENDPOINT_SLOT,
                        |ptr| unsafe {
                            write_value(
                                ptr,
                                rv.session(sid)
                                    .role(&worker_program)
                                    .enter()
                                    .expect("attach worker"),
                            );
                        },
                        |worker| {
                            let offer = worker.offer();
                            let offer_bytes = size_of_val(&offer);
                            drop(offer);

                            futures::executor::block_on(controller.send::<Msg<5, u32>>(&1234))
                                .expect("send outer left data");

                            let branch =
                                futures::executor::block_on(worker.offer()).expect("offer route");
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
        });
    });
}
