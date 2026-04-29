//! Binding inbox helpers for endpoint demux.

use crate::{
    binding::{BindingSlot, IngressEvidence},
    global::role_program::{DENSE_LANE_NONE, DenseLaneOrdinal, LaneSet, LaneSetView, LaneWord},
    transport::FrameLabelMask,
};

#[derive(Clone, Copy)]
struct DenseLaneIndex {
    lane_dense_by_lane: *mut DenseLaneOrdinal,
    active_lane_count: DenseLaneOrdinal,
}

impl DenseLaneIndex {
    unsafe fn init_from_parts(
        dst: *mut Self,
        lane_dense_by_lane: *mut DenseLaneOrdinal,
        active_lane_count: usize,
    ) {
        if active_lane_count >= DENSE_LANE_NONE.get() {
            panic!("binding inbox lane count overflow");
        }
        unsafe {
            core::ptr::addr_of_mut!((*dst).lane_dense_by_lane).write(lane_dense_by_lane);
            core::ptr::addr_of_mut!((*dst).active_lane_count)
                .write(DenseLaneOrdinal::new(active_lane_count).expect("active lane count fits"));
        }
    }

    #[inline]
    fn dense_ordinal(&self, lane_idx: usize) -> Option<usize> {
        if lane_idx >= self.active_lane_count.get() {
            return None;
        }
        let dense = unsafe { *self.lane_dense_by_lane.add(lane_idx) };
        if dense == DENSE_LANE_NONE || dense.get() >= self.active_lane_count.get() {
            None
        } else {
            Some(dense.get())
        }
    }

    #[inline]
    fn contains_lane(&self, lane_idx: usize) -> bool {
        self.dense_ordinal(lane_idx).is_some()
    }
}

#[derive(Clone, Copy)]
struct DenseLaneU8Array {
    ptr: *mut u8,
}

impl DenseLaneU8Array {
    unsafe fn init_from_parts(dst: *mut Self, ptr: *mut u8, active_lane_count: usize) {
        unsafe {
            core::ptr::addr_of_mut!((*dst).ptr).write(ptr);
        }
        let mut idx = 0usize;
        while idx < active_lane_count {
            unsafe {
                ptr.add(idx).write(0);
            }
            idx += 1;
        }
    }

    #[inline]
    fn get_value(&self, lanes: &DenseLaneIndex, lane_idx: usize) -> u8 {
        lanes
            .dense_ordinal(lane_idx)
            .map(|dense| unsafe { *self.ptr.add(dense) })
            .unwrap_or(0)
    }

    #[inline]
    fn set_value(&mut self, lanes: &DenseLaneIndex, lane_idx: usize, value: u8) -> bool {
        let Some(dense) = lanes.dense_ordinal(lane_idx) else {
            return false;
        };
        unsafe {
            self.ptr.add(dense).write(value);
        }
        true
    }
}

#[derive(Clone, Copy)]
pub(super) struct DenseLaneFrameLabelMaskArray {
    ptr: *mut FrameLabelMask,
}

impl DenseLaneFrameLabelMaskArray {
    unsafe fn init_from_parts(dst: *mut Self, ptr: *mut FrameLabelMask, active_lane_count: usize) {
        unsafe {
            core::ptr::addr_of_mut!((*dst).ptr).write(ptr);
        }
        let mut idx = 0usize;
        while idx < active_lane_count {
            unsafe {
                ptr.add(idx).write(FrameLabelMask::EMPTY);
            }
            idx += 1;
        }
    }

    #[inline]
    fn get_value(&self, lanes: &DenseLaneIndex, lane_idx: usize) -> FrameLabelMask {
        lanes
            .dense_ordinal(lane_idx)
            .map(|dense| unsafe { *self.ptr.add(dense) })
            .unwrap_or(FrameLabelMask::EMPTY)
    }

