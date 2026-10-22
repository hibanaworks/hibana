mod common;
#[path = "support/pending_cancel.rs"]
mod pending_cancel_support;
#[path = "support/runtime.rs"]
mod runtime_support;
#[path = "support/tls_ref.rs"]
mod tls_ref_support;

use core::{
    cell::UnsafeCell,
    future::Future,
    pin::pin,
    task::{Context, Poll},
};

use common::TestTransport;
use futures::task::noop_waker_ref;
use hibana::{
    g::{self, Msg},
    runtime::{
        SessionKitStorage,
        ids::SessionId,
        program::{RoleProgram, project},
        resolver::{DecisionArm, ResolverError, ResolverRef},
        tap,
    },
};
use pending_cancel_support::{PENDING_CANCEL_SESSION_SLOT, PendingCancelTransport};
use runtime_support::with_runtime_workspace;
use tls_ref_support::with_resident_tls_ref;

type TestKitStorage = SessionKitStorage<'static, TestTransport>;

const SAME_LABEL: u8 = 55;
const DROP_ROUTE_RESOLVER: u16 = 0x98;
const DEEP_ROUTE_RESOLVER_0: u16 = 0x99;
const DEEP_ROUTE_RESOLVER_1: u16 = 0x9a;
const DEEP_ROUTE_RESOLVER_2: u16 = 0x9b;
const DEEP_ROUTE_RESOLVER_3: u16 = 0x9c;
const DEEP_ROUTE_RESOLVER_4: u16 = 0x9d;
static LEFT_ARM: DecisionArm = DecisionArm::Left;
static UNIT: () = ();

#[test]
fn a_parallel_borrow_rejects_a_siblings_send_before_calling_any_resolver() {
    use core::cell::Cell;
    type A = Msg<210, ()>;
    type B = Msg<211, ()>;
    type C = Msg<212, ()>;
    type D = Msg<213, ()>;
    let global = g::par(
        g::route(g::send::<0, 1, A>(), g::send::<0, 1, B>()).resolve::<0>(),
        g::route(g::send::<0, 1, C>(), g::send::<0, 1, D>()).resolve::<1>(),
    )
    .roll();
    let sender_program = project::<0>(&global);
    let receiver_program = project::<1>(&global);
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            for visit in 0..2 {
                for borrow_right in [false, true] {
                    let calls = [Cell::new(0usize), Cell::new(0usize)];
                    let network = TestTransport::new();
                    let [mut slab0, mut slab1] = [[0; 32 * 1024]; 2];
                    let mut s0 = SessionKitStorage::uninit();
                    let mut s1 = SessionKitStorage::uninit();
                    let rv0 = s0.init().rendezvous(&mut slab0, network.clone()).unwrap();
                    let rv1 = s1.init().rendezvous(&mut slab1, network.clone()).unwrap();
                    rv0.set_resolver(
                        &sender_program,
                        ResolverRef::<0>::decision_state(&calls, |calls| {
                            calls[0].set(calls[0].get() + 1);
                            Ok(DecisionArm::Left)
                        }),
                    )
                    .unwrap();
                    rv0.set_resolver(
                        &sender_program,
                        ResolverRef::<1>::decision_state(&calls, |calls| {
                            calls[1].set(calls[1].get() + 1);
                            Ok(DecisionArm::Left)
                        }),
                    )
                    .unwrap();
                    let sid = SessionId::new(9290);
                    let mut sender = rv0.enter(sid, &sender_program).unwrap();
                    let mut receiver = rv1.enter(sid, &receiver_program).unwrap();
                    if visit == 1 {
                        let [mut a, mut b] = sender.par().unwrap();
                        let [mut c, mut d] = receiver.par().unwrap();
                        tokio::time::timeout(core::time::Duration::from_secs(1), async {
                            tokio::try_join!(
                                a.send::<A>(&()),
                                b.send::<C>(&()),
                                c.recv::<A>(),
                                d.recv::<C>()
                            )
                        })
                        .await
                        .expect("independent resolver branches stalled")
                        .unwrap();
                    }
                    assert_eq!([calls[0].get(), calls[1].get()], [visit, visit]);
                    let [mut a, mut b] = sender.par().unwrap();
                    let rejected =
                        tokio::time::timeout(core::time::Duration::from_secs(1), async {
                            if borrow_right {
                                b.send::<A>(&()).await
                            } else {
                                a.send::<C>(&()).await
                            }
                        })
                        .await
                        .expect("out-of-scope send must be rejected");
                    assert!(rejected.is_err());
                    assert_eq!(
                        [calls[0].get(), calls[1].get()],
                        [visit, visit],
                        "an out-of-scope call must not invoke either resolver"
                    );
                    assert!(network.queue_is_empty());
                }
            }
        });
}

