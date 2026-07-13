use super::columns::{
    PROGRAM_IMAGE_ATOM_STRIDE, PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE,
    PROGRAM_IMAGE_SCOPE_MARKER_STRIDE, ProgramColumnRange, ProgramImageColumns, ProgramImageFacts,
    ROUTE_ORDINAL_BYTES, insert_route_ordinal,
};
use crate::{
    eff::{EffAtom, EffKind},
    global::compiled::lowering::CompiledProgramImage,
    global::const_dsl::{
        DynamicRouteResolver, EffList, INTRINSIC_ROUTE_RESOLVER_ID, ReentryMark, ScopeEvent,
        ScopeId, ScopeKind, ScopeMarker, first_visible_controller_mask,
        route_arm_ranges_from_first_enter, unique_controller_role,
    },
};

/// Injective compact tag for the two non-numeric scope-marker fields.
pub(super) const fn scope_marker_identity_tag(event: ScopeEvent, reentry: ReentryMark) -> u8 {
    let event = match event {
        ScopeEvent::Enter => 0,
        ScopeEvent::Split => 1,
        ScopeEvent::Exit => 2,
    };
    let reentry = match reentry {
        ReentryMark::SinglePass => 0,
        ReentryMark::Reentrant => 1,
    };
    event | (reentry << 2)
}

pub(crate) struct ProgramImageBytes<const N: usize> {
    bytes: [u8; N],
}

impl<const N: usize> ProgramImageBytes<N> {
    #[inline(always)]
    const fn empty() -> Self {
        Self { bytes: [0; N] }
    }

    #[inline(always)]
    pub(crate) const fn from_image_if_fits(
        eff_list: &EffList,
        columns: ProgramImageColumns,
    ) -> Option<Self> {
        if columns.blob_len() > N {
            None
        } else {
            Some(Self::from_image(eff_list, columns))
        }
    }

    #[inline(always)]
    pub(crate) const fn compiled_ref(
        &'static self,
        image: &CompiledProgramImage,
        columns: ProgramImageColumns,
    ) -> super::CompiledProgramRef {
        let facts = ProgramImageFacts::from_image(image);
        super::CompiledProgramRef::compact(facts, columns, &self.bytes)
    }

    #[inline(always)]
    const fn write_u8(&mut self, offset: usize, value: u8) {
        if offset >= self.bytes.len() {
            crate::invariant();
        }
        self.bytes[offset] = value;
    }

    #[inline(always)]
    const fn write_u16(&mut self, offset: usize, value: u16) {
        self.write_u8(offset, value as u8);
        self.write_u8(offset + 1, (value >> 8) as u8);
    }

    #[inline(always)]
    const fn write_payload_schema(&mut self, offset: usize, value: u32) {
        self.write_u16(offset, value as u16);
        self.write_u16(offset + 2, (value >> 16) as u16);
    }

    #[inline(always)]
    const fn column_offset(column: ProgramColumnRange, row: usize, stride: usize) -> usize {
        if row >= column.len as usize {
            crate::invariant();
        }
        column.offset as usize + row * stride
    }

    #[inline(always)]
    const fn write_atom(
        &mut self,
        column: ProgramColumnRange,
        row: usize,
        offset: usize,
        atom: EffAtom,
    ) {
        if offset > u16::MAX as usize {
            crate::invariant();
        }
        let out = Self::column_offset(column, row, PROGRAM_IMAGE_ATOM_STRIDE);
        self.write_u16(out, offset as u16);
        self.write_u8(out + 2, atom.from);
        self.write_u8(out + 3, atom.to);
        self.write_u8(out + 4, atom.label);
        self.write_payload_schema(out + 5, atom.payload_schema);
        self.write_u8(out + 9, atom.origin.packed_bits());
        self.write_u8(out + 10, atom.lane);
    }

    const fn write_route_resolver(
        &mut self,
        column: ProgramColumnRange,
        row: usize,
        scope: ScopeId,
        controller_role: u8,
        resolver: Option<DynamicRouteResolver>,
        arm_participant_masks: [u16; 2],
    ) {
        let out = Self::column_offset(column, row, PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE);
        self.write_u16(out, scope.raw());
        let resolver_id = match resolver {
            Some(resolver) => resolver.resolver_id(),
            None => INTRINSIC_ROUTE_RESOLVER_ID,
        };
        self.write_u16(out + 2, resolver_id);
        self.write_u8(out + 4, controller_role);
        self.write_u16(out + 5, arm_participant_masks[0]);
        self.write_u16(out + 7, arm_participant_masks[1]);
    }

