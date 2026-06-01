use super::{Lane, MAX_TRACKED_ROLES, PhantomData, UnsafeCell};
// # Unsafe Owner Contract
//
// This fragment owns loop-decision table frames and lane head columns. Unsafe
// operations bind resident storage once, thread frames through explicit free
// lists, and access frame/lane slots only after budget and lane-domain checks
// performed by the table owner.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LoopDisposition {
    Continue,
    Break,
}

#[derive(Clone, Copy)]
struct LoopEntry {
    pub(crate) epoch: u16,
    decision: LoopDisposition,
    seen_mask: u16,
}

impl LoopEntry {
    pub(crate) const fn empty() -> Self {
        Self {
            epoch: 0,
            decision: LoopDisposition::Break,
            seen_mask: 0,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct LoopFrame {
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

    pub(crate) fn free(next: u16) -> Self {
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
    lane_base: u32,
    lane_slots: u16,
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
    pub(crate) const NO_FRAME: u16 = u16::MAX;
    pub(crate) const STORAGE_TAG_MASK: usize = Self::storage_align().saturating_sub(1);

    #[inline(always)]
    pub(crate) const fn align_up(value: usize, align: usize) -> usize {
        let mask = align.saturating_sub(1);
        (value + mask) & !mask
    }

    pub(crate) const fn empty() -> Self {
        Self {
            frames: UnsafeCell::new(core::ptr::null_mut()),
            loop_slots: 0,
            lane_base: 0,
            lane_slots: 0,
            lane_heads: UnsafeCell::new(core::ptr::null_mut()),
            free_head: UnsafeCell::new(core::ptr::null_mut()),
            _no_send_sync: PhantomData,
        }
    }

    pub(crate) unsafe fn init_empty(dst: *mut Self) {
        /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
        unsafe {
            core::ptr::addr_of_mut!((*dst).frames).write(UnsafeCell::new(core::ptr::null_mut()));
            core::ptr::addr_of_mut!((*dst).loop_slots).write(0);
            core::ptr::addr_of_mut!((*dst).lane_base).write(0);
            core::ptr::addr_of_mut!((*dst).lane_slots).write(0);
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
        Self::storage_bytes(self.loop_slots, self.lane_slots as usize)
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
    pub(crate) const fn storage_bytes(loop_slots: usize, lane_slots: usize) -> usize {
        if loop_slots == 0 {
            return 0;
        }
        let frames_bytes = loop_slots.saturating_mul(core::mem::size_of::<LoopFrame>());
        let lane_heads_offset = Self::align_up(frames_bytes, core::mem::align_of::<u16>());
        let lane_heads_bytes = lane_slots.saturating_mul(core::mem::size_of::<u16>());
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
        /* SAFETY: the rendezvous table owns initialized slots behind explicit presence state before raw access. */
        unsafe { *self.frames.get() }
    }

    pub(crate) unsafe fn bind_storage(
        &mut self,
        frames: *mut LoopFrame,
        loop_slots: usize,
        lane_base: u32,
        lane_slots: usize,
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
            /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
            unsafe {
                frames.add(frame_idx).write(LoopFrame::free(next));
            }
            frame_idx += 1;
        }
        let mut lane_idx = 0usize;
        while lane_idx < lane_slots {
            /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
            unsafe {
                lane_heads.add(lane_idx).write(Self::NO_FRAME);
            }
            lane_idx += 1;
        }
        /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
        unsafe {
            free_head.write(if loop_slots == 0 { Self::NO_FRAME } else { 0 });
        }
        *self.frames.get_mut() = Self::encode_frames_ptr(frames, reclaim_delta);
        self.loop_slots = loop_slots;
        self.lane_base = lane_base;
        self.lane_slots = lane_slots as u16;
        *self.lane_heads.get_mut() = lane_heads;
        *self.free_head.get_mut() = free_head;
    }

    unsafe fn rebind_storage(
        &mut self,
        frames: *mut LoopFrame,
        loop_slots: usize,
        lane_base: u32,
        lane_slots: usize,
        lane_heads: *mut u16,
        free_head: *mut u16,
        reclaim_delta: usize,
    ) {
        *self.frames.get_mut() = Self::encode_frames_ptr(frames, reclaim_delta);
        self.loop_slots = loop_slots;
        self.lane_base = lane_base;
        self.lane_slots = lane_slots as u16;
        *self.lane_heads.get_mut() = lane_heads;
        *self.free_head.get_mut() = free_head;
    }

    unsafe fn migrate_to(
        &self,
        frames: *mut LoopFrame,
        loop_slots: usize,
        lane_base: u32,
        lane_slots: usize,
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
            /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
            unsafe {
                frames.add(frame_idx).write(LoopFrame::free(next));
            }
            frame_idx += 1;
        }
        let mut lane_idx = 0usize;
        while lane_idx < lane_slots {
            /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
            unsafe {
                lane_heads.add(lane_idx).write(Self::NO_FRAME);
            }
            lane_idx += 1;
        }
        if self.loop_slots == 0 {
            /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
            unsafe {
                free_head.write(if loop_slots == 0 { Self::NO_FRAME } else { 0 });
            }
            return;
        }
        let src_frames = self.frames_ptr();
        let src_lane_heads = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { *self.lane_heads.get() };
        let mut lane_idx = 0usize;
        while lane_idx < self.lane_slots as usize {
            let mut current = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { *src_lane_heads.add(lane_idx) };
            while current != Self::NO_FRAME {
                let src_idx = current as usize;
                let src_frame = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { *src_frames.add(src_idx) };
                let Some(dst_idx) = (/* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */unsafe { Self::raw_pop_free(frames, free_head) })
                else {
                    panic!("loop table migration ran out of frame capacity");
                };
                let head = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { *lane_heads.add(lane_idx) };
                /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
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
        debug_assert_eq!(self.lane_base, lane_base);
        /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
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
        lane_base: u32,
        lane_slots: usize,
        reclaim_delta: usize,
    ) {
        let frames = storage.cast::<LoopFrame>();
        let lane_heads_offset = Self::align_up(
            storage as usize + loop_slots.saturating_mul(core::mem::size_of::<LoopFrame>()),
            core::mem::align_of::<u16>(),
        ) - storage as usize;
        let lane_heads = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { storage.add(lane_heads_offset) }.cast::<u16>();
        let free_head_offset = Self::align_up(
            storage as usize
                + lane_heads_offset
                + lane_slots.saturating_mul(core::mem::size_of::<u16>()),
            core::mem::align_of::<u16>(),
        ) - storage as usize;
        let free_head = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { storage.add(free_head_offset) }.cast::<u16>();
        /* SAFETY: the rendezvous table owns initialized slots behind explicit presence state before raw access. */
        unsafe {
            self.bind_storage(
                frames,
                loop_slots,
                lane_base,
                lane_slots,
                lane_heads,
                free_head,
                reclaim_delta,
            );
        }
    }

    pub(crate) unsafe fn migrate_from_storage(
        &self,
        storage: *mut u8,
        loop_slots: usize,
        lane_base: u32,
        lane_slots: usize,
    ) {
        let frames = storage.cast::<LoopFrame>();
        let lane_heads_offset = Self::align_up(
            storage as usize + loop_slots.saturating_mul(core::mem::size_of::<LoopFrame>()),
            core::mem::align_of::<u16>(),
        ) - storage as usize;
        let lane_heads = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { storage.add(lane_heads_offset) }.cast::<u16>();
        let free_head_offset = Self::align_up(
            storage as usize
                + lane_heads_offset
                + lane_slots.saturating_mul(core::mem::size_of::<u16>()),
            core::mem::align_of::<u16>(),
        ) - storage as usize;
        let free_head = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { storage.add(free_head_offset) }.cast::<u16>();
        /* SAFETY: the rendezvous table owns initialized slots behind explicit presence state before raw access. */
        unsafe {
            self.migrate_to(
                frames, loop_slots, lane_base, lane_slots, lane_heads, free_head,
            );
        }
    }

    pub(crate) unsafe fn rebind_from_storage(
        &mut self,
        storage: *mut u8,
        loop_slots: usize,
        lane_base: u32,
        lane_slots: usize,
        reclaim_delta: usize,
    ) {
        let frames = storage.cast::<LoopFrame>();
        let lane_heads_offset = Self::align_up(
            storage as usize + loop_slots.saturating_mul(core::mem::size_of::<LoopFrame>()),
            core::mem::align_of::<u16>(),
        ) - storage as usize;
        let lane_heads = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { storage.add(lane_heads_offset) }.cast::<u16>();
        let free_head_offset = Self::align_up(
            storage as usize
                + lane_heads_offset
                + lane_slots.saturating_mul(core::mem::size_of::<u16>()),
            core::mem::align_of::<u16>(),
        ) - storage as usize;
        let free_head = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { storage.add(free_head_offset) }.cast::<u16>();
        /* SAFETY: the rendezvous table owns initialized slots behind explicit presence state before raw access. */
        unsafe {
            self.rebind_storage(
                frames,
                loop_slots,
                lane_base,
                lane_slots,
                lane_heads,
                free_head,
                reclaim_delta,
            );
        }
    }

    #[inline]
    fn frames_ptr(&self) -> *mut LoopFrame {
        self.raw_frames()
            .map_addr(|addr| addr & !Self::STORAGE_TAG_MASK)
    }

    #[inline]
    fn lane_heads_ptr(&self) -> *mut u16 {
        /* SAFETY: the rendezvous table owns initialized slots behind explicit presence state before raw access. */
        unsafe { *self.lane_heads.get() }
    }

    #[inline]
    fn free_head_ptr(&self) -> *mut u16 {
        /* SAFETY: the rendezvous table owns initialized slots behind explicit presence state before raw access. */
        unsafe { *self.free_head.get() }
    }

    #[inline]
    fn lane_idx(&self, lane: Lane) -> usize {
        debug_assert!(lane.raw() >= self.lane_base);
        let lane_idx = (lane.raw() - self.lane_base) as usize;
        debug_assert!(lane_idx < self.lane_slots as usize);
        lane_idx
    }

    #[inline]
    fn frame_ref(&self, frame_idx: usize) -> &LoopFrame {
        /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
        unsafe { &*self.frames_ptr().add(frame_idx) }
    }

    #[inline]
    fn frame_mut(&self, frame_idx: usize) -> &mut LoopFrame {
        /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
        unsafe { &mut *self.frames_ptr().add(frame_idx) }
    }

    unsafe fn raw_pop_free(frames: *mut LoopFrame, free_head: *mut u16) -> Option<usize> {
        let head = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { *free_head };
        if head == Self::NO_FRAME {
            return None;
        }
        let idx = head as usize;
        let next = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { (*frames.add(idx)).next };
        /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
        unsafe {
            *free_head = next;
            (*frames.add(idx)).next = Self::NO_FRAME;
        }
        Some(idx)
    }

    unsafe fn raw_push_free(frames: *mut LoopFrame, free_head: *mut u16, frame_idx: usize) {
        let head = /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */ unsafe { *free_head };
        /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
        unsafe {
            frames.add(frame_idx).write(LoopFrame::free(head));
            *free_head = frame_idx as u16;
        }
    }

    fn frame_for_idx(&self, lane_idx: usize, idx: u8) -> Option<usize> {
        let mut current = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { *self.lane_heads_ptr().add(lane_idx) };
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
        let Some(frame_idx) = (/* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */unsafe {
            Self::raw_pop_free(self.frames_ptr(), self.free_head_ptr())
        }) else {
            panic!("loop table slot exhausted");
        };
        let head = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { *self.lane_heads_ptr().add(lane_idx) };
        let frame = self.frame_mut(frame_idx);
        *frame = LoopFrame::assign(idx, head);
        /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
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
        let lane_idx = self.lane_idx(lane);
        let frame_idx = self.frame_or_alloc(lane_idx, idx);
        let entry = &mut self.frame_mut(frame_idx).entry;
        let mut epoch = entry.epoch.wrapping_add(1);
        if epoch == 0 {
            epoch = 1;
        }
        entry.epoch = epoch;
        entry.decision = disposition;
        entry.seen_mask = 0;
        if (role_from as usize) < MAX_TRACKED_ROLES {
            entry.seen_mask |= Self::seen_bit(role_from as usize);
        }

        epoch
    }

    pub(crate) fn acknowledge(&self, lane: Lane, role: u8, idx: u8) {
        if (role as usize) >= MAX_TRACKED_ROLES {
            return;
        }
        if self.loop_slots == 0 {
            return;
        }
        let lane_idx = self.lane_idx(lane);
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
        let lane_idx = self.lane_idx(lane);
        let mut current = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { *self.lane_heads_ptr().add(lane_idx) };
        /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
        unsafe {
            *self.lane_heads_ptr().add(lane_idx) = Self::NO_FRAME;
        }
        while current != Self::NO_FRAME {
            let frame_idx = current as usize;
            let next = self.frame_ref(frame_idx).next;
            /* SAFETY: the rendezvous table owns initialized slots behind explicit presence state before raw access. */
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
        let lane_idx = self.lane_idx(lane);
        let Some(frame_idx) = self.frame_for_idx(lane_idx, idx) else {
            return false;
        };
        self.frame_ref(frame_idx).entry.epoch != 0
    }
}
