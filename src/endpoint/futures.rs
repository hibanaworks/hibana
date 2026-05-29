use super::{
    Endpoint, EndpointError, EndpointOp, EndpointResult, ErrorLocation, RecvResult, RouteBranch,
    carrier, synthetic_wire_payload, validate_wire_payload,
};
use crate::global::MessageRuntime;
use crate::transport::wire::{CodecError, Payload, WirePayload};
use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

pub(crate) struct RawOfferFuture<'e, 'r, const ROLE: u8> {
    pub(super) endpoint: *mut Endpoint<'r, ROLE>,
    pub(super) completed: bool,
    pub(super) _borrow: core::marker::PhantomData<&'e mut Endpoint<'r, ROLE>>,
}

pub(crate) struct OfferFuture<'e, 'r, const ROLE: u8> {
    pub(super) raw: RawOfferFuture<'e, 'r, ROLE>,
    pub(super) location: ErrorLocation,
}

pub(crate) struct RawDecodeFuture<'e, 'r, const ROLE: u8> {
    endpoint: *mut Endpoint<'r, ROLE>,
    completed: bool,
    _borrow: core::marker::PhantomData<&'e mut Endpoint<'r, ROLE>>,
}

pub(crate) struct DecodeFuture<'e, 'r, const ROLE: u8, M>
where
    M: crate::g::MessageSpec,
{
    raw: RawDecodeFuture<'e, 'r, ROLE>,
    location: ErrorLocation,
    _msg: core::marker::PhantomData<M>,
}

pub(crate) struct RawRecvFuture<'e, 'r, const ROLE: u8> {
    endpoint: *mut Endpoint<'r, ROLE>,
    flags: RawRecvFlags,
    _borrow: core::marker::PhantomData<&'e mut Endpoint<'r, ROLE>>,
}

#[derive(Clone, Copy)]
pub(crate) struct RawRecvFlags(u8);

impl RawRecvFlags {
    const COMPLETED: u8 = 1 << 0;
    const ACCEPTS_EMPTY_PAYLOAD: u8 = 1 << 1;

    #[inline]
    pub(crate) const fn new(accepts_empty_payload: bool) -> Self {
        Self(if accepts_empty_payload {
            Self::ACCEPTS_EMPTY_PAYLOAD
        } else {
            0
        })
    }

    #[inline]
    pub(crate) fn mark_completed(&mut self) {
        self.0 |= Self::COMPLETED;
    }

    #[inline]
    pub(crate) const fn completed(self) -> bool {
        self.0 & Self::COMPLETED != 0
    }

    #[inline]
    pub(crate) const fn accepts_empty_payload(self) -> bool {
        self.0 & Self::ACCEPTS_EMPTY_PAYLOAD != 0
    }
}

pub(crate) struct RecvFuture<'e, 'r, const ROLE: u8, M>
where
    M: crate::g::MessageSpec,
{
    raw: RawRecvFuture<'e, 'r, ROLE>,
    location: ErrorLocation,
    _msg: core::marker::PhantomData<M>,
}

impl<'e, 'r, const ROLE: u8> RawDecodeFuture<'e, 'r, ROLE> {
    #[inline]
    fn new(branch: RouteBranch<'e, 'r, ROLE>) -> Self {
        let branch = core::mem::ManuallyDrop::new(branch);
        let endpoint = branch.endpoint;
        /* SAFETY: the pointer comes from pinned owner storage and this path holds unique mutable access for the borrow. */
        unsafe {
            (&mut *endpoint).begin_public_decode_state();
        }
        Self {
            endpoint,
            completed: false,
            _borrow: core::marker::PhantomData,
        }
    }

    #[inline]
    fn poll_raw(
        &mut self,
        logical_label: u8,
        expects_control: bool,
        validate: for<'a> fn(Payload<'a>) -> Result<(), CodecError>,
        synthetic: for<'a> fn(&'a mut [u8]) -> Result<Payload<'a>, CodecError>,
        cx: &mut Context<'_>,
    ) -> Poll<RecvResult<carrier::RawPayload>> {
        if self.completed {
            panic!("completed decode future polled after Ready");
        }
        let endpoint = /* SAFETY: the pointer comes from pinned owner storage and this path holds the unique mutable access for the borrow. */ unsafe { &mut *self.endpoint };
        match endpoint.poll_decode(logical_label, expects_control, validate, synthetic, cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(payload)) => {
                self.completed = true;
                Poll::Ready(Ok(payload))
            }
            Poll::Ready(Err(err)) => {
                self.completed = true;
                Poll::Ready(Err(err))
            }
        }
    }
}

impl<'e, 'r, const ROLE: u8> RawRecvFuture<'e, 'r, ROLE> {
    #[inline]
    fn new(endpoint: &'e mut Endpoint<'r, ROLE>, accepts_empty_payload: bool) -> Self {
        /* SAFETY: the endpoint future owns the in-flight kernel borrow until Ready or Drop resolves the operation. */
        unsafe {
            endpoint.init_public_recv_state();
        }
        Self {
            endpoint: core::ptr::from_mut(endpoint),
            flags: RawRecvFlags::new(accepts_empty_payload),
            _borrow: core::marker::PhantomData,
        }
    }