    #[inline]
    fn set_value(
        &mut self,
        lanes: &DenseLaneIndex,
        lane_idx: usize,
        value: FrameLabelMask,
    ) -> bool {
        let Some(dense) = lanes.dense_ordinal(lane_idx) else {
            return false;
        };
        unsafe {
            self.ptr.add(dense).write(value);
        }
        true
    }
}

#[derive(Clone, Copy)]
#[repr(C)]
pub(super) struct PackedIngressEvidence {
    channel_lo: u32,
    channel_hi: u32,
    meta: u32,
}

impl PackedIngressEvidence {
    const META_FRAME_LABEL_MASK: u32 = 0xFF;
    const META_INSTANCE_SHIFT: u32 = 8;
    const META_FLAGS_SHIFT: u32 = 24;
    const FLAG_PRESENT: u8 = 1;
    const FLAG_HAS_FIN: u8 = 1 << 1;

    pub(super) const EMPTY: Self = Self {
        channel_lo: 0,
        channel_hi: 0,
        meta: 0,
    };

    #[inline]
    pub(super) const fn is_present(self) -> bool {
        ((self.meta >> Self::META_FLAGS_SHIFT) as u8 & Self::FLAG_PRESENT) != 0
    }

    #[inline]
    pub(super) const fn encode(evidence: IngressEvidence) -> Self {
        let channel_raw = evidence.channel.raw();
        let flags = Self::FLAG_PRESENT | ((evidence.has_fin as u8) << 1);
        Self {
            channel_lo: channel_raw as u32,
            channel_hi: (channel_raw >> 32) as u32,
            meta: (evidence.frame_label.raw() as u32)
                | ((evidence.instance as u32) << Self::META_INSTANCE_SHIFT)
                | ((flags as u32) << Self::META_FLAGS_SHIFT),
        }
    }

    #[inline]
    pub(super) const fn decode(self) -> IngressEvidence {
        let flags = (self.meta >> Self::META_FLAGS_SHIFT) as u8;
        IngressEvidence {
            frame_label: crate::transport::FrameLabel::new(
                (self.meta & Self::META_FRAME_LABEL_MASK) as u8,
            ),
            instance: (self.meta >> Self::META_INSTANCE_SHIFT) as u16,
            has_fin: (flags & Self::FLAG_HAS_FIN) != 0,
            channel: crate::binding::Channel::new(
                (self.channel_lo as u64) | ((self.channel_hi as u64) << 32),
            ),
        }
    }

    #[inline]
    pub(super) const fn from_option(value: Option<IngressEvidence>) -> Self {
        match value {
            Some(evidence) => Self::encode(evidence),
            None => Self::EMPTY,
        }
    }

    #[inline]
    pub(super) const fn into_option(self) -> Option<IngressEvidence> {
        if self.is_present() {
            Some(self.decode())
        } else {
            None
        }
    }

    #[inline]
    pub(super) fn take(slot: &mut Self) -> Option<IngressEvidence> {
        let packed = *slot;
        *slot = Self::EMPTY;
        packed.into_option()
    }
}

#[derive(Clone, Copy)]
struct DenseLaneSlots {
    ptr: *mut PackedIngressEvidence,
}

impl DenseLaneSlots {
    unsafe fn init_from_parts(
        dst: *mut Self,
        ptr: *mut PackedIngressEvidence,
        active_lane_count: usize,
    ) {
        unsafe {
            core::ptr::addr_of_mut!((*dst).ptr).write(ptr);
        }
        let mut idx = 0usize;
        while idx < active_lane_count.saturating_mul(BindingInbox::PER_LANE_CAPACITY) {
            unsafe {
                ptr.add(idx).write(PackedIngressEvidence::EMPTY);
            }
            idx += 1;
        }
    }

    #[inline]
    fn slot_ptr(
        &self,
        lanes: &DenseLaneIndex,
        lane_idx: usize,
        idx: usize,
    ) -> Option<*mut PackedIngressEvidence> {
        if idx >= BindingInbox::PER_LANE_CAPACITY {
            return None;
        }
        let dense = lanes.dense_ordinal(lane_idx)?;
        Some(unsafe { self.ptr.add(dense * BindingInbox::PER_LANE_CAPACITY + idx) })
    }

