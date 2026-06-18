use super::columns::{
    PROGRAM_IMAGE_ATOM_STRIDE, PROGRAM_IMAGE_INTRINSIC_ROUTE_DECISION_TAG,
    PROGRAM_IMAGE_INTRINSIC_ROUTE_ROLE, PROGRAM_IMAGE_RESOLVER_STRIDE,
    PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE, ProgramColumnRange, ProgramImageColumns,
    ProgramImageFacts,
};
use crate::{
    eff::EffAtom,
    global::compiled::lowering::{CompiledProgramImage, CompiledProgramView},
    global::const_dsl::{
        INTRINSIC_ROUTE_RESOLVER_ID, RouteResolver, ScopeEvent, ScopeId, ScopeKind,
    },
};

#[derive(Clone, Copy)]
pub(crate) struct ProgramImageBytes<const N: usize> {
    bytes: [u8; N],
}

impl<const N: usize> ProgramImageBytes<N> {
    #[inline(always)]
    const fn empty() -> Self {
        Self { bytes: [0; N] }
    }

    #[inline(always)]
    pub(crate) const fn projected_len(image: &CompiledProgramImage) -> usize {
        Self::columns(image).blob_len()
    }

    #[inline(always)]
    pub(crate) const fn columns(image: &CompiledProgramImage) -> ProgramImageColumns {
        let view = image.view();
        let mut atom_len = 0usize;
        let mut resolver_len = 0usize;
        let mut idx = 0usize;
        while idx < view.len() {
            if view.atom_at(idx).is_some() {
                atom_len += 1;
            }
            if view.resident_resolver_at(idx).is_some() {
                resolver_len += 1;
            }
            idx += 1;
        }
        let markers = view.scope_markers();
        let mut seen_route_ordinals = [false; crate::eff::meta::MAX_EFF_NODES];
        let mut route_resolver_len = 0usize;
        idx = 0;
        while idx < markers.len() {
            let marker = markers[idx];
            if matches!(marker.event, ScopeEvent::Enter)
                && matches!(marker.scope_kind, ScopeKind::Route)
            {
                let ordinal = marker.scope_id.local_ordinal() as usize;
                if ordinal >= seen_route_ordinals.len() {
                    crate::invariant();
                }
                if !seen_route_ordinals[ordinal] {
                    seen_route_ordinals[ordinal] = true;
                    route_resolver_len += 1;
                }
            }
            idx += 1;
        }

        let mut offset = 0usize;
        let atoms = ProgramColumnRange::new(offset, atom_len, PROGRAM_IMAGE_ATOM_STRIDE);
        offset = atoms.end_offset(PROGRAM_IMAGE_ATOM_STRIDE);
        let resolvers =
            ProgramColumnRange::new(offset, resolver_len, PROGRAM_IMAGE_RESOLVER_STRIDE);
        offset = resolvers.end_offset(PROGRAM_IMAGE_RESOLVER_STRIDE);
        let route_resolvers = ProgramColumnRange::new(
            offset,
            route_resolver_len,
            PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE,
        );
        let columns = ProgramImageColumns {
            atoms,
            resolvers,
            route_resolvers,
        };
        if offset > columns.blob_len() {
            crate::invariant();
        }
        columns
    }

    #[inline(always)]
    pub(crate) const fn compiled_ref(
        &'static self,
        image: &CompiledProgramImage,
    ) -> super::CompiledProgramRef {
        let facts = ProgramImageFacts::from_image(image);
        let columns = Self::columns(image);
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

    #[inline(always)]
    const fn write_resolver(
        &mut self,
        column: ProgramColumnRange,
        row: usize,
        offset: usize,
        resolver: RouteResolver,
    ) {
        if offset > u16::MAX as usize {
            crate::invariant();
        }
        let resolver_id = match resolver {
            RouteResolver::Dynamic { resolver_id, .. } => resolver_id,
            RouteResolver::Intrinsic => INTRINSIC_ROUTE_RESOLVER_ID,
        };
        let out = Self::column_offset(column, row, PROGRAM_IMAGE_RESOLVER_STRIDE);
        self.write_u16(out, offset as u16);
        self.write_u16(out + 2, resolver_id);
        self.write_u16(out + 4, resolver.scope().raw());
    }

    #[inline(always)]
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
        view: &CompiledProgramView<'_>,
        route_scope: ScopeId,
        route_enter_marker_idx: usize,
    ) -> Option<(RouteResolver, u8)> {
        let scope_markers = view.scope_markers();
        if route_enter_marker_idx >= scope_markers.len() {
            return None;
        }
        let route_marker = scope_markers[route_enter_marker_idx];
        if !matches!(route_marker.event, ScopeEvent::Enter)
            || !matches!(route_marker.scope_kind, ScopeKind::Route)
            || !route_marker.scope_id.same(route_scope)
        {
            return None;
        }
        match view.resolver_for_scope(route_scope) {
            Some(resolver @ RouteResolver::Dynamic { .. }) => Some((resolver, 0)),
            Some(RouteResolver::Intrinsic) | None => None,
        }
    }

