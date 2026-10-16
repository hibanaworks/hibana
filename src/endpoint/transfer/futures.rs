use crate::endpoint::{
    Endpoint, EndpointError, EndpointOp, RouteBranch, carrier::RawRecvOutput,
    futures::RawRecvFuture, kernel::lane_port::SendInput,
};
use crate::{
    eff::EventPayload,
    transport::wire::{CodecError, Payload},
};
use core::{
    future::Future,
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};

enum OwnedSendPhase {
    Direct(u8),
    Branch(u8),
    Pending,
    Done,
}

pub(super) struct OwnedSendFuture<'e, 'r, const ROLE: u8, const RESOURCE: u8> {
    endpoint: *mut Endpoint<'r, ROLE>,
    payload: Endpoint<'r, RESOURCE>,
    phase: OwnedSendPhase,
    _borrow: PhantomData<&'e mut Endpoint<'r, ROLE>>,
}

impl<'e, 'r, const ROLE: u8, const RESOURCE: u8> OwnedSendFuture<'e, 'r, ROLE, RESOURCE> {
    pub(super) fn direct(
        endpoint: &'e mut Endpoint<'r, ROLE>,
        label: u8,
        payload: Endpoint<'r, RESOURCE>,
    ) -> Self {
        Self {
            endpoint,
            payload,
            phase: OwnedSendPhase::Direct(label),
            _borrow: PhantomData,
        }
    }
    pub(super) fn branch(
        branch: RouteBranch<'e, 'r, ROLE>,
        label: u8,
        payload: Endpoint<'r, RESOURCE>,
    ) -> Self {
        let branch = core::mem::ManuallyDrop::new(branch);
        Self {
            endpoint: branch.endpoint,
            payload,
            phase: OwnedSendPhase::Branch(label),
            _borrow: PhantomData,
        }
    }
}

impl<'e, 'r, const ROLE: u8, const RESOURCE: u8> Future
    for OwnedSendFuture<'e, 'r, ROLE, RESOURCE>
{
    type Output = Result<(), EndpointError>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        // SAFETY: this future holds the unique carrier Endpoint borrow until
        // completion/drop, and owns the distinct resource Endpoint value.
        let endpoint = unsafe { &mut *this.endpoint };
        let input = match this.phase {
            OwnedSendPhase::Direct(label) | OwnedSendPhase::Branch(label) => {
                if let Err(error) =
                    endpoint.begin_public_send_state(label, EventPayload::Endpoint(RESOURCE))
                {
                    this.phase = OwnedSendPhase::Done;
                    return Poll::Ready(Err(EndpointError::new(EndpointOp::Send, error)));
                }
                Some(SendInput::Endpoint(super::EndpointTransfer::borrowed(
                    &this.payload.owner,
                )))
            }
            OwnedSendPhase::Pending => None,
            OwnedSendPhase::Done => crate::invariant(),
        };
        match endpoint.poll_send(cx, input) {
            Poll::Pending => {
                this.phase = OwnedSendPhase::Pending;
                Poll::Pending
            }
            Poll::Ready(result) => {
                this.phase = OwnedSendPhase::Done;
                Poll::Ready(
                    result
                        .map(|_| ())
                        .map_err(|error| EndpointError::new(EndpointOp::Send, error)),
                )
            }
        }
    }
}

impl<const ROLE: u8, const RESOURCE: u8> Drop for OwnedSendFuture<'_, '_, ROLE, RESOURCE> {
    fn drop(&mut self) {
        // The payload owner is dropped after cancellation. A committed transfer
        // has invalidated its tag; an aborted transfer cancels the current run.
        match self.phase {
            OwnedSendPhase::Pending => unsafe { (&mut *self.endpoint).reset_public_send_state() },
            OwnedSendPhase::Branch(_) => unsafe {
                (&mut *self.endpoint).restore_public_route_branch()
            },
            OwnedSendPhase::Direct(_) | OwnedSendPhase::Done => {}
        }
    }
}

pub(super) struct OwnedRecvFuture<'e, 'r, const ROLE: u8, const RESOURCE: u8> {
    raw: RawRecvFuture<'e, 'r, ROLE>,
    label: u8,
}

fn validate_envelope<const RESOURCE: u8>(payload: Payload<'_>) -> Result<(), CodecError> {
    let bytes = payload.as_bytes();
    if bytes.len() == 3 && bytes[0] == RESOURCE {
        Ok(())
    } else {
        Err(CodecError::Malformed)
    }
}

impl<'e, 'r, const ROLE: u8, const RESOURCE: u8> OwnedRecvFuture<'e, 'r, ROLE, RESOURCE> {
    pub(super) fn direct(endpoint: &'e mut Endpoint<'r, ROLE>, label: u8) -> Self {
        Self {
            raw: RawRecvFuture::new(endpoint),
            label,
        }
    }
    pub(super) fn branch(branch: RouteBranch<'e, 'r, ROLE>, label: u8) -> Self {
        Self {
            raw: RawRecvFuture::from_branch(branch),
            label,
        }
    }
}

impl<'e, 'r, const ROLE: u8, const RESOURCE: u8> Future
    for OwnedRecvFuture<'e, 'r, ROLE, RESOURCE>
{
    type Output = Result<Endpoint<'r, RESOURCE>, EndpointError>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        let kind = EventPayload::Endpoint(RESOURCE);
        let result = this
            .raw
            .poll_raw(this.label, kind, validate_envelope::<RESOURCE>, cx);
        match result {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Err(error)) => {
                Poll::Ready(Err(EndpointError::new(EndpointOp::Recv, error)))
            }
            Poll::Ready(Ok(RawRecvOutput::Endpoint(owner))) => {
                // SAFETY: the receive commit issued this unique view in the
                // same pinned runtime as the carrier borrowed for 'r. Its
                // lifetime is independent of the temporary frame borrow 'e.
                match unsafe { owner.into_endpoint::<RESOURCE>() } {
                    Ok(endpoint) => Poll::Ready(Ok(endpoint)),
                    Err(_) => crate::invariant(),
                }
            }
            Poll::Ready(Ok(RawRecvOutput::Payload(_))) => crate::invariant(),
        }
    }
}
