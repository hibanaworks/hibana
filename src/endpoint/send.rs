//! Send future.
//!
//! [`crate::Endpoint::send`] creates an affine future without publishing runtime
//! progress. The projected send descriptor is previewed on first poll, and only
//! an armed resident send is restored on drop.

use core::{
    future::Future,
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};

use crate::{
    endpoint::{EndpointError, EndpointOp, SendError, SendResult, kernel},
    transport::{FrameLabel, wire::WireEncode},
};

#[derive(Clone, Copy)]
enum SendFutureState {
    DirectUnarmed {
        logical_label: u8,
        payload_schema: u32,
    },
    Armed,
    ReadyError(SendError),
    Done,
}

struct RawSendFuture<'a, 'e, 'r, const ROLE: u8> {
    endpoint: *mut super::Endpoint<'r, ROLE>,
    payload: Option<kernel::RawSendPayload>,
    state: SendFutureState,
    _borrow: PhantomData<(&'a (), &'e mut super::Endpoint<'r, ROLE>)>,
}

pub(crate) struct SendFuture<'a, 'e, 'r, const ROLE: u8> {
    raw: RawSendFuture<'a, 'e, 'r, ROLE>,
}

#[inline]
pub(crate) const fn send_runtime_desc(
    logical_label: u8,
    frame_label: FrameLabel,
) -> kernel::SendRuntimeDesc {
    kernel::SendRuntimeDesc::new(logical_label, frame_label)
}

impl<'a, 'e, 'r, const ROLE: u8> SendFuture<'a, 'e, 'r, ROLE> {
    #[inline]
    pub(crate) fn pending_direct(
        endpoint: *mut super::Endpoint<'r, ROLE>,
        logical_label: u8,
        payload_schema: u32,
        payload: &'a impl WireEncode,
    ) -> Self {
        Self {
            raw: RawSendFuture::pending(
                endpoint,
                SendFutureState::DirectUnarmed {
                    logical_label,
                    payload_schema,
                },
                Some(kernel::RawSendPayload::from_typed(payload)),
            ),
        }
    }

    #[inline]
    pub(crate) fn pending_armed(
        endpoint: *mut super::Endpoint<'r, ROLE>,
        payload: &'a impl WireEncode,
    ) -> Self {
        Self {
            raw: RawSendFuture::pending(
                endpoint,
                SendFutureState::Armed,
                Some(kernel::RawSendPayload::from_typed(payload)),
            ),
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
    fn pending(
        endpoint: *mut super::Endpoint<'r, ROLE>,
        state: SendFutureState,
        payload: Option<kernel::RawSendPayload>,
    ) -> Self {
        Self {
            endpoint,
            payload,
            state,
            _borrow: PhantomData,
        }
    }

    #[inline]
    fn ready_error(error: SendError) -> Self {
        Self {
            endpoint: core::ptr::null_mut(),
            payload: None,
            state: SendFutureState::ReadyError(error),
            _borrow: PhantomData,
        }
    }

    #[inline]
    fn mark_done(&mut self) {
        self.endpoint = core::ptr::null_mut();
        self.state = SendFutureState::Done;
    }

    #[inline]
    fn arm_on_first_poll(&mut self) -> SendResult<()> {
        let (logical_label, payload_schema) = match self.state {
            SendFutureState::Armed => return Ok(()),
            SendFutureState::DirectUnarmed {
                logical_label,
                payload_schema,
            } => (logical_label, payload_schema),
            SendFutureState::ReadyError(_) | SendFutureState::Done => crate::invariant(),
        };
        if self.endpoint.is_null() {
            crate::invariant();
        }
        let result = {
            let endpoint = /* SAFETY: unarmed send futures hold the unique
            endpoint borrow but have not yet published resident send state. */
                unsafe { &mut *self.endpoint };
            endpoint.begin_public_send_state(logical_label, payload_schema)
        };
        match result {
            Ok(()) => {
                self.state = SendFutureState::Armed;
                Ok(())
            }
            Err(error) => {
                self.mark_done();
                Err(error)
            }
        }
    }

    #[inline]
    fn poll_raw(&mut self, cx: &mut Context<'_>) -> Poll<SendResult<()>> {
        if let SendFutureState::ReadyError(error) = self.state {
            self.mark_done();
            return Poll::Ready(Err(error));
        }
        if let Err(error) = self.arm_on_first_poll() {
            return Poll::Ready(Err(error));
        }
        if self.endpoint.is_null() {
            crate::invariant();
        }
        let poll = {
            let endpoint = /* SAFETY: an armed send future stores the endpoint
            pointer while the send lease is held. Polling owns `&mut self`, so
            this is the only mutable access until the future resolves or Drop
            resets the send state. */
                unsafe { &mut *self.endpoint };
            endpoint.poll_send(cx, self.payload.take())
        };
        match poll {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(_outcome)) => {
                self.mark_done();
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Err(err)) => {
                self.mark_done();
                Poll::Ready(Err(err))
            }
        }
    }
}

impl<'a, 'e, 'r, const ROLE: u8> Future for SendFuture<'a, 'e, 'r, ROLE> {
    type Output = core::result::Result<(), EndpointError>;

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
        if !self.endpoint.is_null()
            && let SendFutureState::Armed = self.state
        {
            /* SAFETY: an armed send future holds the public send lease.
            Drop owns the future and releases that lease exactly once
            through the same endpoint pointer. */
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