    const fn route_arm_participant_mask(eff_list: &EffList, start: usize, end: usize) -> u16 {
        let mut mask = 0u16;
        let mut idx = start;
        while idx < end && idx < eff_list.len() {
            if matches!(eff_list.node_at(idx).kind, EffKind::Atom) {
                let atom = eff_list.node_at(idx).atom_data();
                if atom.from >= crate::g::ROLE_DOMAIN_SIZE || atom.to >= crate::g::ROLE_DOMAIN_SIZE
                {
                    crate::invariant();
                }
                mask |= 1u16 << atom.from;
                mask |= 1u16 << atom.to;
            }
            idx += 1;
        }
        if mask == 0 {
            crate::invariant();
        }
        mask
    }

    const fn route_arm_participant_masks(
        eff_list: &EffList,
        route_enter_marker_idx: usize,
    ) -> [u16; 2] {
        let markers = eff_list.scope_markers();
        if route_enter_marker_idx >= markers.len() {
            crate::invariant();
        }
        let (_, left_start, left_end, _, right_start, right_end) =
            route_arm_ranges_from_first_enter(markers, route_enter_marker_idx);
        [
            Self::route_arm_participant_mask(eff_list, left_start, left_end),
            Self::route_arm_participant_mask(eff_list, right_start, right_end),
        ]
    }

    const fn write_scope_marker(
        &mut self,
        column: ProgramColumnRange,
        row: usize,
        marker: ScopeMarker,
    ) {
        let out = Self::column_offset(column, row, PROGRAM_IMAGE_SCOPE_MARKER_STRIDE);
        self.write_u16(out, marker.offset() as u16);
        self.write_u16(out + 2, marker.scope_id.raw());
        self.write_u8(
            out + 4,
            scope_marker_identity_tag(marker.event, marker.reentry),
        );
    }

    #[inline(always)]
    const fn route_controller_role(eff_list: &EffList, route_enter_marker_idx: usize) -> u8 {
        let scope_markers = eff_list.scope_markers();
        if route_enter_marker_idx >= scope_markers.len() {
            crate::invariant();
        }
        let (_, arm0_start, arm0_end, _, arm1_start, arm1_end) =
            route_arm_ranges_from_first_enter(scope_markers, route_enter_marker_idx);
        let mask = first_visible_controller_mask(eff_list, arm0_start, arm0_end)
            | first_visible_controller_mask(eff_list, arm1_start, arm1_end);
        match unique_controller_role(mask) {
            Some(role) => role,
            None => crate::invariant(),
        }
    }

    #[inline(always)]
    const fn atom_at(eff_list: &EffList, idx: usize) -> Option<EffAtom> {
        if idx >= eff_list.len() {
            return None;
        }
        let node = eff_list.node_at(idx);
        if matches!(node.kind, EffKind::Atom) {
            Some(node.atom_data())
        } else {
            None
        }
    }

    pub(crate) const fn from_image(eff_list: &EffList, columns: ProgramImageColumns) -> Self {
        let projected_len = columns.blob_len();
        if projected_len > N {
            crate::invariant();
        }
        let markers = eff_list.scope_markers();
        if projected_len > u16::MAX as usize {
            crate::invariant();
        }

        let mut out = Self::empty();
        let mut atom_row = 0usize;
        let mut idx = 0usize;
        while idx < eff_list.len() {
            if let Some(atom) = Self::atom_at(eff_list, idx) {
                out.write_atom(columns.atoms(), atom_row, idx, atom);
                atom_row += 1;
            }
            idx += 1;
        }
        if atom_row != columns.atom_count() {
            crate::invariant();
        }

        let mut route_row = 0usize;
        let mut seen_route_ordinals = [0u8; ROUTE_ORDINAL_BYTES];
        idx = 0;
        while idx < markers.len() {
            let marker = markers[idx];
            if matches!(marker.event, ScopeEvent::Enter)
                && matches!(marker.scope_id.kind(), Some(ScopeKind::Route))
            {
                let ordinal = marker.scope_id.local_ordinal() as usize;
                if !insert_route_ordinal(&mut seen_route_ordinals, ordinal) {
                    idx += 1;
                    continue;
                }
                let controller = Self::route_controller_role(eff_list, idx);
                let resolver = eff_list.resolver_for_scope(marker.scope_id);
                let arm_participant_masks = Self::route_arm_participant_masks(eff_list, idx);
                out.write_route_resolver(
                    columns.route_resolvers(),
                    route_row,
                    marker.scope_id,
                    controller,
                    resolver,
                    arm_participant_masks,
                );
                route_row += 1;
            }
            idx += 1;
        }
        if route_row != columns.route_resolver_count()
            || markers.len() != columns.scope_marker_count()
        {
            crate::invariant();
        }

        idx = 0;
        while idx < markers.len() {
            out.write_scope_marker(columns.scope_markers(), idx, markers[idx]);
            idx += 1;
        }
        out
    }
}
