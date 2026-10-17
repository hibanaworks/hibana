//! Affine view transfer inside the actor's existing commit transaction.
//! Carrier tags are local validation evidence; they are never serialized.
use super::{
    carrier::{EndpointHandle, KernelEndpointHeader},
    owner::EndpointOwner,
};
use core::ptr::NonNull;
mod futures;
#[cfg(kani)]
mod kani;
mod payload;

/// Resource and source occurrence in the existing three-byte owned payload.
/// Actor identity, run isolation and affine delivery belong to the binding
/// and carrier; local Endpoint addresses and generations stay local.
pub(crate) const fn wire_envelope(resource: u8, event: u16) -> [u8; 3] {
    let ordinal = event.to_be_bytes();
    [resource, ordinal[0], ordinal[1]]
}

#[cfg(all(test, hibana_repo_tests))]
mod tests {
    #[test]
    fn owned_receive_future_stays_within_three_words() {
        use core::mem::size_of;
        let bytes = size_of::<super::futures::OwnedRecvFuture<'static, 'static, 0, 10>>();
        std::println!("owned-receive-future bytes={bytes}");
        assert!(bytes <= 3 * size_of::<usize>());
    }
}

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
