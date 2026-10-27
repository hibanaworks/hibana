use super::CompiledProgramRef;
use super::columns::{PROGRAM_IMAGE_ROUTE_PARTICIPANT_STRIDE, PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE};
use crate::global::const_dsl::{DynamicRouteResolver, ScopeId, ScopeKind};

#[derive(Clone, Copy)]
pub(super) struct PackedRouteAuthority {
    packed_scope: u16,
    resolver_id: u16,
}

impl PackedRouteAuthority {
    pub(super) const fn encode(scope: ScopeId, resolver: Option<DynamicRouteResolver>) -> Self {
        if !matches!(scope.kind(), Some(ScopeKind::Route)) {
            crate::invariant();
        }
        match resolver {
            Some(resolver) => {
                if !resolver.scope().same(scope) {
                    crate::invariant();
                }
                Self {
                    packed_scope: scope.raw() | ScopeId::RESERVED_BIT,
                    resolver_id: resolver.resolver_id(),
                }
            }
            None => Self {
                packed_scope: scope.raw(),
                resolver_id: 0,
            },
        }
    }

    const fn decode(
        packed_scope: u16,
        resolver_id: u16,
    ) -> Option<(ScopeId, Option<DynamicRouteResolver>)> {
        let dynamic = (packed_scope & ScopeId::RESERVED_BIT) != 0;
        let scope = match ScopeId::decode_raw(packed_scope & !ScopeId::RESERVED_BIT) {
            Some(scope) => scope,
            None => return None,
        };
        if !matches!(scope.kind(), Some(ScopeKind::Route)) {
            return None;
        }
        let resolver = if dynamic {
            Some(DynamicRouteResolver::new(scope, resolver_id))
        } else if resolver_id == 0 {
            None
        } else {
            return None;
        };
        Some((scope, resolver))
    }

    pub(super) const fn packed_scope(self) -> u16 {
        self.packed_scope
    }

    pub(super) const fn resolver_id(self) -> u16 {
        self.resolver_id
    }
}

#[derive(Clone, Copy)]
struct RouteParticipantRange {
    start: u16,
    end: u16,
}

impl RouteParticipantRange {
    const fn new(start: u16, end: u16) -> Option<Self> {
        if start >= end {
            None
        } else {
            Some(Self { start, end })
        }
    }

    const fn len(self) -> usize {
        (self.end - self.start) as usize
    }
}

#[derive(Clone, Copy)]
struct RouteResolverRow {
    scope: ScopeId,
    resolver: Option<DynamicRouteResolver>,
    controller_role: u8,
    participants: [RouteParticipantRange; 2],
}

impl RouteResolverRow {
    const fn decode(
        packed_scope: u16,
        resolver_id: u16,
        controller_role: u8,
        participant_start: u16,
        left_len_minus_one: u8,
        participant_end: u16,
        participant_count: usize,
    ) -> Option<Self> {
        let (scope, resolver) = match PackedRouteAuthority::decode(packed_scope, resolver_id) {
            Some(authority) => authority,
            None => return None,
        };
        let participant_mid = match participant_start.checked_add(left_len_minus_one as u16 + 1) {
            Some(mid) => mid,
            None => return None,
        };
        if participant_end as usize > participant_count
            || participant_mid >= participant_end
            || participant_end - participant_mid > 256
        {
            return None;
        }
        let left = match RouteParticipantRange::new(participant_start, participant_mid) {
            Some(range) => range,
            None => return None,
        };
        let right = match RouteParticipantRange::new(participant_mid, participant_end) {
            Some(range) => range,
            None => return None,
        };
        Some(Self {
            scope,
            resolver,
            controller_role,
            participants: [left, right],
        })
    }

    const fn resolver(self) -> Option<DynamicRouteResolver> {
        self.resolver
    }

    const fn participant_range(self, arm: u8) -> RouteParticipantRange {
        match arm {
            0 => self.participants[0],
            1 => self.participants[1],
            _ => crate::invariant(),
        }
    }

    const fn participants_are_canonical(self, program: &CompiledProgramRef) -> bool {
        let mut arm = 0u8;
        while arm < 2 {
            let range = self.participant_range(arm);
            let mut controller_present = false;
            let mut previous = None;
            let mut idx = 0usize;
            while idx < range.len() {
                let role = program.participant_role_at(range.start as usize + idx);
                if role > program.facts.max_role
                    || match previous {
                        Some(previous) => previous >= role,
                        None => false,
                    }
                {
                    return false;
                }
                if role == self.controller_role {
                    controller_present = true;
                }
                previous = Some(role);
                idx += 1;
            }
            if !controller_present {
                return false;
            }
            arm += 1;
        }
        true
    }
}

impl CompiledProgramRef {
    #[inline(always)]
    const fn participant_role_at(&self, row: usize) -> u8 {
        let offset = match self.column_offset(
            self.columns.route_participants(),
            row,
            PROGRAM_IMAGE_ROUTE_PARTICIPANT_STRIDE,
        ) {
            Some(offset) => offset,
            None => crate::invariant(),
        };
        self.byte_at(offset)
    }

