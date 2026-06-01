use super::{
    ControlOp, CpError, DistributedTopologyInv, InAcked, InBegin, PhantomData, RendezvousId,
    ResourceScope, SessionId, TopologyAck, TopologyError, TopologyOperands,
    cluster_rendezvous_slot,
};
mod cache;
mod prepared_publication;
pub(crate) use cache::CachedTopologyBucket;
#[cfg(all(test, hibana_repo_tests))]
pub(crate) use cache::CachedTopologyBucketEntry;
pub(crate) use prepared_publication::{
    PreparedDistributedTopologyAck, PreparedDistributedTopologyBegin,
    PreparedDistributedTopologyCommit,
};
// # Unsafe Owner Contract
//
// This file owns the raw resident storage views used for distributed topology
// state buckets. Storage is supplied by the `SessionCluster` construction path,
// bound exactly once to typed bucket owners, and later rebound only during
// explicit resident-layout migration. The pointer tag in the bucket entry
// pointer is an internal reclaim-delta encoding; callers must never observe it
// as a standalone allocation identity. All slot access stays lane/session-local
// to this owner, and initialized entries are represented as `Option<...>` so
// mutation can preserve the table's initialization boundary without allocation.

pub(crate) enum DistributedPhase {
    BeginReserved,
    Begin {
        txn: InBegin<DistributedTopologyInv, crate::control::types::One>,
    },
    AckReserved,
    Acked {
        txn: InAcked<DistributedTopologyInv, crate::control::types::One>,
    },
    CommitReserved,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DistributedPhaseKind {
    BeginReserved,
    Begin,
    AckReserved,
    Acked,
    CommitReserved,
}

pub(crate) struct DistributedEntry {
    pub(crate) operands: TopologyOperands,
    pub(crate) phase: DistributedPhase,
}

pub(crate) struct DistributedTopologyBucketEntry {
    pub(crate) sid: SessionId,
    pub(crate) entry: DistributedEntry,
}

#[derive(Clone, Copy)]
pub(crate) struct DistributedTopologyBucket {
    entries: *mut Option<DistributedTopologyBucketEntry>,
    capacity: usize,
    _no_send_sync: PhantomData<*mut ()>,
}

impl DistributedTopologyBucket {
    pub(crate) const STORAGE_TAG_MASK: usize = Self::storage_align().saturating_sub(1);

    pub(crate) const fn empty() -> Self {
        Self {
            entries: core::ptr::null_mut(),
            capacity: 0,
            _no_send_sync: PhantomData,
        }
    }

    pub(crate) unsafe fn init_empty(dst: *mut Self) {
        /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
        unsafe {
            core::ptr::addr_of_mut!((*dst).entries).write(core::ptr::null_mut());
            core::ptr::addr_of_mut!((*dst).capacity).write(0);
            core::ptr::addr_of_mut!((*dst)._no_send_sync).write(PhantomData);
        }
    }

    #[inline]
    pub(crate) const fn storage_align() -> usize {
        core::mem::align_of::<Option<DistributedTopologyBucketEntry>>()
    }

    #[inline]
    pub(crate) const fn storage_bytes(capacity: usize) -> usize {
        capacity.saturating_mul(core::mem::size_of::<Option<DistributedTopologyBucketEntry>>())
    }

    #[inline]
    pub(crate) fn raw_entries(&self) -> *mut Option<DistributedTopologyBucketEntry> {
        self.entries
    }

    #[inline]
    pub(crate) fn entries_ptr(&self) -> *mut Option<DistributedTopologyBucketEntry> {
        self.raw_entries()
            .map_addr(|addr| addr & !Self::STORAGE_TAG_MASK)
    }

    #[inline]
    fn encode_entries_ptr(
        entries: *mut Option<DistributedTopologyBucketEntry>,
        reclaim_delta: usize,
    ) -> *mut Option<DistributedTopologyBucketEntry> {
        debug_assert_eq!(entries.addr() & Self::STORAGE_TAG_MASK, 0);
        debug_assert!(reclaim_delta <= Self::STORAGE_TAG_MASK);
        entries.map_addr(|addr| addr | reclaim_delta)
    }

    #[inline]
    pub(crate) fn storage_ptr(&self) -> *mut u8 {
        self.entries_ptr().cast::<u8>()
    }

    #[inline]
    pub(crate) fn storage_reclaim_delta(&self) -> usize {
        self.raw_entries().addr() & Self::STORAGE_TAG_MASK
    }