std::thread_local! {
    static SESSION_SLOT: UnsafeCell<TestKitStorage> = const {
        UnsafeCell::new(SessionKitStorage::uninit())
    };
    static DROP_CALLS: UnsafeCell<usize> = const {
        UnsafeCell::new(0)
    };
}

fn reset_drop_calls() {
    DROP_CALLS.with(|cell| unsafe {
        *cell.get() = 0;
    });
}

fn bump_drop_calls() {
    DROP_CALLS.with(|cell| unsafe {
        *cell.get() += 1;
    });
}

fn drop_calls() -> usize {
    DROP_CALLS.with(|cell| unsafe { *cell.get() })
}

fn choose_arm(arm: &DecisionArm) -> Result<DecisionArm, ResolverError> {
    Ok(*arm)
}

fn drop_left(_: &()) -> Result<DecisionArm, ResolverError> {
    bump_drop_calls();
    Ok(DecisionArm::Left)
}

fn same_label_outbound_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let left = g::send::<0, 1, Msg<SAME_LABEL, u32>>();
    let right = g::send::<0, 1, Msg<SAME_LABEL, u64>>();
    project(&g::route(left, right).resolve::<DROP_ROUTE_RESOLVER>())
}

fn five_nested_resolver_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    project(
        &g::route(
            g::route(
                g::route(
                    g::route(
                        g::route(
                            g::send::<0, 1, Msg<56, u8>>(),
                            g::send::<0, 1, Msg<62, u8>>(),
                        )
                        .resolve::<DEEP_ROUTE_RESOLVER_4>(),
                        g::send::<0, 1, Msg<58, u8>>(),
                    )
                    .resolve::<DEEP_ROUTE_RESOLVER_3>(),
                    g::send::<0, 1, Msg<59, u8>>(),
                )
                .resolve::<DEEP_ROUTE_RESOLVER_2>(),
                g::send::<0, 1, Msg<60, u8>>(),
            )
            .resolve::<DEEP_ROUTE_RESOLVER_1>(),
            g::send::<0, 1, Msg<61, u8>>(),
        )
        .resolve::<DEEP_ROUTE_RESOLVER_0>(),
    )
}

fn resolver_audits(events: &[tap::TapEvent]) -> Vec<tap::TapEvent> {
    events
        .iter()
        .copied()
        .filter(|event| event.id() == tap::RESOLVER_AUDIT)
        .collect()
}

fn resolver_id(event: tap::TapEvent) -> u32 {
    event.arg1() & 0xffff
}

fn route_site(event: tap::TapEvent) -> u32 {
    event.arg1() >> 16
}

#[test]
fn five_nested_resolved_send_uses_route_rows_without_fixed_proof_cap() {
    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        let sid = SessionId::new(0x0009_2701);
        let events = with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let rv = cluster
                .rendezvous(slab, transport)
                .expect("register rendezvous");
            let role0 = five_nested_resolver_program::<0>();
            let role1 = five_nested_resolver_program::<1>();
            rv.set_resolver(
                &role0,
                ResolverRef::<DEEP_ROUTE_RESOLVER_0>::decision_state(&LEFT_ARM, choose_arm),
            )
            .expect("install outermost route resolver");
            rv.set_resolver(
                &role0,
                ResolverRef::<DEEP_ROUTE_RESOLVER_1>::decision_state(&LEFT_ARM, choose_arm),
            )
            .expect("install nested route resolver 1");
            rv.set_resolver(
                &role0,
                ResolverRef::<DEEP_ROUTE_RESOLVER_2>::decision_state(&LEFT_ARM, choose_arm),
            )
            .expect("install nested route resolver 2");
            rv.set_resolver(
                &role0,
                ResolverRef::<DEEP_ROUTE_RESOLVER_3>::decision_state(&LEFT_ARM, choose_arm),
            )
            .expect("install nested route resolver 3");
            rv.set_resolver(
                &role0,
                ResolverRef::<DEEP_ROUTE_RESOLVER_4>::decision_state(&LEFT_ARM, choose_arm),
            )
            .expect("install innermost route resolver");
            let mut origin = rv.enter(sid, &role0).expect("attach origin");
            let mut peer = rv.enter(sid, &role1).expect("attach peer");

            futures::executor::block_on(async {
                origin
                    .send::<Msg<56, u8>>(&56)
                    .await
                    .expect("five nested resolved send");
                let branch = peer.offer().await.expect("offer deep route");
                assert_eq!(
                    branch
                        .recv::<Msg<56, u8>>()
                        .await
                        .expect("deep nested recv"),
                    56
                );
            });

            rv.tap().collect::<Vec<_>>()
        });

        let mut ids = resolver_audits(&events)
            .into_iter()
            .map(resolver_id)
            .collect::<Vec<_>>();
        ids.sort_unstable();
        assert_eq!(
            ids,
            [
                DEEP_ROUTE_RESOLVER_0 as u32,
                DEEP_ROUTE_RESOLVER_1 as u32,
                DEEP_ROUTE_RESOLVER_2 as u32,
                DEEP_ROUTE_RESOLVER_3 as u32,
                DEEP_ROUTE_RESOLVER_4 as u32,
            ],
            "five nested route decisions must audit exactly once each: {events:?}"
        );
        let mut sites = resolver_audits(&events)
            .into_iter()
            .map(route_site)
            .collect::<Vec<_>>();
        sites.sort_unstable();
        sites.dedup();
        assert_eq!(
            sites.len(),
            5,
            "five nested route decisions must retain distinct route sites: {events:?}"
        );
    });
}

