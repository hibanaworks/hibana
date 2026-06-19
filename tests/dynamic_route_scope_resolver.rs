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

const ROUTE_RESOLVER: u16 = 0x91;
const OUTER_ROUTE_RESOLVER: u16 = 0x92;
const INNER_ROUTE_RESOLVER: u16 = 0x93;
const LEFT_A: u8 = 31;
const LEFT_B: u8 = 32;
const RIGHT: u8 = 33;
const NESTED_LEFT: u8 = 41;
const NESTED_INNER_RIGHT: u8 = 42;
const NESTED_OUTER_RIGHT: u8 = 43;
const SAME_LABEL: u8 = 55;
const SAME_LABEL_ROUTE_RESOLVER: u16 = 0x94;
static LEFT_ARM: DecisionArm = DecisionArm::Left;
static RIGHT_ARM: DecisionArm = DecisionArm::Right;
static UNIT: () = ();

std::thread_local! {
    static SESSION_SLOT: UnsafeCell<TestKitStorage> = const {
        UnsafeCell::new(SessionKitStorage::uninit())
    };
}

fn choose_arm(arm: &DecisionArm) -> Result<DecisionArm, ResolverError> {
    Ok(*arm)
}

fn program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let left = g::par(
        g::send::<0, 1, Msg<LEFT_A, u8>>(),
        g::send::<0, 2, Msg<LEFT_B, u8>>(),
    );
    let right = g::send::<0, 1, Msg<RIGHT, u8>>();
    project(&g::route(left, right).resolve::<ROUTE_RESOLVER>())
}

fn nested_resolver_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let inner = g::route(
        g::send::<0, 1, Msg<NESTED_LEFT, u8>>(),
        g::send::<0, 1, Msg<NESTED_INNER_RIGHT, u8>>(),
    )
    .resolve::<INNER_ROUTE_RESOLVER>();
    project(
        &g::route(inner, g::send::<0, 1, Msg<NESTED_OUTER_RIGHT, u8>>())
            .resolve::<OUTER_ROUTE_RESOLVER>(),
    )
}

fn same_label_outbound_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let left = g::send::<0, 1, Msg<SAME_LABEL, u32>>();
    let right = g::send::<0, 2, Msg<SAME_LABEL, u32>>();
    project(&g::route(left, right).resolve::<SAME_LABEL_ROUTE_RESOLVER>())
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

fn reject(_: &()) -> Result<DecisionArm, ResolverError> {
    Err(ResolverError::reject())
}

fn resolver_ids(events: &[tap::TapEvent]) -> Vec<u32> {
    resolver_audits(events)
        .into_iter()
        .map(resolver_id)
        .collect()
}

fn resolver_sites(events: &[tap::TapEvent]) -> Vec<u32> {
    resolver_audits(events)
        .into_iter()
        .map(route_site)
        .collect()
}

fn route_arm_selections(events: &[tap::TapEvent]) -> Vec<tap::TapEvent> {
    events
        .iter()
        .copied()
        .filter(|event| event.id() == tap::ROUTE_ARM_SELECTION)
        .collect()
}

fn selected_arm(event: tap::TapEvent) -> u32 {
    event.arg1() & 0xff
}

