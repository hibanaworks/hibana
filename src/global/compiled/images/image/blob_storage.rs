use super::columns::{
    PROGRAM_IMAGE_ATOM_STRIDE, PROGRAM_IMAGE_CONTROL_DESC_STRIDE,
    PROGRAM_IMAGE_NO_ROUTE_CONTROLLER, PROGRAM_IMAGE_POLICY_STRIDE,
    PROGRAM_IMAGE_ROUTE_CONTROL_STRIDE, PROGRAM_IMAGE_SUBJECT_LOOP_BREAK,
    PROGRAM_IMAGE_SUBJECT_LOOP_CONTINUE, PROGRAM_IMAGE_SUBJECT_NONE,
    PROGRAM_IMAGE_SUBJECT_ROUTE_ARM, ProgramColumnRange, ProgramImageColumns, ProgramImageFacts,
};
use crate::{
    control::cluster::core::DecisionSubject,
    eff::{EffAtom, EffIndex},
    global::ControlDesc,
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
        let mut policy_len = 0usize;
        let mut control_desc_len = 0usize;
        let mut idx = 0usize;
        while idx < view.len() {
            if view.atom_at(idx).is_some() {
                atom_len += 1;
            }
            if view.resident_policy_at(idx).is_some() {
                policy_len += 1;
            }
            if view.resident_control_desc_at(idx).is_some() {
                control_desc_len += 1;
            }
            idx += 1;
        }
        let markers = view.scope_markers();
        let mut route_control_len = 0usize;
        idx = 0;
        while idx < markers.len() {
            let marker = markers[idx];
            if matches!(marker.event, ScopeEvent::Enter)
                && matches!(marker.scope_kind, ScopeKind::Route)
            {
                route_control_len += 1;
            }
            idx += 1;
        }

        let mut offset = 0usize;
        let atoms = ProgramColumnRange::new(offset, atom_len, PROGRAM_IMAGE_ATOM_STRIDE);
        offset = atoms.end_offset(PROGRAM_IMAGE_ATOM_STRIDE);
        let policies = ProgramColumnRange::new(offset, policy_len, PROGRAM_IMAGE_POLICY_STRIDE);
        offset = policies.end_offset(PROGRAM_IMAGE_POLICY_STRIDE);
        let control_descs =
            ProgramColumnRange::new(offset, control_desc_len, PROGRAM_IMAGE_CONTROL_DESC_STRIDE);
        offset = control_descs.end_offset(PROGRAM_IMAGE_CONTROL_DESC_STRIDE);
        let route_controls = ProgramColumnRange::new(
            offset,
            route_control_len,
            PROGRAM_IMAGE_ROUTE_CONTROL_STRIDE,
        );
        let columns = ProgramImageColumns {
            atoms,
            policies,
            control_descs,
            route_controls,
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
    const fn encode_subject(subject: DecisionSubject) -> u8 {
        match subject {
            DecisionSubject::RouteArm => PROGRAM_IMAGE_SUBJECT_ROUTE_ARM,
            DecisionSubject::LoopContinue => PROGRAM_IMAGE_SUBJECT_LOOP_CONTINUE,
            DecisionSubject::LoopBreak => PROGRAM_IMAGE_SUBJECT_LOOP_BREAK,
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
        self.write_u8(out + 5, atom.is_control as u8);
        self.write_u8(out + 6, Self::encode_resource(atom.resource));
        self.write_u8(out + 7, atom.lane);
    }

    #[inline(always)]
    const fn write_policy(
        &mut self,
        column: ProgramColumnRange,
        row: usize,
        offset: usize,
        policy: ResolverMode,
    ) {
        if offset > u16::MAX as usize {
            panic!("program image");
        }
        let policy_id = match policy.dynamic_policy_id() {
            Some(policy_id) => policy_id,
            None => ControlDesc::STATIC_POLICY_SITE,
        };
        let out = Self::column_offset(column, row, PROGRAM_IMAGE_POLICY_STRIDE);
        self.write_u16(out, offset as u16);
        self.write_u16(out + 2, policy_id);
        self.write_u32(out + 4, CompactScopeId::from_scope_id(policy.scope()).raw());
    }

    #[inline(always)]
    const fn write_control_desc(
        &mut self,
        column: ProgramColumnRange,
        row: usize,
        offset: usize,
        desc: ControlDesc,
    ) {
        if offset > u16::MAX as usize {
            panic!("program image");
        }
        let out = Self::column_offset(column, row, PROGRAM_IMAGE_CONTROL_DESC_STRIDE);
        self.write_u16(out, offset as u16);
        self.write_u16(out + 2, desc.policy_site());
        self.write_u16(out + 4, desc.tap_id());
        self.write_u8(out + 6, desc.resource_tag());
        self.write_u8(out + 7, desc.op().as_u8());
        self.write_u8(out + 8, desc.scope_kind() as u8);
        self.write_u8(out + 9, desc.path().as_u8());
        self.write_u8(out + 10, desc.shot().as_u8());
        self.write_u8(out + 11, 0);
    }

    #[inline(always)]
    const fn write_route_control(
        &mut self,
        column: ProgramColumnRange,
        row: usize,
        scope: ScopeId,
        controller_role: Option<u8>,
        decision: Option<(ResolverMode, EffIndex, u8, DecisionSubject)>,
    ) {
        let out = Self::column_offset(column, row, PROGRAM_IMAGE_ROUTE_CONTROL_STRIDE);
        self.write_u32(out, CompactScopeId::from_scope_id(scope.canonical()).raw());
        let (policy_id, eff_dense, decision_tag, subject) = match decision {
            Some((policy, eff, tag, subject)) => {
                let policy_id = match policy.dynamic_policy_id() {
                    Some(policy_id) => policy_id,
                    None => ControlDesc::STATIC_POLICY_SITE,
                };
                let dense = eff.dense_ordinal();
                if dense > u16::MAX as usize {
                    panic!("program image");
                }
                (policy_id, dense as u16, tag, Self::encode_subject(subject))
            }
            None => (
                ControlDesc::STATIC_POLICY_SITE,
                u16::MAX,
                0,
                PROGRAM_IMAGE_SUBJECT_NONE,
            ),
        };
        self.write_u16(out + 4, policy_id);
        self.write_u16(out + 6, eff_dense);
        self.write_u8(
            out + 8,
            match controller_role {
                Some(role) => role,
                None => PROGRAM_IMAGE_NO_ROUTE_CONTROLLER,
            },
        );
        self.write_u8(out + 9, decision_tag);
        self.write_u8(out + 10, subject);
        self.write_u8(out + 11, 0);
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
    const fn route_control_decision(
        view: &CompiledProgramView<'_>,
        route_scope: ScopeId,
        route_enter_marker_idx: usize,
    ) -> Option<(ResolverMode, EffIndex, u8, DecisionSubject)> {
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
        let policy = match view.resident_policy_at(scope_start) {
            Some(policy) => policy,
            None => return None,
        };
        if policy.dynamic_policy_id().is_none() {
            return None;
        }
        let control = view.resident_control_desc_at(scope_start);
        if let Some(control) = control
            && !control.supports_dynamic_resolver()
        {
            return None;
        }
        let tag = match control {
            Some(control) => control.resource_tag(),
            None => 0,
        };
        Some((
            policy,
            EffIndex::from_dense_ordinal(scope_start),
            tag,
            DecisionSubject::RouteArm,
        ))
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
        let mut policy_row = 0usize;
        let mut control_desc_row = 0usize;
        let mut idx = 0usize;
        while idx < view.len() {
            if let Some(atom) = view.atom_at(idx) {
                out.write_atom(columns.atoms, atom_row, idx, atom);
                atom_row += 1;
            }
            if let Some(policy) = view.resident_policy_at(idx) {
                out.write_policy(columns.policies, policy_row, idx, policy);
                policy_row += 1;
            }
            if let Some(desc) = view.resident_control_desc_at(idx) {
                out.write_control_desc(columns.control_descs, control_desc_row, idx, desc);
                control_desc_row += 1;
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
                let decision = Self::route_control_decision(&view, marker.scope_id, idx);
                out.write_route_control(
                    columns.route_controls,
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
