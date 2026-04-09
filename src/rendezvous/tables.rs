//! Internal state tables for ra module.
//!
//! These tables manage generation counters, checkpoints, and routing policies.
//! All tables are !Send/!Sync (single-threaded, no_std compatible).

use core::{
    cell::UnsafeCell,
    marker::PhantomData,
    task::{Context, Poll, Waker},
};

use super::error::{GenError, GenerationRecord};
use crate::{
    control::{
        cap::mint::CapsMask,
        lease::map::ArrayMap,
        types::{Generation, Lane},
    },
    eff::EffIndex,
    global::const_dsl::{PolicyMode, ScopeId, ScopeKind},
    runtime::consts::LANES_MAX,
};

const ROLE_SLOTS: usize = LANES_MAX as usize;
#[cfg(test)]
const ROUTE_SLOTS: usize = crate::eff::meta::MAX_EFF_NODES;
const CONTROL_PLAN_SLOTS: usize = 128;

/// Generation counter table (per-lane).
///
/// Tracks the last seen generation number for each lane to ensure monotonic updates.
pub(crate) struct GenTable {
    lanes: UnsafeCell<[u16; LANES_MAX as usize]>,
    present_mask: UnsafeCell<u8>,
    _no_send_sync: PhantomData<*mut ()>,
}

impl Default for GenTable {
    fn default() -> Self {
        Self::new()
    }
}

impl GenTable {
    pub(crate) const fn new() -> Self {
        Self {
            lanes: UnsafeCell::new([0; LANES_MAX as usize]),
            present_mask: UnsafeCell::new(0),
            _no_send_sync: PhantomData,
        }
    }

    pub(crate) unsafe fn init_empty(dst: *mut Self) {
        unsafe {
            let lanes_ptr = core::ptr::addr_of_mut!((*dst).lanes).cast::<u16>();
            let mut idx = 0usize;
            while idx < LANES_MAX as usize {
                lanes_ptr.add(idx).write(0);
                idx += 1;
            }
            core::ptr::addr_of_mut!((*dst).present_mask).write(UnsafeCell::new(0));
            core::ptr::addr_of_mut!((*dst)._no_send_sync).write(PhantomData);
        }
    }

    /// Check and update generation for a lane.
    ///
    /// # Safety
    /// Rendezvous/Port are !Send/!Sync; writer is single-producer.
    #[inline]
    pub(crate) fn check_and_update(&self, lane: Lane, new: Generation) -> Result<(), GenError> {
        let idx = lane.raw() as usize;
        let lane_bit = 1u8 << lane.raw();
        unsafe {
            let lanes = &mut *self.lanes.get();
            let present_mask = &mut *self.present_mask.get();
            if (*present_mask & lane_bit) == 0 {
                if new.raw() == 0 {
                    lanes[idx] = new.raw();
                    *present_mask |= lane_bit;
                    return Ok(());
                }
                return Err(GenError::InvalidInitial { lane, new });
            }
            let prev = lanes[idx];
            if prev == u16::MAX {
                return Err(GenError::Overflow {
                    lane,
                    last: Generation::new(prev),
                });
            }
            if new.raw() > prev {
                lanes[idx] = new.raw();
                return Ok(());
            }
            Err(GenError::StaleOrDuplicate(GenerationRecord {
                lane,
                last: Generation::new(prev),
                new,
            }))
        }
    }

    /// Get last generation for a lane.
    #[inline]
    pub(crate) fn last(&self, lane: Lane) -> Option<Generation> {
        let idx = lane.raw() as usize;
        let lane_bit = 1u8 << lane.raw();
        unsafe {
            ((*self.present_mask.get() & lane_bit) != 0)
                .then_some(Generation::new((*self.lanes.get())[idx]))
        }
    }

