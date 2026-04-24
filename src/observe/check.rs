//! Streaming checker for tap events (test-only).
//!
//! The checker derives a compact summary directly from the actual tap ring so
//! tests observe the same event world that runtime code emits.

use std::collections::{HashMap, HashSet};

use super::core::{PolicyEventKind, PolicyEventSpec, TapEvent, TapRing, policy_event_spec};
use super::ids;
use crate::{endpoint::LocalFailureReason, observe::local::LocalActionFailure};

const LOOP_INDEX_LIMIT: usize = 64;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct CheckReport {
    pub abort_begin: u32,
    pub abort_ack: u32,
    pub abort_inflight: i32,
    pub abort_balanced: bool,
    pub lane_acquire: u32,
    pub lane_release: u32,
    pub lane_inflight: i32,
    pub lane_balanced: bool,
    pub state_restore_req: u32,
    pub state_restore_ok: u32,
    pub state_restore_inflight: i32,
    pub state_restore_balanced: bool,
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
    pub policy_tx_abort: u32,
    pub policy_state_restore: u32,
    pub policy_lane_total: u32,
    pub policy_lane_matched: u32,
    pub policy_lane_mismatched: u32,
    pub policy_sid_matched: u32,
    pub policy_sid_mismatched: u32,
    pub local_action_failures: u32,
    pub local_action_unexpected: bool,
}

#[derive(Default)]
struct CheckSummary {
    report: CheckReport,
}

impl CheckSummary {
    #[inline(always)]
    fn add_u32(cell: &mut u32, delta: u32) {
        *cell = cell.wrapping_add(delta);
    }

    #[inline(always)]
    fn add_i32(cell: &mut i32, delta: i32) {
        *cell = cell.wrapping_add(delta);
    }

    fn record_policy_lane(
        &mut self,
        spec: PolicyEventSpec,
        event: TapEvent,
        lane_sid: &HashMap<usize, u32>,
    ) {
        let lane_marker = event.causal_role();
        if lane_marker == 0 {
            return;
        }

        let lane_idx = lane_marker.saturating_sub(1) as usize;
        Self::add_u32(&mut self.report.policy_lane_total, 1);

        let Some(&active_sid) = lane_sid.get(&lane_idx) else {
            Self::add_u32(&mut self.report.policy_lane_mismatched, 1);
            return;
        };

        Self::add_u32(&mut self.report.policy_lane_matched, 1);

        if let Some(expected_sid) = spec.sid_hint_from_tap(event)
            && expected_sid != 0
        {
            if active_sid == expected_sid {
                Self::add_u32(&mut self.report.policy_sid_matched, 1);
            } else {
                Self::add_u32(&mut self.report.policy_sid_mismatched, 1);
            }
        }
    }

    fn record_policy_event(
        &mut self,
        spec: PolicyEventSpec,
        event: TapEvent,
        lane_sid: &HashMap<usize, u32>,
    ) {
        match spec.kind {
            PolicyEventKind::Abort => Self::add_u32(&mut self.report.policy_abort, 1),
            PolicyEventKind::Trap => Self::add_u32(&mut self.report.policy_trap, 1),
            PolicyEventKind::Annotate => Self::add_u32(&mut self.report.policy_annot, 1),
            PolicyEventKind::Effect => Self::add_u32(&mut self.report.policy_effect, 1),
            PolicyEventKind::EffectOk => Self::add_u32(&mut self.report.policy_effect_ok, 1),
            PolicyEventKind::Commit => Self::add_u32(&mut self.report.policy_commit, 1),
            PolicyEventKind::TxAbort => Self::add_u32(&mut self.report.policy_tx_abort, 1),
            PolicyEventKind::StateRestore => {
                Self::add_u32(&mut self.report.policy_state_restore, 1)
            }
        }
        self.record_policy_lane(spec, event, lane_sid);
    }

