mod common;
#[path = "support/runtime.rs"]
mod runtime_support;
#[path = "support/tls_ref.rs"]
mod tls_ref_support;

use core::cell::UnsafeCell;

use common::TestTransport;
use futures::FutureExt;
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
use runtime_support::with_runtime_workspace;
use tls_ref_support::with_resident_tls_ref;

type TestKitStorage = SessionKitStorage<'static, TestTransport>;

const ROUTE_RESOLVER: u16 = 77;
static ROUTE_STATE: () = ();

std::thread_local! {
    static SESSION_SLOT: UnsafeCell<TestKitStorage> = const {
        UnsafeCell::new(SessionKitStorage::uninit())
    };
}

fn endpoint_retention_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let program = g::send::<0, 1, Msg<1, u32>>();
    let program = g::seq(program, g::send::<0, 1, Msg<2, u32>>());
    let program = g::seq(program, g::send::<0, 1, Msg<3, u32>>());
    let program = g::seq(program, g::send::<0, 1, Msg<4, u32>>());
    let program = g::seq(program, g::send::<0, 1, Msg<5, u32>>());
    let program = g::seq(program, g::send::<0, 1, Msg<6, u32>>());
    let program = g::seq(program, g::send::<0, 1, Msg<7, u32>>());
    let program = g::seq(program, g::send::<0, 1, Msg<8, u32>>());
    let program = g::seq(program, g::send::<0, 1, Msg<9, u32>>());
    let program = g::seq(program, g::send::<0, 1, Msg<10, u32>>());
    let program = g::seq(program, g::send::<0, 1, Msg<13, u32>>());
    let program = g::seq(program, g::send::<0, 1, Msg<14, u32>>());
    let program = g::seq(program, g::send::<0, 1, Msg<15, u32>>());
    let program = g::seq(program, g::send::<0, 1, Msg<16, u32>>());
    let program = g::seq(program, g::send::<0, 1, Msg<17, u32>>());
    let program = g::seq(program, g::send::<0, 1, Msg<18, u32>>());
    let program = g::seq(program, g::send::<0, 1, Msg<19, u32>>());
    let program = g::seq(program, g::send::<0, 1, Msg<20, u32>>());
    let program = g::seq(program, g::send::<0, 1, Msg<21, u32>>());
    let program = g::seq(program, g::send::<0, 1, Msg<22, u32>>());
    project(&program)
}

fn local_route_program() -> RoleProgram<0> {
    project(
        &g::route(
            g::send::<0, 0, Msg<11, ()>>(),
            g::send::<0, 0, Msg<12, ()>>(),
        )
        .resolve::<ROUTE_RESOLVER>(),
    )
}

fn simple_route_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    project(
        &g::route(
            g::send::<0, 1, Msg<11, ()>>(),
            g::send::<0, 1, Msg<12, ()>>(),
        )
        .resolve::<ROUTE_RESOLVER>(),
    )
}

fn two_site_route_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let first = g::route(
        g::send::<0, 1, Msg<11, u32>>(),
        g::send::<0, 1, Msg<12, u32>>(),
    )
    .resolve::<ROUTE_RESOLVER>();
    let second = g::route(
        g::send::<0, 1, Msg<13, u32>>(),
        g::send::<0, 1, Msg<14, u32>>(),
    )
    .resolve::<ROUTE_RESOLVER>();
    project(&g::par(first, second))
}

fn nested_route_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let inner = g::route(
        g::send::<0, 1, Msg<42, u32>>(),
        g::send::<0, 1, Msg<43, u32>>(),
    );
    let outer_left = g::seq(g::send::<0, 1, Msg<41, u32>>(), inner);
    let outer_right = g::send::<0, 1, Msg<44, u32>>();
    project(&g::route(outer_left, outer_right))
}

fn choose_left(_: &()) -> Result<DecisionArm, ResolverError> {
    Ok(DecisionArm::Left)
}

fn reject(_: &()) -> Result<DecisionArm, ResolverError> {
    Err(ResolverError::reject())
}

fn resolver_result(event: tap::TapEvent) -> u16 {
    event.causal_key() & 0x00ff
}

fn resolver_route_site(event: tap::TapEvent) -> u32 {
    event.arg1() >> 16
}

fn resolver_id(event: tap::TapEvent) -> u32 {
    event.arg1() & 0xffff
}

