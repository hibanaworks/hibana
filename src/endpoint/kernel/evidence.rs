//! Scope-evidence owners for route selection.

use super::authority::RouteArmToken;
use crate::{global::const_dsl::ScopeId, transport::FrameLabelMask};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct RouteArmState {
    pub(super) scope: ScopeId,
    pub(super) arm: u8,
}

impl RouteArmState {
    pub(super) const EMPTY: Self = Self {
        scope: ScopeId::none(),
        arm: 0,
    };
}

#[derive(Clone, Copy)]
pub(super) struct ScopeLoopMeta {
    pub(super) flags: u8,
}

impl ScopeLoopMeta {
    pub(super) const FLAG_SCOPE_ACTIVE: u8 = 1;
    pub(super) const FLAG_SCOPE_LINGER: u8 = 1 << 1;
    pub(super) const FLAG_CONTROL_SCOPE: u8 = 1 << 2;
    pub(super) const FLAG_CONTINUE_HAS_RECV: u8 = 1 << 3;
    pub(super) const FLAG_BREAK_HAS_RECV: u8 = 1 << 4;

    pub(super) const EMPTY: Self = Self { flags: 0 };

    #[inline]
    pub(super) fn scope_active(self) -> bool {
        (self.flags & Self::FLAG_SCOPE_ACTIVE) != 0
    }

    #[inline]
    pub(super) fn scope_linger(self) -> bool {
        (self.flags & Self::FLAG_SCOPE_LINGER) != 0
    }

    #[inline]
    pub(super) fn control_scope(self) -> bool {
        (self.flags & Self::FLAG_CONTROL_SCOPE) != 0
    }

    #[inline]
    pub(super) fn loop_label_scope(self) -> bool {
        self.control_scope() || self.scope_linger()
    }

    #[inline]
    pub(super) fn continue_has_recv(self) -> bool {
        (self.flags & Self::FLAG_CONTINUE_HAS_RECV) != 0
    }

    #[inline]
    pub(super) fn break_has_recv(self) -> bool {
        (self.flags & Self::FLAG_BREAK_HAS_RECV) != 0
    }

    #[inline]
    pub(super) fn arm_has_recv(self, arm: u8) -> bool {
        match arm {
            0 => self.continue_has_recv(),
            1 => self.break_has_recv(),
            _ => false,
        }
    }

    #[inline]
    pub(super) fn recvless_ready(self) -> bool {
        (self.scope_active() || self.scope_linger())
            && (!self.continue_has_recv() || !self.break_has_recv())
    }
}

#[derive(Clone, Copy)]
pub(super) struct ScopeFrameLabelMeta {
    pub(super) loop_meta: ScopeLoopMeta,
    pub(super) recv_frame_label: u8,
    pub(super) recv_arm: u8,
    pub(super) controller_frame_labels: [u8; 2],
    pub(super) arm_frame_label_masks: [FrameLabelMask; 2],
    pub(super) evidence_arm_frame_label_masks: [FrameLabelMask; 2],
    pub(super) flags: u8,
}

impl ScopeFrameLabelMeta {
    pub(super) const FLAG_CURRENT_RECV_FRAME_LABEL: u8 = 1;
    pub(super) const FLAG_CURRENT_RECV_ARM: u8 = 1 << 1;
    pub(super) const FLAG_CONTROLLER_ARM0: u8 = 1 << 2;
    pub(super) const FLAG_CONTROLLER_ARM1: u8 = 1 << 3;
    pub(super) const FLAG_CURRENT_RECV_BINDING_EXCLUDED: u8 = 1 << 4;

    pub(super) const EMPTY: Self = Self {
        loop_meta: ScopeLoopMeta::EMPTY,
        recv_frame_label: 0,
        recv_arm: 0,
        controller_frame_labels: [0; 2],
        arm_frame_label_masks: [FrameLabelMask::EMPTY; 2],
        evidence_arm_frame_label_masks: [FrameLabelMask::EMPTY; 2],
        flags: 0,
    };

    #[inline]
    pub(super) fn loop_meta(self) -> ScopeLoopMeta {
        self.loop_meta
    }

