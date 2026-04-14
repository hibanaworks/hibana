//! Binding inbox helpers for endpoint demux.

#[cfg(test)]
use crate::global::role_program::MAX_LANES;
use crate::{
    binding::{BindingSlot, IncomingClassification},
    global::role_program::{LaneMask, lane_mask_bit},
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
#[cfg(test)]
const fn identity_lane_dense_by_lane() -> [u8; MAX_LANES] {
    let mut lanes = [u8::MAX; MAX_LANES];
    let mut idx = 0usize;
    while idx < MAX_LANES {
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
struct BindingInboxTestStorage {
    lane_dense_by_lane: [u8; MAX_LANES],
    slots: [[PackedIncomingClassification; BindingInbox::PER_LANE_CAPACITY]; MAX_LANES],
    len: [u8; MAX_LANES],
    label_masks: [u128; MAX_LANES],
}

#[cfg(test)]
impl BindingInboxTestStorage {
    const EMPTY: Self = Self {
        lane_dense_by_lane: [u8::MAX; MAX_LANES],
        slots: [[PackedIncomingClassification::EMPTY; BindingInbox::PER_LANE_CAPACITY]; MAX_LANES],
        len: [0; MAX_LANES],
        label_masks: [0; MAX_LANES],
    };
}

#[cfg(test)]
std::thread_local! {
    static TEST_BINDING_INBOX_STORAGE: core::cell::UnsafeCell<BindingInboxTestStorage> =
        const { core::cell::UnsafeCell::new(BindingInboxTestStorage::EMPTY) };
}

pub(super) struct BindingInbox {
    lanes: DenseLaneIndex,
    slots: DenseLaneSlots,
    len: DenseLaneU8Array,
    pub(super) nonempty_mask: LaneMask,
    label_masks: DenseLaneU128Array,
}

impl BindingInbox {
    pub(super) const PER_LANE_CAPACITY: usize = 8;

    pub(super) unsafe fn init_empty(
        dst: *mut Self,
        slots: *mut PackedIncomingClassification,
        len: *mut u8,
        label_masks: *mut u128,
        lane_dense_by_lane: *mut u8,
        active_lane_count: usize,
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
            core::ptr::addr_of_mut!((*dst).nonempty_mask).write(0);
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
            *storage = BindingInboxTestStorage::EMPTY;

            let lane_dense_by_lane = identity_lane_dense_by_lane();
            storage.lane_dense_by_lane = lane_dense_by_lane;
            let mut inbox = Self {
                lanes: DenseLaneIndex::EMPTY,
                slots: DenseLaneSlots::EMPTY,
                len: DenseLaneU8Array::EMPTY,
                nonempty_mask: 0,
                label_masks: DenseLaneU128Array::EMPTY,
            };
            unsafe {
                DenseLaneIndex::init_from_parts(
                    core::ptr::addr_of_mut!(inbox.lanes),
                    storage.lane_dense_by_lane.as_mut_ptr(),
                    MAX_LANES,
                );
                DenseLaneSlots::init_from_parts(
                    core::ptr::addr_of_mut!(inbox.slots),
                    storage
                        .slots
                        .as_mut_ptr()
                        .cast::<PackedIncomingClassification>(),
                    MAX_LANES,
                );
                DenseLaneU8Array::init_from_parts(
                    core::ptr::addr_of_mut!(inbox.len),
                    storage.len.as_mut_ptr(),
                    MAX_LANES,
                );
                DenseLaneU128Array::init_from_parts(
                    core::ptr::addr_of_mut!(inbox.label_masks),
                    storage.label_masks.as_mut_ptr(),
                    MAX_LANES,
                );
            }
            inbox
        })
    }

    #[inline]
    pub(super) fn update_nonempty_mask(&mut self, lane_idx: usize) {
        if !self.lanes.contains_lane(lane_idx) {
            return;
        }
        let bit = lane_mask_bit(lane_idx);
        if self.len.get_value(&self.lanes, lane_idx) == 0 {
            self.nonempty_mask &= !bit;
        } else {
            self.nonempty_mask |= bit;
        }
    }

    #[inline]
    pub(super) fn has_buffered_for_lane_mask(&self, lane_mask: LaneMask) -> bool {
        (self.nonempty_mask & lane_mask) != 0
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

    #[inline]
    pub(super) fn buffered_lane_mask_for_labels(&self, label_mask: u128) -> LaneMask {
        let mut lane_mask = 0;
        let lane_limit = self.lanes.active_lane_count as usize;
        let mut lane_idx = 0usize;
        while lane_idx < lane_limit {
            if (self.label_masks.get_value(&self.lanes, lane_idx) & label_mask) != 0 {
                lane_mask |= lane_mask_bit(lane_idx);
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
        self.update_nonempty_mask(lane_idx);
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
        self.nonempty_mask |= lane_mask_bit(lane_idx);
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
        self.nonempty_mask |= lane_mask_bit(lane_idx);
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
