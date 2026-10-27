use super::columns::{
    PROGRAM_IMAGE_ACTOR_BINDING_STRIDE, PROGRAM_IMAGE_ACTOR_DECISION_STRIDE,
    PROGRAM_IMAGE_ATOM_STRIDE, PROGRAM_IMAGE_INITIAL_ACTOR_STRIDE,
    PROGRAM_IMAGE_ROUTE_PARTICIPANT_STRIDE, PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE,
    PROGRAM_IMAGE_SCOPE_MARKER_STRIDE, ProgramColumnRange, ProgramImageColumns, ProgramImageFacts,
    first_endpoint_transfer,
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
    pub(crate) const fn from_source_if_fits<const E: usize>(
        source: &crate::g::ProgramSourceData<E>,
        columns: ProgramImageColumns,
    ) -> Option<Self> {
        let Some(mut out) = Self::from_image_if_fits(source.eff_list(), columns) else {
            return None;
        };
        let Some(actors) = source.actors() else {
            if columns.has_ownership() {
                crate::invariant();
            }
            return Some(out);
        };
        let markers = source.eff_list().scope_markers();
        let Some(ownership) = columns.ownership_columns(markers.len(), actors.decision_count())
        else {
            crate::invariant();
        };
        let header = columns.ownership_header().offset as usize;
        out.write_u16(header, markers.len() as u16);
        out.write_u16(header + 2, actors.decision_count() as u16);
        if actors.event_count() != ownership.bindings.len as usize {
            crate::invariant();
        }
        let mut event = 0;
        while event < actors.event_count() {
            let Some([from, to]) = actors.event_references(event) else {
                crate::invariant();
            };
            let offset = Self::column_offset(
                ownership.bindings,
                event,
                PROGRAM_IMAGE_ACTOR_BINDING_STRIDE,
            );
            out.write_u16(offset, from);
            out.write_u16(offset + 2, to);
            event += 1;
        }

        let mut decision = 0;
        while decision < actors.decision_count() {
            let Some((range, [left, right])) = actors.decision(decision) else {
                crate::invariant();
            };
            let mut scope = None;
            let mut marker_index = 0;
            while marker_index < markers.len() {
                let marker = markers.at(marker_index);
                if marker.event.is_primary_enter()
                    && matches!(marker.scope_id.kind(), Some(ScopeKind::Route))
                {
                    let [(start, split), (_, stop)] =
                        route_arm_ranges_from_first_enter(markers, marker_index);
                    if start == range.start as usize
                        && split == range.split as usize
                        && stop == range.stop as usize
                    {
                        if scope.is_some() {
                            crate::invariant();
                        }
                        scope = Some(marker.scope_id);
                    }
                }
                marker_index += 1;
            }
            let Some(scope) = scope else {
                crate::invariant();
            };
            let offset = Self::column_offset(
                ownership.decisions,
                decision,
                PROGRAM_IMAGE_ACTOR_DECISION_STRIDE,
            );
            out.write_u16(offset, scope.raw());
            out.write_u16(offset + 2, left);
            out.write_u16(offset + 4, right);
            decision += 1;
        }

        let mut row = 0;
        let mut role = 0u16;
        while role <= u8::MAX as u16 {
            if let Some(event) = first_endpoint_transfer(source.eff_list(), role as u8) {
                // The first transfer's sender must agree on the one initial
                // issuer in every source path, as required by source validation.
                let Some((actor, _)) = actors.actors_at(event, &[]) else {
                    crate::invariant();
                };
                let offset =
                    Self::column_offset(ownership.initial, row, PROGRAM_IMAGE_INITIAL_ACTOR_STRIDE);
                out.write_u8(offset, role as u8);
                out.write_u8(offset + 1, actor);
                row += 1;
            }
            role += 1;
        }
        if row != ownership.initial.len as usize {
            crate::invariant();
        }
        Some(out)
    }

    #[inline(always)]
    const fn empty() -> Self {
        Self { bytes: [0; N] }
    }

    #[inline(always)]
    pub(crate) const fn from_image_if_fits<const E: usize>(
        eff_list: &EffList<E>,
        columns: ProgramImageColumns,
    ) -> Option<Self> {
        if columns.blob_len() + super::columns::index_byte_len(eff_list) > N {
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
    const fn write_u32(&mut self, offset: usize, value: u32) {
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
    const fn write_atom(&mut self, column: ProgramColumnRange, row: usize, atom: EffAtom) {
        let out = Self::column_offset(column, row, PROGRAM_IMAGE_ATOM_STRIDE);
        self.write_u8(out, atom.from);
        self.write_u8(out + 1, atom.to);
        self.write_u8(out + 2, atom.label);
        self.write_u32(out + 3, atom.payload.encoded_value());
        self.write_u8(
            out + 7,
            atom.origin.packed_bits() | atom.payload.kind_bits(),
        );
        self.write_u8(out + 8, atom.lane);
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
        if projected_len + super::columns::index_byte_len(eff_list) > N {
            crate::invariant();
        }
        let markers = eff_list.scope_markers();
        let marker_column = if columns.has_ownership() {
            columns.scope_markers_for(markers.len())
        } else {
            columns.scope_markers()
        };
        if projected_len > u16::MAX as usize {
            crate::invariant();
        }

        let mut out = Self::empty();
        let mut atom_row = 0usize;
        let mut idx = 0usize;
        while idx < eff_list.len() {
            out.write_atom(columns.atoms(), atom_row, eff_list.atom_at(idx));
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
            || markers.len() != marker_column.len as usize
        {
            crate::invariant();
        }

        idx = 0;
        while idx < markers.len() {
            out.write_scope_marker(marker_column, idx, markers.at(idx));
            idx += 1;
        }
        let Some((extent, _)) =
            super::SourceOrderIndex::<0>::layout(columns, markers.len(), &out.bytes)
        else {
            crate::invariant();
        };
        let Some(resolver_extent) =
            super::resolver_index::ResolverIndex::<0>::extent(columns, &out.bytes)
        else {
            crate::invariant();
        };
        if N <= ScopeId::LOCAL_CAPACITY as usize && extent <= N && resolver_extent <= N {
            out.write_indexes::<N>(columns, markers.len());
        } else {
            out.write_indexes::<{ ScopeId::LOCAL_CAPACITY as usize }>(columns, markers.len());
        }
        out
    }

    const fn write_indexes<const E: usize>(
        &mut self,
        columns: ProgramImageColumns,
        markers: usize,
    ) {
        let source_empty = matches!(
            super::SourceOrderIndex::<0>::layout(columns, markers, &self.bytes),
            Some((0, _))
        );
        let source_bytes = if source_empty {
            0
        } else {
            let Some(index) =
                super::SourceOrderIndex::<E>::from_bytes(columns, markers, &self.bytes)
            else {
                crate::invariant();
            };
            let mut byte = 0;
            while byte < index.packed_len() {
                self.write_u8(columns.blob_len() + byte, index.packed_byte(byte));
                byte += 1;
            }
            index.packed_len()
        };
        if columns.route_resolver_count() == 0 {
            return;
        }
        let Some(resolvers) =
            super::resolver_index::ResolverIndex::<E>::from_bytes(columns, &self.bytes)
        else {
            crate::invariant();
        };
        let mut byte = 0;
        while byte < resolvers.packed_len() {
            self.write_u8(
                columns.blob_len() + source_bytes + byte,
                resolvers.packed_byte(byte),
            );
            byte += 1;
        }
    }
}
