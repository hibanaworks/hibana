mod common;
#[path = "support/dynamic_route_scope.rs"]
mod dynamic_route_scope;
#[path = "support/runtime.rs"]
mod runtime_support;
#[path = "support/tls_ref.rs"]
mod tls_ref_support;

use common::TestTransport;
use dynamic_route_scope::*;
use hibana::{
    g::Msg,
    runtime::{
        ids::SessionId,
        resolver::{DecisionArm, ResolverRef},
        tap,
    },
};

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
fn resolved_route_send_calls_resolver_once_per_progress() {
    reset_counters();
    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        let sid = SessionId::new(0x0009_1601);
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let rv = cluster
                .rendezvous(slab, transport)
                .expect("register rendezvous");
            let role0 = same_label_outbound_program_for::<0, COUNTING_ROUTE_RESOLVER>();
            let role1 = same_label_outbound_program_for::<1, COUNTING_ROUTE_RESOLVER>();
            let role2 = same_label_outbound_program_for::<2, COUNTING_ROUTE_RESOLVER>();
            rv.set_resolver(
                &role0,
                ResolverRef::<COUNTING_ROUTE_RESOLVER>::decision_state(&UNIT, counting_left),
            )
            .expect("install counting resolver");
            let mut origin = rv.enter(sid, &role0).expect("attach origin");
            let mut left_peer = rv.enter(sid, &role1).expect("attach left peer");
            let mut right_peer = rv.enter(sid, &role2).expect("attach right peer");

            futures::executor::block_on(async {
                origin
                    .send::<Msg<SAME_LABEL, u32>>(&1601)
                    .await
                    .expect("resolved send");
                assert!(
                    right_peer
                        .recv::<Msg<SAME_LABEL, u32>>()
                        .now_or_never()
                        .is_none(),
                    "unselected right peer must not receive"
                );
                assert_eq!(
                    left_peer
                        .recv::<Msg<SAME_LABEL, u32>>()
                        .await
                        .expect("selected peer recv"),
                    1601
                );
            });
        });
    });
    assert_eq!(
        read_counters().counting_calls,
        1,
        "send progress must call ResolverRef::decide exactly once"
    );
}

#[test]
fn same_scope_sites_with_distinct_resolver_ids_keep_distinct_authority() {
    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        let left_sid = SessionId::new(0x0009_1606);
        let right_sid = SessionId::new(0x0009_1607);
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let rv = cluster
                .rendezvous(slab, transport)
                .expect("register rendezvous");
            let left_role0 = same_label_outbound_program_for::<0, SAME_LABEL_ROUTE_RESOLVER>();
            let left_role1 = same_label_outbound_program_for::<1, SAME_LABEL_ROUTE_RESOLVER>();
            let left_role2 = same_label_outbound_program_for::<2, SAME_LABEL_ROUTE_RESOLVER>();
            let right_role0 = same_label_outbound_program_for::<0, COUNTING_ROUTE_RESOLVER>();
            let right_role1 = same_label_outbound_program_for::<1, COUNTING_ROUTE_RESOLVER>();
            let right_role2 = same_label_outbound_program_for::<2, COUNTING_ROUTE_RESOLVER>();

            rv.set_resolver(
                &left_role0,
                ResolverRef::<SAME_LABEL_ROUTE_RESOLVER>::decision_state(&LEFT_ARM, choose_arm),
            )
            .expect("install left resolver");
            rv.set_resolver(
                &right_role0,
                ResolverRef::<COUNTING_ROUTE_RESOLVER>::decision_state(&RIGHT_ARM, choose_arm),
            )
            .expect("install right resolver at the same local route scope");

            let mut left_origin = rv.enter(left_sid, &left_role0).expect("attach left origin");
            let mut left_peer = rv.enter(left_sid, &left_role1).expect("attach left peer");
            let mut left_other = rv.enter(left_sid, &left_role2).expect("attach left other");
            let mut right_origin = rv
                .enter(right_sid, &right_role0)
                .expect("attach right origin");
            let mut right_other = rv
                .enter(right_sid, &right_role1)
                .expect("attach right other");
            let mut right_peer = rv
                .enter(right_sid, &right_role2)
                .expect("attach right peer");

            futures::executor::block_on(async {
                left_origin
                    .send::<Msg<SAME_LABEL, u32>>(&1606)
                    .await
                    .expect("first resolver id keeps left authority");
                assert!(
                    left_other
                        .recv::<Msg<SAME_LABEL, u32>>()
                        .now_or_never()
                        .is_none(),
                    "second registration must not overwrite the first resolver id"
                );
                assert_eq!(
                    left_peer
                        .recv::<Msg<SAME_LABEL, u32>>()
                        .await
                        .expect("left resolver peer receives"),
                    1606
                );

                right_origin
                    .send::<Msg<SAME_LABEL, u32>>(&1607)
                    .await
                    .expect("second resolver id keeps right authority");
                assert!(
                    right_other
                        .recv::<Msg<SAME_LABEL, u32>>()
                        .now_or_never()
                        .is_none(),
                    "first registration must not shadow the second resolver id"
                );
                assert_eq!(
                    right_peer
                        .recv::<Msg<SAME_LABEL, u32>>()
                        .await
                        .expect("right resolver peer receives"),
                    1607
                );
            });
        });
    });
}

