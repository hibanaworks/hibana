use core::{
    cell::{Cell, UnsafeCell},
    marker::PhantomData,
};

use crate::session::types::SessionId;

use super::AssocTable;

#[derive(Clone, Copy)]
struct AssocStorageParts {
    entry_sids: *mut SessionId,
    entry_lanes: *mut u8,
    entry_states: *mut u16,
}

impl AssocTable {
    pub(in crate::rendezvous) unsafe fn init_empty(dst: *mut Self) {
        /* SAFETY: `Rendezvous` initialization passes an unpublished `AssocTable`; lane range is zeroed and column pointers are null before binding. */
        unsafe {
            core::ptr::addr_of_mut!((*dst).lane_base).write(Cell::new(0));
            core::ptr::addr_of_mut!((*dst).lane_slots).write(Cell::new(0));
            core::ptr::addr_of_mut!((*dst).assoc_slots).write(Cell::new(0));
            core::ptr::addr_of_mut!((*dst).entry_sids)
                .write(UnsafeCell::new(core::ptr::null_mut()));
            core::ptr::addr_of_mut!((*dst).entry_lanes)
                .write(UnsafeCell::new(core::ptr::null_mut()));
            core::ptr::addr_of_mut!((*dst).entry_states)
                .write(UnsafeCell::new(core::ptr::null_mut()));
            core::ptr::addr_of_mut!((*dst)._no_send_sync).write(PhantomData);
        }
    }

    #[inline]
    pub(in crate::rendezvous) const fn storage_align() -> usize {
        let sid = core::mem::align_of::<SessionId>();
        let state = core::mem::align_of::<u16>();
        if sid > state { sid } else { state }
    }

    #[inline]
    const fn align_up(value: usize, align: usize) -> usize {
        if align == 0 {
            crate::invariant();
        }
        let mask = align - 1;
        if value > usize::MAX - mask {
            crate::invariant();
        }
        (value + mask) & !mask
    }

    #[inline]
    const fn checked_add_usize(lhs: usize, rhs: usize) -> usize {
        if lhs > usize::MAX - rhs {
            crate::invariant();
        }
        lhs + rhs
    }

    #[inline]
    const fn checked_mul_usize(lhs: usize, rhs: usize) -> usize {
        if lhs != 0 && rhs > usize::MAX / lhs {
            crate::invariant();
        }
        lhs * rhs
    }

    #[inline]
    const fn checked_sub_usize(lhs: usize, rhs: usize) -> usize {
        if lhs < rhs {
            crate::invariant();
        }
        lhs - rhs
    }

    #[inline]
    pub(in crate::rendezvous) const fn storage_bytes(assoc_slots: usize) -> usize {
        let sid_bytes = Self::checked_mul_usize(assoc_slots, core::mem::size_of::<SessionId>());
        let lane_offset = Self::align_up(sid_bytes, core::mem::align_of::<u8>());
        let lane_bytes = Self::checked_mul_usize(assoc_slots, core::mem::size_of::<u8>());
        let state_offset = Self::align_up(
            Self::checked_add_usize(lane_offset, lane_bytes),
            core::mem::align_of::<u16>(),
        );
        let state_bytes = Self::checked_mul_usize(assoc_slots, core::mem::size_of::<u16>());
        Self::checked_add_usize(state_offset, state_bytes)
    }

    unsafe fn storage_parts(storage: *mut u8, assoc_slots: usize) -> AssocStorageParts {
        let entry_sids = storage.cast::<SessionId>();
        let lane_offset = Self::checked_sub_usize(
            Self::align_up(
                Self::checked_add_usize(
                    storage as usize,
                    Self::checked_mul_usize(assoc_slots, core::mem::size_of::<SessionId>()),
                ),
                core::mem::align_of::<u8>(),
            ),
            storage as usize,
        );
        let state_offset = Self::checked_sub_usize(
            Self::align_up(
                Self::checked_add_usize(
                    Self::checked_add_usize(storage as usize, lane_offset),
                    Self::checked_mul_usize(assoc_slots, core::mem::size_of::<u8>()),
                ),
                core::mem::align_of::<u16>(),
            ),
            storage as usize,
        );
        let (entry_lanes, entry_states) =
            /* SAFETY: all offsets are derived by the assoc arena layout and stay inside `storage_bytes(assoc_slots)`; this only computes disjoint column pointers. */ unsafe {
                (
                    storage.add(lane_offset).cast::<u8>(),
                    storage.add(state_offset).cast::<u16>(),
                )
            };
        AssocStorageParts {
            entry_sids,
            entry_lanes,
            entry_states,
        }
    }

