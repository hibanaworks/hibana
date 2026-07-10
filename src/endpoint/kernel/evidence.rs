//! Scope-evidence owners for route selection.

use super::authority::{Arm, RouteArmToken};
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
pub(super) struct ScopeReentryMeta {
    pub(super) flags: u8,
}

impl ScopeReentryMeta {
    pub(super) const FLAG_SCOPE_ACTIVE: u8 = 1;
    pub(super) const FLAG_ROUTE_REENTRY: u8 = 1 << 1;
    pub(super) const FLAG_ARM0_HAS_RECV: u8 = 1 << 3;
    pub(super) const FLAG_ARM1_HAS_RECV: u8 = 1 << 4;

    #[inline]
    pub(super) fn scope_active(self) -> bool {
        (self.flags & Self::FLAG_SCOPE_ACTIVE) != 0
    }

    #[inline]
    pub(super) fn route_reentry(self) -> bool {
        (self.flags & Self::FLAG_ROUTE_REENTRY) != 0
    }

    #[inline]
    pub(super) fn arm0_has_recv(self) -> bool {
        (self.flags & Self::FLAG_ARM0_HAS_RECV) != 0
    }

    #[inline]
    pub(super) fn arm1_has_recv(self) -> bool {
        (self.flags & Self::FLAG_ARM1_HAS_RECV) != 0
    }

    #[inline]
    pub(super) fn recvless_arm_ready(self) -> bool {
        (self.scope_active() || self.route_reentry())
            && (!self.arm0_has_recv() || !self.arm1_has_recv())
    }
}

#[derive(Clone, Copy)]
pub(super) struct ScopeFrameLabelMeta {
    pub(super) recv_frame_label: u8,
    pub(super) recv_arm: u8,
    pub(super) controller_frame_labels: [u8; 2],
    pub(super) flags: u8,
}

pub(super) struct ScopeFrameLabelMasks {
    pub(super) arm_frame_label_masks: [FrameLabelMask; 2],
}

pub(super) struct ScopeFrameLabelScratch {
    meta: ScopeFrameLabelMeta,
    masks: ScopeFrameLabelMasks,
}

pub(super) struct ScopeFrameLabelView<'a> {
    meta: ScopeFrameLabelMeta,
    masks: &'a ScopeFrameLabelMasks,
}

impl ScopeFrameLabelMeta {
    pub(super) const FLAG_CURRENT_RECV_FRAME_LABEL: u8 = 1;
    pub(super) const FLAG_CURRENT_RECV_ARM: u8 = 1 << 1;
    pub(super) const FLAG_CONTROLLER_ARM0: u8 = 1 << 2;
    pub(super) const FLAG_CONTROLLER_ARM1: u8 = 1 << 3;
    pub(super) const FLAG_CURRENT_RECV_BINDING_EXCLUDED: u8 = 1 << 4;
    const FLAG_CONTROLLER_ARM0_EVIDENCE_EXCLUDED: u8 = 1 << 5;
    const FLAG_CONTROLLER_ARM1_EVIDENCE_EXCLUDED: u8 = 1 << 6;

    pub(super) const EMPTY: Self = Self {
        recv_frame_label: 0,
        recv_arm: 0,
        controller_frame_labels: [0; 2],
        flags: 0,
    };
}

impl ScopeFrameLabelMasks {
    pub(super) const EMPTY: Self = Self {
        arm_frame_label_masks: [FrameLabelMask::EMPTY; 2],
    };
}

impl ScopeFrameLabelScratch {
    pub(super) const EMPTY: Self = Self {
        meta: ScopeFrameLabelMeta::EMPTY,
        masks: ScopeFrameLabelMasks::EMPTY,
    };

    #[inline]
    pub(super) fn clear(&mut self) {
        *self = Self::EMPTY;
    }

    pub(super) fn meta_mut(&mut self) -> &mut ScopeFrameLabelMeta {
        &mut self.meta
    }

    pub(super) const fn view(&self) -> ScopeFrameLabelView<'_> {
        ScopeFrameLabelView {
            meta: self.meta,
            masks: &self.masks,
        }
    }

    #[inline]
    pub(super) fn record_arm_frame_label(&mut self, arm: Arm, frame_label: u8) {
        self.meta
            .record_arm_frame_label(&mut self.masks, arm, frame_label);
    }

    #[inline]
    pub(super) fn record_dispatch_arm_frame_label_mask(
        &mut self,
        arm: Arm,
        frame_label_mask: FrameLabelMask,
    ) {
        self.meta
            .record_dispatch_arm_frame_label_mask(&mut self.masks, arm, frame_label_mask);
    }

    #[inline]
    pub(super) fn exclude_controller_arm_frame_label_from_evidence(
        &mut self,
        arm: Arm,
        frame_label: u8,
    ) {
        self.meta
            .exclude_controller_arm_frame_label_from_evidence(arm, frame_label);
    }
}

