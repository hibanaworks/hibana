use crate::global::const_dsl::{ScopeId, ScopeKind};

/// Projection-baked passive route child fact for one route arm.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PassiveArmChildFact {
    route_scope: ScopeId,
    arm: u8,
    child_route_scope: Option<ScopeId>,
}

impl PassiveArmChildFact {
    #[inline(always)]
    pub(crate) const fn new(
        route_scope: ScopeId,
        arm: u8,
        child_route_scope: Option<ScopeId>,
    ) -> Option<Self> {
        if route_scope.is_none() || !matches!(route_scope.kind(), Some(ScopeKind::Route)) || arm > 1
        {
            return None;
        }
        if let Some(child_scope) = child_route_scope
            && (child_scope.is_none()
                || !matches!(child_scope.kind(), Some(ScopeKind::Route))
                || child_scope.same(route_scope))
        {
            return None;
        }
        Some(Self {
            route_scope,
            arm,
            child_route_scope,
        })
    }

    #[inline(always)]
    pub(crate) const fn route_scope(self) -> ScopeId {
        self.route_scope
    }

    #[inline(always)]
    pub(crate) const fn arm(self) -> u8 {
        self.arm
    }

    #[inline(always)]
    pub(crate) const fn child_route_scope(self) -> Option<ScopeId> {
        self.child_route_scope
    }
}
