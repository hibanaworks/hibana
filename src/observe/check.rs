//! Streaming checker for tap events (no_std / no_alloc).
//!
//! The checker maintains a fixed-state summary of the tap stream so that the
//! control/data plane can be validated without replaying the entire trace.  The
//! current implementation keeps per-flow counters for AMPST cancellation, loop,
//! policy, and rollback safety.

use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering};

use super::ids;
use crate::observe::core::PolicyEventKind;
use crate::observe::core::TapEvent;
use crate::observe::core::{PolicyEventSpec, policy_event_spec};
use crate::{
    endpoint::LocalFailureReason, observe::local::LocalActionFailure, runtime::consts::LANES_MAX,
};

const LANES_MAX_USIZE: usize = LANES_MAX as usize;
const LOOP_INDEX_LIMIT: usize = 64;

const fn zero_atomic_u64_array() -> [AtomicU64; LANES_MAX_USIZE] {
    [
        AtomicU64::new(0),
        AtomicU64::new(0),
        AtomicU64::new(0),
        AtomicU64::new(0),
        AtomicU64::new(0),
        AtomicU64::new(0),
        AtomicU64::new(0),
        AtomicU64::new(0),
        AtomicU64::new(0),
        AtomicU64::new(0),
        AtomicU64::new(0),
        AtomicU64::new(0),
        AtomicU64::new(0),
        AtomicU64::new(0),
        AtomicU64::new(0),
        AtomicU64::new(0),
    ]
}

const fn zero_atomic_u32_array() -> [AtomicU32; LANES_MAX_USIZE] {
    [
        AtomicU32::new(0),
        AtomicU32::new(0),
        AtomicU32::new(0),
        AtomicU32::new(0),
        AtomicU32::new(0),
        AtomicU32::new(0),
        AtomicU32::new(0),
        AtomicU32::new(0),
        AtomicU32::new(0),
        AtomicU32::new(0),
        AtomicU32::new(0),
        AtomicU32::new(0),
        AtomicU32::new(0),
        AtomicU32::new(0),
        AtomicU32::new(0),
        AtomicU32::new(0),
    ]
}

struct CheckState {
    cancel_begin: AtomicU32,
    cancel_ack: AtomicU32,
    cancel_inflight: AtomicI32,
    lane_acquire: AtomicU32,
    lane_release: AtomicU32,
    lane_inflight: AtomicI32,
    rollback_req: AtomicU32,
    rollback_ok: AtomicU32,
    rollback_inflight: AtomicI32,
    loop_continue: AtomicU32,
    loop_break: AtomicU32,
    loop_inflight_continue: AtomicI32,
    loop_inflight_break: AtomicI32,
    loop_pending_continue: [AtomicU64; LANES_MAX_USIZE],
    loop_pending_break: [AtomicU64; LANES_MAX_USIZE],
    policy_abort: AtomicU32,
    policy_trap: AtomicU32,
    policy_annot: AtomicU32,
    policy_effect: AtomicU32,
    policy_effect_ok: AtomicU32,
    policy_commit: AtomicU32,
    policy_rollback: AtomicU32,
    lane_sid: [AtomicU32; LANES_MAX_USIZE],
    policy_lane_total: AtomicU32,
    policy_lane_match: AtomicU32,
    policy_lane_mismatch: AtomicU32,
    policy_sid_match: AtomicU32,
    policy_sid_mismatch: AtomicU32,
    local_failures: AtomicU32,
    unexpected_local_failure: AtomicBool,
}

