#![cfg(feature = "std")]

use hibana::PolicyLaneRecord;
use hibana::control::lease::planner::{FacetSlots, LeaseFacetNeeds, resource_needs};
use hibana::observe::ScopeTrace;
use hibana::observe::normalise::{LaneAssociation, mgmt_policy_summary};
use hibana::observe::{self, PolicyEvent, PolicyEventKind, TapEvent, ids};
use hibana::runtime::{consts::RING_EVENTS, mgmt::MgmtFacetProfile};

#[test]
fn mgmt_facet_profile_matches_resource_needs() {
    let mut union = LeaseFacetNeeds::new();
    for tag in MgmtFacetProfile::resource_tags().iter().copied() {
        union = union.union(resource_needs(tag));
    }
    assert_eq!(union, FacetSlots::NEEDS);
    assert_eq!(union, MgmtFacetProfile::facet_needs());
    assert_eq!(
        MgmtFacetProfile::facet_needs(),
        MgmtFacetProfile::resource_facets()
    );
    assert!(MgmtFacetProfile::supports_policy_kind(
        PolicyEventKind::Commit
    ));
    assert!(MgmtFacetProfile::supports_policy_kind(
        PolicyEventKind::Rollback
    ));
    assert!(!MgmtFacetProfile::supports_policy_kind(
        PolicyEventKind::Abort
    ));
}

#[test]
fn mgmt_policy_trace_filters_commit_and_rollback() {
    let mut storage = [TapEvent::default(); RING_EVENTS];
    storage[0] = observe::PolicyCommit::new(1, 0, 1);
    storage[1] = observe::PolicyAbort::new(2, 0, 0);
    storage[2] = observe::PolicyRollback::new(3, 0, 2);

    let records = observe::normalise::mgmt_policy_trace(&storage, 0, 3);
    assert_eq!(records.len(), 2);
    assert!(
        records
            .iter()
            .any(|record| record.spec.kind == PolicyEventKind::Commit),
        "commit event should be retained"
    );
    assert!(
        records
            .iter()
            .any(|record| record.spec.kind == PolicyEventKind::Rollback),
        "rollback event should be retained"
    );
    assert!(
        records
            .iter()
            .all(|record| MgmtFacetProfile::supports_policy_kind(record.spec.kind)),
        "all retained events must be recognised by the management profile"
    );

    // Feed the same events into the streaming checker to ensure global state remains clean.
    observe::reset();
    for event in storage.iter().take(3) {
        observe::feed(*event);
    }
    let report = observe::snapshot();
    assert_eq!(report.policy_commit, 1);
    assert_eq!(report.policy_abort, 1);
    assert_eq!(report.policy_rollback, 1);
}

#[test]
fn mgmt_policy_summary_detects_anomalies() {
    let commit_spec = observe::policy_event_spec(ids::POLICY_COMMIT).unwrap();
    let rollback_spec = observe::policy_event_spec(ids::POLICY_ROLLBACK).unwrap();

    let matching_commit = PolicyLaneRecord {
        spec: commit_spec,
        event: PolicyEvent {
            kind: PolicyEventKind::Commit,
            arg0: 0,
            arg1: 0,
            lane: Some(1),
        },
        lane: Some(1),
        association: Some(LaneAssociation { rv: 10, sid: 20 }),
        sid_hint: Some(20),
        sid_match: Some(true),
        scope: Some(ScopeTrace::new(1, 1)),
    };

    let unmatched_commit = PolicyLaneRecord {
        spec: commit_spec,
        event: PolicyEvent {
            kind: PolicyEventKind::Commit,
            arg0: 0,
            arg1: 0,
            lane: Some(2),
        },
        lane: Some(2),
        association: None,
        sid_hint: None,
        sid_match: None,
        scope: None,
    };

    let mismatched_rollback = PolicyLaneRecord {
        spec: rollback_spec,
        event: PolicyEvent {
            kind: PolicyEventKind::Rollback,
            arg0: 0,
            arg1: 0,
            lane: Some(3),
        },
        lane: Some(3),
        association: Some(LaneAssociation { rv: 11, sid: 30 }),
        sid_hint: Some(99),
        sid_match: Some(false),
        scope: Some(ScopeTrace::new(2, 2)),
    };

    let records = vec![matching_commit, unmatched_commit, mismatched_rollback];
    let summary = mgmt_policy_summary(&records);

    assert_eq!(summary.commits, 2);
    assert_eq!(summary.rollbacks, 1);
    assert_eq!(summary.unmatched_lanes, 1);
    assert_eq!(summary.sid_mismatches, 1);
    assert!(summary.has_alerts(), "anomalies should trigger alerts");
    assert_eq!(summary.total(), 3);
}
