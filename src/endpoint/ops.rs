// # Unsafe Owner Contract
//
// This fragment owns only the public endpoint facade's carrier dispatch
// boundary. Each unsafe call dereferences the rendezvous-installed endpoint
// carrier header and forwards the packed handle to the kernel owner. The
// endpoint borrow keeps localside aliasing exclusive; this fragment must not
// cache raw kernel pointers or publish progress without the carrier operation.

use super::{
    Endpoint, EndpointError, EndpointOp, EndpointResult, RecvFuture, RecvResult, RouteBranch,
    SendResult, carrier, flow, futures::OfferFuture, kernel,
};
use crate::diag::Callsite;
use crate::transport::wire::{CodecError, Payload};
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
        /* SAFETY: this owner validates the concrete pointer identity and initialized storage before raw access. */
        unsafe { self.ptr.as_ref().ops() }
    }

    #[inline]
    fn erased_ptr(&self) -> core::ptr::NonNull<()> {
        self.ptr.cast()
    }

    #[inline]
    pub(crate) fn from_handle(
        ptr: core::ptr::NonNull<carrier::KernelEndpointHeader<'r>>,
        handle: carrier::PackedEndpointHandle,
    ) -> Self {
        Self::new(ptr, handle)
    }

    #[inline]
    pub(super) unsafe fn drop_kernel_endpoint(&mut self) {
        /* SAFETY: this owner validates the concrete pointer identity and initialized storage before raw access. */
        unsafe {
            (self.ops().drop_endpoint)(self.erased_ptr(), self.handle);
        }
    }

    #[inline]
    pub(super) unsafe fn reset_public_offer_state(&mut self) {
        /* SAFETY: this owner validates the concrete pointer identity and initialized storage before raw access. */
        unsafe {
            (self.ops().reset_public_offer_state)(self.erased_ptr(), self.handle);
        }
    }

    #[inline]
    #[must_use]
    pub(super) unsafe fn init_public_offer_state(&mut self) -> kernel::PublicOpLease {
        /* SAFETY: this owner validates the concrete pointer identity and initialized storage before raw access. */
        unsafe { (self.ops().init_public_offer_state)(self.erased_ptr(), self.handle) }
    }

    #[inline]
    pub(super) unsafe fn restore_public_route_branch(&mut self) {
        /* SAFETY: this owner validates the concrete pointer identity and initialized storage before raw access. */
        unsafe {
            (self.ops().restore_public_route_branch)(self.erased_ptr(), self.handle);
        }
    }

    #[inline]
    #[must_use]
    pub(super) unsafe fn init_public_send_state(
        &mut self,
        init: &kernel::SendInit,
    ) -> kernel::PublicOpLease {
        /* SAFETY: this owner validates the concrete pointer identity and initialized storage before raw access. */
        unsafe { (self.ops().init_public_send_state)(self.erased_ptr(), self.handle, init) }
    }

    #[inline]
    pub(super) unsafe fn reset_public_send_state(&mut self) {
        /* SAFETY: this owner validates the concrete pointer identity and initialized storage before raw access. */
        unsafe {
            (self.ops().reset_public_send_state)(self.erased_ptr(), self.handle);
        }
    }

    #[inline]
    #[must_use]
    pub(super) unsafe fn init_public_recv_state(&mut self) -> kernel::PublicOpLease {
        /* SAFETY: this owner validates the concrete pointer identity and initialized storage before raw access. */
        unsafe { (self.ops().init_public_recv_state)(self.erased_ptr(), self.handle) }
    }

    #[inline]
    pub(super) unsafe fn reset_public_recv_state(&mut self) {
        /* SAFETY: this owner validates the concrete pointer identity and initialized storage before raw access. */
        unsafe {
            (self.ops().reset_public_recv_state)(self.erased_ptr(), self.handle);
        }
    }

    #[inline]
    #[must_use]
    pub(super) unsafe fn begin_public_decode_state(&mut self) -> kernel::PublicOpLease {
        /* SAFETY: this owner validates the concrete pointer identity and initialized storage before raw access. */
        unsafe { (self.ops().begin_public_decode_state)(self.erased_ptr(), self.handle) }
    }

    #[inline]
    pub(super) unsafe fn reset_public_decode_state(&mut self) {
        /* SAFETY: this owner validates the concrete pointer identity and initialized storage before raw access. */
        unsafe {
            (self.ops().reset_public_decode_state)(self.erased_ptr(), self.handle);
        }
    }
    #[inline]
    fn preview_flow(&mut self, logical_label: u8, out: *mut kernel::SendPreview) -> SendResult<()> {
        /* SAFETY: this owner validates the concrete pointer identity and initialized storage before raw access. */
        unsafe { (self.ops().preview_flow)(self.erased_ptr(), self.handle, logical_label, out) }
    }

    #[inline]
    pub(super) fn poll_recv(
        &mut self,
        logical_label: u8,
        payload_mode: kernel::RecvPayloadMode,
        validate: for<'a> fn(Payload<'a>) -> Result<(), CodecError>,
        cx: &mut Context<'_>,
    ) -> Poll<RecvResult<carrier::RawPayload>> {
        let mut out = core::mem::MaybeUninit::<Poll<RecvResult<carrier::RawPayload>>>::uninit();
        /* SAFETY: this owner validates the concrete pointer identity and initialized storage before raw access. */
        unsafe {
            (self.ops().poll_recv)(carrier::RecvPollRequest {
                ptr: self.erased_ptr(),
                handle: self.handle,
                logical_label,
                payload_mode,
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
        /* SAFETY: the owner tracks the initialized prefix and this slot is inside that initialized range. */
        unsafe {
            (self.ops().poll_offer)(self.erased_ptr(), self.handle, cx, out.as_mut_ptr());
            out.assume_init()
        }
    }

    #[inline]
    pub(super) fn poll_decode(
        &mut self,
        logical_label: u8,
        validate: for<'a> fn(Payload<'a>) -> Result<(), CodecError>,
        zero_payload: for<'a> fn(&'a mut [u8]) -> Result<Payload<'a>, CodecError>,
        cx: &mut Context<'_>,
    ) -> Poll<RecvResult<carrier::RawPayload>> {
        let mut out = core::mem::MaybeUninit::<Poll<RecvResult<carrier::RawPayload>>>::uninit();
        /* SAFETY: this owner validates the concrete pointer identity and initialized storage before raw access. */
        unsafe {
            (self.ops().poll_decode)(carrier::DecodePollRequest {
                ptr: self.erased_ptr(),
                handle: self.handle,
                logical_label,
                validate,
                zero_payload,
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
        /* SAFETY: the owner tracks the initialized prefix and this slot is inside that initialized range. */
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
    /// Preview the next send for message `M`.
    ///
    /// The returned flow value must be consumed with `.send(...)` to make
    /// progress. Dropping it leaves the endpoint on the same typestate step. A
    /// preview mismatch reports [`EndpointError`] at this callsite and must not
    /// be treated as permission to choose another branch.
    #[track_caller]
    pub fn flow<'e, M>(&'e mut self) -> EndpointResult<crate::Flow<'e, 'r, ROLE, M>>
    where
        M: crate::g::Message,
    {
        let location = Callsite::caller();
        let endpoint = core::ptr::from_mut(self);
        let logical_label = <M as crate::g::Message>::LOGICAL_LABEL;
        let mut preview = core::mem::MaybeUninit::<kernel::SendPreview>::uninit();
        if let Err(error) = self.preview_flow(logical_label, preview.as_mut_ptr()) {
            return Err(EndpointError::new(EndpointOp::Flow, location, error));
        }
        let preview = /* SAFETY: the table owner tracks the initialized prefix and checks this slot before reading initialized storage. */ unsafe { preview.assume_init() };
        let desc =
            flow::send_runtime_desc::<M>(crate::transport::FrameLabel::new(preview.frame_label()));
        let init = kernel::SendInit::new(desc, preview);
        /* SAFETY: this owner validates the concrete pointer identity and initialized storage before raw access. */
        match unsafe { self.init_public_send_state(&init) } {
            kernel::PublicOpLease::Held => {}
            kernel::PublicOpLease::Rejected => {
                return Err(EndpointError::new(
                    EndpointOp::Flow,
                    location,
                    crate::endpoint::SendError::PhaseInvariant,
                ));
            }
        }
        Ok(flow::Flow::new(endpoint))
    }

    #[inline]
    /// Receive the next deterministic message as `M`.
    ///
    /// The projected descriptor must expect the same choreography label and
    /// payload family as `M`. Payload decoding is exact: fixed-size payloads reject
    /// trailing bytes, while borrowed payloads may return views tied to the
    /// endpoint borrow. A committed receive fault poisons the session generation
    /// before the error is returned.
    #[track_caller]
    pub fn recv<'e, M>(
        &'e mut self,
    ) -> impl core::future::Future<Output = EndpointResult<M::Decoded<'e>>> + 'e
    where
        M: crate::g::Message + 'e,
    {
        RecvFuture::<'e, 'r, ROLE, M>::new(self, Callsite::caller())
    }

    #[inline]
    /// Observe the next route decision.
    ///
    /// This is a preview operation. It returns a [`RouteBranch`] whose
    /// [`RouteBranch::label`] is the selected choreography branch label.
    /// Dropping the future before completion leaves endpoint progress unchanged.
    /// Dynamic branches must be selected by an explicit resolver decision at a
    /// projected route point; transport hints and payload labels are demux
    /// evidence only.
    #[track_caller]
    pub fn offer<'e>(
        &'e mut self,
    ) -> impl core::future::Future<Output = EndpointResult<RouteBranch<'e, 'r, ROLE>>> + 'e {
        OfferFuture::new(self, Callsite::caller())
    }
}
