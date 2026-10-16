//! Affine view transfer inside the actor's existing commit transaction.
//! Carrier tags are local validation evidence; they are never serialized.
use super::{
    carrier::{EndpointHandle, KernelEndpointHeader},
    owner::EndpointOwner,
};
use core::ptr::NonNull;
mod futures;
mod payload;

#[derive(Clone, Copy)]
pub(crate) struct EndpointTransfer {
    pub(crate) ptr: NonNull<KernelEndpointHeader>,
    pub(crate) handle: EndpointHandle,
}

impl EndpointTransfer {
    pub(super) fn borrowed(owner: &EndpointOwner) -> Self {
        Self {
            ptr: owner.ptr,
            handle: owner.handle,
        }
    }
}

pub(crate) enum RecvOutput<'r> {
    Payload(crate::transport::wire::Payload<'r>),
    Endpoint(EndpointOwner),
}

impl<'r> RecvOutput<'r> {
    pub(crate) unsafe fn into_resident(self) -> Self {
        match self {
            Self::Payload(payload) => Self::Payload(unsafe {
                // SAFETY: the caller owns the endpoint-resident frame borrow.
                super::kernel::lane_port::endpoint_resident_payload(payload)
            }),
            Self::Endpoint(owner) => Self::Endpoint(owner),
        }
    }
}
