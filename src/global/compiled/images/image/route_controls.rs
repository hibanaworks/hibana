use super::CompiledProgramRef;
use super::columns::{PROGRAM_IMAGE_NO_ROUTE_CONTROLLER, PROGRAM_IMAGE_ROUTE_CONTROL_STRIDE};
use crate::{
    control::cluster::core::DecisionSubject,
    eff::EffIndex,
    global::ControlDesc,
    global::const_dsl::{ResolverMode, ScopeId},
};

impl CompiledProgramRef {
    #[inline(always)]
    fn route_control_row(&self, scope_id: ScopeId) -> Option<usize> {
        if scope_id.is_none() {
            return None;
        }
        let target = scope_id.canonical_raw();
        let mut row = 0usize;
        while row < self.columns.route_controls.len as usize {
            let offset = self.column_offset(
                self.columns.route_controls,
                row,
                PROGRAM_IMAGE_ROUTE_CONTROL_STRIDE,
            )?;
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