    #[inline(always)]
    const fn route_arm_ranges_from_first_enter(
        scope_markers: &[crate::global::const_dsl::ScopeMarker],
        enter_idx: usize,
    ) -> Option<(usize, usize, usize, usize)> {
        if enter_idx >= scope_markers.len() {
            return None;
        }
        let scope_id = scope_markers[enter_idx].scope_id;
        let mut enter_offsets = [usize::MAX; 2];
        let mut exit_offsets = [usize::MAX; 2];
        let mut enter_len = 1usize;
        let mut exit_len = 0usize;
        enter_offsets[0] = scope_markers[enter_idx].offset;
        let mut idx = enter_idx + 1;
        while idx < scope_markers.len() && (enter_len < 2 || exit_len < 2) {
            let marker = scope_markers[idx];
            if marker.scope_id.same(scope_id) && matches!(marker.scope_kind, ScopeKind::Route) {
                match marker.event {
                    ScopeEvent::Enter => {
                        if enter_len < 2 {
                            enter_offsets[enter_len] = marker.offset;
                        }
                        enter_len += 1;
                    }
                    ScopeEvent::Exit => {
                        if exit_len < 2 {
                            exit_offsets[exit_len] = marker.offset;
                        }
                        exit_len += 1;
                    }
                }
            }
            idx += 1;
        }
        if enter_len == 2 && exit_len == 2 {
            Some((
                enter_offsets[0],
                exit_offsets[0],
                enter_offsets[1],
                exit_offsets[1],
            ))
        } else {
            None
        }
    }

    #[inline(always)]
    const fn first_visible_controller_mask(
        view: &CompiledProgramView<'_>,
        start: usize,
        end: usize,
    ) -> u16 {
        let mut seen_lane_words = [0u64; 4];
        let mut controller_mask = 0u16;
        let mut idx = start;
        while idx < end && idx < view.len() {
            if let Some(atom) = view.atom_at(idx) {
                let lane = atom.lane as usize;
                let word = lane / 64;
                let bit = lane % 64;
                if word >= seen_lane_words.len() || atom.from >= u16::BITS as u8 {
                    return 0;
                }
                let mask = 1u64 << bit;
                if (seen_lane_words[word] & mask) != 0 {
                    return controller_mask;
                }
                seen_lane_words[word] |= mask;
                controller_mask |= 1u16 << atom.from;
            }
            idx += 1;
        }
        controller_mask
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
        view: &CompiledProgramView<'_>,
        route_enter_marker_idx: usize,
    ) -> Option<u8> {
        let scope_markers = view.scope_markers();
        let Some((arm0_start, arm0_end, arm1_start, arm1_end)) =
            Self::route_arm_ranges_from_first_enter(scope_markers, route_enter_marker_idx)
        else {
            return None;
        };
        let mask = Self::first_visible_controller_mask(view, arm0_start, arm0_end)
            | Self::first_visible_controller_mask(view, arm1_start, arm1_end);
        Self::unique_controller_role(mask)
    }

    #[inline(always)]
    pub(crate) const fn from_capacity_bucket(image: &CompiledProgramImage) -> Self {
        if Self::projected_len(image) > N {
            return Self::empty();
        }
        Self::from_image(image)
    }

    #[inline(always)]
    pub(crate) const fn from_image(image: &CompiledProgramImage) -> Self {
        let columns = Self::columns(image);
        let projected_len = columns.blob_len();
        if projected_len > N {
            crate::invariant();
        }
        let view = image.view();
        let markers = view.scope_markers();
        if projected_len > u16::MAX as usize {
            crate::invariant();
        }

        let mut out = Self::empty();
        let mut atom_row = 0usize;
        let mut resolver_row = 0usize;
        let mut idx = 0usize;
        while idx < view.len() {
            if let Some(atom) = view.atom_at(idx) {
                out.write_atom(columns.atoms, atom_row, idx, atom);
                atom_row += 1;
            }
            if let Some(resolver) = view.resident_resolver_at(idx) {
                out.write_resolver(columns.resolvers, resolver_row, idx, resolver);
                resolver_row += 1;
            }
            idx += 1;
        }

        let mut route_row = 0usize;
        let mut seen_route_ordinals = [false; crate::eff::meta::MAX_EFF_NODES];
        idx = 0;
        while idx < markers.len() {
            let marker = markers[idx];
            if matches!(marker.event, ScopeEvent::Enter)
                && matches!(marker.scope_kind, ScopeKind::Route)
            {
                let ordinal = marker.scope_id.local_ordinal() as usize;
                if ordinal >= seen_route_ordinals.len() {
                    crate::invariant();
                }
                if seen_route_ordinals[ordinal] {
                    idx += 1;
                    continue;
                }
                seen_route_ordinals[ordinal] = true;
                let controller = Self::route_controller_role(&view, idx);
                let decision = Self::route_resolver_decision(&view, marker.scope_id, idx);
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
