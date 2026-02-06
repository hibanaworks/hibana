#![cfg(feature = "std")]

use hibana::{
    observe::{self, PolicyEventDomain, TapEvent, ids, normalise},
    runtime::consts::RING_EVENTS,
};

#[test]
fn policy_lane_trace_matches_lane_assignments() {
    let mut storage = [TapEvent::default(); RING_EVENTS];
    let rv: u32 = 7;
    let sid: u32 = 0x1234;
    let lane: u16 = 3;
    let causal = TapEvent::make_causal_key((lane as u8) + 1, 0);

    storage[0] = observe::LaneAcquire::new(1, rv, sid, lane);
    storage[1] = observe::RawEvent::with_causal(2, ids::POLICY_ABORT, causal, 0xAA, sid);
    storage[2] = observe::RawEvent::with_causal(3, ids::POLICY_COMMIT, causal, 0, 42);
    storage[3] = observe::LaneRelease::new(4, rv, sid, lane);

    let (records, failures) = normalise::policy_lane_trace(&storage, 0, 4);
    assert!(failures.is_empty(), "unexpected local action failures");
    assert_eq!(records.len(), 2, "expected abort and commit entries");

    let abort = &records[0];
    assert_eq!(
        abort.lane,
        Some(lane),
        "abort lane should match acquisition"
    );
    assert!(
        abort.lane_matches(),
        "abort should have active lane association"
    );
    assert_eq!(abort.sid_hint, Some(sid), "abort carries sid hint");
    assert_eq!(
        abort.sid_match,
        Some(true),
        "abort sid should align with lane association"
    );
    assert_eq!(
        abort.spec.name(),
        "policy_abort",
        "abort record should surface policy event name"
    );
    assert_eq!(
        abort.spec.domain,
        PolicyEventDomain::Policy,
        "abort event must be marked as policy domain"
    );

    let commit = &records[1];
    assert_eq!(
        commit.lane,
        Some(lane),
        "commit lane should match acquisition"
    );
    assert!(
        commit.lane_matches(),
        "commit should have active lane association"
    );
    assert!(commit.sid_hint.is_none(), "commit has no sid hint");
    assert!(
        commit.sid_match.is_none(),
        "commit sid match is not evaluated"
    );
    assert_eq!(
        commit.spec.name(),
        "policy_commit",
        "commit record should surface policy event name"
    );
    assert_eq!(
        commit.spec.domain,
        PolicyEventDomain::Policy,
        "commit event must be marked as policy domain"
    );

    observe::reset();
    for event in storage.iter().take(4) {
        observe::feed(*event);
    }
    let report = observe::snapshot();
    assert_eq!(report.policy_lane_total, 2);
    assert_eq!(report.policy_lane_matched, 2);
    assert_eq!(report.policy_lane_mismatched, 0);
    assert_eq!(report.policy_sid_matched, 1);
    assert_eq!(report.policy_sid_mismatched, 0);
}
