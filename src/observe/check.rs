//! Streaming checker for tap events (no_std / no_alloc).
//!
//! The checker maintains a fixed-state summary of the tap stream so that the
//! control/data plane can be validated without replaying the entire trace.  The
//! current implementation keeps per-flow counters for AMPST cancellation, loop,
//! policy, and rollback safety.

use core::cell::Cell;

use super::ids;
use crate::observe::core::PolicyEventKind;
use crate::observe::core::TapEvent;
use crate::observe::core::{PolicyEventSpec, policy_event_spec};
use crate::{
    endpoint::LocalFailureReason, observe::local::LocalActionFailure, runtime::consts::LANES_MAX,
};

#[cfg(test)]
use std::thread_local;

const LANES_MAX_USIZE: usize = LANES_MAX as usize;
const LOOP_INDEX_LIMIT: usize = 64;

const fn zero_u64_cell_array() -> [Cell<u64>; LANES_MAX_USIZE] {
    [const { Cell::new(0) }; LANES_MAX_USIZE]
}

const fn zero_u32_cell_array() -> [Cell<u32>; LANES_MAX_USIZE] {
    [const { Cell::new(0) }; LANES_MAX_USIZE]
}

struct CheckState {
    cancel_begin: Cell<u32>,
    cancel_ack: Cell<u32>,
    cancel_inflight: Cell<i32>,
    lane_acquire: Cell<u32>,
    lane_release: Cell<u32>,
    lane_inflight: Cell<i32>,
    rollback_req: Cell<u32>,
    rollback_ok: Cell<u32>,
    rollback_inflight: Cell<i32>,
    loop_continue: Cell<u32>,
    loop_break: Cell<u32>,
    loop_inflight_continue: Cell<i32>,
    loop_inflight_break: Cell<i32>,
    loop_pending_continue: [Cell<u64>; LANES_MAX_USIZE],
    loop_pending_break: [Cell<u64>; LANES_MAX_USIZE],
    policy_abort: Cell<u32>,
    policy_trap: Cell<u32>,
    policy_annot: Cell<u32>,
    policy_effect: Cell<u32>,
    policy_effect_ok: Cell<u32>,
    policy_commit: Cell<u32>,
    policy_rollback: Cell<u32>,
    lane_sid: [Cell<u32>; LANES_MAX_USIZE],
    policy_lane_total: Cell<u32>,
    policy_lane_match: Cell<u32>,
    policy_lane_mismatch: Cell<u32>,
    policy_sid_match: Cell<u32>,
    policy_sid_mismatch: Cell<u32>,
    local_failures: Cell<u32>,
    unexpected_local_failure: Cell<bool>,
}

impl CheckState {
    const fn new() -> Self {
        Self {
            cancel_begin: Cell::new(0),
            cancel_ack: Cell::new(0),
            cancel_inflight: Cell::new(0),
            rollback_req: Cell::new(0),
            rollback_ok: Cell::new(0),
            rollback_inflight: Cell::new(0),
            loop_continue: Cell::new(0),
            loop_break: Cell::new(0),
            loop_inflight_continue: Cell::new(0),
            loop_inflight_break: Cell::new(0),
            loop_pending_continue: zero_u64_cell_array(),
            loop_pending_break: zero_u64_cell_array(),
            policy_abort: Cell::new(0),
            policy_trap: Cell::new(0),
            policy_annot: Cell::new(0),
            policy_effect: Cell::new(0),
            policy_effect_ok: Cell::new(0),
            policy_commit: Cell::new(0),
            policy_rollback: Cell::new(0),
            lane_sid: zero_u32_cell_array(),
            policy_lane_total: Cell::new(0),
            policy_lane_match: Cell::new(0),
            policy_lane_mismatch: Cell::new(0),
            policy_sid_match: Cell::new(0),
            policy_sid_mismatch: Cell::new(0),
            local_failures: Cell::new(0),
            unexpected_local_failure: Cell::new(false),
            lane_acquire: Cell::new(0),
            lane_release: Cell::new(0),
            lane_inflight: Cell::new(0),
        }
    }

    #[inline(always)]
    fn add_u32(cell: &Cell<u32>, delta: u32) {
        cell.set(cell.get().wrapping_add(delta));
    }

    #[inline(always)]
    fn add_i32(cell: &Cell<i32>, delta: i32) {
        cell.set(cell.get().wrapping_add(delta));
    }

