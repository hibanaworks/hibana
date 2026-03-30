//! Binding inbox helpers for endpoint demux.

use crate::{
    binding::{BindingSlot, IncomingClassification},
    global::role_program::MAX_LANES,
};

#[inline]
const fn label_bit(label: u8) -> u128 {
    if label < u128::BITS as u8 {
        1u128 << label
    } else {
        0
    }
}

pub(super) struct BindingInbox {
    pub(super) slots: [[Option<IncomingClassification>; Self::PER_LANE_CAPACITY]; MAX_LANES],
    pub(super) len: [u8; MAX_LANES],
    pub(super) nonempty_mask: u8,
    pub(super) label_masks: [u128; MAX_LANES],
    pub(super) buffered_label_lane_masks: [u8; 128],
}

impl BindingInbox {
    pub(super) const PER_LANE_CAPACITY: usize = 8;
    pub(super) const EMPTY: Self = Self {
        slots: [[None; Self::PER_LANE_CAPACITY]; MAX_LANES],
        len: [0; MAX_LANES],
        nonempty_mask: 0,
        label_masks: [0; MAX_LANES],
        buffered_label_lane_masks: [0; 128],
    };

    #[inline]
    pub(super) fn update_nonempty_mask(&mut self, lane_idx: usize) {
        if lane_idx >= MAX_LANES {
            return;
        }
        let bit = 1u8 << lane_idx;
        if self.len[lane_idx] == 0 {
            self.nonempty_mask &= !bit;
        } else {
            self.nonempty_mask |= bit;
        }
    }

    #[inline]
    pub(super) fn has_buffered_for_lane_mask(&self, lane_mask: u8) -> bool {
        (self.nonempty_mask & lane_mask) != 0
    }

    #[inline]
    pub(super) fn recompute_label_mask(&mut self, lane_idx: usize) {
        if lane_idx >= MAX_LANES {
            return;
        }
        let buffered = self.len[lane_idx] as usize;
        let mut mask = 0u128;
        let mut idx = 0usize;
        while idx < buffered {
            if let Some(classification) = self.slots[lane_idx][idx] {
                mask |= label_bit(classification.label);
            }
            idx += 1;
        }
        self.sync_label_mask(lane_idx, mask);
    }

    #[inline]
    pub(super) fn sync_label_mask(&mut self, lane_idx: usize, new_mask: u128) {
        if lane_idx >= MAX_LANES {
            return;
        }
        let old_mask = self.label_masks[lane_idx];
        if old_mask == new_mask {
            return;
        }
        let lane_bit = 1u8 << lane_idx;
        let mut removed = old_mask & !new_mask;
        while removed != 0 {
            let label = removed.trailing_zeros() as usize;
            self.buffered_label_lane_masks[label] &= !lane_bit;
            removed &= removed - 1;
        }
        let mut added = new_mask & !old_mask;
        while added != 0 {
            let label = added.trailing_zeros() as usize;
            self.buffered_label_lane_masks[label] |= lane_bit;
            added &= added - 1;
        }
        self.label_masks[lane_idx] = new_mask;
    }

    #[inline]
    pub(super) fn buffered_lane_mask_for_labels(&self, label_mask: u128) -> u8 {
        let mut labels = label_mask;
        let mut lane_mask = 0u8;
        while labels != 0 {
            let label = labels.trailing_zeros() as usize;
            lane_mask |= self.buffered_label_lane_masks[label];
            labels &= labels - 1;
        }
        lane_mask
    }

    #[inline]
    pub(super) fn remove_buffered_at(
        &mut self,
        lane_idx: usize,
        idx: usize,
    ) -> Option<IncomingClassification> {
        if lane_idx >= MAX_LANES {
            return None;
        }
        let buffered = self.len[lane_idx] as usize;
        if idx >= buffered {
            return None;
        }
        let classification = self.slots[lane_idx][idx]
            .take()
            .expect("binding inbox buffered slot must be populated");
        let mut shift = idx + 1;
        while shift < buffered {
            self.slots[lane_idx][shift - 1] = self.slots[lane_idx][shift];
            shift += 1;
        }
        self.slots[lane_idx][buffered - 1] = None;
        self.len[lane_idx] = (buffered - 1) as u8;
        self.recompute_label_mask(lane_idx);
        self.update_nonempty_mask(lane_idx);
        Some(classification)
    }

    #[inline]
    pub(super) fn take_or_poll<B: BindingSlot>(
        &mut self,
        binding: &mut B,
        lane_idx: usize,
    ) -> Option<IncomingClassification> {
        if lane_idx >= MAX_LANES {
            return None;
        }
        let buffered = self.len[lane_idx] as usize;
        if buffered != 0 {
            return self.remove_buffered_at(lane_idx, 0);
        }
        binding.poll_incoming_for_lane(lane_idx as u8)
    }

