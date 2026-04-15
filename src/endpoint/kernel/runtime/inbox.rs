//! Binding inbox helpers for endpoint demux.

#[cfg(test)]
use crate::global::role_program::LOW_LANE_TEST_WIDTH;
use crate::{
    binding::{BindingSlot, IncomingClassification},
    global::role_program::{LaneSet, LaneSetView, LaneWord},
};

#[inline]
const fn label_bit(label: u8) -> u128 {
    if label < u128::BITS as u8 {
        1u128 << label
    } else {
        0
    }
}

#[cfg(test)]
const TEST_LANE_CAPACITY: usize = LOW_LANE_TEST_WIDTH;

#[cfg(test)]
const fn identity_lane_dense_by_lane() -> [u8; TEST_LANE_CAPACITY] {
    let mut lanes = [u8::MAX; TEST_LANE_CAPACITY];
    let mut idx = 0usize;
    while idx < TEST_LANE_CAPACITY {
        lanes[idx] = idx as u8;
        idx += 1;
    }
    lanes
}

#[derive(Clone, Copy)]
struct DenseLaneIndex {
    lane_dense_by_lane: *mut u8,
    active_lane_count: u8,
}

impl DenseLaneIndex {
    #[cfg(test)]
    const EMPTY: Self = Self {
        lane_dense_by_lane: core::ptr::null_mut(),
        active_lane_count: 0,
    };

    unsafe fn init_from_parts(
        dst: *mut Self,
        lane_dense_by_lane: *mut u8,
        active_lane_count: usize,
    ) {
        if active_lane_count > u8::MAX as usize {
            panic!("binding inbox lane count overflow");
        }
        unsafe {
            core::ptr::addr_of_mut!((*dst).lane_dense_by_lane).write(lane_dense_by_lane);
            core::ptr::addr_of_mut!((*dst).active_lane_count).write(active_lane_count as u8);
        }
    }

