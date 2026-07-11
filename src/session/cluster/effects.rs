//! Projected route-effect metadata helpers.

use crate::global::{compiled::images::CompiledProgramRef, const_dsl::DynamicRouteResolver};

pub(crate) struct ProgramImageRouteResolverSiteIter<'a> {
    program: &'a CompiledProgramRef,
    row: usize,
}

impl<'a> ProgramImageRouteResolverSiteIter<'a> {
    #[inline(always)]
    pub(crate) const fn new(program: &'a CompiledProgramRef) -> Self {
        Self { program, row: 0 }
    }
}

impl Iterator for ProgramImageRouteResolverSiteIter<'_> {
    type Item = DynamicRouteResolver;

    fn next(&mut self) -> Option<Self::Item> {
        while self.row < self.program.route_resolver_row_count() {
            let row = self.row;
            self.row += 1;
            let scope = self.program.route_resolver_scope_at_row(row)?;
            let Some(resolver_id) = self.program.route_resolver_id_at_row(row) else {
                continue;
            };
            return Some(DynamicRouteResolver::new(scope, resolver_id));
        }
        None
    }
}
