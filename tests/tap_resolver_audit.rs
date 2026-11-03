mod common;
#[path = "../examples/support/in_process.rs"]
pub mod in_process;

use common::TestTransport;
use core::{
    future::Future,
    pin::pin,
    task::{Context, Poll, Waker},
};
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
const ROUTE_RESOLVER: u16 = 77;
static ROUTE_STATE: () = ();

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

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn endpoint_events_do_not_emit_resolver_audit_and_retain_latest_twenty_one() {
    {
        let mut slab = [0; 64 * 1024];
        let transport = TestTransport::new();
        let events = {
            let mut storage = SessionKitStorage::uninit();
            let cluster = storage.init();
            let rv = cluster
                .rendezvous(&mut slab, transport.clone())
                .expect("register rendezvous");
            let origin_program = endpoint_retention_program::<0>();
            let target_program = endpoint_retention_program::<1>();
            let sid = SessionId::new(0x0002_1000);
            let mut origin = rv.enter(sid, &origin_program).expect("origin endpoint");
            let mut target = rv.enter(sid, &target_program).expect("target endpoint");

            origin.send::<Msg<1, u32>>(&1).await.expect("send succeeds");
            assert_eq!(
                target.recv::<Msg<1, u32>>().await.expect("recv succeeds"),
                1
            );
            origin.send::<Msg<2, u32>>(&2).await.expect("send succeeds");
            assert_eq!(
                target.recv::<Msg<2, u32>>().await.expect("recv succeeds"),
                2
            );
            origin.send::<Msg<3, u32>>(&3).await.expect("send succeeds");
            assert_eq!(
                target.recv::<Msg<3, u32>>().await.expect("recv succeeds"),
                3
            );
            origin.send::<Msg<4, u32>>(&4).await.expect("send succeeds");
            assert_eq!(
                target.recv::<Msg<4, u32>>().await.expect("recv succeeds"),
                4
            );
            origin.send::<Msg<5, u32>>(&5).await.expect("send succeeds");
            assert_eq!(
                target.recv::<Msg<5, u32>>().await.expect("recv succeeds"),
                5
            );
            origin.send::<Msg<6, u32>>(&6).await.expect("send succeeds");
            assert_eq!(
                target.recv::<Msg<6, u32>>().await.expect("recv succeeds"),
                6
            );
            origin.send::<Msg<7, u32>>(&7).await.expect("send succeeds");
            assert_eq!(
                target.recv::<Msg<7, u32>>().await.expect("recv succeeds"),
                7
            );
            origin.send::<Msg<8, u32>>(&8).await.expect("send succeeds");
            assert_eq!(
                target.recv::<Msg<8, u32>>().await.expect("recv succeeds"),
                8
            );
            origin.send::<Msg<9, u32>>(&9).await.expect("send succeeds");
            assert_eq!(
                target.recv::<Msg<9, u32>>().await.expect("recv succeeds"),
                9
            );
            origin
                .send::<Msg<10, u32>>(&10)
                .await
                .expect("send succeeds");
            assert_eq!(
                target.recv::<Msg<10, u32>>().await.expect("recv succeeds"),
                10
            );
            origin
                .send::<Msg<13, u32>>(&13)
                .await
                .expect("send succeeds");
            assert_eq!(
                target.recv::<Msg<13, u32>>().await.expect("recv succeeds"),
                13
            );
            origin
                .send::<Msg<14, u32>>(&14)
                .await
                .expect("send succeeds");
            assert_eq!(
                target.recv::<Msg<14, u32>>().await.expect("recv succeeds"),
                14
            );
            origin
                .send::<Msg<15, u32>>(&15)
                .await
                .expect("send succeeds");
            assert_eq!(
                target.recv::<Msg<15, u32>>().await.expect("recv succeeds"),
                15
            );
            origin
                .send::<Msg<16, u32>>(&16)
                .await
                .expect("send succeeds");
            assert_eq!(
                target.recv::<Msg<16, u32>>().await.expect("recv succeeds"),
                16
            );
            origin
                .send::<Msg<17, u32>>(&17)
                .await
                .expect("send succeeds");
            assert_eq!(
                target.recv::<Msg<17, u32>>().await.expect("recv succeeds"),
                17
            );
            origin
                .send::<Msg<18, u32>>(&18)
                .await
                .expect("send succeeds");
            assert_eq!(
                target.recv::<Msg<18, u32>>().await.expect("recv succeeds"),
                18
            );
            origin
                .send::<Msg<19, u32>>(&19)
                .await
                .expect("send succeeds");
            assert_eq!(
                target.recv::<Msg<19, u32>>().await.expect("recv succeeds"),
                19
            );
            origin
                .send::<Msg<20, u32>>(&20)
                .await
                .expect("send succeeds");
            assert_eq!(
                target.recv::<Msg<20, u32>>().await.expect("recv succeeds"),
                20
            );
            origin
                .send::<Msg<21, u32>>(&21)
                .await
                .expect("send succeeds");
            assert_eq!(
                target.recv::<Msg<21, u32>>().await.expect("recv succeeds"),
                21
            );
            origin
                .send::<Msg<22, u32>>(&22)
                .await
                .expect("send succeeds");
            assert_eq!(
                target.recv::<Msg<22, u32>>().await.expect("recv succeeds"),
                22
            );

            rv.tap().collect::<Vec<_>>()
        };

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
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn outgoing_offer_records_the_resolver_before_branch_send() {
    {
        let mut slab = [0; 64 * 1024];
        let transport = TestTransport::new();
        {
            let mut storage = SessionKitStorage::uninit();
            let cluster = storage.init();
            let rv = cluster
                .rendezvous(&mut slab, transport)
                .expect("rendezvous");
            let origin_program = simple_route_program::<0>();
            let target_program = simple_route_program::<1>();
            rv.set_resolver(
                &origin_program,
                ResolverRef::<ROUTE_RESOLVER>::decision_state(&ROUTE_STATE, choose_left),
            )
            .expect("install resolver");
            let sid = SessionId::new(0x0004_0005);
            let mut origin = rv.enter(sid, &origin_program).expect("origin");
            let mut target = rv.enter(sid, &target_program).expect("target");
            let branch = (origin.offer()).await.expect("outgoing offer");
            assert_eq!(branch.label(), 11);
            let before_send = rv.tap().collect::<Vec<_>>();
            let audit = before_send
                .iter()
                .find(|event| event.id() == tap::RESOLVER_AUDIT)
                .expect("the actual resolver call must be observable before sending");
            assert_eq!(resolver_id(*audit), u32::from(ROUTE_RESOLVER));
            assert_eq!(resolver_result(*audit), 0);
            assert_eq!(audit.arg0(), sid.raw());
            (branch.send::<Msg<11, ()>>(&()))
                .await
                .expect("branch send");
            (target.recv::<Msg<11, ()>>()).await.expect("receive");
            let events = rv.tap().collect::<Vec<_>>();
            assert_eq!(
                events
                    .iter()
                    .filter(|event| event.id() == tap::RESOLVER_AUDIT)
                    .count(),
                1,
                "committing the branch must not invent another resolver call"
            );
            for selection in events.iter().filter(|event| {
                event.id() == tap::ROUTE_ARM_SELECTION && event.causal_key() & 0xff == 2
            }) {
                assert!(audit.ts() < selection.ts());
                assert_eq!(resolver_route_site(*audit), resolver_route_site(*selection));
            }
        };
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn dynamic_resolver_success_emits_one_reversible_audit() {
    {
        let mut slab = [0; 64 * 1024];
        let transport = TestTransport::new();
        let sid = SessionId::new(0x0004_0001);
        let events = {
            let mut storage = SessionKitStorage::uninit();
            let cluster = storage.init();
            let rv = cluster
                .rendezvous(&mut slab, transport.clone())
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

            (async {
                origin
                    .send::<Msg<11, ()>>(&())
                    .await
                    .expect("left branch sends");
                target
                    .recv::<Msg<11, ()>>()
                    .await
                    .expect("left branch receives");
            })
            .await;

            rv.tap().collect::<Vec<_>>()
        };

        let audits = events
            .iter()
            .copied()
            .filter(|event| event.id() == tap::RESOLVER_AUDIT)
            .collect::<Vec<_>>();
        assert_eq!(audits.len(), 1, "resolver success must emit one audit");
        assert_eq!(audits[0].arg0(), sid.raw());
        assert_eq!(resolver_id(audits[0]), ROUTE_RESOLVER as u32);
        assert_eq!(resolver_result(audits[0]), 0, "Left must encode as 0");
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn reused_resolver_id_keeps_route_site_in_audit() {
    {
        let mut slab = [0; 64 * 1024];
        let transport = TestTransport::new();
        let sid = SessionId::new(0x0004_0003);
        let events = {
            let mut storage = SessionKitStorage::uninit();
            let cluster = storage.init();
            let rv = cluster
                .rendezvous(&mut slab, transport.clone())
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

            (origin.send::<Msg<11, u32>>(&11))
                .await
                .expect("send first route");
            let first = (target.offer()).await.expect("first route");
            assert_eq!(
                (first.recv::<Msg<11, u32>>())
                    .await
                    .expect("first route commit"),
                11
            );
            (origin.send::<Msg<13, u32>>(&13))
                .await
                .expect("send second route");

            rv.tap().collect::<Vec<_>>()
        };

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
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn nested_route_runtime_emits_distinct_route_selection_sites() {
    {
        let mut slab = [0; 64 * 1024];
        let transport = TestTransport::new();
        let sid = SessionId::new(0x0004_0004);
        let events = {
            let mut storage = SessionKitStorage::uninit();
            let cluster = storage.init();
            let rv = cluster
                .rendezvous(&mut slab, transport.clone())
                .expect("register rendezvous");
            let origin_program = nested_route_program::<0>();
            let target_program = nested_route_program::<1>();
            let mut origin = rv.enter(sid, &origin_program).expect("attach origin");
            let mut target = rv.enter(sid, &target_program).expect("attach target");

            (origin.send::<Msg<41, u32>>(&41))
                .await
                .expect("send outer route");
            let outer = (target.offer()).await.expect("offer outer route");
            assert_eq!(
                (outer.recv::<Msg<41, u32>>())
                    .await
                    .expect("outer route recv"),
                41
            );
            (origin.send::<Msg<42, u32>>(&42))
                .await
                .expect("send inner route");
            let inner = (target.offer()).await.expect("offer inner route");
            assert_eq!(
                (inner.recv::<Msg<42, u32>>())
                    .await
                    .expect("inner route recv"),
                42
            );

            rv.tap().collect::<Vec<_>>()
        };

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
    };
}

#[test]
fn dynamic_resolver_reject_is_ready_error_with_one_audit() {
    {
        let mut slab = [0; 64 * 1024];
        let transport = TestTransport::new();
        let sid = SessionId::new(0x0004_0002);
        let (events, rendered) = {
            let mut storage = SessionKitStorage::uninit();
            let cluster = storage.init();
            let rv = cluster
                .rendezvous(&mut slab, transport.clone())
                .expect("register rendezvous");
            let program = local_route_program();
            rv.set_resolver(
                &program,
                ResolverRef::<ROUTE_RESOLVER>::decision_state(&ROUTE_STATE, reject),
            )
            .expect("install resolver");
            let mut endpoint = rv.enter(sid, &program).expect("attach endpoint");

            let result = {
                let mut offer = pin!(endpoint.offer());
                let mut cx = Context::from_waker(Waker::noop());
                let Poll::Ready(result) = offer.as_mut().poll(&mut cx) else {
                    panic!("resolver reject must not park");
                };
                result
            };
            let error = match result {
                Ok(_) => panic!("resolver reject must fail"),
                Err(error) => error,
            };
            (rv.tap().collect::<Vec<_>>(), format!("{error:?}"))
        };

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
    };
}