#[test]
fn same_scope_and_resolver_id_in_distinct_programs_keep_distinct_authority() {
    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        let nested_sid = SessionId::new(0x0009_1608);
        let direct_sid = SessionId::new(0x0009_1609);
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let rv = cluster
                .rendezvous(slab, transport)
                .expect("register rendezvous");
            let nested_role0 = program::<0>();
            let nested_role1 = program::<1>();
            let nested_role2 = program::<2>();
            let direct_role0 = same_label_outbound_program_for::<0, ROUTE_RESOLVER>();
            let direct_role1 = same_label_outbound_program_for::<1, ROUTE_RESOLVER>();
            let direct_role2 = same_label_outbound_program_for::<2, ROUTE_RESOLVER>();

            rv.set_resolver(
                &nested_role0,
                ResolverRef::<ROUTE_RESOLVER>::decision_state(&LEFT_ARM, choose_arm),
            )
            .expect("install nested-program resolver");
            rv.set_resolver(
                &direct_role0,
                ResolverRef::<ROUTE_RESOLVER>::decision_state(&RIGHT_ARM, choose_arm),
            )
            .expect("install distinct-program resolver at the same local route site");

            let mut nested_origin = rv
                .enter(nested_sid, &nested_role0)
                .expect("attach nested origin");
            let mut nested_peer = rv
                .enter(nested_sid, &nested_role1)
                .expect("attach nested peer");
            let nested_other = rv
                .enter(nested_sid, &nested_role2)
                .expect("attach nested other");
            let mut direct_origin = rv
                .enter(direct_sid, &direct_role0)
                .expect("attach direct origin");
            let mut direct_other = rv
                .enter(direct_sid, &direct_role1)
                .expect("attach direct other");
            let mut direct_peer = rv
                .enter(direct_sid, &direct_role2)
                .expect("attach direct peer");

            futures::executor::block_on(async {
                nested_origin
                    .send::<Msg<LEFT_A, u8>>(&18)
                    .await
                    .expect("first program keeps its left resolver authority");
                assert_eq!(
                    nested_peer
                        .recv::<Msg<LEFT_A, u8>>()
                        .await
                        .expect("nested left peer receives"),
                    18
                );

                direct_origin
                    .send::<Msg<SAME_LABEL, u32>>(&1609)
                    .await
                    .expect("second program keeps its right resolver authority");
                assert!(
                    direct_other
                        .recv::<Msg<SAME_LABEL, u32>>()
                        .now_or_never()
                        .is_none(),
                    "first program resolver must not overwrite the second program"
                );
                assert_eq!(
                    direct_peer
                        .recv::<Msg<SAME_LABEL, u32>>()
                        .await
                        .expect("direct right peer receives"),
                    1609
                );
            });

            core::hint::black_box(&nested_other);
        });
    });
}

