//! Capability-based delegation primitives.
//!
//! Implements rendezvous-local registered-token release state.
//!
//! # Unsafe Owner Contract
//!
//! This module is the rendezvous registered-token owner for capability table slots.
//! Unsafe blocks here may bind or migrate caller-provided storage only while
//! preserving the initialized-entry invariant: a present slot contains one fully
//! initialized `CapEntry`, and an absent slot must not be dropped or released.

use core::{
    cell::{Cell, UnsafeCell},
    marker::PhantomData,
};

use super::tables::StateSnapshotTable;
use crate::control::cap::mint::CAP_NONCE_LEN;
use crate::control::types::Lane;

/// Internal capability entry.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CapEntry {
    pub(crate) lane_raw: u8,
    pub(crate) mint_revision: u64,
    pub(crate) released_revision: u64,
    pub(crate) nonce: [u8; CAP_NONCE_LEN],
}

impl CapEntry {
    #[inline]
    pub(crate) fn new(lane: Lane, mint_revision: u64, nonce: [u8; CAP_NONCE_LEN]) -> Self {
        Self {
            lane_raw: lane.as_wire(),
            mint_revision,
            released_revision: 0,
            nonce,
        }
    }
}

pub(crate) struct CapReleaseCtx<'rv> {
    cap_table: &'rv CapTable,
    snapshots: &'rv StateSnapshotTable,
    revisions: &'rv Cell<u64>,
    lane: Lane,
}

impl<'rv> CapReleaseCtx<'rv> {
    #[inline]
    pub(crate) fn new(
        cap_table: &'rv CapTable,
        snapshots: &'rv StateSnapshotTable,
        revisions: &'rv Cell<u64>,
        lane: Lane,
    ) -> Self {
        Self {
            cap_table,
            snapshots,
            revisions,
            lane,
        }
    }

    #[inline]
    pub(crate) fn release(self, nonce: &[u8; CAP_NONCE_LEN]) {
        if let Some(snapshot_revision) = self.snapshots.available_cap_revision(self.lane) {
            self.cap_table.release_by_nonce_at_next_revision(
                nonce,
                self.lane,
                snapshot_revision,
                || {
                    let release_revision = self
                        .revisions
                        .get()
                        .checked_add(1)
                        .expect("capability revision counter exhausted");
                    self.revisions.set(release_revision);
                    release_revision
                },
            );
        } else {
            self.cap_table.release_by_nonce(nonce);
        }
    }
}

/// Capability table (per-Rendezvous).
///
/// Tracks nonce-minted capability tokens scoped to a rendezvous. Each entry
/// stores only the lane, lifecycle revisions, and nonce needed for registered
/// token cleanup and snapshot-aware rollback.
pub(crate) struct CapTable {
    slots: UnsafeCell<*mut Option<CapEntry>>,
    capacity: usize,
    _no_send_sync: PhantomData<*mut ()>,
}

impl Default for CapTable {
    fn default() -> Self {
        Self::empty()
    }
}

impl CapTable {
    const STORAGE_TAG_MASK: usize = Self::storage_align().saturating_sub(1);

    pub(crate) const fn empty() -> Self {
        Self {
            slots: UnsafeCell::new(core::ptr::null_mut()),
            capacity: 0,
            _no_send_sync: PhantomData,
        }
    }

    pub(crate) unsafe fn init_empty(dst: *mut Self) {
        unsafe {
            // SAFETY: caller provides writable, uninitialized storage for one
            // `CapTable`; each field is written exactly once.
            core::ptr::addr_of_mut!((*dst).slots).write(UnsafeCell::new(core::ptr::null_mut()));
            core::ptr::addr_of_mut!((*dst).capacity).write(0);
            core::ptr::addr_of_mut!((*dst)._no_send_sync).write(PhantomData);
        }
    }

    #[inline]
    pub(crate) const fn capacity(&self) -> usize {
        self.capacity
    }