    #[inline]
    fn get(
        &self,
        lanes: &DenseLaneIndex,
        lane_idx: usize,
        idx: usize,
    ) -> Option<Option<IngressEvidence>> {
        self.slot_ptr(lanes, lane_idx, idx).map(|ptr| {
            let packed = unsafe { ptr.read() };
            if packed.is_present() {
                Some(packed.decode())
            } else {
                None
            }
        })
    }

    #[inline]
    fn set(
        &mut self,
        lanes: &DenseLaneIndex,
        lane_idx: usize,
        idx: usize,
        value: Option<IngressEvidence>,
    ) -> bool {
        let Some(ptr) = self.slot_ptr(lanes, lane_idx, idx) else {
            return false;
        };
        unsafe {
            ptr.write(
                value
                    .map(PackedIngressEvidence::encode)
                    .unwrap_or(PackedIngressEvidence::EMPTY),
            );
        }
        true
    }
}

pub(super) struct BindingInbox {
    lanes: DenseLaneIndex,
    slots: DenseLaneSlots,
    len: DenseLaneU8Array,
    nonempty_lanes: LaneSet,
    frame_label_masks: DenseLaneFrameLabelMaskArray,
}

impl BindingInbox {
    pub(super) const PER_LANE_CAPACITY: usize = 8;

    pub(super) unsafe fn init_empty(
        dst: *mut Self,
        slots: *mut PackedIngressEvidence,
        len: *mut u8,
        frame_label_masks: *mut FrameLabelMask,
        nonempty_lane_words: *mut LaneWord,
        lane_dense_by_lane: *mut DenseLaneOrdinal,
        active_lane_count: usize,
        nonempty_lane_word_count: usize,
    ) {
        unsafe {
            DenseLaneIndex::init_from_parts(
                core::ptr::addr_of_mut!((*dst).lanes),
                lane_dense_by_lane,
                active_lane_count,
            );
            DenseLaneSlots::init_from_parts(
                core::ptr::addr_of_mut!((*dst).slots),
                slots,
                active_lane_count,
            );
            DenseLaneU8Array::init_from_parts(
                core::ptr::addr_of_mut!((*dst).len),
                len,
                active_lane_count,
            );
            LaneSet::init_from_parts(
                core::ptr::addr_of_mut!((*dst).nonempty_lanes),
                nonempty_lane_words,
                nonempty_lane_word_count,
            );
            DenseLaneFrameLabelMaskArray::init_from_parts(
                core::ptr::addr_of_mut!((*dst).frame_label_masks),
                frame_label_masks,
                active_lane_count,
            );
        }
    }

    #[inline]
    pub(super) fn nonempty_lanes(&self) -> LaneSetView {
        self.nonempty_lanes.view()
    }

    #[inline]
    pub(super) fn has_buffered_for_lane_set(
        &self,
        lane_set: LaneSetView,
        lane_limit: usize,
    ) -> bool {
        let mut next = lane_set.first_set(lane_limit);
        while let Some(lane_idx) = next {
            if self.nonempty_lanes.contains(lane_idx) {
                return true;
            }
            next = lane_set.next_set_from(lane_idx.saturating_add(1), lane_limit);
        }
        false
    }

    #[inline]
    pub(super) fn lane_has_buffered_frame_label(
        &self,
        lane_idx: usize,
        frame_label_mask: FrameLabelMask,
    ) -> bool {
        if !self.lanes.contains_lane(lane_idx) {
            return false;
        }
        self.frame_label_masks
            .get_value(&self.lanes, lane_idx)
            .intersects(frame_label_mask)
    }

    #[inline]
    pub(super) fn update_nonempty_lanes(&mut self, lane_idx: usize) {
        if !self.lanes.contains_lane(lane_idx) {
            return;
        }
        if self.len.get_value(&self.lanes, lane_idx) == 0 {
            self.nonempty_lanes.remove(lane_idx);
        } else {
            self.nonempty_lanes.insert(lane_idx);
        }
    }

