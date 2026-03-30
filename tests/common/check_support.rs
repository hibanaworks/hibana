use hibana::substrate::mgmt::tap::TapEvent;

use crate::observe_support::{PolicyEventKind, policy_lane_trace};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct PolicyCheckSummary {
    pub(crate) policy_abort: u32,
    pub(crate) policy_commit: u32,
    pub(crate) policy_rollback: u32,
    pub(crate) policy_lane_total: u32,
    pub(crate) policy_lane_matched: u32,
    pub(crate) policy_lane_mismatched: u32,
    pub(crate) policy_sid_matched: u32,
    pub(crate) policy_sid_mismatched: u32,
}

pub(crate) fn policy_check_summary(
    storage: &[TapEvent],
    start: usize,
    end: usize,
) -> PolicyCheckSummary {
    let (records, _) = policy_lane_trace(storage, start, end);
    let mut summary = PolicyCheckSummary::default();
    summary.policy_lane_total = records.len() as u32;
    for record in records {
        match record.kind {
            PolicyEventKind::Abort => summary.policy_abort += 1,
            PolicyEventKind::Commit => summary.policy_commit += 1,
            PolicyEventKind::Rollback => summary.policy_rollback += 1,
            _ => {}
        }
        if record.lane.is_some() {
            if record.has_association {
                summary.policy_lane_matched += 1;
            } else {
                summary.policy_lane_mismatched += 1;
            }
        }
        match record.sid_match {
            Some(true) => summary.policy_sid_matched += 1,
            Some(false) => summary.policy_sid_mismatched += 1,
            None => {}
        }
    }
    summary
}
