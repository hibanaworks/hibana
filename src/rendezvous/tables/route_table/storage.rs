//! # Unsafe Owner Contract
//!
//! `RouteTable` owns a caller-provided resident storage region split into
//! route-frame slots, one active-list head, and one free-list head. Binding and
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
        let head = /* SAFETY: the free-list head belongs to this route-table bundle. */ unsafe {
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
        or currently bound bundle. */
        unsafe {
            *self.free_head = next;
            (*self.frames.add(idx)).next = RouteTable::FRAME_LIST_END;
        }
        Some(idx)
    }

    unsafe fn push_free_slot(&self, idx: usize) {
        let next = /* SAFETY: the free-list head belongs to this route-table bundle. */ unsafe {
            *self.free_head
        };
        /* SAFETY: callers return only frame slots owned by this bundle. */
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
            core::ptr::addr_of_mut!((*dst).active_head)
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
    pub(crate) const fn storage_bytes(route_slots: usize) -> usize {
        let frames_bytes = checked_mul_usize(route_slots, core::mem::size_of::<RouteFrame>());
        let active_head_offset = Self::align_up(frames_bytes, core::mem::align_of::<u16>());
        checked_add_usize(
            checked_add_usize(active_head_offset, core::mem::size_of::<u16>()),
            core::mem::size_of::<u16>(),
        )
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
        let mut current = self.active_head();
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
        false
    }

    fn active_frame_count(&self) -> usize {
        let mut count = 0usize;
        let mut current = self.active_head();
        while current != Self::FRAME_LIST_END {
            count = crate::invariant_some(count.checked_add(1));
            if count > self.route_slots() {
                crate::invariant();
            }
            current = self.frame_ref(current as usize).next;
        }
        count
    }

    fn redirect_active_frame_index(&self, source: usize, destination: usize) {
        let source = crate::invariant_ok(u16::try_from(source));
        let destination = crate::invariant_ok(u16::try_from(destination));
        if self.active_head() == source {
            self.set_active_head(destination);
        }
        let mut current = self.active_head();
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

    pub(crate) unsafe fn shrink_storage_in_place(&self, storage: *mut u8, route_slots: usize) {
        if route_slots == 0
            || route_slots > self.route_slots()
            || route_slots >= Self::FRAME_LIST_END as usize
        {
            crate::invariant();
        }
        let active_count = self.compact_active_frames();
        if active_count > route_slots {
            crate::invariant();
        }
        let active_head = self.active_head();
        let replacement = /* SAFETY: `storage` is this table's current route
        sidecar root; the smaller layout remains inside that allocation. */ unsafe {
            Self::storage_parts(storage, route_slots)
        };
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
        /* SAFETY: both list roots are inside the smaller layout and every
        target frame is initialized. */
        unsafe {
            replacement.active_head.write(active_head);
            replacement.free_head.write(if active_count == route_slots {
                Self::FRAME_LIST_END
            } else {
                active_count as u16
            });
            self.rebind_storage(RouteTableStorageBinding {
                parts: replacement,
                shape: RouteTableStorageShape { route_slots },
            });
        }
    }

    unsafe fn bind_storage(&self, binding: RouteTableStorageBinding) {
        let RouteTableStorageBinding { parts, shape } = binding;
        if shape.route_slots >= Self::FRAME_LIST_END as usize {
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
        /* SAFETY: both list roots belong to the owner-exclusive unpublished
        bundle and every frame has been initialized. */
        unsafe {
            parts.active_head.write(Self::FRAME_LIST_END);
            parts.free_head.write(if shape.route_slots == 0 {
                Self::FRAME_LIST_END
            } else {
                0
            });
            self.frames.get().write(parts.frames);
            self.active_head.get().write(parts.active_head);
            self.free_head.get().write(parts.free_head);
        }
        self.route_slots.set(shape.route_slots as u16);
    }

    unsafe fn rebind_storage(&self, binding: RouteTableStorageBinding) {
        /* SAFETY: migration fully initialized this replacement binding before
        the owner publishes its pointers. */
        unsafe {
            self.frames.get().write(binding.parts.frames);
            self.active_head.get().write(binding.parts.active_head);
            self.free_head.get().write(binding.parts.free_head);
        }
        self.route_slots.set(crate::invariant_ok(u16::try_from(
            binding.shape.route_slots,
        )));
    }

    #[inline]
    fn storage_parts_current(&self) -> RouteTableStorageParts {
        RouteTableStorageParts {
            frames: self.frames_ptr(),
            active_head: self.active_head_ptr(),
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
            || shape.route_slots >= Self::FRAME_LIST_END as usize
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
        /* SAFETY: both roots belong to the unpublished replacement bundle. */
        unsafe {
            dst_parts.active_head.write(Self::FRAME_LIST_END);
            dst_parts.free_head.write(if shape.route_slots == 0 {
                Self::FRAME_LIST_END
            } else {
                0
            });
        }
        let mut current = self.active_head();
        let mut previous = Self::FRAME_LIST_END;
        while current != Self::FRAME_LIST_END {
            let src_idx = current as usize;
            let next = self.frame_ref(src_idx).next;
            let dst_idx = crate::invariant_some(
                /* SAFETY: migration owns the replacement free list. */
                unsafe { dst_parts.pop_free_slot() },
            );
            let mut moved = *self.frame_ref(src_idx);
            moved.next = Self::FRAME_LIST_END;
            /* SAFETY: `dst_idx` was removed from the replacement free list. */
            unsafe {
                dst_parts.frames.add(dst_idx).write(moved);
                if previous == Self::FRAME_LIST_END {
                    *dst_parts.active_head = dst_idx as u16;
                } else {
                    (*dst_parts.frames.add(previous as usize)).next = dst_idx as u16;
                }
            }
            previous = dst_idx as u16;
            current = next;
        }
    }

    pub(crate) unsafe fn bind_from_storage(&self, storage: *mut u8, route_slots: usize) {
        let parts = /* SAFETY: the caller leased this route-table arena with the matching layout. */ unsafe {
            Self::storage_parts(storage, route_slots)
        };
        /* SAFETY: binding initializes every destination slot before publication. */
        unsafe {
            self.bind_storage(RouteTableStorageBinding {
                parts,
                shape: RouteTableStorageShape { route_slots },
            });
        }
    }

    pub(crate) unsafe fn migrate_from_storage(&self, storage: *mut u8, route_slots: usize) {
        let parts = /* SAFETY: the caller leased this unpublished replacement arena. */ unsafe {
            Self::storage_parts(storage, route_slots)
        };
        /* SAFETY: migration copies only initialized route frames into the
        unpublished replacement bundle. */
        unsafe {
            self.migrate_to(parts, RouteTableStorageShape { route_slots });
        }
    }

    pub(crate) unsafe fn rebind_from_storage(&self, storage: *mut u8, route_slots: usize) {
        let parts = /* SAFETY: migration already staged this replacement arena. */ unsafe {
            Self::storage_parts(storage, route_slots)
        };
        /* SAFETY: the replacement columns are fully initialized. */
        unsafe {
            self.rebind_storage(RouteTableStorageBinding {
                parts,
                shape: RouteTableStorageShape { route_slots },
            });
        }
    }

    pub(crate) unsafe fn relocate_storage(&self, storage: *mut u8) {
        /* SAFETY: the owner exclusively moved the complete initialized sidecar
        without changing shape; no table-column alias is used during rebinding. */
        unsafe {
            self.rebind_from_storage(storage, self.route_slots());
        }
    }

    pub(crate) fn clear_storage(&self) {
        /* SAFETY: the owner retires route storage only after no endpoint budget
        owns it and no table-column alias remains; null roots precede reuse. */
        unsafe {
            self.frames.get().write(core::ptr::null_mut());
            self.active_head.get().write(core::ptr::null_mut());
            self.free_head.get().write(core::ptr::null_mut());
        }
        self.route_slots.set(0);
    }

    unsafe fn storage_parts(storage: *mut u8, route_slots: usize) -> RouteTableStorageParts {
        let frames = storage.cast::<RouteFrame>();
        let frames_bytes = checked_mul_usize(route_slots, core::mem::size_of::<RouteFrame>());
        let active_head_offset = checked_sub_usize(
            Self::align_up(
                checked_add_usize(storage as usize, frames_bytes),
                core::mem::align_of::<u16>(),
            ),
            storage as usize,
        );
        let free_head_offset = checked_add_usize(active_head_offset, core::mem::size_of::<u16>());
        /* SAFETY: both offsets come from the same checked storage layout and
        are aligned for their roots. */
        let (active_head, free_head) = unsafe {
            (
                storage.add(active_head_offset).cast::<u16>(),
                storage.add(free_head_offset).cast::<u16>(),
            )
        };
        RouteTableStorageParts {
            frames,
            active_head,
            free_head,
        }
    }

    #[inline]
    pub(super) fn frames_ptr(&self) -> *mut RouteFrame {
        self.raw_frames()
    }

    #[inline]
    pub(super) fn active_head_ptr(&self) -> *mut u16 {
        /* SAFETY: the initialized owner-exclusive root has no mutable alias. */
        unsafe { *self.active_head.get() }
    }

    #[inline]
    pub(super) fn free_head_ptr(&self) -> *mut u16 {
        /* SAFETY: `free_head` is the current initialized free-list root. */
        unsafe { *self.free_head.get() }
    }
}
