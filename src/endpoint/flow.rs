//! Send preview and future.
//!
//! A [`Flow`] is created by [`crate::Endpoint::flow`]. It owns the projected
//! send preview until [`Flow::send`] consumes it or the value is dropped.

use core::{
    future::Future,
    marker::PhantomData,
    mem::ManuallyDrop,
    pin::Pin,
    task::{Context, Poll},
};

use crate::{
    endpoint::{EndpointError, EndpointOp, EndpointResult, ErrorLocation, SendResult, kernel},
    g::MessageSpec,
    global::{ControlDesc, MessageRuntime},
    transport::{FrameLabel, wire::WireEncode},
};

/// Send preview for one projected message.
///
/// Dropping a `Flow` before calling [`Flow::send`] leaves the endpoint on the
/// same typestate step. Calling `send` starts the affine send future and is the
/// operation that can commit endpoint progress.
pub struct Flow<'e, 'r, const ROLE: u8, M>
where
    M: MessageSpec,
{
    endpoint: *mut super::Endpoint<'r, ROLE>,
    _msg: PhantomData<(&'e mut super::Endpoint<'r, ROLE>, M)>,
}

struct RawSendFuture<'e, 'r, const ROLE: u8> {
    endpoint: *mut super::Endpoint<'r, ROLE>,
    completed: bool,
    _borrow: PhantomData<&'e mut super::Endpoint<'r, ROLE>>,
}

pub(crate) struct SendFuture<'e, 'r, const ROLE: u8> {
    raw: RawSendFuture<'e, 'r, ROLE>,
    location: ErrorLocation,
}

#[inline]
pub(crate) fn send_runtime_desc<M>(frame_label: FrameLabel) -> kernel::SendRuntimeDesc
where
    M: MessageSpec,
{
    let control = <M as MessageRuntime>::CONTROL.map(ControlDesc::from_static);
    kernel::SendRuntimeDesc::new(
        <M as MessageSpec>::LOGICAL_LABEL,
        frame_label,
        <M as MessageRuntime>::CONTROL_PAYLOAD,
        control,
        <M as MessageRuntime>::ENCODE_CONTROL_HANDLE,
    )
}

impl<'e, 'r, const ROLE: u8, M> Flow<'e, 'r, ROLE, M>
where
    M: MessageSpec,
{
    pub(crate) fn new(endpoint: *mut super::Endpoint<'r, ROLE>) -> Self {
        Self {
            endpoint,
            _msg: PhantomData,
        }
    }
}

impl<'e, 'r, const ROLE: u8, M> Flow<'e, 'r, ROLE, M>
where
    M: MessageSpec,
    M::Payload: WireEncode,
{
    #[inline]
    /// Send this flow's message and consume the send preview on success.
    ///
    /// Pass the projected payload by reference. Endpoint-owned local controls
    /// use `()` as the request payload; explicit wire controls use an opaque
    /// `GenericCapToken<Kind>` value.
    /// If the committed send fails, the returned [`crate::EndpointError`] is
    /// terminal evidence for this generation, not permission to repeat the
    /// send or take an alternate branch.
    #[track_caller]
    pub fn send<'a>(
        self,
        payload: &'a M::Payload,
    ) -> impl Future<Output = EndpointResult<()>> + 'a + use<'a, 'e, 'r, M, ROLE>
    where
        M::Payload: 'a,
        M: 'a,
        'e: 'a,
        'r: 'a,
    {
        let payload = Some(kernel::RawSendPayload::from_typed::<M::Payload>(payload));
        let flow = ManuallyDrop::new(self);
        let endpoint = flow.endpoint;
        /* SAFETY: the pointer comes from pinned owner storage and this path holds unique mutable access for the borrow. */
        unsafe {
            (&mut *endpoint).set_public_send_payload(&payload);
        }
        SendFuture {
            raw: RawSendFuture::new(endpoint),
            location: ErrorLocation::caller(),
        }
    }
}

impl<'e, 'r, const ROLE: u8, M> Drop for Flow<'e, 'r, ROLE, M>
where
    M: MessageSpec,
{
    fn drop(&mut self) {
        /* SAFETY: the pointer comes from pinned owner storage and this path holds unique mutable access for the borrow. */
        unsafe {
            (&mut *self.endpoint).reset_public_send_state();
        }
    }
}

impl<'e, 'r, const ROLE: u8> RawSendFuture<'e, 'r, ROLE> {
    #[inline]
    fn new(endpoint: *mut super::Endpoint<'r, ROLE>) -> Self {
        Self {
            endpoint,
            completed: false,
            _borrow: PhantomData,
        }
    }

    #[inline]
    fn poll_raw(&mut self, cx: &mut Context<'_>) -> Poll<SendResult<()>> {
        if self.completed {
            panic!("completed send future polled after Ready");
        }
        let poll = {
            let endpoint = /* SAFETY: the pointer comes from pinned owner storage and this path holds the unique mutable access for the borrow. */ unsafe { &mut *self.endpoint };
            endpoint.poll_send(cx)
        };
        match poll {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(outcome)) => {
                self.completed = true;
                outcome.descriptor.publish();
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Err(err)) => {
                self.completed = true;
                Poll::Ready(Err(err))
            }
        }
    }
}

impl<'e, 'r, const ROLE: u8> Future for SendFuture<'e, 'r, ROLE> {
    type Output = EndpointResult<()>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = /* SAFETY: SendFuture has no self-referential fields; its raw endpoint future owns the resident operation state separately. */ unsafe { self.get_unchecked_mut() };
        match this.raw.poll_raw(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(())) => Poll::Ready(Ok(())),
            Poll::Ready(Err(err)) => Poll::Ready(Err(EndpointError::new(
                EndpointOp::Send,
                this.location,
                err,
            ))),
        }
    }
}

impl<'e, 'r, const ROLE: u8> Drop for RawSendFuture<'e, 'r, ROLE> {
    fn drop(&mut self) {
        if !self.completed {
            /* SAFETY: the pointer comes from pinned owner storage and this path holds unique mutable access for the borrow. */
            unsafe {
                (&mut *self.endpoint).reset_public_send_state();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SendFuture;
    use core::mem::size_of;

    type SendFut = SendFuture<'static, 'static, 0>;
    type SendFutAltRole = SendFuture<'static, 'static, 1>;

    #[test]
    fn send_future_stays_within_size_budget() {
        const WORD: usize = size_of::<usize>();
        assert!(
            size_of::<SendFut>() <= 3 * WORD,
            "SendFuture must stay within the 3-word budget"
        );
    }

    #[test]
    fn send_future_layout_is_message_independent() {
        assert_eq!(size_of::<SendFut>(), size_of::<SendFutAltRole>());
    }
}
