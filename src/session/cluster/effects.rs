//! Projected route-effect metadata helpers.

use crate::{
    eff::EffIndex,
    global::compiled::images::{CompiledProgramRef, DynamicResolverSite},
};

#[inline(always)]
pub(crate) const fn lane_open_tap_event_id() -> u16 {
    0x0100
}

pub(crate) struct ProgramImageDynamicResolverSiteIter<'a> {
    program: &'a CompiledProgramRef,
    row: usize,
}

impl<'a> ProgramImageDynamicResolverSiteIter<'a> {
    #[inline(always)]
    pub(crate) const fn new(program: &'a CompiledProgramRef) -> Self {
        Self { program, row: 0 }
    }
}

impl Iterator for ProgramImageDynamicResolverSiteIter<'_> {
    type Item = DynamicResolverSite;

    fn next(&mut self) -> Option<Self::Item> {
        while self.row < self.program.atom_row_count() {
            let row = self.row;
            self.row += 1;
            let offset = self.program.atom_eff_at_row(row)?;
            let Some(resolver) = self.program.resident_resolver_at(offset) else {
                continue;
            };
            let crate::global::const_dsl::RouteResolver::Dynamic { resolver_id, scope } = resolver
            else {
                continue;
            };
            return Some(DynamicResolverSite::new(
                EffIndex::from_dense_ordinal(offset),
                resolver_id,
                scope,
            ));
        }
        None
    }
}