    #[inline]
    fn poll_raw(
        &mut self,
        logical_label: u8,
        expects_control: bool,
        validate: for<'a> fn(Payload<'a>) -> Result<(), CodecError>,
        cx: &mut Context<'_>,
    ) -> Poll<RecvResult<carrier::RawPayload>> {
        if self.flags.completed() {
            panic!("completed recv future polled after Ready");
        }
        let endpoint = /* SAFETY: the pointer comes from pinned owner storage and this path holds the unique mutable access for the borrow. */ unsafe { &mut *self.endpoint };
        match endpoint.poll_recv(
            logical_label,
            expects_control,
            self.flags.accepts_empty_payload(),
            validate,
            cx,
        ) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(payload)) => {
                self.flags.mark_completed();
                Poll::Ready(Ok(payload))
            }
            Poll::Ready(Err(err)) => {
                self.flags.mark_completed();
                Poll::Ready(Err(err))
            }
        }
    }
}

impl<'e, 'r, const ROLE: u8, M> DecodeFuture<'e, 'r, ROLE, M>
where
    M: crate::g::MessageSpec,
{
    #[inline]
    pub(super) fn new(branch: RouteBranch<'e, 'r, ROLE>, location: ErrorLocation) -> Self {
        Self {
            raw: RawDecodeFuture::new(branch),
            location,
            _msg: core::marker::PhantomData,
        }
    }
}

impl<'e, 'r, const ROLE: u8, M> RecvFuture<'e, 'r, ROLE, M>
where
    M: crate::g::MessageSpec,
{
    #[inline]
    pub(super) fn new(endpoint: &'e mut Endpoint<'r, ROLE>, location: ErrorLocation) -> Self {
        let accepts_empty_payload = <M::Payload as WirePayload>::ACCEPTS_EMPTY_PAYLOAD;
        Self {
            raw: RawRecvFuture::new(endpoint, accepts_empty_payload),
            location,
            _msg: core::marker::PhantomData,
        }
    }
}

impl<'e, 'r, const ROLE: u8, M> Future for DecodeFuture<'e, 'r, ROLE, M>
where
    M: crate::g::MessageSpec,
{
    type Output = EndpointResult<M::Decoded<'e>>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = /* SAFETY: these futures are never structurally pinned; the raw endpoint future remains pinned by endpoint ownership, not by this wrapper. */ unsafe { self.get_unchecked_mut() };
        match this.raw.poll_raw(
            <M as crate::g::MessageSpec>::LOGICAL_LABEL,
            <M as MessageRuntime>::CONTROL_PAYLOAD,
            validate_wire_payload::<M::Payload>,
            synthetic_wire_payload::<M::Payload>,
            cx,
        ) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(payload)) => {
                let payload: Payload<'e> = /* SAFETY: the endpoint future owns the in-flight kernel borrow until Ready or Drop resolves the operation. */ unsafe { payload.into_payload() };
                let decoded = <M as MessageRuntime>::decode_validated_payload(payload);
                Poll::Ready(Ok(decoded))
            }
            Poll::Ready(Err(err)) => Poll::Ready(Err(EndpointError::new(
                EndpointOp::Decode,
                this.location,
                err,
            ))),
        }
    }
}

impl<'e, 'r, const ROLE: u8, M> Future for RecvFuture<'e, 'r, ROLE, M>
where
    M: crate::g::MessageSpec,
{
    type Output = EndpointResult<M::Decoded<'e>>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = /* SAFETY: these futures are never structurally pinned; the raw endpoint future remains pinned by endpoint ownership, not by this wrapper. */ unsafe { self.get_unchecked_mut() };
        match this.raw.poll_raw(
            <M as crate::g::MessageSpec>::LOGICAL_LABEL,
            <M as MessageRuntime>::CONTROL_PAYLOAD,
            validate_wire_payload::<M::Payload>,
            cx,
        ) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(payload)) => {
                let payload: Payload<'e> = /* SAFETY: the endpoint future owns the in-flight kernel borrow until Ready or Drop resolves the operation. */ unsafe { payload.into_payload() };
                let decoded = <M as MessageRuntime>::decode_validated_payload(payload);
                Poll::Ready(Ok(decoded))
            }
            Poll::Ready(Err(err)) => Poll::Ready(Err(EndpointError::new(
                EndpointOp::Recv,
                this.location,
                err,
            ))),
        }
    }
}

impl<'e, 'r, const ROLE: u8> Drop for RawDecodeFuture<'e, 'r, ROLE> {
    fn drop(&mut self) {
        if !self.completed {
            /* SAFETY: the pointer comes from pinned owner storage and this path holds unique mutable access for the borrow. */
            unsafe {
                (&mut *self.endpoint).reset_public_decode_state();
            }
        }
    }
}

impl<'e, 'r, const ROLE: u8> Drop for RawRecvFuture<'e, 'r, ROLE> {
    fn drop(&mut self) {
        if !self.flags.completed() {
            /* SAFETY: the pointer comes from pinned owner storage and this path holds unique mutable access for the borrow. */
            unsafe {
                (&mut *self.endpoint).reset_public_recv_state();
            }
        }
    }
}
