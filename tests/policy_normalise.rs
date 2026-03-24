#![cfg(feature = "std")]

#[path = "common/check_support.rs"]
mod check_support;
#[path = "common/observe_support.rs"]
mod observe_support;

use check_support::policy_check_summary;
use hibana::substrate::mgmt::session::tap::TapEvent;
use observe_support::{
    LANE_ACQUIRE_ID, LANE_RELEASE_ID, POLICY_ABORT_ID, POLICY_COMMIT_ID, PolicyEventDomain,
    PolicyEventKind, policy_lane_trace,
};

const RING_EVENTS: usize = 2048;

fn raw_event(ts: u32, id: u16) -> TapEvent {
    TapEvent {
        ts,
        id,
        ..TapEvent::zero()
    }
}

#[test]
fn policy_lane_trace_matches_lane_assignments() {
    let mut storage = [TapEvent::default(); RING_EVENTS];
    let rv: u32 = 7;
    let sid: u32 = 0x1234;
    let lane: u16 = 3;
    let causal = TapEvent::make_causal_key((lane as u8) + 1, 0);

    storage[0] = raw_event(1, LANE_ACQUIRE_ID)
        .with_arg0(rv)
        .with_arg1(((sid as u32) << 16) | lane as u32);
    storage[1] = raw_event(2, POLICY_ABORT_ID)
        .with_causal_key(causal)
        .with_arg0(0xAA)
        .with_arg1(sid);
    storage[2] = raw_event(3, POLICY_COMMIT_ID)
        .with_causal_key(causal)
        .with_arg0(0)
        .with_arg1(42);
    storage[3] = raw_event(4, LANE_RELEASE_ID)
        .with_arg0(rv)
        .with_arg1(((sid as u32) << 16) | lane as u32);

    let (records, local_action_failures) = policy_lane_trace(&storage, 0, 4);
    assert_eq!(local_action_failures, 0, "unexpected local action failures");
    assert_eq!(records.len(), 2, "expected abort and commit entries");

    let abort = &records[0];
    assert_eq!(
        abort.lane,
        Some(lane),
        "abort lane should match acquisition"
    );
    assert!(
        abort.lane.is_none() || abort.has_association,
        "abort should have active lane association"
    );
    assert_eq!(abort.sid_hint, Some(sid), "abort carries sid hint");
    assert_eq!(
        abort.sid_match,
        Some(true),
        "abort sid should align with lane association"
    );
    assert_eq!(
        abort.kind,
        PolicyEventKind::Abort,
        "abort record should surface policy event kind"
    );
    assert_eq!(
        abort.domain,
        PolicyEventDomain::Policy,
        "abort event must be marked as policy domain"
    );
    assert_eq!(abort.policy_id, POLICY_ABORT_ID);

    let commit = &records[1];
    assert_eq!(
        commit.lane,
        Some(lane),
        "commit lane should match acquisition"
    );
    assert!(
        commit.lane.is_none() || commit.has_association,
        "commit should have active lane association"
    );
    assert!(commit.sid_hint.is_none(), "commit has no sid hint");
    assert!(
        commit.sid_match.is_none(),
        "commit sid match is not evaluated"
    );
    assert_eq!(
        commit.kind,
        PolicyEventKind::Commit,
        "commit record should surface policy event kind"
    );
    assert_eq!(
        commit.domain,
        PolicyEventDomain::Policy,
        "commit event must be marked as policy domain"
    );
    assert_eq!(commit.policy_id, POLICY_COMMIT_ID);

    let summary = policy_check_summary(&storage, 0, 4);
    assert_eq!(summary.policy_lane_total, 2);
    assert_eq!(summary.policy_lane_matched, 2);
    assert_eq!(summary.policy_lane_mismatched, 0);
    assert_eq!(summary.policy_sid_matched, 1);
    assert_eq!(summary.policy_sid_mismatched, 0);
}
