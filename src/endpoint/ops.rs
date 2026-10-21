// # Unsafe Owner Contract
//
// This fragment owns only the public endpoint facade's carrier dispatch
// boundary. Each unsafe call dereferences the rendezvous-installed endpoint
// carrier header and forwards the issued role/generation tag to the kernel owner. The
// endpoint borrow keeps role-local aliasing exclusive; this fragment must not
// cache raw kernel pointers or publish progress without the carrier operation.

use super::{
    Endpoint, EndpointError, RecvResult, RouteBranch, SendResult, carrier, futures::OfferFuture,
    kernel,
};
use crate::transport::wire::{CodecError, Payload};
use core::task::{Context, Poll};

impl<'r, const ROLE: u8> Endpoint<'r, ROLE> {
    #[inline]
    fn new(
        ptr: core::ptr::NonNull<carrier::KernelEndpointHeader>,
        handle: carrier::EndpointHandle,
    ) -> Self {
        // SAFETY: the rendezvous attachment retains this issued actor for 'r.
        match unsafe { super::owner::EndpointOwner::new(ptr, handle).into_endpoint() } {
            Ok(endpoint) => endpoint,
            Err(_owner) => crate::invariant(),
        }
    }

    #[inline]
    pub(crate) fn from_handle(
        ptr: core::ptr::NonNull<carrier::KernelEndpointHeader>,
        handle: carrier::EndpointHandle,
    ) -> Self {
        Self::new(ptr, handle)
    }

    #[inline]
    /// Send the next message permitted by the local protocol.
    ///
    /// Ordinary payloads are borrowed. An [`Endpoint`] payload moves into the send
    /// future when it is created, so the sender can no longer use that value.
    /// Cancelling or failing the send does not return the transferred right.
    /// Dropping an unfinished owned-send future faults the local session run,
    /// including when a `tokio::select!` timeout drops it before its first poll.
    /// An unfinished predecessor on this view or payload returns `OutOfOrder`.
    ///
    /// Success means the transport accepted the frame, not that the peer has
    /// received it. Dropping a pending send invokes the transport's cancellation
    /// contract. A protocol mismatch faults the local session run.
    pub fn send<'a, 'e, M>(
        &'e mut self,
        payload: <M::Payload as crate::global::PayloadSpec>::SendArgument<'a, 'r>,
    ) -> impl core::future::Future<Output = Result<(), EndpointError>> + 'a + use<'a, 'e, 'r, M, ROLE>
    where
        M: crate::g::Message + 'a,
        M::Payload: 'a,
        'e: 'a,
        'r: 'e,
    {
        <M::Payload as crate::global::PayloadSpec>::send::<M, ROLE>(
            super::target::OperationTarget::new(self),
            payload,
        )
    }

    #[inline]
    /// Receive the next message permitted by the local protocol.
    ///
    /// The message label and payload format must match `M`. Fixed-size payloads
    /// reject trailing bytes. Decoded data may borrow the receive buffer until
    /// this endpoint borrow ends.
    ///
    /// An [`Endpoint`] payload returns the transferred role's continuation, tied to
    /// the receiving runtime rather than the temporary frame borrow. The caller
    /// can retain it while using this endpoint for other protocol operations.
    /// A rejected frame or transport error faults the local session run.
    pub fn recv<'e, M>(
        &'e mut self,
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
        <M::Payload as crate::global::PayloadSpec>::recv::<M, ROLE>(
            super::target::OperationTarget::new(self),
        )
    }

    #[inline]
    /// Preview the next protocol choice without advancing it.
    ///
    /// This is a preview operation. It returns a [`RouteBranch`] whose
    /// [`RouteBranch::label`] is the selected arm's first logical label.
    /// Dropping the future before completion leaves endpoint progress unchanged.
    /// Consume ordinary receive steps with [`Endpoint::recv`]; they are not
    /// returned as route previews, even when they precede a later choice.
    /// Choices resolved by the application require a registered resolver.
    pub fn offer<'e>(
        &'e mut self,
    ) -> impl core::future::Future<
        Output = core::result::Result<RouteBranch<'e, 'r, ROLE>, EndpointError>,
    > + 'e {
        OfferFuture::new(super::target::OperationTarget::new(self))
    }
}

impl<'e, 'r, const ROLE: u8, D> super::target::OperationTarget<'e, 'r, ROLE, D> {
    #[inline]
    fn ops(&self) -> &carrier::EndpointOps {
        /* SAFETY: `Endpoint::from_handle` stores a non-null `KernelEndpointHeader`
        pointer installed by the rendezvous carrier; the header remains pinned for
        `'r`, and this shared read exposes only the immutable ops table. */
        unsafe { self.parts().0.as_ref().ops() }
    }

    #[inline]
    fn erased_ptr(&self) -> core::ptr::NonNull<()> {
        self.parts().0.cast()
    }

    #[inline]
    pub(super) fn release_public_operation(&mut self) {
        unsafe {
            /* SAFETY: this target retains its exclusive whole-view or arm
            borrow. The carrier validates the issued generation and the
            compiled region, and serializes synchronous kernel entries. */
            (self.ops().release_operation)(self.erased_ptr(), self.parts().1);
        }
    }

