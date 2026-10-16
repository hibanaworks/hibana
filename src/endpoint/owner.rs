//! The sole owned carrier lease behind a typed Endpoint.
//!
//! Moving this value keeps the resident cursor, ports and generation in place.
//! Restoring its typed facade checks the logical role and does not attach again.
//! This is an internal ownership primitive, not a wire representation.
use super::{
    Endpoint,
    carrier::{KernelEndpointHeader, PackedEndpointHandle},
};
use core::{marker::PhantomData, ptr::NonNull};

pub(super) struct EndpointOwner<'r> {
    pub(super) ptr: NonNull<KernelEndpointHeader<'r>>,
    pub(super) handle: PackedEndpointHandle,
    _borrow: PhantomData<&'r mut ()>,
    _local_only: crate::local::LocalOnly,
}

impl<'r> EndpointOwner<'r> {
    pub(super) fn new(
        ptr: NonNull<KernelEndpointHeader<'r>>,
        handle: PackedEndpointHandle,
    ) -> Self {
        Self {
            ptr,
            handle,
            _borrow: PhantomData,
            _local_only: crate::local::LocalOnly::new(),
        }
    }

    pub(super) fn into_endpoint<const ROLE: u8>(self) -> Result<Endpoint<'r, ROLE>, Self> {
        // SAFETY: only the rendezvous-installed live owner constructs this
        // value. Its unique lease keeps the resident header pinned for 'r.
        let header = unsafe { self.ptr.as_ref() };
        if header.role() != ROLE || header.generation() != self.handle.generation() {
            return Err(self);
        }
        Ok(Endpoint { owner: self })
    }
}

impl Drop for EndpointOwner<'_> {
    fn drop(&mut self) {
        // SAFETY: this value owns the same resident carrier lease formerly
        // owned directly by Endpoint. Copy the callback before invoking it so
        // no header borrow spans destruction of the resident endpoint.
        let drop_endpoint = unsafe { self.ptr.as_ref().ops().drop_endpoint };
        // SAFETY: ownership is consumed once by Drop. The carrier callback
        // validates the original generation before releasing its slot.
        unsafe { drop_endpoint(self.ptr.cast(), self.handle) };
    }
}

#[cfg(all(test, hibana_repo_tests))]
mod tests;
