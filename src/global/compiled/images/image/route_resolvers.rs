use super::CompiledProgramRef;
use super::columns::{
    PROGRAM_IMAGE_INTRINSIC_ROUTE_EFF, PROGRAM_IMAGE_INTRINSIC_ROUTE_ROLE,
    PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE,
};
use crate::{
    eff::EffIndex,
    global::const_dsl::{INTRINSIC_ROUTE_RESOLVER_ID, RouteResolver, ScopeId},
};

impl CompiledProgramRef {
    #[inline(always)]
    fn route_resolver_row(&self, scope_id: ScopeId) -> Option<usize> {
        if scope_id.is_none() {
            return None;
        }
        let mut row = 0usize;
        while row < self.columns.route_resolvers.len as usize {
            let offset = self.column_offset(
                self.columns.route_resolvers,
                row,
                PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE,
            )?;
            if ScopeId::from_raw(self.read_u16_at(offset)) == scope_id {
                return Some(offset);
            }
            row += 1;
        }
        None
    }

    #[inline(always)]
    pub(crate) fn route_controller_role(&self, scope_id: ScopeId) -> Option<u8> {
        let offset = self.route_resolver_row(scope_id)?;
        let role = self.byte_at(offset + 6);
        if role == PROGRAM_IMAGE_INTRINSIC_ROUTE_ROLE {
            None
        } else {
            Some(role)
        }
    }

    #[inline(always)]
    pub(crate) fn route_controller(
        &self,
        scope_id: ScopeId,
    ) -> Option<(RouteResolver, crate::eff::EffIndex, u8)> {
        let offset = self.route_resolver_row(scope_id)?;
        let eff_dense = self.read_u16_at(offset + 4);
        if eff_dense == PROGRAM_IMAGE_INTRINSIC_ROUTE_EFF {
            return None;
        }
        let resolver_id = self.read_u16_at(offset + 2);
        let scope = ScopeId::from_raw(self.read_u16_at(offset));
        let resolver = if resolver_id == INTRINSIC_ROUTE_RESOLVER_ID {
            RouteResolver::Intrinsic
        } else {
            RouteResolver::Dynamic { resolver_id, scope }
        };
        Some((
            resolver,
            EffIndex::from_dense_ordinal(eff_dense as usize),
            self.byte_at(offset + 7),
        ))
    }
}
