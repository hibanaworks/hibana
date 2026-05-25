use super::*;
// # Unsafe Owner Contract
//
// This fragment owns route-decision table frames and route-scope head columns.
// Unsafe operations bind resident storage once, keep route frames in explicit
// free lists, and access table slots only after scope canonicalization and
// table-capacity checks performed by this owner.
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
    pub(crate) epoch: u16,
    pub(crate) arm: u8,
    seen_mask: u16,
}

impl RouteEntry {
    pub(crate) const fn empty() -> Self {
        Self {
            epoch: 0,
            arm: 0,
            seen_mask: 0,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct RouteFrame {
    pub(crate) scope: ScopeId,
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

    pub(crate) fn free(next: u16) -> Self {
        Self {
            scope: ScopeId::none(),
            entry: RouteEntry::empty(),
            next,
        }
    }
}

struct RouteTableStorageParts {
    frames: *mut RouteFrame,
    lane_heads: *mut u16,
    free_head: *mut u16,
    pending_frame_hint_masks: *mut FrameLabelMask,
    waiters: *mut WaiterSlot,
}

pub(crate) struct RouteTable {
    frames: UnsafeCell<*mut RouteFrame>,
    route_slots: usize,
    lane_base: u32,
    lane_slots: u16,
    lane_heads: UnsafeCell<*mut u16>,
    free_head: UnsafeCell<*mut u16>,
    pending_frame_hint_masks: UnsafeCell<*mut FrameLabelMask>,
    change_epoch: UnsafeCell<u16>,
    waiters: UnsafeCell<*mut WaiterSlot>,
    _no_send_sync: PhantomData<*mut ()>,
}

impl Default for RouteTable {
    fn default() -> Self {
        Self::empty()
    }
}

impl RouteTable {
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
            route_slots: 0,
            lane_base: 0,
            lane_slots: 0,
            lane_heads: UnsafeCell::new(core::ptr::null_mut()),
            free_head: UnsafeCell::new(core::ptr::null_mut()),
            pending_frame_hint_masks: UnsafeCell::new(core::ptr::null_mut()),
            change_epoch: UnsafeCell::new(0),
            waiters: UnsafeCell::new(core::ptr::null_mut()),
            _no_send_sync: PhantomData,
        }
    }

    pub(crate) unsafe fn init_empty(dst: *mut Self) {
        unsafe {
            // SAFETY: the caller provides exclusive, writable storage for one
            // `RouteTable`; every field is initialized exactly once before the
            // table is exposed through safe methods.
            core::ptr::addr_of_mut!((*dst).frames).write(UnsafeCell::new(core::ptr::null_mut()));
            core::ptr::addr_of_mut!((*dst).route_slots).write(0);
            core::ptr::addr_of_mut!((*dst).lane_base).write(0);
            core::ptr::addr_of_mut!((*dst).lane_slots).write(0);
            core::ptr::addr_of_mut!((*dst).lane_heads)
                .write(UnsafeCell::new(core::ptr::null_mut()));
            core::ptr::addr_of_mut!((*dst).free_head).write(UnsafeCell::new(core::ptr::null_mut()));
            core::ptr::addr_of_mut!((*dst).pending_frame_hint_masks)
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
        let storage = /* SAFETY: the rendezvous table owns initialized slots behind explicit presence state before raw access. */ unsafe { std::alloc::alloc_zeroed(layout) };
        if storage.is_null() {
            std::alloc::handle_alloc_error(layout);
        }
        storage
    }

    #[cfg(test)]
    pub(crate) fn build_test_table(route_slots: usize, lane_base: u32, lane_slots: usize) -> Self {
        let mut table = Self::empty();
        let storage = Self::allocate_test_storage(route_slots, lane_slots);
        /* SAFETY: the rendezvous table owns initialized slots behind explicit presence state before raw access. */
        unsafe {
            table.bind_from_storage_with_layout(storage, route_slots, lane_base, lane_slots, 0);
        }
        table
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
        let hint_align = core::mem::align_of::<FrameLabelMask>();
        let waiter_align = core::mem::align_of::<WaiterSlot>();
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
            core::mem::align_of::<FrameLabelMask>(),
        );
        let hint_bytes = lane_slots.saturating_mul(core::mem::size_of::<FrameLabelMask>());
        let waiters_offset = Self::align_up(
            hint_offset.saturating_add(hint_bytes),
            core::mem::align_of::<WaiterSlot>(),
        );
        waiters_offset.saturating_add(
            lane_slots
                .saturating_mul(MAX_TRACKED_ROLES)
                .saturating_mul(core::mem::size_of::<WaiterSlot>()),
        )
    }

    fn encode_frames_ptr(frames: *mut RouteFrame, reclaim_delta: usize) -> *mut RouteFrame {
        debug_assert_eq!(frames.addr() & Self::STORAGE_TAG_MASK, 0);
        debug_assert!(reclaim_delta <= Self::STORAGE_TAG_MASK);
        frames.map_addr(|addr| addr | reclaim_delta)
    }

    #[inline]
    fn raw_frames(&self) -> *mut RouteFrame {
        /* SAFETY: the rendezvous table owns initialized slots behind explicit presence state before raw access. */
        unsafe { *self.frames.get() }
    }

    #[inline]
    fn raw_pending_frame_hint_masks(&self) -> *mut FrameLabelMask {
        /* SAFETY: the rendezvous table owns initialized slots behind explicit presence state before raw access. */
        unsafe { *self.pending_frame_hint_masks.get() }
    }

    pub(crate) unsafe fn bind_storage(
        &mut self,
        frames: *mut RouteFrame,
        route_slots: usize,
        lane_base: u32,
        lane_slots: usize,
        lane_heads: *mut u16,
        free_head: *mut u16,
        pending_frame_hint_masks: *mut FrameLabelMask,
        waiters: *mut WaiterSlot,
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
                // SAFETY: `bind_storage` owns the route-frame backing slice for
                // `route_slots` entries and initializes each slot exactly once.
                frames.add(idx).write(RouteFrame::free(next));
            }
            idx += 1;
        }
        let mut lane_idx = 0usize;
        while lane_idx < lane_slots {
            unsafe {
                // SAFETY: `lane_heads` points at `lane_slots` caller-owned u16
                // entries reserved for this `RouteTable` owner.
                lane_heads.add(lane_idx).write(Self::NO_FRAME);
            }
            lane_idx += 1;
        }
        unsafe {
            // SAFETY: `free_head` is the single u16 free-list head owned by this
            // route table storage layout.
            free_head.write(if route_slots == 0 { Self::NO_FRAME } else { 0 });
        }
        let mut hint_idx = 0usize;
        while hint_idx < lane_slots {
            unsafe {
                // SAFETY: `pending_frame_hint_masks` has one initialized slot per
                // lane owned by this table.
                pending_frame_hint_masks
                    .add(hint_idx)
                    .write(FrameLabelMask::EMPTY);
            }
            hint_idx += 1;
        }
        let mut waiter_idx = 0usize;
        while waiter_idx < lane_slots.saturating_mul(MAX_TRACKED_ROLES) {
            unsafe {
                // SAFETY: the waiter arena contains `lane_slots *
                // MAX_TRACKED_ROLES` entries owned exclusively by this table.
                WaiterSlot::init_empty(waiters.add(waiter_idx));
            }
            waiter_idx += 1;
        }
        *self.frames.get_mut() = Self::encode_frames_ptr(frames, reclaim_delta);
        self.route_slots = route_slots;
        self.lane_base = lane_base;
        self.lane_slots = lane_slots as u16;
        *self.lane_heads.get_mut() = lane_heads;
        *self.free_head.get_mut() = free_head;
        *self.pending_frame_hint_masks.get_mut() = pending_frame_hint_masks;
        *self.change_epoch.get_mut() = 0;
        *self.waiters.get_mut() = waiters;
    }

    unsafe fn rebind_storage(
        &mut self,
        frames: *mut RouteFrame,
        route_slots: usize,
        lane_base: u32,
        lane_slots: usize,
        lane_heads: *mut u16,
        free_head: *mut u16,
        pending_frame_hint_masks: *mut FrameLabelMask,
        waiters: *mut WaiterSlot,
        reclaim_delta: usize,
    ) {
        *self.frames.get_mut() = Self::encode_frames_ptr(frames, reclaim_delta);
        self.route_slots = route_slots;
        self.lane_base = lane_base;
        self.lane_slots = lane_slots as u16;
        *self.lane_heads.get_mut() = lane_heads;
        *self.free_head.get_mut() = free_head;
        *self.pending_frame_hint_masks.get_mut() = pending_frame_hint_masks;
        *self.waiters.get_mut() = waiters;
    }

    unsafe fn raw_pop_free(frames: *mut RouteFrame, free_head: *mut u16) -> Option<usize> {
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

    unsafe fn raw_push_free(frames: *mut RouteFrame, free_head: *mut u16, idx: usize) {
        let next = /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */ unsafe { *free_head };
        /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
        unsafe {
            frames.add(idx).write(RouteFrame::free(next));
            *free_head = idx as u16;
        }
    }

    unsafe fn migrate_to(
        &self,
        frames: *mut RouteFrame,
        route_slots: usize,
        lane_base: u32,
        lane_slots: usize,
        lane_heads: *mut u16,
        free_head: *mut u16,
        pending_frame_hint_masks: *mut FrameLabelMask,
        waiters: *mut WaiterSlot,
    ) {
        debug_assert!(lane_slots >= self.lane_slots());
        let mut idx = 0usize;
        while idx < route_slots {
            let next = if idx + 1 < route_slots {
                (idx + 1) as u16
            } else {
                Self::NO_FRAME
            };
            /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
            unsafe {
                frames.add(idx).write(RouteFrame::free(next));
            }
            idx += 1;
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
            free_head.write(if route_slots == 0 { Self::NO_FRAME } else { 0 });
        }
        let mut hint_idx = 0usize;
        while hint_idx < self.lane_slots() {
            /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
            unsafe {
                pending_frame_hint_masks
                    .add(hint_idx)
                    .write(*self.pending_frame_hint_masks_ptr().add(hint_idx));
            }
            hint_idx += 1;
        }
        while hint_idx < lane_slots {
            /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
            unsafe {
                pending_frame_hint_masks
                    .add(hint_idx)
                    .write(FrameLabelMask::EMPTY);
            }
            hint_idx += 1;
        }
        let mut waiter_idx = 0usize;
        let waiter_count = lane_slots.saturating_mul(MAX_TRACKED_ROLES);
        let src_waiter_count = self.lane_slots().saturating_mul(MAX_TRACKED_ROLES);
        while waiter_idx < src_waiter_count {
            /* SAFETY: the caller supplies exclusive uninitialized storage and this initializer writes all exposed fields before return. */
            unsafe {
                let src_waiter = &mut *self.waiters_ptr().add(waiter_idx);
                if let Some(waker) = src_waiter.take() {
                    WaiterSlot::init_owned(waiters.add(waiter_idx), waker);
                } else {
                    WaiterSlot::init_empty(waiters.add(waiter_idx));
                }
            }
            waiter_idx += 1;
        }
        while waiter_idx < waiter_count {
            /* SAFETY: the caller supplies exclusive uninitialized storage and this initializer writes all exposed fields before return. */
            unsafe {
                WaiterSlot::init_empty(waiters.add(waiter_idx));
            }
            waiter_idx += 1;
        }
        if self.route_slots == 0 {
            return;
        }
        let mut lane_idx = 0usize;
        while lane_idx < self.lane_slots() {
            let mut current = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { *self.lane_heads_ptr().add(lane_idx) };
            let mut prev_new = Self::NO_FRAME;
            while current != Self::NO_FRAME {
                let src_idx = current as usize;
                let next = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { (*self.frames_ptr().add(src_idx)).next };
                let dst_idx = /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */ unsafe { Self::raw_pop_free(frames, free_head) }
                    .expect("route ledger migration exhausted frame capacity");
                let mut moved = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { *self.frames_ptr().add(src_idx) };
                moved.next = Self::NO_FRAME;
                /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
                unsafe {
                    frames.add(dst_idx).write(moved);
                }
                if prev_new == Self::NO_FRAME {
                    /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
                    unsafe {
                        *lane_heads.add(lane_idx) = dst_idx as u16;
                    }
                } else {
                    /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
                    unsafe {
                        (*frames.add(prev_new as usize)).next = dst_idx as u16;
                    }
                }
                prev_new = dst_idx as u16;
                current = next;
            }
            lane_idx += 1;
        }
        debug_assert_eq!(self.lane_base, lane_base);
    }

    pub(crate) unsafe fn bind_from_storage_with_layout(
        &mut self,
        storage: *mut u8,
        route_slots: usize,
        lane_base: u32,
        lane_slots: usize,
        reclaim_delta: usize,
    ) {
        let parts = /* SAFETY: the rendezvous table owns initialized slots behind explicit presence state before raw access. */ unsafe { Self::storage_parts(storage, route_slots, lane_slots) };
        /* SAFETY: the rendezvous table owns initialized slots behind explicit presence state before raw access. */
        unsafe {
            self.bind_storage(
                parts.frames,
                route_slots,
                lane_base,
                lane_slots,
                parts.lane_heads,
                parts.free_head,
                parts.pending_frame_hint_masks,
                parts.waiters,
                reclaim_delta,
            );
        }
    }

    pub(crate) unsafe fn migrate_from_storage(
        &self,
        storage: *mut u8,
        route_slots: usize,
        lane_base: u32,
        lane_slots: usize,
    ) {
        let parts = /* SAFETY: the rendezvous table owns initialized slots behind explicit presence state before raw access. */ unsafe { Self::storage_parts(storage, route_slots, lane_slots) };
        /* SAFETY: the rendezvous table owns initialized slots behind explicit presence state before raw access. */
        unsafe {
            self.migrate_to(
                parts.frames,
                route_slots,
                lane_base,
                lane_slots,
                parts.lane_heads,
                parts.free_head,
                parts.pending_frame_hint_masks,
                parts.waiters,
            );
        }
    }

    pub(crate) unsafe fn rebind_from_storage(
        &mut self,
        storage: *mut u8,
        route_slots: usize,
        lane_base: u32,
        lane_slots: usize,
        reclaim_delta: usize,
    ) {
        let parts = /* SAFETY: the rendezvous table owns initialized slots behind explicit presence state before raw access. */ unsafe { Self::storage_parts(storage, route_slots, lane_slots) };
        /* SAFETY: the rendezvous table owns initialized slots behind explicit presence state before raw access. */
        unsafe {
            self.rebind_storage(
                parts.frames,
                route_slots,
                lane_base,
                lane_slots,
                parts.lane_heads,
                parts.free_head,
                parts.pending_frame_hint_masks,
                parts.waiters,
                reclaim_delta,
            );
        }
    }

    unsafe fn storage_parts(
        storage: *mut u8,
        route_slots: usize,
        lane_slots: usize,
    ) -> RouteTableStorageParts {
        let frames = storage.cast::<RouteFrame>();
        let frames_bytes = route_slots.saturating_mul(core::mem::size_of::<RouteFrame>());
        let lane_heads_offset = Self::align_up(
            storage as usize + frames_bytes,
            core::mem::align_of::<u16>(),
        ) - storage as usize;
        // SAFETY: `storage` is the caller-provided route-table arena. This
        // owner derives all column pointers from the single layout formula used
        // by `storage_layout`, so each pointer stays within that arena when the
        // caller supplied the advertised layout.
        let lane_heads = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { storage.add(lane_heads_offset) }.cast::<u16>();
        let lane_heads_bytes = lane_slots.saturating_mul(core::mem::size_of::<u16>());
        let free_head_offset = Self::align_up(
            storage as usize + lane_heads_offset + lane_heads_bytes,
            core::mem::align_of::<u16>(),
        ) - storage as usize;
        // SAFETY: See the lane-head derivation above; this is the next aligned
        // column in the same resident route-table arena.
        let free_head = unsafe { storage.add(free_head_offset) }.cast::<u16>();
        let hint_offset = Self::align_up(
            storage as usize + free_head_offset + core::mem::size_of::<u16>(),
            core::mem::align_of::<FrameLabelMask>(),
        ) - storage as usize;
        // SAFETY: The pending-hint column is derived by the same storage layout
        // owner and follows the single free-head slot.
        let pending_frame_hint_masks = unsafe { storage.add(hint_offset) }.cast::<FrameLabelMask>();
        let hint_bytes = lane_slots.saturating_mul(core::mem::size_of::<FrameLabelMask>());
        let waiters_offset = Self::align_up(
            storage as usize + hint_offset + hint_bytes,
            core::mem::align_of::<WaiterSlot>(),
        ) - storage as usize;
        // SAFETY: The waiter column is the final aligned column owned by the
        // route table storage layout.
        let waiters = unsafe { storage.add(waiters_offset) }.cast::<WaiterSlot>();
        RouteTableStorageParts {
            frames,
            lane_heads,
            free_head,
            pending_frame_hint_masks,
            waiters,
        }
    }

    #[inline]
    fn frames_ptr(&self) -> *mut RouteFrame {
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
    fn pending_frame_hint_masks_ptr(&self) -> *mut FrameLabelMask {
        self.raw_pending_frame_hint_masks()
    }

    #[inline]
    fn waiters_ptr(&self) -> *mut WaiterSlot {
        /* SAFETY: the rendezvous table owns initialized slots behind explicit presence state before raw access. */
        unsafe { *self.waiters.get() }
    }

    #[inline]
    fn lane_slot(&self, lane: Lane) -> usize {
        debug_assert!(lane.raw() >= self.lane_base);
        let lane_idx = (lane.raw() - self.lane_base) as usize;
        debug_assert!(
            lane_idx < self.lane_slots(),
            "route lane must fit bound lane span"
        );
        lane_idx
    }

    #[inline]
    fn role_slot_count(role_count: u8) -> usize {
        core::cmp::min(role_count as usize, MAX_TRACKED_ROLES)
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
        /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
        unsafe { &*self.frames_ptr().add(idx) }
    }

    #[inline]
    fn frame_mut(&self, idx: usize) -> &mut RouteFrame {
        /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
        unsafe { &mut *self.frames_ptr().add(idx) }
    }

    #[inline]
    fn slot_for_scope(&self, lane_idx: usize, coord: ScopeCoord) -> Option<usize> {
        let mut current = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { *self.lane_heads_ptr().add(lane_idx) };
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
        let idx = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { Self::raw_pop_free(self.frames_ptr(), self.free_head_ptr()) }?;
        let head = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { *self.lane_heads_ptr().add(lane_idx) };
        *self.frame_mut(idx) = RouteFrame::assign(coord, head);
        /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
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
        let mut current = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { *self.lane_heads_ptr().add(lane_idx) };
        while current != Self::NO_FRAME {
            let current_idx = current as usize;
            let next = self.frame_ref(current_idx).next;
            if current_idx == slot_idx {
                if prev == Self::NO_FRAME {
                    /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
                    unsafe {
                        *self.lane_heads_ptr().add(lane_idx) = next;
                    }
                } else {
                    self.frame_mut(prev as usize).next = next;
                }
                /* SAFETY: the rendezvous table owns initialized slots behind explicit presence state before raw access. */
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
    fn seen_bit(role_idx: usize) -> u16 {
        debug_assert!(role_idx < u16::BITS as usize);
        1u16 << (role_idx as u32)
    }

    #[inline]
    fn bump_change_epoch(&self) {
        let epoch = /* SAFETY: the pointer comes from pinned owner storage and this path holds the unique mutable access for the borrow. */ unsafe { &mut *self.change_epoch.get() };
        let next = epoch.wrapping_add(1);
        *epoch = if next == 0 { 1 } else { next };
    }

    #[inline]
    pub(crate) fn change_epoch(&self) -> u16 {
        /* SAFETY: the rendezvous table owns initialized slots behind explicit presence state before raw access. */
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
            let free_head = /* SAFETY: the rendezvous table owns initialized slots behind explicit presence state before raw access. */ unsafe { *self.free_head_ptr() };
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
        while role_idx < MAX_TRACKED_ROLES {
            /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
            unsafe {
                (*waiters.add(lane_idx * MAX_TRACKED_ROLES + role_idx)).wake();
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
        let slot = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { &mut *waiters.add(lane_idx * MAX_TRACKED_ROLES + role as usize) };
        slot.set(cx.waker());
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

    pub(crate) fn has_pending_lane_with_role_count(
        &self,
        role_count: u8,
        role: u8,
        scope: ScopeId,
        lane: Lane,
    ) -> bool {
        let role_slots = Self::role_slot_count(role_count);
        if (role as usize) >= role_slots {
            return false;
        }
        let coord = match ScopeCoord::from_scope(scope) {
            Some(coord) => coord,
            None => return false,
        };
        let role_bit = Self::seen_bit(role as usize);
        let lane_idx = self.lane_slot(lane);
        if let Some(slot_idx) = Self::slot_for_scope(self, lane_idx, coord) {
            let entry = self.frame_ref(slot_idx).entry;
            return entry.epoch != 0 && (entry.seen_mask & role_bit) == 0;
        }
        false
    }

    #[inline]
    pub(crate) fn pending_frame_hint_mask_for_lane(&self, lane: Lane) -> FrameLabelMask {
        if self.route_slots == 0 {
            return FrameLabelMask::EMPTY;
        }
        let lane_idx = self.lane_slot(lane);
        /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
        unsafe { *self.pending_frame_hint_masks_ptr().add(lane_idx) }
    }

    pub(crate) fn update_pending_frame_hint_mask_for_lane(
        &self,
        lane: Lane,
        before: FrameLabelMask,
        after: FrameLabelMask,
    ) {
        if before == after || self.route_slots == 0 {
            return;
        }
        let lane_idx = self.lane_slot(lane);
        /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
        unsafe {
            *self.pending_frame_hint_masks_ptr().add(lane_idx) = after;
        }
        self.bump_change_epoch();
    }

    pub(crate) fn has_pending_frame_hint_for_lane(
        &self,
        lane: Lane,
        frame_label_mask: FrameLabelMask,
    ) -> bool {
        if self.route_slots == 0 {
            return false;
        }
        let lane_idx = self.lane_slot(lane);
        /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
        unsafe { *self.pending_frame_hint_masks_ptr().add(lane_idx) }.intersects(frame_label_mask)
    }

    pub(crate) fn reset_lane(&self, lane: Lane) {
        if self.route_slots == 0 {
            return;
        }
        let lane_idx = self.lane_slot(lane);
        let mut current = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { *self.lane_heads_ptr().add(lane_idx) };
        /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
        unsafe {
            *self.lane_heads_ptr().add(lane_idx) = Self::NO_FRAME;
        }
        while current != Self::NO_FRAME {
            let idx = current as usize;
            let next = self.frame_ref(idx).next;
            /* SAFETY: the rendezvous table owns initialized slots behind explicit presence state before raw access. */
            unsafe {
                Self::raw_push_free(self.frames_ptr(), self.free_head_ptr(), idx);
            }
            current = next;
        }
        let pending_frame_hint_masks = self.pending_frame_hint_masks_ptr();
        /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
        unsafe {
            *pending_frame_hint_masks.add(lane_idx) = FrameLabelMask::EMPTY;
        }
        let waiters = self.waiters_ptr();
        let mut role_idx = 0usize;
        while role_idx < MAX_TRACKED_ROLES {
            /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
            unsafe {
                (*waiters.add(lane_idx * MAX_TRACKED_ROLES + role_idx)).clear();
            }
            role_idx += 1;
        }
        self.bump_change_epoch();
    }

    pub(crate) fn wake_lane_waiters(&self, lane: Lane) {
        if self.route_slots == 0 {
            return;
        }
        let lane_idx = self.lane_slot(lane);
        let waiters = self.waiters_ptr();
        let mut role_idx = 0usize;
        while role_idx < MAX_TRACKED_ROLES {
            /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
            unsafe {
                (*waiters.add(lane_idx * MAX_TRACKED_ROLES + role_idx)).wake();
            }
            role_idx += 1;
        }
    }
}