    fn observe(
        &mut self,
        event: TapEvent,
        lane_sid: &mut HashMap<usize, u32>,
        loop_pending_continue: &mut HashSet<(usize, usize)>,
        loop_pending_break: &mut HashSet<(usize, usize)>,
    ) {
        if let Some(spec) = policy_event_spec(event.id) {
            self.record_policy_event(spec, event, lane_sid);
            return;
        }

        match event.id {
            id if id == ids::ABORT_BEGIN => {
                Self::add_u32(&mut self.report.abort_begin, 1);
                Self::add_i32(&mut self.report.abort_inflight, 1);
            }
            id if id == ids::ABORT_ACK => {
                Self::add_u32(&mut self.report.abort_ack, 1);
                Self::add_i32(&mut self.report.abort_inflight, -1);
            }
            id if id == ids::LANE_ACQUIRE => {
                Self::add_u32(&mut self.report.lane_acquire, 1);
                Self::add_i32(&mut self.report.lane_inflight, 1);
                let lane_idx = (event.arg1 & 0xFFFF) as usize;
                let sid = event.arg1 >> 16;
                lane_sid.insert(lane_idx, sid);
            }
            id if id == ids::LANE_RELEASE => {
                Self::add_u32(&mut self.report.lane_release, 1);
                Self::add_i32(&mut self.report.lane_inflight, -1);
                let lane_idx = (event.arg1 & 0xFFFF) as usize;
                lane_sid.remove(&lane_idx);
            }
            id if id == ids::STATE_RESTORE_REQ => {
                Self::add_u32(&mut self.report.state_restore_req, 1);
                Self::add_i32(&mut self.report.state_restore_inflight, 1);
            }
            id if id == ids::STATE_RESTORE_OK => {
                Self::add_u32(&mut self.report.state_restore_ok, 1);
                Self::add_i32(&mut self.report.state_restore_inflight, -1);
            }
            id if id == ids::LOOP_DECISION => {
                let lane = ((event.arg1 >> 16) & 0xFFFF) as usize;
                let idx = ((event.arg1 >> 8) & 0xFF) as usize;
                if idx >= LOOP_INDEX_LIMIT {
                    return;
                }
                let decision = (event.arg1 & 0xFF) as u8;
                let key = (lane, idx);
                if decision == 1 {
                    Self::add_u32(&mut self.report.loop_continue, 1);
                    if loop_pending_continue.insert(key) {
                        Self::add_i32(&mut self.report.loop_inflight_continue, 1);
                    } else {
                        loop_pending_continue.remove(&key);
                        Self::add_i32(&mut self.report.loop_inflight_continue, -1);
                    }
                } else {
                    Self::add_u32(&mut self.report.loop_break, 1);
                    if loop_pending_break.insert(key) {
                        Self::add_i32(&mut self.report.loop_inflight_break, 1);
                    } else {
                        loop_pending_break.remove(&key);
                        Self::add_i32(&mut self.report.loop_inflight_break, -1);
                    }
                }
            }
            id if id == ids::LOCAL_ACTION_FAIL => {
                Self::add_u32(&mut self.report.local_action_failures, 1);
                if LocalActionFailure::from_tap(event)
                    .map(|failure| failure.reason == LocalFailureReason::INTERNAL)
                    .unwrap_or(false)
                {
                    self.report.local_action_unexpected = true;
                }
            }
            _ => {}
        }
    }

    fn finish(mut self) -> CheckReport {
        self.report.abort_balanced = self.report.abort_inflight == 0;
        self.report.lane_balanced = self.report.lane_inflight == 0;
        self.report.state_restore_balanced = self.report.state_restore_inflight == 0;
        self.report.loop_balanced =
            self.report.loop_inflight_continue == 0 && self.report.loop_inflight_break == 0;
        self.report
    }
}

fn snapshot_events(events: impl IntoIterator<Item = TapEvent>) -> CheckReport {
    let mut summary = CheckSummary::default();
    let mut lane_sid = HashMap::<usize, u32>::new();
    let mut loop_pending_continue = HashSet::<(usize, usize)>::new();
    let mut loop_pending_break = HashSet::<(usize, usize)>::new();

    for event in events {
        summary.observe(
            event,
            &mut lane_sid,
            &mut loop_pending_continue,
            &mut loop_pending_break,
        );
    }

    summary.finish()
}

#[inline(always)]
pub(super) fn feed(event: TapEvent) {
    let _ = event;
}

