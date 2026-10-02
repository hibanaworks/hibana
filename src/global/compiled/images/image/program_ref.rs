use super::columns::{
    PROGRAM_IMAGE_ATOM_STRIDE, ProgramColumnRange, ProgramImageColumns, ProgramImageFacts,
};
use crate::{
    eff::{EffAtom, EventOrigin},
    global::const_dsl::DynamicRouteResolver,
    global::role_program::BlobPtr,
};

const fn decode_program_atom(
    bytes: [u8; PROGRAM_IMAGE_ATOM_STRIDE],
    max_role: u8,
) -> Option<EffAtom> {
    if bytes[0] > max_role || bytes[1] > max_role {
        return None;
    }
    let origin = match EventOrigin::decode_packed_bits(bytes[7]) {
        Some(origin) => origin,
        None => return None,
    };
    Some(EffAtom {
        from: bytes[0],
        to: bytes[1],
        label: bytes[2],
        payload_schema: u32::from_le_bytes([bytes[3], bytes[4], bytes[5], bytes[6]]),
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
        image.validate_atom_rows();
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
        let mut bytes = [0u8; PROGRAM_IMAGE_ATOM_STRIDE];
        let mut byte = 0;
        while byte < bytes.len() {
            bytes[byte] = self.byte_at(offset + byte);
            byte += 1;
        }
        match decode_program_atom(bytes, self.facts.max_role) {
            Some(atom) => Some(atom),
            None => crate::invariant(),
        }
    }

    const fn validate_atom_rows(&self) {
        let mut row = 0;
        while row < self.columns.atom_count() {
            if self.atom_at(row).is_none() {
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