impl CheckState {
    const fn new() -> Self {
        Self {
            cancel_begin: AtomicU32::new(0),
            cancel_ack: AtomicU32::new(0),
            cancel_inflight: AtomicI32::new(0),
            rollback_req: AtomicU32::new(0),
            rollback_ok: AtomicU32::new(0),
            rollback_inflight: AtomicI32::new(0),
            loop_continue: AtomicU32::new(0),
            loop_break: AtomicU32::new(0),
            loop_inflight_continue: AtomicI32::new(0),
            loop_inflight_break: AtomicI32::new(0),
            loop_pending_continue: zero_atomic_u64_array(),
            loop_pending_break: zero_atomic_u64_array(),
            policy_abort: AtomicU32::new(0),
            policy_trap: AtomicU32::new(0),
            policy_annot: AtomicU32::new(0),
            policy_effect: AtomicU32::new(0),
            policy_effect_ok: AtomicU32::new(0),
            policy_commit: AtomicU32::new(0),
            policy_rollback: AtomicU32::new(0),
            lane_sid: zero_atomic_u32_array(),
            policy_lane_total: AtomicU32::new(0),
            policy_lane_match: AtomicU32::new(0),
            policy_lane_mismatch: AtomicU32::new(0),
            policy_sid_match: AtomicU32::new(0),
            policy_sid_mismatch: AtomicU32::new(0),
            local_failures: AtomicU32::new(0),
            unexpected_local_failure: AtomicBool::new(false),
            lane_acquire: AtomicU32::new(0),
            lane_release: AtomicU32::new(0),
            lane_inflight: AtomicI32::new(0),
        }
    }

    #[cfg(test)]
    fn reset(&self) {
        self.cancel_begin.store(0, Ordering::Relaxed);
        self.cancel_ack.store(0, Ordering::Relaxed);
        self.cancel_inflight.store(0, Ordering::Relaxed);
        self.lane_acquire.store(0, Ordering::Relaxed);
        self.lane_release.store(0, Ordering::Relaxed);
        self.lane_inflight.store(0, Ordering::Relaxed);
        self.rollback_req.store(0, Ordering::Relaxed);
        self.rollback_ok.store(0, Ordering::Relaxed);
        self.rollback_inflight.store(0, Ordering::Relaxed);
        self.loop_continue.store(0, Ordering::Relaxed);
        self.loop_break.store(0, Ordering::Relaxed);
        self.loop_inflight_continue.store(0, Ordering::Relaxed);
        self.loop_inflight_break.store(0, Ordering::Relaxed);
        for lane in &self.loop_pending_continue {
            lane.store(0, Ordering::Relaxed);
        }
        for lane in &self.loop_pending_break {
            lane.store(0, Ordering::Relaxed);
        }
        self.policy_abort.store(0, Ordering::Relaxed);
        self.policy_trap.store(0, Ordering::Relaxed);
        self.policy_annot.store(0, Ordering::Relaxed);
        self.policy_effect.store(0, Ordering::Relaxed);
        self.policy_effect_ok.store(0, Ordering::Relaxed);
        self.policy_commit.store(0, Ordering::Relaxed);
        self.policy_rollback.store(0, Ordering::Relaxed);
        for lane in &self.lane_sid {
            lane.store(0, Ordering::Relaxed);
        }
        self.policy_lane_total.store(0, Ordering::Relaxed);
        self.policy_lane_match.store(0, Ordering::Relaxed);
        self.policy_lane_mismatch.store(0, Ordering::Relaxed);
        self.policy_sid_match.store(0, Ordering::Relaxed);
        self.policy_sid_mismatch.store(0, Ordering::Relaxed);
        self.local_failures.store(0, Ordering::Relaxed);
        self.unexpected_local_failure
            .store(false, Ordering::Relaxed);
    }