    #[inline]
    pub(crate) fn storage_len(&self) -> usize {
        Self::storage_bytes(self.capacity)
    }

    #[inline]
    pub(crate) fn capacity(&self) -> usize {
        self.capacity
    }

    pub(crate) fn occupied_len(&self) -> usize {
        let entries = self.entries_ptr();
        if entries.is_null() {
            return 0;
        }
        let mut idx = 0usize;
        let mut occupied = 0usize;
        while idx < self.capacity {
            /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
            unsafe {
                if (*entries.add(idx)).is_some() {
                    occupied += 1;
                }
            }
            idx += 1;
        }
        occupied
    }

    pub(crate) unsafe fn bind_from_storage(
        &mut self,
        storage: *mut u8,
        capacity: usize,
        reclaim_delta: usize,
    ) {
        let entries = storage.cast::<Option<DistributedTopologyBucketEntry>>();
        let mut idx = 0usize;
        while idx < capacity {
            /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
            unsafe {
                entries.add(idx).write(None);
            }
            idx += 1;
        }
        self.entries = Self::encode_entries_ptr(entries, reclaim_delta);
        self.capacity = capacity;
    }

    pub(crate) unsafe fn rebind_from_storage(
        &mut self,
        storage: *mut u8,
        new_capacity: usize,
        reclaim_delta: usize,
    ) {
        let old_entries = self.entries_ptr();
        let old_capacity = self.capacity;
        let new_entries = storage.cast::<Option<DistributedTopologyBucketEntry>>();
        let mut idx = 0usize;
        while idx < new_capacity {
            /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
            unsafe {
                new_entries.add(idx).write(None);
            }
            idx += 1;
        }

        if !old_entries.is_null() {
            let mut next = 0usize;
            let mut old_idx = 0usize;
            while old_idx < old_capacity {
                /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
                unsafe {
                    if let Some(entry) = (*old_entries.add(old_idx)).take() {
                        debug_assert!(next < new_capacity, "distributed topology rebind overflow");
                        new_entries.add(next).write(Some(entry));
                        next += 1;
                    }
                }
                old_idx += 1;
            }
        }

        self.entries = Self::encode_entries_ptr(new_entries, reclaim_delta);
        self.capacity = new_capacity;
    }

    pub(crate) fn contains_sid(&self, sid: SessionId) -> bool {
        let entries = self.entries_ptr();
        if entries.is_null() {
            return false;
        }
        let mut idx = 0usize;
        while idx < self.capacity {
            /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
            unsafe {
                if let Some(stored) = (&*entries.add(idx)).as_ref()
                    && stored.sid == sid
                {
                    return true;
                }
            }
            idx += 1;
        }
        false
    }

    pub(crate) fn insert(
        &mut self,
        sid: SessionId,
        entry: DistributedEntry,
    ) -> Result<(), CpError> {
        let entries = self.entries_ptr();
        if entries.is_null() {
            return Err(CpError::resource_exhausted(ResourceScope::Generic));
        }
        let mut first_empty = None;
        let mut idx = 0usize;
        while idx < self.capacity {
            /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
            unsafe {
                let slot = &mut *entries.add(idx);
                match slot {
                    Some(stored) if stored.sid == sid => {
                        return Err(CpError::ReplayDetected {
                            operation: ControlOp::TopologyBegin as u8,
                            nonce: sid.raw(),
                        });
                    }
                    None if first_empty.is_none() => first_empty = Some(idx),
                    _ => {}
                }
            }
            idx += 1;
        }
        let Some(idx) = first_empty else {
            return Err(CpError::resource_exhausted(ResourceScope::Generic));
        };
        /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
        unsafe {
            *entries.add(idx) = Some(DistributedTopologyBucketEntry { sid, entry });
        }
        Ok(())
    }

    pub(crate) fn get(&self, sid: SessionId) -> Option<&DistributedEntry> {
        let entries = self.entries_ptr();
        if entries.is_null() {
            return None;
        }
        let mut idx = 0usize;
        while idx < self.capacity {
            /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
            unsafe {
                if let Some(stored) = (&*entries.add(idx)).as_ref()
                    && stored.sid == sid
                {
                    return Some(&stored.entry);
                }
            }
            idx += 1;
        }
        None
    }