    #[inline]
    pub(super) fn evidence_arm_for_frame_label(self, frame_label: u8) -> Option<u8> {
        let left = self.evidence_arm_frame_label_masks[0].contains_frame_label(frame_label);
        let right = self.evidence_arm_frame_label_masks[1].contains_frame_label(frame_label);
        if left == right {
            return None;
        }
        if left {
            return Some(0);
        }
        Some(1)
    }

    #[inline]
    pub(super) fn frame_hint_mask(self) -> FrameLabelMask {
        let shared = self.arm_frame_label_masks[0] & self.arm_frame_label_masks[1];
        let mut mask =
            (self.arm_frame_label_masks[0] | self.arm_frame_label_masks[1]).without(shared);
        if (self.flags & Self::FLAG_CURRENT_RECV_FRAME_LABEL) != 0 {
            mask |= FrameLabelMask::from_frame_label(self.recv_frame_label);
        }
        mask
    }

    #[inline]
    pub(super) fn record_arm_frame_label(&mut self, arm: u8, frame_label: u8) {
        if (arm as usize) < self.arm_frame_label_masks.len() {
            self.arm_frame_label_masks[arm as usize] |=
                FrameLabelMask::from_frame_label(frame_label);
            self.evidence_arm_frame_label_masks[arm as usize] |=
                FrameLabelMask::from_frame_label(frame_label);
        }
    }

    #[inline]
    pub(super) fn record_dispatch_arm_frame_label_mask(
        &mut self,
        arm: u8,
        frame_label_mask: FrameLabelMask,
    ) {
        if (arm as usize) < self.arm_frame_label_masks.len() {
            self.arm_frame_label_masks[arm as usize] |= frame_label_mask;
        }
    }

    #[inline]
    pub(super) fn clear_evidence_arm_frame_label(&mut self, arm: u8, frame_label: u8) {
        if (arm as usize) < self.evidence_arm_frame_label_masks.len() {
            self.evidence_arm_frame_label_masks[arm as usize].remove_frame_label(frame_label);
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct ScopeEvidence {
    pub(super) ack: Option<RouteArmToken>,
    pub(super) hint_frame_label: u8,
    pub(super) hint_lane: u8,
    pub(super) ready_arm_mask: u8,
    pub(super) poll_ready_arm_mask: u8,
    pub(super) flags: u8,
}

impl ScopeEvidence {
    pub(super) const ARM0_READY: u8 = 1 << 0;
    pub(super) const ARM1_READY: u8 = 1 << 1;
    pub(super) const FLAG_ACK_CONFLICT: u8 = 1;
    pub(super) const FLAG_HINT_CONFLICT: u8 = 1 << 1;
    pub(super) const FLAG_HAS_HINT: u8 = 1 << 2;
    pub(super) const EMPTY: Self = Self {
        ack: None,
        hint_frame_label: 0,
        hint_lane: 0,
        ready_arm_mask: 0,
        poll_ready_arm_mask: 0,
        flags: 0,
    };

    #[inline]
    pub(super) const fn arm_bit(arm: u8) -> u8 {
        match arm {
            0 => Self::ARM0_READY,
            1 => Self::ARM1_READY,
            _ => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{FrameLabelMask, ScopeFrameLabelMeta};

    #[test]
    fn overlapping_frame_label_is_not_route_evidence() {
        let mut meta = ScopeFrameLabelMeta::EMPTY;
        meta.record_arm_frame_label(0, 7);
        meta.record_arm_frame_label(1, 7);

        assert_eq!(meta.evidence_arm_for_frame_label(7), None);
        assert!(!meta.frame_hint_mask().contains_frame_label(7));
    }

    #[test]
    fn unique_frame_label_remains_route_evidence() {
        let mut meta = ScopeFrameLabelMeta::EMPTY;
        meta.record_arm_frame_label(0, 7);
        meta.record_arm_frame_label(1, 8);

        assert_eq!(meta.evidence_arm_for_frame_label(7), Some(0));
        assert_eq!(meta.evidence_arm_for_frame_label(8), Some(1));
        assert!(meta.frame_hint_mask().contains_frame_label(7));
        assert!(meta.frame_hint_mask().contains_frame_label(8));
        assert!(!FrameLabelMask::EMPTY.contains_frame_label(7));
    }
}
