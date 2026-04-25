#![cfg(feature = "std")]

#[path = "common/check_support.rs"]
mod check_support;
#[path = "common/observe_support.rs"]
mod observe_support;

use check_support::policy_check_summary;
use core::cell::UnsafeCell;
use hibana::substrate::tap::TapEvent;
use observe_support::{
    LANE_ACQUIRE_ID, LANE_RELEASE_ID, POLICY_ABORT_ID, POLICY_COMMIT_ID, POLICY_TX_ABORT_ID,
    PolicyEventDomain, PolicyEventKind, policy_lane_trace,
};

const RING_EVENTS: usize = 128;

std::thread_local! {
    static POLICY_TRACE_STORAGE: UnsafeCell<[TapEvent; RING_EVENTS]> =
        const { UnsafeCell::new([TapEvent::zero(); RING_EVENTS]) };
}

fn with_policy_trace_storage<R>(f: impl FnOnce(&'static mut [TapEvent; RING_EVENTS]) -> R) -> R {
    POLICY_TRACE_STORAGE.with(|storage| unsafe {
        let storage = &mut *storage.get();
        storage.fill(TapEvent::zero());
        f(storage)
    })
}

fn raw_event(ts: u32, id: u16) -> TapEvent {
    TapEvent {
        ts,
        id,
        ..TapEvent::zero()
    }
}

#[test]
fn policy_lane_trace_matches_lane_assignments() {
    with_policy_trace_storage(|storage| {
        let rv: u32 = 7;
        let sid: u32 = 0x1234;
        let lane: u16 = 3;
        let causal = TapEvent::make_causal_key(lane as u8, 1);

        storage[0] = raw_event(1, LANE_ACQUIRE_ID)
            .with_arg0(rv)
            .with_arg1(((sid as u32) << 16) | lane as u32);
        storage[1] = raw_event(2, POLICY_ABORT_ID)
            .with_causal_key(causal)
            .with_arg0(0xAA)
            .with_arg1(sid);
        storage[2] = raw_event(3, POLICY_TX_ABORT_ID)
            .with_causal_key(causal)
            .with_arg0(sid)
            .with_arg1(9);
        storage[3] = raw_event(4, POLICY_COMMIT_ID)
            .with_causal_key(causal)
            .with_arg0(sid)
            .with_arg1(42);
        storage[4] = raw_event(5, LANE_RELEASE_ID)
            .with_arg0(rv)
            .with_arg1(((sid as u32) << 16) | lane as u32);

        let records = policy_lane_trace(storage, 0, 5);
        assert_eq!(
            records.len(),
            3,
            "expected abort, tx-abort, and commit entries"
        );

        let abort = records.get(0).expect("abort record");
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

        let tx_abort = records.get(1).expect("tx abort record");
        assert_eq!(
            tx_abort.lane,
            Some(lane),
            "tx-abort lane should match acquisition"
        );
        assert!(
            tx_abort.lane.is_none() || tx_abort.has_association,
            "tx-abort should have active lane association"
        );
        assert_eq!(tx_abort.sid_hint, Some(sid), "tx-abort carries sid hint");
        assert_eq!(
            tx_abort.sid_match,
            Some(true),
            "tx-abort sid should align with lane association"
        );
        assert_eq!(
            tx_abort.kind,
            PolicyEventKind::TxAbort,
            "tx-abort record should surface policy event kind"
        );
        assert_eq!(
            tx_abort.domain,
            PolicyEventDomain::Policy,
            "tx-abort event must be marked as policy domain"
        );
        assert_eq!(tx_abort.policy_id, POLICY_TX_ABORT_ID);

        let commit = records.get(2).expect("commit record");
        assert_eq!(
            commit.lane,
            Some(lane),
            "commit lane should match acquisition"
        );
        assert!(
            commit.lane.is_none() || commit.has_association,
            "commit should have active lane association"
        );
        assert_eq!(commit.sid_hint, Some(sid), "commit carries sid hint");
        assert_eq!(
            commit.sid_match,
            Some(true),
            "commit sid should align with lane association"
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

        let summary = policy_check_summary(storage, 0, 5);
        assert_eq!(summary.policy_tx_abort, 1);
        assert_eq!(summary.policy_lane_total, 3);
        assert_eq!(summary.policy_lane_matched, 3);
        assert_eq!(summary.policy_lane_mismatched, 0);
        assert_eq!(summary.policy_sid_matched, 3);
        assert_eq!(summary.policy_sid_mismatched, 0);
    });
}
