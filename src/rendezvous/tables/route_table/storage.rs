//! # Unsafe Owner Contract
//!
//! `RouteTable` owns a caller-provided resident storage region split into
//! route-frame slots, lane-head indices, and one free-list head. Binding and
//! migration initialize every region before safe table methods can observe it.
//! The rendezvous owner keeps the backing storage pinned for the table lifetime
//! and serializes all storage transitions.

use super::super::{checked_add_usize, checked_mul_usize, checked_sub_usize};
use super::{
    Cell, PhantomData, RouteFrame, RouteTable, RouteTableStorageBinding, RouteTableStorageParts,
    RouteTableStorageShape, UnsafeCell,
};

impl RouteTableStorageParts {
    unsafe fn pop_free_slot(&self) -> Option<usize> {
        let head = /* SAFETY: the free-list head belongs to this route-table column bundle. */ unsafe {
            *self.free_head
        };
        if head == RouteTable::FRAME_LIST_END {
            return None;
        }
        let idx = head as usize;
        let next = /* SAFETY: `idx` came from this bundle's free list. */ unsafe {
            (*self.frames.add(idx)).next
        };
        /* SAFETY: the frame and free-list head belong to the same unpublished
        or currently bound column bundle. */
        unsafe {
            *self.free_head = next;
            (*self.frames.add(idx)).next = RouteTable::FRAME_LIST_END;
        }
        Some(idx)
    }

    unsafe fn push_free_slot(&self, idx: usize) {
        let next = /* SAFETY: the free-list head belongs to this route-table column bundle. */ unsafe {
            *self.free_head
        };
        /* SAFETY: callers return only frame slots owned by this column bundle. */
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
        RouteTable; every field is initialized before safe exposure. */
        unsafe {
            core::ptr::addr_of_mut!((*dst).frames).write(UnsafeCell::new(core::ptr::null_mut()));
            core::ptr::addr_of_mut!((*dst).route_slots).write(Cell::new(0));
            core::ptr::addr_of_mut!((*dst).lane_base).write(Cell::new(0));
            core::ptr::addr_of_mut!((*dst).lane_slots).write(Cell::new(0));
            core::ptr::addr_of_mut!((*dst).lane_heads)
                .write(UnsafeCell::new(core::ptr::null_mut()));
            core::ptr::addr_of_mut!((*dst).free_head).write(UnsafeCell::new(core::ptr::null_mut()));
            core::ptr::addr_of_mut!((*dst)._no_send_sync).write(PhantomData);
        }
    }

    #[inline]
    pub(crate) const fn route_slots(&self) -> usize {
        self.route_slots.get() as usize
    }

    #[inline]
    pub(crate) const fn lane_slots(&self) -> usize {
        self.lane_slots.get() as usize
    }

    #[inline]
    pub(crate) const fn storage_align() -> usize {
        let frame_align = core::mem::align_of::<RouteFrame>();
        let head_align = core::mem::align_of::<u16>();
        if frame_align > head_align {
            frame_align
        } else {
            head_align
        }
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
        checked_add_usize(free_head_offset, core::mem::size_of::<u16>())
    }

    #[inline]
    fn raw_frames(&self) -> *mut RouteFrame {
        /* SAFETY: `frames` is published only after the complete column is
        initialized and remains owner-bound until the next storage transition. */
        unsafe { *self.frames.get() }
    }

    fn frame_index_is_active(&self, target: usize) -> bool {
        if target >= self.route_slots() {
            crate::invariant();
        }
        let mut lane_idx = 0usize;
        while lane_idx < self.lane_slots() {
            let mut current = self.lane_head(lane_idx);
            let mut visited = 0usize;
            while current != Self::FRAME_LIST_END {
                let idx = current as usize;
                if idx == target {
                    return true;
                }
                current = self.frame_ref(idx).next;
                visited += 1;
                if visited > self.route_slots() {
                    crate::invariant();
                }
            }
            lane_idx += 1;
        }
        false
    }

