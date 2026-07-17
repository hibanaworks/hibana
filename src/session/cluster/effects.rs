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
            let Some((_, resolver)) = self.program.route_resolver_authority_at_row(row) else {
                crate::invariant();
            };
            let Some(resolver) = resolver else {
                continue;
            };
            return Some(resolver);
        }
        None
    }
}
