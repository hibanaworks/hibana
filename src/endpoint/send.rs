//! Send future.
//!
//! [`crate::Endpoint::send`] previews the projected send descriptor and stores
//! the resident send state before returning this future. Dropping the future
//! before completion restores that resident state.

use core::{
    future::Future,
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};

use crate::{
    endpoint::{EndpointError, EndpointOp, EndpointResult, SendError, SendResult, kernel},
    g::Message,
    transport::{FrameLabel, wire::WireEncode},
};

struct RawSendFuture<'a, 'e, 'r, const ROLE: u8> {
    endpoint: *mut super::Endpoint<'r, ROLE>,
    payload: kernel::RawSendPayload,
    init_error: Option<SendError>,
    _borrow: PhantomData<(&'a (), &'e mut super::Endpoint<'r, ROLE>)>,
}

pub(crate) struct SendFuture<'a, 'e, 'r, const ROLE: u8> {
    raw: RawSendFuture<'a, 'e, 'r, ROLE>,
}

#[inline]
pub(crate) fn send_runtime_desc<M>(frame_label: FrameLabel) -> kernel::SendRuntimeDesc
where
    M: Message,
{
    kernel::SendRuntimeDesc::new(<M as Message>::LOGICAL_LABEL, frame_label)
}

impl<'a, 'e, 'r, const ROLE: u8> SendFuture<'a, 'e, 'r, ROLE> {
    #[inline]
    pub(crate) fn pending(
        endpoint: *mut super::Endpoint<'r, ROLE>,
        payload: &'a impl WireEncode,
    ) -> Self {
        Self {
            raw: RawSendFuture::pending(endpoint, kernel::RawSendPayload::from_typed(payload)),
        }
    }

    #[inline]
    pub(crate) fn ready_error(error: SendError) -> Self {
        Self {
            raw: RawSendFuture::ready_error(error),
        }
    }
}

impl<'a, 'e, 'r, const ROLE: u8> RawSendFuture<'a, 'e, 'r, ROLE> {
    #[inline]
    fn pending(endpoint: *mut super::Endpoint<'r, ROLE>, payload: kernel::RawSendPayload) -> Self {
        Self {
            endpoint,
            payload,
            init_error: None,
            _borrow: PhantomData,
        }
    }

    #[inline]
    fn ready_error(error: SendError) -> Self {
        Self {
            endpoint: core::ptr::null_mut(),
            payload: kernel::RawSendPayload::empty(),
            init_error: Some(error),
            _borrow: PhantomData,
        }
    }

    #[inline]
    fn poll_raw(&mut self, cx: &mut Context<'_>) -> Poll<SendResult<()>> {
        if let Some(error) = self.init_error.take() {
            return Poll::Ready(Err(error));
        }
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
            Poll::Ready(Err(err)) => Poll::Ready(Err(EndpointError::new(EndpointOp::Send, err))),
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
            size_of::<SendFut>() <= 5 * WORD,
            "SendFuture must stay within the direct-send future budget"
        );
    }

    #[test]
    fn send_future_layout_is_message_independent() {
        assert_eq!(size_of::<SendFut>(), size_of::<SendFutAltRole>());
    }
}
