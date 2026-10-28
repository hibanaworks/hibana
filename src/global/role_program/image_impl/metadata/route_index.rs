use super::super::super::{BlobPtr, RoleImageColumns};
#[cfg(all(test, hibana_repo_tests))]
use super::super::super::{ColumnRange, ROLE_IMAGE_ROUTE_SCOPE_STRIDE};
use crate::global::const_dsl::ScopeId;

#[inline(always)]
pub(in crate::global::role_program::image_impl) const fn route_slot(
    bytes: BlobPtr,
    index: core::ops::Range<usize>,
    scope: ScopeId,
) -> Option<usize> {
    let query = scope.raw() as usize;
    if query >= (index.end - index.start) / 2 {
        return None;
    }
    let at = index.start + query * 2;
    let slot = bytes.byte_at(at) as u16 | (bytes.byte_at(at + 1) as u16) << 8;
    if slot == u16::MAX {
        None
    } else {
        Some(slot as usize)
    }
}

#[cfg(all(test, hibana_repo_tests))]
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
        let at = routes.offset as usize + slot * 2;
        let raw = bytes[at] as u16 | (bytes[at + 1] as u16) << 8;
        if raw >= ScopeId::LOCAL_CAPACITY || (slot != 0 && previous >= raw) {
            return false;
        }
        previous = raw;
        slot += 1;
    }
    true
}

/// Check both directions of the map. Every route has exactly one slot and
/// every present index entry names that route, including sparse/permuted IDs.
pub(in crate::global::role_program::image_impl) const fn route_index_is_coherent<const N: usize>(
    bytes: &[u8; N],
    columns: RoleImageColumns,
) -> bool {
    let routes = columns.route_scopes;
    let index = columns.route_index();
    if routes.end_offset(2) > N
        || index.end > N
        || columns.route_index_len > ScopeId::LOCAL_CAPACITY
    {
        return false;
    }
    let mut extent = 0;
    let mut slot = 0;
    while slot < routes.len as usize {
        let at = routes.offset as usize + slot * 2;
        let raw = bytes[at] as usize | (bytes[at + 1] as usize) << 8;
        if raw >= columns.route_index_len as usize {
            return false;
        }
        let at = index.start + raw * 2;
        let found = bytes[at] as usize | (bytes[at + 1] as usize) << 8;
        if found != slot {
            return false;
        }
        if raw + 1 > extent {
            extent = raw + 1;
        }
        slot += 1;
    }
    if extent != columns.route_index_len as usize {
        return false;
    }
    let mut raw = 0;
    while raw < extent {
        let at = index.start + raw * 2;
        let slot = bytes[at] as usize | (bytes[at + 1] as usize) << 8;
        if slot != u16::MAX as usize {
            if slot >= routes.len as usize {
                return false;
            }
            let at = routes.offset as usize + slot * 2;
            let found = bytes[at] as usize | (bytes[at + 1] as usize) << 8;
            if found != raw {
                return false;
            }
        }
        raw += 1;
    }
    true
}
