//! The sole owned carrier lease behind a typed Endpoint.
//!
//! Moving this value keeps the resident cursor, ports and generation in place.
//! Restoring its typed facade checks the issued role/generation and does not attach again.
//! This is an internal ownership primitive, not a wire representation.
use super::{
    Endpoint,
    carrier::{EndpointHandle, KernelEndpointHeader},
};
use core::ptr::NonNull;

pub(crate) struct EndpointOwner {
    pub(super) ptr: NonNull<KernelEndpointHeader>,
    pub(super) handle: EndpointHandle,
    _local_only: crate::local::LocalOnly,
}

impl EndpointOwner {
    pub(super) fn new(ptr: NonNull<KernelEndpointHeader>, handle: EndpointHandle) -> Self {
        Self {
            ptr,
            handle,
            _local_only: crate::local::LocalOnly::new(),
        }
    }

    /// # Safety
    /// The caller must retain this owner's resident actor and runtime for 'r.
    /// Only attachment or a completed owned receive may select that lifetime.
    pub(super) unsafe fn into_endpoint<'r, const ROLE: u8>(
        self,
    ) -> Result<Endpoint<'r, ROLE>, Self> {
        // SAFETY: only the rendezvous-installed live owner constructs this
        // value. Its unique lease keeps the resident header pinned for 'r.
        let header = unsafe { self.ptr.as_ref() };
        if header.role() != ROLE || !self.handle.matches_header(header) {
            return Err(self);
        }
        Ok(Endpoint {
            owner: self,
            _borrow: core::marker::PhantomData,
        })
    }
}

impl Drop for EndpointOwner {
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
