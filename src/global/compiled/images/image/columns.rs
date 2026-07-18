use crate::global::{
    compiled::lowering::CompiledProgramImage,
    const_dsl::{EffList, ScopeId, ScopeKind, route_arm_ranges_from_first_enter},
};

pub(crate) const PROGRAM_IMAGE_ATOM_STRIDE: usize = 11;
pub(crate) const PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE: usize = 8;
pub(crate) const PROGRAM_IMAGE_ROUTE_PARTICIPANT_STRIDE: usize = 1;
pub(crate) const PROGRAM_IMAGE_SCOPE_MARKER_STRIDE: usize = 5;
pub(crate) const COMPACT_DESCRIPTOR_BYTE_CAPACITY: usize = u16::MAX as usize;
pub(crate) const PROGRAM_IMAGE_MIN_SCOPE_BYTES: usize = 2 * PROGRAM_IMAGE_SCOPE_MARKER_STRIDE;
const PROGRAM_IMAGE_SCOPE_CAPACITY: usize =
    COMPACT_DESCRIPTOR_BYTE_CAPACITY / PROGRAM_IMAGE_MIN_SCOPE_BYTES;
pub(crate) const PROGRAM_IMAGE_ATOM_ONLY_EVENT_CAPACITY: usize =
    COMPACT_DESCRIPTOR_BYTE_CAPACITY / PROGRAM_IMAGE_ATOM_STRIDE;
const _: () = assert!(
    PROGRAM_IMAGE_ATOM_ONLY_EVENT_CAPACITY < crate::eff::meta::COMPACT_EVENT_IDENTITY_CAPACITY
);
const _: () = assert!(PROGRAM_IMAGE_SCOPE_CAPACITY < ScopeId::LOCAL_CAPACITY as usize);
const _: () = assert!(PROGRAM_IMAGE_SCOPE_CAPACITY < u16::MAX as usize);
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ProgramColumnRange {
    pub(crate) offset: u16,
    pub(crate) len: u16,
}

