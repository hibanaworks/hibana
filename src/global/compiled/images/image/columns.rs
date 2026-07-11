use crate::{
    eff::EffKind,
    global::{
        compiled::lowering::CompiledProgramImage,
        const_dsl::{EffList, ScopeEvent, ScopeKind},
    },
};

pub(crate) const PROGRAM_IMAGE_ATOM_STRIDE: usize = 11;
pub(crate) const PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE: usize = 5;
pub(crate) const PROGRAM_IMAGE_SCOPE_MARKER_STRIDE: usize = 5;
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
        let byte_len = match len.checked_mul(stride) {
            Some(byte_len) => byte_len,
            None => crate::invariant(),
        };
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

/// Canonical contiguous program-image counts. Column offsets are derived so an
/// alternate layout cannot become a second identity for the same rows.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ProgramImageColumns {
    atom_len: u16,
    route_resolver_len: u16,
    scope_marker_len: u16,
}

impl ProgramImageColumns {
    pub(crate) const fn new(
        atom_len: usize,
        route_resolver_len: usize,
        scope_marker_len: usize,
    ) -> Self {
        if atom_len > u16::MAX as usize
            || route_resolver_len > u16::MAX as usize
            || scope_marker_len > u16::MAX as usize
        {
            crate::invariant();
        }
        let columns = Self {
            atom_len: atom_len as u16,
            route_resolver_len: route_resolver_len as u16,
            scope_marker_len: scope_marker_len as u16,
        };
        let _ = columns.scope_markers();
        columns
    }

    #[inline(always)]
    pub(crate) const fn atoms(self) -> ProgramColumnRange {
        ProgramColumnRange::new(0, self.atom_len as usize, PROGRAM_IMAGE_ATOM_STRIDE)
    }

    #[inline(always)]
    pub(crate) const fn route_resolvers(self) -> ProgramColumnRange {
        ProgramColumnRange::new(
            self.atoms().end_offset(PROGRAM_IMAGE_ATOM_STRIDE),
            self.route_resolver_len as usize,
            PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE,
        )
    }

    #[inline(always)]
    pub(crate) const fn scope_markers(self) -> ProgramColumnRange {
        ProgramColumnRange::new(
            self.route_resolvers()
                .end_offset(PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE),
            self.scope_marker_len as usize,
            PROGRAM_IMAGE_SCOPE_MARKER_STRIDE,
        )
    }

    #[inline(always)]
    pub(crate) const fn atom_count(self) -> usize {
        self.atom_len as usize
    }

    #[inline(always)]
    pub(crate) const fn route_resolver_count(self) -> usize {
        self.route_resolver_len as usize
    }

    #[inline(always)]
    pub(crate) const fn scope_marker_count(self) -> usize {
        self.scope_marker_len as usize
    }

    #[inline(always)]
    pub(crate) const fn blob_len(self) -> usize {
        self.scope_markers()
            .end_offset(PROGRAM_IMAGE_SCOPE_MARKER_STRIDE)
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

    ProgramImageColumns::new(atom_len, route_resolver_len, markers.len())
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