    #[cfg(test)]
    fn reset(&self) {
        self.cancel_begin.set(0);
        self.cancel_ack.set(0);
        self.cancel_inflight.set(0);
        self.lane_acquire.set(0);
        self.lane_release.set(0);
        self.lane_inflight.set(0);
        self.rollback_req.set(0);
        self.rollback_ok.set(0);
        self.rollback_inflight.set(0);
        self.loop_continue.set(0);
        self.loop_break.set(0);
        self.loop_inflight_continue.set(0);
        self.loop_inflight_break.set(0);
        for lane in &self.loop_pending_continue {
            lane.set(0);
        }
        for lane in &self.loop_pending_break {
            lane.set(0);
        }
        self.policy_abort.set(0);
        self.policy_trap.set(0);
        self.policy_annot.set(0);
        self.policy_effect.set(0);
        self.policy_effect_ok.set(0);
        self.policy_commit.set(0);
        self.policy_rollback.set(0);
        for lane in &self.lane_sid {
            lane.set(0);
        }
        self.policy_lane_total.set(0);
        self.policy_lane_match.set(0);
        self.policy_lane_mismatch.set(0);
        self.policy_sid_match.set(0);
        self.policy_sid_mismatch.set(0);
        self.local_failures.set(0);
        self.unexpected_local_failure.set(false);
    }

    fn record_policy_lane(&self, spec: PolicyEventSpec, event: TapEvent) {
        let lane_marker = event.causal_role();
        if lane_marker == 0 {
            return;
        }

        let lane_idx = lane_marker.saturating_sub(1) as usize;
        Self::add_u32(&self.policy_lane_total, 1);

        if lane_idx >= LANES_MAX_USIZE {
            Self::add_u32(&self.policy_lane_mismatch, 1);
            return;
        }

        let active_sid_marker = self.lane_sid[lane_idx].get();
        if active_sid_marker == 0 {
            Self::add_u32(&self.policy_lane_mismatch, 1);
            return;
        }

        Self::add_u32(&self.policy_lane_match, 1);

        if let Some(expected_sid) = spec.sid_hint_from_tap(event)
            && expected_sid != 0
        {
            let active_sid = active_sid_marker.wrapping_sub(1);
            if active_sid == expected_sid {
                Self::add_u32(&self.policy_sid_match, 1);
            } else {
                Self::add_u32(&self.policy_sid_mismatch, 1);
            }
        }
    }

    fn record_policy_event(&self, spec: PolicyEventSpec, event: TapEvent) {
        match spec.kind {
            PolicyEventKind::Abort => {
                Self::add_u32(&self.policy_abort, 1);
            }
            PolicyEventKind::Trap => {
                Self::add_u32(&self.policy_trap, 1);
            }
            PolicyEventKind::Annotate => {
                Self::add_u32(&self.policy_annot, 1);
            }
            PolicyEventKind::Effect => {
                Self::add_u32(&self.policy_effect, 1);
            }
            PolicyEventKind::EffectOk => {
                Self::add_u32(&self.policy_effect_ok, 1);
            }
            PolicyEventKind::Commit => {
                Self::add_u32(&self.policy_commit, 1);
            }
            PolicyEventKind::Rollback => {
                Self::add_u32(&self.policy_rollback, 1);
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
                Self::add_u32(&self.cancel_begin, 1);
                Self::add_i32(&self.cancel_inflight, 1);
            }
            id if id == ids::CANCEL_ACK => {
                Self::add_u32(&self.cancel_ack, 1);
                Self::add_i32(&self.cancel_inflight, -1);
            }
            id if id == ids::LANE_ACQUIRE => {
                Self::add_u32(&self.lane_acquire, 1);
                Self::add_i32(&self.lane_inflight, 1);
                let lane_idx = (event.arg1 & 0xFFFF) as usize;
                let sid = event.arg1 >> 16;
                if lane_idx < LANES_MAX_USIZE {
                    self.lane_sid[lane_idx].set(sid.wrapping_add(1));
                }
            }
            id if id == ids::LANE_RELEASE => {
                Self::add_u32(&self.lane_release, 1);
                Self::add_i32(&self.lane_inflight, -1);
                let lane_idx = (event.arg1 & 0xFFFF) as usize;
                if lane_idx < LANES_MAX_USIZE {
                    self.lane_sid[lane_idx].set(0);
                }
            }
            id if id == ids::ROLLBACK_REQ => {
                Self::add_u32(&self.rollback_req, 1);
                Self::add_i32(&self.rollback_inflight, 1);
            }
            id if id == ids::ROLLBACK_OK => {
                Self::add_u32(&self.rollback_ok, 1);
                Self::add_i32(&self.rollback_inflight, -1);
            }
            id if id == ids::LOOP_DECISION => {
                let lane = ((event.arg1 >> 16) & 0xFFFF) as usize;
                let idx = ((event.arg1 >> 8) & 0xFF) as usize;
                let decision = (event.arg1 & 0xFF) as u8;
                let within_bounds = lane < LANES_MAX_USIZE && idx < LOOP_INDEX_LIMIT;
                if decision == 1 {
                    Self::add_u32(&self.loop_continue, 1);
                    if within_bounds {
                        let mask = 1u64 << idx;
                        let prev = self.loop_pending_continue[lane].get();
                        self.loop_pending_continue[lane].set(prev ^ mask);
                        if (prev & mask) == 0 {
                            Self::add_i32(&self.loop_inflight_continue, 1);
                        } else {
                            Self::add_i32(&self.loop_inflight_continue, -1);
                        }
                    }
                } else {
                    Self::add_u32(&self.loop_break, 1);
                    if within_bounds {
                        let mask = 1u64 << idx;
                        let prev = self.loop_pending_break[lane].get();
                        self.loop_pending_break[lane].set(prev ^ mask);
                        if (prev & mask) == 0 {
                            Self::add_i32(&self.loop_inflight_break, 1);
                        } else {
                            Self::add_i32(&self.loop_inflight_break, -1);
                        }
                    }
                }
            }
            id if id == ids::LOCAL_ACTION_FAIL => {
                Self::add_u32(&self.local_failures, 1);
                if LocalActionFailure::from_tap(event)
                    .map(|failure| failure.reason == LocalFailureReason::INTERNAL)
                    .unwrap_or(false)
                {
                    self.unexpected_local_failure.set(true);
                }
            }
            _ => {}
        }
    }

