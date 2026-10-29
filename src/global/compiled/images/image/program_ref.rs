use super::columns::{
    PROGRAM_IMAGE_ATOM_STRIDE, ProgramColumnRange, ProgramImageColumns, ProgramImageFacts,
};
use crate::{
    eff::{EffAtom, EventOrigin, EventPayload},
    global::const_dsl::DynamicRouteResolver,
    global::role_program::BlobPtr,
};

mod actors;
mod source_order;
pub(crate) use source_order::SourceOrderIndex;

const fn decode_program_atom(
    bytes: [u8; PROGRAM_IMAGE_ATOM_STRIDE],
    max_role: u8,
) -> Option<EffAtom> {
    if bytes[0] > max_role || bytes[1] > max_role {
        return None;
    }
    if bytes[7] & !(EventPayload::OWNED_FLAG | 1) != 0 {
        return None;
    }
    let origin = match EventOrigin::decode_packed_bits(bytes[7] & 1) {
        Some(origin) => origin,
        None => return None,
    };
    let value = u32::from_le_bytes([bytes[3], bytes[4], bytes[5], bytes[6]]);
    let payload = match EventPayload::decode(value, bytes[7], max_role) {
        Some(payload) => payload,
        None => return None,
    };
    Some(EffAtom {
        from: bytes[0],
        to: bytes[1],
        label: bytes[2],
        payload,
        origin,
        lane: bytes[8],
    })
}

/// Sealed runtime owner for immutable program-wide compiled facts.
#[derive(Clone, Copy)]
pub(crate) struct CompiledProgramRef {
    pub(crate) facts: ProgramImageFacts,
    pub(crate) columns: ProgramImageColumns,
    pub(crate) blob: BlobPtr,
    source_order: BlobPtr,
    pub(super) resolver_index: BlobPtr,
    source_scopes: u16,
    // Independent facts of the checked, contiguous ownership tail.
    actor_binding_start: u16,
    actor_decisions: u16,
}

impl core::fmt::Debug for CompiledProgramRef {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("CompiledProgramRef")
            .field("blob", &(self.blob.as_ptr(), self.blob.len()))
            .finish()
    }
}

impl CompiledProgramRef {
    /// Count coverage uses the checked final bytes, including an owned image's
    /// header. A count-only column prefix cannot certify its ownership tail.
    pub(crate) const fn covers_source_counts(
        &self,
        events: usize,
        scope_markers: usize,
        resolver_markers: usize,
    ) -> bool {
        if !self.columns.has_ownership() {
            return self
                .columns
                .covers_source_counts(events, scope_markers, resolver_markers);
        }
        let Some(rows) = events.checked_add(scope_markers) else {
            return false;
        };
        let Some(rows) = rows.checked_add(resolver_markers) else {
            return false;
        };
        events == self.columns.atom_count()
            && scope_markers == self.source_scope_marker_count()
            && resolver_markers <= self.columns.route_resolver_count()
            && rows <= self.blob.len()
    }

    #[inline(always)]
    pub(crate) const fn compact<const N: usize>(
        facts: ProgramImageFacts,
        columns: ProgramImageColumns,
        bytes: &'static [u8; N],
    ) -> Self {
        let blob = BlobPtr::from_array(bytes, columns.blob_len());
        let mut image = Self {
            facts,
            columns,
            blob,
            source_order: BlobPtr::from_array(bytes, 0),
            resolver_index: BlobPtr::from_array(bytes, 0),
            source_scopes: 0,
            actor_binding_start: 0,
            actor_decisions: 0,
        };
        if let Some(ownership) = image.decode_ownership_columns() {
            image.actor_binding_start = ownership.bindings.offset;
            image.actor_decisions = ownership.decisions.len;
        }
        image.validate_atom_rows();
        // Immutable descriptor validation belongs at construction, not on
        // every endpoint lookup. The private certificate is not protocol state.
        image.validate_route_resolver_rows();
        let Some((scopes, rolls)) = SourceOrderIndex::<0>::layout(
            columns,
            image.source_scope_marker_count(),
            blob.as_slice(),
        ) else {
            crate::invariant();
        };
        image.source_scopes = scopes as u16;
        let len = if scopes == 0 {
            0
        } else {
            scopes * 9 + columns.atom_count() * if rolls { 4 } else { 2 }
        };
        image.source_order = BlobPtr::from_array_range(bytes, columns.blob_len(), len);
        let Some(resolver_extent) =
            super::resolver_index::ResolverIndex::<0>::extent(columns, blob.as_slice())
        else {
            crate::invariant();
        };
        image.resolver_index =
            BlobPtr::from_array_range(bytes, columns.blob_len() + len, resolver_extent * 2);
        if scopes != 0 {
            if N <= crate::global::const_dsl::ScopeId::LOCAL_CAPACITY as usize && scopes <= N {
                image.source_order_index::<N>();
            } else {
                image.source_order_index::<{ crate::global::const_dsl::ScopeId::LOCAL_CAPACITY as usize }>();
            }
        }
        if N <= crate::global::const_dsl::ScopeId::LOCAL_CAPACITY as usize && resolver_extent <= N {
            image.validate_resolver_index::<N>();
        } else {
            image.validate_resolver_index::<{ crate::global::const_dsl::ScopeId::LOCAL_CAPACITY as usize }>();
        }
        image.validate_actor_rows();
        image
    }

