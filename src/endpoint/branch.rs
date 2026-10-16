use super::{
    Endpoint, EndpointError, EndpointOp, RecvResult, RouteBranch,
    futures::{OfferFuture, OfferFutureLease, RawOfferFuture},
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

    /// Return the selected arm's first logical label.
    ///
    /// This is not branch authority for resolved routes; resolver decisions own
    /// that authority before the branch is materialized.
    #[inline]
    pub fn label(&self) -> u8 {
        self.label
    }

    /// Receive the first payload of a selected route arm.
    ///
    /// This consumes the branch preview on success. The message `M` must match
    /// the selected branch label and payload family. Physical frame-label or
    /// descriptor mismatches are reported as invariant failures, not as route
    /// choices. A decode failure is terminal for the current generation.
    #[inline]
    pub fn recv<M>(
        self,
    ) -> impl core::future::Future<
        Output = Result<
            <M::Payload as crate::global::PayloadSpec>::Received<'e, 'r>,
            EndpointError,
        >,
    > + 'e
    where
        M: crate::g::Message + 'e,
        'r: 'e,
    {
        <M::Payload as crate::global::PayloadSpec>::branch_recv::<M, ROLE>(self)
    }

    /// Send the first payload of a selected route arm.
    ///
    /// This consumes the branch preview only when the selected arm begins with
    /// a send. Dropping the returned future before completion restores the
    /// branch preview so the route can be offered again. A descriptor mismatch
    /// is terminal for the current session generation.
    #[inline]
    pub fn send<'a, M>(
        self,
        payload: <M::Payload as crate::global::PayloadSpec>::SendArgument<'a, 'r>,
    ) -> impl core::future::Future<Output = Result<(), EndpointError>> + 'a + use<'a, 'e, 'r, M, ROLE>
    where
        M: crate::g::Message + 'a,
        M::Payload: 'a,
        'e: 'a,
        'r: 'e,
    {
        <M::Payload as crate::global::PayloadSpec>::branch_send::<M, ROLE>(self, payload)
    }
}

impl<'e, 'r, const ROLE: u8> Drop for RouteBranch<'e, 'r, ROLE> {
    fn drop(&mut self) {
        /* SAFETY: `RouteBranch` owns the route preview borrow produced by
        `offer`. Dropping an unconsumed branch restores that preview exactly
        once through the endpoint pointer carried by the branch. */
        unsafe {
            (&mut *self.endpoint).restore_public_route_branch();
        }
    }
}

impl<'e, 'r, const ROLE: u8> RawOfferFuture<'e, 'r, ROLE> {
    #[inline]
    pub(super) fn new(endpoint: &'e mut Endpoint<'r, ROLE>) -> Self {
        let lease = endpoint.init_public_offer_state();
        let endpoint_ptr = core::ptr::from_mut(endpoint);
        Self {
            endpoint: endpoint_ptr,
            lease: OfferFutureLease::from_public_lease(lease),
            _borrow: core::marker::PhantomData,
        }
    }

    #[inline]
    pub(super) fn poll_raw(&mut self, cx: &mut Context<'_>) -> Poll<RecvResult<u8>> {
        match self.lease {
            OfferFutureLease::Completed => crate::invariant(),
            OfferFutureLease::Rejected => {
                self.lease = OfferFutureLease::Completed;
                return Poll::Ready(Err(crate::endpoint::RecvError::PhaseInvariant));
            }
            OfferFutureLease::RestoreOnDrop | OfferFutureLease::Faulted => {}
        }
        match /* SAFETY: `RawOfferFuture` owns the unique endpoint borrow. A
        restore-on-drop state holds the public offer lease; a faulted state
        performs one terminal fault observation without arming offer state. */ unsafe {
            (&mut *self.endpoint).poll_offer(cx)
        } {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Err(err)) => {
                self.lease = OfferFutureLease::Completed;
                Poll::Ready(Err(err))
            }
            Poll::Ready(Ok(label)) => {
                self.lease = OfferFutureLease::Completed;
                Poll::Ready(Ok(label))
            }
        }
    }
}

impl<'e, 'r, const ROLE: u8> OfferFuture<'e, 'r, ROLE> {
    #[inline]
    pub(super) fn new(endpoint: &'e mut Endpoint<'r, ROLE>) -> Self {
        Self {
            raw: RawOfferFuture::new(endpoint),
        }
    }
}

impl<'e, 'r, const ROLE: u8> Future for OfferFuture<'e, 'r, ROLE> {
    type Output = core::result::Result<RouteBranch<'e, 'r, ROLE>, EndpointError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        match this.raw.poll_raw(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Err(err)) => Poll::Ready(Err(EndpointError::new(EndpointOp::Offer, err))),
            Poll::Ready(Ok(label)) => {
                Poll::Ready(Ok(RouteBranch::from_parts(this.raw.endpoint, label)))
            }
        }
    }
}

impl<'e, 'r, const ROLE: u8> Drop for RawOfferFuture<'e, 'r, ROLE> {
    fn drop(&mut self) {
        if self.lease == OfferFutureLease::RestoreOnDrop {
            /* SAFETY: this offer future still owns the restore-on-drop lease.
            Dropping it releases the public offer state before another endpoint
            operation can borrow the same endpoint. */
            unsafe {
                (&mut *self.endpoint).reset_public_offer_state();
            }
        }
    }
}