impl ProgramColumnRange {
    #[inline(always)]
    pub(crate) const fn new(offset: usize, len: usize, stride: usize) -> Self {
        if offset > COMPACT_DESCRIPTOR_BYTE_CAPACITY || len > COMPACT_DESCRIPTOR_BYTE_CAPACITY {
            crate::invariant();
        }
        if stride == 0 {
            crate::invariant();
        }
        let byte_len = match len.checked_mul(stride) {
            Some(byte_len) => byte_len,
            None => crate::invariant(),
        };
        if byte_len > (COMPACT_DESCRIPTOR_BYTE_CAPACITY - offset) {
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
    route_participant_len: u16,
    scope_marker_len: u16,
}

impl ProgramImageColumns {
    pub(crate) const fn try_new(
        atom_len: usize,
        route_resolver_len: usize,
        route_participant_len: usize,
        scope_marker_len: usize,
    ) -> Option<Self> {
        if atom_len > PROGRAM_IMAGE_ATOM_ONLY_EVENT_CAPACITY
            || route_resolver_len > COMPACT_DESCRIPTOR_BYTE_CAPACITY
            || route_participant_len > COMPACT_DESCRIPTOR_BYTE_CAPACITY
            || scope_marker_len > COMPACT_DESCRIPTOR_BYTE_CAPACITY
        {
            return None;
        }
        let atom_bytes = match atom_len.checked_mul(PROGRAM_IMAGE_ATOM_STRIDE) {
            Some(bytes) => bytes,
            None => return None,
        };
        let resolver_bytes =
            match route_resolver_len.checked_mul(PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE) {
                Some(bytes) => bytes,
                None => return None,
            };
        let participant_bytes =
            match route_participant_len.checked_mul(PROGRAM_IMAGE_ROUTE_PARTICIPANT_STRIDE) {
                Some(bytes) => bytes,
                None => return None,
            };
        let marker_bytes = match scope_marker_len.checked_mul(PROGRAM_IMAGE_SCOPE_MARKER_STRIDE) {
            Some(bytes) => bytes,
            None => return None,
        };
        let blob_len = match atom_bytes.checked_add(resolver_bytes) {
            Some(len) => len,
            None => return None,
        };
        let blob_len = match blob_len.checked_add(participant_bytes) {
            Some(len) => len,
            None => return None,
        };
        let blob_len = match blob_len.checked_add(marker_bytes) {
            Some(len) => len,
            None => return None,
        };
        if blob_len > COMPACT_DESCRIPTOR_BYTE_CAPACITY {
            return None;
        }
        Some(Self {
            atom_len: atom_len as u16,
            route_resolver_len: route_resolver_len as u16,
            route_participant_len: route_participant_len as u16,
            scope_marker_len: scope_marker_len as u16,
        })
    }

    pub(crate) const fn new(
        atom_len: usize,
        route_resolver_len: usize,
        route_participant_len: usize,
        scope_marker_len: usize,
    ) -> Self {
        match Self::try_new(
            atom_len,
            route_resolver_len,
            route_participant_len,
            scope_marker_len,
        ) {
            Some(columns) => columns,
            None => crate::invariant(),
        }
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
            self.route_participants()
                .end_offset(PROGRAM_IMAGE_ROUTE_PARTICIPANT_STRIDE),
            self.scope_marker_len as usize,
            PROGRAM_IMAGE_SCOPE_MARKER_STRIDE,
        )
    }

    #[inline(always)]
    pub(crate) const fn route_participants(self) -> ProgramColumnRange {
        ProgramColumnRange::new(
            self.route_resolvers()
                .end_offset(PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE),
            self.route_participant_len as usize,
            PROGRAM_IMAGE_ROUTE_PARTICIPANT_STRIDE,
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
    pub(crate) const fn route_participant_count(self) -> usize {
        self.route_participant_len as usize
    }

    /// Confirm that source-arena counts fit the exact final columns and byte
    /// length. Descriptor translation validation separately owns row content.
    /// Dynamic resolver markers are a subset of route-resolver rows because
    /// intrinsic routes own a row without a marker.
    #[inline(always)]
    pub(crate) const fn covers_source_counts(
        self,
        event_count: usize,
        scope_marker_count: usize,
        resolver_marker_count: usize,
    ) -> bool {
        let Some(source_rows) = event_count.checked_add(scope_marker_count) else {
            return false;
        };
        let Some(source_rows) = source_rows.checked_add(resolver_marker_count) else {
            return false;
        };
        event_count == self.atom_count()
            && scope_marker_count == self.scope_marker_count()
            && resolver_marker_count <= self.route_resolver_count()
            && source_rows <= self.blob_len()
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
    pub(crate) const fn from_program<const E: usize>(eff_list: &EffList<E>) -> Self {
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
const fn program_image_columns<const E: usize>(eff_list: &EffList<E>) -> ProgramImageColumns {
    let atom_len = eff_list.len();

    let markers = eff_list.scope_markers();
    let mut route_resolver_len = 0usize;
    let mut route_participant_len = 0usize;
    let mut idx = 0;
    while idx < markers.len() {
        let marker = markers.at(idx);
        if marker.event.is_primary_enter()
            && matches!(marker.scope_id.kind(), Some(ScopeKind::Route))
        {
            route_resolver_len += 1;
            let [(left_start, left_end), (right_start, right_end)] =
                route_arm_ranges_from_first_enter(markers, idx);
            route_participant_len += route_arm_participant_count(eff_list, left_start, left_end);
            route_participant_len += route_arm_participant_count(eff_list, right_start, right_end);
        }
        idx += 1;
    }

    ProgramImageColumns::new(
        atom_len,
        route_resolver_len,
        route_participant_len,
        markers.len(),
    )
}

const fn role_occurs_before<const E: usize>(
    eff_list: &EffList<E>,
    start: usize,
    end: usize,
    role: u8,
) -> bool {
    let mut idx = start;
    while idx < end && idx < eff_list.len() {
        let atom = eff_list.atom_at(idx);
        if atom.from == role || atom.to == role {
            return true;
        }
        idx += 1;
    }
    false
}

const fn route_arm_participant_count<const E: usize>(
    eff_list: &EffList<E>,
    start: usize,
    end: usize,
) -> usize {
    let mut count = 0usize;
    let mut idx = start;
    while idx < end && idx < eff_list.len() {
        let atom = eff_list.atom_at(idx);
        if !role_occurs_before(eff_list, start, idx, atom.from) {
            count += 1;
        }
        if atom.to != atom.from && !role_occurs_before(eff_list, start, idx, atom.to) {
            count += 1;
        }
        idx += 1;
    }
    if count == 0 {
        crate::invariant();
    }
    count
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ProgramImageFacts {
    pub(crate) max_role: u8,
}

impl ProgramImageFacts {
    #[inline(always)]
    pub(crate) const fn from_image(image: &CompiledProgramImage) -> Self {
        Self {
            max_role: image.max_role(),
        }
    }
}
