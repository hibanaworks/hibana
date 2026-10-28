use super::{
    Endpoint, EndpointError, EndpointOp, RouteBranch,
    carrier::OperationHandle,
    target::{BorrowedEndpoint, OperationTarget},
};
use core::marker::PhantomData;

#[cfg(all(test, hibana_repo_tests))]
mod tests;

/// An exclusive borrow of one projected parallel fragment.
///
/// Created by [`Endpoint::par`]. Its operations follow the parent's single
/// protocol cursor and are restricted to this fragment. The parent cannot be
/// used or delegated until all its parallel borrows have ended. This borrow
/// cannot be cloned or sent as an owned [`Endpoint`] payload.
///
/// Dropping a completed fragment releases the borrow. Dropping an unfinished
/// fragment faults the current session run, including its other fragments.
pub struct ParallelEndpoint<'p, 'r, const ROLE: u8> {
    borrowed: BorrowedEndpoint,
    _parent: PhantomData<&'p mut Endpoint<'r, ROLE>>,
}

impl<'e, 'r, const ROLE: u8> OperationTarget<'e, 'r, ROLE> {
    fn split<const N: usize>(self) -> Result<[ParallelEndpoint<'e, 'r, ROLE>; N], EndpointError> {
        const { assert!(N >= 1 && N <= u8::MAX as usize + 1) };
        let (ptr, handle) = self.parts();
        let mut regions = [super::carrier::OperationRegion::WHOLE; N];
        // SAFETY: the exclusive parent borrow is transferred to the disjoint
        // regions derived from its projected global parallel scope.
        unsafe { (ptr.as_ref().ops().split_parallel)(ptr.cast(), handle, &mut regions) }
            .map_err(|error| EndpointError::new(EndpointOp::Par, error))?;
        Ok(regions.map(|region| ParallelEndpoint {
            borrowed: BorrowedEndpoint {
                ptr,
                handle: handle.endpoint,
                region,
            },
            _parent: PhantomData,
        }))
    }
}

impl<'r, const ROLE: u8> Endpoint<'r, ROLE> {
    /// Borrow the independent fragments of the current projected `g::par`.
    ///
    /// The array length is inferred from the binding pattern. Fragments follow
    /// global source order, omitting fragments in which this role has no action.
    /// The array can have one element when this endpoint participates in only
    /// one fragment. Other endpoints held by the participant can borrow their
    /// own fragments and run alongside it.
    /// When requesting multiple fragments, a source `par` with only one local
    /// arm is skipped to reach a nested local fork. A one-fragment borrow can
    /// retain that outer scope explicitly. Sequential prefixes still have to
    /// complete before the nested fork can be borrowed.
    /// A fragment may wait for a preceding operation on another held endpoint.
    /// Borrowing it leaves that dependency in place for its sends and receives.
    /// Adjacent `par` groups are flattened. Sequential prefixes and enclosing
    /// `roll` scopes keep their boundaries. A fragment can be borrowed again
    /// when it reaches a nested `par`.
    /// Borrowing a completed repeated group prepares its next repetition. The
    /// first successful send or receive starts it. Nested borrows keep that
    /// same entry until an operation advances the protocol.
    /// Keep a fragment borrowed to continue a repeat inside the current group.
    /// When another `par` follows a repeated group, keep all its borrows across
    /// the repetitions. After they end, `par()` prefers an enabled later group
    /// to restarting the completed group.
    /// With nested repeats, identical message sequences can have different
    /// boundaries; the borrow determines which group its operations belong to.
    ///
    /// Array positions identify fragments; variable names do not select them.
    /// The fragment types do not encode their individual protocols. An
    /// operation not permitted by its fragment is rejected at runtime.
    ///
    /// The parent stays borrowed until all fragments are dropped. Poll their
    /// operations concurrently with your executor, which can be single-threaded.
    /// Calling `par` only creates borrows. The fragment operations perform the
    /// communication and advance the protocol.
    ///
    /// Returns an error if there is no unambiguous current parallel fragment,
    /// the array length differs from its width, or an earlier operation remains
    /// active. A rejected borrow leaves the parent protocol unchanged.
    ///
    /// Inside a choice, first call
    /// `let label = endpoint.offer().await?.label();`, then borrow the selected
    /// branch's fragments. They keep that choice, including any buffered first
    /// message. Their matching `send` or `recv` advances the protocol. Independent
    /// fragments can run while that first operation waits.
    ///
    /// ```
    /// use hibana::{Endpoint, EndpointError, g};
    /// type Request = g::Msg<1, u32>;
    /// type Reply = g::Msg<2, u32>;
    /// type Notice = g::Msg<3, u32>;
    ///
    /// let global = g::par(
    ///     g::seq(g::send::<0, 1, Request>(), g::send::<1, 0, Reply>()),
    ///     g::send::<1, 0, Notice>(),
    /// );
    ///
    /// async fn client(mut client: Endpoint<'_, 0>) -> Result<(), EndpointError> {
    ///     let [mut requests, mut notices] = client.par()?;
    ///     let (reply, notice) = tokio::try_join!(
    ///         async {
    ///             requests.send::<Request>(&7).await?;
    ///             requests.recv::<Reply>().await
    ///         },
    ///         notices.recv::<Notice>(),
    ///     )?;
    ///     Ok(())
    /// }
    /// ```
    ///
    /// At least one fragment is required, with at most 256 fragments per
    /// borrow, matching the transport lane range. The core uses no heap storage.
    pub fn par<'p, const N: usize>(
        &'p mut self,
    ) -> Result<[ParallelEndpoint<'p, 'r, ROLE>; N], EndpointError> {
        OperationTarget::new(self).split()
    }
}

