use super::columns::{
    PROGRAM_IMAGE_ATOM_STRIDE, PROGRAM_IMAGE_CONTROL_DESC_STRIDE, PROGRAM_IMAGE_POLICY_STRIDE,
    PROGRAM_IMAGE_SUBJECT_LOOP_BREAK, PROGRAM_IMAGE_SUBJECT_LOOP_CONTINUE,
    PROGRAM_IMAGE_SUBJECT_NONE, PROGRAM_IMAGE_SUBJECT_ROUTE_ARM, ProgramColumnRange,
    ProgramImageColumns, ProgramImageFacts,
};
use crate::{
    control::cap::mint::{CapShot, ControlOp, ControlPath},
    control::cluster::core::DecisionSubject,
    control::cluster::effects::EffectEnvelopeRef,
    eff::{EffAtom, EffIndex, EffStruct},
    global::ControlDesc,
    global::compiled::images::program::DynamicPolicySite,
    global::const_dsl::{CompactScopeId, ControlScopeKind, ResolverMode},
    global::role_program::BlobPtr,
};

/// Sealed runtime owner for immutable program-wide compiled facts.
#[derive(Clone, Copy)]
pub(crate) struct CompiledProgramRef {
    pub(crate) facts: ProgramImageFacts,
    pub(crate) columns: ProgramImageColumns,
    pub(crate) blob: BlobPtr,
}

impl core::fmt::Debug for CompiledProgramRef {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("CompiledProgramRef")
            .field("blob", &(self.blob.as_ptr(), self.columns.blob_len()))
            .finish()
    }
}

impl CompiledProgramRef {
    #[inline(always)]
    pub(crate) const fn compact<const N: usize>(
        facts: ProgramImageFacts,
        columns: ProgramImageColumns,
        bytes: &'static [u8; N],
    ) -> Self {
        let blob = BlobPtr::from_array(bytes, columns.blob_len());
        Self {
            facts,
            columns,
            blob,
        }
    }

    #[inline(always)]
    pub(super) const fn column_offset(
        &self,
        column: ProgramColumnRange,
        row: usize,
        stride: usize,
    ) -> Option<usize> {
        if row >= column.len as usize {
            return None;
        }
        let offset = column.offset as usize + row * stride;
        if offset + stride > self.columns.blob_len() {
            panic!("program image");
        }
        Some(offset)
    }

    #[inline(always)]
    pub(super) const fn byte_at(&self, offset: usize) -> u8 {
        if offset >= self.columns.blob_len() {
            panic!("program image");
        }
        self.blob.byte_at(offset)
    }

    #[inline(always)]
    pub(super) const fn read_u16_at(&self, offset: usize) -> u16 {
        self.byte_at(offset) as u16 | ((self.byte_at(offset + 1) as u16) << 8)
    }

    #[inline(always)]
    pub(super) const fn read_u32_at(&self, offset: usize) -> u32 {
        self.read_u16_at(offset) as u32 | ((self.read_u16_at(offset + 2) as u32) << 16)
    }

    #[inline(always)]
    const fn decode_resource(raw: u8) -> Option<u8> {
        if raw == u8::MAX { None } else { Some(raw) }
    }

    #[inline(always)]
    pub(super) const fn decode_subject(raw: u8) -> Option<DecisionSubject> {
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
    pub(super) const fn compact_scope_from_bits(raw: u32) -> CompactScopeId {
        match CompactScopeId::decode_raw(raw) {
            Some(scope) => scope,
            None => panic!("program image"),
        }
    }

    #[inline(always)]
    pub(crate) const fn atom_at(&self, eff_idx: usize) -> Option<EffAtom> {
        let mut row = 0usize;
        while row < self.columns.atoms.len as usize {
            let offset =
                match self.column_offset(self.columns.atoms, row, PROGRAM_IMAGE_ATOM_STRIDE) {
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
        let offset = match self.column_offset(self.columns.atoms, row, PROGRAM_IMAGE_ATOM_STRIDE) {
            Some(offset) => offset,
            None => return None,
        };
        Some(self.read_u16_at(offset) as usize)
    }

    #[inline(always)]
    pub(crate) const fn resident_policy_at(&self, eff_idx: usize) -> Option<ResolverMode> {
        let mut row = 0usize;
        while row < self.columns.policies.len as usize {
            let offset =
                match self.column_offset(self.columns.policies, row, PROGRAM_IMAGE_POLICY_STRIDE) {
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
            let offset = match self.column_offset(
                self.columns.control_descs,
                row,
                PROGRAM_IMAGE_CONTROL_DESC_STRIDE,
            ) {
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
        EffectEnvelopeRef::from_program_ref(self)
    }

    #[inline(always)]
    pub(crate) const fn role_count(&self) -> usize {
        self.facts.role_count as usize
    }

    #[inline(always)]
    pub(crate) const fn control_scope_mask(&self) -> u8 {
        self.facts.control_scope_mask
    }

    #[inline(always)]
    pub(crate) fn dynamic_policy_sites_for(
        &self,
        policy_id: u16,
    ) -> impl Iterator<Item = DynamicPolicySite> + '_ {
        crate::control::cluster::effects::ProgramImageDynamicPolicySiteIter::new(self)
            .filter(move |site| site.policy_id() == policy_id)
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
                .column_offset(self.columns.atoms, row, PROGRAM_IMAGE_ATOM_STRIDE)
                .expect("program image");
            let actual = self.byte_at(offset + 4);
            if actual > max {
                return Err(crate::global::role_program::LabelUniverseViolation { max, actual });
            }
            row += 1;
        }
        Ok(())
    }
}
