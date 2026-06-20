use super::columns::{
    PROGRAM_IMAGE_ATOM_STRIDE, PROGRAM_IMAGE_INTRINSIC_ROUTE_DECISION_TAG,
    PROGRAM_IMAGE_INTRINSIC_ROUTE_ROLE, PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE, ProgramColumnRange,
    ProgramImageColumns, ProgramImageFacts, ROUTE_ORDINAL_BYTES, insert_route_ordinal,
};
use crate::{
    eff::{EffAtom, EffKind},
    global::compiled::lowering::CompiledProgramImage,
    global::const_dsl::{
        EffList, INTRINSIC_ROUTE_RESOLVER_ID, RouteResolver, ScopeEvent, ScopeId, ScopeKind,
        parallel_arm_ranges_from_enter, route_arm_ranges_from_first_enter,
    },
};

pub(crate) struct ProgramImageBytes<const N: usize> {
    bytes: [u8; N],
}

impl<const N: usize> ProgramImageBytes<N> {
    #[inline(always)]
    const fn empty() -> Self {
        Self { bytes: [0; N] }
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
        self.write_u8(out + 5, atom.origin.packed_bits());
        self.write_u8(out + 6, atom.lane);
    }

    const fn write_route_resolver(
        &mut self,
        column: ProgramColumnRange,
        row: usize,
        scope: ScopeId,
        controller_role: Option<u8>,
        decision: Option<(RouteResolver, u8)>,
    ) {
        let out = Self::column_offset(column, row, PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE);
        self.write_u16(out, scope.raw());
        let (resolver_id, decision_tag) = match decision {
            Some((resolver, tag)) => {
                let resolver_id = match resolver {
                    RouteResolver::Dynamic { resolver_id, .. } => resolver_id,
                    RouteResolver::Intrinsic => INTRINSIC_ROUTE_RESOLVER_ID,
                };
                (resolver_id, tag)
            }
            None => (
                INTRINSIC_ROUTE_RESOLVER_ID,
                PROGRAM_IMAGE_INTRINSIC_ROUTE_DECISION_TAG,
            ),
        };
        self.write_u16(out + 2, resolver_id);
        self.write_u8(
            out + 4,
            match controller_role {
                Some(role) => role,
                None => PROGRAM_IMAGE_INTRINSIC_ROUTE_ROLE,
            },
        );
        self.write_u8(out + 5, decision_tag);
    }

    #[inline(always)]
    const fn route_resolver_decision(
        eff_list: &EffList,
        route_scope: ScopeId,
        route_enter_marker_idx: usize,
    ) -> Option<(RouteResolver, u8)> {
        let scope_markers = eff_list.scope_markers();
        if route_enter_marker_idx >= scope_markers.len() {
            return None;
        }
        let route_marker = scope_markers[route_enter_marker_idx];
        if !matches!(route_marker.event, ScopeEvent::Enter)
            || !matches!(route_marker.scope_id.kind(), Some(ScopeKind::Route))
            || !route_marker.scope_id.same(route_scope)
        {
            return None;
        }
        match eff_list.resolver_for_scope(route_scope) {
            Some(resolver @ RouteResolver::Dynamic { .. }) => Some((resolver, 0)),
            Some(RouteResolver::Intrinsic) | None => None,
        }
    }

    #[inline(always)]
    const fn unique_controller_role(mask: u16) -> Option<u8> {
        if mask == 0 || (mask & (mask - 1)) != 0 {
            return None;
        }
        let mut role = 0u8;
        while role < u16::BITS as u8 {
            if (mask & (1u16 << role)) != 0 {
                return Some(role);
            }
            role += 1;
        }
        None
    }

    #[inline(always)]
    const fn route_controller_role(
        eff_list: &EffList,
        route_enter_marker_idx: usize,
    ) -> Option<u8> {
        let scope_markers = eff_list.scope_markers();
        if route_enter_marker_idx >= scope_markers.len() {
            return None;
        }
        let (_, arm0_start, arm0_end, _, arm1_start, arm1_end) =
            route_arm_ranges_from_first_enter(scope_markers, route_enter_marker_idx);
        let mask = Self::first_visible_controller_mask(eff_list, arm0_start, arm0_end)
            | Self::first_visible_controller_mask(eff_list, arm1_start, arm1_end);
        Self::unique_controller_role(mask)
    }