#[test]
fn endpoint_events_do_not_emit_resolver_audit_and_retain_latest_twenty_one() {
    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        let events = with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let rv = cluster
                .rendezvous(slab, transport.clone())
                .expect("register rendezvous");
            let origin_program = endpoint_retention_program::<0>();
            let target_program = endpoint_retention_program::<1>();
            let sid = SessionId::new(0x0002_1000);
            let mut origin = rv.enter(sid, &origin_program).expect("origin endpoint");
            let mut target = rv.enter(sid, &target_program).expect("target endpoint");

            macro_rules! roundtrip {
                ($label:literal, $payload:expr) => {{
                    let payload = $payload;
                    futures::executor::block_on(origin.send::<Msg<$label, u32>>(&payload))
                        .expect("send succeeds");
                    let decoded = futures::executor::block_on(target.recv::<Msg<$label, u32>>())
                        .expect("recv succeeds");
                    assert_eq!(decoded, payload);
                }};
            }

            roundtrip!(1, 1);
            roundtrip!(2, 2);
            roundtrip!(3, 3);
            roundtrip!(4, 4);
            roundtrip!(5, 5);
            roundtrip!(6, 6);
            roundtrip!(7, 7);
            roundtrip!(8, 8);
            roundtrip!(9, 9);
            roundtrip!(10, 10);
            roundtrip!(13, 13);
            roundtrip!(14, 14);
            roundtrip!(15, 15);
            roundtrip!(16, 16);
            roundtrip!(17, 17);
            roundtrip!(18, 18);
            roundtrip!(19, 19);
            roundtrip!(20, 20);
            roundtrip!(21, 21);
            roundtrip!(22, 22);

            rv.tap().collect::<Vec<_>>()
        });

        assert_eq!(events.len(), 21, "tap must retain the latest 21 events");
        assert!(
            events.windows(2).all(|pair| pair[0].ts() < pair[1].ts()),
            "retained tap events must be returned oldest-to-newest: {events:?}"
        );
        let endpoint_events = events
            .iter()
            .filter(|event| {
                event.id() == tap::ENDPOINT_SEND
                    || event.id() == tap::ENDPOINT_RECV
                    || event.id() == tap::ENDPOINT_SESSION
            })
            .count();
        assert!(
            endpoint_events >= 10,
            "retained window must not be diluted by resolver replay audit: {events:?}"
        );
        assert!(
            events.iter().all(|event| event.id() != tap::RESOLVER_AUDIT),
            "plain endpoint send/recv must not emit resolver audit"
        );
    });
}

#[test]
fn dynamic_resolver_success_emits_one_reversible_audit() {
    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        let sid = SessionId::new(0x0004_0001);
        let events = with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let rv = cluster
                .rendezvous(slab, transport.clone())
                .expect("register rendezvous");
            let origin_program = simple_route_program::<0>();
            let target_program = simple_route_program::<1>();
            rv.set_resolver(
                &origin_program,
                ResolverRef::<ROUTE_RESOLVER>::decision_state(&ROUTE_STATE, choose_left),
            )
            .expect("install resolver");
            let mut origin = rv.enter(sid, &origin_program).expect("attach origin");
            let mut target = rv.enter(sid, &target_program).expect("attach target");

            futures::executor::block_on(async {
                origin
                    .send::<Msg<11, ()>>(&())
                    .await
                    .expect("left branch sends");
                target
                    .recv::<Msg<11, ()>>()
                    .await
                    .expect("left branch receives");
            });

            rv.tap().collect::<Vec<_>>()
        });

        let audits = events
            .iter()
            .copied()
            .filter(|event| event.id() == tap::RESOLVER_AUDIT)
            .collect::<Vec<_>>();
        assert_eq!(audits.len(), 1, "resolver success must emit one audit");
        assert_eq!(audits[0].arg0(), sid.raw());
        assert_eq!(resolver_id(audits[0]), ROUTE_RESOLVER as u32);
        assert_eq!(resolver_result(audits[0]), 0, "Left must encode as 0");
    });
}

