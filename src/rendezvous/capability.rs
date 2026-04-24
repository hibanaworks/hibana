//! Capability-based delegation primitives.
//!
//! Implements CapTable for managing nonce-authenticated capability tokens
//! minted by the rendezvous.

use core::{
    cell::{Cell, UnsafeCell},
    marker::PhantomData,
    ptr::NonNull,
};

use super::error::CapError;
use super::tables::StateSnapshotTable;
use crate::control::cap::mint::{
    CAP_HANDLE_LEN, CAP_NONCE_LEN, CapShot, EndpointResource, ResourceKind,
};
use crate::control::types::{Lane, SessionId};

/// Internal capability entry.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CapEntry {
    pub(crate) sid: SessionId,
    pub(crate) lane_raw: u8,
    pub(crate) kind_tag: u8,
    pub(crate) shot_state: u8,
    pub(crate) role: u8,
    pub(crate) mint_revision: u64,
    pub(crate) consumed_revision: u64,
    pub(crate) released_revision: u64,
    pub(crate) nonce: [u8; CAP_NONCE_LEN],
    pub(crate) handle: [u8; CAP_HANDLE_LEN],
}

#[derive(Clone, Copy)]
pub(crate) struct CapReleaseCtx {
    cap_table: NonNull<CapTable>,
    snapshots: NonNull<StateSnapshotTable>,
    revisions: NonNull<Cell<u64>>,
    lane: Lane,
}

impl CapReleaseCtx {
    #[inline]
    pub(crate) fn new(
        cap_table: &CapTable,
        snapshots: &StateSnapshotTable,
        revisions: &Cell<u64>,
        lane: Lane,
    ) -> Self {
        Self {
            cap_table: NonNull::from(cap_table),
            snapshots: NonNull::from(snapshots),
            revisions: NonNull::from(revisions),
            lane,
        }
    }

    #[inline]
    pub(crate) fn release(self, nonce: &[u8; CAP_NONCE_LEN]) {
        unsafe {
            let revisions = self.revisions.as_ref();
            let release_revision = revisions
                .get()
                .checked_add(1)
                .expect("capability revision counter exhausted");
            revisions.set(release_revision);
            let snapshots = self.snapshots.as_ref();
            let cap_table = self.cap_table.as_ref();
            if let Some(snapshot_revision) = snapshots.available_cap_revision(self.lane) {
                cap_table.release_by_nonce_at_revision(
                    nonce,
                    self.lane,
                    release_revision,
                    snapshot_revision,
                );
            } else {
                cap_table.release_by_nonce(nonce);
            }
        }
    }
}

