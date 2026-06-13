use super::CompiledProgramRef;
use super::columns::{PROGRAM_IMAGE_NO_ROUTE_CONTROLLER, PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE};
use crate::{
    eff::EffIndex,
    global::const_dsl::{ResolverMode, ScopeId},
};

impl CompiledProgramRef {
    #[inline(always)]
    fn route_resolver_row(&self, scope_id: ScopeId) -> Option<usize> {
        if scope_id.is_none() {
            return None;
        }
        let target = scope_id.canonical_raw();
        let mut row = 0usize;
        while row < self.columns.route_resolvers.len as usize {
            let offset = self.column_offset(
                self.columns.route_resolvers,
                row,
                PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE,
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
        let offset = self.route_resolver_row(scope_id)?;
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
    ) -> Option<(ResolverMode, crate::eff::EffIndex, u8)> {
        let offset = self.route_resolver_row(scope_id)?;
        let eff_dense = self.read_u16_at(offset + 6);
        if eff_dense == u16::MAX {
            return None;
        }
        let resolver_id = self.read_u16_at(offset + 4);
        let scope = Self::compact_scope_from_bits(self.read_u32_at(offset));
        let resolver = if resolver_id == u16::MAX {
            ResolverMode::Static
        } else {
            ResolverMode::Dynamic { resolver_id, scope }
        };
        Some((
            resolver,
            EffIndex::from_dense_ordinal(eff_dense as usize),
            self.byte_at(offset + 9),
        ))
    }
}