    fn active_frame_count(&self) -> usize {
        let mut count = 0usize;
        let mut lane_idx = 0usize;
        while lane_idx < self.lane_slots() {
            let mut current = self.lane_head(lane_idx);
            while current != Self::FRAME_LIST_END {
                count = crate::invariant_some(count.checked_add(1));
                if count > self.route_slots() {
                    crate::invariant();
                }
                current = self.frame_ref(current as usize).next;
            }
            lane_idx += 1;
        }
        count
    }

    fn redirect_active_frame_index(&self, source: usize, destination: usize) {
        let source = crate::invariant_ok(u16::try_from(source));
        let destination = crate::invariant_ok(u16::try_from(destination));
        let mut lane_idx = 0usize;
        while lane_idx < self.lane_slots() {
            if self.lane_head(lane_idx) == source {
                self.set_lane_head(lane_idx, destination);
            }
            let mut current = self.lane_head(lane_idx);
            let mut visited = 0usize;
            while current != Self::FRAME_LIST_END {
                if current == source {
                    crate::invariant();
                }
                let idx = current as usize;
                let next = self.frame_ref(idx).next;
                if next == source {
                    self.with_frame_mut(idx, |frame| frame.next = destination);
                    current = destination;
                } else {
                    current = next;
                }
                visited += 1;
                if visited > self.route_slots() {
                    crate::invariant();
                }
            }
            lane_idx += 1;
        }
    }

    fn compact_active_frames(&self) -> usize {
        let active_count = self.active_frame_count();
        let mut destination = 0usize;
        while destination < active_count {
            if self.frame_index_is_active(destination) {
                destination += 1;
                continue;
            }
            let mut source = destination + 1;
            while source < self.route_slots() && !self.frame_index_is_active(source) {
                source += 1;
            }
            if source == self.route_slots() {
                crate::invariant();
            }
            let moved = *self.frame_ref(source);
            self.with_frame_mut(destination, |frame| *frame = moved);
            self.redirect_active_frame_index(source, destination);
            self.with_frame_mut(source, |frame| {
                *frame = RouteFrame::free(Self::FRAME_LIST_END)
            });
            destination += 1;
        }
        active_count
    }

    pub(crate) unsafe fn shrink_storage_in_place(
        &self,
        storage: *mut u8,
        route_slots: usize,
        lane_slots: usize,
    ) {
        if route_slots == 0
            || route_slots > self.route_slots()
            || lane_slots == 0
            || lane_slots > self.lane_slots()
            || route_slots >= Self::FRAME_LIST_END as usize
        {
            crate::invariant();
        }
        let mut removed_lane = lane_slots;
        while removed_lane < self.lane_slots() {
            if self.lane_head(removed_lane) != Self::FRAME_LIST_END {
                crate::invariant();
            }
            removed_lane += 1;
        }
        let active_count = self.compact_active_frames();
        if active_count > route_slots {
            crate::invariant();
        }
        let replacement = /* SAFETY: `storage` is this table's current route
        sidecar root; the smaller layout remains inside that allocation. */ unsafe {
            Self::storage_parts(storage, route_slots, lane_slots)
        };
        /* SAFETY: both lane-head ranges lie in the same current sidecar.
        `ptr::copy` permits overlap when the smaller frame column moves the
        lane-head column toward the front. */
        unsafe {
            core::ptr::copy(self.lane_heads_ptr(), replacement.lane_heads, lane_slots);
        }
        let mut idx = active_count;
        while idx < route_slots {
            let next = if idx + 1 < route_slots {
                (idx + 1) as u16
            } else {
                Self::FRAME_LIST_END
            };
            /* SAFETY: compacted live frames occupy `0..active_count`; this
            initializes each remaining target frame as the replacement free list. */
            unsafe {
                replacement.frames.add(idx).write(RouteFrame::free(next));
            }
            idx += 1;
        }
        /* SAFETY: the replacement free-list root is inside the smaller layout
        and every target frame/lane-head slot is initialized. */
        unsafe {
            replacement.free_head.write(if active_count == route_slots {
                Self::FRAME_LIST_END
            } else {
                active_count as u16
            });
            self.rebind_storage(RouteTableStorageBinding {
                parts: replacement,
                shape: RouteTableStorageShape {
                    route_slots,
                    lane_base: self.lane_base.get(),
                    lane_slots,
                },
            });
        }
    }