    pub(super) unsafe fn remove_entry(&self, idx: usize) {
        if idx >= self.assoc_slots() {
            crate::invariant();
        }
        let lanes = self.entry_lanes_ptr();
        let sids = self.entry_sids_ptr();
        let states = self.entry_states_ptr();
        /* SAFETY: `idx` and `last` are live assoc-table indexes. Moving the last
        live entry into the gap keeps all synchronized columns prefix-packed. */
        unsafe {
            let mut last = self.assoc_slots();
            loop {
                if last == 0 {
                    crate::invariant();
                }
                last -= 1;
                if Self::entry_count(*states.add(last)) != 0 {
                    break;
                }
            }
            if last != idx {
                lanes.add(idx).write(*lanes.add(last));
                sids.add(idx).write(*sids.add(last));
                states.add(idx).write(*states.add(last));
            }
            lanes.add(last).write(0);
            sids.add(last).write(SessionId::new(0));
            states.add(last).write(Self::EMPTY_ENTRY_STATE);
        }
    }

    unsafe fn bind_storage(
        &self,
        lane_base: u32,
        lane_slots: usize,
        assoc_slots: usize,
        parts: AssocStorageParts,
    ) {
        if lane_slots > usize::from(u16::MAX) || assoc_slots > usize::from(u16::MAX) {
            crate::invariant();
        }
        /* SAFETY: all entry indexes are bounded by the freshly carved assoc
        storage before the column pointers are published. */
        unsafe {
            let mut idx = 0usize;
            while idx < assoc_slots {
                parts.entry_sids.add(idx).write(SessionId::new(0));
                parts.entry_lanes.add(idx).write(0);
                parts.entry_states.add(idx).write(Self::EMPTY_ENTRY_STATE);
                idx += 1;
            }
        }
        self.lane_base.set(lane_base);
        self.lane_slots.set(lane_slots as u16);
        self.assoc_slots.set(assoc_slots as u16);
        /* SAFETY: all columns belong to this freshly initialized table binding
        and are published only after every slot is initialized. */
        unsafe {
            self.entry_sids.get().write(parts.entry_sids);
            self.entry_lanes.get().write(parts.entry_lanes);
            self.entry_states.get().write(parts.entry_states);
        }
    }

    pub(in crate::rendezvous) unsafe fn bind_from_storage(
        &self,
        storage: *mut u8,
        lane_base: u32,
        lane_slots: usize,
        assoc_slots: usize,
    ) {
        let parts = /* SAFETY: the caller supplies the assoc-table arena with `assoc_slots` capacity. */ unsafe {
            Self::storage_parts(storage, assoc_slots)
        };
        /* SAFETY: `parts` was carved from the caller-provided assoc arena for
        exactly `assoc_slots`; `bind_storage` initializes every entry slot before
        installing these column pointers on `self`. */
        unsafe {
            self.bind_storage(lane_base, lane_slots, assoc_slots, parts);
        }
    }

    #[inline]
    pub(in crate::rendezvous) fn is_bound(&self) -> bool {
        !self.entry_sids_ptr().is_null()
    }

    pub(in crate::rendezvous) unsafe fn init_replacement_storage(
        &self,
        storage: *mut u8,
        lane_base: u32,
        lane_slots: usize,
        assoc_slots: usize,
    ) {
        let source_base = self.lane_base.get();
        let source_slots = self.assoc_slots();
        let source_lanes = self.entry_lanes_ptr();
        let source_sids = self.entry_sids_ptr();
        let source_states = self.entry_states_ptr();
        let parts = /* SAFETY: `storage` is the freshly leased assoc-table arena. */ unsafe {
            Self::storage_parts(storage, assoc_slots)
        };
        /* SAFETY: replacement entry indexes are bounded by the freshly leased
        assoc storage before any replacement pointer is published. */
        unsafe {
            let mut idx = 0usize;
            while idx < assoc_slots {
                parts.entry_sids.add(idx).write(SessionId::new(0));
                parts.entry_lanes.add(idx).write(0);
                parts.entry_states.add(idx).write(Self::EMPTY_ENTRY_STATE);
                idx += 1;
            }
        }
        let mut source_idx = 0usize;
        let mut dst_idx = 0usize;
        while source_idx < source_slots {
            let source_state = /* SAFETY: `source_idx < source_slots` selects one source state. */ unsafe {
                *source_states.add(source_idx)
            };
            if Self::entry_count(source_state) == 0 {
                source_idx += 1;
                continue;
            }
            let lane = source_base
                + /* SAFETY: `source_idx < source_slots` selects one source lane offset. */ unsafe {
                    *source_lanes.add(source_idx)
                } as u32;
            if lane < lane_base {
                crate::invariant();
            }
            let new_offset = lane - lane_base;
            if (new_offset as usize) >= lane_slots
                || new_offset > u8::MAX as u32
                || dst_idx >= assoc_slots
            {
                crate::invariant();
            }
            /* SAFETY: `source_idx < source_slots` reads one initialized source
            assoc entry, and `dst_idx < assoc_slots` writes the corresponding
            unpublished replacement entry in all columns. */
            unsafe {
                parts
                    .entry_sids
                    .add(dst_idx)
                    .write(*source_sids.add(source_idx));
                parts.entry_lanes.add(dst_idx).write(new_offset as u8);
                parts.entry_states.add(dst_idx).write(source_state);
            }
            dst_idx += 1;
            source_idx += 1;
        }
    }

