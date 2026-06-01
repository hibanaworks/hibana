use super::*;

impl CachedTopologyBucket {
    #[inline]
    pub(crate) const fn storage_bytes(capacity: usize) -> usize {
        capacity.saturating_mul(core::mem::size_of::<Option<CachedTopologyBucketEntry>>())
    }
    #[inline]
    fn encode_entries_ptr(
        entries: *mut Option<CachedTopologyBucketEntry>,
        reclaim_delta: usize,
    ) -> *mut Option<CachedTopologyBucketEntry> {
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
        let entries = storage.cast::<Option<CachedTopologyBucketEntry>>();
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
        let new_entries = storage.cast::<Option<CachedTopologyBucketEntry>>();
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
                /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
                unsafe {
                    if let Some(entry) = (*old_entries.add(old_idx)).take() {
                        debug_assert!(
                            next < new_capacity,
                            "cached topology bucket rebind overflow"
                        );
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
        operands: TopologyOperands,
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
                        stored.operands = operands;
                        return Ok(());
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
        /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
        unsafe {
            entries
                .add(idx)
                .write(Some(CachedTopologyBucketEntry { sid, operands }));
        }
        Ok(())
    }
}
