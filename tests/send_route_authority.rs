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
    let right = g::send::<0, 2, Msg<SAME_LABEL, u32>>();
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
            let role2 = same_label_outbound_program::<2>();
            rv.set_resolver(
                &role0,
                ResolverRef::<DROP_ROUTE_RESOLVER>::decision_state(&UNIT, drop_left),
            )
            .expect("install drop resolver");
            let mut origin = rv.enter(sid, &role0).expect("attach origin");
            let left_peer = rv.enter(sid, &role1).expect("attach left peer");
            let right_peer = rv.enter(sid, &role2).expect("attach right peer");

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
            drop((left_peer, right_peer));
        });
    });
}