#[test]
fn stateful_send_resolver_is_not_evaluated_twice() {
    reset_counters();
    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        let sid = SessionId::new(0x0009_1602);
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let rv = cluster
                .rendezvous(slab, transport)
                .expect("register rendezvous");
            let role0 = same_label_outbound_program_for::<0, FLIP_ROUTE_RESOLVER>();
            let role1 = same_label_outbound_program_for::<1, FLIP_ROUTE_RESOLVER>();
            let role2 = same_label_outbound_program_for::<2, FLIP_ROUTE_RESOLVER>();
            rv.set_resolver(
                &role0,
                ResolverRef::<FLIP_ROUTE_RESOLVER>::decision_state(&UNIT, flip_left_then_right),
            )
            .expect("install flipping resolver");
            let mut origin = rv.enter(sid, &role0).expect("attach origin");
            let mut left_peer = rv.enter(sid, &role1).expect("attach left peer");
            let mut right_peer = rv.enter(sid, &role2).expect("attach right peer");

            futures::executor::block_on(async {
                origin.send::<Msg<SAME_LABEL, u32>>(&1602).await.expect(
                    "first resolver decision selects left and must not be rechecked as right",
                );
                assert!(
                    right_peer
                        .recv::<Msg<SAME_LABEL, u32>>()
                        .now_or_never()
                        .is_none(),
                    "second resolver arm must not materialize"
                );
                assert_eq!(
                    left_peer
                        .recv::<Msg<SAME_LABEL, u32>>()
                        .await
                        .expect("left recv"),
                    1602
                );
            });
        });
    });
    assert_eq!(
        read_counters().flip_calls,
        1,
        "a Left then Right resolver would false reject if send re-evaluated it"
    );
}

#[test]
fn nested_send_after_selected_prefix_audits_only_new_inner_decision() {
    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        let sid = SessionId::new(0x0009_1605);
        let events = with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let rv = cluster
                .rendezvous(slab, transport)
                .expect("register rendezvous");
            let role0 = prefix_then_nested_resolver_program::<0>();
            let role1 = prefix_then_nested_resolver_program::<1>();
            rv.set_resolver(
                &role0,
                ResolverRef::<PREFIX_OUTER_ROUTE_RESOLVER>::decision_state(&LEFT_ARM, choose_arm),
            )
            .expect("install outer prefix resolver");
            rv.set_resolver(
                &role0,
                ResolverRef::<PREFIX_INNER_ROUTE_RESOLVER>::decision_state(&LEFT_ARM, choose_arm),
            )
            .expect("install inner prefix resolver");
            let mut origin = rv.enter(sid, &role0).expect("attach origin");
            let mut peer = rv.enter(sid, &role1).expect("attach peer");

            futures::executor::block_on(async {
                origin
                    .send::<Msg<PREFIX_LEFT, u8>>(&1)
                    .await
                    .expect("outer prefix send");
                assert_eq!(
                    peer.recv::<Msg<PREFIX_LEFT, u8>>()
                        .await
                        .expect("outer prefix recv"),
                    1
                );
                origin
                    .send::<Msg<PREFIX_INNER_LEFT, u8>>(&2)
                    .await
                    .expect("inner selected send");
                assert_eq!(
                    peer.recv::<Msg<PREFIX_INNER_LEFT, u8>>()
                        .await
                        .expect("inner selected recv"),
                    2
                );
            });

            rv.tap().collect::<Vec<_>>()
        });

        let mut ids = resolver_ids(&events);
        ids.sort_unstable();
        assert_eq!(
            ids,
            [
                PREFIX_OUTER_ROUTE_RESOLVER as u32,
                PREFIX_INNER_ROUTE_RESOLVER as u32,
            ],
            "send inside an already selected route must not re-audit the prefix resolver: {events:?}"
        );
    });
}

