use super::{EndpointHandle, NonNull, WaiterTransfer};
impl<'cfg, T> crate::runtime::SessionKit<'cfg, T>
where
    T: crate::transport::Transport + 'cfg,
{
    pub(super) unsafe fn drop_public_endpoint_raw(ptr: NonNull<()>, handle: EndpointHandle) {
        let Some(actor) = (/* SAFETY: validate the immutable header and live generation before accessing the resident allocation. */unsafe { Self::resident_actor_from_header::<'cfg>(ptr, handle) })
        else {
            return;
        };
        let mut waiters = WaiterTransfer::empty();
        let operation_lease = actor.context.try_operation_lease();
        let release = /* SAFETY: the sole public owner is being destroyed.
        A callback may drop a different actor while the rendezvous is leased,
        but cannot borrow this actor through the consumed affine Endpoint. */ unsafe {
            let mut kernel = crate::endpoint::kernel::OperationKernel {
                public_header: &actor.public_header,
                context: &actor.context,
                progress: &mut *actor.progress.get(),
                io: &mut *actor.io.get(),
            };
            kernel.clear_endpoint_waiter(&mut waiters);
            kernel.take_owned_slot_release()
        };
        /* SAFETY: endpoint carrier validates the resident header tag and
        generation before projecting the stored endpoint pointer. */
        unsafe {
            core::ptr::drop_in_place(
                ptr.cast::<crate::endpoint::kernel::ResidentActor<'cfg, T>>()
                    .as_ptr(),
            );
        }
        drop(operation_lease);
        if let Some((cluster, rv_id, slot, generation)) = release {
            cluster.release_public_endpoint_slot_owned(rv_id, slot, generation);
        }
    }
}
