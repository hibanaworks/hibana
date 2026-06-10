use super::{
    Generation, Lane, PhantomData, UnsafeCell, align_up, checked_add_usize, checked_mul_usize,
    checked_sub_usize, lane_storage_align,
};
// # Unsafe Owner Contract
//
// This fragment owns the state-snapshot table columns. Unsafe operations bind
// caller-provided resident storage to column pointers and then access only
// indices proven inside `lane_base..lane_base + lane_slots`; initialized
// columns remain owned by the rendezvous table until explicit reset/restore.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SnapshotFinalization {
    Available = 0,
    Restored = 1,
    Committed = 2,
    RecordReserved = 3,
    RestoreReserved = 4,
    CommitReserved = 5,
}

impl SnapshotFinalization {
    #[inline]
    const fn from_u8(raw: u8) -> Self {
        match raw {
            0 => Self::Available,
            1 => Self::Restored,
            2 => Self::Committed,
            3 => Self::RecordReserved,
            4 => Self::RestoreReserved,
            5 => Self::CommitReserved,
            6..=u8::MAX => crate::invariant(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SnapshotRecord {
    snapshot: u16,
    cap_revision: u64,
    present: u8,
    finalization: u8,
}

mod reservation;
pub(crate) use reservation::*;

/// State snapshot table (per-lane).
///
/// Tracks the last snapshot epoch and finalization status for state-restore
/// and commit operations.
pub(crate) struct StateSnapshotTable {
    lane_base: u32,
    lane_slots: u16,
    last_snapshot: UnsafeCell<*mut u16>,
    cap_revision: UnsafeCell<*mut u64>,
    present: UnsafeCell<*mut u8>,
    finalization: UnsafeCell<*mut u8>,
    _no_send_sync: PhantomData<*mut ()>,
}

impl Default for StateSnapshotTable {
    fn default() -> Self {
        Self::empty()
    }
}

impl StateSnapshotTable {
    pub(crate) const fn empty() -> Self {
        Self {
            lane_base: 0,
            lane_slots: 0,
            last_snapshot: UnsafeCell::new(core::ptr::null_mut()),
            cap_revision: UnsafeCell::new(core::ptr::null_mut()),
            present: UnsafeCell::new(core::ptr::null_mut()),
            finalization: UnsafeCell::new(core::ptr::null_mut()),
            _no_send_sync: PhantomData,
        }
    }

    pub(crate) unsafe fn init_empty(dst: *mut Self) {
        /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
        unsafe {
            core::ptr::addr_of_mut!((*dst).lane_base).write(0);
            core::ptr::addr_of_mut!((*dst).lane_slots).write(0);
            core::ptr::addr_of_mut!((*dst).last_snapshot)
                .write(UnsafeCell::new(core::ptr::null_mut()));
            core::ptr::addr_of_mut!((*dst).cap_revision)
                .write(UnsafeCell::new(core::ptr::null_mut()));
            core::ptr::addr_of_mut!((*dst).present).write(UnsafeCell::new(core::ptr::null_mut()));
            core::ptr::addr_of_mut!((*dst).finalization)
                .write(UnsafeCell::new(core::ptr::null_mut()));
            core::ptr::addr_of_mut!((*dst)._no_send_sync).write(PhantomData);
        }
    }

    #[inline]
    pub(crate) const fn storage_align() -> usize {
        let lane_align = lane_storage_align();
        let cap_revision_align = core::mem::align_of::<u64>();
        if cap_revision_align > lane_align {
            cap_revision_align
        } else {
            lane_align
        }
    }

    #[inline]
    pub(crate) const fn storage_bytes(lane_slots: usize) -> usize {
        let snapshots_bytes = checked_mul_usize(lane_slots, core::mem::size_of::<u16>());
        let cap_revision_offset = align_up(snapshots_bytes, core::mem::align_of::<u64>());
        let cap_revision_bytes = checked_mul_usize(lane_slots, core::mem::size_of::<u64>());
        let present_offset = align_up(
            checked_add_usize(cap_revision_offset, cap_revision_bytes),
            core::mem::align_of::<u8>(),
        );
        let finalization_offset = align_up(
            checked_add_usize(
                present_offset,
                checked_mul_usize(lane_slots, core::mem::size_of::<u8>()),
            ),
            core::mem::align_of::<u8>(),
        );
        checked_add_usize(
            finalization_offset,
            checked_mul_usize(lane_slots, core::mem::size_of::<u8>()),
        )
    }

    pub(crate) unsafe fn bind_from_storage(
        &mut self,
        storage: *mut u8,
        lane_base: u32,
        lane_slots: usize,
    ) {
        let snapshots = storage.cast::<u16>();
        let cap_revision_offset = checked_sub_usize(
            align_up(
                checked_add_usize(
                    storage as usize,
                    checked_mul_usize(lane_slots, core::mem::size_of::<u16>()),
                ),
                core::mem::align_of::<u64>(),
            ),
            storage as usize,
        );
        let cap_revision = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { storage.add(cap_revision_offset) }.cast::<u64>();
        let present_offset = checked_sub_usize(
            align_up(
                checked_add_usize(
                    checked_add_usize(storage as usize, cap_revision_offset),
                    checked_mul_usize(lane_slots, core::mem::size_of::<u64>()),
                ),
                core::mem::align_of::<u8>(),
            ),
            storage as usize,
        );
        let present = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { storage.add(present_offset) }.cast::<u8>();
        let finalization_offset = checked_sub_usize(
            align_up(
                checked_add_usize(
                    checked_add_usize(storage as usize, present_offset),
                    checked_mul_usize(lane_slots, core::mem::size_of::<u8>()),
                ),
                core::mem::align_of::<u8>(),
            ),
            storage as usize,
        );
        let finalization = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { storage.add(finalization_offset) }.cast::<u8>();
        let mut idx = 0usize;
        while idx < lane_slots {
            /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
            unsafe {
                snapshots.add(idx).write(0);
                cap_revision.add(idx).write(0);
                present.add(idx).write(0);
                finalization
                    .add(idx)
                    .write(SnapshotFinalization::Available as u8);
            }
            idx += 1;
        }
        self.lane_base = lane_base;
        self.lane_slots = lane_slots as u16;
        *self.last_snapshot.get_mut() = snapshots;
        *self.cap_revision.get_mut() = cap_revision;
        *self.present.get_mut() = present;
        *self.finalization.get_mut() = finalization;
    }

    #[inline]
    fn last_snapshot_ptr(&self) -> *mut u16 {
        /* SAFETY: the rendezvous table owns initialized slots behind explicit presence state before raw access. */
        unsafe { *self.last_snapshot.get() }
    }

    #[inline]
    fn present_ptr(&self) -> *mut u8 {
        /* SAFETY: the rendezvous table owns initialized slots behind explicit presence state before raw access. */
        unsafe { *self.present.get() }
    }

    #[inline]
    fn cap_revision_ptr(&self) -> *mut u64 {
        /* SAFETY: the rendezvous table owns initialized slots behind explicit presence state before raw access. */
        unsafe { *self.cap_revision.get() }
    }

    #[inline]
    fn finalization_ptr(&self) -> *mut u8 {
        /* SAFETY: the rendezvous table owns initialized slots behind explicit presence state before raw access. */
        unsafe { *self.finalization.get() }
    }

    #[inline]
    pub(crate) fn is_bound(&self) -> bool {
        !self.last_snapshot_ptr().is_null()
    }

    #[inline]
    pub(crate) fn storage_ptr(&self) -> *mut u8 {
        self.last_snapshot_ptr().cast::<u8>()
    }

    #[inline]
    pub(crate) const fn storage_bytes_current(&self) -> usize {
        Self::storage_bytes(self.lane_slots as usize)
    }

    pub(crate) unsafe fn rebind_from_storage_preserving(
        &mut self,
        storage: *mut u8,
        lane_base: u32,
        lane_slots: usize,
    ) {
        let old_base = self.lane_base;
        let old_slots = self.lane_slots as usize;
        let old_snapshots = self.last_snapshot_ptr();
        let old_cap_revisions = self.cap_revision_ptr();
        let old_present = self.present_ptr();
        let old_finalization = self.finalization_ptr();
        let snapshots = storage.cast::<u16>();
        let cap_revision_offset = checked_sub_usize(
            align_up(
                checked_add_usize(
                    storage as usize,
                    checked_mul_usize(lane_slots, core::mem::size_of::<u16>()),
                ),
                core::mem::align_of::<u64>(),
            ),
            storage as usize,
        );
        let cap_revision = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { storage.add(cap_revision_offset) }.cast::<u64>();
        let present_offset = checked_sub_usize(
            align_up(
                checked_add_usize(
                    checked_add_usize(storage as usize, cap_revision_offset),
                    checked_mul_usize(lane_slots, core::mem::size_of::<u64>()),
                ),
                core::mem::align_of::<u8>(),
            ),
            storage as usize,
        );
        let present = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { storage.add(present_offset) }.cast::<u8>();
        let finalization_offset = checked_sub_usize(
            align_up(
                checked_add_usize(
                    checked_add_usize(storage as usize, present_offset),
                    checked_mul_usize(lane_slots, core::mem::size_of::<u8>()),
                ),
                core::mem::align_of::<u8>(),
            ),
            storage as usize,
        );
        let finalization = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { storage.add(finalization_offset) }.cast::<u8>();
        let mut idx = 0usize;
        while idx < lane_slots {
            /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
            unsafe {
                snapshots.add(idx).write(0);
                cap_revision.add(idx).write(0);
                present.add(idx).write(0);
                finalization
                    .add(idx)
                    .write(SnapshotFinalization::Available as u8);
            }
            idx += 1;
        }
        let mut old_idx = 0usize;
        while old_idx < old_slots {
            let lane = old_base + old_idx as u32;
            if lane >= lane_base {
                let new_idx = (lane - lane_base) as usize;
                if new_idx < lane_slots {
                    /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
                    unsafe {
                        snapshots.add(new_idx).write(*old_snapshots.add(old_idx));
                        cap_revision
                            .add(new_idx)
                            .write(*old_cap_revisions.add(old_idx));
                        present.add(new_idx).write(*old_present.add(old_idx));
                        finalization
                            .add(new_idx)
                            .write(*old_finalization.add(old_idx));
                    }
                }
            }
            old_idx += 1;
        }
        self.lane_base = lane_base;
        self.lane_slots = lane_slots as u16;
        *self.last_snapshot.get_mut() = snapshots;
        *self.cap_revision.get_mut() = cap_revision;
        *self.present.get_mut() = present;
        *self.finalization.get_mut() = finalization;
    }

    #[inline]
    fn lane_slot(&self, lane: Lane) -> Option<usize> {
        let lane_raw = lane.raw();
        if lane_raw < self.lane_base {
            return None;
        }
        let slot = (lane_raw - self.lane_base) as usize;
        (slot < self.lane_slots as usize).then_some(slot)
    }

    #[inline]
    fn read_record(&self, slot: usize) -> SnapshotRecord {
        /* SAFETY: the caller provides a slot proven by `lane_slot`. */
        unsafe {
            SnapshotRecord {
                snapshot: *self.last_snapshot_ptr().add(slot),
                cap_revision: *self.cap_revision_ptr().add(slot),
                present: *self.present_ptr().add(slot),
                finalization: *self.finalization_ptr().add(slot),
            }
        }
    }

    #[inline]
    fn write_record(&self, slot: usize, record: SnapshotRecord) {
        /* SAFETY: the caller provides a slot proven by `lane_slot`. */
        unsafe {
            self.last_snapshot_ptr().add(slot).write(record.snapshot);
            self.cap_revision_ptr().add(slot).write(record.cap_revision);
            self.present_ptr().add(slot).write(record.present);
            self.finalization_ptr().add(slot).write(record.finalization);
        }
    }

    /// Get the last state snapshot for a lane.
    #[inline]
    pub(crate) fn last_snapshot(&self, lane: Lane) -> Option<Generation> {
        let slot = self.lane_slot(lane)?;
        /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
        unsafe {
            (*self.present_ptr().add(slot) != 0)
                .then_some(Generation::new(*self.last_snapshot_ptr().add(slot)))
        }
    }

    /// Get the capability revision recorded with the last state snapshot.
    #[inline]
    pub(crate) fn last_cap_revision(&self, lane: Lane) -> Option<u64> {
        let slot = self.lane_slot(lane)?;
        /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
        unsafe {
            (*self.present_ptr().add(slot) != 0).then_some(*self.cap_revision_ptr().add(slot))
        }
    }

    /// Return the recorded capability revision only while the snapshot remains restorable.
    #[inline]
    pub(crate) fn available_cap_revision(&self, lane: Lane) -> Option<u64> {
        let slot = self.lane_slot(lane)?;
        /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
        unsafe {
            if *self.present_ptr().add(slot) == 0 {
                return None;
            }
            matches!(
                SnapshotFinalization::from_u8(*self.finalization_ptr().add(slot)),
                SnapshotFinalization::Available
            )
            .then_some(*self.cap_revision_ptr().add(slot))
        }
    }

    /// Return the current finalization state for a recorded snapshot.
    #[inline]
    pub(crate) fn finalization(&self, lane: Lane) -> Option<SnapshotFinalization> {
        let slot = self.lane_slot(lane)?;
        /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
        unsafe {
            (*self.present_ptr().add(slot) != 0).then_some(SnapshotFinalization::from_u8(
                *self.finalization_ptr().add(slot),
            ))
        }
    }

    /// Reset lane.
    #[inline]
    pub(crate) fn reset_lane(&self, lane: Lane) {
        let Some(slot) = self.lane_slot(lane) else {
            return;
        };
        /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
        unsafe {
            self.last_snapshot_ptr().add(slot).write(0);
            self.cap_revision_ptr().add(slot).write(0);
            self.present_ptr().add(slot).write(0);
            self.finalization_ptr()
                .add(slot)
                .write(SnapshotFinalization::Available as u8);
        }
    }
}
