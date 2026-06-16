use super::{
    Endpoint, EndpointError, EndpointOp, EndpointResult, RecvResult, RouteBranch,
    futures::{DecodeFuture, OfferFuture, OfferFutureLease, RawOfferFuture},
    send::SendFuture,
};
use crate::diag::Callsite;
use crate::transport::wire::{WireEncode, WirePayload};
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

    /// Return the selected choreography label for this route branch.
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
    #[track_caller]
    pub fn recv<M>(
        self,
    ) -> impl core::future::Future<Output = EndpointResult<<M::Payload as WirePayload>::Decoded<'e>>>
    + use<'e, 'r, M, ROLE>
    where
        M: crate::g::Message,
        M::Payload: WirePayload,
    {
        DecodeFuture::<'e, 'r, ROLE, M>::new(self, Callsite::caller())
    }

    /// Send the first payload of a selected route arm.
    ///
    /// This consumes the branch preview only when the selected arm begins with
    /// a send. Dropping the returned future before completion restores the
    /// branch preview so the route can be offered again.
    #[inline]
    #[track_caller]
    pub fn send<'a, M>(
        self,
        payload: &'a M::Payload,
    ) -> impl core::future::Future<Output = EndpointResult<()>> + 'a + use<'a, 'e, 'r, M, ROLE>
    where
        M: crate::g::Message + 'a,
        M::Payload: WireEncode + 'a,
        'e: 'a,
        'r: 'a,
    {
        let branch = core::mem::ManuallyDrop::new(self);
        let endpoint = branch.endpoint;
        let location = Callsite::caller();
        let logical_label = <M as crate::g::Message>::LOGICAL_LABEL;
        let mut preview = core::mem::MaybeUninit::<crate::endpoint::kernel::SendPreview>::uninit();
        if let Err(error) =
            /* SAFETY: consuming the branch transfers its unique endpoint borrow into the returned future or a terminal ready error. */
            unsafe { (&mut *endpoint).preview_send(logical_label, preview.as_mut_ptr()) }
        {
            return SendFuture::ready_error(error, location);
        }
        let preview = /* SAFETY: preview_send returned Ok and initialized the out slot. */ unsafe {
            preview.assume_init()
        };
        let desc = crate::endpoint::send::send_runtime_desc::<M>(
            crate::transport::FrameLabel::new(preview.frame_label()),
        );
        let init = crate::endpoint::kernel::SendInit::new(desc, preview);
        match /* SAFETY: the consumed branch owns the in-flight kernel borrow until the returned future completes or drops. */ unsafe {
            (&mut *endpoint).init_public_send_state(&init)
        } {
            crate::endpoint::kernel::PublicOpLease::Held => {}
            crate::endpoint::kernel::PublicOpLease::Rejected => {
                return SendFuture::ready_error(crate::endpoint::SendError::PhaseInvariant, location);
            }
        }
        SendFuture::pending(endpoint, payload, location)
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
        /* SAFETY: the endpoint future owns the in-flight kernel borrow until Ready or Drop resolves the operation. */
        let lease = unsafe { endpoint.init_public_offer_state() };
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
            OfferFutureLease::RestoreOnDrop => {}
        }
        match /* SAFETY: the pointer comes from pinned owner storage and this path holds the unique mutable access for the borrow. */ unsafe { (&mut *self.endpoint).poll_offer(cx) } {
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
    pub(super) fn new(endpoint: &'e mut Endpoint<'r, ROLE>, location: Callsite) -> Self {
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
        if self.lease == OfferFutureLease::RestoreOnDrop {
            /* SAFETY: the pointer comes from pinned owner storage and this path holds unique mutable access for the borrow. */
            unsafe {
                (&mut *self.endpoint).reset_public_offer_state();
            }
        }
    }
}
