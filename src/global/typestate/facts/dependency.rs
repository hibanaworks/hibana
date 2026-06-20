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
    #[inline(always)]
    pub(crate) const fn with_conflict_range(
        scope: ScopeId,
        conflict: LocalConflict,
        start: usize,
        end: usize,
    ) -> Self {
        if start > PackedLocalDependency::STEP_MASK as usize
            || end > PackedLocalDependency::STEP_MASK as usize
        {
            crate::invariant();
        }
        if start > end {
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
    const DEP_ORDINAL_MASK: u16 = (1 << 12) - 1;
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
        if scope.is_none() {
            return Self::none();
        }
        if !matches!(scope.kind(), Some(ScopeKind::Parallel)) {
            crate::invariant();
        }
        let dep_ordinal = scope.local_ordinal();
        if dep_ordinal > Self::DEP_ORDINAL_MASK {
            crate::invariant();
        }
        let start = dependency.start();
        let end = dependency.end();
        if start > Self::STEP_MASK as usize || end > Self::STEP_MASK as usize || start > end {
            crate::invariant();
        }

        let (conflict_tag, route_ordinal) = match dependency.conflict() {
            LocalConflict::Unconditional => (Self::CONFLICT_UNCONDITIONAL, 0),
            LocalConflict::SharedRoute => (Self::CONFLICT_SHARED_ROUTE, 0),
            LocalConflict::RouteArm { scope, arm } => {
                if scope.is_none() || !matches!(scope.kind(), Some(ScopeKind::Route)) {
                    crate::invariant();
                }
                let route_ordinal = scope.local_ordinal();
                if route_ordinal > Self::ROUTE_ORDINAL_MASK {
                    crate::invariant();
                }
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

    pub(crate) const fn to_dependency(self) -> Option<LocalDependency> {
        if self.is_none() {
            return None;
        }
        if self.start > Self::STEP_MASK || self.end > Self::STEP_MASK || self.start > self.end {
            crate::invariant();
        }
        if self.dep_ordinal > Self::DEP_ORDINAL_MASK {
            crate::invariant();
        }
        if (self.conflict_route & !Self::CONFLICT_ROUTE_MASK) != 0 {
            crate::invariant();
        }
        let conflict_tag = self.conflict_route & Self::CONFLICT_MASK;
        let route_ordinal = (self.conflict_route >> Self::ROUTE_SHIFT) & Self::ROUTE_ORDINAL_MASK;
        let scope = ScopeId::parallel(self.dep_ordinal);
        let conflict = match conflict_tag {
            Self::CONFLICT_UNCONDITIONAL => {
                if route_ordinal != 0 {
                    crate::invariant();
                }
                LocalConflict::Unconditional
            }
            Self::CONFLICT_SHARED_ROUTE => {
                if route_ordinal != 0 {
                    crate::invariant();
                }
                LocalConflict::SharedRoute
            }
            Self::CONFLICT_ROUTE_ARM_0 => LocalConflict::RouteArm {
                scope: ScopeId::route(route_ordinal),
                arm: 0,
            },
            Self::CONFLICT_ROUTE_ARM_1 => LocalConflict::RouteArm {
                scope: ScopeId::route(route_ordinal),
                arm: 1,
            },
            _ => crate::invariant(),
        };
        Some(LocalDependency::with_conflict_range(
            scope,
            conflict,
            self.start as usize,
            self.end as usize,
        ))
    }
}