/// Capability table (per-Rendezvous).
///
/// Tracks nonce-minted capability tokens scoped to a rendezvous. Each entry
/// stores the originating session/lane pair, shot discipline, resource tag,
/// and the encoded handle bytes needed for descriptor/header validation.
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
                dst_slots.add(idx).write(None);
            }
            idx += 1;
        }
        let src_slots = self.slots_ptr();
        let mut dst_idx = 0usize;
        let mut src_idx = 0usize;
        while src_idx < self.capacity {
            let entry = unsafe { *src_slots.add(src_idx) };
            if let Some(entry) = entry {
                if dst_idx >= dst_capacity {
                    return false;
                }
                unsafe {
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
            self.rebind_storage(slots, capacity, reclaim_delta);
        }
    }

    pub(crate) unsafe fn migrate_from_storage(&self, storage: *mut u8, capacity: usize) -> bool {
        let slots = storage.cast::<Option<CapEntry>>();
        unsafe { self.migrate_to(slots, capacity) }
    }

    #[inline]
    pub(crate) fn insert_entry(&self, entry: CapEntry) -> Result<(), ()> {
        if self.capacity == 0 {
            return Err(());
        }
        unsafe {
            let slots = self.slots_ptr();
            let mut idx = 0usize;
            while idx < self.capacity {
                let slot = &mut *slots.add(idx);
                if slot.is_none() {
                    *slot = Some(entry);
                    return Ok(());
                }
                idx += 1;
            }
        }
        Err(())
    }

    /// Constant-time comparison of two 16-byte arrays.
    ///
    /// This prevents timing attacks where an attacker could incrementally
    /// guess nonce bytes by measuring response time differences.
    ///
    /// # Security
    /// - Always compares all 16 bytes, regardless of early mismatches
    /// - Uses bitwise operations to avoid conditional branches
    /// - A volatile read keeps the accumulator observable to the optimizer
    #[inline(never)] // Prevent inlining that might break constant-time guarantee
    fn ct_eq_nonce(a: &[u8; CAP_NONCE_LEN], b: &[u8; CAP_NONCE_LEN]) -> bool {
        let mut diff = 0u8;
        for i in 0..CAP_NONCE_LEN {
            diff |= a[i] ^ b[i];
        }
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
                if entry.shot_state == 2 && entry.consumed_revision > snapshot_revision {
                    entry.shot_state = CapShot::One.as_u8();
                    entry.consumed_revision = 0;
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
    pub(crate) fn release_by_nonce(&self, nonce: &[u8; CAP_NONCE_LEN]) {
        if self.capacity == 0 {
            return;
        }
        unsafe {
            let slots = self.slots_ptr();
            let mut idx = 0usize;
            while idx < self.capacity {
                let slot = &mut *slots.add(idx);
                if slot.is_some_and(|entry| Self::ct_eq_nonce(&entry.nonce, nonce)) {
                    *slot = None;
                    break;
                }
                idx += 1;
            }
        }
    }

    #[inline]
    pub(crate) fn release_by_nonce_at_revision(
        &self,
        nonce: &[u8; CAP_NONCE_LEN],
        lane: Lane,
        release_revision: u64,
        snapshot_revision: u64,
    ) {
        if self.capacity == 0 {
            return;
        }
        unsafe {
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
                if entry.lane_raw == lane.as_wire() && entry.mint_revision <= snapshot_revision {
                    entry.released_revision = release_revision;
                } else {
                    *slot = None;
                }
                break;
            }
        }
    }

    pub(crate) fn claim_by_nonce(
        &self,
        nonce: &[u8; CAP_NONCE_LEN],
        sid: SessionId,
        lane: Lane,
        expected_tag: u8,
        expected_role: u8,
        expected_shot: CapShot,
        claim_revision: u64,
    ) -> Result<(bool, [u8; CAP_HANDLE_LEN]), CapError> {
        if self.capacity == 0 {
            return Err(CapError::UnknownToken);
        }
        unsafe {
            let slots = self.slots_ptr();
            let mut idx = 0usize;
            while idx < self.capacity {
                let Some(entry) = (&mut *slots.add(idx)).as_mut() else {
                    idx += 1;
                    continue;
                };
                if entry.sid != sid || entry.lane_raw != lane.as_wire() {
                    idx += 1;
                    continue;
                }
                if !Self::ct_eq_nonce(&entry.nonce, nonce) {
                    idx += 1;
                    continue;
                }
                if entry.kind_tag != expected_tag {
                    return Err(CapError::Mismatch);
                }
                if entry.role != expected_role {
                    return Err(CapError::Mismatch);
                }
                if entry.released_revision != 0 {
                    return Err(CapError::UnknownToken);
                }
                let exhausted = entry.shot_state == 2;
                let stored_shot = match entry.shot_state {
                    x if x == CapShot::One.as_u8() => CapShot::One,
                    x if x == CapShot::Many.as_u8() => CapShot::Many,
                    2 => CapShot::One,
                    _ => return Err(CapError::Mismatch),
                };
                if stored_shot != expected_shot {
                    return Err(CapError::Mismatch);
                }

                if expected_tag == EndpointResource::TAG {
                    let mut handle = EndpointResource::decode_handle(entry.handle)
                        .map_err(|_| CapError::Mismatch)?;
                    if handle.sid != sid || handle.lane != lane || handle.role != entry.role {
                        EndpointResource::zeroize(&mut handle);
                        return Err(CapError::Mismatch);
                    }
                    EndpointResource::zeroize(&mut handle);
                }

                let handle_bytes = entry.handle;
                if exhausted {
                    return Err(CapError::Exhausted);
                }
                return match stored_shot {
                    CapShot::One => {
                        entry.shot_state = 2;
                        entry.consumed_revision = claim_revision;
                        Ok((true, handle_bytes))
                    }
                    CapShot::Many => Ok((false, handle_bytes)),
                };
            }
        }
        Err(CapError::UnknownToken)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::cap::mint::{EndpointHandle, EndpointResource};

    fn cap_table() -> CapTable {
        const CAP_TABLE_SLOTS: usize = 64;
        let mut table = CapTable::empty();
        let storage = std::vec![Option::<CapEntry>::None; CAP_TABLE_SLOTS].into_boxed_slice();
        let ptr = std::boxed::Box::leak(storage).as_mut_ptr().cast::<u8>();
        unsafe {
            table.bind_from_storage(ptr, CAP_TABLE_SLOTS, 0);
        }
        table
    }

    #[test]
    fn claim_by_nonce_returns_verified_handle() {
        let table = cap_table();
        let nonce = [0xAB; 16];
        let endpoint = EndpointHandle::new(SessionId::new(7), Lane::new(3), 9);
        let entry = CapEntry {
            sid: SessionId::new(7),
            lane_raw: Lane::new(3).as_wire(),
            kind_tag: EndpointResource::TAG,
            shot_state: CapShot::Many.as_u8(),
            role: endpoint.role,
            mint_revision: 1,
            consumed_revision: 0,
            released_revision: 0,
            nonce,
            handle: EndpointResource::encode_handle(&endpoint),
        };
        table.insert_entry(entry).expect("insert succeeds");

        let (exhausted, handle_bytes) = table
            .claim_by_nonce(
                &nonce,
                SessionId::new(7),
                Lane::new(3),
                EndpointResource::TAG,
                endpoint.role,
                CapShot::Many,
                2,
            )
            .expect("claim succeeds");

        assert!(!exhausted);
        assert_eq!(handle_bytes, EndpointResource::encode_handle(&endpoint));
    }

    #[test]
    fn one_shot_exhausts_on_second_claim() {
        let table = cap_table();
        let nonce = [0xCD; 16];
        let endpoint = EndpointHandle::new(SessionId::new(8), Lane::new(2), 5);
        let entry = CapEntry {
            sid: SessionId::new(8),
            lane_raw: Lane::new(2).as_wire(),
            kind_tag: EndpointResource::TAG,
            shot_state: CapShot::One.as_u8(),
            role: endpoint.role,
            mint_revision: 1,
            consumed_revision: 0,
            released_revision: 0,
            nonce,
            handle: EndpointResource::encode_handle(&endpoint),
        };
        table.insert_entry(entry).expect("insert succeeds");

        // First claim succeeds and marks as consumed
        let (exhausted, _) = table
            .claim_by_nonce(
                &nonce,
                SessionId::new(8),
                Lane::new(2),
                EndpointResource::TAG,
                endpoint.role,
                CapShot::One,
                2,
            )
            .expect("first claim succeeds");
        assert!(exhausted, "One shot should be exhausted after first claim");

        // Second claim fails because entry is consumed
        let result = table.claim_by_nonce(
            &nonce,
            SessionId::new(8),
            Lane::new(2),
            EndpointResource::TAG,
            endpoint.role,
            CapShot::One,
            3,
        );
        assert!(
            matches!(result, Err(CapError::Exhausted)),
            "second claim should fail with Exhausted for consumed One entry"
        );
    }

    #[test]
    fn restore_lane_to_revision_preserves_pre_snapshot_authority() {
        let table = cap_table();
        let sid = SessionId::new(12);
        let lane = Lane::new(4);
        let endpoint = EndpointHandle::new(sid, lane, 3);
        let nonce_pre = [0x11; 16];
        let nonce_post = [0x22; 16];

        table
            .insert_entry(CapEntry {
                sid,
                lane_raw: lane.as_wire(),
                kind_tag: EndpointResource::TAG,
                shot_state: CapShot::One.as_u8(),
                role: endpoint.role,
                mint_revision: 1,
                consumed_revision: 0,
                released_revision: 0,
                nonce: nonce_pre,
                handle: EndpointResource::encode_handle(&endpoint),
            })
            .expect("insert succeeds");
        table
            .insert_entry(CapEntry {
                sid,
                lane_raw: lane.as_wire(),
                kind_tag: EndpointResource::TAG,
                shot_state: CapShot::Many.as_u8(),
                role: endpoint.role,
                mint_revision: 3,
                consumed_revision: 0,
                released_revision: 0,
                nonce: nonce_post,
                handle: EndpointResource::encode_handle(&endpoint),
            })
            .expect("insert succeeds");

        let _ = table
            .claim_by_nonce(
                &nonce_pre,
                sid,
                lane,
                EndpointResource::TAG,
                endpoint.role,
                CapShot::One,
                4,
            )
            .expect("pre-snapshot token claim succeeds");

        table.restore_lane_to_revision(lane, 2);

        let (exhausted, _) = table
            .claim_by_nonce(
                &nonce_pre,
                sid,
                lane,
                EndpointResource::TAG,
                endpoint.role,
                CapShot::One,
                5,
            )
            .expect("restore must revive a pre-snapshot one-shot consumed later");
        assert!(exhausted);

        assert!(matches!(
            table.claim_by_nonce(
                &nonce_post,
                sid,
                lane,
                EndpointResource::TAG,
                endpoint.role,
                CapShot::Many,
                6,
            ),
            Err(CapError::UnknownToken)
        ));
    }

    #[test]
    fn restore_lane_to_revision_revives_pre_snapshot_release_tombstone() {
        let table = cap_table();
        let sid = SessionId::new(15);
        let lane = Lane::new(2);
        let endpoint = EndpointHandle::new(sid, lane, 4);
        let nonce = [0x33; 16];

        table
            .insert_entry(CapEntry {
                sid,
                lane_raw: lane.as_wire(),
                kind_tag: EndpointResource::TAG,
                shot_state: CapShot::Many.as_u8(),
                role: endpoint.role,
                mint_revision: 1,
                consumed_revision: 0,
                released_revision: 0,
                nonce,
                handle: EndpointResource::encode_handle(&endpoint),
            })
            .expect("insert succeeds");

        table.release_by_nonce_at_revision(&nonce, lane, 4, 2);

        assert!(matches!(
            table.claim_by_nonce(
                &nonce,
                sid,
                lane,
                EndpointResource::TAG,
                endpoint.role,
                CapShot::Many,
                5,
            ),
            Err(CapError::UnknownToken)
        ));

        table.restore_lane_to_revision(lane, 2);

        let (exhausted, _) = table
            .claim_by_nonce(
                &nonce,
                sid,
                lane,
                EndpointResource::TAG,
                endpoint.role,
                CapShot::Many,
                6,
            )
            .expect("restore must revive pre-snapshot released capability");
        assert!(!exhausted);
    }
}