impl<'p, 'r, const ROLE: u8> ParallelEndpoint<'p, 'r, ROLE> {
    #[cfg(all(test, hibana_repo_tests))]
    pub(crate) fn proof_tag(&self) -> super::proof_snapshot::ViewTagObservation {
        super::proof_snapshot::operation_tag(
            self.borrowed.ptr,
            OperationHandle {
                endpoint: self.borrowed.handle,
                region: self.borrowed.region,
            },
        )
    }

    /// Execute a permitted send in this fragment, as with [`Endpoint::send`].
    /// This can move a separately owned endpoint payload; the parallel borrow
    /// itself is not an owned endpoint and cannot be sent as one.
    pub fn send<'a, 'e, M>(
        &'e mut self,
        payload: <M::Payload as crate::global::PayloadSpec>::SendArgument<'a, 'r>,
    ) -> impl core::future::Future<Output = Result<(), EndpointError>> + 'a + use<'a, 'e, 'p, 'r, M, ROLE>
    where
        M: crate::g::Message + 'a,
        M::Payload: 'a,
        'e: 'a,
        'r: 'e,
    {
        <M::Payload as crate::global::PayloadSpec>::send::<M, ROLE>(
            OperationTarget::parallel(&mut self.borrowed),
            payload,
        )
    }

    /// Receive a permitted message, as with [`Endpoint::recv`].
    /// A borrowed payload keeps this fragment borrowed until it is released.
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
        <M::Payload as crate::global::PayloadSpec>::recv::<M, ROLE>(OperationTarget::parallel(
            &mut self.borrowed,
        ))
    }

    /// Preview a permitted choice in this fragment, as with [`Endpoint::offer`].
    pub fn offer<'e>(
        &'e mut self,
    ) -> impl core::future::Future<Output = Result<RouteBranch<'e, 'r, ROLE>, EndpointError>> + 'e
    where
        'r: 'e,
    {
        super::futures::OfferFuture::new(OperationTarget::parallel(&mut self.borrowed))
    }

    /// Borrow a nested parallel phase using the same rules as [`Endpoint::par`].
    pub fn par<'e, const N: usize>(
        &'e mut self,
    ) -> Result<[ParallelEndpoint<'e, 'r, ROLE>; N], EndpointError> {
        OperationTarget::parallel(&mut self.borrowed).split()
    }
}

impl<const ROLE: u8> Drop for ParallelEndpoint<'_, '_, ROLE> {
    fn drop(&mut self) {
        let arm = &self.borrowed;
        // SAFETY: all operation futures and borrowed payloads end before the
        // exclusive arm borrow. The parent continues to own the resident view.
        unsafe {
            (arm.ptr.as_ref().ops().drop_parallel)(
                arm.ptr.cast(),
                OperationHandle {
                    endpoint: arm.handle,
                    region: arm.region,
                },
            );
        }
    }
}
