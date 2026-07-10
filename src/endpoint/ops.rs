// # Unsafe Owner Contract
//
// This fragment owns only the public endpoint facade's carrier dispatch
// boundary. Each unsafe call dereferences the rendezvous-installed endpoint
// carrier header and forwards the packed handle to the kernel owner. The
// endpoint borrow keeps localside aliasing exclusive; this fragment must not
// cache raw kernel pointers or publish progress without the carrier operation.

use super::{
    Endpoint, EndpointError, RecvFuture, RecvResult, RouteBranch, SendResult, carrier,
    futures::OfferFuture, kernel, send,
};
use crate::transport::wire::{CodecError, Payload, WireEncode, WirePayload};
use core::task::{Context, Poll};

impl<'r, const ROLE: u8> Endpoint<'r, ROLE> {
    #[inline]
    fn new(
        ptr: core::ptr::NonNull<carrier::KernelEndpointHeader<'r>>,
        handle: carrier::PackedEndpointHandle,
    ) -> Self {
        Self {
            ptr,
            handle,
            _borrow: core::marker::PhantomData,
            _local_only: crate::local::LocalOnly::new(),
        }
    }

    #[inline]
    fn ops(&self) -> &carrier::EndpointOps<'r> {
        /* SAFETY: `Endpoint::from_handle` stores a non-null `KernelEndpointHeader`
        pointer installed by the rendezvous carrier; the header remains pinned for
        `'r`, and this shared read exposes only the immutable ops table. */
        unsafe { self.ptr.as_ref().ops() }
    }

    #[inline]
    fn erased_ptr(&self) -> core::ptr::NonNull<()> {
        self.ptr.cast()
    }

    #[inline]
    fn call_handle_op(
        &mut self,
        op: unsafe fn(core::ptr::NonNull<()>, carrier::PackedEndpointHandle),
    ) {
        unsafe {
            /* SAFETY: handle-only carrier calls use this endpoint's rendezvous-installed
            `KernelEndpointHeader`, `self.handle` is the packed carrier lease for
            the same generation, and `&mut self` excludes a second public
            endpoint operation while the callback mutates resident state. */
            op(self.erased_ptr(), self.handle);
        }
    }

    #[inline]
    #[must_use]
    fn call_lease_op(
        &mut self,
        op: unsafe fn(
            core::ptr::NonNull<()>,
            carrier::PackedEndpointHandle,
        ) -> kernel::PublicOpLease,
    ) -> kernel::PublicOpLease {
        unsafe {
            /* SAFETY: lease-producing carrier calls use this endpoint's
            rendezvous-installed
            `KernelEndpointHeader`, `self.handle` is the packed carrier lease for
            the same generation, and `&mut self` excludes a second public
            endpoint operation while the callback mutates resident state. */
            op(self.erased_ptr(), self.handle)
        }
    }

    #[inline]
    #[must_use]
    fn call_send_init_op(
        &mut self,
        op: unsafe fn(
            core::ptr::NonNull<()>,
            carrier::PackedEndpointHandle,
            *const kernel::SendInit,
        ) -> kernel::PublicOpLease,
        init: &kernel::SendInit,
    ) -> kernel::PublicOpLease {
        unsafe {
            /* SAFETY: send-init carrier calls use this endpoint's
            rendezvous-installed
            `KernelEndpointHeader`, `self.handle` is the packed carrier lease for
            the same generation, and `init` is a caller-owned initialized
            `SendInit` slot read only during this carrier callback. */
            op(self.erased_ptr(), self.handle, core::ptr::from_ref(init))
        }
    }

    #[inline]
    pub(crate) fn from_handle(
        ptr: core::ptr::NonNull<carrier::KernelEndpointHeader<'r>>,
        handle: carrier::PackedEndpointHandle,
    ) -> Self {
        Self::new(ptr, handle)
    }

    #[inline]
    pub(super) fn drop_kernel_endpoint(&mut self) {
        self.call_handle_op(self.ops().drop_endpoint);
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
    #[must_use]
    pub(super) fn init_public_send_state(
        &mut self,
        init: &kernel::SendInit,
    ) -> kernel::PublicOpLease {
        self.call_send_init_op(self.ops().init_public_send_state, init)
    }

    #[inline]
    pub(super) fn begin_public_send_state(&mut self, logical_label: u8) -> SendResult<()> {
        let mut preview = core::mem::MaybeUninit::<kernel::SendPreview>::uninit();
        self.preview_send(logical_label, preview.as_mut_ptr())?;
        let preview = /* SAFETY: `preview_send` returned `Ok`, so the carrier
        callback wrote one `SendPreview` into this local `MaybeUninit` slot
        before return. */ unsafe { preview.assume_init() };
        let desc = send::send_runtime_desc(
            logical_label,
            crate::transport::FrameLabel::new(preview.frame_label()),
        );
        let init = kernel::SendInit::new(desc, preview);
        match self.init_public_send_state(&init) {
            kernel::PublicOpLease::Held => Ok(()),
            kernel::PublicOpLease::Rejected => Err(crate::endpoint::SendError::PhaseInvariant),
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
    pub(super) fn preview_send(
        &mut self,
        logical_label: u8,
        out: *mut kernel::SendPreview,
    ) -> SendResult<()> {
        /* SAFETY: `self.ptr` identifies this endpoint's carrier header and
        `self.handle` names the same generation; `out` is the caller-owned
        `SendPreview` slot that `preview_send` writes before returning `Ok`. */
        unsafe { (self.ops().preview_send)(self.erased_ptr(), self.handle, logical_label, out) }
    }

    #[inline]
    pub(super) fn poll_recv(
        &mut self,
        logical_label: u8,
        validate: for<'a> fn(Payload<'a>) -> Result<(), CodecError>,
        cx: &mut Context<'_>,
    ) -> Poll<RecvResult<carrier::RawPayload>> {
        let mut out = core::mem::MaybeUninit::<Poll<RecvResult<carrier::RawPayload>>>::uninit();
        /* SAFETY: `self.ptr` identifies this endpoint's carrier header,
        `self.handle` names the same generation, and the local `out` slot is
        written by `poll_recv` before `assume_init` reads it. */
        unsafe {
            (self.ops().poll_recv)(carrier::RecvPollRequest {
                ptr: self.erased_ptr(),
                handle: self.handle,
                logical_label,
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
            (self.ops().poll_offer)(self.erased_ptr(), self.handle, cx, out.as_mut_ptr());
            out.assume_init()
        }
    }

    #[inline]
    pub(super) fn poll_branch_recv(
        &mut self,
        logical_label: u8,
        validate: for<'a> fn(Payload<'a>) -> Result<(), CodecError>,
        cx: &mut Context<'_>,
    ) -> Poll<RecvResult<carrier::RawPayload>> {
        let mut out = core::mem::MaybeUninit::<Poll<RecvResult<carrier::RawPayload>>>::uninit();
        /* SAFETY: `self.ptr` identifies this endpoint's carrier header,
        `self.handle` names the same generation, and the local `out` slot is
        written by `poll_branch_recv` before `assume_init` reads it. */
        unsafe {
            (self.ops().poll_branch_recv)(carrier::BranchRecvPollRequest {
                ptr: self.erased_ptr(),
                handle: self.handle,
                logical_label,
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
        payload: Option<kernel::RawSendPayload>,
    ) -> Poll<SendResult<kernel::SendCommitOutcome<'r>>> {
        let mut out =
            core::mem::MaybeUninit::<Poll<SendResult<kernel::SendCommitOutcome<'r>>>>::uninit();
        /* SAFETY: the carrier callback receives this endpoint's header pointer and handle plus a local `MaybeUninit` out slot; the callback writes the poll result before returning and the slot is not read on any other path. */
        unsafe {
            (self.ops().poll_send)(
                self.erased_ptr(),
                self.handle,
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
    /// The endpoint previews the projected send descriptor on first poll.
    /// Dropping the future before completion leaves the endpoint on the same
    /// typestate step. A preview mismatch is reported as a send failure and
    /// must not be treated as permission to choose another branch.
    pub fn send<'a, 'e, M>(
        &'e mut self,
        payload: &'a M::Payload,
    ) -> impl core::future::Future<Output = core::result::Result<(), EndpointError>>
    + 'a
    + use<'a, 'e, 'r, M, ROLE>
    where
        M: crate::g::Message + 'a,
        M::Payload: WireEncode + 'a,
        'e: 'a,
        'r: 'a,
    {
        let endpoint = core::ptr::from_mut(self);
        let logical_label = <M as crate::g::Message>::LOGICAL_LABEL;
        send::SendFuture::pending_direct(endpoint, logical_label, payload)
    }

    #[inline]
    /// Receive the next message as `M` after descriptor evidence matches.
    ///
    /// The projected descriptor must expect the same choreography label and
    /// payload family as `M`. Payload decoding is exact: fixed-size payloads reject
    /// trailing bytes, while borrowed payloads may return views tied to the
    /// endpoint borrow. A committed receive fault poisons the session generation
    /// before the error is returned.
    pub fn recv<'e, M>(
        &'e mut self,
    ) -> impl core::future::Future<
        Output = core::result::Result<<M::Payload as WirePayload>::Decoded<'e>, EndpointError>,
    > + 'e
    where
        M: crate::g::Message + 'e,
        M::Payload: WirePayload,
    {
        RecvFuture::<'e, 'r, ROLE, M>::new(self)
    }

    #[inline]
    /// Observe the next route decision.
    ///
    /// This is a preview operation. It returns a [`RouteBranch`] whose
    /// [`RouteBranch::label`] is the selected arm's first logical label.
    /// Dropping the future before completion leaves endpoint progress unchanged.
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
