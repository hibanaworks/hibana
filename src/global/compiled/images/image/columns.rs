use crate::{
    eff::EffKind,
    global::{
        compiled::lowering::CompiledProgramImage,
        const_dsl::{EffList, ScopeEvent, ScopeKind},
    },
};

pub(crate) const PROGRAM_IMAGE_ATOM_STRIDE: usize = 7;
pub(crate) const PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE: usize = 5;
pub(crate) const PROGRAM_IMAGE_ROUTE_CONTROLLER_ABSENT: u8 = u8::MAX;
pub(super) const ROUTE_ORDINAL_BYTES: usize = crate::eff::meta::MAX_EFF_NODES.div_ceil(8);

#[inline(always)]
pub(super) const fn insert_route_ordinal(
    words: &mut [u8; ROUTE_ORDINAL_BYTES],
    ordinal: usize,
) -> bool {
    let byte = ordinal >> 3;
    let bit = ordinal & 7;
    if byte >= words.len() {
        crate::invariant();
    }
    let mask = 1u8 << bit;
    let seen = (words[byte] & mask) != 0;
    if !seen {
        words[byte] |= mask;
    }
    !seen
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct ProgramColumnRange {
    pub(crate) offset: u16,
    pub(crate) len: u16,
}

impl ProgramColumnRange {
    #[inline(always)]
    pub(crate) const fn new(offset: usize, len: usize, stride: usize) -> Self {
        if offset > u16::MAX as usize || len > u16::MAX as usize {
            crate::invariant();
        }
        if stride == 0 {
            crate::invariant();
        }
        let byte_len = len * stride;
        if byte_len > (u16::MAX as usize - offset) {
            crate::invariant();
        }
        Self {
            offset: offset as u16,
            len: len as u16,
        }
    }

    #[inline(always)]
    pub(crate) const fn byte_len(self, stride: usize) -> usize {
        self.len as usize * stride
    }

    #[inline(always)]
    pub(crate) const fn end_offset(self, stride: usize) -> usize {
        self.offset as usize + self.byte_len(stride)
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ProgramImageColumns {
    pub(crate) atoms: ProgramColumnRange,
    pub(crate) route_resolvers: ProgramColumnRange,
}

impl ProgramImageColumns {
    #[inline(always)]
    pub(crate) const fn blob_len(self) -> usize {
        let mut len = self.atoms.end_offset(PROGRAM_IMAGE_ATOM_STRIDE);
        if self
            .route_resolvers
            .end_offset(PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE)
            > len
        {
            len = self
                .route_resolvers
                .end_offset(PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE);
        }
        len
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ProgramImagePlan {
    columns: ProgramImageColumns,
}

impl ProgramImagePlan {
    #[inline(always)]
    pub(crate) const fn from_program(eff_list: &EffList) -> Self {
        Self {
            columns: program_image_columns(eff_list),
        }
    }

    #[inline(always)]
    pub(crate) const fn columns(self) -> ProgramImageColumns {
        self.columns
    }

    #[inline(always)]
    pub(crate) const fn blob_len(self) -> usize {
        self.columns.blob_len()
    }
}

#[inline]
const fn program_image_columns(eff_list: &EffList) -> ProgramImageColumns {
    let mut atom_len = 0usize;
    let mut idx = 0usize;
    while idx < eff_list.len() {
        let node = eff_list.node_at(idx);
        if matches!(node.kind, EffKind::Atom) {
            atom_len += 1;
        }
        idx += 1;
    }

    let markers = eff_list.scope_markers();
    let mut seen_route_ordinals = [0u8; ROUTE_ORDINAL_BYTES];
    let mut route_resolver_len = 0usize;
    idx = 0;
    while idx < markers.len() {
        let marker = markers[idx];
        if matches!(marker.event, ScopeEvent::Enter)
            && matches!(marker.scope_id.kind(), Some(ScopeKind::Route))
        {
            let ordinal = marker.scope_id.local_ordinal() as usize;
            if insert_route_ordinal(&mut seen_route_ordinals, ordinal) {
                route_resolver_len += 1;
            }
        }
        idx += 1;
    }

    let mut offset = 0usize;
    let atoms = ProgramColumnRange::new(offset, atom_len, PROGRAM_IMAGE_ATOM_STRIDE);
    offset = atoms.end_offset(PROGRAM_IMAGE_ATOM_STRIDE);
    let route_resolvers = ProgramColumnRange::new(
        offset,
        route_resolver_len,
        PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE,
    );
    let columns = ProgramImageColumns {
        atoms,
        route_resolvers,
    };
    if offset > columns.blob_len() {
        crate::invariant();
    }
    columns
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ProgramImageFacts {
    pub(crate) role_count: u8,
}

impl ProgramImageFacts {
    #[inline(always)]
    pub(crate) const fn from_image(image: &CompiledProgramImage) -> Self {
        Self {
            role_count: image.compiled_program_role_count() as u8,
        }
    }
}