    #[inline]
    pub(crate) fn storage_ptr(&self) -> *mut u8 {
        self.slots_ptr().cast::<u8>()
    }

    #[inline]
    pub(crate) fn storage_reclaim_delta(&self) -> usize {
        self.raw_slots().addr() & Self::STORAGE_TAG_MASK
    }

    #[inline]
    pub(crate) const fn storage_bytes_current(&self) -> usize {
        Self::storage_bytes(self.capacity)
    }

    #[inline]
    pub(crate) const fn storage_align() -> usize {
        core::mem::align_of::<Option<CapEntry>>()
    }

    #[inline]
    pub(crate) const fn storage_bytes(capacity: usize) -> usize {
        capacity.saturating_mul(core::mem::size_of::<Option<CapEntry>>())
    }

    #[inline]
    fn encode_slots_ptr(
        slots: *mut Option<CapEntry>,
        reclaim_delta: usize,
    ) -> *mut Option<CapEntry> {
        debug_assert_eq!(slots.addr() & Self::STORAGE_TAG_MASK, 0);
        debug_assert!(reclaim_delta <= Self::STORAGE_TAG_MASK);
        slots.map_addr(|addr| addr | reclaim_delta)
    }

    #[inline]
    fn raw_slots(&self) -> *mut Option<CapEntry> {
        // SAFETY: `slots` is an UnsafeCell-owned table pointer updated only
        // through this table's storage binding/migration methods.
        unsafe { *self.slots.get() }
    }

    #[inline]
    fn slots_ptr(&self) -> *mut Option<CapEntry> {
        self.raw_slots()
            .map_addr(|addr| addr & !Self::STORAGE_TAG_MASK)
    }

    pub(crate) fn live_count(&self) -> usize {
        let mut live = 0usize;
        let slots = self.slots_ptr();
        let mut idx = 0usize;
        while idx < self.capacity {
            // SAFETY: `idx < capacity`, and `slots_ptr` points to the
            // initialized Option<CapEntry> array owned by this table.
            let entry = unsafe { &*slots.add(idx) };
            if entry.is_some() {
                live += 1;
            }
            idx += 1;
        }
        live
    }

    unsafe fn bind_storage(
        &mut self,
        slots: *mut Option<CapEntry>,
        capacity: usize,
        reclaim_delta: usize,
    ) {
        let mut idx = 0usize;
        while idx < capacity {
            unsafe {
                // SAFETY: caller provides `capacity` writable slots for this
                // binding; each slot is initialized exactly once here.
                slots.add(idx).write(None);
            }
            idx += 1;
        }
        *self.slots.get_mut() = Self::encode_slots_ptr(slots, reclaim_delta);
        self.capacity = capacity;
    }

    unsafe fn rebind_storage(
        &mut self,
        slots: *mut Option<CapEntry>,
        capacity: usize,
        reclaim_delta: usize,
    ) {
        *self.slots.get_mut() = Self::encode_slots_ptr(slots, reclaim_delta);
        self.capacity = capacity;
    }

    unsafe fn migrate_to(&self, dst_slots: *mut Option<CapEntry>, dst_capacity: usize) -> bool {
        let mut idx = 0usize;
        while idx < dst_capacity {
            unsafe {
                // SAFETY: caller provides `dst_capacity` writable destination
                // slots; each slot is initialized exactly once before migration.
                dst_slots.add(idx).write(None);
            }
            idx += 1;
        }
        let src_slots = self.slots_ptr();
        let mut dst_idx = 0usize;
        let mut src_idx = 0usize;
        while src_idx < self.capacity {
            // SAFETY: `src_idx < self.capacity`, and source slots are the
            // initialized table array owned by this CapTable.
            let entry = unsafe { *src_slots.add(src_idx) };
            if let Some(entry) = entry {
                if dst_idx >= dst_capacity {
                    return false;
                }
                unsafe {
                    // SAFETY: `dst_idx < dst_capacity`; destination slots were
                    // initialized above and are rewritten with migrated entries.
                    dst_slots.add(dst_idx).write(Some(entry));
                }
                dst_idx += 1;
            }
            src_idx += 1;
        }
        true
    }

