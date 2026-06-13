use super::columns::{
    PROGRAM_IMAGE_ATOM_STRIDE, PROGRAM_IMAGE_NO_ROUTE_CONTROLLER, PROGRAM_IMAGE_RESOLVER_STRIDE,
    PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE, ProgramColumnRange, ProgramImageColumns,
    ProgramImageFacts,
};
use crate::{
    eff::{EffAtom, EffIndex},
    global::compiled::lowering::{CompiledProgramImage, CompiledProgramView},
    global::const_dsl::{CompactScopeId, ResolverMode, ScopeEvent, ScopeId, ScopeKind},
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
        let mut route_resolver_len = 0usize;
        idx = 0;
        while idx < markers.len() {
            let marker = markers[idx];
            if matches!(marker.event, ScopeEvent::Enter)
                && matches!(marker.scope_kind, ScopeKind::Route)
            {
                route_resolver_len += 1;
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
            panic!("program image");
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
            panic!("program image");
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
            panic!("program image");
        }
        column.offset as usize + row * stride
    }

    #[inline(always)]
    const fn encode_resource(resource: Option<u8>) -> u8 {
        match resource {
            Some(tag) => tag,
            None => u8::MAX,
        }
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
            panic!("program image");
        }
        let out = Self::column_offset(column, row, PROGRAM_IMAGE_ATOM_STRIDE);
        self.write_u16(out, offset as u16);
        self.write_u8(out + 2, atom.from);
        self.write_u8(out + 3, atom.to);
        self.write_u8(out + 4, atom.label);
        self.write_u8(out + 5, atom.is_internal as u8);
        self.write_u8(out + 6, Self::encode_resource(atom.resource));
        self.write_u8(out + 7, atom.lane);
    }

    #[inline(always)]
    const fn write_resolver(
        &mut self,
        column: ProgramColumnRange,
        row: usize,
        offset: usize,
        resolver: ResolverMode,
    ) {
        if offset > u16::MAX as usize {
            panic!("program image");
        }
        let resolver_id = match resolver.dynamic_resolver_id() {
            Some(resolver_id) => resolver_id,
            None => u16::MAX,
        };
        let out = Self::column_offset(column, row, PROGRAM_IMAGE_RESOLVER_STRIDE);
        self.write_u16(out, offset as u16);
        self.write_u16(out + 2, resolver_id);
        self.write_u32(
            out + 4,
            CompactScopeId::from_scope_id(resolver.scope()).raw(),
        );
    }

    #[inline(always)]
    const fn write_route_resolver(
        &mut self,
        column: ProgramColumnRange,
        row: usize,
        scope: ScopeId,
        controller_role: Option<u8>,
        decision: Option<(ResolverMode, EffIndex, u8)>,
    ) {
        let out = Self::column_offset(column, row, PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE);
        self.write_u32(out, CompactScopeId::from_scope_id(scope.canonical()).raw());
        let (resolver_id, eff_dense, decision_tag) = match decision {
            Some((resolver, eff, tag)) => {
                let resolver_id = match resolver.dynamic_resolver_id() {
                    Some(resolver_id) => resolver_id,
                    None => u16::MAX,
                };
                let dense = eff.dense_ordinal();
                if dense > u16::MAX as usize {
                    panic!("program image");
                }
                (resolver_id, dense as u16, tag)
            }
            None => (u16::MAX, u16::MAX, 0),
        };
        self.write_u16(out + 4, resolver_id);
        self.write_u16(out + 6, eff_dense);
        self.write_u8(
            out + 8,
            match controller_role {
                Some(role) => role,
                None => PROGRAM_IMAGE_NO_ROUTE_CONTROLLER,
            },
        );
        self.write_u8(out + 9, decision_tag);
    }

    #[inline(always)]
    const fn route_scope_end(
        scope_markers: &[crate::global::const_dsl::ScopeMarker],
        enter_idx: usize,
        scope: ScopeId,
        default_end: usize,
    ) -> usize {
        let mut scope_end = default_end;
        let mut scan_idx = enter_idx + 1;
        let mut nest_depth = 1usize;
        while scan_idx < scope_markers.len() {
            let scan_marker = scope_markers[scan_idx];
            if scan_marker.scope_id.local_ordinal() == scope.local_ordinal() {
                match scan_marker.event {
                    ScopeEvent::Enter => nest_depth += 1,
                    ScopeEvent::Exit => {
                        nest_depth -= 1;
                        if nest_depth == 0 {
                            scope_end = scan_marker.offset;
                            break;
                        }
                    }
                }
            }
            scan_idx += 1;
        }
        scope_end
    }

    #[inline(always)]
    const fn route_resolver_decision(
        view: &CompiledProgramView<'_>,
        route_scope: ScopeId,
        route_enter_marker_idx: usize,
    ) -> Option<(ResolverMode, EffIndex, u8)> {
        let scope_markers = view.scope_markers();
        if route_enter_marker_idx >= scope_markers.len() {
            return None;
        }
        let route_marker = scope_markers[route_enter_marker_idx];
        if !matches!(route_marker.event, ScopeEvent::Enter)
            || !matches!(route_marker.scope_kind, ScopeKind::Route)
            || route_marker.scope_id.canonical().raw() != route_scope.canonical().raw()
        {
            return None;
        }
        let scope_start = route_marker.offset;
        let scope_end = Self::route_scope_end(
            scope_markers,
            route_enter_marker_idx,
            route_marker.scope_id,
            view.len(),
        );
        if scope_start >= crate::eff::meta::MAX_EFF_NODES || scope_start >= scope_end {
            return None;
        }

        let mut marker_idx = route_enter_marker_idx + 1;
        while marker_idx < scope_markers.len() {
            let marker = scope_markers[marker_idx];
            if marker.offset != scope_start {
                break;
            }
            if matches!(marker.event, ScopeEvent::Enter)
                && !matches!(marker.scope_kind, ScopeKind::Generic)
            {
                return None;
            }
            marker_idx += 1;
        }
        let resolver = match view.resident_resolver_at(scope_start) {
            Some(resolver) => resolver,
            None => return None,
        };
        if resolver.dynamic_resolver_id().is_none() {
            return None;
        }
        Some((resolver, EffIndex::from_dense_ordinal(scope_start), 0))
    }

    #[inline(always)]
    pub(crate) const fn from_unselected_bucket_or_empty(image: &CompiledProgramImage) -> Self {
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
            panic!("program image");
        }
        let view = image.view();
        let markers = view.scope_markers();
        if projected_len > u16::MAX as usize {
            panic!("program image");
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
        idx = 0;
        while idx < markers.len() {
            let marker = markers[idx];
            if matches!(marker.event, ScopeEvent::Enter)
                && matches!(marker.scope_kind, ScopeKind::Route)
            {
                let controller = marker.controller_role;
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
