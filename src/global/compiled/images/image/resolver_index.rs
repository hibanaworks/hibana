use super::columns::{PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE, ProgramImageColumns};
use crate::global::const_dsl::{ScopeId, ScopeKind};

/// Immutable scope-to-row map. Duplicate internal rows retain first-match
/// semantics; sparse and unsorted identifiers use the same lookup operation.
pub(super) struct ResolverIndex<const E: usize> {
    slots: [u16; E],
    extent: usize,
}

const fn scope_at(columns: ProgramImageColumns, bytes: &[u8], row: usize) -> Option<ScopeId> {
    let routes = columns.route_resolvers();
    if row >= routes.len as usize {
        return None;
    }
    let at = routes.offset as usize + row * PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE;
    if at + PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE > bytes.len() {
        return None;
    }
    let packed = u16::from_le_bytes([bytes[at], bytes[at + 1]]);
    match ScopeId::decode_raw(packed & !ScopeId::RESERVED_BIT) {
        Some(scope) if matches!(scope.kind(), Some(ScopeKind::Route)) => Some(scope),
        _ => None,
    }
}

impl<const E: usize> ResolverIndex<E> {
    pub(super) const fn extent(columns: ProgramImageColumns, bytes: &[u8]) -> Option<usize> {
        let mut extent = 0;
        let mut row = 0;
        while row < columns.route_resolver_count() {
            let Some(scope) = scope_at(columns, bytes, row) else {
                return None;
            };
            let end = scope.raw() as usize + 1;
            if end > extent {
                extent = end;
            }
            row += 1;
        }
        Some(extent)
    }

    pub(super) const fn from_bytes(columns: ProgramImageColumns, bytes: &[u8]) -> Option<Self> {
        let mut index = Self {
            slots: [u16::MAX; E],
            extent: 0,
        };
        let mut row = 0;
        while row < columns.route_resolver_count() {
            let Some(scope) = scope_at(columns, bytes, row) else {
                return None;
            };
            let key = scope.raw() as usize;
            if key >= E {
                return None;
            }
            if index.slots[key] == u16::MAX {
                index.slots[key] = row as u16;
            }
            if key + 1 > index.extent {
                index.extent = key + 1;
            }
            row += 1;
        }
        Some(index)
    }

    pub(super) const fn packed_len(&self) -> usize {
        self.extent * 2
    }

    pub(super) const fn packed_byte(&self, byte: usize) -> u8 {
        if byte >= self.packed_len() {
            crate::invariant();
        }
        (self.slots[byte / 2] >> (8 * (byte % 2))) as u8
    }
}