    pub(crate) unsafe fn bind_from_storage(
        &mut self,
        storage: *mut u8,
        capacity: usize,
        reclaim_delta: usize,
    ) {
        let slots = storage.cast::<Option<CapEntry>>();
        unsafe {
            // SAFETY: caller supplied storage aligned and sized for `capacity`
            // CapEntry option slots; ownership transfers to this table.
            self.bind_storage(slots, capacity, reclaim_delta);
        }
    }

    pub(crate) unsafe fn rebind_from_storage(
        &mut self,
        storage: *mut u8,
        capacity: usize,
        reclaim_delta: usize,
    ) {
        let slots = storage.cast::<Option<CapEntry>>();
        unsafe {
            // SAFETY: caller supplied the current table storage for this table
            // owner; rebind only records the already-initialized slot array.
            self.rebind_storage(slots, capacity, reclaim_delta);
        }
    }

    pub(crate) unsafe fn migrate_from_storage(&self, storage: *mut u8, capacity: usize) -> bool {
        let slots = storage.cast::<Option<CapEntry>>();
        // SAFETY: caller supplied a writable destination slot array with
        // `capacity` entries; `migrate_to` initializes and copies within bounds.
        unsafe { self.migrate_to(slots, capacity) }
    }

    #[cfg(test)]
    #[inline]
    pub(crate) fn insert_entry(&self, entry: CapEntry) -> Result<(), ()> {
        self.insert_entry_with(|| entry)
    }

    #[inline]
    pub(crate) fn insert_entry_with(&self, build: impl FnOnce() -> CapEntry) -> Result<(), ()> {
        if self.capacity == 0 {
            return Err(());
        }
        unsafe {
            // SAFETY: `bind_from_storage` and `migrate_from_storage` are the only
            // writers for `slots`/`capacity`. The builder is called only after a
            // vacant slot is found, so failed inserts cannot allocate nonce,
            // revision, or rollback authority. The loop stays within
            // `0..capacity`, and each initialized entry is represented by
            // `Option<CapEntry>` in that slot.
            let slots = self.slots_ptr();
            let mut idx = 0usize;
            while idx < self.capacity {
                let slot = &mut *slots.add(idx);
                if slot.is_none() {
                    let entry = build();
                    *slot = Some(entry);
                    return Ok(());
                }
                idx += 1;
            }
        }
        Err(())
    }

    /// Compare two nonce byte arrays without byte-index early exit.
    ///
    /// This is local hygiene for the rendezvous registered-token scan. The
    /// release path remains a trusted-domain cleanup path, not a
    /// cryptographic side-channel boundary.
    #[inline(never)]
    fn ct_eq_nonce(a: &[u8; CAP_NONCE_LEN], b: &[u8; CAP_NONCE_LEN]) -> bool {
        let mut diff = 0u8;
        for i in 0..CAP_NONCE_LEN {
            diff |= a[i] ^ b[i];
        }
        // SAFETY: `diff` is a live local byte; volatile read keeps the final
        // accumulator observable without exposing aliasing or lifetime state.
        let diff = unsafe { core::ptr::read_volatile(&diff) };
        diff == 0
    }

    /// Purge all capabilities for a lane (on release).
    #[inline]
    pub(crate) fn purge_lane(&self, lane: Lane) {
        if self.capacity == 0 {
            return;
        }
        unsafe {
            // SAFETY: this table owns `capacity` initialized Option<CapEntry>
            // slots; purge only mutates entries within the lane-owned scan.
            let slots = self.slots_ptr();
            let mut idx = 0usize;
            while idx < self.capacity {
                let slot = &mut *slots.add(idx);
                if slot.is_some_and(|entry| entry.lane_raw == lane.as_wire()) {
                    *slot = None;
                }
                idx += 1;
            }
        }
    }

