//! Scope-evidence owners for route selection.

use super::authority::RouteDecisionToken;
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
    #[cfg(test)]
    pub(super) scope_id: ScopeId,
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
        #[cfg(test)]
        scope_id: ScopeId::none(),
        loop_meta: ScopeLoopMeta::EMPTY,
        recv_frame_label: 0,
        recv_arm: 0,
        controller_frame_labels: [0; 2],
        arm_frame_label_masks: [FrameLabelMask::EMPTY; 2],
        evidence_arm_frame_label_masks: [FrameLabelMask::EMPTY; 2],
        flags: 0,
    };

    #[inline]
    #[cfg(test)]
    pub(super) fn scope_id(self) -> ScopeId {
        self.scope_id
    }

    #[inline]
    pub(super) fn loop_meta(self) -> ScopeLoopMeta {
        self.loop_meta
    }

    #[inline]
    pub(super) fn matches_current_recv_frame_label(self, frame_label: u8) -> bool {
        (self.flags & Self::FLAG_CURRENT_RECV_FRAME_LABEL) != 0
            && self.recv_frame_label == frame_label
    }

    #[inline]
    #[cfg(test)]
    pub(super) fn current_recv_arm_for_frame_label(self, frame_label: u8) -> Option<u8> {
        if self.matches_current_recv_frame_label(frame_label)
            && (self.flags & Self::FLAG_CURRENT_RECV_ARM) != 0
        {
            Some(self.recv_arm)
        } else {
            None
        }
    }

    #[inline]
    pub(super) fn matches_frame_hint(self, frame_label: u8) -> bool {
        self.frame_hint_mask().contains_frame_label(frame_label)
    }

    #[inline]
    #[cfg(test)]
    pub(super) fn controller_arm_for_frame_label(self, frame_label: u8) -> Option<u8> {
        if (self.flags & Self::FLAG_CONTROLLER_ARM0) != 0
            && self.controller_frame_labels[0] == frame_label
        {
            return Some(0);
        }
        if (self.flags & Self::FLAG_CONTROLLER_ARM1) != 0
            && self.controller_frame_labels[1] == frame_label
        {
            return Some(1);
        }
        None
    }

    #[inline]
    pub(super) fn arm_for_frame_label(self, frame_label: u8) -> Option<u8> {
        if self.arm_frame_label_masks[0].contains_frame_label(frame_label) {
            return Some(0);
        }
        if self.arm_frame_label_masks[1].contains_frame_label(frame_label) {
            return Some(1);
        }
        None
    }

    #[inline]
    pub(super) fn evidence_arm_for_frame_label(self, frame_label: u8) -> Option<u8> {
        if self.evidence_arm_frame_label_masks[0].contains_frame_label(frame_label) {
            return Some(0);
        }
        if self.evidence_arm_frame_label_masks[1].contains_frame_label(frame_label) {
            return Some(1);
        }
        None
    }

    #[inline]
    pub(super) fn binding_evidence_arm_for_frame_label(self, frame_label: u8) -> Option<u8> {
        if self.matches_current_recv_frame_label(frame_label)
            && (self.flags & Self::FLAG_CURRENT_RECV_BINDING_EXCLUDED) != 0
        {
            return None;
        }
        self.evidence_arm_for_frame_label(frame_label)
    }

    #[inline]
    pub(super) fn binding_evidence_frame_label_mask_for_arm(self, arm: u8) -> FrameLabelMask {
        let arm_idx = arm as usize;
        if arm_idx >= self.evidence_arm_frame_label_masks.len() {
            return FrameLabelMask::EMPTY;
        }
        let mut mask = self.evidence_arm_frame_label_masks[arm_idx];
        if (self.flags & Self::FLAG_CURRENT_RECV_BINDING_EXCLUDED) != 0
            && (self.flags & Self::FLAG_CURRENT_RECV_ARM) != 0
            && self.recv_arm == arm
        {
            mask.remove_frame_label(self.recv_frame_label);
        }
        mask
    }

    #[inline]
    pub(super) fn binding_demux_frame_label_mask_for_arm(self, arm: u8) -> FrameLabelMask {
        let arm_idx = arm as usize;
        if arm_idx >= self.arm_frame_label_masks.len() {
            return FrameLabelMask::EMPTY;
        }
        self.arm_frame_label_masks[arm_idx]
    }

    #[inline]
    pub(super) fn preferred_binding_frame_label_mask(
        self,
        preferred_arm: Option<u8>,
    ) -> FrameLabelMask {
        preferred_arm
            .map(|arm| self.binding_demux_frame_label_mask_for_arm(arm))
            .unwrap_or_else(|| self.frame_hint_mask())
    }

    #[inline]
    pub(super) fn preferred_binding_frame_label(self, preferred_arm: Option<u8>) -> Option<u8> {
        if let Some(arm) = preferred_arm {
            return self
                .binding_evidence_frame_label_mask_for_arm(arm)
                .singleton_frame_label();
        }
        let arm0 = self
            .binding_evidence_frame_label_mask_for_arm(0)
            .singleton_frame_label();
        let arm1 = self
            .binding_evidence_frame_label_mask_for_arm(1)
            .singleton_frame_label();
        match (arm0, arm1) {
            (Some(label), None) | (None, Some(label)) => Some(label),
            (Some(left), Some(right)) if left == right => Some(left),
            _ => None,
        }
    }

    #[inline]
    pub(super) fn frame_hint_mask(self) -> FrameLabelMask {
        let mut mask = self.arm_frame_label_masks[0] | self.arm_frame_label_masks[1];
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

    #[cfg(test)]
    #[inline]
    pub(super) fn record_dispatch_arm_frame_label(&mut self, arm: u8, frame_label: u8) {
        if (arm as usize) < self.arm_frame_label_masks.len() {
            self.arm_frame_label_masks[arm as usize] |=
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
    pub(super) ack: Option<RouteDecisionToken>,
    pub(super) hint_frame_label: u8,
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
