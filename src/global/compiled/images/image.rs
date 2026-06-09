use crate::{
    control::cap::mint::{CapShot, ControlOp, ControlPath},
    control::cluster::core::DecisionSubject,
    control::cluster::effects::EffectEnvelopeRef,
    eff::{EffAtom, EffIndex, EffStruct},
    endpoint::kernel::EndpointArenaLayout,
    global::ControlDesc,
    global::const_dsl::{ResolverMode, ScopeId},
};
#[cfg(all(test, hibana_repo_tests))]
use crate::{
    eff::EffKind,
    global::typestate::{LocalAtomFacts, LocalNode, LocalNodeMeta, StateIndex},
};

#[cfg(all(test, hibana_repo_tests))]
use super::program::ControlSemanticKind;
use super::{
    program::{ControlSemanticsTable, DynamicPolicySite},
    role::CompiledRoleImage,
};
use crate::global::compiled::lowering::{CompiledProgramImage, CompiledProgramView, ProgramStamp};
use crate::global::const_dsl::{CompactScopeId, ControlScopeKind, ScopeEvent, ScopeKind};

mod role_descriptor_ref;
pub(crate) use self::role_descriptor_ref::RoleDescriptorRef;
#[cfg(all(test, hibana_repo_tests))]
#[inline(always)]
fn same_scope(left: ScopeId, right: ScopeId) -> bool {
    !left.is_none() && left.canonical_raw() == right.canonical_raw()
}

const PROGRAM_IMAGE_ATOM_STRIDE: usize = 8;
const PROGRAM_IMAGE_POLICY_STRIDE: usize = 8;
const PROGRAM_IMAGE_CONTROL_DESC_STRIDE: usize = 12;
const PROGRAM_IMAGE_ROUTE_CONTROL_STRIDE: usize = 12;
const PROGRAM_IMAGE_NO_ROUTE_CONTROLLER: u8 = u8::MAX;
const PROGRAM_IMAGE_SUBJECT_NONE: u8 = u8::MAX;
const PROGRAM_IMAGE_SUBJECT_ROUTE_ARM: u8 = 0;
const PROGRAM_IMAGE_SUBJECT_LOOP_CONTINUE: u8 = 1;
const PROGRAM_IMAGE_SUBJECT_LOOP_BREAK: u8 = 2;

#[derive(Clone, Copy, Debug)]
pub(crate) struct ProgramImageColumn {
    offset: u16,
    len: u16,
    stride: u8,
}

impl ProgramImageColumn {
    const EMPTY: Self = Self {
        offset: 0,
        len: 0,
        stride: 1,
    };

    #[inline(always)]
    const fn new(offset: usize, len: usize, stride: usize) -> Self {
        if offset > u16::MAX as usize || len > u16::MAX as usize || stride > u8::MAX as usize {
            panic!("program image");
        }
        if stride == 0 {
            panic!("program image");
        }
        if offset.saturating_add(len.saturating_mul(stride)) > u16::MAX as usize {
            panic!("program image");
        }
        Self {
            offset: offset as u16,
            len: len as u16,
            stride: stride as u8,
        }
    }

    #[inline(always)]
    const fn byte_len(self) -> usize {
        self.len as usize * self.stride as usize
    }

