use super::{INTRINSIC_ROUTE_RESOLVER_ID, ScopeId, ScopeKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DynamicRouteResolver {
    resolver_id: u16,
    scope: ScopeId,
}

impl DynamicRouteResolver {
    pub(crate) const fn new(scope: ScopeId, resolver_id: u16) -> Self {
        if !matches!(scope.kind(), Some(ScopeKind::Route))
            || resolver_id == INTRINSIC_ROUTE_RESOLVER_ID
        {
            crate::invariant();
        }
        Self { resolver_id, scope }
    }

    pub(crate) const fn resolver_id(self) -> u16 {
        self.resolver_id
    }

    pub(crate) const fn scope(self) -> ScopeId {
        self.scope
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReentryMark {
    SinglePass,
    Reentrant,
}

impl ReentryMark {
    pub(crate) const fn is_reentrant(self) -> bool {
        matches!(self, Self::Reentrant)
    }
}

#[derive(Clone, Copy)]
pub(crate) struct RouteResolverMarker {
    pub(crate) scope: ScopeId,
    pub(crate) resolver_id: u16,
}

impl RouteResolverMarker {
    pub(crate) const fn new(scope: ScopeId, resolver_id: u16) -> Self {
        let _ = DynamicRouteResolver::new(scope, resolver_id);
        Self { scope, resolver_id }
    }

    pub(crate) const fn resolver(self) -> DynamicRouteResolver {
        DynamicRouteResolver::new(self.scope, self.resolver_id)
    }
}