    #[inline]
    #[must_use]
    fn call_lease_op(
        &mut self,
        op: unsafe fn(core::ptr::NonNull<()>, carrier::OperationHandle) -> kernel::PublicOpLease,
    ) -> kernel::PublicOpLease {
        unsafe {
            /* SAFETY: this target retains its exclusive whole-view or arm
            borrow. The carrier validates the issued generation and compiled
            region before installing this operation's resident state. */
            op(self.erased_ptr(), self.parts().1)
        }
    }

    #[inline]
    #[must_use]
    pub(super) fn init_public_offer_state(&mut self) -> kernel::PublicOpLease {
        self.call_lease_op(self.ops().init_public_offer_state)
    }

    #[inline]
    pub(super) fn begin_public_send_state(
        &mut self,
        logical_label: u8,
        payload_kind: crate::eff::EventPayload,
    ) -> SendResult<()> {
        // SAFETY: the issued handle and pinned carrier belong to this uniquely
        // borrowed Endpoint. The callback stores the message contract by value.
        unsafe {
            (self.ops().init_public_send_state)(
                self.erased_ptr(),
                self.parts().1,
                logical_label,
                payload_kind,
            )
        }
    }

    #[inline]
    #[must_use]
    pub(super) fn init_public_recv_state(&mut self) -> kernel::PublicOpLease {
        self.call_lease_op(self.ops().init_public_recv_state)
    }

    #[inline]
    #[must_use]
    pub(super) fn begin_public_branch_recv_state(&mut self) -> kernel::PublicOpLease {
        self.call_lease_op(self.ops().begin_public_branch_recv_state)
    }

    #[inline]
    pub(super) fn poll_recv(
        &mut self,
        logical_label: u8,
        payload_kind: crate::eff::EventPayload,
        validate: for<'a> fn(Payload<'a>) -> Result<(), CodecError>,
        cx: &mut Context<'_>,
    ) -> Poll<RecvResult<carrier::RawRecvOutput>> {
        let mut out = core::mem::MaybeUninit::<Poll<RecvResult<carrier::RawRecvOutput>>>::uninit();
        /* SAFETY: `self.parts().0` identifies this endpoint's carrier header,
        `self.parts().1` names the same generation, and the local `out` slot is
        written by `poll_recv` before `assume_init` reads it. */
        unsafe {
            (self.ops().poll_recv)(carrier::RecvPollRequest {
                ptr: self.erased_ptr(),
                handle: self.parts().1,
                logical_label,
                payload_kind,
                validate,
                cx,
                out: out.as_mut_ptr(),
            });
            out.assume_init()
        }
    }

    #[inline]
    pub(super) fn poll_offer(&mut self, cx: &mut Context<'_>) -> Poll<RecvResult<u8>> {
        let mut out = core::mem::MaybeUninit::<Poll<RecvResult<u8>>>::uninit();
        /* SAFETY: the carrier callback receives this endpoint's header pointer and handle plus a local `MaybeUninit` out slot; the callback writes the poll result before returning and the slot is not read on any other path. */
        unsafe {
            (self.ops().poll_offer)(self.erased_ptr(), self.parts().1, cx, out.as_mut_ptr());
            out.assume_init()
        }
    }

    #[inline]
    pub(super) fn poll_branch_recv(
        &mut self,
        logical_label: u8,
        payload_kind: crate::eff::EventPayload,
        validate: for<'a> fn(Payload<'a>) -> Result<(), CodecError>,
        cx: &mut Context<'_>,
    ) -> Poll<RecvResult<carrier::RawRecvOutput>> {
        let mut out = core::mem::MaybeUninit::<Poll<RecvResult<carrier::RawRecvOutput>>>::uninit();
        /* SAFETY: `self.parts().0` identifies this endpoint's carrier header,
        `self.parts().1` names the same generation, and the local `out` slot is
        written by `poll_branch_recv` before `assume_init` reads it. */
        unsafe {
            (self.ops().poll_branch_recv)(carrier::BranchRecvPollRequest {
                ptr: self.erased_ptr(),
                handle: self.parts().1,
                logical_label,
                payload_kind,
                validate,
                cx,
                out: out.as_mut_ptr(),
            });
            out.assume_init()
        }
    }

    #[inline]
    pub(crate) fn poll_send(
        &mut self,
        cx: &mut Context<'_>,
        payload: Option<kernel::lane_port::SendInput>,
    ) -> Poll<SendResult<kernel::SendCommitOutcome<'r>>> {
        let mut out =
            core::mem::MaybeUninit::<Poll<SendResult<kernel::SendCommitOutcome<'r>>>>::uninit();
        /* SAFETY: the carrier callback receives this endpoint's header pointer and handle plus a local `MaybeUninit` out slot; the callback writes the poll result before returning and the slot is not read on any other path. */
        unsafe {
            (self.ops().poll_send)(
                self.erased_ptr(),
                self.parts().1,
                payload,
                cx,
                out.as_mut_ptr().cast(),
            );
            out.assume_init()
        }
    }
}