fn assert_same_label_route_send_uses_selected_arm(
    resolver_arm: &'static DecisionArm,
    expected_arm: u32,
    value: u32,
) {
    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        let sid = SessionId::new(0x0009_1500 + expected_arm);
        let events = with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let rv = cluster
                .rendezvous(slab, transport)
                .expect("register rendezvous");
            let role0 = same_label_outbound_program::<0>();
            let role1 = same_label_outbound_program::<1>();
            let role2 = same_label_outbound_program::<2>();
            rv.set_resolver(
                &role0,
                ResolverRef::<SAME_LABEL_ROUTE_RESOLVER>::decision_state(resolver_arm, choose_arm),
            )
            .expect("install sender resolver");
            let mut origin = rv.enter(sid, &role0).expect("attach origin");
            let mut left_peer = rv.enter(sid, &role1).expect("attach left peer");
            let mut right_peer = rv.enter(sid, &role2).expect("attach right peer");

            futures::executor::block_on(async {
                origin
                    .send::<Msg<SAME_LABEL, u32>>(&value)
                    .await
                    .expect("same-label resolved route send");
                if expected_arm == 0 {
                    assert!(
                        right_peer
                            .recv::<Msg<SAME_LABEL, u32>>()
                            .now_or_never()
                            .is_none(),
                        "right peer must not receive the unselected left-arm send"
                    );
                    assert_eq!(
                        left_peer
                            .recv::<Msg<SAME_LABEL, u32>>()
                            .await
                            .expect("left peer receives selected arm"),
                        value
                    );
                } else {
                    assert!(
                        left_peer
                            .recv::<Msg<SAME_LABEL, u32>>()
                            .now_or_never()
                            .is_none(),
                        "left peer must not receive the unselected right-arm send"
                    );
                    assert_eq!(
                        right_peer
                            .recv::<Msg<SAME_LABEL, u32>>()
                            .await
                            .expect("right peer receives selected arm"),
                        value
                    );
                }
            });

            rv.tap().collect::<Vec<_>>()
        });

        let audits = resolver_audits(&events);
        assert_eq!(
            audits.len(),
            1,
            "same-label send must audit one resolver decision: {events:?}"
        );
        assert_eq!(resolver_id(audits[0]), SAME_LABEL_ROUTE_RESOLVER as u32);

        let selections = route_arm_selections(&events);
        assert!(
            !selections.is_empty(),
            "same-label send must emit selected route arm evidence: {events:?}"
        );
        assert!(
            selections
                .iter()
                .all(|event| selected_arm(*event) == expected_arm),
            "route selection evidence must name only selected arm {expected_arm}: {selections:?}"
        );
    });
}

#[test]
fn left_decision_materializes_nested_parallel_arm_once() {
    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        let sid = SessionId::new(0x0009_1001);
        let events = with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let rv = cluster
                .rendezvous(slab, transport)
                .expect("register rendezvous");
            let role0 = program::<0>();
            let role1 = program::<1>();
            let role2 = program::<2>();
            rv.set_resolver(
                &role0,
                ResolverRef::<ROUTE_RESOLVER>::decision_state(&LEFT_ARM, choose_arm),
            )
            .expect("install route resolver");
            let mut origin = rv.enter(sid, &role0).expect("attach origin");
            let mut peer_a = rv.enter(sid, &role1).expect("attach peer a");
            let mut peer_b = rv.enter(sid, &role2).expect("attach peer b");

            futures::executor::block_on(async {
                origin
                    .send::<Msg<LEFT_A, u8>>(&11)
                    .await
                    .expect("left lane a send");
                assert_eq!(
                    peer_a
                        .recv::<Msg<LEFT_A, u8>>()
                        .await
                        .expect("left lane a recv"),
                    11
                );
                origin
                    .send::<Msg<LEFT_B, u8>>(&12)
                    .await
                    .expect("left lane b send");
                assert_eq!(
                    peer_b
                        .recv::<Msg<LEFT_B, u8>>()
                        .await
                        .expect("left lane b recv"),
                    12
                );
                let _ = origin
                    .send::<Msg<RIGHT, u8>>(&13)
                    .await
                    .expect_err("right arm must stay unmaterialized after left decision");
            });

            rv.tap().collect::<Vec<_>>()
        });

        let audits = resolver_audits(&events);
        assert_eq!(audits.len(), 1, "left decision must audit once: {events:?}");
        assert_eq!(resolver_id(audits[0]), ROUTE_RESOLVER as u32);
        assert_eq!(route_site(audits[0]), 0);
    });
}

#[test]
fn same_label_resolved_outbound_left_sends_to_left_peer_only() {
    assert_same_label_route_send_uses_selected_arm(&LEFT_ARM, 0, 5501);
}

#[test]
fn same_label_resolved_outbound_right_sends_to_right_peer_only() {
    assert_same_label_route_send_uses_selected_arm(&RIGHT_ARM, 1, 5502);
}

