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

fn choose_left(_: &()) -> Result<DecisionArm, ResolverError> {
    Ok(DecisionArm::Left)
}

fn reject(_: &()) -> Result<DecisionArm, ResolverError> {
    Err(ResolverError::reject())
}

fn resolver_result(event: tap::TapEvent) -> u32 {
    event.arg1() & 0xffff
}

fn resolver_id(event: tap::TapEvent) -> u32 {
    event.arg1() >> 16
}

#[test]
fn endpoint_events_do_not_emit_resolver_audit_and_retain_latest_thirty_two() {
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

        assert_eq!(events.len(), 32, "tap must retain the latest 32 events");
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
            endpoint_events >= 20,
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
            let program = local_route_program();
            rv.set_resolver(
                &program,
                ResolverRef::<ROUTE_RESOLVER>::decision_state(&ROUTE_STATE, choose_left),
            )
            .expect("install resolver");
            let mut endpoint = rv.enter(sid, &program).expect("attach endpoint");

            let branch = futures::executor::block_on(endpoint.offer()).expect("offer route");
            futures::executor::block_on(branch.recv::<Msg<11, ()>>()).expect("left branch commits");

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
