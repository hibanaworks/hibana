use super::{Endpoint, EndpointError, EndpointOp, RecvResult, RouteBranch, carrier};
use crate::transport::wire::{CodecError, Payload, WirePayload};
use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

pub(crate) struct RawOfferFuture<'e, 'r, const ROLE: u8> {
    pub(super) endpoint: *mut Endpoint<'r, ROLE>,
    pub(super) lease: OfferFutureLease,
    pub(super) _borrow: core::marker::PhantomData<&'e mut Endpoint<'r, ROLE>>,
}

pub(crate) struct OfferFuture<'e, 'r, const ROLE: u8> {
    pub(super) raw: RawOfferFuture<'e, 'r, ROLE>,
}

pub(crate) struct RawBranchRecvFuture<'e, 'r, const ROLE: u8> {
    endpoint: *mut Endpoint<'r, ROLE>,
    lease: crate::endpoint::kernel::PublicOpLease,
    progress: BranchRecvFutureProgress,
    _borrow: core::marker::PhantomData<&'e mut Endpoint<'r, ROLE>>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum BranchRecvFutureProgress {
    Pending,
    Finished,
}

pub(crate) struct BranchRecvFuture<'e, 'r, const ROLE: u8, M>
where
    M: crate::g::Message,
{
    raw: RawBranchRecvFuture<'e, 'r, ROLE>,
    _msg: core::marker::PhantomData<M>,
}

pub(crate) struct RawRecvFuture<'e, 'r, const ROLE: u8> {
    endpoint: *mut Endpoint<'r, ROLE>,
    lease: RecvFutureLease,
    _borrow: core::marker::PhantomData<&'e mut Endpoint<'r, ROLE>>,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum OfferFutureLease {
    Rejected = 0,
    RestoreOnDrop = 1,
    Completed = 2,
}

impl OfferFutureLease {
    #[inline]
    pub(super) const fn from_public_lease(lease: crate::endpoint::kernel::PublicOpLease) -> Self {
        match lease {
            crate::endpoint::kernel::PublicOpLease::Held => Self::RestoreOnDrop,
            crate::endpoint::kernel::PublicOpLease::Rejected => Self::Rejected,
        }
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RecvFutureLease {
    Rejected = 0,
    RestoreOnDrop = 1,
    Completed = 2,
}

impl RecvFutureLease {
    #[inline]
    pub(crate) const fn from_public_lease(lease: crate::endpoint::kernel::PublicOpLease) -> Self {
        match lease {
            crate::endpoint::kernel::PublicOpLease::Held => Self::RestoreOnDrop,
            crate::endpoint::kernel::PublicOpLease::Rejected => Self::Rejected,
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

impl<'e, 'r, const ROLE: u8> RawBranchRecvFuture<'e, 'r, ROLE> {
    #[inline]
    fn new(branch: RouteBranch<'e, 'r, ROLE>) -> Self {
        let branch = core::mem::ManuallyDrop::new(branch);
        let endpoint = branch.endpoint;
        /* SAFETY: consuming `RouteBranch` transfers its route preview borrow to
        this branch-recv future. Beginning branch recv arms the public
        branch-recv state once on the same endpoint pointer carried by the
        branch. */
        let lease = unsafe { (&mut *endpoint).begin_public_branch_recv_state() };
        Self {
            endpoint,
            lease,
            progress: BranchRecvFutureProgress::Pending,
            _borrow: core::marker::PhantomData,
        }
    }

    #[inline]
    fn finish(&mut self) {
        self.progress = BranchRecvFutureProgress::Finished;
    }

    #[inline]
    fn poll_raw(
        &mut self,
        logical_label: u8,
        validate: for<'a> fn(Payload<'a>) -> Result<(), CodecError>,
        cx: &mut Context<'_>,
    ) -> Poll<RecvResult<carrier::RawPayload>> {
        if self.progress == BranchRecvFutureProgress::Finished {
            crate::invariant();
        }
        match self.lease {
            crate::endpoint::kernel::PublicOpLease::Held => {}
            crate::endpoint::kernel::PublicOpLease::Rejected => {
                self.finish();
                return Poll::Ready(Err(crate::endpoint::RecvError::PhaseInvariant));
            }
        }
        let endpoint = /* SAFETY: this branch-recv future holds the public branch-recv
        lease while `progress` is Pending. Polling owns `&mut self`, so no
        second endpoint operation can borrow the same branch-recv state. */ unsafe { &mut *self.endpoint };
        match endpoint.poll_branch_recv(logical_label, validate, cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(payload)) => {
                self.finish();
                Poll::Ready(Ok(payload))
            }
            Poll::Ready(Err(err)) => {
                self.finish();
                Poll::Ready(Err(err))
            }
        }
    }
}

impl<'e, 'r, const ROLE: u8> RawRecvFuture<'e, 'r, ROLE> {
    #[inline]
    fn new(endpoint: &'e mut Endpoint<'r, ROLE>) -> Self {
        let lease = RecvFutureLease::from_public_lease(endpoint.init_public_recv_state());
        Self {
            endpoint: core::ptr::from_mut(endpoint),
            lease,
            _borrow: core::marker::PhantomData,
        }
    }

    #[inline]
    fn poll_raw(
        &mut self,
        logical_label: u8,
        validate: for<'a> fn(Payload<'a>) -> Result<(), CodecError>,
        cx: &mut Context<'_>,
    ) -> Poll<RecvResult<carrier::RawPayload>> {
        match self.lease {
            RecvFutureLease::Completed => crate::invariant(),
            RecvFutureLease::Rejected => {
                self.lease = RecvFutureLease::Completed;
                return Poll::Ready(Err(crate::endpoint::RecvError::PhaseInvariant));
            }
            RecvFutureLease::RestoreOnDrop => {}
        }
        let endpoint = /* SAFETY: this recv future holds the public recv lease
        while it is in `RestoreOnDrop`. Polling owns `&mut self`, which keeps
        the endpoint recv operation exclusive. */ unsafe { &mut *self.endpoint };
        match endpoint.poll_recv(logical_label, validate, cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(payload)) => {
                self.lease = RecvFutureLease::Completed;
                Poll::Ready(Ok(payload))
            }
            Poll::Ready(Err(err)) => {
                self.lease = RecvFutureLease::Completed;
                Poll::Ready(Err(err))
            }
        }
    }
}

impl<'e, 'r, const ROLE: u8, M> BranchRecvFuture<'e, 'r, ROLE, M>
where
    M: crate::g::Message,
{
    #[inline]
    pub(super) fn new(branch: RouteBranch<'e, 'r, ROLE>) -> Self {
        Self {
            raw: RawBranchRecvFuture::new(branch),
            _msg: core::marker::PhantomData,
        }
    }
}

impl<'e, 'r, const ROLE: u8, M> RecvFuture<'e, 'r, ROLE, M>
where
    M: crate::g::Message,
    M::Payload: WirePayload,
{
    #[inline]
    pub(super) fn new(endpoint: &'e mut Endpoint<'r, ROLE>) -> Self {
        Self {
            raw: RawRecvFuture::new(endpoint),
            _msg: core::marker::PhantomData,
        }
    }
}

impl<'e, 'r, const ROLE: u8, M> Future for BranchRecvFuture<'e, 'r, ROLE, M>
where
    M: crate::g::Message,
    M::Payload: WirePayload,
{
    type Output = core::result::Result<<M::Payload as WirePayload>::Decoded<'e>, EndpointError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = /* SAFETY: these futures are never structurally pinned; the raw endpoint future remains pinned by endpoint ownership, not by this facade. */ unsafe { self.get_unchecked_mut() };
        match this.raw.poll_raw(
            <M as crate::g::Message>::LOGICAL_LABEL,
            <M::Payload as WirePayload>::validate_payload,
            cx,
        ) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(payload)) => {
                let payload: Payload<'e> = /* SAFETY: the endpoint future owns the in-flight kernel borrow until Ready or Drop resolves the operation. */ unsafe { payload.into_payload() };
                let decoded = <M::Payload as WirePayload>::decode_validated_payload(payload);
                Poll::Ready(Ok(decoded))
            }
            Poll::Ready(Err(err)) => Poll::Ready(Err(EndpointError::new(EndpointOp::Recv, err))),
        }
    }
}