    unsafe fn bind_storage(&self, binding: RouteTableStorageBinding) {
        let RouteTableStorageBinding { parts, shape } = binding;
        if shape.route_slots >= Self::FRAME_LIST_END as usize
            || shape.lane_slots > usize::from(u16::MAX)
        {
            crate::invariant();
        }
        let mut idx = 0usize;
        while idx < shape.route_slots {
            let next = if idx + 1 < shape.route_slots {
                (idx + 1) as u16
            } else {
                Self::FRAME_LIST_END
            };
            /* SAFETY: `idx` is inside the owner-exclusive unpublished frame
            column, so no initialized frame alias exists yet. */
            unsafe {
                parts.frames.add(idx).write(RouteFrame::free(next));
            }
            idx += 1;
        }
        let mut lane_idx = 0usize;
        while lane_idx < shape.lane_slots {
            /* SAFETY: `lane_idx` is inside the owner-exclusive unpublished
            lane-head column, so no initialized head alias exists yet. */
            unsafe {
                parts.lane_heads.add(lane_idx).write(Self::FRAME_LIST_END);
            }
            lane_idx += 1;
        }
        /* SAFETY: `free_head` is the owner bundle's single initialized list
        root; the unpublished columns have no external aliases. */
        unsafe {
            parts.free_head.write(if shape.route_slots == 0 {
                Self::FRAME_LIST_END
            } else {
                0
            });
            self.frames.get().write(parts.frames);
            self.lane_heads.get().write(parts.lane_heads);
            self.free_head.get().write(parts.free_head);
        }
        self.route_slots.set(shape.route_slots as u16);
        self.lane_base.set(shape.lane_base);
        self.lane_slots.set(shape.lane_slots as u16);
    }

    unsafe fn rebind_storage(&self, binding: RouteTableStorageBinding) {
        /* SAFETY: migration fully initialized this replacement binding before
        the owner publishes its pointers. */
        unsafe {
            self.frames.get().write(binding.parts.frames);
            self.lane_heads.get().write(binding.parts.lane_heads);
            self.free_head.get().write(binding.parts.free_head);
        }
        self.route_slots.set(binding.shape.route_slots as u16);
        self.lane_base.set(binding.shape.lane_base);
        self.lane_slots.set(binding.shape.lane_slots as u16);
    }

    #[inline]
    fn storage_parts_current(&self) -> RouteTableStorageParts {
        RouteTableStorageParts {
            frames: self.frames_ptr(),
            lane_heads: self.lane_heads_ptr(),
            free_head: self.free_head_ptr(),
        }
    }

    #[inline]
    pub(super) fn pop_free_slot(&self) -> Option<usize> {
        let parts = self.storage_parts_current();
        /* SAFETY: all columns belong to this table's current binding. */
        unsafe { parts.pop_free_slot() }
    }

    #[inline]
    pub(super) fn push_free_slot(&self, idx: usize) {
        if idx >= self.route_slots() {
            crate::invariant();
        }
        let parts = self.storage_parts_current();
        /* SAFETY: `idx` is a frame slot owned by the current binding. */
        unsafe { parts.push_free_slot(idx) }
    }