    pub(crate) const fn source_order_index<const E: usize>(&self) -> SourceOrderIndex<E> {
        let Some(index) = SourceOrderIndex::<E>::from_program(self) else {
            crate::invariant();
        };
        if index.packed_len() != self.source_order.len() {
            crate::invariant();
        }
        let mut byte = 0;
        while byte < index.packed_len() {
            if index.packed_byte(byte) != self.source_order.byte_at(byte) {
                crate::invariant();
            }
            byte += 1;
        }
        index
    }

    #[inline(always)]
    const fn source_order_u16(&self, offset: usize) -> u16 {
        u16::from_le_bytes([
            self.source_order.byte_at(offset),
            self.source_order.byte_at(offset + 1),
        ])
    }

    pub(crate) fn same_image(&self, other: &Self) -> bool {
        if core::ptr::eq(self, other) {
            return true;
        }
        if self.facts != other.facts || self.columns != other.columns {
            return false;
        }
        let mut offset = 0usize;
        let len = self.blob.len();
        while offset < len {
            if self.byte_at(offset) != other.byte_at(offset) {
                return false;
            }
            offset += 1;
        }
        true
    }

    #[inline(always)]
    pub(super) const fn column_offset(
        &self,
        column: ProgramColumnRange,
        row: usize,
        stride: usize,
    ) -> Option<usize> {
        if row >= column.len as usize {
            return None;
        }
        let offset = column.offset as usize + row * stride;
        if offset + stride > self.blob.len() {
            crate::invariant();
        }
        Some(offset)
    }

    #[inline(always)]
    pub(super) const fn byte_at(&self, offset: usize) -> u8 {
        self.blob.byte_at(offset)
    }

    #[inline(always)]
    pub(super) const fn read_u16_at(&self, offset: usize) -> u16 {
        self.byte_at(offset) as u16 | ((self.byte_at(offset + 1) as u16) << 8)
    }

    // Row position is the dense global event identity. The image builder emits
    // every event in source order, so a duplicate key column and search are absent.
    #[inline(always)]
    pub(crate) const fn atom_at(&self, eff_idx: usize) -> Option<EffAtom> {
        if eff_idx >= crate::eff::meta::COMPACT_EVENT_IDENTITY_CAPACITY {
            crate::invariant();
        }
        let offset =
            match self.column_offset(self.columns.atoms(), eff_idx, PROGRAM_IMAGE_ATOM_STRIDE) {
                Some(offset) => offset,
                None => return None,
            };
        // Construction validates every field before this immutable image is
        // published. Read fields directly so callers that need only a peer or
        // label do not also load and validate the payload.
        let flags = self.byte_at(offset + 7);
        Some(EffAtom {
            from: self.byte_at(offset),
            to: self.byte_at(offset + 1),
            label: self.byte_at(offset + 2),
            payload: if flags & EventPayload::OWNED_FLAG == 0 {
                EventPayload::Wire(u32::from_le_bytes([
                    self.byte_at(offset + 3),
                    self.byte_at(offset + 4),
                    self.byte_at(offset + 5),
                    self.byte_at(offset + 6),
                ]))
            } else {
                EventPayload::Endpoint(self.byte_at(offset + 3))
            },
            origin: if flags & 1 == 0 {
                EventOrigin::User
            } else {
                EventOrigin::Session
            },
            lane: self.byte_at(offset + 8),
        })
    }

    const fn validate_atom_rows(&self) {
        let mut row = 0;
        while row < self.columns.atom_count() {
            let offset =
                match self.column_offset(self.columns.atoms(), row, PROGRAM_IMAGE_ATOM_STRIDE) {
                    Some(offset) => offset,
                    None => crate::invariant(),
                };
            let mut bytes = [0; PROGRAM_IMAGE_ATOM_STRIDE];
            let mut byte = 0;
            while byte < bytes.len() {
                bytes[byte] = self.byte_at(offset + byte);
                byte += 1;
            }
            if decode_program_atom(bytes, self.facts.max_role).is_none() {
                crate::invariant();
            }
            row += 1;
        }
    }

    #[inline(always)]
    pub(crate) const fn event_atom_at(&self, eff_idx: usize) -> EffAtom {
        match self.atom_at(eff_idx) {
            Some(atom) => atom,
            None => crate::invariant(),
        }
    }

    #[inline(always)]
    #[cfg(any(kani, all(test, hibana_repo_tests)))]
    pub(crate) const fn role_count(&self) -> usize {
        self.facts.max_role as usize + 1
    }

    #[cfg(all(test, hibana_repo_tests))]
    pub(crate) const fn proof_atom_count(&self) -> usize {
        self.columns.atom_count()
    }

    #[cfg(all(test, hibana_repo_tests))]
    pub(crate) const fn proof_blob_len(&self) -> usize {
        self.columns.blob_len()
    }

    #[cfg(all(test, hibana_repo_tests))]
    pub(crate) const fn proof_byte_at(&self, offset: usize) -> u8 {
        self.byte_at(offset)
    }

    #[inline(always)]
    pub(crate) fn route_resolver_sites_for(
        &self,
        resolver_id: u16,
    ) -> impl Iterator<Item = DynamicRouteResolver> + '_ {
        crate::session::cluster::effects::ProgramImageRouteResolverSiteIter::new(self)
            .filter(move |resolver| resolver.resolver_id() == resolver_id)
    }
}

#[cfg(kani)]
mod kani;

#[cfg(all(test, hibana_repo_tests))]
mod tests;
