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
    Cell, FrameLabelMask, MAX_TRACKED_ROLES, PhantomData, RouteFrame, RouteTable,
    RouteTableStorageBinding, RouteTableStorageParts, RouteTableStorageShape, UnsafeCell,
    WaiterSlot,
};
impl RouteTableStorageParts {
    unsafe fn pop_free_slot(&self) -> Option<usize> {
        let head = /* SAFETY: the free-list head belongs to this route-table column bundle. */ unsafe { *self.free_head };
        if head == RouteTable::FRAME_LIST_END {
            return None;
        }
        let idx = head as usize;
        let next = /* SAFETY: `idx` was obtained from this bundle's free list and therefore names a frame slot in this bundle. */ unsafe {
            (*self.frames.add(idx)).next
        };
        /* SAFETY: the frame slot and free-list head are owned by the same column bundle and are updated as one free-list transition. */
        unsafe {
            *self.free_head = next;
            (*self.frames.add(idx)).next = RouteTable::FRAME_LIST_END;
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
    pub(crate) const FRAME_LIST_END: u16 = u16::MAX;

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

    pub(crate) unsafe fn init_empty(dst: *mut Self) {
        /* SAFETY: the caller provides exclusive, writable storage for one
        RouteTable with exact size/alignment; every field is initialized before
        safe exposure. */
        unsafe {
            core::ptr::addr_of_mut!((*dst).frames).write(UnsafeCell::new(core::ptr::null_mut()));
            core::ptr::addr_of_mut!((*dst).route_slots).write(Cell::new(0));
            core::ptr::addr_of_mut!((*dst).lane_base).write(Cell::new(0));
            core::ptr::addr_of_mut!((*dst).lane_slots).write(Cell::new(0));
            core::ptr::addr_of_mut!((*dst).lane_heads)
                .write(UnsafeCell::new(core::ptr::null_mut()));
            core::ptr::addr_of_mut!((*dst).free_head).write(UnsafeCell::new(core::ptr::null_mut()));
            core::ptr::addr_of_mut!((*dst).pending_frame_hint_masks)
                .write(UnsafeCell::new(core::ptr::null_mut()));
            core::ptr::addr_of_mut!((*dst).waiters).write(UnsafeCell::new(core::ptr::null_mut()));
            core::ptr::addr_of_mut!((*dst)._no_send_sync).write(PhantomData);
        }
    }

    #[inline]
    pub(crate) const fn route_slots(&self) -> usize {
        self.route_slots.get()
    }

    #[inline]
    pub(crate) const fn lane_slots(&self) -> usize {
        self.lane_slots.get() as usize
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

    #[inline]
    fn raw_frames(&self) -> *mut RouteFrame {
        /* SAFETY: `frames` is written during route-table binding/rebinding and
        then remains the table-owned frame column until the next owner-local
        storage transition; slot helpers bound indices by `route_slots`. */
        unsafe { *self.frames.get() }
    }

    #[inline]
    fn raw_pending_frame_hint_masks(&self) -> *mut FrameLabelMask {
        /* SAFETY: pending hint masks are initialized for every bound lane slot
        before safe route-table methods can query the column. */
        unsafe { *self.pending_frame_hint_masks.get() }
    }

    unsafe fn bind_storage(&self, binding: RouteTableStorageBinding) {
        let RouteTableStorageBinding { parts, shape } = binding;
        let mut idx = 0usize;
        while idx < shape.route_slots {
            let next = if idx + 1 < shape.route_slots {
                (idx + 1) as u16
            } else {
                Self::FRAME_LIST_END
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
                // SAFETY: `bind_storage` has exclusive publication of the
                // replacement lane-head column; `lane_idx` is within
                // `shape.lane_slots`.
                parts.lane_heads.add(lane_idx).write(Self::FRAME_LIST_END);
            }
            lane_idx += 1;
        }
        unsafe {
            // SAFETY: `free_head` is the single u16 free-list head owned by this
            // route table storage layout.
            parts.free_head.write(if shape.route_slots == 0 {
                Self::FRAME_LIST_END
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
        unsafe {
            self.frames.get().write(parts.frames);
            self.lane_heads.get().write(parts.lane_heads);
            self.free_head.get().write(parts.free_head);
            self.pending_frame_hint_masks
                .get()
                .write(parts.pending_frame_hint_masks);
            self.waiters.get().write(parts.waiters);
        }
        self.route_slots.set(shape.route_slots);
        self.lane_base.set(shape.lane_base);
        self.lane_slots.set(shape.lane_slots as u16);
    }

    unsafe fn rebind_storage(&self, binding: RouteTableStorageBinding) {
        /* SAFETY: migration initialized every replacement column described by
        this binding before the owner publishes their pointers. */
        unsafe {
            self.frames.get().write(binding.parts.frames);
            self.lane_heads.get().write(binding.parts.lane_heads);
            self.free_head.get().write(binding.parts.free_head);
            self.pending_frame_hint_masks
                .get()
                .write(binding.parts.pending_frame_hint_masks);
            self.waiters.get().write(binding.parts.waiters);
        }
        self.route_slots.set(binding.shape.route_slots);
        self.lane_base.set(binding.shape.lane_base);
        self.lane_slots.set(binding.shape.lane_slots as u16);
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
                Self::FRAME_LIST_END
            };
            /* SAFETY: `migrate_to` owns the destination frame column in
            `dst_parts`; `idx` is bounded by `shape.route_slots`, and each
            destination frame is initialized before the table is rebound. */
            unsafe {
                dst_parts.frames.add(idx).write(RouteFrame::free(next));
            }
            idx += 1;
        }
        let mut lane_idx = 0usize;
        while lane_idx < shape.lane_slots {
            /* SAFETY: `dst_parts.lane_heads` has `shape.lane_slots` initialized
            entries reserved for the replacement table, and `lane_idx` is inside
            that destination column. */
            unsafe {
                dst_parts
                    .lane_heads
                    .add(lane_idx)
                    .write(Self::FRAME_LIST_END);
            }
            lane_idx += 1;
        }
        /* SAFETY: `dst_parts.free_head` is the replacement table's single
        free-list head and is written before any destination frame is popped. */
        unsafe {
            dst_parts.free_head.write(if shape.route_slots == 0 {
                Self::FRAME_LIST_END
            } else {
                0
            });
        }
        let mut hint_idx = 0usize;
        while hint_idx < self.lane_slots() {
            /* SAFETY: both pending-hint columns are initialized for their lane
            counts; `hint_idx` is within the old table and the replacement has
            at least that many lanes. */
            unsafe {
                dst_parts
                    .pending_frame_hint_masks
                    .add(hint_idx)
                    .write(*self.pending_frame_hint_masks_ptr().add(hint_idx));
            }
            hint_idx += 1;
        }
        while hint_idx < shape.lane_slots {
            /* SAFETY: `hint_idx` is within the replacement lane count but past
            the old table's lane count, so this slot is initialized to the empty
            hint mask before publication. */
            unsafe {
                dst_parts
                    .pending_frame_hint_masks
                    .add(hint_idx)
                    .write(FrameLabelMask::EMPTY);
            }
            hint_idx += 1;
        }
        let waiter_count = checked_mul_usize(shape.lane_slots, MAX_TRACKED_ROLES);
        let src_waiter_count = checked_mul_usize(self.lane_slots(), MAX_TRACKED_ROLES);
        let mut waiter_idx = 0usize;
        while waiter_idx < waiter_count {
            /* SAFETY: every replacement waiter slot is initialized before any
            source waiter ownership is transferred into it. */
            unsafe {
                WaiterSlot::init_empty(dst_parts.waiters.add(waiter_idx));
            }
            waiter_idx += 1;
        }
        waiter_idx = 0;
        while waiter_idx < src_waiter_count {
            /* SAFETY: both indexes are inside initialized waiter columns.
            `take` transfers the Waker without invoking its clone/drop vtable,
            so migration has no external reentry point before publication. */
            unsafe {
                if let Some(waker) = (*self.waiters_ptr().add(waiter_idx)).take() {
                    (*dst_parts.waiters.add(waiter_idx)).set_owned(waker);
                }
            }
            waiter_idx += 1;
        }
        if self.route_slots.get() == 0 {
            return;
        }
        let mut lane_idx = 0usize;
        while lane_idx < self.lane_slots() {
            let mut current = /* SAFETY: `lane_idx` is bounded by the current
            table's `lane_slots`; the lane-head column is initialized before
            migration begins. */
                unsafe { *self.lane_heads_ptr().add(lane_idx) };
            let mut prev_new = Self::FRAME_LIST_END;
            while current != Self::FRAME_LIST_END {
                let src_idx = current as usize;
                let next = /* SAFETY: `src_idx` comes from the current table's
                route-frame linked list for `lane_idx`, so it names an
                initialized source frame. */
                    unsafe { (*self.frames_ptr().add(src_idx)).next };
                let dst_idx = crate::invariant_some(
                    /* SAFETY: migration owns the destination column bundle and pops each destination frame at most once. */
                    unsafe { dst_parts.pop_free_slot() },
                );
                let mut moved = /* SAFETY: `src_idx` is the current source frame
                being migrated; the source table remains bound and initialized
                for the whole copy phase. */
                    unsafe { *self.frames_ptr().add(src_idx) };
                moved.next = Self::FRAME_LIST_END;
                /* SAFETY: `dst_idx` was popped from the replacement free list,
                so this destination frame slot is owned by migration and not
                linked elsewhere yet. */
                unsafe {
                    dst_parts.frames.add(dst_idx).write(moved);
                }
                if prev_new == Self::FRAME_LIST_END {
                    /* SAFETY: `lane_idx` is inside the replacement lane-head
                    column; this publishes the first migrated frame for that
                    lane into the replacement list. */
                    unsafe {
                        *dst_parts.lane_heads.add(lane_idx) = dst_idx as u16;
                    }
                } else {
                    /* SAFETY: `prev_new` is the previous frame slot popped from
                    the same replacement free list and linked in this migration
                    pass. */
                    unsafe {
                        (*dst_parts.frames.add(prev_new as usize)).next = dst_idx as u16;
                    }
                }
                prev_new = dst_idx as u16;
                current = next;
            }
            lane_idx += 1;
        }
        if self.lane_base.get() != shape.lane_base {
            crate::invariant();
        }
    }

    pub(crate) unsafe fn bind_from_storage_with_layout(
        &self,
        storage: *mut u8,
        route_slots: usize,
        lane_base: u32,
        lane_slots: usize,
    ) {
        let parts = /* SAFETY: `storage` is the fresh route-table arena leased
        by rendezvous capacity growth for `route_slots` and `lane_slots`. */
            unsafe { Self::storage_parts(storage, route_slots, lane_slots) };
        /* SAFETY: the rendezvous storage owner serializes route-table binding;
        `bind_storage` initializes all columns before publishing their pointers. */
        unsafe {
            self.bind_storage(RouteTableStorageBinding {
                parts,
                shape: RouteTableStorageShape {
                    route_slots,
                    lane_base,
                    lane_slots,
                },
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
        let parts = /* SAFETY: `storage` is the replacement route-table arena;
        migration derives columns with the same layout used for capacity
        accounting. */
            unsafe { Self::storage_parts(storage, route_slots, lane_slots) };
        /* SAFETY: `migrate_to` copies initialized source route frames and
        waiters into the replacement column bundle without rebinding this table
        during the copy phase. */
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
        &self,
        storage: *mut u8,
        route_slots: usize,
        lane_base: u32,
        lane_slots: usize,
    ) {
        let parts = /* SAFETY: `storage` is the already staged replacement arena
        whose route-frame, lane-head, hint, and waiter columns were initialized
        by `migrate_from_storage`. */
            unsafe { Self::storage_parts(storage, route_slots, lane_slots) };
        /* SAFETY: the rendezvous storage owner serializes rebinding and this
        swaps column pointers only to the fully staged replacement bundle. */
        unsafe {
            self.rebind_storage(RouteTableStorageBinding {
                parts,
                shape: RouteTableStorageShape {
                    route_slots,
                    lane_base,
                    lane_slots,
                },
            });
        }
    }

    pub(crate) unsafe fn relocate_storage(&self, storage: *mut u8) {
        /* SAFETY: the rendezvous owner exclusively moved the complete route
        sidecar without changing shape; rebinding mutates only its sole roots. */
        unsafe {
            self.rebind_from_storage(
                storage,
                self.route_slots(),
                self.lane_base.get(),
                self.lane_slots(),
            );
        }
    }

    pub(crate) fn clear_storage(&self) {
        let waiter_count = checked_mul_usize(self.lane_slots(), MAX_TRACKED_ROLES);
        let mut waiter_idx = 0usize;
        while waiter_idx < waiter_count {
            /* SAFETY: the scan is bounded by the currently published route
            waiter column. Retiring storage with a live Waker would leak its
            ownership and permit the reclaimed bytes to overwrite it. */
            if unsafe { !(*self.waiters_ptr().add(waiter_idx)).is_empty() } {
                crate::invariant();
            }
            waiter_idx += 1;
        }
        unsafe {
            /* SAFETY: the caller releases route storage only after all live
            endpoint route budgets have reached zero and the scan above proved
            no Waker ownership remains. Publishing null columns and zero shape
            makes the unbound state visible before the sidecar can be reused. */
            self.frames.get().write(core::ptr::null_mut());
            self.lane_heads.get().write(core::ptr::null_mut());
            self.free_head.get().write(core::ptr::null_mut());
            self.pending_frame_hint_masks
                .get()
                .write(core::ptr::null_mut());
            self.waiters.get().write(core::ptr::null_mut());
        }
        self.route_slots.set(0);
        self.lane_base.set(0);
        self.lane_slots.set(0);
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
        let hint_bytes = checked_mul_usize(lane_slots, core::mem::size_of::<FrameLabelMask>());
        let waiters_offset = checked_sub_usize(
            Self::align_up(
                checked_add_usize(checked_add_usize(storage as usize, hint_offset), hint_bytes),
                core::mem::align_of::<WaiterSlot>(),
            ),
            storage as usize,
        );
        /* SAFETY: `storage_parts` owns only pointer derivation for the
        caller-provided route-table arena. All offsets are computed by this
        layout formula, aligned for their typed columns, and no references are
        created until bounded route-table methods index the initialized slots. */
        let (lane_heads, free_head, pending_frame_hint_masks, waiters) = unsafe {
            (
                storage.add(lane_heads_offset).cast::<u16>(),
                storage.add(free_head_offset).cast::<u16>(),
                storage.add(hint_offset).cast::<FrameLabelMask>(),
                storage.add(waiters_offset).cast::<WaiterSlot>(),
            )
        };
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
    }

    #[inline]
    pub(super) fn lane_heads_ptr(&self) -> *mut u16 {
        /* SAFETY: `lane_heads` is the initialized route-list head column for
        the currently bound lane range. */
        unsafe { *self.lane_heads.get() }
    }

    #[inline]
    pub(super) fn free_head_ptr(&self) -> *mut u16 {
        /* SAFETY: `free_head` is the single initialized frame free-list head
        owned by this route table. */
        unsafe { *self.free_head.get() }
    }

    #[inline]
    pub(super) fn pending_frame_hint_masks_ptr(&self) -> *mut FrameLabelMask {
        self.raw_pending_frame_hint_masks()
    }

    #[inline]
    pub(super) fn waiters_ptr(&self) -> *mut WaiterSlot {
        /* SAFETY: `RouteTable` owns `waiters`, initialized with `lane_slots *
        MAX_TRACKED_ROLES` entries during bind or migration. */
        unsafe { *self.waiters.get() }
    }
}