    #[inline]
    pub(super) fn push_back(
        &mut self,
        lane_idx: usize,
        classification: IncomingClassification,
    ) -> bool {
        if lane_idx >= MAX_LANES {
            return false;
        }
        let buffered = self.len[lane_idx] as usize;
        if buffered >= Self::PER_LANE_CAPACITY {
            return false;
        }
        self.slots[lane_idx][buffered] = Some(classification);
        self.len[lane_idx] = (buffered + 1) as u8;
        self.nonempty_mask |= 1u8 << lane_idx;
        self.sync_label_mask(
            lane_idx,
            self.label_masks[lane_idx] | label_bit(classification.label),
        );
        true
    }

    #[inline]
    pub(super) fn take_matching_or_poll<B: BindingSlot>(
        &mut self,
        binding: &mut B,
        lane_idx: usize,
        expected_label: u8,
    ) -> Option<IncomingClassification> {
        if lane_idx >= MAX_LANES {
            return None;
        }
        let expected_bit = label_bit(expected_label);
        if (self.label_masks[lane_idx] & expected_bit) != 0 {
            let buffered = self.len[lane_idx] as usize;
            let mut idx = 0usize;
            while idx < buffered {
                if let Some(classification) = self.slots[lane_idx][idx]
                    && classification.label == expected_label
                {
                    return self.remove_buffered_at(lane_idx, idx);
                }
                idx += 1;
            }
            self.recompute_label_mask(lane_idx);
        }

        let mut scans = 0usize;
        while scans < Self::PER_LANE_CAPACITY {
            scans += 1;
            if (self.len[lane_idx] as usize) >= Self::PER_LANE_CAPACITY {
                break;
            }
            let Some(classification) = binding.poll_incoming_for_lane(lane_idx as u8) else {
                break;
            };
            if classification.label == expected_label {
                return Some(classification);
            }
            if !self.push_back(lane_idx, classification) {
                break;
            }
        }
        None
    }

    #[inline]
    pub(super) fn take_matching_mask_or_poll<B: BindingSlot, F: FnMut(u8) -> bool>(
        &mut self,
        binding: &mut B,
        lane_idx: usize,
        label_mask: u128,
        drop_label_mask: u128,
        mut drop_mismatch: F,
    ) -> Option<IncomingClassification> {
        if lane_idx >= MAX_LANES || label_mask == 0 {
            return None;
        }
        let buffered_scan_mask = label_mask | drop_label_mask;
        if (self.label_masks[lane_idx] & buffered_scan_mask) != 0 {
            let mut idx = 0usize;
            while idx < (self.len[lane_idx] as usize) {
                let Some(classification) = self.slots[lane_idx][idx] else {
                    idx += 1;
                    continue;
                };
                let label_bit = label_bit(classification.label);
                if (label_mask & label_bit) != 0 {
                    return self.remove_buffered_at(lane_idx, idx);
                }
                if (drop_label_mask & label_bit) != 0 && drop_mismatch(classification.label) {
                    let _ = self.remove_buffered_at(lane_idx, idx);
                    continue;
                }
                idx += 1;
            }
        }

        let mut scans = 0usize;
        while scans < Self::PER_LANE_CAPACITY {
            scans += 1;
            if (self.len[lane_idx] as usize) >= Self::PER_LANE_CAPACITY {
                break;
            }
            let Some(classification) = binding.poll_incoming_for_lane(lane_idx as u8) else {
                break;
            };
            let label_bit = label_bit(classification.label);
            if (label_mask & label_bit) != 0 {
                return Some(classification);
            }
            if (drop_label_mask & label_bit) != 0 && drop_mismatch(classification.label) {
                continue;
            }
            if !self.push_back(lane_idx, classification) {
                break;
            }
        }
        None
    }

    #[inline]
    pub(super) fn put_back(&mut self, lane_idx: usize, classification: IncomingClassification) {
        if lane_idx >= MAX_LANES {
            return;
        }
        let buffered = self.len[lane_idx] as usize;
        if buffered >= Self::PER_LANE_CAPACITY {
            return;
        }
        let mut idx = buffered;
        while idx > 0 {
            self.slots[lane_idx][idx] = self.slots[lane_idx][idx - 1];
            idx -= 1;
        }
        self.slots[lane_idx][0] = Some(classification);
        self.len[lane_idx] = (buffered + 1) as u8;
        self.nonempty_mask |= 1u8 << lane_idx;
        self.sync_label_mask(
            lane_idx,
            self.label_masks[lane_idx] | label_bit(classification.label),
        );
    }
}