impl<'e, 'r, const ROLE: u8, M> Future for RecvFuture<'e, 'r, ROLE, M>
where
    M: crate::g::Message,
    M::Payload: WirePayload,
{
    type Output = core::result::Result<<M::Payload as WirePayload>::Decoded<'e>, EndpointError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = /* SAFETY: these futures are never structurally pinned; the raw endpoint future remains pinned by endpoint ownership, not by this facade. */ unsafe { self.get_unchecked_mut() };
        match this.raw.poll_raw(
            <M as crate::g::Message>::LOGICAL_LABEL,
            <M::Payload as WirePayload>::validate_payload,
            cx,
        ) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(payload)) => {
                let payload: Payload<'e> = /* SAFETY: the endpoint future owns the in-flight kernel borrow until Ready or Drop resolves the operation. */ unsafe { payload.into_payload() };
                let decoded = <M::Payload as WirePayload>::decode_validated_payload(payload);
                Poll::Ready(Ok(decoded))
            }
            Poll::Ready(Err(err)) => Poll::Ready(Err(EndpointError::new(EndpointOp::Recv, err))),
        }
    }
}

impl<'e, 'r, const ROLE: u8> Drop for RawBranchRecvFuture<'e, 'r, ROLE> {
    fn drop(&mut self) {
        match (self.lease, self.progress) {
            (crate::endpoint::kernel::PublicOpLease::Held, BranchRecvFutureProgress::Pending) => {
                /* SAFETY: pending branch recv still owns the public branch-recv
                lease. Drop disarms that state once before the route branch
                preview can be offered again. */
                unsafe {
                    (&mut *self.endpoint).reset_public_branch_recv_state();
                }
            }
            (crate::endpoint::kernel::PublicOpLease::Held, BranchRecvFutureProgress::Finished)
            | (crate::endpoint::kernel::PublicOpLease::Rejected, _) => {}
        }
    }
}

impl<'e, 'r, const ROLE: u8> Drop for RawRecvFuture<'e, 'r, ROLE> {
    fn drop(&mut self) {
        if self.lease == RecvFutureLease::RestoreOnDrop {
            /* SAFETY: pending recv still owns the public recv lease. Drop
            resets that endpoint state exactly once because `self.lease` has not
            moved to Completed. */
            unsafe {
                (&mut *self.endpoint).reset_public_recv_state();
            }
        }
    }
}
