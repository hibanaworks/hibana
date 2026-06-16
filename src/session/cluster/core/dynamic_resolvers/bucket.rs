use core::marker::PhantomData;

use super::DynamicResolverEntry;
use crate::{
    eff::EffIndex,
    rendezvous::core::Sidecar,
    session::cluster::error::{ClusterError, ResourceScope},
};

#[derive(Clone, Copy)]
pub(in crate::session::cluster::core) struct ResolverBucketEntry<'cfg> {
    pub(crate) eff_index: EffIndex,
    entry: DynamicResolverEntry<'cfg>,
}

pub(crate) struct ResolverBucket<'cfg> {
    storage: Sidecar<Option<ResolverBucketEntry<'cfg>>>,
    capacity: usize,
    _no_send_sync: PhantomData<*mut ()>,
}

impl<'cfg> ResolverBucket<'cfg> {
    pub(crate) unsafe fn init_empty(dst: *mut Self) {
        /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
        unsafe {
            core::ptr::addr_of_mut!((*dst).storage).write(Sidecar::EMPTY);
            core::ptr::addr_of_mut!((*dst).capacity).write(0);
            core::ptr::addr_of_mut!((*dst)._no_send_sync).write(PhantomData);
        }
    }

    #[inline]
    pub(crate) const fn storage_align() -> usize {
        core::mem::align_of::<Option<ResolverBucketEntry<'cfg>>>()
    }

    #[inline]
    pub(crate) const fn storage_bytes(capacity: usize) -> usize {
        let size = core::mem::size_of::<Option<ResolverBucketEntry<'cfg>>>();
        if size != 0 && capacity > usize::MAX / size {
            crate::invariant();
        }
        capacity * size
    }

    #[inline]
    pub(in crate::session::cluster::core) fn entries_ptr(
        &self,
    ) -> *mut Option<ResolverBucketEntry<'cfg>> {
        self.storage.ptr()
    }

    #[inline]
    pub(in crate::session::cluster::core) fn storage_sidecar(
        &self,
    ) -> Sidecar<Option<ResolverBucketEntry<'cfg>>> {
        self.storage
    }

    #[inline]
    pub(crate) fn capacity(&self) -> usize {
        self.capacity
    }

    pub(crate) fn entry_count(&self) -> usize {
        let entries = self.entries_ptr();
        if entries.is_null() {
            return 0;
        }
        let mut idx = 0usize;
        let mut count = 0usize;
        while idx < self.capacity {
            /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
            unsafe {
                if (*entries.add(idx)).is_some() {
                    count += 1;
                }
            }
            idx += 1;
        }
        count
    }

    pub(in crate::session::cluster::core) unsafe fn bind_from_storage(
        &mut self,
        storage: Sidecar<Option<ResolverBucketEntry<'cfg>>>,
        capacity: usize,
    ) {
        let entries = storage.ptr();
        let mut idx = 0usize;
        while idx < capacity {
            /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
            unsafe {
                entries.add(idx).write(None);
            }
            idx += 1;
        }
        self.commit_storage(storage, capacity);
    }

    pub(in crate::session::cluster::core) unsafe fn init_replacement_storage(
        &self,
        storage: Sidecar<Option<ResolverBucketEntry<'cfg>>>,
        new_capacity: usize,
    ) {
        let source_entries = self.entries_ptr();
        let source_capacity = self.capacity;
        let new_entries = storage.ptr();
        let mut idx = 0usize;
        while idx < new_capacity {
            /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
            unsafe {
                new_entries.add(idx).write(None);
            }
            idx += 1;
        }

        if !source_entries.is_null() {
            let mut next = 0usize;
            let mut source_idx = 0usize;
            while source_idx < source_capacity {
                /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
                unsafe {
                    if let Some(entry) = *source_entries.add(source_idx) {
                        if next >= new_capacity {
                            crate::invariant();
                        }
                        new_entries.add(next).write(Some(entry));
                        next += 1;
                    }
                }
                source_idx += 1;
            }
        }
    }

    #[inline]
    pub(in crate::session::cluster::core) fn commit_storage(
        &mut self,
        storage: Sidecar<Option<ResolverBucketEntry<'cfg>>>,
        new_capacity: usize,
    ) {
        self.storage = storage;
        self.capacity = new_capacity;
    }

    pub(crate) fn ensure_capacity<FA, FF>(
        &mut self,
        additional_entries: usize,
        allocate: FA,
        mut free: FF,
    ) -> Result<(), ClusterError>
    where
        FA: FnOnce(usize, usize) -> Option<Sidecar<u8>>,
        FF: FnMut(Sidecar<u8>) -> Result<(), ResourceScope>,
    {
        if additional_entries == 0 {
            return Ok(());
        }
        let required = self.entry_count().checked_add(additional_entries).ok_or(
            ClusterError::resource_exhausted(ResourceScope::ResolverTable),
        )?;
        if self.capacity() >= required {
            return Ok(());
        }

        let source_storage = self.storage_sidecar();
        let storage = allocate(
            ResolverBucket::storage_bytes(required),
            ResolverBucket::storage_align(),
        )
        .ok_or(ClusterError::resource_exhausted(
            ResourceScope::ResolverTable,
        ))?;
        /* SAFETY: session cluster storage owns this resident slab region and checks the carved offset before raw access. */
        unsafe {
            if source_storage.ptr().is_null() {
                self.bind_from_storage(storage.cast(), required);
            } else {
                self.init_replacement_storage(storage.cast(), required);
                if let Err(resource) = free(source_storage.cast()) {
                    if free(storage).is_err() {
                        crate::invariant();
                    }
                    return Err(ClusterError::resource_exhausted(resource));
                }
                self.commit_storage(storage.cast(), required);
            }
        }
        Ok(())
    }

    pub(crate) fn insert(
        &mut self,
        eff_index: EffIndex,
        entry: DynamicResolverEntry<'cfg>,
    ) -> Result<(), ClusterError> {
        let entries = self.entries_ptr();
        if entries.is_null() {
            return Err(ClusterError::resource_exhausted(
                ResourceScope::ResolverTable,
            ));
        }
        let mut first_empty = None;
        let mut idx = 0usize;
        while idx < self.capacity {
            /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
            unsafe {
                let slot = &mut *entries.add(idx);
                if let Some(stored) = slot {
                    if stored.eff_index == eff_index {
                        stored.entry = entry;
                        return Ok(());
                    }
                } else if first_empty.is_none() {
                    first_empty = Some(idx);
                }
            }
            idx += 1;
        }
        let Some(idx) = first_empty else {
            return Err(ClusterError::resource_exhausted(
                ResourceScope::ResolverTable,
            ));
        };
        /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
        unsafe {
            *entries.add(idx) = Some(ResolverBucketEntry { eff_index, entry });
        }
        Ok(())
    }

    pub(crate) fn get(&self, eff_index: EffIndex) -> Option<&DynamicResolverEntry<'cfg>> {
        let entries = self.entries_ptr();
        if entries.is_null() {
            return None;
        }
        let mut idx = 0usize;
        while idx < self.capacity {
            /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
            unsafe {
                if let Some(stored) = (&*entries.add(idx)).as_ref()
                    && stored.eff_index == eff_index
                {
                    return Some(&stored.entry);
                }
            }
            idx += 1;
        }
        None
    }
}