    pub(crate) fn remove(&mut self, sid: SessionId) -> Option<DistributedEntry> {
        let entries = self.entries_ptr();
        if entries.is_null() {
            return None;
        }
        let mut idx = 0usize;
        while idx < self.capacity {
            /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
            unsafe {
                let slot = &mut *entries.add(idx);
                if slot.as_ref().is_some_and(|stored| stored.sid == sid) {
                    return slot.take().map(|stored| stored.entry);
                }
            }
            idx += 1;
        }
        None
    }
}

/// Distributed topology state tracking.
///
/// Tracks in-flight distributed topology operations to ensure exactly-once semantics.
pub(crate) struct DistributedTopologyState<const MAX: usize> {
    buckets: [DistributedTopologyBucket; MAX],
}

impl<const MAX: usize> Default for DistributedTopologyState<MAX> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const MAX: usize> DistributedTopologyState<MAX> {
    /// Create a new empty state.
    pub(crate) const fn new() -> Self {
        Self {
            buckets: [DistributedTopologyBucket::empty(); MAX],
        }
    }

    pub(crate) unsafe fn init_empty(dst: *mut Self) {
        /* SAFETY: the caller supplies exclusive uninitialized storage and this initializer writes all exposed fields before return. */
        unsafe {
            let mut slot = 0usize;
            while slot < MAX {
                DistributedTopologyBucket::init_empty(core::ptr::addr_of_mut!(
                    (*dst).buckets[slot]
                ));
                slot += 1;
            }
        }
    }

    pub(crate) fn bucket(&self, rv_id: RendezvousId) -> Option<&DistributedTopologyBucket> {
        let slot = cluster_rendezvous_slot::<MAX>(rv_id)?;
        Some(&self.buckets[slot])
    }

    fn bucket_mut(&mut self, rv_id: RendezvousId) -> Option<&mut DistributedTopologyBucket> {
        let slot = cluster_rendezvous_slot::<MAX>(rv_id)?;
        Some(&mut self.buckets[slot])
    }

    pub(crate) fn contains_sid(&self, sid: SessionId) -> bool {
        let mut slot = 0usize;
        while slot < MAX {
            if self.buckets[slot].contains_sid(sid) {
                return true;
            }
            slot += 1;
        }
        false
    }

    pub(crate) fn phase(&self, sid: SessionId) -> Option<DistributedPhaseKind> {
        let mut slot = 0usize;
        while slot < MAX {
            if let Some(entry) = self.buckets[slot].get(sid) {
                return Some(Self::phase_kind(entry));
            }
            slot += 1;
        }
        None
    }

    #[inline]
    fn phase_kind(entry: &DistributedEntry) -> DistributedPhaseKind {
        match &entry.phase {
            DistributedPhase::BeginReserved => DistributedPhaseKind::BeginReserved,
            DistributedPhase::Begin { .. } => DistributedPhaseKind::Begin,
            DistributedPhase::AckReserved => DistributedPhaseKind::AckReserved,
            DistributedPhase::Acked { .. } => DistributedPhaseKind::Acked,
            DistributedPhase::CommitReserved => DistributedPhaseKind::CommitReserved,
        }
    }

    pub(crate) fn preflight_begin(
        &self,
        sid: SessionId,
        operands: TopologyOperands,
    ) -> Result<(), CpError> {
        if self.contains_sid(sid) {
            return Err(CpError::ReplayDetected {
                operation: ControlOp::TopologyBegin as u8,
                nonce: sid.raw(),
            });
        }
        self.bucket(operands.src_rv)
            .ok_or(CpError::RendezvousMismatch {
                expected: operands.src_rv.raw(),
                actual: 0,
            })
            .map(|_| ())
    }

    pub(crate) fn begin_capacity_reservation_layout(
        &self,
        sid: SessionId,
        operands: TopologyOperands,
    ) -> Result<Option<(usize, usize, usize)>, CpError> {
        self.preflight_begin(sid, operands)?;
        let bucket = self
            .bucket(operands.src_rv)
            .expect("topology begin preflight guaranteed source bucket");
        let required = bucket
            .occupied_len()
            .checked_add(1)
            .ok_or(CpError::resource_exhausted(ResourceScope::Generic))?;
        if bucket.capacity() >= required {
            return Ok(None);
        }
        Ok(Some((
            required,
            DistributedTopologyBucket::storage_bytes(required),
            DistributedTopologyBucket::storage_align(),
        )))
    }