    #[inline]
    pub(super) fn recompute_frame_label_mask(&mut self, lane_idx: usize) {
        if !self.lanes.contains_lane(lane_idx) {
            return;
        }
        let buffered = self.len.get_value(&self.lanes, lane_idx) as usize;
        let mut mask = FrameLabelMask::EMPTY;
        let mut idx = 0usize;
        while idx < buffered {
            if let Some(evidence) = self.slots.get(&self.lanes, lane_idx, idx).flatten() {
                mask |= FrameLabelMask::from_frame_label(evidence.frame_label.raw());
            }
            idx += 1;
        }
        self.sync_frame_label_mask(lane_idx, mask);
    }

    #[inline]
    pub(super) fn sync_frame_label_mask(&mut self, lane_idx: usize, new_mask: FrameLabelMask) {
        if !self.lanes.contains_lane(lane_idx) {
            return;
        }
        let _ = self
            .frame_label_masks
            .set_value(&self.lanes, lane_idx, new_mask);
    }

    #[cfg(test)]
    #[inline]
    pub(super) fn buffered_lanes_for_frame_labels(
        &self,
        frame_label_mask: FrameLabelMask,
        dst: &mut [u8],
    ) -> usize {
        let mut len = 0usize;
        let lane_limit = self.lanes.active_lane_count.get();
        let mut lane_idx = 0usize;
        while lane_idx < lane_limit {
            if self
                .frame_label_masks
                .get_value(&self.lanes, lane_idx)
                .intersects(frame_label_mask)
            {
                assert!(
                    len < dst.len(),
                    "lane-index destination is too small for buffered label matches"
                );
                dst[len] = lane_idx as u8;
                len += 1;
            }
            lane_idx += 1;
        }
        len
    }

    #[inline]
    pub(super) fn remove_buffered_at(
        &mut self,
        lane_idx: usize,
        idx: usize,
    ) -> Option<IngressEvidence> {
        if !self.lanes.contains_lane(lane_idx) {
            return None;
        }
        let buffered = self.len.get_value(&self.lanes, lane_idx) as usize;
        if idx >= buffered {
            return None;
        }
        let evidence = self
            .slots
            .get(&self.lanes, lane_idx, idx)
            .flatten()
            .expect("binding inbox buffered slot must be populated");
        let mut shift = idx + 1;
        while shift < buffered {
            let next = self.slots.get(&self.lanes, lane_idx, shift).flatten();
            let _ = self.slots.set(&self.lanes, lane_idx, shift - 1, next);
            shift += 1;
        }
        let _ = self.slots.set(&self.lanes, lane_idx, buffered - 1, None);
        let _ = self
            .len
            .set_value(&self.lanes, lane_idx, (buffered - 1) as u8);
        self.recompute_frame_label_mask(lane_idx);
        self.update_nonempty_lanes(lane_idx);
        Some(evidence)
    }

    #[inline]
    pub(super) fn take_or_poll<B: BindingSlot>(
        &mut self,
        binding: &mut B,
        lane_idx: usize,
    ) -> Option<IngressEvidence> {
        if !self.lanes.contains_lane(lane_idx) {
            return None;
        }
        let buffered = self.len.get_value(&self.lanes, lane_idx) as usize;
        if buffered != 0 {
            return self.remove_buffered_at(lane_idx, 0);
        }
        binding.poll_incoming_for_lane(lane_idx as u8)
    }

