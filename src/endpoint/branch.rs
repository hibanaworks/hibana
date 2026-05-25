use super::{
    Endpoint, EndpointError, EndpointOp, EndpointResult, ErrorLocation, RecvResult, RouteBranch,
    futures::{DecodeFuture, OfferFuture, RawOfferFuture},
};
use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

impl<'e, 'r, const ROLE: u8> RouteBranch<'e, 'r, ROLE> {
    #[inline]
    pub(crate) fn from_parts(endpoint: *mut Endpoint<'r, ROLE>, label: u8) -> Self {
        Self {
            endpoint,
            label,
            _borrow: core::marker::PhantomData,
            _local_only: crate::local::LocalOnly::new(),
        }
    }

    #[inline]
    /// Return the selected choreography label for this route branch.
    pub fn label(&self) -> u8 {
        self.label
    }

    #[inline]
    /// Receive the first payload of a selected route arm.
    ///
    /// This consumes the branch preview on success. The message `M` must match
    /// the selected branch label and control kind. Physical frame-label or
    /// descriptor mismatches are reported as invariant failures, not as route
    /// choices. A decode failure is terminal for the current generation.
    #[track_caller]
    pub fn decode<M>(
        self,
    ) -> impl core::future::Future<
        Output = EndpointResult<<<M as crate::global::MessageSpec>::Payload as crate::transport::wire::WirePayload>::Decoded<'e>>,
    > + use<'e, 'r, M, ROLE>
    where
        M: crate::global::MessageSpec,
        M::Payload: crate::transport::wire::WirePayload,
    {
        DecodeFuture::<'e, 'r, ROLE, M>::new(self, ErrorLocation::caller())
    }
}

impl<'r, const ROLE: u8> Drop for Endpoint<'r, ROLE> {
    fn drop(&mut self) {
        /* SAFETY: the endpoint future owns the in-flight kernel borrow until Ready or Drop resolves the operation. */
        unsafe {
            self.drop_kernel_endpoint();
        }
    }
}

impl<'e, 'r, const ROLE: u8> Drop for RouteBranch<'e, 'r, ROLE> {
    fn drop(&mut self) {
        /* SAFETY: the pointer comes from pinned owner storage and this path holds unique mutable access for the borrow. */
        unsafe {
            (&mut *self.endpoint).restore_public_route_branch();
        }
    }
}

impl<'e, 'r, const ROLE: u8> RawOfferFuture<'e, 'r, ROLE> {
    #[inline]
    pub(super) fn new(endpoint: &'e mut Endpoint<'r, ROLE>) -> Self {
        let endpoint_ptr = core::ptr::from_mut(endpoint);
        Self {
            endpoint: endpoint_ptr,
            completed: false,
            _borrow: core::marker::PhantomData,
        }
    }

    #[inline]
    pub(super) fn poll_raw(&mut self, cx: &mut Context<'_>) -> Poll<RecvResult<u8>> {
        if self.completed {
            panic!("completed offer future polled after Ready");
        }
        match /* SAFETY: the pointer comes from pinned owner storage and this path holds the unique mutable access for the borrow. */ unsafe { (&mut *self.endpoint).poll_offer(cx) } {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Err(err)) => {
                self.completed = true;
                Poll::Ready(Err(err))
            }
            Poll::Ready(Ok(label)) => {
                self.completed = true;
                Poll::Ready(Ok(label))
            }
        }
    }
}

impl<'e, 'r, const ROLE: u8> OfferFuture<'e, 'r, ROLE> {
    #[inline]
    pub(super) fn new(endpoint: &'e mut Endpoint<'r, ROLE>, location: ErrorLocation) -> Self {
        Self {
            raw: RawOfferFuture::new(endpoint),
            location,
        }
    }
}

impl<'e, 'r, const ROLE: u8> Future for OfferFuture<'e, 'r, ROLE> {
    type Output = EndpointResult<RouteBranch<'e, 'r, ROLE>>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        match this.raw.poll_raw(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Err(err)) => Poll::Ready(Err(EndpointError::new(
                EndpointOp::Offer,
                this.location,
                err,
            ))),
            Poll::Ready(Ok(label)) => {
                Poll::Ready(Ok(RouteBranch::from_parts(this.raw.endpoint, label)))
            }
        }
    }
}

impl<'e, 'r, const ROLE: u8> Drop for RawOfferFuture<'e, 'r, ROLE> {
    fn drop(&mut self) {
        if !self.completed {
            /* SAFETY: the pointer comes from pinned owner storage and this path holds unique mutable access for the borrow. */
            unsafe {
                (&mut *self.endpoint).reset_public_offer_state();
            }
        }
    }
}
