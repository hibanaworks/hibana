//! # Unsafe Owner Contract
//!
//! `RouteTable` owns a caller-provided resident storage region split into
//! route-frame slots, lane-head indices, one free-list head, pending hint
//! masks, and waiter slots. Ingress and migration initialize every region
//! before safe table methods can observe it. All raw pointer helpers in this
//! module are reached through that table owner, which keeps the backing storage
//! pinned for the table lifetime, bounds slot/lane indices before pointer
//! arithmetic, and serializes mutation through `&mut RouteTable` or through
//! table-owned `UnsafeCell` fields.

use super::super::{checked_add_usize, checked_mul_usize, checked_sub_usize};
use super::{
    FrameLabelMask, MAX_TRACKED_ROLES, PhantomData, RouteFrame, RouteTable,
    RouteTableStorageBinding, RouteTableStorageParts, RouteTableStorageShape, UnsafeCell,
    WaiterSlot,
};
impl RouteTableStorageParts {
    unsafe fn pop_free_slot(&self) -> Option<usize> {
        let head = /* SAFETY: the free-list head belongs to this route-table column bundle. */ unsafe { *self.free_head };
        if head == RouteTable::NO_FRAME {
            return None;
        }
        let idx = head as usize;
        let next = /* SAFETY: `idx` was obtained from this bundle's free list and therefore names a frame slot in this bundle. */ unsafe {
            (*self.frames.add(idx)).next
        };
        /* SAFETY: the frame slot and free-list head are owned by the same column bundle and are updated as one free-list transition. */
        unsafe {
            *self.free_head = next;
            (*self.frames.add(idx)).next = RouteTable::NO_FRAME;
        }
        Some(idx)
    }

    unsafe fn push_free_slot(&self, idx: usize) {
        let next = /* SAFETY: the free-list head belongs to this route-table column bundle. */ unsafe { *self.free_head };
        /* SAFETY: callers return only frame slots owned by this column bundle; the slot becomes the new free-list head. */
        unsafe {
            self.frames.add(idx).write(RouteFrame::free(next));
            *self.free_head = idx as u16;
        }
    }
}

impl RouteTable {
    pub(crate) const NO_FRAME: u16 = u16::MAX;
    pub(crate) const STORAGE_TAG_MASK: usize = Self::storage_align() - 1;