    #[inline]
    pub(super) fn push_back(&mut self, lane_idx: usize, evidence: IngressEvidence) -> bool {
        if !self.lanes.contains_lane(lane_idx) {
            return false;
        }
        let buffered = self.len.get_value(&self.lanes, lane_idx) as usize;
        if buffered >= Self::PER_LANE_CAPACITY {
            return false;
        }
        let _ = self
            .slots
            .set(&self.lanes, lane_idx, buffered, Some(evidence));
        let _ = self
            .len
            .set_value(&self.lanes, lane_idx, (buffered + 1) as u8);
        self.nonempty_lanes.insert(lane_idx);
        self.sync_frame_label_mask(
            lane_idx,
            self.frame_label_masks.get_value(&self.lanes, lane_idx)
                | FrameLabelMask::from_frame_label(evidence.frame_label.raw()),
        );
        true
    }

    #[inline]
    pub(super) fn take_matching_or_poll<B: BindingSlot>(
        &mut self,
        binding: &mut B,
        lane_idx: usize,
        expected_frame_label: u8,
    ) -> Option<IngressEvidence> {
        if !self.lanes.contains_lane(lane_idx) {
            return None;
        }
        let expected_mask = FrameLabelMask::from_frame_label(expected_frame_label);
        if self
            .frame_label_masks
            .get_value(&self.lanes, lane_idx)
            .intersects(expected_mask)
        {
            let buffered = self.len.get_value(&self.lanes, lane_idx) as usize;
            let mut idx = 0usize;
            while idx < buffered {
                if let Some(evidence) = self.slots.get(&self.lanes, lane_idx, idx).flatten()
                    && evidence.frame_label.raw() == expected_frame_label
                {
                    return self.remove_buffered_at(lane_idx, idx);
                }
                idx += 1;
            }
            self.recompute_frame_label_mask(lane_idx);
        }

        let mut scans = 0usize;
        while scans < Self::PER_LANE_CAPACITY {
            scans += 1;
            if (self.len.get_value(&self.lanes, lane_idx) as usize) >= Self::PER_LANE_CAPACITY {
                break;
            }
            let Some(evidence) = binding.poll_incoming_for_lane(lane_idx as u8) else {
                break;
            };
            if evidence.frame_label.raw() == expected_frame_label {
                return Some(evidence);
            }
            if !self.push_back(lane_idx, evidence) {
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
        frame_label_mask: FrameLabelMask,
        drop_frame_label_mask: FrameLabelMask,
        mut drop_mismatch: F,
    ) -> Option<IngressEvidence> {
        if !self.lanes.contains_lane(lane_idx) || frame_label_mask.is_empty() {
            return None;
        }
        let buffered_scan_mask = frame_label_mask | drop_frame_label_mask;
        if self
            .frame_label_masks
            .get_value(&self.lanes, lane_idx)
            .intersects(buffered_scan_mask)
        {
            let mut idx = 0usize;
            while idx < (self.len.get_value(&self.lanes, lane_idx) as usize) {
                let Some(evidence) = self.slots.get(&self.lanes, lane_idx, idx).flatten() else {
                    idx += 1;
                    continue;
                };
                let evidence_mask = FrameLabelMask::from_frame_label(evidence.frame_label.raw());
                if frame_label_mask.intersects(evidence_mask) {
                    return self.remove_buffered_at(lane_idx, idx);
                }
                if drop_frame_label_mask.intersects(evidence_mask)
                    && drop_mismatch(evidence.frame_label.raw())
                {
                    let _ = self.remove_buffered_at(lane_idx, idx);
                    continue;
                }
                idx += 1;
            }
        }

        let mut scans = 0usize;
        while scans < Self::PER_LANE_CAPACITY {
            scans += 1;
            if (self.len.get_value(&self.lanes, lane_idx) as usize) >= Self::PER_LANE_CAPACITY {
                break;
            }
            let Some(evidence) = binding.poll_incoming_for_lane(lane_idx as u8) else {
                break;
            };
            let evidence_mask = FrameLabelMask::from_frame_label(evidence.frame_label.raw());
            if frame_label_mask.intersects(evidence_mask) {
                return Some(evidence);
            }
            if drop_frame_label_mask.intersects(evidence_mask)
                && drop_mismatch(evidence.frame_label.raw())
            {
                continue;
            }
            if !self.push_back(lane_idx, evidence) {
                break;
            }
        }
        None
    }

    #[inline]
    pub(super) fn put_back(&mut self, lane_idx: usize, evidence: IngressEvidence) {
        if !self.lanes.contains_lane(lane_idx) {
            return;
        }
        let buffered = self.len.get_value(&self.lanes, lane_idx) as usize;
        if buffered >= Self::PER_LANE_CAPACITY {
            return;
        }
        let mut idx = buffered;
        while idx > 0 {
            let prev = self.slots.get(&self.lanes, lane_idx, idx - 1).flatten();
            let _ = self.slots.set(&self.lanes, lane_idx, idx, prev);
            idx -= 1;
        }
        let _ = self.slots.set(&self.lanes, lane_idx, 0, Some(evidence));
        let _ = self
            .len
            .set_value(&self.lanes, lane_idx, (buffered + 1) as u8);
        self.nonempty_lanes.insert(lane_idx);
        self.sync_frame_label_mask(
            lane_idx,
            self.frame_label_masks.get_value(&self.lanes, lane_idx)
                | FrameLabelMask::from_frame_label(evidence.frame_label.raw()),
        );
    }

    #[cfg(test)]
    #[inline]
    pub(super) fn buffered_frame_label_mask_for_lane(&self, lane_idx: usize) -> FrameLabelMask {
        self.frame_label_masks.get_value(&self.lanes, lane_idx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        binding::Channel,
        global::role_program::{DenseLaneOrdinal, lane_word_count},
        transport::FrameLabel,
    };
    use core::mem::MaybeUninit;

    #[test]
    fn binding_inbox_keeps_lane_255_addressable_in_full_lane_domain() {
        const LANES: usize = 256;
        let mut lane_dense_by_lane: std::vec::Vec<DenseLaneOrdinal> = (0..LANES)
            .map(|lane| DenseLaneOrdinal::new(lane).expect("test lane dense ordinal"))
            .collect();
        let mut slots = std::vec::Vec::with_capacity(LANES * BindingInbox::PER_LANE_CAPACITY);
        slots.resize(
            LANES * BindingInbox::PER_LANE_CAPACITY,
            PackedIngressEvidence::EMPTY,
        );
        let mut len = std::vec::Vec::with_capacity(LANES);
        len.resize(LANES, 0u8);
        let mut frame_label_masks = std::vec::Vec::with_capacity(LANES);
        frame_label_masks.resize(LANES, FrameLabelMask::EMPTY);
        let mut nonempty_lane_words = std::vec::Vec::with_capacity(lane_word_count(LANES));
        nonempty_lane_words.resize(lane_word_count(LANES), 0usize);
        let mut inbox = MaybeUninit::<BindingInbox>::uninit();
        unsafe {
            BindingInbox::init_empty(
                inbox.as_mut_ptr(),
                slots.as_mut_ptr(),
                len.as_mut_ptr(),
                frame_label_masks.as_mut_ptr(),
                nonempty_lane_words.as_mut_ptr(),
                lane_dense_by_lane.as_mut_ptr(),
                LANES,
                lane_word_count(LANES),
            );
        }
        let mut inbox = unsafe { inbox.assume_init() };

        inbox.put_back(
            255,
            IngressEvidence {
                frame_label: FrameLabel::new(200),
                instance: 1,
                has_fin: false,
                channel: Channel::new(9),
            },
        );

        assert!(inbox.nonempty_lanes().contains(255));
        assert!(inbox.lane_has_buffered_frame_label(255, FrameLabelMask::from_frame_label(200)));
        assert_eq!(
            inbox.buffered_frame_label_mask_for_lane(255),
            FrameLabelMask::from_frame_label(200)
        );
        assert_eq!(
            inbox
                .remove_buffered_at(255, 0)
                .expect("lane 255 buffered evidence")
                .channel,
            Channel::new(9)
        );
        assert!(!inbox.nonempty_lanes().contains(255));
    }
}