    #[inline]
    fn dense_ordinal(&self, lane_idx: usize) -> Option<usize> {
        if lane_idx >= self.active_lane_count as usize {
            return None;
        }
        let dense = unsafe { *self.lane_dense_by_lane.add(lane_idx) };
        if dense == u8::MAX || dense as usize >= self.active_lane_count as usize {
            None
        } else {
            Some(dense as usize)
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
    #[cfg(test)]
    const EMPTY: Self = Self {
        ptr: core::ptr::null_mut(),
    };

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
pub(super) struct DenseLaneU128Array {
    ptr: *mut u128,
}

impl DenseLaneU128Array {
    #[cfg(test)]
    const EMPTY: Self = Self {
        ptr: core::ptr::null_mut(),
    };

    unsafe fn init_from_parts(dst: *mut Self, ptr: *mut u128, active_lane_count: usize) {
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
    fn get_value(&self, lanes: &DenseLaneIndex, lane_idx: usize) -> u128 {
        lanes
            .dense_ordinal(lane_idx)
            .map(|dense| unsafe { *self.ptr.add(dense) })
            .unwrap_or(0)
    }

    #[inline]
    fn set_value(&mut self, lanes: &DenseLaneIndex, lane_idx: usize, value: u128) -> bool {
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
pub(super) struct PackedIncomingClassification {
    channel_lo: u32,
    channel_hi: u32,
    meta: u32,
}

impl PackedIncomingClassification {
    const META_LABEL_MASK: u32 = 0xFF;
    const META_INSTANCE_SHIFT: u32 = 8;
    const META_FLAGS_SHIFT: u32 = 24;
    const FLAG_PRESENT: u8 = 1;
    const FLAG_HAS_FIN: u8 = 1 << 1;

    const EMPTY: Self = Self {
        channel_lo: 0,
        channel_hi: 0,
        meta: 0,
    };

    #[inline]
    const fn is_present(self) -> bool {
        ((self.meta >> Self::META_FLAGS_SHIFT) as u8 & Self::FLAG_PRESENT) != 0
    }

    #[inline]
    const fn encode(classification: IncomingClassification) -> Self {
        let channel_raw = classification.channel.raw();
        let flags = Self::FLAG_PRESENT | ((classification.has_fin as u8) << 1);
        Self {
            channel_lo: channel_raw as u32,
            channel_hi: (channel_raw >> 32) as u32,
            meta: (classification.label as u32)
                | ((classification.instance as u32) << Self::META_INSTANCE_SHIFT)
                | ((flags as u32) << Self::META_FLAGS_SHIFT),
        }
    }

    #[inline]
    const fn decode(self) -> IncomingClassification {
        let flags = (self.meta >> Self::META_FLAGS_SHIFT) as u8;
        IncomingClassification {
            label: (self.meta & Self::META_LABEL_MASK) as u8,
            instance: (self.meta >> Self::META_INSTANCE_SHIFT) as u16,
            has_fin: (flags & Self::FLAG_HAS_FIN) != 0,
            channel: crate::binding::Channel::new(
                (self.channel_lo as u64) | ((self.channel_hi as u64) << 32),
            ),
        }
    }
}

#[derive(Clone, Copy)]
struct DenseLaneSlots {
    ptr: *mut PackedIncomingClassification,
}

impl DenseLaneSlots {
    #[cfg(test)]
    const EMPTY: Self = Self {
        ptr: core::ptr::null_mut(),
    };

    unsafe fn init_from_parts(
        dst: *mut Self,
        ptr: *mut PackedIncomingClassification,
        active_lane_count: usize,
    ) {
        unsafe {
            core::ptr::addr_of_mut!((*dst).ptr).write(ptr);
        }
        let mut idx = 0usize;
        while idx < active_lane_count.saturating_mul(BindingInbox::PER_LANE_CAPACITY) {
            unsafe {
                ptr.add(idx).write(PackedIncomingClassification::EMPTY);
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
    ) -> Option<*mut PackedIncomingClassification> {
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
    ) -> Option<Option<IncomingClassification>> {
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
        value: Option<IncomingClassification>,
    ) -> bool {
        let Some(ptr) = self.slot_ptr(lanes, lane_idx, idx) else {
            return false;
        };
        unsafe {
            ptr.write(
                value
                    .map(PackedIncomingClassification::encode)
                    .unwrap_or(PackedIncomingClassification::EMPTY),
            );
        }
        true
    }
}

#[cfg(test)]
struct BindingInboxTestArena {
    lane_dense_by_lane: [u8; TEST_LANE_CAPACITY],
    slots: [[PackedIncomingClassification; BindingInbox::PER_LANE_CAPACITY]; TEST_LANE_CAPACITY],
    len: [u8; TEST_LANE_CAPACITY],
    label_masks: [u128; TEST_LANE_CAPACITY],
    nonempty_lane_words:
        [LaneWord; crate::global::role_program::lane_word_count(TEST_LANE_CAPACITY)],
}

#[cfg(test)]
impl BindingInboxTestArena {
    const EMPTY: Self = Self {
        lane_dense_by_lane: [u8::MAX; TEST_LANE_CAPACITY],
        slots: [[PackedIncomingClassification::EMPTY; BindingInbox::PER_LANE_CAPACITY];
            TEST_LANE_CAPACITY],
        len: [0; TEST_LANE_CAPACITY],
        label_masks: [0; TEST_LANE_CAPACITY],
        nonempty_lane_words: [0; crate::global::role_program::lane_word_count(TEST_LANE_CAPACITY)],
    };
}

#[cfg(test)]
std::thread_local! {
    static TEST_BINDING_INBOX_STORAGE: core::cell::UnsafeCell<BindingInboxTestArena> =
        const { core::cell::UnsafeCell::new(BindingInboxTestArena::EMPTY) };
}

pub(super) struct BindingInbox {
    lanes: DenseLaneIndex,
    slots: DenseLaneSlots,
    len: DenseLaneU8Array,
    nonempty_lanes: LaneSet,
    label_masks: DenseLaneU128Array,
}

impl BindingInbox {
    pub(super) const PER_LANE_CAPACITY: usize = 8;

    pub(super) unsafe fn init_empty(
        dst: *mut Self,
        slots: *mut PackedIncomingClassification,
        len: *mut u8,
        label_masks: *mut u128,
        nonempty_lane_words: *mut LaneWord,
        lane_dense_by_lane: *mut u8,
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
            DenseLaneU128Array::init_from_parts(
                core::ptr::addr_of_mut!((*dst).label_masks),
                label_masks,
                active_lane_count,
            );
        }
    }

    #[cfg(test)]
    pub(super) fn test_empty() -> Self {
        TEST_BINDING_INBOX_STORAGE.with(|storage| {
            let storage = unsafe { &mut *storage.get() };
            *storage = BindingInboxTestArena::EMPTY;

            let lane_dense_by_lane = identity_lane_dense_by_lane();
            storage.lane_dense_by_lane = lane_dense_by_lane;
            let mut inbox = Self {
                lanes: DenseLaneIndex::EMPTY,
                slots: DenseLaneSlots::EMPTY,
                len: DenseLaneU8Array::EMPTY,
                nonempty_lanes: LaneSet::EMPTY,
                label_masks: DenseLaneU128Array::EMPTY,
            };
            unsafe {
                DenseLaneIndex::init_from_parts(
                    core::ptr::addr_of_mut!(inbox.lanes),
                    storage.lane_dense_by_lane.as_mut_ptr(),
                    TEST_LANE_CAPACITY,
                );
                DenseLaneSlots::init_from_parts(
                    core::ptr::addr_of_mut!(inbox.slots),
                    storage
                        .slots
                        .as_mut_ptr()
                        .cast::<PackedIncomingClassification>(),
                    TEST_LANE_CAPACITY,
                );
                DenseLaneU8Array::init_from_parts(
                    core::ptr::addr_of_mut!(inbox.len),
                    storage.len.as_mut_ptr(),
                    TEST_LANE_CAPACITY,
                );
                DenseLaneU128Array::init_from_parts(
                    core::ptr::addr_of_mut!(inbox.label_masks),
                    storage.label_masks.as_mut_ptr(),
                    TEST_LANE_CAPACITY,
                );
                LaneSet::init_from_parts(
                    core::ptr::addr_of_mut!(inbox.nonempty_lanes),
                    storage.nonempty_lane_words.as_mut_ptr(),
                    crate::global::role_program::lane_word_count(TEST_LANE_CAPACITY),
                );
            }
            inbox
        })
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
        let mut lane_idx = 0usize;
        while lane_idx < lane_limit {
            if lane_set.contains(lane_idx) && self.nonempty_lanes.contains(lane_idx) {
                return true;
            }
            lane_idx += 1;
        }
        false
    }

    #[inline]
    pub(super) fn lane_has_buffered_label(&self, lane_idx: usize, label_mask: u128) -> bool {
        if !self.lanes.contains_lane(lane_idx) {
            return false;
        }
        (self.label_masks.get_value(&self.lanes, lane_idx) & label_mask) != 0
    }

    #[cfg(test)]
    #[cfg(test)]
    #[inline]
    pub(super) fn nonempty_lane_mask_for_role_mask(&self, lane_mask: u32) -> u32 {
        let mut projected = 0u32;
        let mut remaining = lane_mask;
        while remaining != 0 {
            let lane_idx = remaining.trailing_zeros() as usize;
            remaining &= !(1u32 << lane_idx);
            if self.nonempty_lanes.contains(lane_idx) {
                projected |= 1u32 << lane_idx;
            }
        }
        projected
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

    #[cfg(test)]
    #[cfg(test)]
    #[inline]
    pub(super) fn has_buffered_for_lane_mask(&self, lane_mask: u32) -> bool {
        self.nonempty_lane_mask_for_role_mask(lane_mask) != 0
    }

    #[inline]
    pub(super) fn recompute_label_mask(&mut self, lane_idx: usize) {
        if !self.lanes.contains_lane(lane_idx) {
            return;
        }
        let buffered = self.len.get_value(&self.lanes, lane_idx) as usize;
        let mut mask = 0u128;
        let mut idx = 0usize;
        while idx < buffered {
            if let Some(classification) = self.slots.get(&self.lanes, lane_idx, idx).flatten() {
                mask |= label_bit(classification.label);
            }
            idx += 1;
        }
        self.sync_label_mask(lane_idx, mask);
    }

    #[inline]
    pub(super) fn sync_label_mask(&mut self, lane_idx: usize, new_mask: u128) {
        if !self.lanes.contains_lane(lane_idx) {
            return;
        }
        let _ = self.label_masks.set_value(&self.lanes, lane_idx, new_mask);
    }

    #[cfg(test)]
    #[cfg(test)]
    #[inline]
    pub(super) fn buffered_lane_mask_for_labels(&self, label_mask: u128) -> u32 {
        let mut lane_mask = 0u32;
        let lane_limit = self.lanes.active_lane_count as usize;
        let mut lane_idx = 0usize;
        while lane_idx < lane_limit && lane_idx < TEST_LANE_CAPACITY {
            if (self.label_masks.get_value(&self.lanes, lane_idx) & label_mask) != 0 {
                lane_mask |= 1u32 << lane_idx;
            }
            lane_idx += 1;
        }
        lane_mask
    }

    #[inline]
    pub(super) fn remove_buffered_at(
        &mut self,
        lane_idx: usize,
        idx: usize,
    ) -> Option<IncomingClassification> {
        if !self.lanes.contains_lane(lane_idx) {
            return None;
        }
        let buffered = self.len.get_value(&self.lanes, lane_idx) as usize;
        if idx >= buffered {
            return None;
        }
        let classification = self
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
        self.recompute_label_mask(lane_idx);
        self.update_nonempty_lanes(lane_idx);
        Some(classification)
    }

    #[inline]
    pub(super) fn take_or_poll<B: BindingSlot>(
        &mut self,
        binding: &mut B,
        lane_idx: usize,
    ) -> Option<IncomingClassification> {
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
    pub(super) fn push_back(
        &mut self,
        lane_idx: usize,
        classification: IncomingClassification,
    ) -> bool {
        if !self.lanes.contains_lane(lane_idx) {
            return false;
        }
        let buffered = self.len.get_value(&self.lanes, lane_idx) as usize;
        if buffered >= Self::PER_LANE_CAPACITY {
            return false;
        }
        let _ = self
            .slots
            .set(&self.lanes, lane_idx, buffered, Some(classification));
        let _ = self
            .len
            .set_value(&self.lanes, lane_idx, (buffered + 1) as u8);
        self.nonempty_lanes.insert(lane_idx);
        self.sync_label_mask(
            lane_idx,
            self.label_masks.get_value(&self.lanes, lane_idx) | label_bit(classification.label),
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
        if !self.lanes.contains_lane(lane_idx) {
            return None;
        }
        let expected_bit = label_bit(expected_label);
        if (self.label_masks.get_value(&self.lanes, lane_idx) & expected_bit) != 0 {
            let buffered = self.len.get_value(&self.lanes, lane_idx) as usize;
            let mut idx = 0usize;
            while idx < buffered {
                if let Some(classification) = self.slots.get(&self.lanes, lane_idx, idx).flatten()
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
            if (self.len.get_value(&self.lanes, lane_idx) as usize) >= Self::PER_LANE_CAPACITY {
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
        if !self.lanes.contains_lane(lane_idx) || label_mask == 0 {
            return None;
        }
        let buffered_scan_mask = label_mask | drop_label_mask;
        if (self.label_masks.get_value(&self.lanes, lane_idx) & buffered_scan_mask) != 0 {
            let mut idx = 0usize;
            while idx < (self.len.get_value(&self.lanes, lane_idx) as usize) {
                let Some(classification) = self.slots.get(&self.lanes, lane_idx, idx).flatten()
                else {
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
            if (self.len.get_value(&self.lanes, lane_idx) as usize) >= Self::PER_LANE_CAPACITY {
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
        let _ = self
            .slots
            .set(&self.lanes, lane_idx, 0, Some(classification));
        let _ = self
            .len
            .set_value(&self.lanes, lane_idx, (buffered + 1) as u8);
        self.nonempty_lanes.insert(lane_idx);
        self.sync_label_mask(
            lane_idx,
            self.label_masks.get_value(&self.lanes, lane_idx) | label_bit(classification.label),
        );
    }

    #[cfg(test)]
    #[inline]
    pub(super) fn buffered_label_mask_for_lane(&self, lane_idx: usize) -> u128 {
        self.label_masks.get_value(&self.lanes, lane_idx)
    }
}
