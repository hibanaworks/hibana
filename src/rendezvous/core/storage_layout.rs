use core::marker::PhantomData;

use super::{AssocTable, EndpointLeaseRecord, Rendezvous, Transport};
mod capacity;

// # Unsafe Owner Contract
//
// This file owns rendezvous slab layout, sidecar allocation, migration, and
// resident table ingress. The slab pointer and endpoint-lease table are created
// by the rendezvous constructor and remain pinned for the rendezvous lifetime.
// Every raw allocation returned here is aligned and range-checked against live
// owner roots and endpoint storage. Typed owners bind only
// after replacement initialization, and retirement canonically packs live roots
// before lowering the image frontier.

pub(crate) struct Sidecar<T> {
    ptr: *mut T,
    bytes: usize,
    _marker: PhantomData<T>,
}

impl<T> Clone for Sidecar<T> {
    #[inline]
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Sidecar<T> {}

impl<T> Sidecar<T> {
    pub(crate) const EMPTY: Self = Self {
        ptr: core::ptr::null_mut(),
        bytes: 0,
        _marker: PhantomData,
    };

    #[inline]
    pub(crate) const fn from_raw_parts(ptr: *mut T, bytes: usize) -> Self {
        if ptr.is_null() || bytes == 0 {
            crate::invariant();
        }
        Self {
            ptr,
            bytes,
            _marker: PhantomData,
        }
    }

    #[inline]
    pub(crate) const fn ptr(self) -> *mut T {
        self.ptr
    }

    #[inline]
    pub(crate) const fn bytes(self) -> usize {
        self.bytes
    }

    #[inline]
    pub(crate) fn is_empty(self) -> bool {
        if self.ptr.is_null() {
            if self.bytes != 0 {
                crate::invariant();
            }
            true
        } else {
            if self.bytes == 0 {
                crate::invariant();
            }
            false
        }
    }

    #[inline]
    pub(crate) const fn cast<U>(self) -> Sidecar<U> {
        Sidecar {
            ptr: self.ptr.cast::<U>(),
            bytes: self.bytes,
            _marker: PhantomData,
        }
    }
}

impl<'rv, 'cfg, T: Transport> Rendezvous<'rv, 'cfg, T>
where
    'cfg: 'rv,
{
    #[inline]
    pub(crate) fn slab_ptr_and_len(&self) -> (*mut u8, usize) {
        (self.slab_ptr, self.slab_len)
    }

    #[inline]
    pub(crate) fn endpoint_storage_floor(&self) -> usize {
        let (_, slab_len) = self.slab_ptr_and_len();
        let mut floor = slab_len;
        let mut idx = 0usize;
        while idx < self.endpoint_lease_slot_count() {
            let slot = crate::invariant_some(self.endpoint_lease_slot_by_index(idx));
            if slot.is_occupied() && slot.len != 0 && (slot.offset as usize) < floor {
                floor = slot.offset as usize;
            }
            idx += 1;
        }
        floor
    }

    #[inline]
    pub(crate) fn endpoint_lease_floor(&self) -> usize {
        self.image_frontier.get() as usize
    }

    #[inline]
    pub(crate) fn endpoint_lease_records_ptr(&self) -> *mut EndpointLeaseRecord {
        self.endpoint_lease_storage.get().ptr()
    }

    #[inline]
    fn set_image_frontier(&self, frontier: u32) {
        self.image_frontier.set(frontier);
    }
}
