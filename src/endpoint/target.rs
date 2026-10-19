//! A borrowed operation target, distinct from the uniquely owned Endpoint.
//!
//! Parallel arms retain only a source region and an exclusive parent borrow.
//! All targets dispatch through the same carrier and the same resident cursor.

use super::{
    Endpoint,
    carrier::{EndpointHandle, KernelEndpointHeader, OperationHandle, OperationRegion},
};
use core::{marker::PhantomData, ptr::NonNull};

pub(super) struct BorrowedEndpoint {
    pub(super) ptr: NonNull<KernelEndpointHeader>,
    pub(super) handle: EndpointHandle,
    pub(super) region: OperationRegion,
}

enum TargetKind<'r, const ROLE: u8, D> {
    Endpoint(*mut Endpoint<'r, ROLE>, D),
    Parallel(*mut BorrowedEndpoint, D),
}

// Public only inside this private module, so the sealed payload dispatch trait
// can name its argument without exposing a new application capability.
pub struct OperationTarget<'e, 'r, const ROLE: u8, D = ()> {
    kind: TargetKind<'r, ROLE, D>,
    _borrow: PhantomData<&'e mut Endpoint<'r, ROLE>>,
}

impl<'e, 'r, const ROLE: u8> OperationTarget<'e, 'r, ROLE> {
    pub(super) fn new(endpoint: &'e mut Endpoint<'r, ROLE>) -> Self {
        Self {
            kind: TargetKind::Endpoint(endpoint, ()),
            _borrow: PhantomData,
        }
    }

    pub(super) fn parallel(endpoint: &'e mut BorrowedEndpoint) -> Self {
        Self {
            kind: TargetKind::Parallel(endpoint, ()),
            _borrow: PhantomData,
        }
    }
}

impl<'e, 'r, const ROLE: u8, D> OperationTarget<'e, 'r, ROLE, D> {
    pub(super) fn metadata(&self) -> &D {
        match &self.kind {
            TargetKind::Endpoint(_, value) | TargetKind::Parallel(_, value) => value,
        }
    }

    pub(super) fn metadata_mut(&mut self) -> &mut D {
        match &mut self.kind {
            TargetKind::Endpoint(_, value) | TargetKind::Parallel(_, value) => value,
        }
    }

    pub(super) fn with_metadata<N>(self, value: N) -> OperationTarget<'e, 'r, ROLE, N> {
        let kind = match self.kind {
            TargetKind::Endpoint(ptr, _) => TargetKind::Endpoint(ptr, value),
            TargetKind::Parallel(ptr, _) => TargetKind::Parallel(ptr, value),
        };
        OperationTarget {
            kind,
            _borrow: PhantomData,
        }
    }

    /// Copy only a completed preview's borrow into its returned branch.
    /// The caller must ensure the old future will neither poll nor restore it.
    pub(super) unsafe fn completed_preview(&self, label: u8) -> OperationTarget<'e, 'r, ROLE, u8> {
        let kind = match self.kind {
            TargetKind::Endpoint(ptr, _) => TargetKind::Endpoint(ptr, label),
            TargetKind::Parallel(ptr, _) => TargetKind::Parallel(ptr, label),
        };
        OperationTarget {
            kind,
            _borrow: PhantomData,
        }
    }

    pub(super) fn parts(&self) -> (NonNull<KernelEndpointHeader>, OperationHandle) {
        match self.kind {
            TargetKind::Endpoint(ptr, _) => {
                // SAFETY: the target retains this Endpoint's unique operation
                // borrow. Only immutable owner metadata is read here.
                let endpoint = unsafe { &*ptr };
                (
                    endpoint.owner.ptr,
                    OperationHandle {
                        endpoint: endpoint.owner.handle,
                        region: OperationRegion::WHOLE,
                    },
                )
            }
            TargetKind::Parallel(ptr, _) => {
                // SAFETY: the target retains the exclusive arm borrow, which
                // cannot outlive its parent Endpoint's exclusive borrow.
                let arm = unsafe { &*ptr };
                (
                    arm.ptr,
                    OperationHandle {
                        endpoint: arm.handle,
                        region: arm.region,
                    },
                )
            }
        }
    }
}