    /// Restore lane capabilities to a previously recorded snapshot revision.
    #[inline]
    pub(crate) fn restore_lane_to_revision(&self, lane: Lane, snapshot_revision: u64) {
        if self.capacity == 0 {
            return;
        }
        unsafe {
            // SAFETY: this table owns `capacity` initialized Option<CapEntry>
            // slots; restore only rewrites entries for the requested lane.
            let slots = self.slots_ptr();
            let mut idx = 0usize;
            while idx < self.capacity {
                let slot = &mut *slots.add(idx);
                let Some(entry) = slot.as_mut() else {
                    idx += 1;
                    continue;
                };
                if entry.lane_raw != lane.as_wire() {
                    idx += 1;
                    continue;
                }
                if entry.mint_revision > snapshot_revision {
                    *slot = None;
                    idx += 1;
                    continue;
                }
                if entry.released_revision > snapshot_revision {
                    entry.released_revision = 0;
                }
                idx += 1;
            }
        }
    }

    /// Drop snapshot-only release tombstones once the snapshot baseline changes.
    #[inline]
    pub(crate) fn discard_released_lane_entries(&self, lane: Lane) {
        if self.capacity == 0 {
            return;
        }
        unsafe {
            // SAFETY: this table owns `capacity` initialized Option<CapEntry>
            // slots; discard scans in bounds and clears tombstones only.
            let slots = self.slots_ptr();
            let mut idx = 0usize;
            while idx < self.capacity {
                let slot = &mut *slots.add(idx);
                if slot.is_some_and(|entry| {
                    entry.lane_raw == lane.as_wire() && entry.released_revision != 0
                }) {
                    *slot = None;
                }
                idx += 1;
            }
        }
    }

    /// Release a capability entry by nonce for registered-token drop cleanup.
    ///
    /// This is called automatically by registered-token wrappers, ensuring
    /// RAII-based cleanup of registered capabilities.
    #[inline]
    pub(crate) fn release_by_nonce(&self, nonce: &[u8; CAP_NONCE_LEN]) -> bool {
        if self.capacity == 0 {
            return false;
        }
        unsafe {
            // SAFETY: this table owns `capacity` initialized Option<CapEntry>
            // slots; release scans in bounds and clears at most the matching nonce.
            let slots = self.slots_ptr();
            let mut idx = 0usize;
            while idx < self.capacity {
                let slot = &mut *slots.add(idx);
                if slot.is_some_and(|entry| Self::ct_eq_nonce(&entry.nonce, nonce)) {
                    *slot = None;
                    return true;
                }
                idx += 1;
            }
        }
        false
    }

    #[inline]
    pub(crate) fn release_by_nonce_at_next_revision(
        &self,
        nonce: &[u8; CAP_NONCE_LEN],
        lane: Lane,
        snapshot_revision: u64,
        next_release_revision: impl FnOnce() -> u64,
    ) -> bool {
        if self.capacity == 0 {
            return false;
        }
        unsafe {
            // SAFETY: this table owns `capacity` initialized Option<CapEntry>
            // slots; release-at-revision mutates only the matching nonce entry.
            // The revision allocator is called only after a matching live entry
            // is found, so failed cleanup attempts leave lifecycle clocks still.
            let slots = self.slots_ptr();
            let mut idx = 0usize;
            while idx < self.capacity {
                let slot = &mut *slots.add(idx);
                let Some(entry) = slot.as_mut() else {
                    idx += 1;
                    continue;
                };
                if !Self::ct_eq_nonce(&entry.nonce, nonce) {
                    idx += 1;
                    continue;
                }
                let release_revision = next_release_revision();
                if entry.lane_raw == lane.as_wire() && entry.mint_revision <= snapshot_revision {
                    entry.released_revision = release_revision;
                } else {
                    *slot = None;
                }
                return true;
            }
        }
        false
    }
}

#[cfg(all(test, hibana_repo_tests))]
mod tests;