    unsafe fn migrate_to(&self, dst_parts: RouteTableStorageParts, shape: RouteTableStorageShape) {
        if shape.route_slots < self.route_slots()
            || shape.lane_slots < self.lane_slots()
            || shape.route_slots >= Self::FRAME_LIST_END as usize
            || shape.lane_slots > usize::from(u16::MAX)
        {
            crate::invariant();
        }
        let mut idx = 0usize;
        while idx < shape.route_slots {
            let next = if idx + 1 < shape.route_slots {
                (idx + 1) as u16
            } else {
                Self::FRAME_LIST_END
            };
            /* SAFETY: `idx` is inside the unpublished replacement frame column. */
            unsafe {
                dst_parts.frames.add(idx).write(RouteFrame::free(next));
            }
            idx += 1;
        }
        let mut lane_idx = 0usize;
        while lane_idx < shape.lane_slots {
            /* SAFETY: `lane_idx` is inside the replacement lane-head column. */
            unsafe {
                dst_parts
                    .lane_heads
                    .add(lane_idx)
                    .write(Self::FRAME_LIST_END);
            }
            lane_idx += 1;
        }
        /* SAFETY: `free_head` belongs to the unpublished replacement bundle. */
        unsafe {
            dst_parts.free_head.write(if shape.route_slots == 0 {
                Self::FRAME_LIST_END
            } else {
                0
            });
        }
        let mut lane_idx = 0usize;
        while lane_idx < self.lane_slots() {
            let mut current = /* SAFETY: `lane_idx` is inside the current lane column. */ unsafe {
                *self.lane_heads_ptr().add(lane_idx)
            };
            let mut prev_new = Self::FRAME_LIST_END;
            while current != Self::FRAME_LIST_END {
                let src_idx = current as usize;
                let next = /* SAFETY: the current list owns `src_idx`. */ unsafe {
                    (*self.frames_ptr().add(src_idx)).next
                };
                let dst_idx = crate::invariant_some(
                    /* SAFETY: migration owns the replacement free list. */
                    unsafe { dst_parts.pop_free_slot() },
                );
                let mut moved = /* SAFETY: `src_idx` names an initialized source frame. */ unsafe {
                    *self.frames_ptr().add(src_idx)
                };
                moved.next = Self::FRAME_LIST_END;
                /* SAFETY: `dst_idx` was removed from the replacement free list. */
                unsafe {
                    dst_parts.frames.add(dst_idx).write(moved);
                    if prev_new == Self::FRAME_LIST_END {
                        *dst_parts.lane_heads.add(lane_idx) = dst_idx as u16;
                    } else {
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
        let parts = /* SAFETY: the caller leased this route-table arena with the matching layout. */ unsafe {
            Self::storage_parts(storage, route_slots, lane_slots)
        };
        /* SAFETY: binding initializes every destination slot before publication. */
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
        let parts = /* SAFETY: the caller leased this unpublished replacement arena. */ unsafe {
            Self::storage_parts(storage, route_slots, lane_slots)
        };
        /* SAFETY: migration copies only initialized route frames into the
        unpublished replacement bundle. */
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
        let parts = /* SAFETY: migration already staged this replacement arena. */ unsafe {
            Self::storage_parts(storage, route_slots, lane_slots)
        };
        /* SAFETY: the replacement columns are fully initialized. */
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
        /* SAFETY: the owner exclusively moved the complete initialized sidecar
        without changing shape; no table-column alias is used during rebinding. */
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
        /* SAFETY: the owner retires route storage only after no endpoint budget
        owns it and no table-column alias remains; null roots precede reuse. */
        unsafe {
            self.frames.get().write(core::ptr::null_mut());
            self.lane_heads.get().write(core::ptr::null_mut());
            self.free_head.get().write(core::ptr::null_mut());
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
        /* SAFETY: both offsets come from the same checked storage layout and
        are aligned for their columns. */
        let (lane_heads, free_head) = unsafe {
            (
                storage.add(lane_heads_offset).cast::<u16>(),
                storage.add(free_head_offset).cast::<u16>(),
            )
        };
        RouteTableStorageParts {
            frames,
            lane_heads,
            free_head,
        }
    }

    #[inline]
    pub(super) fn frames_ptr(&self) -> *mut RouteFrame {
        self.raw_frames()
    }

    #[inline]
    pub(super) fn lane_heads_ptr(&self) -> *mut u16 {
        /* SAFETY: `lane_heads` is the current initialized lane-head column. */
        unsafe { *self.lane_heads.get() }
    }

    #[inline]
    pub(super) fn free_head_ptr(&self) -> *mut u16 {
        /* SAFETY: `free_head` is the current initialized free-list root. */
        unsafe { *self.free_head.get() }
    }
}
