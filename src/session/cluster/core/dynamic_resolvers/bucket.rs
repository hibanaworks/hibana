use core::marker::PhantomData;

use super::DynamicResolverEntry;
use crate::{
    global::const_dsl::ScopeId,
    rendezvous::core::Sidecar,
    session::cluster::error::{ClusterError, ResourceScope},
};

#[derive(Clone, Copy)]
pub(in crate::session::cluster::core) struct ResolverBucketEntry<'cfg> {
    pub(crate) scope: ScopeId,
    entry: DynamicResolverEntry<'cfg>,
}

pub(crate) struct ResolverBucket<'cfg> {
    storage: Sidecar<Option<ResolverBucketEntry<'cfg>>>,
    _no_send_sync: PhantomData<*mut ()>,
}

impl<'cfg> ResolverBucket<'cfg> {
    pub(crate) const fn empty() -> Self {
        Self {
            storage: Sidecar::EMPTY,
            _no_send_sync: PhantomData,
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
    pub(crate) fn erased_storage_sidecar(&self) -> Sidecar<u8> {
        self.storage.cast()
    }

    #[inline]
    pub(crate) fn capacity(&self) -> usize {
        Self::sidecar_capacity(self.storage)
    }

    #[inline]
    fn sidecar_capacity(storage: Sidecar<Option<ResolverBucketEntry<'cfg>>>) -> usize {
        if storage.is_empty() {
            return 0;
        }
        let entry_bytes = core::mem::size_of::<Option<ResolverBucketEntry<'cfg>>>();
        if entry_bytes == 0 || !storage.bytes().is_multiple_of(entry_bytes) {
            crate::invariant();
        }
        storage.bytes() / entry_bytes
    }

    pub(crate) fn entry_count(&self) -> usize {
        let entries = self.entries_ptr();
        if entries.is_null() {
            return 0;
        }
        let mut idx = 0usize;
        let mut count = 0usize;
        while idx < self.capacity() {
            /* SAFETY: `idx < self.capacity()` bounds this resolver-bucket slot,
            and `entries` is the sidecar pointer currently installed for this
            bucket. Shared counting does not mutate resolver storage. */
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
        if Self::sidecar_capacity(storage) != capacity {
            crate::invariant();
        }
        let entries = storage.ptr();
        let mut idx = 0usize;
        while idx < capacity {
            /* SAFETY: `storage` is the fresh resolver sidecar allocated for
            this bucket. `idx < capacity` selects one uninitialized slot, and
            the bucket is not committed until every slot is written to `None`. */
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
        if Self::sidecar_capacity(storage) != new_capacity {
            crate::invariant();
        }
        let source_entries = self.entries_ptr();
        let source_capacity = self.capacity();
        let new_entries = storage.ptr();
        let mut idx = 0usize;
        while idx < new_capacity {
            /* SAFETY: `new_entries` is the unpublished replacement resolver
            sidecar. The loop initializes every slot in `0..new_capacity`
            before any copied entry can be observed through `self.storage`. */
            unsafe {
                new_entries.add(idx).write(None);
            }
            idx += 1;
        }

        if !source_entries.is_null() {
            let mut next = 0usize;
            let mut source_idx = 0usize;
            while source_idx < source_capacity {
                /* SAFETY: `source_idx < source_capacity` reads an initialized
                slot from the current bucket, and `next < new_capacity` is
                checked before writing the unpublished replacement slot. */
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
        if Self::sidecar_capacity(storage) != new_capacity {
            crate::invariant();
        }
        self.storage = storage;
    }

    pub(crate) fn required_capacity(
        &self,
        additional_entries: usize,
    ) -> Result<Option<usize>, ClusterError> {
        if additional_entries == 0 {
            return Ok(None);
        }
        let required = self.entry_count().checked_add(additional_entries).ok_or(
            ClusterError::resource_exhausted(ResourceScope::ResolverTable),
        )?;
        if self.capacity() >= required {
            return Ok(None);
        }
        Ok(Some(required))
    }

    pub(crate) unsafe fn replace_storage(&mut self, storage: Sidecar<u8>, required: usize) {
        let source_storage = self.storage_sidecar();
        /* SAFETY: the rendezvous allocator supplied a resolver sidecar with the
        exact bucket size/alignment. Copy initialization has no callback and the
        new root is committed before the old sidecar is retired. */
        unsafe {
            if source_storage.ptr().is_null() {
                self.bind_from_storage(storage.cast(), required);
            } else {
                self.init_replacement_storage(storage.cast(), required);
                self.commit_storage(storage.cast(), required);
            }
        }
    }

    pub(crate) unsafe fn relocate_storage(&mut self, storage: Sidecar<u8>) {
        let relocated = storage.cast();
        if Self::sidecar_capacity(relocated) != self.capacity() {
            crate::invariant();
        }
        self.storage = relocated;
    }

    pub(crate) fn insert(
        &mut self,
        scope: ScopeId,
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
        while idx < self.capacity() {
            /* SAFETY: `idx < self.capacity()` bounds the installed resolver
            bucket sidecar. `&mut self` is the bucket mutation token, so this
            scan may update an existing slot or remember a vacant one. */
            unsafe {
                let slot = &mut *entries.add(idx);
                if let Some(stored) = slot {
                    if stored.scope == scope {
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
        /* SAFETY: `first_empty` was produced by the bounded scan above over the
        installed resolver sidecar, and `&mut self` still owns the bucket slot
        mutation. */
        unsafe {
            *entries.add(idx) = Some(ResolverBucketEntry { scope, entry });
        }
        Ok(())
    }

    pub(crate) fn get(&self, scope: ScopeId) -> Option<DynamicResolverEntry<'cfg>> {
        let entries = self.entries_ptr();
        if entries.is_null() {
            return None;
        }
        let mut idx = 0usize;
        while idx < self.capacity() {
            /* SAFETY: `idx < self.capacity()` bounds the installed resolver
            bucket sidecar. This shared lookup copies the entry without exposing
            a borrow into relocatable storage. */
            unsafe {
                if let Some(stored) = (&*entries.add(idx)).as_ref()
                    && stored.scope == scope
                {
                    return Some(stored.entry);
                }
            }
            idx += 1;
        }
        None
    }
}