#[test]
fn dropping_pending_resolved_send_future_does_not_publish_success_evidence() {
    reset_drop_calls();
    with_runtime_workspace(|slab| {
        let transport = PendingCancelTransport::new();
        let cancel_count = transport.cancel_count();
        let transport_view = transport.clone();
        let sid = SessionId::new(0x0009_2702);
        with_resident_tls_ref(&PENDING_CANCEL_SESSION_SLOT, |cluster| {
            let rv = cluster
                .rendezvous(slab, transport)
                .expect("register rendezvous");
            let role0 = same_label_outbound_program::<0>();
            let role1 = same_label_outbound_program::<1>();
            rv.set_resolver(
                &role0,
                ResolverRef::<DROP_ROUTE_RESOLVER>::decision_state(&UNIT, drop_left),
            )
            .expect("install drop resolver");
            let mut origin = rv.enter(sid, &role0).expect("attach origin");
            let peer = rv.enter(sid, &role1).expect("attach peer");

            {
                let value = 2702u32;
                let mut send = pin!(origin.send::<Msg<SAME_LABEL, u32>>(&value));
                let waker = noop_waker_ref();
                let mut cx = Context::from_waker(waker);
                assert!(matches!(send.as_mut().poll(&mut cx), Poll::Pending));
            }

            let after_drop = rv.tap().collect::<Vec<_>>();
            assert_eq!(drop_calls(), 1, "first poll may decide the route once");
            assert_eq!(cancel_count.get(), 1, "pending send drop must cancel once");
            assert!(
                transport_view.queue_is_empty(),
                "cancelled pending send must not leave a staged frame"
            );
            assert!(
                after_drop.iter().all(|event| {
                    event.id() != tap::RESOLVER_AUDIT
                        && event.id() != tap::ROUTE_ARM_SELECTION
                        && event.id() != tap::ENDPOINT_SEND
                }),
                "dropping after transport pending must not publish success evidence: {after_drop:?}"
            );
            drop(peer);
        });
    });
}

#[test]
fn every_parallel_entry_checks_its_containing_resolver_before_sending() {
    type LeftFirst = Msg<201, ()>;
    type LeftParallel = Msg<202, ()>;
    type RightFirst = Msg<203, ()>;
    type RightParallel = Msg<204, ()>;
    let global = g::route(
        g::par(
            g::send::<0, 1, LeftFirst>(),
            g::send::<0, 2, LeftParallel>(),
        ),
        g::par(
            g::send::<0, 1, RightFirst>(),
            g::send::<0, 2, RightParallel>(),
        ),
    )
    .resolve::<317>();
    let owner_program = project::<0>(&global);
    let receiver_program = project::<2>(&global);
    let first_program = project::<1>(&global);
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async {
            for choice in [DecisionArm::Left, DecisionArm::Right] {
                for requested_right in [false, true] {
                    for first_entry in [false, true] {
                        let network = TestTransport::new();
                        let mut slab = [0; 32 * 1024];
                        let mut storage = SessionKitStorage::uninit();
                        let rv = storage
                            .init()
                            .rendezvous(&mut slab, network.clone())
                            .unwrap();
                        rv.set_resolver(
                            &owner_program,
                            ResolverRef::<317>::decision_state(&choice, choose_arm),
                        )
                        .unwrap();
                        let sid = SessionId::new(9278);
                        let mut owner = rv.enter(sid, &owner_program).unwrap();
                        let mut receiver = rv.enter(sid, &receiver_program).unwrap();
                        let mut first_receiver = rv.enter(sid, &first_program).unwrap();
                        let result = if first_entry {
                            if requested_right {
                                owner.send::<RightFirst>(&()).await
                            } else {
                                owner.send::<LeftFirst>(&()).await
                            }
                        } else if requested_right {
                            owner.send::<RightParallel>(&()).await
                        } else {
                            owner.send::<LeftParallel>(&()).await
                        };
                        let expected = requested_right == (choice == DecisionArm::Right);
                        assert_eq!(
                            result.is_ok(),
                            expected,
                            "{choice:?} right={requested_right} first={first_entry}: {result:?}"
                        );
                        if expected && first_entry {
                            if requested_right {
                                first_receiver.recv::<RightFirst>().await.unwrap();
                            } else {
                                first_receiver.recv::<LeftFirst>().await.unwrap();
                            }
                        } else if expected {
                            if requested_right {
                                receiver.recv::<RightParallel>().await.unwrap();
                            } else {
                                receiver.recv::<LeftParallel>().await.unwrap();
                            }
                        }
                        assert!(
                            network.queue_is_empty(),
                            "a rejected send must not enqueue a frame"
                        );
                    }
                }
            }
        });
}

