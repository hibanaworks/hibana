use super::CompiledProgramRef;
use super::columns::PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE;
use crate::global::const_dsl::{
    DynamicRouteResolver, INTRINSIC_ROUTE_RESOLVER_ID, ScopeId, ScopeKind,
};

#[derive(Clone, Copy)]
struct RouteResolverRow {
    scope: ScopeId,
    resolver_id: u16,
    controller_role: u8,
    arm_participant_masks: [u16; 2],
}

impl RouteResolverRow {
    const fn decode(
        raw_scope: u16,
        resolver_id: u16,
        controller_role: u8,
        left_participant_mask: u16,
        right_participant_mask: u16,
        role_count: u8,
    ) -> Option<Self> {
        if role_count == 0 || role_count > crate::g::ROLE_DOMAIN_SIZE {
            return None;
        }
        let scope = match ScopeId::decode_raw(raw_scope) {
            Some(scope) => scope,
            None => return None,
        };
        if !matches!(scope.kind(), Some(ScopeKind::Route))
            || scope.local_ordinal() as usize >= crate::eff::meta::MAX_EFF_NODES
        {
            return None;
        }
        let role_mask = if role_count == u16::BITS as u8 {
            u16::MAX
        } else {
            (1u16 << role_count) - 1
        };
        if left_participant_mask == 0
            || right_participant_mask == 0
            || ((left_participant_mask | right_participant_mask) & !role_mask) != 0
            || controller_role >= role_count
            || (left_participant_mask & (1u16 << controller_role)) == 0
            || (right_participant_mask & (1u16 << controller_role)) == 0
        {
            return None;
        }
        Some(Self {
            scope,
            resolver_id,
            controller_role,
            arm_participant_masks: [left_participant_mask, right_participant_mask],
        })
    }

    const fn resolver(self) -> Option<DynamicRouteResolver> {
        if self.resolver_id == INTRINSIC_ROUTE_RESOLVER_ID {
            None
        } else {
            Some(DynamicRouteResolver::new(self.scope, self.resolver_id))
        }
    }

    const fn controller_role(self) -> u8 {
        self.controller_role
    }

    const fn participant_mask(self, arm: u8) -> u16 {
        match arm {
            0 => self.arm_participant_masks[0],
            1 => self.arm_participant_masks[1],
            _ => crate::invariant(),
        }
    }
}

impl CompiledProgramRef {
    #[inline]
    fn route_resolver_row_at(&self, row: usize) -> Option<RouteResolverRow> {
        let offset = self.column_offset(
            self.columns.route_resolvers(),
            row,
            PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE,
        )?;
        match RouteResolverRow::decode(
            self.read_u16_at(offset),
            self.read_u16_at(offset + 2),
            self.byte_at(offset + 4),
            self.read_u16_at(offset + 5),
            self.read_u16_at(offset + 7),
            self.facts.role_count,
        ) {
            Some(row) => Some(row),
            None => crate::invariant(),
        }
    }

    #[inline]
    fn route_resolver_row(&self, scope_id: ScopeId) -> RouteResolverRow {
        if !matches!(scope_id.kind(), Some(ScopeKind::Route))
            || scope_id.local_ordinal() as usize >= crate::eff::meta::MAX_EFF_NODES
        {
            crate::invariant();
        }
        let mut row = 0usize;
        while row < self.columns.route_resolver_count() {
            let decoded = match self.route_resolver_row_at(row) {
                Some(decoded) => decoded,
                None => crate::invariant(),
            };
            if decoded.scope == scope_id {
                return decoded;
            }
            row += 1;
        }
        crate::invariant()
    }

    #[inline(always)]
    pub(crate) fn route_controller_role(&self, scope_id: ScopeId) -> u8 {
        self.route_resolver_row(scope_id).controller_role()
    }

    #[inline(always)]
    pub(crate) fn route_resolver(&self, scope_id: ScopeId) -> Option<DynamicRouteResolver> {
        self.route_resolver_row(scope_id).resolver()
    }

    #[inline(always)]
    pub(crate) fn route_participant_mask(&self, scope_id: ScopeId, arm: u8) -> u16 {
        self.route_resolver_row(scope_id).participant_mask(arm)
    }

    #[inline(always)]
    pub(crate) const fn route_resolver_row_count(&self) -> usize {
        self.columns.route_resolver_count()
    }

    #[inline(always)]
    pub(crate) fn route_resolver_scope_at_row(&self, row: usize) -> Option<ScopeId> {
        Some(self.route_resolver_row_at(row)?.scope)
    }

    #[inline(always)]
    pub(crate) fn route_resolver_id_at_row(&self, row: usize) -> Option<u16> {
        self.route_resolver_row_at(row)?
            .resolver()
            .map(DynamicRouteResolver::resolver_id)
    }
}

#[cfg(kani)]
mod kani;

#[cfg(all(test, hibana_repo_tests))]
mod tests;