#[test]
fn resolver_reject_does_not_encode_or_stage_send_payload() {
    reset_counters();
    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        let sid = SessionId::new(0x0009_1603);
        let events = with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let rv = cluster
                .rendezvous(slab, transport)
                .expect("register rendezvous");
            let role0 = same_label_reject_payload_program::<0>();
            rv.set_resolver(
                &role0,
                ResolverRef::<REJECT_ROUTE_RESOLVER>::decision_state(&UNIT, rejecting_counted),
            )
            .expect("install rejecting resolver");
            let mut origin = rv.enter(sid, &role0).expect("attach origin");

            let error = futures::executor::block_on(
                origin.send::<Msg<SAME_LABEL, RejectCountedPayload>>(&RejectCountedPayload(1603)),
            )
            .expect_err("resolver reject must fail the send before staging payload");
            let rendered = format!("{error:?}");
            assert!(
                rendered.contains("ResolverReject"),
                "resolver reject must remain visible in Debug: {rendered}"
            );
            rv.tap().collect::<Vec<_>>()
        });

        assert_eq!(
            read_counters().reject_calls,
            1,
            "rejecting resolver must be called once"
        );
        assert_eq!(
            read_counters().reject_payload_encodes,
            0,
            "payload encoder must not run after resolver reject"
        );
        let audits = resolver_audits(&events);
        assert_eq!(
            audits.len(),
            1,
            "resolver reject must audit once: {events:?}"
        );
        assert_eq!(resolver_id(audits[0]), REJECT_ROUTE_RESOLVER as u32);
        assert!(
            route_arm_selections(&events).is_empty(),
            "rejected send must not publish route selection: {events:?}"
        );
    });
}

#[test]
fn dropped_resolved_send_future_does_not_publish_runtime_evidence() {
    reset_counters();
    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        let sid = SessionId::new(0x0009_1604);
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let rv = cluster
                .rendezvous(slab, transport)
                .expect("register rendezvous");
            let role0 = same_label_outbound_program_for::<0, DROP_ROUTE_RESOLVER>();
            let role1 = same_label_outbound_program_for::<1, DROP_ROUTE_RESOLVER>();
            let role2 = same_label_outbound_program_for::<2, DROP_ROUTE_RESOLVER>();
            rv.set_resolver(
                &role0,
                ResolverRef::<DROP_ROUTE_RESOLVER>::decision_state(&UNIT, drop_left),
            )
            .expect("install drop resolver");
            let mut origin = rv.enter(sid, &role0).expect("attach origin");
            let mut left_peer = rv.enter(sid, &role1).expect("attach left peer");
            let mut right_peer = rv.enter(sid, &role2).expect("attach right peer");

            let value = 1604u32;
            let future = origin.send::<Msg<SAME_LABEL, u32>>(&value);
            drop(future);
            let after_drop = rv.tap().collect::<Vec<_>>();
            assert_eq!(
                read_counters().drop_calls,
                0,
                "dropping an unpolled send future must not call ResolverRef::decide"
            );
            assert!(
                after_drop.iter().all(|event| {
                    event.id() != tap::RESOLVER_AUDIT
                        && event.id() != tap::ROUTE_ARM_SELECTION
                        && event.id() != tap::ENDPOINT_SEND
                }),
                "dropping before poll must not publish send progress evidence: {after_drop:?}"
            );

            futures::executor::block_on(async {
                origin
                    .send::<Msg<SAME_LABEL, u32>>(&value)
                    .await
                    .expect("send state must reset after dropped future");
                assert!(
                    right_peer
                        .recv::<Msg<SAME_LABEL, u32>>()
                        .now_or_never()
                        .is_none(),
                    "dropped preview must not leave right arm materialized"
                );
                assert_eq!(
                    left_peer
                        .recv::<Msg<SAME_LABEL, u32>>()
                        .await
                        .expect("left recv after reset"),
                    value
                );
            });
        });
    });
    assert_eq!(
        read_counters().drop_calls,
        1,
        "only the polled send attempt may call ResolverRef::decide"
    );
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
