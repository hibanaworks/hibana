use super::*;
#[test]
fn replay_from_audit_log_tracks_digest_transitions() {
    let mut log = ReplayLog::default();
    let mut cursor = log.head();
    let digest_v1 = 0x1020_3040;
    let digest_v2 = 0x5060_7080;
    let attrs = policy_attrs(None, None);

    push_policy_audit_tuple(
        &mut log,
        1,
        digest_v1,
        PolicySlot::Decision,
        0,
        raw_event(1, POLICY_COMMIT_ID)
            .with_arg0(0x1111)
            .with_arg1(1),
        [0; 4],
        attrs,
        0,
        0,
        0,
    );
    push_policy_audit_tuple(
        &mut log,
        2,
        digest_v2,
        PolicySlot::Decision,
        0,
        raw_event(2, POLICY_COMMIT_ID)
            .with_arg0(0x1111)
            .with_arg1(2),
        [0; 4],
        attrs,
        0,
        0,
        0,
    );
    push_policy_audit_tuple(
        &mut log,
        3,
        digest_v1,
        PolicySlot::Decision,
        0,
        raw_event(3, POLICY_STATE_RESTORE_ID)
            .with_arg0(0x1111)
            .with_arg1(1),
        [0; 4],
        attrs,
        0,
        0,
        0,
    );

    let replay = replay_digests(&log, PolicySlot::Decision, &mut cursor);
    assert_eq!(
        replay,
        DigestState {
            active_digest: Some(digest_v1),
            standby_digest: Some(digest_v2),
            last_good_digest: Some(digest_v1),
        }
    );
}

#[test]
fn replay_from_audit_log_tracks_tx_abort_reverts() {
    let mut log = ReplayLog::default();
    let mut cursor = log.head();
    let digest_v1 = 0x0A0B_0C0D;
    let digest_v2 = 0x0102_0304;
    let attrs = policy_attrs(None, None);

    push_policy_audit_tuple(
        &mut log,
        1,
        digest_v1,
        PolicySlot::Decision,
        0,
        raw_event(1, POLICY_COMMIT_ID)
            .with_arg0(0x2222)
            .with_arg1(1),
        [0; 4],
        attrs,
        0,
        0,
        0,
    );
    push_policy_audit_tuple(
        &mut log,
        2,
        digest_v2,
        PolicySlot::Decision,
        0,
        raw_event(2, POLICY_COMMIT_ID)
            .with_arg0(0x2222)
            .with_arg1(2),
        [0; 4],
        attrs,
        0,
        0,
        0,
    );
    push_policy_audit_tuple(
        &mut log,
        3,
        digest_v1,
        PolicySlot::Decision,
        0,
        raw_event(3, POLICY_TX_ABORT_ID)
            .with_arg0(0x2222)
            .with_arg1(1),
        [0; 4],
        attrs,
        0,
        0,
        0,
    );

    let replay = replay_digests(&log, PolicySlot::Decision, &mut cursor);
    assert_eq!(
        replay,
        DigestState {
            active_digest: Some(digest_v1),
            standby_digest: Some(digest_v2),
            last_good_digest: Some(digest_v1),
        }
    );
}

#[test]
fn replay_digest_ignores_live_effect_taps_without_audit_tuple() {
    let mut log = ReplayLog::default();
    let mut cursor = log.head();

    log.push(
        raw_event(1, POLICY_COMMIT_ID)
            .with_arg0(slot_tag(PolicySlot::Decision) as u32)
            .with_arg1(2)
            .with_arg2(0xDEAD_BEEF),
    );
    log.push(
        raw_event(2, POLICY_TX_ABORT_ID)
            .with_arg0(slot_tag(PolicySlot::Decision) as u32)
            .with_arg1(1)
            .with_arg2(0x0102_0304),
    );

    let replay = replay_digests(&log, PolicySlot::Decision, &mut cursor);
    assert_eq!(
        replay,
        DigestState {
            active_digest: None,
            standby_digest: None,
            last_good_digest: None,
        }
    );
}

#[test]
fn public_policy_audit_tuple_roundtrips_logged_inputs() {
    let mut log = ReplayLog::default();
    let mut cursor = log.head();

    let event_one = raw_event(11, ROUTE_DECISION_ID)
        .with_causal_key(7)
        .with_arg0(1)
        .with_arg1(42)
        .with_arg2(99);
    let input_one = [9, 8, 7, 6];
    let policy_attrs_one = policy_attrs(Some(15), Some(3));
    push_policy_audit_tuple(
        &mut log,
        100,
        0xAABB_CCDD,
        PolicySlot::Decision,
        1,
        event_one,
        input_one,
        policy_attrs_one,
        0x0102_0000,
        0,
        5,
    );

    let event_two = raw_event(12, ENDPOINT_RX_EVENT_ID)
        .with_arg0(3)
        .with_arg1(4);
    let input_two = [1, 2, 3, 4];
    let policy_attrs_two = policy_attrs(None, Some(1));
    push_policy_audit_tuple(
        &mut log,
        101,
        0x1122_3344,
        PolicySlot::EndpointRx,
        0,
        event_two,
        input_two,
        policy_attrs_two,
        0,
        7,
        9,
    );
    log.push(
        raw_event(102, POLICY_AUDIT_DEFER_ID)
            .with_arg0((u32::from(DEFER_SOURCE_RESOLVER) << 24) | (1u32 << 16) | 4u32)
            .with_arg1(0)
            .with_arg2((1u32 << 16) | 0),
    );

    let rows = replay_audit_rows(&log, &mut cursor).expect("audit tuples must roundtrip");
    assert_eq!(rows.len(), 2);

    let first = rows.get(0).expect("first audit row");
    assert_eq!(first.digest, 0xAABB_CCDD);
    assert_eq!(first.slot, PolicySlot::Decision);
    assert_eq!(first.mode_tag, 1);
    assert_eq!(first.event, event_one);
    assert_eq!(first.policy_input, input_one);
    assert_eq!(first.policy_attrs, policy_attrs_one);
    assert_eq!(first.verdict_meta, 0x0102_0000);
    assert_eq!(first.fuel_used, 5);

    let second = rows.get(1).expect("second audit row");
    assert_eq!(second.digest, 0x1122_3344);
    assert_eq!(second.slot, PolicySlot::EndpointRx);
    assert_eq!(second.mode_tag, 0);
    assert_eq!(second.event, event_two);
    assert_eq!(second.policy_input, input_two);
    assert_eq!(second.policy_attrs, policy_attrs_two);
    assert_eq!(second.reason, 7);
}

#[test]
fn public_policy_audit_tuple_rejects_corruption() {
    let mut log = ReplayLog::default();
    let mut cursor = log.head();

    let event = raw_event(21, ROUTE_DECISION_ID).with_arg0(1).with_arg1(2);
    let input = [4, 3, 2, 1];
    let policy_attrs = policy_attrs(Some(9), Some(1));
    push_policy_audit_tuple(
        &mut log,
        200,
        0xDEAD_BEEF,
        PolicySlot::Decision,
        1,
        event,
        input,
        policy_attrs,
        0,
        0,
        1,
    );
    log.push(
        raw_event(201, POLICY_AUDIT_DEFER_ID)
            .with_arg0((u32::from(DEFER_SOURCE_RESOLVER) << 24) | (3u32 << 16) | 6u32)
            .with_arg1(1)
            .with_arg2((9u32 << 16) | 0),
    );

    let result = replay_audit_rows(&log, &mut cursor);
    assert!(matches!(result, Err("invalid defer reason")));
}
