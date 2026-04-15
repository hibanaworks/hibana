//! Route fact helpers derived during typestate lowering.

use super::facts::{
    JumpReason, LocalAction, LocalNode, MAX_STATES, StateIndex, state_index_to_usize,
};
use crate::{eff, global::const_dsl::ScopeId};

pub(super) const MAX_PREFIX_ACTIONS: usize = eff::meta::MAX_EFF_NODES;
pub(super) const PREFIX_KIND_SEND: u8 = 0;
pub(super) const PREFIX_KIND_LOCAL: u8 = 1;

#[derive(Clone, Copy, Debug)]
pub(super) struct PrefixAction {
    pub kind: u8,
    pub peer: u8,
    pub label: u8,
    pub lane: u8,
}

impl PrefixAction {
    pub(super) const EMPTY: Self = Self {
        kind: 0,
        peer: 0,
        label: 0,
        lane: 0,
    };
}

const fn actions_equivalent(
    left: LocalAction,
    right: LocalAction,
    left_next: StateIndex,
    right_next: StateIndex,
) -> bool {
    match (left, right) {
        (
            LocalAction::Send {
                peer: left_peer,
                label: left_label,
                lane: left_lane,
                ..
            },
            LocalAction::Send {
                peer: right_peer,
                label: right_label,
                lane: right_lane,
                ..
            },
        ) => left_peer == right_peer && left_label == right_label && left_lane == right_lane,
        (
            LocalAction::Recv {
                peer: left_peer,
                label: left_label,
                lane: left_lane,
                ..
            },
            LocalAction::Recv {
                peer: right_peer,
                label: right_label,
                lane: right_lane,
                ..
            },
        ) => left_peer == right_peer && left_label == right_label && left_lane == right_lane,
        (
            LocalAction::Local {
                label: left_label,
                lane: left_lane,
                ..
            },
            LocalAction::Local {
                label: right_label,
                lane: right_lane,
                ..
            },
        ) => left_label == right_label && left_lane == right_lane,
        (LocalAction::Terminate, LocalAction::Terminate) => true,
        (
            LocalAction::Jump {
                reason: left_reason,
            },
            LocalAction::Jump {
                reason: right_reason,
            },
        ) => jump_reason_eq(left_reason, right_reason) && left_next.raw() == right_next.raw(),
        _ => false,
    }
}

const fn jump_reason_eq(left: JumpReason, right: JumpReason) -> bool {
    matches!(
        (left, right),
        (JumpReason::RouteArmEnd, JumpReason::RouteArmEnd)
            | (JumpReason::LoopContinue, JumpReason::LoopContinue)
            | (JumpReason::LoopBreak, JumpReason::LoopBreak)
            | (
                JumpReason::PassiveObserverBranch,
                JumpReason::PassiveObserverBranch
            )
    )
}

#[inline(never)]
pub(super) fn arm_sequences_equal(
    nodes: &[LocalNode],
    scope_end: StateIndex,
    arm0_entry: StateIndex,
    arm1_entry: StateIndex,
) -> bool {
    if arm0_entry.is_max() || arm1_entry.is_max() {
        return false;
    }
    let end = state_index_to_usize(scope_end);
    let mut idx0 = state_index_to_usize(arm0_entry);
    let mut idx1 = state_index_to_usize(arm1_entry);
    let mut steps = 0usize;
    while steps < MAX_STATES {
        if idx0 < end {
            let node0 = nodes[idx0];
            if matches!(
                node0.action(),
                LocalAction::Jump {
                    reason: JumpReason::RouteArmEnd | JumpReason::LoopBreak
                }
            ) {
                idx0 = end;
            }
        }
        if idx1 < end {
            let node1 = nodes[idx1];
            if matches!(
                node1.action(),
                LocalAction::Jump {
                    reason: JumpReason::RouteArmEnd | JumpReason::LoopBreak
                }
            ) {
                idx1 = end;
            }
        }
        let at_end0 = idx0 >= end;
        let at_end1 = idx1 >= end;
        if at_end0 && at_end1 {
            return true;
        }
        if at_end0 || at_end1 {
            return false;
        }
        let node0 = nodes[idx0];
        let node1 = nodes[idx1];
        if !actions_equivalent(node0.action(), node1.action(), node0.next(), node1.next()) {
            return false;
        }
        let next0 = node0.next();
        let next1 = node1.next();
        idx0 = if next0.is_max() {
            end
        } else {
            state_index_to_usize(next0)
        };
        idx1 = if next1.is_max() {
            end
        } else {
            state_index_to_usize(next1)
        };
        steps += 1;
    }
    false
}

#[inline(never)]
pub(super) fn continuations_equivalent(
    nodes: &[LocalNode],
    scope_end: StateIndex,
    left_entry: StateIndex,
    right_entry: StateIndex,
) -> bool {
    if left_entry.raw() == right_entry.raw() {
        return true;
    }
    arm_sequences_equal(nodes, scope_end, left_entry, right_entry)
}

#[inline(always)]
pub(super) fn arm_common_prefix_end(
    nodes: &[LocalNode],
    scope: ScopeId,
    scope_end: StateIndex,
    arm0_entry: StateIndex,
    arm1_entry: StateIndex,
) -> (StateIndex, StateIndex, usize) {
    if arm0_entry.is_max() || arm1_entry.is_max() {
        return (arm0_entry, arm1_entry, 0);
    }
    let end = state_index_to_usize(scope_end);
    let scope_raw = scope.raw();
    let mut idx0 = arm0_entry;
    let mut idx1 = arm1_entry;
    let mut prefix_len = 0usize;

    loop {
        let mut idx0_us = state_index_to_usize(idx0);
        let mut idx1_us = state_index_to_usize(idx1);

        if idx0_us < end {
            let node0 = nodes[idx0_us];
            if matches!(
                node0.action(),
                LocalAction::Jump {
                    reason: JumpReason::RouteArmEnd | JumpReason::LoopBreak
                }
            ) {
                idx0_us = end;
                idx0 = scope_end;
            }
        }
        if idx1_us < end {
            let node1 = nodes[idx1_us];
            if matches!(
                node1.action(),
                LocalAction::Jump {
                    reason: JumpReason::RouteArmEnd | JumpReason::LoopBreak
                }
            ) {
                idx1_us = end;
                idx1 = scope_end;
            }
        }

        let at_end0 = idx0_us >= end;
        let at_end1 = idx1_us >= end;
        if at_end0 || at_end1 {
            return (
                if at_end0 { scope_end } else { idx0 },
                if at_end1 { scope_end } else { idx1 },
                prefix_len,
            );
        }

        let node0 = nodes[idx0_us];
        let node1 = nodes[idx1_us];
        if node0.scope().raw() != scope_raw || node1.scope().raw() != scope_raw {
            return (idx0, idx1, prefix_len);
        }
        if !actions_equivalent(node0.action(), node1.action(), node0.next(), node1.next()) {
            return (idx0, idx1, prefix_len);
        }

        let next0 = node0.next();
        let next1 = node1.next();
        prefix_len += 1;
        idx0 = if next0.is_max() { scope_end } else { next0 };
        idx1 = if next1.is_max() { scope_end } else { next1 };
    }
}

pub(super) const fn prefix_action_eq(left: PrefixAction, right: PrefixAction) -> bool {
    left.kind == right.kind
        && left.peer == right.peer
        && left.label == right.label
        && left.lane == right.lane
}
