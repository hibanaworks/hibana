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
    pub(super) fn route_reentry_scope(self) -> bool {
        self.route_reentry()
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

#[derive(Clone, Copy)]
pub(super) struct ScopeFrameLabelMasks {
    pub(super) arm_frame_label_masks: [FrameLabelMask; 2],
}

#[derive(Clone, Copy)]
pub(super) struct ScopeFrameLabelScratch {
    meta: ScopeFrameLabelMeta,
    masks: ScopeFrameLabelMasks,
}

#[derive(Clone, Copy)]
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

    #[inline]
    pub(super) fn meta_mut(&mut self) -> &mut ScopeFrameLabelMeta {
        &mut self.meta
    }

    #[inline]
    pub(super) const fn view(&self) -> ScopeFrameLabelView<'_> {
        ScopeFrameLabelView {
            meta: self.meta,
            masks: &self.masks,
        }
    }

    #[inline]
    pub(super) fn record_arm_frame_label(&mut self, arm: u8, frame_label: u8) {
        self.meta
            .record_arm_frame_label(&mut self.masks, arm, frame_label);
    }

    #[inline]
    pub(super) fn record_dispatch_arm_frame_label_mask(
        &mut self,
        arm: u8,
        frame_label_mask: FrameLabelMask,
    ) {
        self.meta
            .record_dispatch_arm_frame_label_mask(&mut self.masks, arm, frame_label_mask);
    }

    #[inline]
    pub(super) fn exclude_controller_arm_frame_label_from_evidence(
        &mut self,
        arm: u8,
        frame_label: u8,
    ) {
        self.meta
            .exclude_controller_arm_frame_label_from_evidence(arm, frame_label);
    }
}

impl ScopeFrameLabelMeta {
    #[inline]
    fn controller_evidence_excluded(self, arm: u8, frame_label: u8) -> bool {
        match arm {
            0 => {
                self.controller_frame_labels[0] == frame_label
                    && (self.flags & Self::FLAG_CONTROLLER_ARM0_EVIDENCE_EXCLUDED) != 0
            }
            1 => {
                self.controller_frame_labels[1] == frame_label
                    && (self.flags & Self::FLAG_CONTROLLER_ARM1_EVIDENCE_EXCLUDED) != 0
            }
            _ => false,
        }
    }

    #[inline]
    fn arm_contains_evidence_frame_label(
        self,
        masks: &ScopeFrameLabelMasks,
        arm: u8,
        frame_label: u8,
    ) -> bool {
        let arm_idx = arm as usize;
        arm_idx < masks.arm_frame_label_masks.len()
            && masks.arm_frame_label_masks[arm_idx].contains_frame_label(frame_label)
            && !self.controller_evidence_excluded(arm, frame_label)
    }

    #[inline]
    pub(super) fn evidence_arm_for_frame_label(
        &self,
        masks: &ScopeFrameLabelMasks,
        frame_label: u8,
    ) -> Option<u8> {
        let left = self.arm_contains_evidence_frame_label(masks, 0, frame_label);
        let right = self.arm_contains_evidence_frame_label(masks, 1, frame_label);
        if left == right {
            return None;
        }
        if left {
            return Some(0);
        }
        Some(1)
    }

    #[inline]
    pub(super) fn frame_hint_mask(&self, masks: &ScopeFrameLabelMasks) -> FrameLabelMask {
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
        arm: u8,
        frame_label: u8,
    ) {
        if (arm as usize) < masks.arm_frame_label_masks.len() {
            masks.arm_frame_label_masks[arm as usize] |=
                FrameLabelMask::from_frame_label(frame_label);
        }
    }

    #[inline]
    pub(super) fn record_dispatch_arm_frame_label_mask(
        &mut self,
        masks: &mut ScopeFrameLabelMasks,
        arm: u8,
        frame_label_mask: FrameLabelMask,
    ) {
        if (arm as usize) < masks.arm_frame_label_masks.len() {
            masks.arm_frame_label_masks[arm as usize] |= frame_label_mask;
        }
    }

    #[inline]
    pub(super) fn exclude_controller_arm_frame_label_from_evidence(
        &mut self,
        arm: u8,
        frame_label: u8,
    ) {
        match arm {
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
    pub(super) fn evidence_arm_for_frame_label(self, frame_label: u8) -> Option<u8> {
        self.meta
            .evidence_arm_for_frame_label(self.masks, frame_label)
    }

    #[inline]
    pub(super) fn frame_hint_mask(self) -> FrameLabelMask {
        self.meta.frame_hint_mask(self.masks)
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
            2..=u8::MAX => crate::invariant(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{FrameLabelMask, ScopeFrameLabelMeta, ScopeFrameLabelScratch, ScopeFrameLabelView};

    #[test]
    fn overlapping_frame_label_is_not_route_evidence() {
        let mut scratch = ScopeFrameLabelScratch::EMPTY;
        scratch.record_arm_frame_label(0, 7);
        scratch.record_arm_frame_label(1, 7);
        let meta = scratch.view();

        assert_eq!(meta.evidence_arm_for_frame_label(7), None);
        assert!(!meta.frame_hint_mask().contains_frame_label(7));
    }

    #[test]
    fn unique_frame_label_remains_route_evidence() {
        let mut scratch = ScopeFrameLabelScratch::EMPTY;
        scratch.record_arm_frame_label(0, 7);
        scratch.record_arm_frame_label(1, 8);
        let meta = scratch.view();

        assert_eq!(meta.evidence_arm_for_frame_label(7), Some(0));
        assert_eq!(meta.evidence_arm_for_frame_label(8), Some(1));
        assert!(meta.frame_hint_mask().contains_frame_label(7));
        assert!(meta.frame_hint_mask().contains_frame_label(8));
        assert!(!FrameLabelMask::EMPTY.contains_frame_label(7));
    }

    #[test]
    fn controller_frame_label_exclusion_does_not_need_duplicate_masks() {
        let mut scratch = ScopeFrameLabelScratch::EMPTY;
        scratch.meta_mut().controller_frame_labels[0] = 7;
        scratch.meta_mut().flags |= ScopeFrameLabelMeta::FLAG_CONTROLLER_ARM0;
        scratch.record_arm_frame_label(0, 7);
        scratch.exclude_controller_arm_frame_label_from_evidence(0, 7);
        let meta = scratch.view();

        assert_eq!(meta.evidence_arm_for_frame_label(7), None);
        assert!(meta.frame_hint_mask().contains_frame_label(7));
    }

    #[test]
    fn scope_frame_label_meta_size_budget() {
        assert_eq!(core::mem::size_of::<ScopeFrameLabelMeta>(), 5);
        assert_eq!(core::mem::align_of::<ScopeFrameLabelMeta>(), 1);
        assert!(core::mem::size_of::<ScopeFrameLabelView<'_>>() <= 16);
        assert!(core::mem::size_of::<ScopeFrameLabelScratch>() <= 72);
    }
}
