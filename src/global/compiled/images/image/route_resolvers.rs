use super::CompiledProgramRef;
use super::columns::{PROGRAM_IMAGE_INTRINSIC_ROUTE_ROLE, PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE};
use crate::global::const_dsl::{INTRINSIC_ROUTE_RESOLVER_ID, RouteResolver, ScopeId};

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
        let role = self.byte_at(offset + 4);
        if role == PROGRAM_IMAGE_INTRINSIC_ROUTE_ROLE {
            None
        } else {
            Some(role)
        }
    }

    #[inline(always)]
    pub(crate) fn route_controller(&self, scope_id: ScopeId) -> Option<(RouteResolver, u8)> {
        let offset = self.route_resolver_row(scope_id)?;
        let resolver_id = self.read_u16_at(offset + 2);
        if resolver_id == INTRINSIC_ROUTE_RESOLVER_ID {
            return None;
        }
        let scope = ScopeId::from_raw(self.read_u16_at(offset));
        let resolver = RouteResolver::Dynamic { resolver_id, scope };
        Some((resolver, self.byte_at(offset + 5)))
    }

    #[inline(always)]
    pub(crate) const fn route_resolver_row_count(&self) -> usize {
        self.columns.route_resolvers.len as usize
    }

    #[inline(always)]
    pub(crate) fn route_resolver_scope_at_row(&self, row: usize) -> Option<ScopeId> {
        let offset = self.column_offset(
            self.columns.route_resolvers,
            row,
            PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE,
        )?;
        Some(ScopeId::from_raw(self.read_u16_at(offset)))
    }

    #[inline(always)]
    pub(crate) fn route_resolver_id_at_row(&self, row: usize) -> Option<u16> {
        let offset = self.column_offset(
            self.columns.route_resolvers,
            row,
            PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE,
        )?;
        let resolver_id = self.read_u16_at(offset + 2);
        (resolver_id != INTRINSIC_ROUTE_RESOLVER_ID).then_some(resolver_id)
    }
}
