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
    fn ops(&self) -> &carrier::EndpointOps {
        /* SAFETY: `Endpoint::from_handle` stores a non-null `KernelEndpointHeader`
        pointer installed by the rendezvous carrier; the header remains pinned for
        `'r`, and this shared read exposes only the immutable ops table. */
        unsafe { self.owner.ptr.as_ref().ops() }
    }

    #[inline]
    fn erased_ptr(&self) -> core::ptr::NonNull<()> {
        self.owner.ptr.cast()
    }

    #[inline]
    fn call_handle_op(&mut self, op: unsafe fn(core::ptr::NonNull<()>, carrier::EndpointHandle)) {
        unsafe {
            /* SAFETY: handle-only carrier calls use this endpoint's rendezvous-installed
            `KernelEndpointHeader`, `self.owner.handle` is the issued carrier tag for
            the same generation, and `&mut self` excludes a second public
            endpoint operation while the callback mutates resident state. */
            op(self.erased_ptr(), self.owner.handle);
        }
    }

    #[inline]
    #[must_use]
    fn call_lease_op(
        &mut self,
        op: unsafe fn(core::ptr::NonNull<()>, carrier::EndpointHandle) -> kernel::PublicOpLease,
    ) -> kernel::PublicOpLease {
        unsafe {
            /* SAFETY: lease-producing carrier calls use this endpoint's
            rendezvous-installed
            `KernelEndpointHeader`, `self.owner.handle` is the issued carrier tag for
            the same generation, and `&mut self` excludes a second public
            endpoint operation while the callback mutates resident state. */
            op(self.erased_ptr(), self.owner.handle)
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
    pub(super) fn reset_public_offer_state(&mut self) {
        self.call_handle_op(self.ops().reset_public_offer_state);
    }

    #[inline]
    #[must_use]
    pub(super) fn init_public_offer_state(&mut self) -> kernel::PublicOpLease {
        self.call_lease_op(self.ops().init_public_offer_state)
    }

    #[inline]
    pub(super) fn restore_public_route_branch(&mut self) {
        self.call_handle_op(self.ops().restore_public_route_branch);
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
                self.owner.handle,
                logical_label,
                payload_kind,
            )
        }
    }

    #[inline]
    pub(super) fn reset_public_send_state(&mut self) {
        self.call_handle_op(self.ops().reset_public_send_state);
    }

    #[inline]
    #[must_use]
    pub(super) fn init_public_recv_state(&mut self) -> kernel::PublicOpLease {
        self.call_lease_op(self.ops().init_public_recv_state)
    }

    #[inline]
    pub(super) fn reset_public_recv_state(&mut self) {
        self.call_handle_op(self.ops().reset_public_recv_state);
    }

    #[inline]
    #[must_use]
    pub(super) fn begin_public_branch_recv_state(&mut self) -> kernel::PublicOpLease {
        self.call_lease_op(self.ops().begin_public_branch_recv_state)
    }

    #[inline]
    pub(super) fn reset_public_branch_recv_state(&mut self) {
        self.call_handle_op(self.ops().reset_public_branch_recv_state);
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
        /* SAFETY: `self.owner.ptr` identifies this endpoint's carrier header,
        `self.owner.handle` names the same generation, and the local `out` slot is
        written by `poll_recv` before `assume_init` reads it. */
        unsafe {
            (self.ops().poll_recv)(carrier::RecvPollRequest {
                ptr: self.erased_ptr(),
                handle: self.owner.handle,
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
            (self.ops().poll_offer)(self.erased_ptr(), self.owner.handle, cx, out.as_mut_ptr());
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
        /* SAFETY: `self.owner.ptr` identifies this endpoint's carrier header,
        `self.owner.handle` names the same generation, and the local `out` slot is
        written by `poll_branch_recv` before `assume_init` reads it. */
        unsafe {
            (self.ops().poll_branch_recv)(carrier::BranchRecvPollRequest {
                ptr: self.erased_ptr(),
                handle: self.owner.handle,
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
                self.owner.handle,
                payload,
                cx,
                out.as_mut_ptr().cast(),
            );
            out.assume_init()
        }
    }

    #[inline]
    /// Send the next projected message as `M`.
    ///
    /// Wire payloads are borrowed; Endpoint payloads move into the future.
    /// Cancelling an owned send does not restore the transferred operation
    /// right. Dropping an unfinished resource ends the current run.
    ///
    /// The endpoint previews the projected send descriptor on first poll.
    /// Dropping the future before completion leaves the endpoint on the same
    /// typestate step. A preview mismatch reports its exact send failure and
    /// makes the current session generation terminal.
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
        <M::Payload as crate::global::PayloadSpec>::send::<M, ROLE>(self, payload)
    }

    #[inline]
    /// Receive the next message as `M` after descriptor evidence matches.
    ///
    /// The projected descriptor must expect the same choreography label and
    /// payload family as `M`. Payload decoding is exact: fixed-size payloads reject
    /// trailing bytes, while borrowed payloads may return views tied to the
    /// endpoint borrow. A committed receive fault poisons the session generation
    /// before the error is returned.
    /// Endpoint payloads return a uniquely issued continuation tied to the
    /// receiving runtime, independent of this temporary receive borrow.
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
        <M::Payload as crate::global::PayloadSpec>::recv::<M, ROLE>(self)
    }

    #[inline]
    /// Observe the next route decision.
    ///
    /// This is a preview operation. It returns a [`RouteBranch`] whose
    /// [`RouteBranch::label`] is the selected arm's first logical label.
    /// Dropping the future before completion leaves endpoint progress unchanged.
    /// Consume ordinary receive steps with [`Endpoint::recv`]; they are not
    /// returned as route previews, even when they precede a later choice.
    /// Dynamic branches must be selected by an explicit resolver decision at a
    /// projected route point; transport observations and payload labels are demux
    /// evidence only.
    pub fn offer<'e>(
        &'e mut self,
    ) -> impl core::future::Future<
        Output = core::result::Result<RouteBranch<'e, 'r, ROLE>, EndpointError>,
    > + 'e {
        OfferFuture::new(self)
    }
}