    pub(crate) fn bind_reserved_begin_capacity<FF>(
        &mut self,
        rv_id: RendezvousId,
        capacity: usize,
        storage: *mut u8,
        reclaim_delta: usize,
        free: FF,
    ) where
        FF: FnOnce(*mut u8, usize, usize),
    {
        let bucket = self
            .bucket_mut(rv_id)
            .expect("reserved distributed topology capacity owner disappeared");
        let old_ptr = bucket.storage_ptr();
        let old_len = bucket.storage_len();
        let old_reclaim_delta = bucket.storage_reclaim_delta();
        /* SAFETY: storage was reserved for this bucket and is not visible until this bind. */
        unsafe {
            if old_ptr.is_null() {
                bucket.bind_from_storage(storage, capacity, reclaim_delta);
            } else {
                bucket.rebind_from_storage(storage, capacity, reclaim_delta);
                free(old_ptr, old_len, old_reclaim_delta);
            }
        }
    }

    pub(crate) fn preflight_ack(
        &self,
        sid: SessionId,
        src_rv: RendezvousId,
        expected: TopologyAck,
    ) -> Result<(), CpError> {
        let entry = self
            .bucket(src_rv)
            .and_then(|bucket| bucket.get(sid))
            .ok_or(CpError::Topology(TopologyError::InvalidSession))?;

        match &entry.phase {
            DistributedPhase::Begin { .. } => {}
            DistributedPhase::BeginReserved
            | DistributedPhase::AckReserved
            | DistributedPhase::Acked { .. }
            | DistributedPhase::CommitReserved => {
                return Err(CpError::ReplayDetected {
                    operation: ControlOp::TopologyAck as u8,
                    nonce: sid.raw(),
                });
            }
        }

        if entry.operands.ack(sid) != expected {
            return Err(CpError::Topology(TopologyError::GenerationMismatch));
        }

        Ok(())
    }

    pub(crate) fn preflight_commit(
        &self,
        sid: SessionId,
        src_rv: RendezvousId,
        expected: Option<TopologyAck>,
    ) -> Result<(), CpError> {
        let entry = self
            .bucket(src_rv)
            .and_then(|bucket| bucket.get(sid))
            .ok_or(CpError::Topology(TopologyError::InvalidSession))?;

        match &entry.phase {
            DistributedPhase::Acked { .. } => {}
            DistributedPhase::BeginReserved
            | DistributedPhase::Begin { .. }
            | DistributedPhase::AckReserved
            | DistributedPhase::CommitReserved => {
                return Err(CpError::Topology(TopologyError::InvalidState));
            }
        }

        if let Some(exp) = expected
            && entry.operands.ack(sid) != exp
        {
            return Err(CpError::Topology(TopologyError::CommitFailed));
        }

        Ok(())
    }

    pub(crate) fn preflight_abort(
        &self,
        sid: SessionId,
        src_rv: RendezvousId,
    ) -> Result<(TopologyOperands, DistributedPhaseKind), CpError> {
        let bucket = self
            .bucket(src_rv)
            .ok_or(CpError::Topology(TopologyError::InvalidSession))?;
        let phase = bucket
            .get(sid)
            .map(Self::phase_kind)
            .ok_or(CpError::Topology(TopologyError::InvalidSession))?;
        if matches!(phase, DistributedPhaseKind::CommitReserved) {
            return Err(CpError::Topology(TopologyError::InvalidState));
        }
        let operands = bucket
            .get(sid)
            .expect("distributed topology abort preflight lost entry")
            .operands;
        Ok((operands, phase))
    }

    pub(crate) fn commit_preflighted_abort(
        &mut self,
        sid: SessionId,
        src_rv: RendezvousId,
        expected_operands: TopologyOperands,
        expected_phase: DistributedPhaseKind,
    ) -> TopologyOperands {
        let entry = self
            .bucket_mut(src_rv)
            .expect("distributed topology abort preflight owner disappeared")
            .remove(sid)
            .expect("distributed topology abort commit lost preflighted entry");
        assert_eq!(
            entry.operands, expected_operands,
            "distributed topology abort operands changed after preflight"
        );
        assert_eq!(
            Self::phase_kind(&entry),
            expected_phase,
            "distributed topology abort phase changed after preflight"
        );
        entry.operands
    }

    pub(crate) fn get(&self, sid: SessionId) -> Option<&TopologyOperands> {
        let mut slot = 0usize;
        while slot < MAX {
            if let Some(entry) = self.buckets[slot].get(sid) {
                return Some(&entry.operands);
            }
            slot += 1;
        }
        None
    }
}

#[cfg(all(test, hibana_repo_tests))]
#[path = "topology_state/tests.rs"]
mod tests;
