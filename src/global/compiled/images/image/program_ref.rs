use super::columns::{
    PROGRAM_IMAGE_ATOM_STRIDE, ProgramColumnRange, ProgramImageColumns, ProgramImageFacts,
};
use crate::{
    eff::{EffAtom, EffStruct, EventOrigin},
    global::const_dsl::DynamicRouteResolver,
    global::role_program::BlobPtr,
};

#[derive(Clone, Copy)]
struct ProgramAtomRow {
    eff_idx: u16,
    atom: EffAtom,
}

#[derive(Clone, Copy)]
struct PackedProgramAtomFields {
    from: u8,
    to: u8,
    label: u8,
    payload_schema: u32,
    origin: u8,
    lane: u8,
}

impl ProgramAtomRow {
    const fn decode(eff_idx: u16, fields: PackedProgramAtomFields, max_role: u8) -> Option<Self> {
        if eff_idx as usize >= crate::eff::meta::COMPACT_EVENT_IDENTITY_CAPACITY
            || fields.from > max_role
            || fields.to > max_role
        {
            return None;
        }
        let origin = match EventOrigin::decode_packed_bits(fields.origin) {
            Some(origin) => origin,
            None => return None,
        };
        Some(Self {
            eff_idx,
            atom: EffAtom {
                from: fields.from,
                to: fields.to,
                label: fields.label,
                payload_schema: fields.payload_schema,
                origin,
                lane: fields.lane,
            },
        })
    }
}

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
        let image = Self {
            facts,
            columns,
            blob,
        };
        image.validate_atom_order();
        image
    }

    pub(crate) fn same_image(&self, other: &Self) -> bool {
        if core::ptr::eq(self, other) {
            return true;
        }
        if self.facts != other.facts || self.columns != other.columns {
            return false;
        }
        let mut offset = 0usize;
        let len = self.columns.blob_len();
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
    const fn read_payload_schema_at(&self, offset: usize) -> u32 {
        self.read_u16_at(offset) as u32 | ((self.read_u16_at(offset + 2) as u32) << 16)
    }

    #[inline]
    const fn atom_row_at(&self, row: usize) -> Option<ProgramAtomRow> {
        let offset = match self.column_offset(self.columns.atoms(), row, PROGRAM_IMAGE_ATOM_STRIDE)
        {
            Some(offset) => offset,
            None => return None,
        };
        match ProgramAtomRow::decode(
            self.read_u16_at(offset),
            PackedProgramAtomFields {
                from: self.byte_at(offset + 2),
                to: self.byte_at(offset + 3),
                label: self.byte_at(offset + 4),
                payload_schema: self.read_payload_schema_at(offset + 5),
                origin: self.byte_at(offset + 9),
                lane: self.byte_at(offset + 10),
            },
            self.facts.max_role,
        ) {
            Some(row) => Some(row),
            None => crate::invariant(),
        }
    }

    const fn validate_atom_order(&self) {
        let mut row = 1usize;
        while row < self.columns.atom_count() {
            let previous_offset = match self.column_offset(
                self.columns.atoms(),
                row - 1,
                PROGRAM_IMAGE_ATOM_STRIDE,
            ) {
                Some(offset) => offset,
                None => crate::invariant(),
            };
            let offset =
                match self.column_offset(self.columns.atoms(), row, PROGRAM_IMAGE_ATOM_STRIDE) {
                    Some(offset) => offset,
                    None => crate::invariant(),
                };
            if self.read_u16_at(previous_offset) >= self.read_u16_at(offset) {
                crate::invariant();
            }
            row += 1;
        }
    }

    #[inline]
    pub(crate) const fn atom_at(&self, eff_idx: usize) -> Option<EffAtom> {
        if eff_idx >= crate::eff::meta::COMPACT_EVENT_IDENTITY_CAPACITY {
            crate::invariant();
        }
        let mut start = 0usize;
        let mut end = self.columns.atom_count();
        while start < end {
            let row = start + (end - start) / 2;
            let decoded = match self.atom_row_at(row) {
                Some(decoded) => decoded,
                None => crate::invariant(),
            };
            if (decoded.eff_idx as usize) < eff_idx {
                start = row + 1;
            } else {
                end = row;
            }
        }
        if start >= self.columns.atom_count() {
            return None;
        }
        let decoded = match self.atom_row_at(start) {
            Some(decoded) => decoded,
            None => crate::invariant(),
        };
        if decoded.eff_idx as usize == eff_idx {
            Some(decoded.atom)
        } else {
            None
        }
    }

    #[inline(always)]
    pub(crate) const fn node_at(&self, eff_idx: usize) -> EffStruct {
        match self.atom_at(eff_idx) {
            Some(atom) => EffStruct::atom(atom),
            None => EffStruct::pure(),
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