#[test]
fn reused_resolver_id_keeps_route_site_in_audit() {
    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        let sid = SessionId::new(0x0004_0003);
        let events = with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let rv = cluster
                .rendezvous(slab, transport.clone())
                .expect("register rendezvous");
            let origin_program = two_site_route_program::<0>();
            let target_program = two_site_route_program::<1>();
            rv.set_resolver(
                &origin_program,
                ResolverRef::<ROUTE_RESOLVER>::decision_state(&ROUTE_STATE, choose_left),
            )
            .expect("install origin resolver for both route sites");
            rv.set_resolver(
                &target_program,
                ResolverRef::<ROUTE_RESOLVER>::decision_state(&ROUTE_STATE, choose_left),
            )
            .expect("install target resolver for both route sites");
            let mut origin = rv.enter(sid, &origin_program).expect("attach origin");
            let mut target = rv.enter(sid, &target_program).expect("attach target");

            futures::executor::block_on(origin.send::<Msg<11, u32>>(&11))
                .expect("send first route");
            let first = futures::executor::block_on(target.offer()).expect("first route");
            assert_eq!(
                futures::executor::block_on(first.recv::<Msg<11, u32>>())
                    .expect("first route commit"),
                11
            );
            futures::executor::block_on(origin.send::<Msg<13, u32>>(&13))
                .expect("send second route");

            rv.tap().collect::<Vec<_>>()
        });

        let selections = events
            .iter()
            .copied()
            .filter(|event| event.id() == tap::ROUTE_ARM_SELECTION)
            .collect::<Vec<_>>();
        let audits = events
            .iter()
            .copied()
            .filter(|event| event.id() == tap::RESOLVER_AUDIT)
            .collect::<Vec<_>>();
        assert!(
            audits.len() >= 2,
            "both route sites must emit resolver audit: {audits:?}"
        );

        assert!(
            selections
                .iter()
                .all(|event| resolver_id(*event) != ROUTE_RESOLVER as u32),
            "route selection arg1 must not pack resolver id in the low word: {selections:?}"
        );
        let mut audit_sites = audits
            .iter()
            .filter(|event| resolver_id(**event) == ROUTE_RESOLVER as u32)
            .map(|event| resolver_route_site(*event))
            .collect::<Vec<_>>();
        audit_sites.sort_unstable();
        audit_sites.dedup();
        assert!(
            audit_sites.len() >= 2,
            "resolver audit must carry distinct route sites: {audit_sites:?}"
        );
    });
}

#[test]
fn nested_route_runtime_emits_distinct_route_selection_sites() {
    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        let sid = SessionId::new(0x0004_0004);
        let events = with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let rv = cluster
                .rendezvous(slab, transport.clone())
                .expect("register rendezvous");
            let origin_program = nested_route_program::<0>();
            let target_program = nested_route_program::<1>();
            let mut origin = rv.enter(sid, &origin_program).expect("attach origin");
            let mut target = rv.enter(sid, &target_program).expect("attach target");

            futures::executor::block_on(origin.send::<Msg<41, u32>>(&41))
                .expect("send outer route");
            let outer = futures::executor::block_on(target.offer()).expect("offer outer route");
            assert_eq!(
                futures::executor::block_on(outer.recv::<Msg<41, u32>>())
                    .expect("outer route recv"),
                41
            );
            futures::executor::block_on(origin.send::<Msg<42, u32>>(&42))
                .expect("send inner route");
            let inner = futures::executor::block_on(target.offer()).expect("offer inner route");
            assert_eq!(
                futures::executor::block_on(inner.recv::<Msg<42, u32>>())
                    .expect("inner route recv"),
                42
            );

            rv.tap().collect::<Vec<_>>()
        });

        let mut selection_sites = events
            .iter()
            .copied()
            .filter(|event| event.id() == tap::ROUTE_ARM_SELECTION)
            .map(|event| event.arg1() >> 16)
            .collect::<Vec<_>>();
        selection_sites.sort_unstable();
        selection_sites.dedup();
        assert!(
            selection_sites.len() >= 2,
            "nested route runtime must emit distinct route selection sites: {events:?}"
        );
    });
}

#[test]
fn dynamic_resolver_reject_is_ready_error_with_one_audit() {
    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        let sid = SessionId::new(0x0004_0002);
        let (events, rendered) = with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let rv = cluster
                .rendezvous(slab, transport.clone())
                .expect("register rendezvous");
            let program = local_route_program();
            rv.set_resolver(
                &program,
                ResolverRef::<ROUTE_RESOLVER>::decision_state(&ROUTE_STATE, reject),
            )
            .expect("install resolver");
            let mut endpoint = rv.enter(sid, &program).expect("attach endpoint");

            let error = match endpoint
                .offer()
                .now_or_never()
                .expect("resolver reject must not park")
            {
                Ok(_) => panic!("resolver reject must fail"),
                Err(error) => error,
            };
            (rv.tap().collect::<Vec<_>>(), format!("{error:?}"))
        });

        assert!(
            rendered.contains("ResolverReject"),
            "resolver reject should remain the public error: {rendered}"
        );
        let audits = events
            .iter()
            .copied()
            .filter(|event| event.id() == tap::RESOLVER_AUDIT)
            .collect::<Vec<_>>();
        assert_eq!(audits.len(), 1, "resolver reject must emit one audit");
        assert_eq!(audits[0].arg0(), sid.raw());
        assert_eq!(resolver_id(audits[0]), ROUTE_RESOLVER as u32);
        assert_eq!(
            resolver_result(audits[0]),
            0xff,
            "Reject must encode as 0xff"
        );
    });
}