    #[inline]
    const fn route_resolver_row_at(&self, row: usize) -> Option<RouteResolverRow> {
        let offset = match self.column_offset(
            self.columns.route_resolvers(),
            row,
            PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE,
        ) {
            Some(offset) => offset,
            None => return None,
        };
        let participant_end = if row + 1 < self.columns.route_resolver_count() {
            let next_offset = match self.column_offset(
                self.columns.route_resolvers(),
                row + 1,
                PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE,
            ) {
                Some(offset) => offset,
                None => crate::invariant(),
            };
            self.read_u16_at(next_offset + 5)
        } else {
            self.columns.route_participant_count() as u16
        };
        let decoded = match RouteResolverRow::decode(
            self.read_u16_at(offset),
            self.read_u16_at(offset + 2),
            self.byte_at(offset + 4),
            self.read_u16_at(offset + 5),
            self.byte_at(offset + 7),
            participant_end,
            self.columns.route_participant_count(),
        ) {
            Some(row) => row,
            None => crate::invariant(),
        };
        if row == 0 && decoded.participants[0].start != 0 {
            crate::invariant();
        }
        Some(decoded)
    }

    pub(super) const fn validate_route_resolver_rows(&self) {
        let mut row = 0;
        while row < self.columns.route_resolver_count() {
            let decoded = match self.route_resolver_row_at(row) {
                Some(decoded) => decoded,
                None => crate::invariant(),
            };
            if !decoded.participants_are_canonical(self) {
                crate::invariant();
            }
            row += 1;
        }
    }

    pub(super) const fn validate_resolver_index<const E: usize>(&self) {
        if self.columns.route_resolver_count() == 0 {
            if self.resolver_index.len() != 0 {
                crate::invariant();
            }
            return;
        }
        let Some(expected) = super::resolver_index::ResolverIndex::<E>::from_bytes(
            self.columns,
            self.blob.as_slice(),
        ) else {
            crate::invariant();
        };
        if expected.packed_len() != self.resolver_index.len() {
            crate::invariant();
        }
        let mut byte = 0;
        while byte < expected.packed_len() {
            if self.resolver_index.byte_at(byte) != expected.packed_byte(byte) {
                crate::invariant();
            }
            byte += 1;
        }
    }

    #[inline(always)]
    pub(super) const fn route_resolver_slot(&self, scope_id: ScopeId) -> Option<usize> {
        let query = scope_id.raw() as usize;
        if query >= self.resolver_index.len() / 2 {
            return None;
        }
        let byte = query * 2;
        let slot = u16::from_le_bytes([
            self.resolver_index.byte_at(byte),
            self.resolver_index.byte_at(byte + 1),
        ]);
        if slot == u16::MAX {
            None
        } else {
            Some(slot as usize)
        }
    }

    #[inline]
    #[cfg(any(kani, all(test, hibana_repo_tests)))]
    fn route_resolver_row(&self, scope_id: ScopeId) -> RouteResolverRow {
        let slot = match self.route_resolver_slot(scope_id) {
            Some(slot) => slot,
            None => crate::invariant(),
        };
        match self.route_resolver_row_at(slot) {
            Some(row) => row,
            None => crate::invariant(),
        }
    }

    #[inline]
    fn route_resolver_offset(&self, scope_id: ScopeId) -> usize {
        let slot = match self.route_resolver_slot(scope_id) {
            Some(slot) => slot,
            None => crate::invariant(),
        };
        match self.column_offset(
            self.columns.route_resolvers(),
            slot,
            PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE,
        ) {
            Some(offset) => offset,
            None => crate::invariant(),
        }
    }

    #[inline(always)]
    pub(crate) fn route_controller_role(&self, scope_id: ScopeId) -> u8 {
        // Construction validates the whole row and both participant ranges.
        // Controller lookup needs only the role byte of that immutable row.
        self.byte_at(self.route_resolver_offset(scope_id) + 4)
    }

    #[inline(always)]
    pub(crate) fn route_resolver(&self, scope_id: ScopeId) -> Option<DynamicRouteResolver> {
        let offset = self.route_resolver_offset(scope_id);
        let packed = self.read_u16_at(offset);
        if packed & ScopeId::RESERVED_BIT == 0 {
            None
        } else {
            Some(DynamicRouteResolver::new(
                scope_id,
                self.read_u16_at(offset + 2),
            ))
        }
    }

    #[inline]
    #[cfg(any(kani, all(test, hibana_repo_tests)))]
    pub(crate) fn route_participant_count(&self, scope_id: ScopeId, arm: u8) -> usize {
        self.route_resolver_row(scope_id)
            .participant_range(arm)
            .len()
    }

    #[inline]
    #[cfg(any(kani, all(test, hibana_repo_tests)))]
    pub(crate) fn route_participant_at(
        &self,
        scope_id: ScopeId,
        arm: u8,
        idx: usize,
    ) -> Option<u8> {
        let range = self.route_resolver_row(scope_id).participant_range(arm);
        if idx >= range.len() {
            return None;
        }
        Some(self.participant_role_at(range.start as usize + idx))
    }

    #[inline]
    #[cfg(any(kani, all(test, hibana_repo_tests)))]
    pub(crate) fn route_has_participant(&self, scope_id: ScopeId, arm: u8, role: u8) -> bool {
        let range = self.route_resolver_row(scope_id).participant_range(arm);
        let mut idx = 0usize;
        while idx < range.len() {
            let candidate = self.participant_role_at(range.start as usize + idx);
            if candidate == role {
                return true;
            }
            if candidate > role {
                return false;
            }
            idx += 1;
        }
        false
    }

    #[inline(always)]
    pub(crate) const fn route_resolver_row_count(&self) -> usize {
        self.columns.route_resolver_count()
    }

    #[inline(always)]
    pub(crate) fn route_resolver_authority_at_row(
        &self,
        row: usize,
    ) -> Option<(ScopeId, Option<DynamicRouteResolver>)> {
        let authority = self.route_resolver_row_at(row)?;
        Some((authority.scope, authority.resolver()))
    }
}

#[cfg(kani)]
mod kani;

#[cfg(all(test, hibana_repo_tests))]
mod tests;
