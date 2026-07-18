use super::columns::{
    PROGRAM_IMAGE_ATOM_STRIDE, PROGRAM_IMAGE_ROUTE_PARTICIPANT_STRIDE,
    PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE, PROGRAM_IMAGE_SCOPE_MARKER_STRIDE, ProgramColumnRange,
    ProgramImageColumns, ProgramImageFacts,
};
use super::route_resolvers::PackedRouteAuthority;
use crate::{
    eff::EffAtom,
    global::compiled::lowering::CompiledProgramImage,
    global::const_dsl::{
        DynamicRouteResolver, EffList, ReentryMark, ScopeEvent, ScopeId, ScopeKind, ScopeMarker,
        first_visible_controller, route_arm_ranges_from_first_enter,
    },
};

#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg_attr(kani, derive(kani::Arbitrary))]
pub(super) enum DescriptorScopeEvent {
    Enter,
    Split,
    Exit,
}

pub(super) const fn erase_scope_event(event: ScopeEvent) -> DescriptorScopeEvent {
    match event {
        ScopeEvent::Enter(_) => DescriptorScopeEvent::Enter,
        ScopeEvent::Split => DescriptorScopeEvent::Split,
        ScopeEvent::Exit => DescriptorScopeEvent::Exit,
    }
}

/// Injective compact tag for the two descriptor-resident marker fields.
pub(super) const fn scope_marker_identity_tag(
    event: DescriptorScopeEvent,
    reentry: ReentryMark,
) -> u8 {
    let event = match event {
        DescriptorScopeEvent::Enter => 0,
        DescriptorScopeEvent::Split => 1,
        DescriptorScopeEvent::Exit => 2,
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
    pub(crate) const fn from_image_if_fits<const E: usize>(
        eff_list: &EffList<E>,
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
        participant_boundaries: [u16; 3],
    ) {
        let left_len = participant_boundaries[1] - participant_boundaries[0];
        let right_len = participant_boundaries[2] - participant_boundaries[1];
        if left_len == 0 || left_len > 256 || right_len == 0 || right_len > 256 {
            crate::invariant();
        }
        let out = Self::column_offset(column, row, PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE);
        let authority = PackedRouteAuthority::encode(scope, resolver);
        self.write_u16(out, authority.packed_scope());
        self.write_u16(out + 2, authority.resolver_id());
        self.write_u8(out + 4, controller_role);
        self.write_u16(out + 5, participant_boundaries[0]);
        self.write_u8(out + 7, (left_len - 1) as u8);
    }

    const fn next_route_arm_participant<const E: usize>(
        eff_list: &EffList<E>,
        start: usize,
        end: usize,
        role_floor: u16,
    ) -> Option<u8> {
        let mut candidate = None;
        let mut idx = start;
        while idx < end && idx < eff_list.len() {
            let atom = eff_list.atom_at(idx);
            let from = atom.from as u16;
            if from >= role_floor
                && match candidate {
                    Some(current) => atom.from < current,
                    None => true,
                }
            {
                candidate = Some(atom.from);
            }
            let to = atom.to as u16;
            if to >= role_floor
                && match candidate {
                    Some(current) => atom.to < current,
                    None => true,
                }
            {
                candidate = Some(atom.to);
            }
            idx += 1;
        }
        candidate
    }

    const fn write_route_arm_participants<const E: usize>(
        &mut self,
        column: ProgramColumnRange,
        mut row: usize,
        eff_list: &EffList<E>,
        start: usize,
        end: usize,
    ) -> usize {
        let first_row = row;
        let mut role_floor = 0u16;
        while role_floor <= u8::MAX as u16 {
            let Some(role) = Self::next_route_arm_participant(eff_list, start, end, role_floor)
            else {
                break;
            };
            let out = Self::column_offset(column, row, PROGRAM_IMAGE_ROUTE_PARTICIPANT_STRIDE);
            self.write_u8(out, role);
            row += 1;
            role_floor = role as u16 + 1;
        }
        if row == first_row {
            crate::invariant();
        }
        row
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
            scope_marker_identity_tag(erase_scope_event(marker.event), marker.reentry),
        );
    }

    #[inline(always)]
    const fn route_controller_role<const E: usize>(
        eff_list: &EffList<E>,
        route_enter_marker_idx: usize,
    ) -> u8 {
        let scope_markers = eff_list.scope_markers();
        if route_enter_marker_idx >= scope_markers.len() {
            crate::invariant();
        }
        let [(arm0_start, arm0_end), (arm1_start, arm1_end)] =
            route_arm_ranges_from_first_enter(scope_markers, route_enter_marker_idx);
        match first_visible_controller(eff_list, arm0_start, arm0_end)
            .merge(first_visible_controller(eff_list, arm1_start, arm1_end))
            .unique()
        {
            Some(role) => role,
            None => crate::invariant(),
        }
    }

    pub(crate) const fn from_image<const E: usize>(
        eff_list: &EffList<E>,
        columns: ProgramImageColumns,
    ) -> Self {
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
            out.write_atom(columns.atoms(), atom_row, idx, eff_list.atom_at(idx));
            atom_row += 1;
            idx += 1;
        }
        if atom_row != columns.atom_count() {
            crate::invariant();
        }

        let mut route_row = 0usize;
        let mut participant_row = 0usize;
        idx = 0;
        while idx < markers.len() {
            let marker = markers.at(idx);
            if marker.event.is_primary_enter()
                && matches!(marker.scope_id.kind(), Some(ScopeKind::Route))
            {
                let controller = Self::route_controller_role(eff_list, idx);
                let resolver = eff_list.resolver_for_scope(marker.scope_id);
                let [(left_start, left_end), (right_start, right_end)] =
                    route_arm_ranges_from_first_enter(markers, idx);
                let participant_start = participant_row;
                participant_row = out.write_route_arm_participants(
                    columns.route_participants(),
                    participant_row,
                    eff_list,
                    left_start,
                    left_end,
                );
                let participant_mid = participant_row;
                participant_row = out.write_route_arm_participants(
                    columns.route_participants(),
                    participant_row,
                    eff_list,
                    right_start,
                    right_end,
                );
                if participant_row > u16::MAX as usize {
                    crate::invariant();
                }
                out.write_route_resolver(
                    columns.route_resolvers(),
                    route_row,
                    marker.scope_id,
                    controller,
                    resolver,
                    [
                        participant_start as u16,
                        participant_mid as u16,
                        participant_row as u16,
                    ],
                );
                route_row += 1;
            }
            idx += 1;
        }
        if route_row != columns.route_resolver_count()
            || participant_row != columns.route_participant_count()
            || markers.len() != columns.scope_marker_count()
        {
            crate::invariant();
        }

        idx = 0;
        while idx < markers.len() {
            out.write_scope_marker(columns.scope_markers(), idx, markers.at(idx));
            idx += 1;
        }
        out
    }
}