#[test]
fn resolved_parents_can_commit_message_selected_children() {
    type A = Msg<211, ()>;
    type B = Msg<212, ()>;
    type C = Msg<213, ()>;
    type D = Msg<214, ()>;
    let global = g::route(
        g::route(g::send::<0, 1, A>(), g::send::<0, 1, B>()),
        g::route(g::send::<0, 1, C>(), g::send::<0, 1, D>()),
    )
    .resolve::<318>();
    let owner_program = project::<0>(&global);
    let receiver_program = project::<1>(&global);
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async {
            for choice in [DecisionArm::Left, DecisionArm::Right] {
                for right_child in [false, true] {
                    let network = TestTransport::new();
                    let mut slab = [0; 32 * 1024];
                    let mut storage = SessionKitStorage::uninit();
                    let rv = storage
                        .init()
                        .rendezvous(&mut slab, network.clone())
                        .unwrap();
                    rv.set_resolver(
                        &owner_program,
                        ResolverRef::<318>::decision_state(&choice, choose_arm),
                    )
                    .unwrap();
                    let sid = SessionId::new(9279);
                    let mut owner = rv.enter(sid, &owner_program).unwrap();
                    let mut receiver = rv.enter(sid, &receiver_program).unwrap();
                    match (choice, right_child) {
                        (DecisionArm::Left, false) => {
                            owner.send::<A>(&()).await.unwrap();
                            receiver.recv::<A>().await.unwrap();
                        }
                        (DecisionArm::Left, true) => {
                            owner.send::<B>(&()).await.unwrap();
                            receiver.recv::<B>().await.unwrap();
                        }
                        (DecisionArm::Right, false) => {
                            owner.send::<C>(&()).await.unwrap();
                            receiver.recv::<C>().await.unwrap();
                        }
                        (DecisionArm::Right, true) => {
                            owner.send::<D>(&()).await.unwrap();
                            receiver.recv::<D>().await.unwrap();
                        }
                    }
                    assert!(network.queue_is_empty());
                    let events = rv.tap().collect::<Vec<_>>();
                    assert_eq!(
                        resolver_audits(&events).len(),
                        1,
                        "only the parent has a resolver"
                    );
                }
            }
        });
}

#[test]
fn an_inner_roll_retains_its_enclosing_choice_until_the_outer_reentry() {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async {
            use core::cell::Cell;
            type Again = Msg<221, ()>;
            type Other = Msg<222, ()>;
            let global = g::route(g::send::<0, 1, Again>().roll(), g::send::<0, 1, Other>())
                .resolve::<319>()
                .roll();
            let p0 = project::<0>(&global);
            let p1 = project::<1>(&global);
            let calls = Cell::new(0);
            let network = TestTransport::new();
            let mut slab = [0; 32 * 1024];
            let mut storage = SessionKitStorage::uninit();
            let rv = storage
                .init()
                .rendezvous(&mut slab, network.clone())
                .unwrap();
            rv.set_resolver(
                &p0,
                ResolverRef::<319>::decision_state(&calls, |calls| {
                    let count = calls.get();
                    calls.set(count + 1);
                    Ok(if count == 0 {
                        DecisionArm::Left
                    } else {
                        DecisionArm::Right
                    })
                }),
            )
            .unwrap();
            let sid = SessionId::new(9280);
            let mut sender = rv.enter(sid, &p0).unwrap();
            let mut receiver = rv.enter(sid, &p1).unwrap();
            sender.send::<Again>(&()).await.unwrap();
            receiver.recv::<Again>().await.unwrap();
            sender
                .send::<Again>(&())
                .await
                .expect("the inner visit retains the outer decision");
            receiver.recv::<Again>().await.unwrap();
            assert_eq!(calls.get(), 1);
            sender.send::<Other>(&()).await.unwrap();
            receiver.recv::<Other>().await.unwrap();
            assert_eq!(calls.get(), 2);
            assert!(network.queue_is_empty());
        });
}
