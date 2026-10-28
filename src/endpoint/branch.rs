use super::{
    EndpointError, EndpointOp, RecvResult, RouteBranch,
    futures::{OfferFuture, OfferFutureLease, RawOfferFuture},
};
use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

impl<'e, 'r, const ROLE: u8> RouteBranch<'e, 'r, ROLE> {
    #[inline]
    pub(crate) fn from_parts(target: super::target::OperationTarget<'e, 'r, ROLE, u8>) -> Self {
        Self { target }
    }

    /// Return the logical message label of the previewed operation.
    ///
    /// A branch can contain several independent operations. The label
    /// identifies the operation selected by this preview.
    ///
    /// Reading the label does not advance the protocol or invoke a resolver.
    #[inline]
    pub fn label(&self) -> u8 {
        *self.target.metadata()
    }

    /// Receive the payload of the previewed operation.
    ///
    /// On success, this consumes the preview and advances that operation.
    /// The label and payload format must match `M`. For a self-role unit step,
    /// it consumes `()` locally without receiving a frame. A rejected payload
    /// faults the local session run.
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

    /// Send the payload of the previewed operation.
    ///
    /// The preview must identify a send. On success, this advances that
    /// operation. Ordinary payloads are borrowed. An Endpoint payload moves
    /// into the future. Dropping the future does not advance the protocol or
    /// return a moved Endpoint. A protocol mismatch faults the local session run.
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
        self.target.release_public_operation();
    }
}

impl<'e, 'r, const ROLE: u8> RawOfferFuture<'e, 'r, ROLE> {
    #[inline]
    pub(super) fn new(mut endpoint: super::target::OperationTarget<'e, 'r, ROLE>) -> Self {
        let lease = endpoint.init_public_offer_state();
        Self {
            endpoint: endpoint.with_metadata(OfferFutureLease::from_public_lease(lease)),
        }
    }

    #[inline]
    pub(super) fn poll_raw(&mut self, cx: &mut Context<'_>) -> Poll<RecvResult<u8>> {
        match *self.endpoint.metadata() {
            OfferFutureLease::Completed => crate::invariant(),
            OfferFutureLease::Rejected => {
                *self.endpoint.metadata_mut() = OfferFutureLease::Completed;
                return Poll::Ready(Err(crate::endpoint::RecvError::PhaseInvariant));
            }
            OfferFutureLease::RestoreOnDrop | OfferFutureLease::Faulted => {}
        }
        match self.endpoint.poll_offer(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Err(err)) => {
                *self.endpoint.metadata_mut() = OfferFutureLease::Completed;
                Poll::Ready(Err(err))
            }
            Poll::Ready(Ok(label)) => {
                *self.endpoint.metadata_mut() = OfferFutureLease::Completed;
                Poll::Ready(Ok(label))
            }
        }
    }
}

impl<'e, 'r, const ROLE: u8> OfferFuture<'e, 'r, ROLE> {
    #[inline]
    pub(super) fn new(endpoint: super::target::OperationTarget<'e, 'r, ROLE>) -> Self {
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
                // SAFETY: the completed future will not poll or restore the preview.
                Poll::Ready(Ok(RouteBranch::from_parts(unsafe {
                    this.raw.endpoint.completed_preview(label)
                })))
            }
        }
    }
}

impl<'e, 'r, const ROLE: u8> Drop for RawOfferFuture<'e, 'r, ROLE> {
    fn drop(&mut self) {
        if *self.endpoint.metadata_mut() == OfferFutureLease::RestoreOnDrop {
            /* SAFETY: this offer future still owns the restore-on-drop lease.
            Dropping it releases the public offer state before another endpoint
            operation can borrow the same endpoint. */
            self.endpoint.release_public_operation();
        }
    }
}
