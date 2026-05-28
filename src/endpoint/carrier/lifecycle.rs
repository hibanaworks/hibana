use super::{
    KernelEndpointHeader, Lane, NonNull, PackedEndpointHandle, PublicKernelEndpoint, SessionId,
};
impl<'cfg, T, U, C, const MAX_RV: usize> crate::integration::SessionKit<'cfg, T, U, C, MAX_RV>
where
    T: crate::transport::Transport + 'cfg,
    U: crate::runtime::consts::LabelUniverse + 'cfg,
    C: crate::runtime::config::Clock + 'cfg,
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
        /* SAFETY: endpoint carrier validates the resident header tag and generation before projecting the stored endpoint pointer. */
        unsafe {
            core::ptr::drop_in_place(endpoint);
        }
    }

    pub(super) unsafe fn revoke_public_endpoint_raw<const ROLE: u8>(
        ptr: NonNull<()>,
        sid: SessionId,
        lanes: *mut Lane,
        lane_capacity: usize,
    ) -> usize {
        let header = /* SAFETY: endpoint carrier validates the resident header tag and generation before projecting the stored endpoint pointer. */ unsafe { ptr.cast::<KernelEndpointHeader<'cfg>>().as_ref() };
        if header.role() != ROLE || header.generation() == 0 {
            return 0;
        }
        let endpoint = ptr
            .cast::<PublicKernelEndpoint<'cfg, ROLE, T, U, C, MAX_RV>>()
            .as_ptr();
        let endpoint = /* SAFETY: the pointer comes from pinned owner storage and this path holds the unique mutable access for the borrow. */ unsafe { &mut *endpoint };
        if !endpoint.matches_session(sid) {
            return 0;
        }

        let mut released = 0usize;
        endpoint.for_each_physical_lane(|owned_lane| {
            if released < lane_capacity {
                /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
                unsafe {
                    lanes.add(released).write(owned_lane);
                }
            }
            released += 1;
        });
        debug_assert!(
            released <= lane_capacity,
            "public endpoint revoke lane buffer must cover every owned lane"
        );
        endpoint.revoke_public_owner();
        core::cmp::min(released, lane_capacity)
    }
}