    const fn first_visible_controller_mask(eff_list: &EffList, start: usize, end: usize) -> u16 {
        let markers = eff_list.scope_markers();
        let mut idx = start;
        while idx < end && idx < eff_list.len() {
            if let Some(route_enter) = Self::route_enter_at(markers, idx, end) {
                let (_, arm0_start, arm0_end, _, arm1_start, arm1_end) =
                    route_arm_ranges_from_first_enter(markers, route_enter);
                return Self::first_visible_controller_mask(eff_list, arm0_start, arm0_end)
                    | Self::first_visible_controller_mask(eff_list, arm1_start, arm1_end);
            }
            if let Some(par_enter) = Self::parallel_enter_at(markers, idx, end) {
                let Some((arm0_start, arm0_end, arm1_start, arm1_end)) =
                    parallel_arm_ranges_from_enter(markers, par_enter)
                else {
                    return 0;
                };
                return Self::first_visible_controller_mask(eff_list, arm0_start, arm0_end)
                    | Self::first_visible_controller_mask(eff_list, arm1_start, arm1_end);
            }
            if let Some(atom) = Self::atom_at(eff_list, idx) {
                if atom.from >= crate::g::ROLE_DOMAIN_SIZE {
                    return 0;
                }
                return 1u16 << atom.from;
            }
            idx += 1;
        }
        0
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

    const fn route_enter_at(
        markers: &[crate::global::const_dsl::ScopeMarker],
        start: usize,
        end: usize,
    ) -> Option<usize> {
        let mut idx = 0usize;
        while idx < markers.len() {
            let marker = markers[idx];
            if marker.offset() == start
                && matches!(marker.event, ScopeEvent::Enter)
                && matches!(marker.scope_id.kind(), Some(ScopeKind::Route))
                && Self::first_enter_for_scope(markers, idx)
            {
                let (_, _, _, _, _, arm1_end) = route_arm_ranges_from_first_enter(markers, idx);
                if arm1_end <= end {
                    return Some(idx);
                }
            }
            idx += 1;
        }
        None
    }

    const fn first_enter_for_scope(
        markers: &[crate::global::const_dsl::ScopeMarker],
        marker_idx: usize,
    ) -> bool {
        let marker = markers[marker_idx];
        let mut idx = 0usize;
        while idx < marker_idx {
            let candidate = markers[idx];
            if matches!(candidate.event, ScopeEvent::Enter)
                && candidate.scope_id.same(marker.scope_id)
            {
                return false;
            }
            idx += 1;
        }
        true
    }

    const fn parallel_enter_at(
        markers: &[crate::global::const_dsl::ScopeMarker],
        start: usize,
        end: usize,
    ) -> Option<usize> {
        let mut idx = 0usize;
        while idx < markers.len() {
            let marker = markers[idx];
            if marker.offset() == start
                && matches!(marker.event, ScopeEvent::Enter)
                && matches!(marker.scope_id.kind(), Some(ScopeKind::Parallel))
                && matches!(
                    parallel_arm_ranges_from_enter(markers, idx),
                    Some((_, _, _, right_end)) if right_end <= end
                )
            {
                return Some(idx);
            }
            idx += 1;
        }
        None
    }

    #[inline(always)]
    pub(crate) const fn from_capacity_bucket(
        eff_list: &EffList,
        columns: ProgramImageColumns,
    ) -> Self {
        if columns.blob_len() > N {
            return Self::empty();
        }
        Self::from_image(eff_list, columns)
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
                out.write_atom(columns.atoms, atom_row, idx, atom);
                atom_row += 1;
            }
            idx += 1;
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
                let decision = Self::route_resolver_decision(eff_list, marker.scope_id, idx);
                out.write_route_resolver(
                    columns.route_resolvers,
                    route_row,
                    marker.scope_id,
                    controller,
                    decision,
                );
                route_row += 1;
            }
            idx += 1;
        }
        out
    }
}