    #[inline(always)]
    const fn end_offset(self) -> usize {
        self.offset as usize + self.byte_len()
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ProgramImageColumns {
    atoms: ProgramImageColumn,
    policies: ProgramImageColumn,
    control_descs: ProgramImageColumn,
    route_controls: ProgramImageColumn,
}

impl ProgramImageColumns {
    #[inline(always)]
    const fn empty() -> Self {
        Self {
            atoms: ProgramImageColumn::EMPTY,
            policies: ProgramImageColumn::EMPTY,
            control_descs: ProgramImageColumn::EMPTY,
            route_controls: ProgramImageColumn::EMPTY,
        }
    }

    #[inline(always)]
    const fn blob_len(self) -> usize {
        let mut len = self.atoms.end_offset();
        if self.policies.end_offset() > len {
            len = self.policies.end_offset();
        }
        if self.control_descs.end_offset() > len {
            len = self.control_descs.end_offset();
        }
        if self.route_controls.end_offset() > len {
            len = self.route_controls.end_offset();
        }
        len
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ProgramImageFacts {
    role_count: u8,
    control_scope_mask: u8,
}

impl ProgramImageFacts {
    #[inline(always)]
    const fn from_image(image: &CompiledProgramImage) -> Self {
        let role_count = image.compiled_program_role_count();
        if role_count > u8::MAX as usize {
            panic!("program image");
        }
        Self {
            role_count: role_count as u8,
            control_scope_mask: image.compiled_program_control_scope_mask(),
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct ProgramImageBlobStorage<const N: usize> {
    pub(crate) stamp: ProgramStamp,
    pub(crate) facts: ProgramImageFacts,
    pub(crate) columns: ProgramImageColumns,
    bytes: [u8; N],
    len: u16,
}

impl<const N: usize> ProgramImageBlobStorage<N> {
    #[inline(always)]
    const fn empty(image: &CompiledProgramImage) -> Self {
        Self {
            stamp: image.stamp(),
            facts: ProgramImageFacts::from_image(image),
            columns: ProgramImageColumns::empty(),
            bytes: [0; N],
            len: 0,
        }
    }

    #[inline(always)]
    pub(crate) const fn projected_len(image: &CompiledProgramImage) -> usize {
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
        (atom_len * PROGRAM_IMAGE_ATOM_STRIDE)
            + (policy_len * PROGRAM_IMAGE_POLICY_STRIDE)
            + (control_desc_len * PROGRAM_IMAGE_CONTROL_DESC_STRIDE)
            + (route_control_len * PROGRAM_IMAGE_ROUTE_CONTROL_STRIDE)
    }

    #[inline(always)]
    pub(crate) const fn blob(&'static self) -> &'static [u8] {
        if self.len as usize > self.bytes.len() {
            panic!("program image");
        }
        // SAFETY: len is checked against this static backing array and the returned slice borrows it.
        unsafe { core::slice::from_raw_parts(self.bytes.as_ptr(), self.len as usize) }
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
    const fn row_offset(column: ProgramImageColumn, row: usize) -> usize {
        if row >= column.len as usize {
            panic!("program image");
        }
        column.offset as usize + row * column.stride as usize
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
        column: ProgramImageColumn,
        row: usize,
        offset: usize,
        atom: EffAtom,
    ) {
        if offset > u16::MAX as usize {
            panic!("program image");
        }
        let out = Self::row_offset(column, row);
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
        column: ProgramImageColumn,
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
        let out = Self::row_offset(column, row);
        self.write_u16(out, offset as u16);
        self.write_u16(out + 2, policy_id);
        self.write_u32(out + 4, CompactScopeId::from_scope_id(policy.scope()).raw());
    }

    #[inline(always)]
    const fn write_control_desc(
        &mut self,
        column: ProgramImageColumn,
        row: usize,
        offset: usize,
        desc: ControlDesc,
    ) {
        if offset > u16::MAX as usize {
            panic!("program image");
        }
        let out = Self::row_offset(column, row);
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
        column: ProgramImageColumn,
        row: usize,
        scope: ScopeId,
        controller_role: Option<u8>,
        decision: Option<(ResolverMode, EffIndex, u8, DecisionSubject)>,
    ) {
        let out = Self::row_offset(column, row);
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
            return Self::empty(image);
        }
        Self::from_image(image)
    }

    #[inline(always)]
    pub(crate) const fn from_image(image: &CompiledProgramImage) -> Self {
        let projected_len = Self::projected_len(image);
        if projected_len > N {
            panic!("program image");
        }
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

        let mut out = Self::empty(image);
        let mut offset = 0usize;
        out.columns.atoms = ProgramImageColumn::new(offset, atom_len, PROGRAM_IMAGE_ATOM_STRIDE);
        offset = out.columns.atoms.end_offset();
        out.columns.policies =
            ProgramImageColumn::new(offset, policy_len, PROGRAM_IMAGE_POLICY_STRIDE);
        offset = out.columns.policies.end_offset();
        out.columns.control_descs =
            ProgramImageColumn::new(offset, control_desc_len, PROGRAM_IMAGE_CONTROL_DESC_STRIDE);
        offset = out.columns.control_descs.end_offset();
        out.columns.route_controls = ProgramImageColumn::new(
            offset,
            route_control_len,
            PROGRAM_IMAGE_ROUTE_CONTROL_STRIDE,
        );
        offset = out.columns.route_controls.end_offset();
        if offset != projected_len || offset != out.columns.blob_len() {
            panic!("program image");
        }
        if offset > u16::MAX as usize {
            panic!("program image");
        }

        let mut atom_row = 0usize;
        let mut policy_row = 0usize;
        let mut control_desc_row = 0usize;
        idx = 0;
        while idx < view.len() {
            if let Some(atom) = view.atom_at(idx) {
                out.write_atom(out.columns.atoms, atom_row, idx, atom);
                atom_row += 1;
            }
            if let Some(policy) = view.resident_policy_at(idx) {
                out.write_policy(out.columns.policies, policy_row, idx, policy);
                policy_row += 1;
            }
            if let Some(desc) = view.resident_control_desc_at(idx) {
                out.write_control_desc(out.columns.control_descs, control_desc_row, idx, desc);
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
                    out.columns.route_controls,
                    route_row,
                    marker.scope_id,
                    controller,
                    decision,
                );
                route_row += 1;
            }
            idx += 1;
        }
        out.len = offset as u16;
        out
    }
}

/// Sealed runtime owner for immutable program-wide compiled facts.
#[derive(Clone, Copy)]
pub(crate) struct CompiledProgramRef {
    stamp: ProgramStamp,
    facts: ProgramImageFacts,
    columns: ProgramImageColumns,
    blob: &'static [u8],
}

impl core::fmt::Debug for CompiledProgramRef {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("CompiledProgramRef")
            .field("stamp", &self.stamp.words())
            .field("blob", &(self.blob.as_ptr(), self.blob.len()))
            .finish()
    }
}

impl PartialEq for CompiledProgramRef {
    #[inline(always)]
    fn eq(&self, other: &Self) -> bool {
        self.stamp.words() == other.stamp.words()
            && self.facts == other.facts
            && core::ptr::eq(self.blob.as_ptr(), other.blob.as_ptr())
            && self.blob.len() == other.blob.len()
    }
}

impl Eq for CompiledProgramRef {}

impl CompiledProgramRef {
    #[inline(always)]
    pub(crate) const fn compact(
        stamp: ProgramStamp,
        facts: ProgramImageFacts,
        columns: ProgramImageColumns,
        blob: &'static [u8],
    ) -> Self {
        if blob.len() != columns.blob_len() {
            panic!("program image");
        }
        Self {
            stamp,
            facts,
            columns,
            blob,
        }
    }

    #[inline(always)]
    const fn column_offset(&self, column: ProgramImageColumn, row: usize) -> Option<usize> {
        if row >= column.len as usize {
            return None;
        }
        let offset = column.offset as usize + row * column.stride as usize;
        if offset + column.stride as usize > self.blob.len() {
            panic!("program image");
        }
        Some(offset)
    }

    #[inline(always)]
    const fn byte_at(&self, offset: usize) -> u8 {
        if offset >= self.blob.len() {
            panic!("program image");
        }
        self.blob[offset]
    }

    #[inline(always)]
    const fn read_u16_at(&self, offset: usize) -> u16 {
        self.byte_at(offset) as u16 | ((self.byte_at(offset + 1) as u16) << 8)
    }

    #[inline(always)]
    const fn read_u32_at(&self, offset: usize) -> u32 {
        self.read_u16_at(offset) as u32 | ((self.read_u16_at(offset + 2) as u32) << 16)
    }

    #[inline(always)]
    const fn decode_resource(raw: u8) -> Option<u8> {
        if raw == u8::MAX { None } else { Some(raw) }
    }

    #[inline(always)]
    const fn decode_subject(raw: u8) -> Option<DecisionSubject> {
        match raw {
            PROGRAM_IMAGE_SUBJECT_ROUTE_ARM => Some(DecisionSubject::RouteArm),
            PROGRAM_IMAGE_SUBJECT_LOOP_CONTINUE => Some(DecisionSubject::LoopContinue),
            PROGRAM_IMAGE_SUBJECT_LOOP_BREAK => Some(DecisionSubject::LoopBreak),
            PROGRAM_IMAGE_SUBJECT_NONE => None,
            _ => panic!("program image"),
        }
    }

    #[inline(always)]
    const fn control_op_from_u8(raw: u8) -> ControlOp {
        match ControlOp::from_u8(raw) {
            Some(op) => op,
            None => panic!("program image"),
        }
    }

    #[inline(always)]
    const fn control_scope_kind_from_u8(raw: u8) -> ControlScopeKind {
        match ControlScopeKind::from_u8(raw) {
            Some(kind) => kind,
            None => panic!("program image"),
        }
    }

    #[inline(always)]
    const fn control_path_from_u8(raw: u8) -> ControlPath {
        match ControlPath::from_u8(raw) {
            Some(path) => path,
            None => panic!("program image"),
        }
    }

    #[inline(always)]
    const fn cap_shot_from_u8(raw: u8) -> CapShot {
        match CapShot::from_u8(raw) {
            Some(shot) => shot,
            None => panic!("program image"),
        }
    }

    #[inline(always)]
    const fn compact_scope_from_bits(raw: u32) -> CompactScopeId {
        match CompactScopeId::decode_raw(raw) {
            Some(scope) => scope,
            None => panic!("program image"),
        }
    }

    #[inline(always)]
    pub(crate) const fn atom_at(&self, eff_idx: usize) -> Option<EffAtom> {
        let mut row = 0usize;
        while row < self.columns.atoms.len as usize {
            let offset = match self.column_offset(self.columns.atoms, row) {
                Some(offset) => offset,
                None => return None,
            };
            if self.read_u16_at(offset) as usize == eff_idx {
                return Some(EffAtom {
                    from: self.byte_at(offset + 2),
                    to: self.byte_at(offset + 3),
                    label: self.byte_at(offset + 4),
                    is_control: self.byte_at(offset + 5) != 0,
                    resource: Self::decode_resource(self.byte_at(offset + 6)),
                    lane: self.byte_at(offset + 7),
                });
            }
            row += 1;
        }
        None
    }

    #[inline(always)]
    pub(crate) const fn node_at(&self, eff_idx: usize) -> EffStruct {
        match self.atom_at(eff_idx) {
            Some(atom) => EffStruct::atom(atom),
            None => EffStruct::pure(),
        }
    }

    #[inline(always)]
    pub(crate) const fn atom_row_count(&self) -> usize {
        self.columns.atoms.len as usize
    }

    #[inline(always)]
    pub(crate) const fn atom_eff_at_row(&self, row: usize) -> Option<usize> {
        let offset = match self.column_offset(self.columns.atoms, row) {
            Some(offset) => offset,
            None => return None,
        };
        Some(self.read_u16_at(offset) as usize)
    }

    #[inline(always)]
    pub(crate) const fn resident_policy_at(&self, eff_idx: usize) -> Option<ResolverMode> {
        let mut row = 0usize;
        while row < self.columns.policies.len as usize {
            let offset = match self.column_offset(self.columns.policies, row) {
                Some(offset) => offset,
                None => return None,
            };
            if self.read_u16_at(offset) as usize == eff_idx {
                let policy_id = self.read_u16_at(offset + 2);
                if policy_id == ControlDesc::STATIC_POLICY_SITE {
                    return Some(ResolverMode::Static);
                }
                return Some(ResolverMode::Dynamic {
                    policy_id,
                    scope: Self::compact_scope_from_bits(self.read_u32_at(offset + 4)),
                });
            }
            row += 1;
        }
        None
    }

    #[inline(always)]
    pub(crate) const fn resident_control_desc_at(&self, eff_idx: usize) -> Option<ControlDesc> {
        let mut row = 0usize;
        while row < self.columns.control_descs.len as usize {
            let offset = match self.column_offset(self.columns.control_descs, row) {
                Some(offset) => offset,
                None => return None,
            };
            if self.read_u16_at(offset) as usize == eff_idx {
                return Some(ControlDesc::new(
                    EffIndex::from_dense_ordinal(eff_idx),
                    self.read_u16_at(offset + 2),
                    self.read_u16_at(offset + 4),
                    self.byte_at(offset + 6),
                    Self::control_op_from_u8(self.byte_at(offset + 7)),
                    Self::control_scope_kind_from_u8(self.byte_at(offset + 8)),
                    Self::control_path_from_u8(self.byte_at(offset + 9)),
                    Self::cap_shot_from_u8(self.byte_at(offset + 10)),
                ));
            }
            row += 1;
        }
        None
    }

    #[inline(always)]
    pub(crate) fn effect_envelope(&self) -> EffectEnvelopeRef<'_> {
        EffectEnvelopeRef::from_program_ref(*self)
    }

    #[inline(always)]
    pub(crate) const fn role_count(&self) -> usize {
        self.facts.role_count as usize
    }

    #[inline(always)]
    pub(crate) const fn control_scope_mask(&self) -> u8 {
        self.facts.control_scope_mask
    }

    #[cfg(all(test, hibana_repo_tests))]
    pub(crate) const fn compact_blob_len(&self) -> usize {
        self.blob.len()
    }

    #[cfg(all(test, hibana_repo_tests))]
    pub(crate) const fn largest_section_bytes(&self) -> usize {
        let mut largest = self.columns.atoms.byte_len();
        if self.columns.policies.byte_len() > largest {
            largest = self.columns.policies.byte_len();
        }
        if self.columns.control_descs.byte_len() > largest {
            largest = self.columns.control_descs.byte_len();
        }
        if self.columns.route_controls.byte_len() > largest {
            largest = self.columns.route_controls.byte_len();
        }
        largest
    }

    #[inline(always)]
    pub(crate) fn dynamic_policy_sites_for(
        &self,
        policy_id: u16,
    ) -> impl Iterator<Item = DynamicPolicySite> + '_ {
        crate::control::cluster::effects::ProgramImageDynamicPolicySiteIter::new(*self)
            .filter(move |site| site.policy_id() == policy_id)
    }

    #[inline(always)]
    pub(crate) fn control_semantics(&self) -> &'static ControlSemanticsTable {
        &super::program::CONTROL_SEMANTICS_TABLE
    }

    pub(crate) fn validate_label_universe(
        &self,
        max: u8,
    ) -> Result<(), crate::global::role_program::LabelUniverseViolation> {
        if max == u8::MAX {
            return Ok(());
        }
        let mut row = 0usize;
        while row < self.columns.atoms.len as usize {
            let offset = self
                .column_offset(self.columns.atoms, row)
                .expect("program image");
            let actual = self.byte_at(offset + 4);
            if actual > max {
                return Err(crate::global::role_program::LabelUniverseViolation { max, actual });
            }
            row += 1;
        }
        Ok(())
    }

    #[inline(always)]
    fn route_control_row(&self, scope_id: ScopeId) -> Option<usize> {
        if scope_id.is_none() {
            return None;
        }
        let target = scope_id.canonical_raw();
        let mut row = 0usize;
        while row < self.columns.route_controls.len as usize {
            let offset = self.column_offset(self.columns.route_controls, row)?;
            let scope = Self::compact_scope_from_bits(self.read_u32_at(offset)).to_scope_id();
            if scope.canonical_raw() == target {
                return Some(offset);
            }
            row += 1;
        }
        None
    }

    #[inline(always)]
    pub(crate) fn route_controller_role(&self, scope_id: ScopeId) -> Option<u8> {
        let offset = self.route_control_row(scope_id)?;
        let role = self.byte_at(offset + 8);
        if role == PROGRAM_IMAGE_NO_ROUTE_CONTROLLER {
            None
        } else {
            Some(role)
        }
    }

    #[inline(always)]
    pub(crate) fn route_controller(
        &self,
        scope_id: ScopeId,
    ) -> Option<(ResolverMode, crate::eff::EffIndex, u8, DecisionSubject)> {
        let offset = self.route_control_row(scope_id)?;
        let eff_dense = self.read_u16_at(offset + 6);
        if eff_dense == u16::MAX {
            return None;
        }
        let subject = Self::decode_subject(self.byte_at(offset + 10))?;
        let policy_id = self.read_u16_at(offset + 4);
        let scope = Self::compact_scope_from_bits(self.read_u32_at(offset));
        let policy = if policy_id == ControlDesc::STATIC_POLICY_SITE {
            ResolverMode::Static
        } else {
            ResolverMode::Dynamic { policy_id, scope }
        };
        Some((
            policy,
            EffIndex::from_dense_ordinal(eff_dense as usize),
            self.byte_at(offset + 9),
            subject,
        ))
    }
}

/// Sealed runtime owner for role-local immutable compiled facts within a compiled program ref.
#[derive(Clone, Copy)]
pub(crate) struct RoleImageSlice<const ROLE: u8> {
    descriptor: RoleDescriptorRef,
}

impl<const ROLE: u8> RoleImageSlice<ROLE> {
    #[inline(always)]
    pub(crate) const fn from_resident(compiled: &'static CompiledRoleImage) -> Self {
        Self {
            descriptor: RoleDescriptorRef::from_resident(compiled),
        }
    }

    #[inline(always)]
    pub(crate) const fn descriptor(&self) -> RoleDescriptorRef {
        self.descriptor
    }

    #[inline(always)]
    pub(crate) const fn program(&self) -> CompiledProgramRef {
        self.descriptor.program()
    }

    #[inline(always)]
    pub(crate) fn has_active_lane(&self, lane_idx: usize) -> bool {
        self.descriptor.has_active_lane(lane_idx)
    }

    #[inline(always)]
    pub(crate) fn first_active_lane(&self) -> Option<usize> {
        self.descriptor.first_active_lane()
    }

    #[inline(always)]
    pub(crate) fn endpoint_lane_slot_count(&self) -> usize {
        self.descriptor.endpoint_lane_slot_count()
    }

    #[inline(always)]
    pub(crate) fn logical_lane_count(&self) -> usize {
        self.descriptor.logical_lane_count()
    }

    #[inline(always)]
    pub(crate) fn route_table_frame_slots(&self) -> usize {
        self.descriptor.route_table_frame_slots()
    }

    #[inline(always)]
    pub(crate) fn route_table_lane_slots(&self) -> usize {
        self.descriptor.route_table_lane_slots()
    }

    #[inline(always)]
    pub(crate) fn loop_table_slots(&self) -> usize {
        self.descriptor.loop_table_slots()
    }

    #[inline(always)]
    pub(crate) fn resident_cap_entries(&self) -> usize {
        self.descriptor.resident_cap_entries()
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) fn active_lane_count(&self) -> usize {
        self.descriptor.active_lane_count()
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) fn max_route_stack_depth(&self) -> usize {
        self.descriptor.max_route_stack_depth()
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) fn max_loop_stack_depth(&self) -> usize {
        self.descriptor.max_loop_stack_depth()
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) fn route_scope_count(&self) -> usize {
        self.descriptor.route_scope_count()
    }

    #[inline(always)]
    pub(crate) fn endpoint_arena_layout(&self) -> EndpointArenaLayout {
        self.descriptor.endpoint_arena_layout()
    }
}