    pub(in crate::rendezvous) unsafe fn commit_storage(
        &self,
        storage: *mut u8,
        lane_base: u32,
        lane_slots: usize,
        assoc_slots: usize,
    ) {
        let parts = /* SAFETY: `storage` is an initialized, exclusively owned
        assoc arena staged before pointer publication. */ unsafe {
            Self::storage_parts(storage, assoc_slots)
        };
        if lane_slots > usize::from(u16::MAX) || assoc_slots > usize::from(u16::MAX) {
            crate::invariant();
        }
        self.lane_base.set(lane_base);
        self.lane_slots.set(lane_slots as u16);
        self.assoc_slots.set(assoc_slots as u16);
        /* SAFETY: replacement columns were fully initialized before this
        owner-local pointer publication. */
        unsafe {
            self.entry_sids.get().write(parts.entry_sids);
            self.entry_lanes.get().write(parts.entry_lanes);
            self.entry_states.get().write(parts.entry_states);
        }
    }

    pub(in crate::rendezvous) unsafe fn relocate_storage(&self, storage: *mut u8) {
        /* SAFETY: the rendezvous owner moved the complete sidecar byte range
        without changing shape; commit recomputes only the column pointers. */
        unsafe {
            self.commit_storage(
                storage,
                self.lane_base.get(),
                self.lane_slots(),
                self.assoc_slots(),
            );
        }
    }

    pub(in crate::rendezvous) unsafe fn shrink_storage_in_place(
        &self,
        storage: *mut u8,
        assoc_slots: usize,
    ) {
        let current_slots = self.assoc_slots();
        let active_slots = self.active_entry_count();
        if assoc_slots == 0 || assoc_slots >= current_slots || assoc_slots < active_slots {
            crate::invariant();
        }
        let states = self.entry_states_ptr();
        let mut idx = 0usize;
        while idx < current_slots {
            let count = /* SAFETY: `idx` is bounded by the current initialized
            assoc state column. */ unsafe { Self::entry_count(*states.add(idx)) };
            if (idx < active_slots) != (count != 0) {
                crate::invariant();
            }
            idx += 1;
        }
        let replacement = /* SAFETY: the smaller assoc layout remains inside
        this owner-exclusive current sidecar allocation. */ unsafe {
            Self::storage_parts(storage, assoc_slots)
        };
        /* SAFETY: live entries occupy the prefix. The replacement sid prefix
        stays in place, and `ptr::copy` safely moves overlapping initialized
        lane/state prefixes before any replacement pointer is published. */
        unsafe {
            core::ptr::copy(self.entry_lanes_ptr(), replacement.entry_lanes, assoc_slots);
            core::ptr::copy(
                self.entry_states_ptr(),
                replacement.entry_states,
                assoc_slots,
            );
            self.entry_sids.get().write(replacement.entry_sids);
            self.entry_lanes.get().write(replacement.entry_lanes);
            self.entry_states.get().write(replacement.entry_states);
        }
        self.assoc_slots.set(assoc_slots as u16);
    }

    pub(in crate::rendezvous) fn clear_storage(&self) {
        if self.active_entry_count() != 0 {
            crate::invariant();
        }
        /* SAFETY: no live entry or external column alias remains. Publishing
        null owner roots precedes reuse of the retired assoc sidecar bytes. */
        unsafe {
            self.entry_sids.get().write(core::ptr::null_mut());
            self.entry_lanes.get().write(core::ptr::null_mut());
            self.entry_states.get().write(core::ptr::null_mut());
        }
        self.lane_base.set(0);
        self.lane_slots.set(0);
        self.assoc_slots.set(0);
    }
}