    #[cfg(test)]
    fn snapshot(&self) -> CheckReport {
        let cancel_begin = self.cancel_begin.get();
        let cancel_ack = self.cancel_ack.get();
        let cancel_inflight = self.cancel_inflight.get();
        let lane_acquire = self.lane_acquire.get();
        let lane_release = self.lane_release.get();
        let lane_inflight = self.lane_inflight.get();
        let rollback_req = self.rollback_req.get();
        let rollback_ok = self.rollback_ok.get();
        let rollback_inflight = self.rollback_inflight.get();
        let loop_continue = self.loop_continue.get();
        let loop_break = self.loop_break.get();
        let loop_inflight_continue = self.loop_inflight_continue.get();
        let loop_inflight_break = self.loop_inflight_break.get();
        let loop_balanced = loop_inflight_continue == 0 && loop_inflight_break == 0;
        let policy_abort = self.policy_abort.get();
        let policy_trap = self.policy_trap.get();
        let policy_annot = self.policy_annot.get();
        let policy_effect = self.policy_effect.get();
        let policy_effect_ok = self.policy_effect_ok.get();
        let policy_commit = self.policy_commit.get();
        let policy_rollback = self.policy_rollback.get();
        let policy_lane_total = self.policy_lane_total.get();
        let policy_lane_match = self.policy_lane_match.get();
        let policy_lane_mismatch = self.policy_lane_mismatch.get();
        let policy_sid_match = self.policy_sid_match.get();
        let policy_sid_mismatch = self.policy_sid_mismatch.get();
        let local_failures = self.local_failures.get();
        let unexpected_local_failure = self.unexpected_local_failure.get();

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

#[cfg(not(test))]
static STATE: CheckState = CheckState::new();

#[cfg(not(test))]
unsafe impl Sync for CheckState {}

#[cfg(test)]
thread_local! {
    static STATE: CheckState = const { CheckState::new() };
}

#[inline(always)]
fn with_state<R>(f: impl FnOnce(&CheckState) -> R) -> R {
    #[cfg(test)]
    {
        STATE.with(f)
    }
    #[cfg(not(test))]
    {
        f(&STATE)
    }
}

/// Reset the streaming checker state back to zero.
#[cfg(test)]
fn reset() {
    with_state(CheckState::reset);
}

/// Feed a tap event into the streaming checker.
pub(super) fn feed(event: TapEvent) {
    with_state(|state| state.observe(event));
}

/// Snapshot the current checker summary.
#[cfg(test)]
fn snapshot() -> CheckReport {
    with_state(CheckState::snapshot)
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

    #[test]
    fn effect_init_does_not_break_cancel_balance() {
        use crate::observe::events;
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
