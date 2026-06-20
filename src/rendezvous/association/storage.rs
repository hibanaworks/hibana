use core::{cell::UnsafeCell, marker::PhantomData};

use crate::{rendezvous::waiter::WaiterSlot, session::types::SessionId};

use super::AssocTable;

#[derive(Clone, Copy)]
struct AssocStorageParts {
    entry_sids: *mut SessionId,
    entry_lanes: *mut u8,
    entry_states: *mut u8,
    waiters: *mut WaiterSlot,
}

impl AssocTable {
    pub(in crate::rendezvous) unsafe fn init_empty(dst: *mut Self) {
        /* SAFETY: `Rendezvous` initialization passes an unpublished `AssocTable`; lane range is zeroed and column pointers are null before binding. */
        unsafe {
            core::ptr::addr_of_mut!((*dst).lane_base).write(0);
            core::ptr::addr_of_mut!((*dst).lane_slots).write(0);
            core::ptr::addr_of_mut!((*dst).assoc_slots).write(0);
            core::ptr::addr_of_mut!((*dst).entry_sids)
                .write(UnsafeCell::new(core::ptr::null_mut()));
            core::ptr::addr_of_mut!((*dst).entry_lanes)
                .write(UnsafeCell::new(core::ptr::null_mut()));
            core::ptr::addr_of_mut!((*dst).entry_states)
                .write(UnsafeCell::new(core::ptr::null_mut()));
            core::ptr::addr_of_mut!((*dst).inline_waiter)
                .write(UnsafeCell::new(WaiterSlot::empty()));
            core::ptr::addr_of_mut!((*dst).waiters).write(UnsafeCell::new(core::ptr::null_mut()));
            core::ptr::addr_of_mut!((*dst)._no_send_sync).write(PhantomData);
        }
    }

