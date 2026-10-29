use super::{EndpointError, EndpointOp, RecvResult, RouteBranch, carrier};
use crate::transport::wire::{CodecError, Payload, ReceivedPayload, WireEncode, WirePayload};
use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

pub(crate) struct RawOfferFuture<'e, 'r, const ROLE: u8> {
    pub(super) endpoint: super::target::OperationTarget<'e, 'r, ROLE, OfferFutureLease>,
}

pub(crate) struct OfferFuture<'e, 'r, const ROLE: u8> {
    pub(super) raw: RawOfferFuture<'e, 'r, ROLE>,
}

pub(crate) struct BranchRecvFuture<'e, 'r, const ROLE: u8, M>
where
    M: crate::g::Message,
{
    raw: RawRecvFuture<'e, 'r, ROLE>,
    _msg: core::marker::PhantomData<M>,
}

pub(crate) struct RawRecvFuture<'e, 'r, const ROLE: u8> {
    endpoint: super::target::OperationTarget<'e, 'r, ROLE, RecvFutureLease>,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum OfferFutureLease {
    Rejected = 0,
    RestoreOnDrop = 1,
    Completed = 2,
    Faulted = 3,
}

impl OfferFutureLease {
    #[inline]
    pub(super) const fn from_public_lease(lease: crate::endpoint::kernel::PublicOpLease) -> Self {
        match lease {
            crate::endpoint::kernel::PublicOpLease::Held => Self::RestoreOnDrop,
            crate::endpoint::kernel::PublicOpLease::Rejected => Self::Rejected,
            crate::endpoint::kernel::PublicOpLease::Faulted => Self::Faulted,
        }
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RecvFutureLease {
    Rejected = 0,
    RestoreOnDrop = 1,
    Completed = 2,
    Faulted = 3,
    BranchRestoreOnDrop = 4,
    BranchFaulted = 5,
}

impl RecvFutureLease {
    #[inline]
    pub(crate) const fn from_public_lease(lease: crate::endpoint::kernel::PublicOpLease) -> Self {
        match lease {
            crate::endpoint::kernel::PublicOpLease::Held => Self::RestoreOnDrop,
            crate::endpoint::kernel::PublicOpLease::Rejected => Self::Rejected,
            crate::endpoint::kernel::PublicOpLease::Faulted => Self::Faulted,
        }
    }
}

pub(crate) struct RecvFuture<'e, 'r, const ROLE: u8, M>
where
    M: crate::g::Message,
{
    raw: RawRecvFuture<'e, 'r, ROLE>,
    _msg: core::marker::PhantomData<M>,
}

impl<'e, 'r, const ROLE: u8> RawRecvFuture<'e, 'r, ROLE> {
    #[inline]
    pub(super) fn from_branch(branch: RouteBranch<'e, 'r, ROLE>) -> Self {
        let branch = core::mem::ManuallyDrop::new(branch);
        // SAFETY: the consumed preview transfers its sole borrow to this future.
        let mut endpoint = unsafe { core::ptr::read(&branch.target) }.with_metadata(());
        /* SAFETY: consuming `RouteBranch` transfers its route preview borrow to
        this branch-recv future. Beginning branch recv arms the public
        branch-recv state once on the same endpoint pointer carried by the
        branch. */
        let lease = match endpoint.begin_public_branch_recv_state() {
            crate::endpoint::kernel::PublicOpLease::Held => RecvFutureLease::BranchRestoreOnDrop,
            crate::endpoint::kernel::PublicOpLease::Rejected => RecvFutureLease::Rejected,
            crate::endpoint::kernel::PublicOpLease::Faulted => RecvFutureLease::BranchFaulted,
        };
        Self {
            endpoint: endpoint.with_metadata(lease),
        }
    }
    #[inline]
    pub(super) fn new(mut endpoint: super::target::OperationTarget<'e, 'r, ROLE>) -> Self {
        let lease = RecvFutureLease::from_public_lease(endpoint.init_public_recv_state());
        Self {
            endpoint: endpoint.with_metadata(lease),
        }
    }

    #[inline]
    pub(super) fn poll_raw(
        &mut self,
        logical_label: u8,
        payload_kind: crate::eff::EventPayload,
        validate: for<'a> fn(Payload<'a>) -> Result<(), CodecError>,
        cx: &mut Context<'_>,
    ) -> Poll<RecvResult<carrier::RawRecvOutput>> {
        let result = match *self.endpoint.metadata() {
            RecvFutureLease::Completed => crate::invariant(),
            RecvFutureLease::Rejected => {
                *self.endpoint.metadata_mut() = RecvFutureLease::Completed;
                return Poll::Ready(Err(crate::endpoint::RecvError::PhaseInvariant));
            }
            RecvFutureLease::RestoreOnDrop | RecvFutureLease::Faulted => {
                // SAFETY: the unique endpoint borrow is retained. The lease
                // owns direct receive state or makes one terminal observation.
                self.endpoint
                    .poll_recv(logical_label, payload_kind, validate, cx)
            }
            RecvFutureLease::BranchRestoreOnDrop | RecvFutureLease::BranchFaulted => {
                // SAFETY: consuming the branch retained the same unique borrow
                // for branch receive state or one terminal fault observation.
                self.endpoint
                    .poll_branch_recv(logical_label, payload_kind, validate, cx)
            }
        };
        if result.is_ready() {
            *self.endpoint.metadata_mut() = RecvFutureLease::Completed;
        }
        result
    }
}

impl<'e, 'r, const ROLE: u8, M> BranchRecvFuture<'e, 'r, ROLE, M>
where
    M: crate::g::Message,
{
    #[inline]
    pub(super) fn new(branch: RouteBranch<'e, 'r, ROLE>) -> Self {
        Self {
            raw: RawRecvFuture::from_branch(branch),
            _msg: core::marker::PhantomData,
        }
    }
}

impl<'e, 'r, const ROLE: u8, M> RecvFuture<'e, 'r, ROLE, M>
where
    M: crate::g::Message,
    M::Payload: WireEncode + WirePayload,
{
    #[inline]
    pub(super) fn new(endpoint: super::target::OperationTarget<'e, 'r, ROLE>) -> Self {
        Self {
            raw: RawRecvFuture::new(endpoint),
            _msg: core::marker::PhantomData,
        }
    }
}

impl<'e, 'r, const ROLE: u8, M> Future for BranchRecvFuture<'e, 'r, ROLE, M>
where
    M: crate::g::Message,
    M::Payload: WireEncode + WirePayload,
{
    type Output = core::result::Result<<M::Payload as WirePayload>::Decoded<'e>, EndpointError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = /* SAFETY: this facade's initialized fields are not structurally
        pinned; endpoint ownership keeps the raw future valid and excludes an
        aliasing mutable poll while this facade owns `Pin<&mut Self>`. */ unsafe {
            self.get_unchecked_mut()
        };
        match this.raw.poll_raw(
            <M as crate::g::Message>::LOGICAL_LABEL,
            crate::global::payload_kind::<M>(),
            <M::Payload as WirePayload>::validate_payload,
            cx,
        ) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(payload)) => {
                let payload: Payload<'e> = /* SAFETY: the endpoint future owns the in-flight kernel borrow until Ready or Drop resolves the operation. */ unsafe { payload.into_payload() };
                let decoded = <M::Payload as WirePayload>::decode_validated_payload(
                    ReceivedPayload::new(payload),
                );
                Poll::Ready(Ok(decoded))
            }
            Poll::Ready(Err(err)) => Poll::Ready(Err(EndpointError::new(EndpointOp::Recv, err))),
        }
    }
}