    #[inline(always)]
    pub(crate) const fn align_up(value: usize, align: usize) -> usize {
        if align == 0 {
            crate::invariant();
        }
        let mask = align - 1;
        if value > usize::MAX - mask {
            crate::invariant();
        }
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
            change_generation: UnsafeCell::new(0),
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
            core::ptr::addr_of_mut!((*dst).change_generation)
                .cast::<u16>()
                .write(0);
            core::ptr::addr_of_mut!((*dst).waiters).write(UnsafeCell::new(core::ptr::null_mut()));
            core::ptr::addr_of_mut!((*dst)._no_send_sync).write(PhantomData);
        }
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
        let frames_bytes = checked_mul_usize(route_slots, core::mem::size_of::<RouteFrame>());
        let lane_heads_offset = Self::align_up(frames_bytes, core::mem::align_of::<u16>());
        let lane_heads_bytes = checked_mul_usize(lane_slots, core::mem::size_of::<u16>());
        let free_head_offset = Self::align_up(
            checked_add_usize(lane_heads_offset, lane_heads_bytes),
            core::mem::align_of::<u16>(),
        );
        let free_head_bytes = core::mem::size_of::<u16>();
        let hint_offset = Self::align_up(
            checked_add_usize(free_head_offset, free_head_bytes),
            core::mem::align_of::<FrameLabelMask>(),
        );
        let hint_bytes = checked_mul_usize(lane_slots, core::mem::size_of::<FrameLabelMask>());
        let waiters_offset = Self::align_up(
            checked_add_usize(hint_offset, hint_bytes),
            core::mem::align_of::<WaiterSlot>(),
        );
        checked_add_usize(
            waiters_offset,
            checked_mul_usize(
                checked_mul_usize(lane_slots, MAX_TRACKED_ROLES),
                core::mem::size_of::<WaiterSlot>(),
            ),
        )
    }

    fn encode_frames_ptr(frames: *mut RouteFrame, reclaim_delta: usize) -> *mut RouteFrame {
        if frames.addr() & Self::STORAGE_TAG_MASK != 0 {
            crate::invariant();
        }
        if reclaim_delta > Self::STORAGE_TAG_MASK {
            crate::invariant();
        }
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

    unsafe fn bind_storage(&mut self, binding: RouteTableStorageBinding) {
        let RouteTableStorageBinding {
            parts,
            shape,
            reclaim_delta,
        } = binding;
        let mut idx = 0usize;
        while idx < shape.route_slots {
            let next = if idx + 1 < shape.route_slots {
                (idx + 1) as u16
            } else {
                Self::NO_FRAME
            };
            unsafe {
                // SAFETY: `bind_storage` owns the route-frame backing slice for
                // `route_slots` entries and initializes each slot exactly once.
                parts.frames.add(idx).write(RouteFrame::free(next));
            }
            idx += 1;
        }
        let mut lane_idx = 0usize;
        while lane_idx < shape.lane_slots {
            unsafe {
                // SAFETY: `lane_heads` points at `lane_slots` caller-owned u16
                // entries reserved for this `RouteTable` owner.
                parts.lane_heads.add(lane_idx).write(Self::NO_FRAME);
            }
            lane_idx += 1;
        }
        unsafe {
            // SAFETY: `free_head` is the single u16 free-list head owned by this
            // route table storage layout.
            parts.free_head.write(if shape.route_slots == 0 {
                Self::NO_FRAME
            } else {
                0
            });
        }
        let mut hint_idx = 0usize;
        while hint_idx < shape.lane_slots {
            unsafe {
                // SAFETY: `pending_frame_hint_masks` has one initialized slot per
                // lane owned by this table.
                parts
                    .pending_frame_hint_masks
                    .add(hint_idx)
                    .write(FrameLabelMask::EMPTY);
            }
            hint_idx += 1;
        }
        let waiter_count = checked_mul_usize(shape.lane_slots, MAX_TRACKED_ROLES);
        let mut waiter_idx = 0usize;
        while waiter_idx < waiter_count {
            unsafe {
                // SAFETY: the waiter arena contains `lane_slots *
                // MAX_TRACKED_ROLES` entries owned exclusively by this table.
                WaiterSlot::init_empty(parts.waiters.add(waiter_idx));
            }
            waiter_idx += 1;
        }
        *self.frames.get_mut() = Self::encode_frames_ptr(parts.frames, reclaim_delta);
        self.route_slots = shape.route_slots;
        self.lane_base = shape.lane_base;
        self.lane_slots = shape.lane_slots as u16;
        *self.lane_heads.get_mut() = parts.lane_heads;
        *self.free_head.get_mut() = parts.free_head;
        *self.pending_frame_hint_masks.get_mut() = parts.pending_frame_hint_masks;
        *self.change_generation.get_mut() = 0;
        *self.waiters.get_mut() = parts.waiters;
    }

    unsafe fn rebind_storage(&mut self, binding: RouteTableStorageBinding) {
        *self.frames.get_mut() =
            Self::encode_frames_ptr(binding.parts.frames, binding.reclaim_delta);
        self.route_slots = binding.shape.route_slots;
        self.lane_base = binding.shape.lane_base;
        self.lane_slots = binding.shape.lane_slots as u16;
        *self.lane_heads.get_mut() = binding.parts.lane_heads;
        *self.free_head.get_mut() = binding.parts.free_head;
        *self.pending_frame_hint_masks.get_mut() = binding.parts.pending_frame_hint_masks;
        *self.waiters.get_mut() = binding.parts.waiters;
    }

    #[inline]
    fn storage_parts_current(&self) -> RouteTableStorageParts {
        RouteTableStorageParts {
            frames: self.frames_ptr(),
            lane_heads: self.lane_heads_ptr(),
            free_head: self.free_head_ptr(),
            pending_frame_hint_masks: self.pending_frame_hint_masks_ptr(),
            waiters: self.waiters_ptr(),
        }
    }

    #[inline]
    pub(super) fn pop_free_slot(&self) -> Option<usize> {
        let parts = self.storage_parts_current();
        /* SAFETY: `storage_parts_current` returns columns owned by this table; the
        free-list head and frame arena are initialized together and all slot
        mutations stay within the table's recorded route capacity. */
        unsafe { parts.pop_free_slot() }
    }

    #[inline]
    pub(super) fn push_free_slot(&self, idx: usize) {
        let parts = self.storage_parts_current();
        /* SAFETY: the caller only returns slots previously owned by this table's
        frame arena, and the free-list head belongs to the same column bundle. */
        unsafe { parts.push_free_slot(idx) }
    }

    unsafe fn migrate_to(&self, dst_parts: RouteTableStorageParts, shape: RouteTableStorageShape) {
        if shape.lane_slots < self.lane_slots() {
            crate::invariant();
        }
        let mut idx = 0usize;
        while idx < shape.route_slots {
            let next = if idx + 1 < shape.route_slots {
                (idx + 1) as u16
            } else {
                Self::NO_FRAME
            };
            /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
            unsafe {
                dst_parts.frames.add(idx).write(RouteFrame::free(next));
            }
            idx += 1;
        }
        let mut lane_idx = 0usize;
        while lane_idx < shape.lane_slots {
            /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
            unsafe {
                dst_parts.lane_heads.add(lane_idx).write(Self::NO_FRAME);
            }
            lane_idx += 1;
        }
        /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
        unsafe {
            dst_parts.free_head.write(if shape.route_slots == 0 {
                Self::NO_FRAME
            } else {
                0
            });
        }
        let mut hint_idx = 0usize;
        while hint_idx < self.lane_slots() {
            /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
            unsafe {
                dst_parts
                    .pending_frame_hint_masks
                    .add(hint_idx)
                    .write(*self.pending_frame_hint_masks_ptr().add(hint_idx));
            }
            hint_idx += 1;
        }
        while hint_idx < shape.lane_slots {
            /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
            unsafe {
                dst_parts
                    .pending_frame_hint_masks
                    .add(hint_idx)
                    .write(FrameLabelMask::EMPTY);
            }
            hint_idx += 1;
        }
        let mut waiter_idx = 0usize;
        let waiter_count = checked_mul_usize(shape.lane_slots, MAX_TRACKED_ROLES);
        let src_waiter_count = checked_mul_usize(self.lane_slots(), MAX_TRACKED_ROLES);
        while waiter_idx < src_waiter_count {
            /* SAFETY: the caller supplies exclusive uninitialized storage and this initializer writes all exposed fields before return. */
            unsafe {
                let src_waiter = &mut *self.waiters_ptr().add(waiter_idx);
                if let Some(waker) = src_waiter.take() {
                    WaiterSlot::init_owned(dst_parts.waiters.add(waiter_idx), waker);
                } else {
                    WaiterSlot::init_empty(dst_parts.waiters.add(waiter_idx));
                }
            }
            waiter_idx += 1;
        }
        while waiter_idx < waiter_count {
            /* SAFETY: the caller supplies exclusive uninitialized storage and this initializer writes all exposed fields before return. */
            unsafe {
                WaiterSlot::init_empty(dst_parts.waiters.add(waiter_idx));
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
                let dst_idx = /* SAFETY: migration owns the destination column bundle and pops each destination frame at most once. */ unsafe { dst_parts.pop_free_slot() }
                    .expect("invariant");
                let mut moved = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { *self.frames_ptr().add(src_idx) };
                moved.next = Self::NO_FRAME;
                /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
                unsafe {
                    dst_parts.frames.add(dst_idx).write(moved);
                }
                if prev_new == Self::NO_FRAME {
                    /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
                    unsafe {
                        *dst_parts.lane_heads.add(lane_idx) = dst_idx as u16;
                    }
                } else {
                    /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
                    unsafe {
                        (*dst_parts.frames.add(prev_new as usize)).next = dst_idx as u16;
                    }
                }
                prev_new = dst_idx as u16;
                current = next;
            }
            lane_idx += 1;
        }
        if self.lane_base != shape.lane_base {
            crate::invariant();
        }
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
            self.bind_storage(RouteTableStorageBinding {
                parts,
                shape: RouteTableStorageShape {
                    route_slots,
                    lane_base,
                    lane_slots,
                },
                reclaim_delta,
            });
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
                parts,
                RouteTableStorageShape {
                    route_slots,
                    lane_base,
                    lane_slots,
                },
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
            self.rebind_storage(RouteTableStorageBinding {
                parts,
                shape: RouteTableStorageShape {
                    route_slots,
                    lane_base,
                    lane_slots,
                },
                reclaim_delta,
            });
        }
    }

    unsafe fn storage_parts(
        storage: *mut u8,
        route_slots: usize,
        lane_slots: usize,
    ) -> RouteTableStorageParts {
        let frames = storage.cast::<RouteFrame>();
        let frames_bytes = checked_mul_usize(route_slots, core::mem::size_of::<RouteFrame>());
        let lane_heads_offset = checked_sub_usize(
            Self::align_up(
                checked_add_usize(storage as usize, frames_bytes),
                core::mem::align_of::<u16>(),
            ),
            storage as usize,
        );
        // SAFETY: `storage` is the caller-provided route-table arena. This
        // owner derives all column pointers from the single layout formula used
        // by `storage_layout`, so each pointer stays within that arena when the
        // caller supplied the advertised layout.
        let lane_heads = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { storage.add(lane_heads_offset) }.cast::<u16>();
        let lane_heads_bytes = checked_mul_usize(lane_slots, core::mem::size_of::<u16>());
        let free_head_offset = checked_sub_usize(
            Self::align_up(
                checked_add_usize(
                    checked_add_usize(storage as usize, lane_heads_offset),
                    lane_heads_bytes,
                ),
                core::mem::align_of::<u16>(),
            ),
            storage as usize,
        );
        // SAFETY: See the lane-head derivation above; this is the next aligned
        // column in the same resident route-table arena.
        let free_head = unsafe { storage.add(free_head_offset) }.cast::<u16>();
        let hint_offset = checked_sub_usize(
            Self::align_up(
                checked_add_usize(
                    checked_add_usize(storage as usize, free_head_offset),
                    core::mem::size_of::<u16>(),
                ),
                core::mem::align_of::<FrameLabelMask>(),
            ),
            storage as usize,
        );
        // SAFETY: The pending-hint column is derived by the same storage layout
        // owner and follows the single free-head slot.
        let pending_frame_hint_masks = unsafe { storage.add(hint_offset) }.cast::<FrameLabelMask>();
        let hint_bytes = checked_mul_usize(lane_slots, core::mem::size_of::<FrameLabelMask>());
        let waiters_offset = checked_sub_usize(
            Self::align_up(
                checked_add_usize(checked_add_usize(storage as usize, hint_offset), hint_bytes),
                core::mem::align_of::<WaiterSlot>(),
            ),
            storage as usize,
        );
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
    pub(super) fn frames_ptr(&self) -> *mut RouteFrame {
        self.raw_frames()
            .map_addr(|addr| addr & !Self::STORAGE_TAG_MASK)
    }

    #[inline]
    pub(super) fn lane_heads_ptr(&self) -> *mut u16 {
        /* SAFETY: the rendezvous table owns initialized slots behind explicit presence state before raw access. */
        unsafe { *self.lane_heads.get() }
    }

    #[inline]
    pub(super) fn free_head_ptr(&self) -> *mut u16 {
        /* SAFETY: the rendezvous table owns initialized slots behind explicit presence state before raw access. */
        unsafe { *self.free_head.get() }
    }

    #[inline]
    pub(super) fn pending_frame_hint_masks_ptr(&self) -> *mut FrameLabelMask {
        self.raw_pending_frame_hint_masks()
    }

    #[inline]
    pub(super) fn waiters_ptr(&self) -> *mut WaiterSlot {
        /* SAFETY: the rendezvous table owns initialized slots behind explicit presence state before raw access. */
        unsafe { *self.waiters.get() }
    }
}
