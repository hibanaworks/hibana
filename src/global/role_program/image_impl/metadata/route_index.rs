use super::super::super::{ColumnRange, ROLE_IMAGE_ROUTE_SCOPE_STRIDE};
use crate::global::const_dsl::ScopeId;

/// Certify that the immutable route column is a strictly sorted raw-ID index.
/// Failure keeps the checked scan; it never rejects or accepts a descriptor.
/// Complete raw values establish valid route kinds and unique authority.
pub(in crate::global::role_program::image_impl) const fn route_scopes_are_sorted<const N: usize>(
    bytes: &[u8; N],
    routes: ColumnRange,
) -> bool {
    if routes.end_offset(ROLE_IMAGE_ROUTE_SCOPE_STRIDE) > N {
        return false;
    }
    let mut previous = 0u16;
    let mut slot = 0usize;
    while slot < routes.len as usize {
        let offset = routes.offset as usize + slot * ROLE_IMAGE_ROUTE_SCOPE_STRIDE;
        let raw = bytes[offset] as u16 | ((bytes[offset + 1] as u16) << 8);
        if raw >= ScopeId::LOCAL_CAPACITY || (slot != 0 && previous >= raw) {
            return false;
        }
        previous = raw;
        slot += 1;
    }
    true
}
