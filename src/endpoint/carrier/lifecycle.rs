use super::{NonNull, PackedEndpointHandle};
impl<'cfg, T, const MAX_RV: usize> crate::runtime::SessionKit<'cfg, T, MAX_RV>
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
        /* SAFETY: endpoint carrier validates the resident header tag and generation before projecting the stored endpoint pointer. */
        unsafe {
            core::ptr::drop_in_place(endpoint);
        }
    }
}
