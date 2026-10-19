use super::{NonNull, OperationHandle, OperationRegion};

impl<'cfg, T: crate::transport::Transport + 'cfg> crate::runtime::SessionKit<'cfg, T> {
    pub(super) unsafe fn split_parallel_raw(
        ptr: NonNull<()>,
        handle: OperationHandle,
        regions: &mut [OperationRegion],
    ) -> crate::endpoint::SendResult<()> {
        // SAFETY: the issued parent is exclusively borrowed by the split.
        unsafe {
            Self::with_public_endpoint_mut::<'cfg, _>(
                ptr,
                handle,
                Err(crate::endpoint::SendError::PhaseInvariant),
                |kernel| kernel.split_parallel(regions),
            )
        }
    }

    pub(super) unsafe fn drop_parallel_raw(ptr: NonNull<()>, handle: OperationHandle) {
        // SAFETY: this borrow retains the live parent allocation. Queue its
        // destruction even when an I/O callback already owns the kernel lease.
        unsafe {
            let Some(actor) = Self::resident_actor_from_header::<'cfg>(ptr, handle.endpoint) else {
                return;
            };
            let role = ptr.cast::<super::KernelEndpointHeader>().as_ref().role();
            let view = crate::invariant_some(actor.views.get(role));
            actor.defer_operation(
                view,
                handle.region,
                crate::endpoint::kernel::OperationRetirement::Parallel,
            );
            // A busy entry leaves the queue for the active kernel to reconcile.
            Self::with_public_endpoint_mut::<'cfg, _>(ptr, handle, (), |_| ());
        }
    }
}