    /// Reset lane (for release).
    #[inline]
    pub(crate) fn reset_lane(&self, lane: Lane) {
        let idx = lane.raw() as usize;
        let lane_bit = 1u8 << lane.raw();
        unsafe {
            (*self.lanes.get())[idx] = 0;
            *self.present_mask.get() &= !lane_bit;
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LoopDisposition {
    Continue,
    Break,
}

#[derive(Clone, Copy)]
struct LoopEntry {
    epoch: u16,
    decision: LoopDisposition,
    seen_mask: u16,
}

impl LoopEntry {
    const fn empty() -> Self {
        Self {
            epoch: 0,
            decision: LoopDisposition::Break,
            seen_mask: 0,
        }
    }
}

#[derive(Clone, Copy)]
struct LoopFrame {
    idx: u8,
    entry: LoopEntry,
    next: u16,
}

impl LoopFrame {
    fn assign(idx: u8, next: u16) -> Self {
        Self {
            idx,
            entry: LoopEntry::empty(),
            next,
        }
    }

    fn free(next: u16) -> Self {
        Self {
            idx: 0,
            entry: LoopEntry::empty(),
            next,
        }
    }
}

pub(crate) struct LoopTable {
    frames: UnsafeCell<*mut LoopFrame>,
    loop_slots: usize,
    lane_heads: UnsafeCell<*mut u16>,
    free_head: UnsafeCell<*mut u16>,
    _no_send_sync: PhantomData<*mut ()>,
}

impl Default for LoopTable {
    fn default() -> Self {
        Self::empty()
    }
}

impl LoopTable {
    const NO_FRAME: u16 = u16::MAX;
    const STORAGE_TAG_MASK: usize = Self::storage_align().saturating_sub(1);

    #[inline(always)]
    const fn align_up(value: usize, align: usize) -> usize {
        let mask = align.saturating_sub(1);
        (value + mask) & !mask
    }

    pub(crate) const fn empty() -> Self {
        Self {
            frames: UnsafeCell::new(core::ptr::null_mut()),
            loop_slots: 0,
            lane_heads: UnsafeCell::new(core::ptr::null_mut()),
            free_head: UnsafeCell::new(core::ptr::null_mut()),
            _no_send_sync: PhantomData,
        }
    }

    pub(crate) unsafe fn init_empty(dst: *mut Self) {
        unsafe {
            core::ptr::addr_of_mut!((*dst).frames).write(UnsafeCell::new(core::ptr::null_mut()));
            core::ptr::addr_of_mut!((*dst).loop_slots).write(0);
            core::ptr::addr_of_mut!((*dst).lane_heads)
                .write(UnsafeCell::new(core::ptr::null_mut()));
            core::ptr::addr_of_mut!((*dst).free_head).write(UnsafeCell::new(core::ptr::null_mut()));
            core::ptr::addr_of_mut!((*dst)._no_send_sync).write(PhantomData);
        }
    }

    #[inline]
    pub(crate) const fn loop_slots(&self) -> usize {
        self.loop_slots
    }

    #[inline]
    pub(crate) fn storage_ptr(&self) -> *mut u8 {
        self.frames_ptr().cast::<u8>()
    }

    #[inline]
    pub(crate) fn storage_reclaim_delta(&self) -> usize {
        self.raw_frames().addr() & Self::STORAGE_TAG_MASK
    }

    #[inline]
    pub(crate) const fn storage_bytes_current(&self) -> usize {
        Self::storage_bytes(self.loop_slots)
    }

    #[inline]
    pub(crate) const fn storage_align() -> usize {
        let frame_align = core::mem::align_of::<LoopFrame>();
        let u16_align = core::mem::align_of::<u16>();
        let mut max_align = frame_align;
        if u16_align > max_align {
            max_align = u16_align;
        }
        max_align
    }

    #[inline]
    pub(crate) const fn storage_bytes(loop_slots: usize) -> usize {
        if loop_slots == 0 {
            return 0;
        }
        let frames_bytes = loop_slots.saturating_mul(core::mem::size_of::<LoopFrame>());
        let lane_heads_offset = Self::align_up(frames_bytes, core::mem::align_of::<u16>());
        let lane_heads_bytes = (LANES_MAX as usize).saturating_mul(core::mem::size_of::<u16>());
        let free_head_offset = Self::align_up(
            lane_heads_offset.saturating_add(lane_heads_bytes),
            core::mem::align_of::<u16>(),
        );
        free_head_offset.saturating_add(core::mem::size_of::<u16>())
    }

    fn encode_frames_ptr(frames: *mut LoopFrame, reclaim_delta: usize) -> *mut LoopFrame {
        debug_assert_eq!(frames.addr() & Self::STORAGE_TAG_MASK, 0);
        debug_assert!(reclaim_delta <= Self::STORAGE_TAG_MASK);
        frames.map_addr(|addr| addr | reclaim_delta)
    }

    #[inline]
    fn raw_frames(&self) -> *mut LoopFrame {
        unsafe { *self.frames.get() }
    }

    unsafe fn bind_storage(
        &mut self,
        frames: *mut LoopFrame,
        loop_slots: usize,
        lane_heads: *mut u16,
        free_head: *mut u16,
        reclaim_delta: usize,
    ) {
        let mut frame_idx = 0usize;
        while frame_idx < loop_slots {
            let next = if frame_idx + 1 < loop_slots {
                (frame_idx + 1) as u16
            } else {
                Self::NO_FRAME
            };
            unsafe {
                frames.add(frame_idx).write(LoopFrame::free(next));
            }
            frame_idx += 1;
        }
        let mut lane_idx = 0usize;
        while lane_idx < LANES_MAX as usize {
            unsafe {
                lane_heads.add(lane_idx).write(Self::NO_FRAME);
            }
            lane_idx += 1;
        }
        unsafe {
            free_head.write(if loop_slots == 0 { Self::NO_FRAME } else { 0 });
        }
        *self.frames.get_mut() = Self::encode_frames_ptr(frames, reclaim_delta);
        self.loop_slots = loop_slots;
        *self.lane_heads.get_mut() = lane_heads;
        *self.free_head.get_mut() = free_head;
    }

    unsafe fn rebind_storage(
        &mut self,
        frames: *mut LoopFrame,
        loop_slots: usize,
        lane_heads: *mut u16,
        free_head: *mut u16,
        reclaim_delta: usize,
    ) {
        *self.frames.get_mut() = Self::encode_frames_ptr(frames, reclaim_delta);
        self.loop_slots = loop_slots;
        *self.lane_heads.get_mut() = lane_heads;
        *self.free_head.get_mut() = free_head;
    }

    unsafe fn migrate_to(
        &self,
        frames: *mut LoopFrame,
        loop_slots: usize,
        lane_heads: *mut u16,
        free_head: *mut u16,
    ) {
        let mut frame_idx = 0usize;
        while frame_idx < loop_slots {
            let next = if frame_idx + 1 < loop_slots {
                (frame_idx + 1) as u16
            } else {
                Self::NO_FRAME
            };
            unsafe {
                frames.add(frame_idx).write(LoopFrame::free(next));
            }
            frame_idx += 1;
        }
        let mut lane_idx = 0usize;
        while lane_idx < LANES_MAX as usize {
            unsafe {
                lane_heads.add(lane_idx).write(Self::NO_FRAME);
            }
            lane_idx += 1;
        }
        if self.loop_slots == 0 {
            unsafe {
                free_head.write(if loop_slots == 0 { Self::NO_FRAME } else { 0 });
            }
            return;
        }
        let src_frames = self.frames_ptr();
        let src_lane_heads = unsafe { *self.lane_heads.get() };
        let mut lane_idx = 0usize;
        while lane_idx < LANES_MAX as usize {
            let mut current = unsafe { *src_lane_heads.add(lane_idx) };
            while current != Self::NO_FRAME {
                let src_idx = current as usize;
                let src_frame = unsafe { *src_frames.add(src_idx) };
                let Some(dst_idx) = (unsafe { Self::raw_pop_free(frames, free_head) }) else {
                    panic!("loop table migration ran out of frame capacity");
                };
                let head = unsafe { *lane_heads.add(lane_idx) };
                unsafe {
                    frames
                        .add(dst_idx)
                        .write(LoopFrame::assign(src_frame.idx, head));
                    (*frames.add(dst_idx)).entry = src_frame.entry;
                    lane_heads.add(lane_idx).write(dst_idx as u16);
                }
                current = src_frame.next;
            }
            lane_idx += 1;
        }
        unsafe {
            if loop_slots == 0 || *free_head == Self::NO_FRAME {
                free_head.write(Self::NO_FRAME);
            }
        }
    }

    pub(crate) unsafe fn bind_from_storage(
        &mut self,
        storage: *mut u8,
        loop_slots: usize,
        reclaim_delta: usize,
    ) {
        let frames = storage.cast::<LoopFrame>();
        let lane_heads_offset = Self::align_up(
            storage as usize + loop_slots.saturating_mul(core::mem::size_of::<LoopFrame>()),
            core::mem::align_of::<u16>(),
        ) - storage as usize;
        let lane_heads = unsafe { storage.add(lane_heads_offset) }.cast::<u16>();
        let free_head_offset = Self::align_up(
            storage as usize
                + lane_heads_offset
                + (LANES_MAX as usize).saturating_mul(core::mem::size_of::<u16>()),
            core::mem::align_of::<u16>(),
        ) - storage as usize;
        let free_head = unsafe { storage.add(free_head_offset) }.cast::<u16>();
        unsafe {
            self.bind_storage(frames, loop_slots, lane_heads, free_head, reclaim_delta);
        }
    }

    pub(crate) unsafe fn migrate_from_storage(&self, storage: *mut u8, loop_slots: usize) {
        let frames = storage.cast::<LoopFrame>();
        let lane_heads_offset = Self::align_up(
            storage as usize + loop_slots.saturating_mul(core::mem::size_of::<LoopFrame>()),
            core::mem::align_of::<u16>(),
        ) - storage as usize;
        let lane_heads = unsafe { storage.add(lane_heads_offset) }.cast::<u16>();
        let free_head_offset = Self::align_up(
            storage as usize
                + lane_heads_offset
                + (LANES_MAX as usize).saturating_mul(core::mem::size_of::<u16>()),
            core::mem::align_of::<u16>(),
        ) - storage as usize;
        let free_head = unsafe { storage.add(free_head_offset) }.cast::<u16>();
        unsafe {
            self.migrate_to(frames, loop_slots, lane_heads, free_head);
        }
    }

    pub(crate) unsafe fn rebind_from_storage(
        &mut self,
        storage: *mut u8,
        loop_slots: usize,
        reclaim_delta: usize,
    ) {
        let frames = storage.cast::<LoopFrame>();
        let lane_heads_offset = Self::align_up(
            storage as usize + loop_slots.saturating_mul(core::mem::size_of::<LoopFrame>()),
            core::mem::align_of::<u16>(),
        ) - storage as usize;
        let lane_heads = unsafe { storage.add(lane_heads_offset) }.cast::<u16>();
        let free_head_offset = Self::align_up(
            storage as usize
                + lane_heads_offset
                + (LANES_MAX as usize).saturating_mul(core::mem::size_of::<u16>()),
            core::mem::align_of::<u16>(),
        ) - storage as usize;
        let free_head = unsafe { storage.add(free_head_offset) }.cast::<u16>();
        unsafe {
            self.rebind_storage(frames, loop_slots, lane_heads, free_head, reclaim_delta);
        }
    }

    #[inline]
    fn frames_ptr(&self) -> *mut LoopFrame {
        self.raw_frames()
            .map_addr(|addr| addr & !Self::STORAGE_TAG_MASK)
    }

    #[inline]
    fn lane_heads_ptr(&self) -> *mut u16 {
        unsafe { *self.lane_heads.get() }
    }

    #[inline]
    fn free_head_ptr(&self) -> *mut u16 {
        unsafe { *self.free_head.get() }
    }

    #[inline]
    fn lane_idx(lane: Lane) -> usize {
        lane.raw() as usize
    }

    #[inline]
    fn frame_ref(&self, frame_idx: usize) -> &LoopFrame {
        unsafe { &*self.frames_ptr().add(frame_idx) }
    }

    #[inline]
    fn frame_mut(&self, frame_idx: usize) -> &mut LoopFrame {
        unsafe { &mut *self.frames_ptr().add(frame_idx) }
    }

    unsafe fn raw_pop_free(frames: *mut LoopFrame, free_head: *mut u16) -> Option<usize> {
        let head = unsafe { *free_head };
        if head == Self::NO_FRAME {
            return None;
        }
        let idx = head as usize;
        let next = unsafe { (*frames.add(idx)).next };
        unsafe {
            *free_head = next;
            (*frames.add(idx)).next = Self::NO_FRAME;
        }
        Some(idx)
    }

    unsafe fn raw_push_free(frames: *mut LoopFrame, free_head: *mut u16, frame_idx: usize) {
        let head = unsafe { *free_head };
        unsafe {
            frames.add(frame_idx).write(LoopFrame::free(head));
            *free_head = frame_idx as u16;
        }
    }

    fn frame_for_idx(&self, lane_idx: usize, idx: u8) -> Option<usize> {
        let mut current = unsafe { *self.lane_heads_ptr().add(lane_idx) };
        while current != Self::NO_FRAME {
            let frame_idx = current as usize;
            let frame = self.frame_ref(frame_idx);
            if frame.idx == idx {
                return Some(frame_idx);
            }
            current = frame.next;
        }
        None
    }

    fn frame_or_alloc(&self, lane_idx: usize, idx: u8) -> usize {
        if let Some(frame_idx) = self.frame_for_idx(lane_idx, idx) {
            return frame_idx;
        }
        let Some(frame_idx) =
            (unsafe { Self::raw_pop_free(self.frames_ptr(), self.free_head_ptr()) })
        else {
            panic!("loop table slot exhausted");
        };
        let head = unsafe { *self.lane_heads_ptr().add(lane_idx) };
        let frame = self.frame_mut(frame_idx);
        *frame = LoopFrame::assign(idx, head);
        unsafe {
            self.lane_heads_ptr().add(lane_idx).write(frame_idx as u16);
        }
        frame_idx
    }

    #[inline]
    fn seen_bit(role_idx: usize) -> u16 {
        debug_assert!(role_idx < u16::BITS as usize);
        1u16 << (role_idx as u32)
    }

    pub(crate) fn record(
        &self,
        lane: Lane,
        role_from: u8,
        idx: u8,
        disposition: LoopDisposition,
    ) -> u16 {
        assert!(self.loop_slots != 0, "loop table storage must be bound");
        let lane_idx = Self::lane_idx(lane);
        let frame_idx = self.frame_or_alloc(lane_idx, idx);
        let entry = &mut self.frame_mut(frame_idx).entry;
        let mut epoch = entry.epoch.wrapping_add(1);
        if epoch == 0 {
            epoch = 1;
        }
        entry.epoch = epoch;
        entry.decision = disposition;
        entry.seen_mask = 0;
        if (role_from as usize) < ROLE_SLOTS {
            entry.seen_mask |= Self::seen_bit(role_from as usize);
        }

        epoch
    }

    pub(crate) fn acknowledge(&self, lane: Lane, role: u8, idx: u8) {
        if (role as usize) >= ROLE_SLOTS {
            return;
        }
        if self.loop_slots == 0 {
            return;
        }
        let lane_idx = Self::lane_idx(lane);
        let Some(frame_idx) = self.frame_for_idx(lane_idx, idx) else {
            return;
        };
        let entry = &mut self.frame_mut(frame_idx).entry;
        let epoch = entry.epoch;
        if epoch != 0 {
            entry.seen_mask |= Self::seen_bit(role as usize);
        }
    }

    pub(crate) fn reset_lane(&self, lane: Lane) {
        if self.loop_slots == 0 {
            return;
        }
        let lane_idx = Self::lane_idx(lane);
        let mut current = unsafe { *self.lane_heads_ptr().add(lane_idx) };
        unsafe {
            *self.lane_heads_ptr().add(lane_idx) = Self::NO_FRAME;
        }
        while current != Self::NO_FRAME {
            let frame_idx = current as usize;
            let next = self.frame_ref(frame_idx).next;
            unsafe {
                Self::raw_push_free(self.frames_ptr(), self.free_head_ptr(), frame_idx);
            }
            current = next;
        }
    }

    #[inline]
    pub(crate) fn has_decision(&self, lane: Lane, idx: u8) -> bool {
        if self.loop_slots == 0 {
            return false;
        }
        let lane_idx = Self::lane_idx(lane);
        let Some(frame_idx) = self.frame_for_idx(lane_idx, idx) else {
            return false;
        };
        self.frame_ref(frame_idx).entry.epoch != 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ScopeCoord {
    canonical: ScopeId,
}

impl ScopeCoord {
    fn from_scope(scope: ScopeId) -> Option<Self> {
        if scope.is_none() || scope.kind() != ScopeKind::Route {
            return None;
        }
        Some(Self {
            canonical: scope.canonical(),
        })
    }
}

#[derive(Clone, Copy)]
struct RouteEntry {
    epoch: u16,
    arm: u8,
    seen_mask: u16,
}

impl RouteEntry {
    const fn empty() -> Self {
        Self {
            epoch: 0,
            arm: 0,
            seen_mask: 0,
        }
    }
}

#[derive(Clone, Copy)]
struct RouteFrame {
    scope: ScopeId,
    entry: RouteEntry,
    next: u16,
}

impl RouteFrame {
    fn assign(coord: ScopeCoord, next: u16) -> Self {
        Self {
            scope: coord.canonical,
            entry: RouteEntry::empty(),
            next,
        }
    }

    fn free(next: u16) -> Self {
        Self {
            scope: ScopeId::none(),
            entry: RouteEntry::empty(),
            next,
        }
    }
}

pub(crate) struct RouteTable {
    frames: UnsafeCell<*mut RouteFrame>,
    route_slots: usize,
    lane_slots: u8,
    lane_heads: UnsafeCell<*mut u16>,
    free_head: UnsafeCell<*mut u16>,
    pending_hint_label_masks: UnsafeCell<*mut u128>,
    change_epoch: UnsafeCell<u16>,
    waiters: UnsafeCell<*mut Option<Waker>>,
    _no_send_sync: PhantomData<*mut ()>,
}

impl Default for RouteTable {
    fn default() -> Self {
        Self::empty()
    }
}

impl RouteTable {
    const NO_FRAME: u16 = u16::MAX;
    const STORAGE_TAG_MASK: usize = Self::storage_align().saturating_sub(1);

    #[inline(always)]
    const fn align_up(value: usize, align: usize) -> usize {
        let mask = align.saturating_sub(1);
        (value + mask) & !mask
    }

    pub(crate) const fn empty() -> Self {
        Self {
            frames: UnsafeCell::new(core::ptr::null_mut()),
            route_slots: 0,
            lane_slots: 0,
            lane_heads: UnsafeCell::new(core::ptr::null_mut()),
            free_head: UnsafeCell::new(core::ptr::null_mut()),
            pending_hint_label_masks: UnsafeCell::new(core::ptr::null_mut()),
            change_epoch: UnsafeCell::new(0),
            waiters: UnsafeCell::new(core::ptr::null_mut()),
            _no_send_sync: PhantomData,
        }
    }

    pub(crate) unsafe fn init_empty(dst: *mut Self) {
        unsafe {
            core::ptr::addr_of_mut!((*dst).frames).write(UnsafeCell::new(core::ptr::null_mut()));
            core::ptr::addr_of_mut!((*dst).route_slots).write(0);
            core::ptr::addr_of_mut!((*dst).lane_slots).write(0);
            core::ptr::addr_of_mut!((*dst).lane_heads)
                .write(UnsafeCell::new(core::ptr::null_mut()));
            core::ptr::addr_of_mut!((*dst).free_head).write(UnsafeCell::new(core::ptr::null_mut()));
            core::ptr::addr_of_mut!((*dst).pending_hint_label_masks)
                .write(UnsafeCell::new(core::ptr::null_mut()));
            core::ptr::addr_of_mut!((*dst).change_epoch)
                .cast::<u16>()
                .write(0);
            core::ptr::addr_of_mut!((*dst).waiters).write(UnsafeCell::new(core::ptr::null_mut()));
            core::ptr::addr_of_mut!((*dst)._no_send_sync).write(PhantomData);
        }
    }

    #[cfg(test)]
    fn allocate_test_storage(route_slots: usize, lane_slots: usize) -> *mut u8 {
        let layout = std::alloc::Layout::from_size_align(
            Self::storage_bytes(route_slots, lane_slots),
            Self::storage_align(),
        )
        .expect("route table test layout");
        let storage = unsafe { std::alloc::alloc_zeroed(layout) };
        if storage.is_null() {
            std::alloc::handle_alloc_error(layout);
        }
        storage
    }

    #[cfg(test)]
    fn build_test_table(route_slots: usize, lane_slots: usize) -> Self {
        let mut table = Self::empty();
        let storage = Self::allocate_test_storage(route_slots, lane_slots);
        unsafe {
            table.bind_from_storage_with_layout(storage, route_slots, lane_slots, 0);
        }
        table
    }

    #[cfg(test)]
    pub(crate) fn new() -> Self {
        Self::build_test_table(ROUTE_SLOTS, LANES_MAX as usize)
    }

    #[inline]
    pub(crate) const fn route_slots(&self) -> usize {
        self.route_slots
    }

    #[inline]
    pub(crate) const fn lane_slots(&self) -> usize {
        self.lane_slots as usize
    }

    #[inline]
    pub(crate) fn storage_ptr(&self) -> *mut u8 {
        self.frames_ptr().cast::<u8>()
    }

    #[inline]
    pub(crate) fn storage_reclaim_delta(&self) -> usize {
        self.raw_frames().addr() & Self::STORAGE_TAG_MASK
    }

    #[inline]
    pub(crate) const fn storage_bytes_current(&self) -> usize {
        Self::storage_bytes(self.route_slots, self.lane_slots())
    }

    #[inline]
    pub(crate) const fn storage_align() -> usize {
        let frame_align = core::mem::align_of::<RouteFrame>();
        let u16_align = core::mem::align_of::<u16>();
        let hint_align = core::mem::align_of::<u128>();
        let waiter_align = core::mem::align_of::<Option<Waker>>();
        let mut max_align = frame_align;
        if u16_align > max_align {
            max_align = u16_align;
        }
        if hint_align > max_align {
            max_align = hint_align;
        }
        if waiter_align > max_align {
            max_align = waiter_align;
        }
        max_align
    }

    #[inline]
    pub(crate) const fn storage_bytes(route_slots: usize, lane_slots: usize) -> usize {
        let frames_bytes = route_slots.saturating_mul(core::mem::size_of::<RouteFrame>());
        let lane_heads_offset = Self::align_up(frames_bytes, core::mem::align_of::<u16>());
        let lane_heads_bytes = lane_slots.saturating_mul(core::mem::size_of::<u16>());
        let free_head_offset = Self::align_up(
            lane_heads_offset.saturating_add(lane_heads_bytes),
            core::mem::align_of::<u16>(),
        );
        let free_head_bytes = core::mem::size_of::<u16>();
        let hint_offset = Self::align_up(
            free_head_offset.saturating_add(free_head_bytes),
            core::mem::align_of::<u128>(),
        );
        let hint_bytes = lane_slots.saturating_mul(core::mem::size_of::<u128>());
        let waiters_offset = Self::align_up(
            hint_offset.saturating_add(hint_bytes),
            core::mem::align_of::<Option<Waker>>(),
        );
        waiters_offset.saturating_add(
            lane_slots
                .saturating_mul(ROLE_SLOTS)
                .saturating_mul(core::mem::size_of::<Option<Waker>>()),
        )
    }

    fn encode_frames_ptr(frames: *mut RouteFrame, reclaim_delta: usize) -> *mut RouteFrame {
        debug_assert_eq!(frames.addr() & Self::STORAGE_TAG_MASK, 0);
        debug_assert!(reclaim_delta <= Self::STORAGE_TAG_MASK);
        frames.map_addr(|addr| addr | reclaim_delta)
    }

    #[inline]
    fn raw_frames(&self) -> *mut RouteFrame {
        unsafe { *self.frames.get() }
    }

    #[inline]
    fn raw_pending_hint_label_masks(&self) -> *mut u128 {
        unsafe { *self.pending_hint_label_masks.get() }
    }

    unsafe fn bind_storage(
        &mut self,
        frames: *mut RouteFrame,
        route_slots: usize,
        lane_slots: usize,
        lane_heads: *mut u16,
        free_head: *mut u16,
        pending_hint_label_masks: *mut u128,
        waiters: *mut Option<Waker>,
        reclaim_delta: usize,
    ) {
        let mut idx = 0usize;
        while idx < route_slots {
            let next = if idx + 1 < route_slots {
                (idx + 1) as u16
            } else {
                Self::NO_FRAME
            };
            unsafe {
                frames.add(idx).write(RouteFrame::free(next));
            }
            idx += 1;
        }
        let mut lane_idx = 0usize;
        while lane_idx < lane_slots {
            unsafe {
                lane_heads.add(lane_idx).write(Self::NO_FRAME);
            }
            lane_idx += 1;
        }
        unsafe {
            free_head.write(if route_slots == 0 { Self::NO_FRAME } else { 0 });
        }
        let mut hint_idx = 0usize;
        while hint_idx < lane_slots {
            unsafe {
                pending_hint_label_masks.add(hint_idx).write(0);
            }
            hint_idx += 1;
        }
        let mut waiter_idx = 0usize;
        while waiter_idx < lane_slots.saturating_mul(ROLE_SLOTS) {
            unsafe {
                waiters.add(waiter_idx).write(None);
            }
            waiter_idx += 1;
        }
        *self.frames.get_mut() = Self::encode_frames_ptr(frames, reclaim_delta);
        self.route_slots = route_slots;
        debug_assert!(lane_slots <= u8::MAX as usize);
        self.lane_slots = lane_slots as u8;
        *self.lane_heads.get_mut() = lane_heads;
        *self.free_head.get_mut() = free_head;
        *self.pending_hint_label_masks.get_mut() = pending_hint_label_masks;
        *self.change_epoch.get_mut() = 0;
        *self.waiters.get_mut() = waiters;
    }

    unsafe fn rebind_storage(
        &mut self,
        frames: *mut RouteFrame,
        route_slots: usize,
        lane_slots: usize,
        lane_heads: *mut u16,
        free_head: *mut u16,
        pending_hint_label_masks: *mut u128,
        waiters: *mut Option<Waker>,
        reclaim_delta: usize,
    ) {
        *self.frames.get_mut() = Self::encode_frames_ptr(frames, reclaim_delta);
        self.route_slots = route_slots;
        debug_assert!(lane_slots <= u8::MAX as usize);
        self.lane_slots = lane_slots as u8;
        *self.lane_heads.get_mut() = lane_heads;
        *self.free_head.get_mut() = free_head;
        *self.pending_hint_label_masks.get_mut() = pending_hint_label_masks;
        *self.waiters.get_mut() = waiters;
    }

    unsafe fn raw_pop_free(frames: *mut RouteFrame, free_head: *mut u16) -> Option<usize> {
        let head = unsafe { *free_head };
        if head == Self::NO_FRAME {
            return None;
        }
        let idx = head as usize;
        let next = unsafe { (*frames.add(idx)).next };
        unsafe {
            *free_head = next;
            (*frames.add(idx)).next = Self::NO_FRAME;
        }
        Some(idx)
    }

    unsafe fn raw_push_free(frames: *mut RouteFrame, free_head: *mut u16, idx: usize) {
        let next = unsafe { *free_head };
        unsafe {
            frames.add(idx).write(RouteFrame::free(next));
            *free_head = idx as u16;
        }
    }

    unsafe fn migrate_to(
        &self,
        frames: *mut RouteFrame,
        route_slots: usize,
        lane_slots: usize,
        lane_heads: *mut u16,
        free_head: *mut u16,
        pending_hint_label_masks: *mut u128,
        waiters: *mut Option<Waker>,
    ) {
        debug_assert!(lane_slots >= self.lane_slots());
        let mut idx = 0usize;
        while idx < route_slots {
            let next = if idx + 1 < route_slots {
                (idx + 1) as u16
            } else {
                Self::NO_FRAME
            };
            unsafe {
                frames.add(idx).write(RouteFrame::free(next));
            }
            idx += 1;
        }
        let mut lane_idx = 0usize;
        while lane_idx < lane_slots {
            unsafe {
                lane_heads.add(lane_idx).write(Self::NO_FRAME);
            }
            lane_idx += 1;
        }
        unsafe {
            free_head.write(if route_slots == 0 { Self::NO_FRAME } else { 0 });
        }
        let mut hint_idx = 0usize;
        while hint_idx < self.lane_slots() {
            unsafe {
                pending_hint_label_masks
                    .add(hint_idx)
                    .write(*self.pending_hint_label_masks_ptr().add(hint_idx));
            }
            hint_idx += 1;
        }
        while hint_idx < lane_slots {
            unsafe {
                pending_hint_label_masks.add(hint_idx).write(0);
            }
            hint_idx += 1;
        }
        let mut waiter_idx = 0usize;
        let waiter_count = lane_slots.saturating_mul(ROLE_SLOTS);
        let src_waiter_count = self.lane_slots().saturating_mul(ROLE_SLOTS);
        while waiter_idx < src_waiter_count {
            unsafe {
                let src_waiter = &mut *self.waiters_ptr().add(waiter_idx);
                waiters.add(waiter_idx).write(src_waiter.take());
            }
            waiter_idx += 1;
        }
        while waiter_idx < waiter_count {
            unsafe {
                waiters.add(waiter_idx).write(None);
            }
            waiter_idx += 1;
        }
        if self.route_slots == 0 {
            return;
        }
        let mut lane_idx = 0usize;
        while lane_idx < self.lane_slots() {
            let mut current = unsafe { *self.lane_heads_ptr().add(lane_idx) };
            let mut prev_new = Self::NO_FRAME;
            while current != Self::NO_FRAME {
                let src_idx = current as usize;
                let next = unsafe { (*self.frames_ptr().add(src_idx)).next };
                let dst_idx = unsafe { Self::raw_pop_free(frames, free_head) }
                    .expect("route ledger migration exhausted frame capacity");
                let mut moved = unsafe { *self.frames_ptr().add(src_idx) };
                moved.next = Self::NO_FRAME;
                unsafe {
                    frames.add(dst_idx).write(moved);
                }
                if prev_new == Self::NO_FRAME {
                    unsafe {
                        *lane_heads.add(lane_idx) = dst_idx as u16;
                    }
                } else {
                    unsafe {
                        (*frames.add(prev_new as usize)).next = dst_idx as u16;
                    }
                }
                prev_new = dst_idx as u16;
                current = next;
            }
            lane_idx += 1;
        }
    }

    pub(crate) unsafe fn bind_from_storage_with_layout(
        &mut self,
        storage: *mut u8,
        route_slots: usize,
        lane_slots: usize,
        reclaim_delta: usize,
    ) {
        let frames = storage.cast::<RouteFrame>();
        let frames_bytes = route_slots.saturating_mul(core::mem::size_of::<RouteFrame>());
        let lane_heads_offset = Self::align_up(
            storage as usize + frames_bytes,
            core::mem::align_of::<u16>(),
        ) - storage as usize;
        let lane_heads = unsafe { storage.add(lane_heads_offset) }.cast::<u16>();
        let lane_heads_bytes = lane_slots.saturating_mul(core::mem::size_of::<u16>());
        let free_head_offset = Self::align_up(
            storage as usize + lane_heads_offset + lane_heads_bytes,
            core::mem::align_of::<u16>(),
        ) - storage as usize;
        let free_head = unsafe { storage.add(free_head_offset) }.cast::<u16>();
        let hint_offset = Self::align_up(
            storage as usize + free_head_offset + core::mem::size_of::<u16>(),
            core::mem::align_of::<u128>(),
        ) - storage as usize;
        let pending_hint_label_masks = unsafe { storage.add(hint_offset) }.cast::<u128>();
        let hint_bytes = lane_slots.saturating_mul(core::mem::size_of::<u128>());
        let waiters_offset = Self::align_up(
            storage as usize + hint_offset + hint_bytes,
            core::mem::align_of::<Option<Waker>>(),
        ) - storage as usize;
        let waiters = unsafe { storage.add(waiters_offset) }.cast::<Option<Waker>>();
        unsafe {
            self.bind_storage(
                frames,
                route_slots,
                lane_slots,
                lane_heads,
                free_head,
                pending_hint_label_masks,
                waiters,
                reclaim_delta,
            );
        }
    }

    pub(crate) unsafe fn migrate_from_storage(
        &self,
        storage: *mut u8,
        route_slots: usize,
        lane_slots: usize,
    ) {
        let frames = storage.cast::<RouteFrame>();
        let frames_bytes = route_slots.saturating_mul(core::mem::size_of::<RouteFrame>());
        let lane_heads_offset = Self::align_up(
            storage as usize + frames_bytes,
            core::mem::align_of::<u16>(),
        ) - storage as usize;
        let lane_heads = unsafe { storage.add(lane_heads_offset) }.cast::<u16>();
        let lane_heads_bytes = lane_slots.saturating_mul(core::mem::size_of::<u16>());
        let free_head_offset = Self::align_up(
            storage as usize + lane_heads_offset + lane_heads_bytes,
            core::mem::align_of::<u16>(),
        ) - storage as usize;
        let free_head = unsafe { storage.add(free_head_offset) }.cast::<u16>();
        let hint_offset = Self::align_up(
            storage as usize + free_head_offset + core::mem::size_of::<u16>(),
            core::mem::align_of::<u128>(),
        ) - storage as usize;
        let pending_hint_label_masks = unsafe { storage.add(hint_offset) }.cast::<u128>();
        let hint_bytes = lane_slots.saturating_mul(core::mem::size_of::<u128>());
        let waiters_offset = Self::align_up(
            storage as usize + hint_offset + hint_bytes,
            core::mem::align_of::<Option<Waker>>(),
        ) - storage as usize;
        let waiters = unsafe { storage.add(waiters_offset) }.cast::<Option<Waker>>();
        unsafe {
            self.migrate_to(
                frames,
                route_slots,
                lane_slots,
                lane_heads,
                free_head,
                pending_hint_label_masks,
                waiters,
            );
        }
    }

    pub(crate) unsafe fn rebind_from_storage(
        &mut self,
        storage: *mut u8,
        route_slots: usize,
        lane_slots: usize,
        reclaim_delta: usize,
    ) {
        let frames = storage.cast::<RouteFrame>();
        let frames_bytes = route_slots.saturating_mul(core::mem::size_of::<RouteFrame>());
        let lane_heads_offset = Self::align_up(
            storage as usize + frames_bytes,
            core::mem::align_of::<u16>(),
        ) - storage as usize;
        let lane_heads = unsafe { storage.add(lane_heads_offset) }.cast::<u16>();
        let lane_heads_bytes = lane_slots.saturating_mul(core::mem::size_of::<u16>());
        let free_head_offset = Self::align_up(
            storage as usize + lane_heads_offset + lane_heads_bytes,
            core::mem::align_of::<u16>(),
        ) - storage as usize;
        let free_head = unsafe { storage.add(free_head_offset) }.cast::<u16>();
        let hint_offset = Self::align_up(
            storage as usize + free_head_offset + core::mem::size_of::<u16>(),
            core::mem::align_of::<u128>(),
        ) - storage as usize;
        let pending_hint_label_masks = unsafe { storage.add(hint_offset) }.cast::<u128>();
        let hint_bytes = lane_slots.saturating_mul(core::mem::size_of::<u128>());
        let waiters_offset = Self::align_up(
            storage as usize + hint_offset + hint_bytes,
            core::mem::align_of::<Option<Waker>>(),
        ) - storage as usize;
        let waiters = unsafe { storage.add(waiters_offset) }.cast::<Option<Waker>>();
        unsafe {
            self.rebind_storage(
                frames,
                route_slots,
                lane_slots,
                lane_heads,
                free_head,
                pending_hint_label_masks,
                waiters,
                reclaim_delta,
            );
        }
    }

    #[inline]
    fn frames_ptr(&self) -> *mut RouteFrame {
        self.raw_frames()
            .map_addr(|addr| addr & !Self::STORAGE_TAG_MASK)
    }

    #[inline]
    fn lane_heads_ptr(&self) -> *mut u16 {
        unsafe { *self.lane_heads.get() }
    }

    #[inline]
    fn free_head_ptr(&self) -> *mut u16 {
        unsafe { *self.free_head.get() }
    }

    #[inline]
    fn pending_hint_label_masks_ptr(&self) -> *mut u128 {
        self.raw_pending_hint_label_masks()
    }

    #[inline]
    fn waiters_ptr(&self) -> *mut Option<Waker> {
        unsafe { *self.waiters.get() }
    }

    #[inline]
    fn lane_idx(lane: Lane) -> usize {
        lane.raw() as usize
    }

    #[inline]
    fn lane_slot(&self, lane: Lane) -> usize {
        let lane_idx = Self::lane_idx(lane);
        debug_assert!(
            lane_idx < self.lane_slots(),
            "route lane must fit bound lane span"
        );
        lane_idx
    }

    #[inline]
    fn role_slot_count(role_count: u8) -> usize {
        core::cmp::min(role_count as usize, ROLE_SLOTS)
    }

    #[inline]
    fn complete_seen_mask(role_slots: usize) -> u16 {
        if role_slots == 0 {
            0
        } else if role_slots >= u16::BITS as usize {
            u16::MAX
        } else {
            (1u16 << role_slots) - 1
        }
    }

    #[inline]
    fn frame_ref(&self, idx: usize) -> &RouteFrame {
        unsafe { &*self.frames_ptr().add(idx) }
    }

    #[inline]
    fn frame_mut(&self, idx: usize) -> &mut RouteFrame {
        unsafe { &mut *self.frames_ptr().add(idx) }
    }

    #[inline]
    fn slot_for_scope(&self, lane_idx: usize, coord: ScopeCoord) -> Option<usize> {
        let mut current = unsafe { *self.lane_heads_ptr().add(lane_idx) };
        while current != Self::NO_FRAME {
            let idx = current as usize;
            if self.frame_ref(idx).scope == coord.canonical {
                return Some(idx);
            }
            current = self.frame_ref(idx).next;
        }
        None
    }

    fn slot_or_alloc(&self, lane_idx: usize, coord: ScopeCoord) -> Option<usize> {
        if let Some(idx) = Self::slot_for_scope(self, lane_idx, coord) {
            return Some(idx);
        }
        if self.route_slots == 0 {
            return None;
        }
        let idx = unsafe { Self::raw_pop_free(self.frames_ptr(), self.free_head_ptr()) }?;
        let head = unsafe { *self.lane_heads_ptr().add(lane_idx) };
        *self.frame_mut(idx) = RouteFrame::assign(coord, head);
        unsafe {
            *self.lane_heads_ptr().add(lane_idx) = idx as u16;
        }
        Some(idx)
    }

    fn try_reclaim_route_slot(&self, lane_idx: usize, slot_idx: usize, role_count: u8) {
        let role_mask = Self::complete_seen_mask(Self::role_slot_count(role_count));
        if role_mask == 0 {
            return;
        }
        let frame = self.frame_ref(slot_idx);
        if frame.entry.epoch == 0 || (frame.entry.seen_mask & role_mask) != role_mask {
            return;
        }
        let mut prev = Self::NO_FRAME;
        let mut current = unsafe { *self.lane_heads_ptr().add(lane_idx) };
        while current != Self::NO_FRAME {
            let current_idx = current as usize;
            let next = self.frame_ref(current_idx).next;
            if current_idx == slot_idx {
                if prev == Self::NO_FRAME {
                    unsafe {
                        *self.lane_heads_ptr().add(lane_idx) = next;
                    }
                } else {
                    self.frame_mut(prev as usize).next = next;
                }
                unsafe {
                    Self::raw_push_free(self.frames_ptr(), self.free_head_ptr(), slot_idx);
                }
                return;
            }
            prev = current;
            current = next;
        }
    }

    #[inline]
    fn pending_lane_mask_bit(lane_idx: usize) -> u16 {
        1u16 << lane_idx
    }

    #[inline]
    fn seen_bit(role_idx: usize) -> u16 {
        debug_assert!(role_idx < u16::BITS as usize);
        1u16 << (role_idx as u32)
    }

    #[inline]
    fn bump_change_epoch(&self) {
        let epoch = unsafe { &mut *self.change_epoch.get() };
        let next = epoch.wrapping_add(1);
        *epoch = if next == 0 { 1 } else { next };
    }

    #[inline]
    pub(crate) fn change_epoch(&self) -> u16 {
        unsafe { *self.change_epoch.get() }
    }

    pub(crate) fn record_with_role_count(
        &self,
        lane: Lane,
        role_count: u8,
        role_from: u8,
        scope: ScopeId,
        arm: u8,
    ) -> u16 {
        let coord = ScopeCoord::from_scope(scope).expect("route record requires structured scope");
        let lane_idx = self.lane_slot(lane);
        let slot_idx = Self::slot_or_alloc(self, lane_idx, coord).unwrap_or_else(|| {
            let free_head = unsafe { *self.free_head_ptr() };
            panic!(
                "route ledger exhausted: lane_idx={lane_idx} frame_capacity={} free_head={} coord_local={}",
                self.route_slots,
                free_head,
                coord.canonical.local_ordinal()
            );
        });
        let entry = &mut self.frame_mut(slot_idx).entry;
        let mut epoch = entry.epoch.wrapping_add(1);
        if epoch == 0 {
            epoch = 1;
        }
        entry.epoch = epoch;
        entry.arm = arm;
        entry.seen_mask = 0;
        let role_slots = Self::role_slot_count(role_count);
        if (role_from as usize) < role_slots {
            entry.seen_mask |= Self::seen_bit(role_from as usize);
        }
        self.bump_change_epoch();

        let waiters = self.waiters_ptr();
        let mut role_idx = 0usize;
        while role_idx < ROLE_SLOTS {
            let waiter = unsafe { &mut *waiters.add(lane_idx * ROLE_SLOTS + role_idx) };
            if let Some(waker) = waiter.take() {
                waker.wake();
            }
            role_idx += 1;
        }
        epoch
    }

    pub(crate) fn poll_with_role_count(
        &self,
        lane: Lane,
        role_count: u8,
        role: u8,
        scope: ScopeId,
        cx: &mut Context<'_>,
    ) -> Poll<u8> {
        let role_slots = Self::role_slot_count(role_count);
        if (role as usize) >= role_slots {
            return Poll::Ready(0);
        }
        let coord = ScopeCoord::from_scope(scope).expect("route poll requires structured scope");
        let lane_idx = self.lane_slot(lane);
        let slot_idx = match Self::slot_or_alloc(self, lane_idx, coord) {
            Some(idx) => idx,
            None => return Poll::Pending,
        };
        let entry = &mut self.frame_mut(slot_idx).entry;
        let role_bit = Self::seen_bit(role as usize);
        if entry.epoch != 0 && (entry.seen_mask & role_bit) == 0 {
            entry.seen_mask |= role_bit;
            let arm = entry.arm;
            self.try_reclaim_route_slot(lane_idx, slot_idx, role_count);
            self.bump_change_epoch();
            return Poll::Ready(arm);
        }

        let waiters = self.waiters_ptr();
        let slot = unsafe { &mut *waiters.add(lane_idx * ROLE_SLOTS + role as usize) };
        *slot = Some(cx.waker().clone());
        Poll::Pending
    }

    pub(crate) fn acknowledge_with_role_count(
        &self,
        lane: Lane,
        role_count: u8,
        role: u8,
        scope: ScopeId,
    ) -> Option<u8> {
        let role_slots = Self::role_slot_count(role_count);
        if (role as usize) >= role_slots {
            return None;
        }
        let coord = ScopeCoord::from_scope(scope)?;
        let lane_idx = self.lane_slot(lane);
        let slot_idx = Self::slot_for_scope(self, lane_idx, coord)?;
        let entry = &mut self.frame_mut(slot_idx).entry;
        if entry.epoch == 0 {
            return None;
        }
        let role_bit = Self::seen_bit(role as usize);
        if (entry.seen_mask & role_bit) != 0 {
            return None;
        }
        entry.seen_mask |= role_bit;
        let arm = entry.arm;
        self.try_reclaim_route_slot(lane_idx, slot_idx, role_count);
        self.bump_change_epoch();
        Some(arm)
    }

    pub(crate) fn peek_with_role_count(
        &self,
        lane: Lane,
        role_count: u8,
        role: u8,
        scope: ScopeId,
    ) -> Option<u8> {
        let role_slots = Self::role_slot_count(role_count);
        if (role as usize) >= role_slots {
            return None;
        }
        let coord = ScopeCoord::from_scope(scope)?;
        let lane_idx = self.lane_slot(lane);
        let slot_idx = Self::slot_for_scope(self, lane_idx, coord)?;
        let entry = self.frame_ref(slot_idx).entry;
        let role_bit = Self::seen_bit(role as usize);
        (entry.epoch != 0 && (entry.seen_mask & role_bit) == 0).then_some(entry.arm)
    }

    pub(crate) fn pending_lane_mask_with_role_count(
        &self,
        role_count: u8,
        role: u8,
        scope: ScopeId,
    ) -> u16 {
        let role_slots = Self::role_slot_count(role_count);
        if (role as usize) >= role_slots {
            return 0;
        }
        let coord = match ScopeCoord::from_scope(scope) {
            Some(coord) => coord,
            None => return 0,
        };
        let role_bit = Self::seen_bit(role as usize);
        let mut lanes = 0u16;
        let mut lane_idx = 0usize;
        while lane_idx < self.lane_slots() {
            if let Some(slot_idx) = Self::slot_for_scope(self, lane_idx, coord) {
                let entry = self.frame_ref(slot_idx).entry;
                if entry.epoch != 0 && (entry.seen_mask & role_bit) == 0 {
                    lanes |= Self::pending_lane_mask_bit(lane_idx);
                }
            }
            lane_idx += 1;
        }
        lanes
    }

    #[inline]
    pub(crate) fn pending_hint_labels_for_lane(&self, lane: Lane) -> u128 {
        if self.route_slots == 0 {
            return 0;
        }
        let lane_idx = self.lane_slot(lane);
        unsafe { *self.pending_hint_label_masks_ptr().add(lane_idx) }
    }

    pub(crate) fn update_pending_hint_lane_masks(&self, lane: Lane, before: u128, after: u128) {
        if before == after || self.route_slots == 0 {
            return;
        }
        let lane_idx = self.lane_slot(lane);
        unsafe {
            *self.pending_hint_label_masks_ptr().add(lane_idx) = after;
        }
        self.bump_change_epoch();
    }

    pub(crate) fn pending_hint_lane_mask_for_labels(&self, label_mask: u128) -> u16 {
        if self.route_slots == 0 {
            return 0;
        }
        let mut lanes = 0u16;
        let pending_hint_label_masks = self.pending_hint_label_masks_ptr();
        let mut lane_idx = 0usize;
        while lane_idx < self.lane_slots() {
            if (unsafe { *pending_hint_label_masks.add(lane_idx) } & label_mask) != 0 {
                lanes |= Self::pending_lane_mask_bit(lane_idx);
            }
            lane_idx += 1;
        }
        lanes
    }

    pub(crate) fn reset_lane(&self, lane: Lane) {
        if self.route_slots == 0 {
            return;
        }
        let lane_idx = self.lane_slot(lane);
        let mut current = unsafe { *self.lane_heads_ptr().add(lane_idx) };
        unsafe {
            *self.lane_heads_ptr().add(lane_idx) = Self::NO_FRAME;
        }
        while current != Self::NO_FRAME {
            let idx = current as usize;
            let next = self.frame_ref(idx).next;
            unsafe {
                Self::raw_push_free(self.frames_ptr(), self.free_head_ptr(), idx);
            }
            current = next;
        }
        let pending_hint_label_masks = self.pending_hint_label_masks_ptr();
        unsafe {
            *pending_hint_label_masks.add(lane_idx) = 0;
        }
        let waiters = self.waiters_ptr();
        let mut role_idx = 0usize;
        while role_idx < ROLE_SLOTS {
            unsafe {
                waiters.add(lane_idx * ROLE_SLOTS + role_idx).write(None);
            }
            role_idx += 1;
        }
        self.bump_change_epoch();
    }
}

#[cfg(test)]
mod tests {
    use super::{GenTable, LoopDisposition, LoopFrame, LoopTable, RouteTable};
    use crate::{
        control::types::{Generation, Lane},
        global::const_dsl::ScopeId,
    };
    const ROLE_COUNT: u8 = 2;

    fn tiny_loop_table(loop_slots: usize) -> LoopTable {
        let mut table = LoopTable::empty();
        let frames = std::vec![LoopFrame::free(LoopTable::NO_FRAME); loop_slots].into_boxed_slice();
        let lane_heads =
            std::vec![LoopTable::NO_FRAME; super::LANES_MAX as usize].into_boxed_slice();
        let free_head = std::boxed::Box::new(LoopTable::NO_FRAME);
        unsafe {
            table.bind_storage(
                std::boxed::Box::leak(frames).as_mut_ptr(),
                loop_slots,
                std::boxed::Box::leak(lane_heads).as_mut_ptr(),
                std::boxed::Box::leak(free_head),
                0,
            );
        }
        table
    }

    fn tiny_route_table(route_slots: usize) -> RouteTable {
        let lane_slots = super::LANES_MAX as usize;
        RouteTable::build_test_table(route_slots, lane_slots)
    }

    #[test]
    fn gen_table_tracks_presence_with_explicit_mask() {
        let table = GenTable::new();
        let lane = Lane::new(0);

        assert_eq!(table.last(lane), None);
        assert!(matches!(
            table.check_and_update(lane, Generation::new(1)),
            Err(super::GenError::InvalidInitial { lane: err_lane, new })
                if err_lane == lane && new == Generation::new(1)
        ));

        assert_eq!(table.check_and_update(lane, Generation::ZERO), Ok(()));
        assert_eq!(table.last(lane), Some(Generation::ZERO));

        table.reset_lane(lane);
        assert_eq!(table.last(lane), None);
        assert_eq!(table.check_and_update(lane, Generation::ZERO), Ok(()));
        assert_eq!(table.last(lane), Some(Generation::ZERO));
    }

    #[test]
    fn gen_table_preserves_stale_and_overflow_semantics() {
        let table = GenTable::new();
        let lane = Lane::new(2);

        assert_eq!(table.check_and_update(lane, Generation::ZERO), Ok(()));
        assert_eq!(table.check_and_update(lane, Generation::new(7)), Ok(()));
        assert!(matches!(
            table.check_and_update(lane, Generation::new(7)),
            Err(super::GenError::StaleOrDuplicate(record))
                if record.lane == lane
                    && record.last == Generation::new(7)
                    && record.new == Generation::new(7)
        ));

        assert_eq!(
            table.check_and_update(lane, Generation::new(u16::MAX)),
            Ok(())
        );
        assert!(matches!(
            table.check_and_update(lane, Generation::new(u16::MAX)),
            Err(super::GenError::Overflow { lane: err_lane, last })
                if err_lane == lane && last == Generation::new(u16::MAX)
        ));
    }

    #[test]
    fn route_table_peek_is_non_consuming() {
        let table = RouteTable::new();
        let lane = Lane::new(0);
        let scope = ScopeId::route(9);

        assert_eq!(table.peek_with_role_count(lane, ROLE_COUNT, 1, scope), None);
        table.record_with_role_count(lane, ROLE_COUNT, 0, scope, 1);
        assert_eq!(
            table.peek_with_role_count(lane, ROLE_COUNT, 1, scope),
            Some(1)
        );
        assert_eq!(
            table.peek_with_role_count(lane, ROLE_COUNT, 1, scope),
            Some(1)
        );
        assert_eq!(
            table.acknowledge_with_role_count(lane, ROLE_COUNT, 1, scope),
            Some(1)
        );
        assert_eq!(table.peek_with_role_count(lane, ROLE_COUNT, 1, scope), None);
    }

    #[test]
    fn route_table_pending_lane_mask_tracks_unacked_decisions() {
        let table = RouteTable::new();
        let lane0 = Lane::new(0);
        let lane2 = Lane::new(2);
        let scope = ScopeId::route(9);

        assert_eq!(
            table.pending_lane_mask_with_role_count(ROLE_COUNT, 1, scope),
            0
        );

        table.record_with_role_count(lane0, ROLE_COUNT, 0, scope, 1);
        table.record_with_role_count(lane2, ROLE_COUNT, 0, scope, 1);
        assert_eq!(
            table.pending_lane_mask_with_role_count(ROLE_COUNT, 0, scope),
            0
        );
        assert_eq!(
            table.pending_lane_mask_with_role_count(ROLE_COUNT, 1, scope),
            (1u16 << 0) | (1u16 << 2)
        );

        assert_eq!(
            table.acknowledge_with_role_count(lane0, ROLE_COUNT, 1, scope),
            Some(1)
        );
        assert_eq!(
            table.pending_lane_mask_with_role_count(ROLE_COUNT, 1, scope),
            1u16 << 2
        );

        table.record_with_role_count(lane0, ROLE_COUNT, 0, scope, 0);
        assert_eq!(
            table.pending_lane_mask_with_role_count(ROLE_COUNT, 1, scope),
            (1u16 << 0) | (1u16 << 2)
        );

        table.reset_lane(lane2);
        assert_eq!(
            table.pending_lane_mask_with_role_count(ROLE_COUNT, 1, scope),
            1u16 << 0
        );
    }

    #[test]
    fn route_table_reuses_lane_slot_after_all_roles_acknowledge() {
        let table = tiny_route_table(1);
        let lane = Lane::new(0);
        let scope_a = ScopeId::route(9);
        let scope_b = ScopeId::route(10);

        table.record_with_role_count(lane, ROLE_COUNT, 0, scope_a, 1);
        assert_eq!(
            table.pending_lane_mask_with_role_count(ROLE_COUNT, 1, scope_a),
            1u16 << 0
        );
        assert_eq!(
            table.acknowledge_with_role_count(lane, ROLE_COUNT, 1, scope_a),
            Some(1)
        );
        assert_eq!(
            table.pending_lane_mask_with_role_count(ROLE_COUNT, 1, scope_a),
            0
        );

        table.record_with_role_count(lane, ROLE_COUNT, 0, scope_b, 2);
        assert_eq!(
            table.peek_with_role_count(lane, ROLE_COUNT, 1, scope_b),
            Some(2)
        );
        assert_eq!(
            table.pending_lane_mask_with_role_count(ROLE_COUNT, 1, scope_b),
            1u16 << 0
        );
    }

    #[test]
    fn route_table_change_epoch_tracks_route_and_hint_updates() {
        let table = RouteTable::new();
        let lane = Lane::new(0);
        let scope = ScopeId::route(9);

        let initial = table.change_epoch();
        table.record_with_role_count(lane, ROLE_COUNT, 0, scope, 1);
        let after_record = table.change_epoch();
        assert_ne!(after_record, initial);

        assert_eq!(
            table.acknowledge_with_role_count(lane, ROLE_COUNT, 1, scope),
            Some(1)
        );
        let after_ack = table.change_epoch();
        assert_ne!(after_ack, after_record);

        table.update_pending_hint_lane_masks(lane, 0, 1u128 << 25);
        let after_hint = table.change_epoch();
        assert_ne!(after_hint, after_ack);

        table.reset_lane(lane);
        assert_ne!(table.change_epoch(), after_hint);
    }

    #[test]
    fn loop_table_reuses_lane_slot_after_lane_reset() {
        let table = tiny_loop_table(1);
        let lane = Lane::new(0);

        assert!(!table.has_decision(lane, 0));
        assert_eq!(table.record(lane, 0, 0, LoopDisposition::Continue), 1);
        assert!(table.has_decision(lane, 0));

        table.reset_lane(lane);
        assert!(!table.has_decision(lane, 0));

        assert_eq!(table.record(lane, 0, 1, LoopDisposition::Break), 1);
        assert!(table.has_decision(lane, 1));
    }

    #[test]
    fn loop_table_empty_layout_has_no_resident_bytes() {
        assert_eq!(LoopTable::storage_bytes(0), 0);
    }

    #[test]
    fn route_table_hint_lane_mask_tracks_buffered_labels() {
        let table = RouteTable::new();
        let lane0 = Lane::new(0);
        let lane2 = Lane::new(2);

        assert_eq!(table.pending_hint_lane_mask_for_labels(1u128 << 25), 0);

        table.update_pending_hint_lane_masks(lane0, 0, (1u128 << 25) | (1u128 << 41));
        assert_eq!(
            table.pending_hint_lane_mask_for_labels(1u128 << 25),
            1u16 << 0
        );
        assert_eq!(
            table.pending_hint_lane_mask_for_labels((1u128 << 25) | (1u128 << 41)),
            1u16 << 0
        );

        table.update_pending_hint_lane_masks(lane2, 0, 1u128 << 41);
        assert_eq!(
            table.pending_hint_lane_mask_for_labels(1u128 << 41),
            (1u16 << 0) | (1u16 << 2)
        );

        table.update_pending_hint_lane_masks(lane0, (1u128 << 25) | (1u128 << 41), 1u128 << 41);
        assert_eq!(table.pending_hint_lane_mask_for_labels(1u128 << 25), 0);
        assert_eq!(
            table.pending_hint_lane_mask_for_labels(1u128 << 41),
            (1u16 << 0) | (1u16 << 2)
        );

        table.reset_lane(lane2);
        assert_eq!(
            table.pending_hint_lane_mask_for_labels(1u128 << 41),
            1u16 << 0
        );
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PolicyKey {
    eff_index: EffIndex,
    tag: u8,
}

struct PolicySlot {
    policies: ArrayMap<PolicyKey, PolicyMode, CONTROL_PLAN_SLOTS>,
}

impl PolicySlot {
    unsafe fn init_empty(dst: *mut Self) {
        unsafe {
            ArrayMap::init_empty(core::ptr::addr_of_mut!((*dst).policies));
        }
    }

    fn register(
        &mut self,
        eff_index: EffIndex,
        tag: u8,
        policy: PolicyMode,
    ) -> Result<(), PolicyMode> {
        let key = PolicyKey { eff_index, tag };
        self.policies.insert(key, policy)
    }

    fn get(&self, eff_index: EffIndex, tag: u8) -> Option<PolicyMode> {
        let key = PolicyKey { eff_index, tag };
        self.policies.get(&key).copied()
    }

    fn reset(&mut self) {
        self.policies.clear();
    }
}

pub(crate) struct PolicyTable {
    lanes: UnsafeCell<*mut PolicySlot>,
    _no_send_sync: PhantomData<*mut ()>,
}

impl Default for PolicyTable {
    fn default() -> Self {
        Self::empty()
    }
}

impl PolicyTable {
    pub(crate) const fn empty() -> Self {
        Self {
            lanes: UnsafeCell::new(core::ptr::null_mut()),
            _no_send_sync: PhantomData,
        }
    }

    pub(crate) unsafe fn init_empty(dst: *mut Self) {
        unsafe {
            core::ptr::addr_of_mut!((*dst).lanes).write(UnsafeCell::new(core::ptr::null_mut()));
            core::ptr::addr_of_mut!((*dst)._no_send_sync).write(PhantomData);
        }
    }

    #[inline]
    pub(crate) const fn storage_align() -> usize {
        core::mem::align_of::<PolicySlot>()
    }

    #[inline]
    pub(crate) const fn storage_bytes() -> usize {
        (LANES_MAX as usize).saturating_mul(core::mem::size_of::<PolicySlot>())
    }

    unsafe fn bind_storage(&mut self, lanes: *mut PolicySlot) {
        let mut idx = 0usize;
        while idx < LANES_MAX as usize {
            unsafe {
                PolicySlot::init_empty(lanes.add(idx));
            }
            idx += 1;
        }
        *self.lanes.get_mut() = lanes;
    }

    pub(crate) unsafe fn bind_from_storage(&mut self, storage: *mut u8) {
        unsafe {
            self.bind_storage(storage.cast::<PolicySlot>());
        }
    }

    #[inline]
    fn lanes_ptr(&self) -> *mut PolicySlot {
        unsafe { *self.lanes.get() }
    }

    #[inline]
    pub(crate) fn is_bound(&self) -> bool {
        !self.lanes_ptr().is_null()
    }

    pub(crate) fn register(
        &self,
        lane: Lane,
        eff_index: EffIndex,
        tag: u8,
        policy: PolicyMode,
    ) -> Result<(), PolicyMode> {
        if policy.is_static() {
            return Ok(());
        }
        if self.lanes_ptr().is_null() {
            return Err(policy);
        }
        unsafe {
            (&mut *self.lanes_ptr().add(lane.raw() as usize)).register(eff_index, tag, policy)
        }
    }

    pub(crate) fn get(&self, lane: Lane, eff_index: EffIndex, tag: u8) -> Option<PolicyMode> {
        if self.lanes_ptr().is_null() {
            return None;
        }
        unsafe { (&*self.lanes_ptr().add(lane.raw() as usize)).get(eff_index, tag) }
    }

    pub(crate) fn reset_lane(&self, lane: Lane) {
        if self.lanes_ptr().is_null() {
            return;
        }
        unsafe {
            (&mut *self.lanes_ptr().add(lane.raw() as usize)).reset();
        }
    }
}

/// Per-lane capability table for the effect VM.
pub(crate) struct VmCapsTable {
    lanes: UnsafeCell<[CapsMask; LANES_MAX as usize]>,
    _no_send_sync: PhantomData<*mut ()>,
}

impl Default for VmCapsTable {
    fn default() -> Self {
        Self::new()
    }
}

impl VmCapsTable {
    pub(crate) const fn new() -> Self {
        Self {
            lanes: UnsafeCell::new([CapsMask::allow_all(); LANES_MAX as usize]),
            _no_send_sync: PhantomData,
        }
    }

    pub(crate) unsafe fn init_empty(dst: *mut Self) {
        unsafe {
            let lanes_ptr = core::ptr::addr_of_mut!((*dst).lanes).cast::<CapsMask>();
            let mut idx = 0usize;
            while idx < LANES_MAX as usize {
                lanes_ptr.add(idx).write(CapsMask::allow_all());
                idx += 1;
            }
            core::ptr::addr_of_mut!((*dst)._no_send_sync).write(PhantomData);
        }
    }

    /// Set capability bits for a lane.
    #[inline]
    pub(crate) fn set(&self, lane: Lane, caps: CapsMask) {
        unsafe {
            (*self.lanes.get())[lane.raw() as usize] = caps;
        }
    }

    /// Get capability bits for a lane.
    #[inline]
    pub(crate) fn get(&self, lane: Lane) -> CapsMask {
        unsafe { (*self.lanes.get())[lane.raw() as usize] }
    }

    /// Reset lane (clear permissions).
    #[inline]
    pub(crate) fn reset_lane(&self, lane: Lane) {
        unsafe {
            (*self.lanes.get())[lane.raw() as usize] = CapsMask::allow_all();
        }
    }
}

/// Checkpoint table (per-lane).
///
/// Tracks last checkpoint epoch and consumption status for rollback operations.
pub(crate) struct CheckpointTable {
    last_checkpoint: UnsafeCell<[u16; LANES_MAX as usize]>,
    present_mask: UnsafeCell<u8>,
    consumed_mask: UnsafeCell<u8>,
    _no_send_sync: PhantomData<*mut ()>,
}

impl Default for CheckpointTable {
    fn default() -> Self {
        Self::new()
    }
}

impl CheckpointTable {
    pub(crate) const fn new() -> Self {
        Self {
            last_checkpoint: UnsafeCell::new([0; LANES_MAX as usize]),
            present_mask: UnsafeCell::new(0),
            consumed_mask: UnsafeCell::new(0),
            _no_send_sync: PhantomData,
        }
    }

    pub(crate) unsafe fn init_empty(dst: *mut Self) {
        unsafe {
            let last_checkpoint_ptr = core::ptr::addr_of_mut!((*dst).last_checkpoint).cast::<u16>();
            let mut idx = 0usize;
            while idx < LANES_MAX as usize {
                last_checkpoint_ptr.add(idx).write(0);
                idx += 1;
            }
            core::ptr::addr_of_mut!((*dst).present_mask).write(UnsafeCell::new(0));
            core::ptr::addr_of_mut!((*dst).consumed_mask).write(UnsafeCell::new(0));
            core::ptr::addr_of_mut!((*dst)._no_send_sync).write(PhantomData);
        }
    }

    /// Record a checkpoint.
    #[inline]
    pub(crate) fn record(&self, lane: Lane, checkpoint: Generation) {
        let lane_idx = lane.raw() as usize;
        let lane_bit = 1u8 << lane.raw();
        unsafe {
            (*self.last_checkpoint.get())[lane_idx] = checkpoint.raw();
            *self.present_mask.get() |= lane_bit;
            *self.consumed_mask.get() &= !lane_bit;
        }
    }

    /// Get last checkpoint for a lane.
    #[inline]
    pub(crate) fn last(&self, lane: Lane) -> Option<Generation> {
        let lane_idx = lane.raw() as usize;
        let lane_bit = 1u8 << lane.raw();
        unsafe {
            ((*self.present_mask.get() & lane_bit) != 0)
                .then_some(Generation::new((*self.last_checkpoint.get())[lane_idx]))
        }
    }

    /// Mark checkpoint as consumed.
    #[inline]
    pub(crate) fn mark_consumed(&self, lane: Lane) {
        let lane_bit = 1u8 << lane.raw();
        unsafe {
            *self.consumed_mask.get() |= lane_bit;
        }
    }

    /// Check if checkpoint is consumed.
    #[inline]
    pub(crate) fn is_consumed(&self, lane: Lane) -> bool {
        let lane_bit = 1u8 << lane.raw();
        unsafe { (*self.consumed_mask.get() & lane_bit) != 0 }
    }

    /// Reset lane.
    #[inline]
    pub(crate) fn reset_lane(&self, lane: Lane) {
        let lane_idx = lane.raw() as usize;
        let lane_bit = 1u8 << lane.raw();
        unsafe {
            (*self.last_checkpoint.get())[lane_idx] = 0;
            *self.present_mask.get() &= !lane_bit;
            *self.consumed_mask.get() &= !lane_bit;
        }
    }
}