    #[inline]
    pub(in crate::rendezvous) const fn storage_align() -> usize {
        let entry_align = core::mem::align_of::<SessionId>();
        let waiter_align = core::mem::align_of::<WaiterSlot>();
        if entry_align > waiter_align {
            entry_align
        } else {
            waiter_align
        }
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
    const fn overflow_waiter_slots(assoc_slots: usize) -> usize {
        assoc_slots.saturating_sub(1)
    }

    #[inline]
    pub(in crate::rendezvous) const fn storage_bytes(assoc_slots: usize) -> usize {
        let sid_bytes = Self::checked_mul_usize(assoc_slots, core::mem::size_of::<SessionId>());
        let lane_offset = Self::align_up(sid_bytes, core::mem::align_of::<u8>());
        let lane_bytes = Self::checked_mul_usize(assoc_slots, core::mem::size_of::<u8>());
        let state_offset = Self::align_up(
            Self::checked_add_usize(lane_offset, lane_bytes),
            core::mem::align_of::<u8>(),
        );
        let state_bytes = Self::checked_mul_usize(assoc_slots, core::mem::size_of::<u8>());
        let waiter_offset = Self::align_up(
            Self::checked_add_usize(state_offset, state_bytes),
            core::mem::align_of::<WaiterSlot>(),
        );
        Self::checked_add_usize(
            waiter_offset,
            Self::checked_mul_usize(
                Self::overflow_waiter_slots(assoc_slots),
                core::mem::size_of::<WaiterSlot>(),
            ),
        )
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
                core::mem::align_of::<u8>(),
            ),
            storage as usize,
        );
        let waiter_offset = Self::checked_sub_usize(
            Self::align_up(
                Self::checked_add_usize(
                    Self::checked_add_usize(storage as usize, state_offset),
                    Self::checked_mul_usize(assoc_slots, core::mem::size_of::<u8>()),
                ),
                core::mem::align_of::<WaiterSlot>(),
            ),
            storage as usize,
        );
        let (entry_lanes, entry_states, waiters) =
            /* SAFETY: all offsets are derived by the assoc arena layout and stay inside `storage_bytes(assoc_slots)`; this only computes disjoint column pointers. */ unsafe {
                (
                    storage.add(lane_offset).cast::<u8>(),
                    storage.add(state_offset).cast::<u8>(),
                    storage.add(waiter_offset).cast::<WaiterSlot>(),
                )
            };
        AssocStorageParts {
            entry_sids,
            entry_lanes,
            entry_states,
            waiters,
        }
    }

    #[inline]
    pub(super) fn waiter_ptr(&self, idx: usize) -> *mut WaiterSlot {
        if idx >= self.assoc_slots() {
            crate::invariant();
        }
        if idx == 0 {
            self.inline_waiter.get()
        } else {
            self.waiters_ptr().wrapping_add(idx - 1)
        }
    }

    pub(super) unsafe fn remove_entry(&self, idx: usize) {
        if idx >= self.assoc_slots() {
            crate::invariant();
        }
        let lanes = self.entry_lanes_ptr();
        let sids = self.entry_sids_ptr();
        let states = self.entry_states_ptr();
        /* SAFETY: `idx` and `last` are live assoc-table indexes. The removed
        waiter is cleared, then the last live entry is moved into the gap so
        live entries stay prefix-packed and entry 0 keeps the inline waiter. */
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
            (*self.waiter_ptr(idx)).clear();
            if last != idx {
                lanes.add(idx).write(*lanes.add(last));
                sids.add(idx).write(*sids.add(last));
                states.add(idx).write(*states.add(last));
                if let Some(waker) = (*self.waiter_ptr(last)).take() {
                    (*self.waiter_ptr(idx)).set_owned(waker);
                }
            }
            lanes.add(last).write(0);
            sids.add(last).write(SessionId::new(0));
            states.add(last).write(Self::EMPTY_ENTRY_STATE);
            (*self.waiter_ptr(last)).clear();
        }
    }

    unsafe fn bind_storage(
        &mut self,
        lane_base: u32,
        lane_slots: usize,
        assoc_slots: usize,
        parts: AssocStorageParts,
    ) {
        if lane_slots > usize::from(u16::MAX) || assoc_slots > usize::from(u16::MAX) {
            crate::invariant();
        }
        /* SAFETY: all entry and overflow waiter indexes are bounded by the
        freshly carved assoc storage before the column pointers are published. */
        unsafe {
            let mut idx = 0usize;
            while idx < assoc_slots {
                parts.entry_sids.add(idx).write(SessionId::new(0));
                parts.entry_lanes.add(idx).write(0);
                parts.entry_states.add(idx).write(Self::EMPTY_ENTRY_STATE);
                idx += 1;
            }
            let mut waiter_idx = 0usize;
            while waiter_idx < Self::overflow_waiter_slots(assoc_slots) {
                WaiterSlot::init_empty(parts.waiters.add(waiter_idx));
                waiter_idx += 1;
            }
        }
        self.lane_base = lane_base;
        self.lane_slots = lane_slots as u16;
        self.assoc_slots = assoc_slots as u16;
        *self.inline_waiter.get_mut() = WaiterSlot::empty();
        *self.entry_sids.get_mut() = parts.entry_sids;
        *self.entry_lanes.get_mut() = parts.entry_lanes;
        *self.entry_states.get_mut() = parts.entry_states;
        *self.waiters.get_mut() = parts.waiters;
    }

    pub(in crate::rendezvous) unsafe fn bind_from_storage(
        &mut self,
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
        let source_base = self.lane_base;
        let source_slots = self.assoc_slots();
        let source_lanes = self.entry_lanes_ptr();
        let source_sids = self.entry_sids_ptr();
        let source_states = self.entry_states_ptr();
        let parts = /* SAFETY: `storage` is the freshly leased assoc-table arena. */ unsafe {
            Self::storage_parts(storage, assoc_slots)
        };
        /* SAFETY: replacement entry and overflow waiter indexes are bounded by
        the freshly leased assoc storage before any replacement pointer is published. */
        unsafe {
            let mut idx = 0usize;
            while idx < assoc_slots {
                parts.entry_sids.add(idx).write(SessionId::new(0));
                parts.entry_lanes.add(idx).write(0);
                parts.entry_states.add(idx).write(Self::EMPTY_ENTRY_STATE);
                idx += 1;
            }
            let mut waiter_idx = 0usize;
            while waiter_idx < Self::overflow_waiter_slots(assoc_slots) {
                WaiterSlot::init_empty(parts.waiters.add(waiter_idx));
                waiter_idx += 1;
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
            if lane >= lane_base {
                let new_offset = lane - lane_base;
                if (new_offset as usize) < lane_slots {
                    if new_offset > u8::MAX as u32 || dst_idx >= assoc_slots {
                        crate::invariant();
                    }
                    /* SAFETY: `source_idx < source_slots` reads one initialized
                    source assoc entry, and `dst_idx < assoc_slots` writes the
                    corresponding unpublished replacement entry in all columns. */
                    unsafe {
                        parts
                            .entry_sids
                            .add(dst_idx)
                            .write(*source_sids.add(source_idx));
                        parts.entry_lanes.add(dst_idx).write(new_offset as u8);
                        parts.entry_states.add(dst_idx).write(source_state);
                        if dst_idx == 0 {
                            if source_idx != 0 {
                                crate::invariant();
                            }
                        } else {
                            WaiterSlot::init_clone_from(
                                parts.waiters.add(dst_idx - 1),
                                &*self.waiter_ptr(source_idx),
                            );
                        }
                    }
                    dst_idx += 1;
                }
            }
            source_idx += 1;
        }
    }

    pub(in crate::rendezvous) fn clear_current_overflow_waiters(&self) {
        let waiters = self.waiters_ptr();
        let mut idx = 0usize;
        while idx < Self::overflow_waiter_slots(self.assoc_slots()) {
            /* SAFETY: waiters_ptr points at the currently bound overflow waiter column. */
            unsafe {
                (*waiters.add(idx)).clear();
            }
            idx += 1;
        }
    }

    pub(in crate::rendezvous) unsafe fn commit_storage(
        &mut self,
        storage: *mut u8,
        lane_base: u32,
        lane_slots: usize,
        assoc_slots: usize,
    ) {
        let parts = /* SAFETY: `storage` is an initialized assoc-table arena staged for publication. */ unsafe {
            Self::storage_parts(storage, assoc_slots)
        };
        if lane_slots > usize::from(u16::MAX) || assoc_slots > usize::from(u16::MAX) {
            crate::invariant();
        }
        self.lane_base = lane_base;
        self.lane_slots = lane_slots as u16;
        self.assoc_slots = assoc_slots as u16;
        *self.entry_sids.get_mut() = parts.entry_sids;
        *self.entry_lanes.get_mut() = parts.entry_lanes;
        *self.entry_states.get_mut() = parts.entry_states;
        *self.waiters.get_mut() = parts.waiters;
    }
}
