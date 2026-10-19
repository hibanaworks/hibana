use super::{
    Endpoint, EndpointError, EndpointOp, RouteBranch,
    carrier::OperationHandle,
    target::{BorrowedEndpoint, OperationTarget},
};
use core::marker::PhantomData;

#[cfg(all(test, hibana_repo_tests))]
mod tests;

/// An exclusive borrow of one projected parallel arm. It neither owns nor
/// retires the parent view, and cannot be sent as an Endpoint payload.
pub(crate) struct ParallelEndpoint<'p, 'r, const ROLE: u8> {
    borrowed: BorrowedEndpoint,
    _parent: PhantomData<&'p mut Endpoint<'r, ROLE>>,
}

impl<'e, 'r, const ROLE: u8> OperationTarget<'e, 'r, ROLE> {
    fn split(
        self,
    ) -> Result<
        (
            ParallelEndpoint<'e, 'r, ROLE>,
            ParallelEndpoint<'e, 'r, ROLE>,
        ),
        EndpointError,
    > {
        let (ptr, handle) = self.parts();
        // SAFETY: the exclusive parent/arm borrow is transferred to the two
        // disjoint regions derived from its projected global parallel scope.
        let [left, right] = unsafe { (ptr.as_ref().ops().split_parallel)(ptr.cast(), handle) }
            .map_err(|error| EndpointError::new(EndpointOp::Par, error))?;
        let arm = |region| ParallelEndpoint {
            borrowed: BorrowedEndpoint {
                ptr,
                handle: handle.endpoint,
                region,
            },
            _parent: PhantomData,
        };
        Ok((arm(left), arm(right)))
    }
}

impl<'r, const ROLE: u8> Endpoint<'r, ROLE> {
    pub(crate) fn par<'p>(
        &'p mut self,
    ) -> Result<
        (
            ParallelEndpoint<'p, 'r, ROLE>,
            ParallelEndpoint<'p, 'r, ROLE>,
        ),
        EndpointError,
    > {
        OperationTarget::new(self).split()
    }
}

impl<'p, 'r, const ROLE: u8> ParallelEndpoint<'p, 'r, ROLE> {
    pub(crate) fn send<'a, 'e, M>(
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

    pub(crate) fn recv<'e, M>(
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

    pub(crate) fn offer<'e>(
        &'e mut self,
    ) -> impl core::future::Future<Output = Result<RouteBranch<'e, 'r, ROLE>, EndpointError>> + 'e
    where
        'r: 'e,
    {
        super::futures::OfferFuture::new(OperationTarget::parallel(&mut self.borrowed))
    }

    pub(crate) fn par<'e>(
        &'e mut self,
    ) -> Result<
        (
            ParallelEndpoint<'e, 'r, ROLE>,
            ParallelEndpoint<'e, 'r, ROLE>,
        ),
        EndpointError,
    > {
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