#[cfg(test)]
fn snapshot_ring(ring: &TapRing<'_>) -> CheckReport {
    let mut cursor = 0usize;
    snapshot_events(ring.events_since(&mut cursor, Some))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        observe::{core::TapEvent, events},
        runtime::consts::RING_EVENTS,
    };

    fn report_for(write: impl FnOnce(&TapRing<'_>)) -> CheckReport {
        let mut storage = [TapEvent::zero(); RING_EVENTS];
        let ring = TapRing::from_storage(&mut storage);
        write(&ring);
        snapshot_ring(&ring)
    }

    #[test]
    fn effect_init_does_not_break_abort_balance() {
        let report = report_for(|ring| {
            ring.push(events::EffectInit::new(1, 7, 3));
            ring.push(events::AbortBegin::new(2, 7, 11));
            ring.push(events::AbortAck::new(3, 7, 11));
        });

        assert_eq!(report.abort_begin, 1);
        assert_eq!(report.abort_ack, 1);
        assert!(report.abort_balanced);
    }

    #[test]
    fn abort_begin_ack_balance_survives_initialisation() {
        let report = report_for(|ring| {
            ring.push(events::EffectInit::new(10, 42, 1));
            ring.push(events::AbortBegin::new(11, 42, 0));
            ring.push(events::AbortAck::new(12, 42, 0));
        });

        assert_eq!(report.abort_begin, 1);
        assert_eq!(report.abort_ack, 1);
        assert!(report.abort_balanced);
    }

    #[test]
    fn state_restore_balance_with_acknowledgement() {
        let report = report_for(|ring| {
            ring.push(events::EffectInit::new(20, 9, 2));
            ring.push(events::StateRestoreReq::new(21, 9, 0));
            ring.push(events::StateRestoreOk::new(22, 9, 0));
        });

        assert_eq!(report.state_restore_req, 1);
        assert_eq!(report.state_restore_ok, 1);
        assert!(report.state_restore_balanced);
    }

    #[test]
    fn lane_acquire_release_balance() {
        let report = report_for(|ring| {
            ring.push(events::LaneAcquire::new(30, 1, 0, 0));
            ring.push(events::LaneRelease::new(31, 1, 0, 0));
        });

        assert_eq!(report.lane_acquire, 1);
        assert_eq!(report.lane_release, 1);
        assert!(report.lane_balanced);
    }

    #[test]
    fn tx_abort_events_participate_in_policy_lane_accounting() {
        let sid = 0x1234;
        let lane = 2u16;
        let causal = TapEvent::make_causal_key((lane as u8) + 1, 0);

        let report = report_for(|ring| {
            ring.push(events::LaneAcquire::new(40, 7, sid, lane));
            ring.push(
                TapEvent {
                    ts: 41,
                    id: ids::POLICY_TX_ABORT,
                    ..TapEvent::zero()
                }
                .with_causal_key(causal)
                .with_arg0(sid)
                .with_arg1(9),
            );
            ring.push(events::LaneRelease::new(42, 7, sid, lane));
        });

        assert_eq!(report.policy_tx_abort, 1);
        assert_eq!(report.policy_lane_total, 1);
        assert_eq!(report.policy_lane_matched, 1);
        assert_eq!(report.policy_sid_matched, 1);
        assert_eq!(report.policy_sid_mismatched, 0);
    }

    #[test]
    fn commit_events_participate_in_policy_lane_and_sid_accounting() {
        let sid = 0xBABE;
        let lane = 1u16;
        let causal = TapEvent::make_causal_key((lane as u8) + 1, 0);

        let report = report_for(|ring| {
            ring.push(events::LaneAcquire::new(50, 9, sid, lane));
            ring.push(
                TapEvent {
                    ts: 51,
                    id: ids::POLICY_COMMIT,
                    ..TapEvent::zero()
                }
                .with_causal_key(causal)
                .with_arg0(sid)
                .with_arg1(3),
            );
            ring.push(events::LaneRelease::new(52, 9, sid, lane));
        });

        assert_eq!(report.policy_commit, 1);
        assert_eq!(report.policy_lane_total, 1);
        assert_eq!(report.policy_lane_matched, 1);
        assert_eq!(report.policy_sid_matched, 1);
        assert_eq!(report.policy_sid_mismatched, 0);
    }
}