impl ScopeFrameLabelMeta {
    #[inline]
    fn controller_evidence_excluded(self, arm: Arm, frame_label: u8) -> bool {
        match arm.as_u8() {
            0 => {
                self.controller_frame_labels[0] == frame_label
                    && (self.flags & Self::FLAG_CONTROLLER_ARM0_EVIDENCE_EXCLUDED) != 0
            }
            1 => {
                self.controller_frame_labels[1] == frame_label
                    && (self.flags & Self::FLAG_CONTROLLER_ARM1_EVIDENCE_EXCLUDED) != 0
            }
            _ => crate::invariant(),
        }
    }

    #[inline]
    fn arm_contains_evidence_frame_label(
        self,
        masks: &ScopeFrameLabelMasks,
        arm: Arm,
        frame_label: u8,
    ) -> bool {
        let arm_idx = arm.as_u8() as usize;
        masks.arm_frame_label_masks[arm_idx].contains_frame_label(frame_label)
            && !self.controller_evidence_excluded(arm, frame_label)
    }

    #[inline]
    pub(super) fn evidence_arm_for_frame_label(
        &self,
        masks: &ScopeFrameLabelMasks,
        frame_label: u8,
    ) -> Option<Arm> {
        let left = self.arm_contains_evidence_frame_label(masks, Arm::LEFT, frame_label);
        let right = self.arm_contains_evidence_frame_label(masks, Arm::RIGHT, frame_label);
        if left == right {
            return None;
        }
        if left {
            return Some(Arm::LEFT);
        }
        Some(Arm::RIGHT)
    }

    #[inline]
    pub(super) fn evidence_frame_label_mask(&self, masks: &ScopeFrameLabelMasks) -> FrameLabelMask {
        let shared = masks.arm_frame_label_masks[0] & masks.arm_frame_label_masks[1];
        let mut mask =
            (masks.arm_frame_label_masks[0] | masks.arm_frame_label_masks[1]).without(shared);
        if (self.flags & Self::FLAG_CURRENT_RECV_FRAME_LABEL) != 0 {
            mask |= FrameLabelMask::from_frame_label(self.recv_frame_label);
        }
        mask
    }

    #[inline]
    pub(super) fn record_arm_frame_label(
        &mut self,
        masks: &mut ScopeFrameLabelMasks,
        arm: Arm,
        frame_label: u8,
    ) {
        masks.arm_frame_label_masks[arm.as_u8() as usize] |=
            FrameLabelMask::from_frame_label(frame_label);
    }

    #[inline]
    pub(super) fn record_dispatch_arm_frame_label_mask(
        &mut self,
        masks: &mut ScopeFrameLabelMasks,
        arm: Arm,
        frame_label_mask: FrameLabelMask,
    ) {
        masks.arm_frame_label_masks[arm.as_u8() as usize] |= frame_label_mask;
    }

    #[inline]
    pub(super) fn exclude_controller_arm_frame_label_from_evidence(
        &mut self,
        arm: Arm,
        frame_label: u8,
    ) {
        match arm.as_u8() {
            0 if self.controller_frame_labels[0] == frame_label => {
                self.flags |= Self::FLAG_CONTROLLER_ARM0_EVIDENCE_EXCLUDED;
            }
            1 if self.controller_frame_labels[1] == frame_label => {
                self.flags |= Self::FLAG_CONTROLLER_ARM1_EVIDENCE_EXCLUDED;
            }
            _ => crate::invariant(),
        }
    }
}

impl ScopeFrameLabelView<'_> {
    #[inline]
    pub(super) fn evidence_arm_for_frame_label(&self, frame_label: u8) -> Option<Arm> {
        self.meta
            .evidence_arm_for_frame_label(self.masks, frame_label)
    }

    #[inline]
    pub(super) fn evidence_frame_label_mask(&self) -> FrameLabelMask {
        self.meta.evidence_frame_label_mask(self.masks)
    }
}

#[derive(Clone, Copy)]
pub(super) struct ScopeEvidence {
    pub(super) ack: Option<RouteArmToken>,
    pub(super) ready_arm_mask: u8,
    pub(super) poll_ready_arm_mask: u8,
    pub(super) flags: u8,
}

impl ScopeEvidence {
    pub(super) const FLAG_ACK_CONFLICT: u8 = 1;
    pub(super) const EMPTY: Self = Self {
        ack: None,
        ready_arm_mask: 0,
        poll_ready_arm_mask: 0,
        flags: 0,
    };

    #[inline]
    pub(super) const fn arm_bit(arm: Arm) -> u8 {
        1 << arm.as_u8()
    }
}

#[cfg(all(test, hibana_repo_tests))]
mod tests;
