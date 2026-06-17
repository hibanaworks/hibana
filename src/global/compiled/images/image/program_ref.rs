use super::columns::{
    PROGRAM_IMAGE_ATOM_STRIDE, PROGRAM_IMAGE_RESOLVER_STRIDE, ProgramColumnRange,
    ProgramImageColumns, ProgramImageFacts,
};
use crate::{
    eff::{EffAtom, EffStruct, EventOrigin},
    global::compiled::images::program::DynamicResolverSite,
    global::const_dsl::{CompactScopeId, INTRINSIC_ROUTE_RESOLVER_ID, RouteResolver},
    global::role_program::BlobPtr,
};

/// Sealed runtime owner for immutable program-wide compiled facts.
#[derive(Clone, Copy)]
pub(crate) struct CompiledProgramRef {
    pub(crate) facts: ProgramImageFacts,
    pub(crate) columns: ProgramImageColumns,
    pub(crate) blob: BlobPtr,
}

impl core::fmt::Debug for CompiledProgramRef {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("CompiledProgramRef")
            .field("blob", &(self.blob.as_ptr(), self.columns.blob_len()))
            .finish()
    }
}

impl CompiledProgramRef {
    #[inline(always)]
    pub(crate) const fn compact<const N: usize>(
        facts: ProgramImageFacts,
        columns: ProgramImageColumns,
        bytes: &'static [u8; N],
    ) -> Self {
        let blob = BlobPtr::from_array(bytes, columns.blob_len());
        Self {
            facts,
            columns,
            blob,
        }
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
        if offset + stride > self.columns.blob_len() {
            crate::invariant();
        }
        Some(offset)
    }

    #[inline(always)]
    pub(super) const fn byte_at(&self, offset: usize) -> u8 {
        if offset >= self.columns.blob_len() {
            crate::invariant();
        }
        self.blob.byte_at(offset)
    }

    #[inline(always)]
    pub(super) const fn read_u16_at(&self, offset: usize) -> u16 {
        self.byte_at(offset) as u16 | ((self.byte_at(offset + 1) as u16) << 8)
    }

    #[inline(always)]
    pub(super) const fn read_u32_at(&self, offset: usize) -> u32 {
        self.read_u16_at(offset) as u32 | ((self.read_u16_at(offset + 2) as u32) << 16)
    }

    #[inline(always)]
    pub(super) const fn compact_scope_from_bits(raw: u32) -> CompactScopeId {
        match CompactScopeId::decode_raw(raw) {
            Some(scope) => scope,
            None => crate::invariant(),
        }
    }

    #[inline(always)]
    pub(crate) const fn atom_at(&self, eff_idx: usize) -> Option<EffAtom> {
        let mut row = 0usize;
        while row < self.columns.atoms.len as usize {
            let offset =
                match self.column_offset(self.columns.atoms, row, PROGRAM_IMAGE_ATOM_STRIDE) {
                    Some(offset) => offset,
                    None => return None,
                };
            if self.read_u16_at(offset) as usize == eff_idx {
                return Some(EffAtom {
                    from: self.byte_at(offset + 2),
                    to: self.byte_at(offset + 3),
                    label: self.byte_at(offset + 4),
                    origin: EventOrigin::from_packed_bits(self.byte_at(offset + 5)),
                    lane: self.byte_at(offset + 6),
                });
            }
            row += 1;
        }
        None
    }

    #[inline(always)]
    pub(crate) const fn node_at(&self, eff_idx: usize) -> EffStruct {
        match self.atom_at(eff_idx) {
            Some(atom) => EffStruct::atom(atom),
            None => EffStruct::pure(),
        }
    }

    #[inline(always)]
    pub(crate) const fn atom_row_count(&self) -> usize {
        self.columns.atoms.len as usize
    }

    #[inline(always)]
    pub(crate) const fn atom_eff_at_row(&self, row: usize) -> Option<usize> {
        let offset = match self.column_offset(self.columns.atoms, row, PROGRAM_IMAGE_ATOM_STRIDE) {
            Some(offset) => offset,
            None => return None,
        };
        Some(self.read_u16_at(offset) as usize)
    }

    #[inline(always)]
    pub(crate) const fn resident_resolver_at(&self, eff_idx: usize) -> Option<RouteResolver> {
        let mut row = 0usize;
        while row < self.columns.resolvers.len as usize {
            let offset = match self.column_offset(
                self.columns.resolvers,
                row,
                PROGRAM_IMAGE_RESOLVER_STRIDE,
            ) {
                Some(offset) => offset,
                None => return None,
            };
            if self.read_u16_at(offset) as usize == eff_idx {
                let resolver_id = self.read_u16_at(offset + 2);
                if resolver_id == INTRINSIC_ROUTE_RESOLVER_ID {
                    return Some(RouteResolver::Intrinsic);
                }
                return Some(RouteResolver::Dynamic {
                    resolver_id,
                    scope: Self::compact_scope_from_bits(self.read_u32_at(offset + 4)),
                });
            }
            row += 1;
        }
        None
    }

    #[inline(always)]
    pub(crate) const fn role_count(&self) -> usize {
        self.facts.role_count as usize
    }

    #[inline(always)]
    pub(crate) fn dynamic_resolver_sites_for(
        &self,
        resolver_id: u16,
    ) -> impl Iterator<Item = DynamicResolverSite> + '_ {
        crate::session::cluster::effects::ProgramImageDynamicResolverSiteIter::new(self)
            .filter(move |site| site.resolver_id() == resolver_id)
    }
}
