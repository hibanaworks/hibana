use super::{NonNull, OperationHandle, OperationRegion, WaiterTransfer};

impl<'cfg, T: crate::transport::Transport + 'cfg> crate::runtime::SessionKit<'cfg, T> {
    pub(super) unsafe fn split_parallel_raw(
        ptr: NonNull<()>,
        handle: OperationHandle,
    ) -> crate::endpoint::SendResult<[OperationRegion; 2]> {
        // SAFETY: the issued parent is exclusively borrowed by the split.
        unsafe {
            Self::with_public_endpoint_mut::<'cfg, _>(
                ptr,
                handle,
                Err(crate::endpoint::SendError::PhaseInvariant),
                |kernel| kernel.split_parallel(),
            )
        }
    }

    pub(super) unsafe fn drop_parallel_raw(ptr: NonNull<()>, handle: OperationHandle) {
        let mut waiters = WaiterTransfer::empty();
        // SAFETY: the arm's Rust borrow excludes any live operation future.
        unsafe {
            Self::with_public_endpoint_mut::<'cfg, _>(ptr, handle, (), |kernel| {
                kernel.finish_parallel(&mut waiters);
            });
            // The kernel borrow has ended before arbitrary sibling wakeups.
            Self::wake_public_actor_waiters(ptr, handle.endpoint);
        }
    }
}