    fn record_policy_lane(&self, spec: PolicyEventSpec, event: TapEvent) {
        let lane_marker = event.causal_role();
        if lane_marker == 0 {
            return;
        }

        let lane_idx = lane_marker.saturating_sub(1) as usize;
        self.policy_lane_total.fetch_add(1, Ordering::Relaxed);

        if lane_idx >= LANES_MAX_USIZE {
            self.policy_lane_mismatch.fetch_add(1, Ordering::Relaxed);
            return;
        }

        let active_sid_marker = self.lane_sid[lane_idx].load(Ordering::Relaxed);
        if active_sid_marker == 0 {
            self.policy_lane_mismatch.fetch_add(1, Ordering::Relaxed);
            return;
        }

        self.policy_lane_match.fetch_add(1, Ordering::Relaxed);

        if let Some(expected_sid) = spec.sid_hint_from_tap(event)
            && expected_sid != 0
        {
            let active_sid = active_sid_marker.wrapping_sub(1);
            if active_sid == expected_sid {
                self.policy_sid_match.fetch_add(1, Ordering::Relaxed);
            } else {
                self.policy_sid_mismatch.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    fn record_policy_event(&self, spec: PolicyEventSpec, event: TapEvent) {
        match spec.kind {
            PolicyEventKind::Abort => {
                self.policy_abort.fetch_add(1, Ordering::Relaxed);
            }
            PolicyEventKind::Trap => {
                self.policy_trap.fetch_add(1, Ordering::Relaxed);
            }
            PolicyEventKind::Annotate => {
                self.policy_annot.fetch_add(1, Ordering::Relaxed);
            }
            PolicyEventKind::Effect => {
                self.policy_effect.fetch_add(1, Ordering::Relaxed);
            }
            PolicyEventKind::EffectOk => {
                self.policy_effect_ok.fetch_add(1, Ordering::Relaxed);
            }
            PolicyEventKind::Commit => {
                self.policy_commit.fetch_add(1, Ordering::Relaxed);
            }
            PolicyEventKind::Rollback => {
                self.policy_rollback.fetch_add(1, Ordering::Relaxed);
            }
        }
        self.record_policy_lane(spec, event);
    }

    fn observe(&self, event: TapEvent) {
        if let Some(spec) = policy_event_spec(event.id) {
            self.record_policy_event(spec, event);
            return;
        }
        match event.id {
            id if id == ids::CANCEL_BEGIN => {
                self.cancel_begin.fetch_add(1, Ordering::Relaxed);
                self.cancel_inflight.fetch_add(1, Ordering::Relaxed);
            }
            id if id == ids::CANCEL_ACK => {
                self.cancel_ack.fetch_add(1, Ordering::Relaxed);
                self.cancel_inflight.fetch_sub(1, Ordering::Relaxed);
            }
            id if id == ids::LANE_ACQUIRE => {
                self.lane_acquire.fetch_add(1, Ordering::Relaxed);
                self.lane_inflight.fetch_add(1, Ordering::Relaxed);
                let lane_idx = (event.arg1 & 0xFFFF) as usize;
                let sid = event.arg1 >> 16;
                if lane_idx < LANES_MAX_USIZE {
                    self.lane_sid[lane_idx].store(sid.wrapping_add(1), Ordering::Relaxed);
                }
            }
            id if id == ids::LANE_RELEASE => {
                self.lane_release.fetch_add(1, Ordering::Relaxed);
                self.lane_inflight.fetch_sub(1, Ordering::Relaxed);
                let lane_idx = (event.arg1 & 0xFFFF) as usize;
                if lane_idx < LANES_MAX_USIZE {
                    self.lane_sid[lane_idx].store(0, Ordering::Relaxed);
                }
            }
            id if id == ids::ROLLBACK_REQ => {
                self.rollback_req.fetch_add(1, Ordering::Relaxed);
                self.rollback_inflight.fetch_add(1, Ordering::Relaxed);
            }
            id if id == ids::ROLLBACK_OK => {
                self.rollback_ok.fetch_add(1, Ordering::Relaxed);
                self.rollback_inflight.fetch_sub(1, Ordering::Relaxed);
            }
            id if id == ids::LOOP_DECISION => {
                let lane = ((event.arg1 >> 16) & 0xFFFF) as usize;
                let idx = ((event.arg1 >> 8) & 0xFF) as usize;
                let decision = (event.arg1 & 0xFF) as u8;
                let within_bounds = lane < LANES_MAX_USIZE && idx < LOOP_INDEX_LIMIT;
                if decision == 1 {
                    self.loop_continue.fetch_add(1, Ordering::Relaxed);
                    if within_bounds {
                        let mask = 1u64 << idx;
                        let prev =
                            self.loop_pending_continue[lane].fetch_xor(mask, Ordering::Relaxed);
                        if (prev & mask) == 0 {
                            self.loop_inflight_continue.fetch_add(1, Ordering::Relaxed);
                        } else {
                            self.loop_inflight_continue.fetch_sub(1, Ordering::Relaxed);
                        }
                    }
                } else {
                    self.loop_break.fetch_add(1, Ordering::Relaxed);
                    if within_bounds {
                        let mask = 1u64 << idx;
                        let prev = self.loop_pending_break[lane].fetch_xor(mask, Ordering::Relaxed);
                        if (prev & mask) == 0 {
                            self.loop_inflight_break.fetch_add(1, Ordering::Relaxed);
                        } else {
                            self.loop_inflight_break.fetch_sub(1, Ordering::Relaxed);
                        }
                    }
                }
            }
            id if id == ids::LOCAL_ACTION_FAIL => {
                self.local_failures.fetch_add(1, Ordering::Relaxed);
                if LocalActionFailure::from_tap(event)
                    .map(|failure| failure.reason == LocalFailureReason::INTERNAL)
                    .unwrap_or(false)
                {
                    self.unexpected_local_failure.store(true, Ordering::Relaxed);
                }
            }
            _ => {}
        }
    }

    #[cfg(test)]
    fn snapshot(&self) -> CheckReport {
        let cancel_begin = self.cancel_begin.load(Ordering::Relaxed);
        let cancel_ack = self.cancel_ack.load(Ordering::Relaxed);
        let cancel_inflight = self.cancel_inflight.load(Ordering::Relaxed);
        let lane_acquire = self.lane_acquire.load(Ordering::Relaxed);
        let lane_release = self.lane_release.load(Ordering::Relaxed);
        let lane_inflight = self.lane_inflight.load(Ordering::Relaxed);
        let rollback_req = self.rollback_req.load(Ordering::Relaxed);
        let rollback_ok = self.rollback_ok.load(Ordering::Relaxed);
        let rollback_inflight = self.rollback_inflight.load(Ordering::Relaxed);
        let loop_continue = self.loop_continue.load(Ordering::Relaxed);
        let loop_break = self.loop_break.load(Ordering::Relaxed);
        let loop_inflight_continue = self.loop_inflight_continue.load(Ordering::Relaxed);
        let loop_inflight_break = self.loop_inflight_break.load(Ordering::Relaxed);
        let loop_balanced = loop_inflight_continue == 0 && loop_inflight_break == 0;
        let policy_abort = self.policy_abort.load(Ordering::Relaxed);
        let policy_trap = self.policy_trap.load(Ordering::Relaxed);
        let policy_annot = self.policy_annot.load(Ordering::Relaxed);
        let policy_effect = self.policy_effect.load(Ordering::Relaxed);
        let policy_effect_ok = self.policy_effect_ok.load(Ordering::Relaxed);
        let policy_commit = self.policy_commit.load(Ordering::Relaxed);
        let policy_rollback = self.policy_rollback.load(Ordering::Relaxed);
        let policy_lane_total = self.policy_lane_total.load(Ordering::Relaxed);
        let policy_lane_match = self.policy_lane_match.load(Ordering::Relaxed);
        let policy_lane_mismatch = self.policy_lane_mismatch.load(Ordering::Relaxed);
        let policy_sid_match = self.policy_sid_match.load(Ordering::Relaxed);
        let policy_sid_mismatch = self.policy_sid_mismatch.load(Ordering::Relaxed);
        let local_failures = self.local_failures.load(Ordering::Relaxed);
        let unexpected_local_failure = self.unexpected_local_failure.load(Ordering::Relaxed);

        CheckReport {
            cancel_begin,
            cancel_ack,
            cancel_inflight,
            cancel_balanced: cancel_inflight == 0,
            lane_acquire,
            lane_release,
            lane_inflight,
            lane_balanced: lane_inflight == 0,
            rollback_req,
            rollback_ok,
            rollback_inflight,
            rollback_balanced: rollback_inflight == 0,
            loop_continue,
            loop_break,
            loop_inflight_continue,
            loop_inflight_break,
            loop_balanced,
            policy_abort,
            policy_trap,
            policy_annot,
            policy_effect,
            policy_effect_ok,
            policy_commit,
            policy_rollback,
            policy_lane_total,
            policy_lane_matched: policy_lane_match,
            policy_lane_mismatched: policy_lane_mismatch,
            policy_sid_matched: policy_sid_match,
            policy_sid_mismatched: policy_sid_mismatch,
            local_action_failures: local_failures,
            local_action_unexpected: unexpected_local_failure,
        }
    }
}

static STATE: CheckState = CheckState::new();

/// Reset the streaming checker state back to zero.
#[cfg(test)]
fn reset() {
    STATE.reset();
}

/// Feed a tap event into the streaming checker.
pub(super) fn feed(event: TapEvent) {
    STATE.observe(event);
}

/// Snapshot the current checker summary.
#[cfg(test)]
fn snapshot() -> CheckReport {
    STATE.snapshot()
}

/// Summary of the streaming checker counters.
///
/// Loop fields track the number of continue/break decisions observed and the
/// outstanding unmatched events. A non-zero `loop_inflight_*` indicates that a
/// decision has been recorded without a matching acknowledgement yet.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg(test)]
struct CheckReport {
    pub cancel_begin: u32,
    pub cancel_ack: u32,
    pub cancel_inflight: i32,
    pub cancel_balanced: bool,
    pub lane_acquire: u32,
    pub lane_release: u32,
    pub lane_inflight: i32,
    pub lane_balanced: bool,
    pub rollback_req: u32,
    pub rollback_ok: u32,
    pub rollback_inflight: i32,
    pub rollback_balanced: bool,
    pub loop_continue: u32,
    pub loop_break: u32,
    pub loop_inflight_continue: i32,
    pub loop_inflight_break: i32,
    pub loop_balanced: bool,
    pub policy_abort: u32,
    pub policy_trap: u32,
    pub policy_annot: u32,
    pub policy_effect: u32,
    pub policy_effect_ok: u32,
    pub policy_commit: u32,
    pub policy_rollback: u32,
    pub policy_lane_total: u32,
    pub policy_lane_matched: u32,
    pub policy_lane_mismatched: u32,
    pub policy_sid_matched: u32,
    pub policy_sid_mismatched: u32,
    pub local_action_failures: u32,
    pub local_action_unexpected: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard, OnceLock};

    fn test_guard() -> MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .expect("test lock")
    }

    #[test]
    fn effect_init_does_not_break_cancel_balance() {
        use crate::observe::events;
        let _guard = test_guard();
        reset();
        feed(events::EffectInit::new(1, 7, 3));
        feed(events::CancelBegin::new(2, 7, 11));
        feed(events::CancelAck::new(3, 7, 11));

        let report = snapshot();
        assert_eq!(report.cancel_begin, 1);
        assert_eq!(report.cancel_ack, 1);
        assert!(report.cancel_balanced);
        reset();
    }

    #[test]
    fn cancel_begin_ack_balance_survives_initialisation() {
        use crate::observe::events;
        let _guard = test_guard();
        reset();
        feed(events::EffectInit::new(10, 42, 1));
        feed(events::CancelBegin::new(11, 42, 0));
        feed(events::CancelAck::new(12, 42, 0));

        let report = snapshot();
        assert_eq!(report.cancel_begin, 1);
        assert_eq!(report.cancel_ack, 1);
        assert!(report.cancel_balanced);
        reset();
    }

    #[test]
    fn rollback_balance_with_acknowledgement() {
        use crate::observe::events;
        let _guard = test_guard();
        reset();
        feed(events::EffectInit::new(20, 9, 2));
        feed(events::RollbackReq::new(21, 9, 0));
        feed(events::RollbackOk::new(22, 9, 0));

        let report = snapshot();
        assert_eq!(report.rollback_req, 1);
        assert_eq!(report.rollback_ok, 1);
        assert!(report.rollback_balanced);
        reset();
    }

    #[test]
    fn lane_acquire_release_balance() {
        use crate::observe::events;
        let _guard = test_guard();
        reset();
        feed(events::LaneAcquire::new(30, 1, 0, 0));
        feed(events::LaneRelease::new(31, 1, 0, 0));

        let report = snapshot();
        assert_eq!(report.lane_acquire, 1);
        assert_eq!(report.lane_release, 1);
        assert!(report.lane_balanced);
        reset();
    }
}
