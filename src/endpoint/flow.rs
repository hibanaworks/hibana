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
    diag::Callsite,
    endpoint::{EndpointError, EndpointOp, EndpointResult, SendResult, kernel},
    g::Message,
    transport::{FrameLabel, wire::WireEncode},
};

/// Send preview for one projected message.
///
/// Dropping a `Flow` before calling [`Flow::send`] leaves the endpoint on the
/// same typestate step. Calling `send` starts the affine send future and is the
/// operation that can commit endpoint progress.
pub struct Flow<'e, 'r, const ROLE: u8, M>
where
    M: Message,
{
    endpoint: *mut super::Endpoint<'r, ROLE>,
    _msg: PhantomData<(&'e mut super::Endpoint<'r, ROLE>, M)>,
}

struct RawSendFuture<'a, 'e, 'r, const ROLE: u8> {
    endpoint: *mut super::Endpoint<'r, ROLE>,
    payload: kernel::RawSendPayload,
    _borrow: PhantomData<(&'a (), &'e mut super::Endpoint<'r, ROLE>)>,
}

pub(crate) struct SendFuture<'a, 'e, 'r, const ROLE: u8> {
    raw: RawSendFuture<'a, 'e, 'r, ROLE>,
    location: Callsite,
}

#[inline]
pub(crate) fn send_runtime_desc<M>(frame_label: FrameLabel) -> kernel::SendRuntimeDesc
where
    M: Message,
{
    kernel::SendRuntimeDesc::new(<M as Message>::LOGICAL_LABEL, frame_label)
}

impl<'e, 'r, const ROLE: u8, M> Flow<'e, 'r, ROLE, M>
where
    M: Message,
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
    M: Message,
    M::Payload: WireEncode,
{
    #[inline]
    /// Send this flow's message and consume the send preview on success.
    ///
    /// Pass the projected payload by reference. Endpoint-owned session evidence
    /// is internal; application flows send only protocol payloads.
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
        let flow = ManuallyDrop::new(self);
        let endpoint = flow.endpoint;
        SendFuture {
            raw: RawSendFuture::new(
                endpoint,
                kernel::RawSendPayload::from_typed::<M::Payload>(payload),
            ),
            location: Callsite::caller(),
        }
    }
}

impl<'e, 'r, const ROLE: u8, M> Drop for Flow<'e, 'r, ROLE, M>
where
    M: Message,
{
    fn drop(&mut self) {
        /* SAFETY: the pointer comes from pinned owner storage and this path holds unique mutable access for the borrow. */
        unsafe {
            (&mut *self.endpoint).reset_public_send_state();
        }
    }
}

impl<'a, 'e, 'r, const ROLE: u8> RawSendFuture<'a, 'e, 'r, ROLE> {
    #[inline]
    fn new(endpoint: *mut super::Endpoint<'r, ROLE>, payload: kernel::RawSendPayload) -> Self {
        Self {
            endpoint,
            payload,
            _borrow: PhantomData,
        }
    }

    #[inline]
    fn poll_raw(&mut self, cx: &mut Context<'_>) -> Poll<SendResult<()>> {
        if self.endpoint.is_null() {
            crate::invariant();
        }
        let poll = {
            let endpoint = /* SAFETY: the pointer comes from pinned owner storage and this path holds the unique mutable access for the borrow. */ unsafe { &mut *self.endpoint };
            endpoint.poll_send(cx, self.payload.take())
        };
        match poll {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(_outcome)) => {
                self.endpoint = core::ptr::null_mut();
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Err(err)) => {
                self.endpoint = core::ptr::null_mut();
                Poll::Ready(Err(err))
            }
        }
    }
}

impl<'a, 'e, 'r, const ROLE: u8> Future for SendFuture<'a, 'e, 'r, ROLE> {
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

impl<'a, 'e, 'r, const ROLE: u8> Drop for RawSendFuture<'a, 'e, 'r, ROLE> {
    fn drop(&mut self) {
        if !self.endpoint.is_null() {
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

    type SendFut = SendFuture<'static, 'static, 'static, 0>;
    type SendFutAltRole = SendFuture<'static, 'static, 'static, 1>;

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