impl<'e, 'r, const ROLE: u8, M> Future for RecvFuture<'e, 'r, ROLE, M>
where
    M: crate::g::Message,
    M::Payload: WireEncode + WirePayload,
{
    type Output = core::result::Result<<M::Payload as WirePayload>::Decoded<'e>, EndpointError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = /* SAFETY: this facade's initialized fields are not structurally
        pinned; endpoint ownership keeps the raw future valid and excludes an
        aliasing mutable poll while this facade owns `Pin<&mut Self>`. */ unsafe {
            self.get_unchecked_mut()
        };
        match this.raw.poll_raw(
            <M as crate::g::Message>::LOGICAL_LABEL,
            crate::global::payload_kind::<M>(),
            <M::Payload as WirePayload>::validate_payload,
            cx,
        ) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(payload)) => {
                let payload: Payload<'e> = /* SAFETY: the endpoint future owns the in-flight kernel borrow until Ready or Drop resolves the operation. */ unsafe { payload.into_payload() };
                let decoded = <M::Payload as WirePayload>::decode_validated_payload(
                    ReceivedPayload::new(payload),
                );
                Poll::Ready(Ok(decoded))
            }
            Poll::Ready(Err(err)) => Poll::Ready(Err(EndpointError::new(EndpointOp::Recv, err))),
        }
    }
}

impl<'e, 'r, const ROLE: u8> Drop for RawRecvFuture<'e, 'r, ROLE> {
    fn drop(&mut self) {
        match *self.endpoint.metadata() {
            RecvFutureLease::RestoreOnDrop | RecvFutureLease::BranchRestoreOnDrop => {
                // This future retains its direct or branch receive lease.
                self.endpoint.release_public_operation();
            }
            RecvFutureLease::Rejected
            | RecvFutureLease::Completed
            | RecvFutureLease::Faulted
            | RecvFutureLease::BranchFaulted => {}
        }
    }
}
