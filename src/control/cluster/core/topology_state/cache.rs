use super::{PhantomData, SessionId, TopologyOperands};

#[derive(Clone, Copy)]
pub(crate) struct CachedTopologyBucketEntry {
    pub(crate) sid: SessionId,
    pub(crate) operands: TopologyOperands,
}

#[derive(Clone, Copy)]
pub(crate) struct CachedTopologyBucket {
    pub(super) entries: *mut Option<CachedTopologyBucketEntry>,
    pub(super) capacity: usize,
    pub(super) _no_send_sync: PhantomData<*mut ()>,
}

impl CachedTopologyBucket {
    pub(crate) const STORAGE_TAG_MASK: usize = Self::storage_align().saturating_sub(1);

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
        core::mem::align_of::<Option<CachedTopologyBucketEntry>>()
    }

    #[inline]
    pub(crate) fn raw_entries(&self) -> *mut Option<CachedTopologyBucketEntry> {
        self.entries
    }

    #[inline]
    pub(crate) fn entries_ptr(&self) -> *mut Option<CachedTopologyBucketEntry> {
        self.raw_entries()
            .map_addr(|addr| addr & !Self::STORAGE_TAG_MASK)
    }

    #[cfg(all(test, hibana_repo_tests))]
    pub(crate) fn get(&self, sid: SessionId) -> Option<&TopologyOperands> {
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
                    return Some(&stored.operands);
                }
            }
            idx += 1;
        }
        None
    }

    pub(crate) fn remove(&mut self, sid: SessionId) -> Option<TopologyOperands> {
        let entries = self.entries_ptr();
        if entries.is_null() {
            return None;
        }
        let mut idx = 0usize;
        while idx < self.capacity {
            /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
            unsafe {
                if let Some(stored) = (&mut *entries.add(idx)).take() {
                    if stored.sid == sid {
                        return Some(stored.operands);
                    }
                    entries.add(idx).write(Some(stored));
                }
            }
            idx += 1;
        }
        None
    }
}