#[test]
fn nested_left_route_evaluates_outer_and_inner_resolvers_by_scope() {
    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        let sid = SessionId::new(0x0009_1003);
        let events = with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let rv = cluster
                .rendezvous(slab, transport)
                .expect("register rendezvous");
            let role0 = nested_resolver_program::<0>();
            let role1 = nested_resolver_program::<1>();
            rv.set_resolver(
                &role0,
                ResolverRef::<OUTER_ROUTE_RESOLVER>::decision_state(&LEFT_ARM, choose_arm),
            )
            .expect("install outer route resolver");
            rv.set_resolver(
                &role0,
                ResolverRef::<INNER_ROUTE_RESOLVER>::decision_state(&LEFT_ARM, choose_arm),
            )
            .expect("install inner route resolver");
            let mut origin = rv.enter(sid, &role0).expect("attach origin");
            let mut peer = rv.enter(sid, &role1).expect("attach peer");

            futures::executor::block_on(async {
                origin
                    .send::<Msg<NESTED_LEFT, u8>>(&41)
                    .await
                    .expect("nested left send");
                let branch = peer.offer().await.expect("offer nested outer route");
                assert_eq!(
                    branch
                        .recv::<Msg<NESTED_LEFT, u8>>()
                        .await
                        .expect("nested left recv"),
                    41
                );
            });

            rv.tap().collect::<Vec<_>>()
        });

        let mut ids = resolver_ids(&events);
        ids.sort_unstable();
        assert_eq!(
            ids,
            [OUTER_ROUTE_RESOLVER as u32, INNER_ROUTE_RESOLVER as u32],
            "outer and inner route scopes must use their own resolver markers: {events:?}"
        );

        let mut sites = resolver_sites(&events);
        sites.sort_unstable();
        sites.dedup();
        assert_eq!(
            sites.len(),
            2,
            "outer and inner route scopes must audit distinct route sites: {events:?}"
        );
    });
}

#[test]
fn nested_outer_right_does_not_evaluate_inner_resolver() {
    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        let sid = SessionId::new(0x0009_1004);
        let events = with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let rv = cluster
                .rendezvous(slab, transport)
                .expect("register rendezvous");
            let role0 = nested_resolver_program::<0>();
            let role1 = nested_resolver_program::<1>();
            rv.set_resolver(
                &role0,
                ResolverRef::<OUTER_ROUTE_RESOLVER>::decision_state(&RIGHT_ARM, choose_arm),
            )
            .expect("install outer route resolver");
            rv.set_resolver(
                &role0,
                ResolverRef::<INNER_ROUTE_RESOLVER>::decision_state(&UNIT, reject),
            )
            .expect("install inner route resolver");
            let mut origin = rv.enter(sid, &role0).expect("attach origin");
            let mut peer = rv.enter(sid, &role1).expect("attach peer");

            futures::executor::block_on(async {
                origin
                    .send::<Msg<NESTED_OUTER_RIGHT, u8>>(&43)
                    .await
                    .expect("outer right send");
                let branch = peer.offer().await.expect("offer outer right route");
                assert_eq!(
                    branch
                        .recv::<Msg<NESTED_OUTER_RIGHT, u8>>()
                        .await
                        .expect("outer right recv"),
                    43
                );
            });

            rv.tap().collect::<Vec<_>>()
        });

        let ids = resolver_ids(&events);
        assert_eq!(
            ids,
            [OUTER_ROUTE_RESOLVER as u32],
            "outer right must not evaluate the inner route resolver: {events:?}"
        );
    });
}

#[test]
fn right_decision_rejects_nested_parallel_left_arm() {
    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        let sid = SessionId::new(0x0009_1002);
        let events = with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let rv = cluster
                .rendezvous(slab, transport)
                .expect("register rendezvous");
            let role0 = program::<0>();
            let role1 = program::<1>();
            rv.set_resolver(
                &role0,
                ResolverRef::<ROUTE_RESOLVER>::decision_state(&RIGHT_ARM, choose_arm),
            )
            .expect("install route resolver");
            let mut origin = rv.enter(sid, &role0).expect("attach origin");
            let mut peer = rv.enter(sid, &role1).expect("attach peer");

            futures::executor::block_on(async {
                origin
                    .send::<Msg<RIGHT, u8>>(&21)
                    .await
                    .expect("right arm send");
                assert_eq!(
                    peer.recv::<Msg<RIGHT, u8>>().await.expect("right arm recv"),
                    21
                );
                let _ = origin
                    .send::<Msg<LEFT_A, u8>>(&22)
                    .await
                    .expect_err("left arm must stay unmaterialized after right decision");
            });

            rv.tap().collect::<Vec<_>>()
        });

        let audits = resolver_audits(&events);
        assert_eq!(
            audits.len(),
            1,
            "right decision must audit once: {events:?}"
        );
        assert_eq!(resolver_id(audits[0]), ROUTE_RESOLVER as u32);
    });
}
