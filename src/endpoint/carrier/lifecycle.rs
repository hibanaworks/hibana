use super::{NonNull, PackedEndpointHandle, WaiterTransfer};
impl<'cfg, T> crate::runtime::SessionKit<'cfg, T>
where
    T: crate::transport::Transport + 'cfg,
{
    pub(super) unsafe fn drop_public_endpoint_raw<const ROLE: u8>(
        ptr: NonNull<()>,
        handle: PackedEndpointHandle,
    ) {
        let Some(endpoint) = (/* SAFETY: endpoint carrier validates the resident header tag and generation before projecting the stored endpoint pointer. */unsafe {
            Self::public_endpoint_ptr_from_header::<'cfg, ROLE>(ptr, handle)
        }) else {
            return;
        };
        let mut waiters = WaiterTransfer::empty();
        let release = /* SAFETY: the validated endpoint pointer is uniquely
        owned by this carrier drop callback until `drop_in_place` returns. */ unsafe {
            let endpoint = &mut *endpoint;
            let operation_lease = endpoint.try_public_operation_lease();
            endpoint.clear_endpoint_waiter(&mut waiters);
            let release = endpoint.take_owned_slot_release();
            (release, operation_lease)
        };
        /* SAFETY: endpoint carrier validates the resident header tag and
        generation before projecting the stored endpoint pointer. */
        unsafe {
            core::ptr::drop_in_place(endpoint);
        }
        let (release, operation_lease) = release;
        drop(operation_lease);
        if let Some((cluster, rv_id, slot, generation)) = release {
            cluster.release_public_endpoint_slot_owned(rv_id, slot, generation);
        }
    }
}
