use super::LocalConflict;
use crate::global::const_dsl::{ScopeId, ScopeKind};

/// Role-local dependency row guarding an event.
///
/// This is a descriptor fact: the row says which local dependency scope must be
/// complete before the guarded event is enabled, plus the route conflict that
/// decides whether the dependency applies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LocalDependency {
    scope: ScopeId,
    conflict: LocalConflict,
    start: u16,
    end: u16,
}

impl LocalDependency {
    #[inline]
    pub(crate) const fn with_conflict_range(
        scope: ScopeId,
        conflict: LocalConflict,
        start: usize,
        end: usize,
    ) -> Self {
        if scope.is_none()
            || !matches!(scope.kind(), Some(ScopeKind::Parallel))
            || scope.local_ordinal() as usize >= crate::eff::meta::MAX_EFF_NODES
        {
            crate::invariant();
        }
        if start > PackedLocalDependency::STEP_MASK as usize
            || end > PackedLocalDependency::STEP_MASK as usize
        {
            crate::invariant();
        }
        if start >= end {
            crate::invariant();
        }
        if let LocalConflict::RouteArm { scope, arm } = conflict
            && (scope.is_none()
                || !matches!(scope.kind(), Some(ScopeKind::Route))
                || scope.local_ordinal() as usize >= crate::eff::meta::MAX_EFF_NODES
                || arm > 1)
        {
            crate::invariant();
        }
        Self {
            scope,
            conflict,
            start: start as u16,
            end: end as u16,
        }
    }

    #[inline(always)]
    pub(crate) const fn scope(self) -> ScopeId {
        self.scope
    }

    #[inline(always)]
    pub(crate) const fn conflict(self) -> LocalConflict {
        self.conflict
    }

    #[inline(always)]
    pub(crate) const fn start(self) -> usize {
        self.start as usize
    }

    #[inline(always)]
    pub(crate) const fn end(self) -> usize {
        self.end as usize
    }
}

/// Compact role-local dependency row stored beside local step lanes.
///
/// Dependency scopes are always parallel scopes. Route conflicts only need the
/// enclosing route ordinal plus the selected arm. The row stays in four
/// byte-addressable u16 limbs so Cortex-M0+ does not need 64-bit helpers on
/// descriptor reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PackedLocalDependency {
    start: u16,
    end: u16,
    dep_ordinal: u16,
    conflict_route: u16,
}

impl PackedLocalDependency {
    const ABSENT_FIELD: u16 = u16::MAX;
    pub(crate) const STEP_MASK: u16 = (1 << 12) - 1;
    const ROUTE_ORDINAL_MASK: u16 = (1 << 13) - 1;
    const CONFLICT_MASK: u16 = 0b11;
    const ROUTE_SHIFT: u16 = 2;
    const CONFLICT_ROUTE_MASK: u16 =
        (Self::ROUTE_ORDINAL_MASK << Self::ROUTE_SHIFT) | Self::CONFLICT_MASK;
    const CONFLICT_UNCONDITIONAL: u16 = 0;
    const CONFLICT_SHARED_ROUTE: u16 = 1;
    const CONFLICT_ROUTE_ARM_0: u16 = 2;
    const CONFLICT_ROUTE_ARM_1: u16 = 3;

    #[inline(always)]
    pub(crate) const fn none() -> Self {
        Self {
            start: Self::ABSENT_FIELD,
            end: Self::ABSENT_FIELD,
            dep_ordinal: Self::ABSENT_FIELD,
            conflict_route: Self::ABSENT_FIELD,
        }
    }

    #[inline(always)]
    pub(crate) const fn from_packed_parts(
        start: u16,
        end: u16,
        dep_ordinal: u16,
        conflict_route: u16,
    ) -> Self {
        Self {
            start,
            end,
            dep_ordinal,
            conflict_route,
        }
    }

    #[inline(always)]
    pub(crate) const fn start(self) -> u16 {
        self.start
    }

    #[inline(always)]
    pub(crate) const fn end(self) -> u16 {
        self.end
    }

    #[inline(always)]
    pub(crate) const fn dep_ordinal(self) -> u16 {
        self.dep_ordinal
    }

    #[inline(always)]
    pub(crate) const fn conflict_route(self) -> u16 {
        self.conflict_route
    }

    #[inline(always)]
    pub(crate) const fn is_none(self) -> bool {
        if self.start == Self::ABSENT_FIELD {
            if self.end != Self::ABSENT_FIELD
                || self.dep_ordinal != Self::ABSENT_FIELD
                || self.conflict_route != Self::ABSENT_FIELD
            {
                crate::invariant();
            }
            true
        } else {
            false
        }
    }

    pub(crate) const fn from_dependency(dependency: LocalDependency) -> Self {
        let scope = dependency.scope();
        let dep_ordinal = scope.local_ordinal();
        let start = dependency.start();
        let end = dependency.end();

        let (conflict_tag, route_ordinal) = match dependency.conflict() {
            LocalConflict::Unconditional => (Self::CONFLICT_UNCONDITIONAL, 0),
            LocalConflict::SharedRoute => (Self::CONFLICT_SHARED_ROUTE, 0),
            LocalConflict::RouteArm { scope, arm } => {
                let route_ordinal = scope.local_ordinal();
                match arm {
                    0 => (Self::CONFLICT_ROUTE_ARM_0, route_ordinal),
                    1 => (Self::CONFLICT_ROUTE_ARM_1, route_ordinal),
                    _ => crate::invariant(),
                }
            }
        };

        Self {
            start: start as u16,
            end: end as u16,
            dep_ordinal,
            conflict_route: (route_ordinal << Self::ROUTE_SHIFT) | conflict_tag,
        }
    }

