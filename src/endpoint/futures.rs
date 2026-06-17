use super::{
    Endpoint, EndpointError, EndpointOp, EndpointResult, RecvResult, RouteBranch, carrier,
};
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

pub(crate) struct RawDecodeFuture<'e, 'r, const ROLE: u8> {
    endpoint: *mut Endpoint<'r, ROLE>,
    lease: crate::endpoint::kernel::PublicOpLease,
    progress: DecodeFutureProgress,
    _borrow: core::marker::PhantomData<&'e mut Endpoint<'r, ROLE>>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum DecodeFutureProgress {
    Pending,
    Finished,
}

pub(crate) struct DecodeFuture<'e, 'r, const ROLE: u8, M>
where
    M: crate::g::Message,
{
    raw: RawDecodeFuture<'e, 'r, ROLE>,
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

impl<'e, 'r, const ROLE: u8> RawDecodeFuture<'e, 'r, ROLE> {
    #[inline]
    fn new(branch: RouteBranch<'e, 'r, ROLE>) -> Self {
        let branch = core::mem::ManuallyDrop::new(branch);
        let endpoint = branch.endpoint;
        /* SAFETY: the pointer comes from pinned owner storage and this path holds unique mutable access for the borrow. */
        let lease = unsafe { (&mut *endpoint).begin_public_decode_state() };
        Self {
            endpoint,
            lease,
            progress: DecodeFutureProgress::Pending,
            _borrow: core::marker::PhantomData,
        }
    }

    #[inline]
    fn finish(&mut self) {
        self.progress = DecodeFutureProgress::Finished;
    }

    #[inline]
    fn poll_raw(
        &mut self,
        logical_label: u8,
        validate: for<'a> fn(Payload<'a>) -> Result<(), CodecError>,
        cx: &mut Context<'_>,
    ) -> Poll<RecvResult<carrier::RawPayload>> {
        if self.progress == DecodeFutureProgress::Finished {
            crate::invariant();
        }
        match self.lease {
            crate::endpoint::kernel::PublicOpLease::Held => {}
            crate::endpoint::kernel::PublicOpLease::Rejected => {
                self.finish();
                return Poll::Ready(Err(crate::endpoint::RecvError::PhaseInvariant));
            }
        }
        let endpoint = /* SAFETY: the pointer comes from pinned owner storage and this path holds the unique mutable access for the borrow. */ unsafe { &mut *self.endpoint };
        match endpoint.poll_decode(logical_label, validate, cx) {
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
        /* SAFETY: the endpoint future owns the in-flight kernel borrow until Ready or Drop resolves the operation. */
        let lease =
            RecvFutureLease::from_public_lease(unsafe { endpoint.init_public_recv_state() });
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
        let endpoint = /* SAFETY: the pointer comes from pinned owner storage and this path holds the unique mutable access for the borrow. */ unsafe { &mut *self.endpoint };
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

impl<'e, 'r, const ROLE: u8, M> DecodeFuture<'e, 'r, ROLE, M>
where
    M: crate::g::Message,
{
    #[inline]
    pub(super) fn new(branch: RouteBranch<'e, 'r, ROLE>) -> Self {
        Self {
            raw: RawDecodeFuture::new(branch),
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

impl<'e, 'r, const ROLE: u8, M> Future for DecodeFuture<'e, 'r, ROLE, M>
where
    M: crate::g::Message,
    M::Payload: WirePayload,
{
    type Output = EndpointResult<<M::Payload as WirePayload>::Decoded<'e>>;

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
    type Output = EndpointResult<<M::Payload as WirePayload>::Decoded<'e>>;

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

impl<'e, 'r, const ROLE: u8> Drop for RawDecodeFuture<'e, 'r, ROLE> {
    fn drop(&mut self) {
        match (self.lease, self.progress) {
            (crate::endpoint::kernel::PublicOpLease::Held, DecodeFutureProgress::Pending) => {
                /* SAFETY: the pointer comes from pinned owner storage and this path holds unique mutable access for the borrow. */
                unsafe {
                    (&mut *self.endpoint).reset_public_decode_state();
                }
            }
            (crate::endpoint::kernel::PublicOpLease::Held, DecodeFutureProgress::Finished)
            | (crate::endpoint::kernel::PublicOpLease::Rejected, _) => {}
        }
    }
}

impl<'e, 'r, const ROLE: u8> Drop for RawRecvFuture<'e, 'r, ROLE> {
    fn drop(&mut self) {
        if self.lease == RecvFutureLease::RestoreOnDrop {
            /* SAFETY: the pointer comes from pinned owner storage and this path holds unique mutable access for the borrow. */
            unsafe {
                (&mut *self.endpoint).reset_public_recv_state();
            }
        }
    }
}