    pub(super) const fn decode(self) -> Option<Option<LocalDependency>> {
        if self.start == Self::ABSENT_FIELD {
            return if self.end == Self::ABSENT_FIELD
                && self.dep_ordinal == Self::ABSENT_FIELD
                && self.conflict_route == Self::ABSENT_FIELD
            {
                Some(None)
            } else {
                None
            };
        }
        if self.start >= self.end || self.end > Self::STEP_MASK {
            return None;
        }
        if self.dep_ordinal as usize >= crate::eff::meta::MAX_EFF_NODES {
            return None;
        }
        if (self.conflict_route & !Self::CONFLICT_ROUTE_MASK) != 0 {
            return None;
        }
        let conflict_tag = self.conflict_route & Self::CONFLICT_MASK;
        let route_ordinal = (self.conflict_route >> Self::ROUTE_SHIFT) & Self::ROUTE_ORDINAL_MASK;
        let scope = ScopeId::parallel(self.dep_ordinal);
        let conflict = if conflict_tag == Self::CONFLICT_UNCONDITIONAL {
            if route_ordinal != 0 {
                return None;
            }
            LocalConflict::Unconditional
        } else if conflict_tag == Self::CONFLICT_SHARED_ROUTE {
            if route_ordinal != 0 {
                return None;
            }
            LocalConflict::SharedRoute
        } else if conflict_tag == Self::CONFLICT_ROUTE_ARM_0 {
            if route_ordinal as usize >= crate::eff::meta::MAX_EFF_NODES {
                return None;
            }
            LocalConflict::RouteArm {
                scope: ScopeId::route(route_ordinal),
                arm: 0,
            }
        } else {
            if route_ordinal as usize >= crate::eff::meta::MAX_EFF_NODES {
                return None;
            }
            LocalConflict::RouteArm {
                scope: ScopeId::route(route_ordinal),
                arm: 1,
            }
        };
        Some(Some(LocalDependency::with_conflict_range(
            scope,
            conflict,
            self.start as usize,
            self.end as usize,
        )))
    }

    pub(super) const fn decode_for_event_count(
        self,
        event_count: usize,
    ) -> Option<Option<LocalDependency>> {
        match self.decode() {
            Some(Some(dependency)) if dependency.end() > event_count => None,
            decoded => decoded,
        }
    }

    pub(crate) const fn to_dependency(self, event_count: usize) -> Option<LocalDependency> {
        match self.decode_for_event_count(event_count) {
            Some(dependency) => dependency,
            None => crate::invariant(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{LocalConflict, LocalDependency, PackedLocalDependency, ScopeId};

    #[test]
    fn packed_dependency_round_trips_every_conflict_kind() {
        for conflict in [
            LocalConflict::Unconditional,
            LocalConflict::SharedRoute,
            LocalConflict::RouteArm {
                scope: ScopeId::route(7),
                arm: 0,
            },
            LocalConflict::RouteArm {
                scope: ScopeId::route(7),
                arm: 1,
            },
        ] {
            let dependency =
                LocalDependency::with_conflict_range(ScopeId::parallel(3), conflict, 2, 5);
            let packed = PackedLocalDependency::from_dependency(dependency);
            assert_eq!(packed.decode(), Some(Some(dependency)));
        }
        assert_eq!(PackedLocalDependency::none().decode(), Some(None));
    }

    #[test]
    fn packed_dependency_rejects_partial_absence_and_empty_ranges() {
        let absent = u16::MAX;
        assert_eq!(
            PackedLocalDependency::from_packed_parts(absent, absent, absent, 0).decode(),
            None
        );
        assert_eq!(
            PackedLocalDependency::from_packed_parts(2, 2, 0, 0).decode(),
            None
        );
    }

    #[test]
    #[should_panic]
    fn local_dependency_rejects_absent_scope() {
        let _ = LocalDependency::with_conflict_range(
            ScopeId::none(),
            LocalConflict::Unconditional,
            0,
            1,
        );
    }

    #[test]
    #[should_panic]
    fn local_dependency_rejects_out_of_domain_parallel_scope() {
        let _ = LocalDependency::with_conflict_range(
            ScopeId::parallel(crate::eff::meta::MAX_EFF_NODES as u16),
            LocalConflict::Unconditional,
            0,
            1,
        );
    }

    #[test]
    #[should_panic]
    fn local_dependency_rejects_out_of_domain_route_conflict() {
        let _ = LocalDependency::with_conflict_range(
            ScopeId::parallel(0),
            LocalConflict::RouteArm {
                scope: ScopeId::route(crate::eff::meta::MAX_EFF_NODES as u16),
                arm: 0,
            },
            0,
            1,
        );
    }
}
