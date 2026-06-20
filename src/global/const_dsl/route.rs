use super::{INTRINSIC_ROUTE_RESOLVER_ID, ScopeId, ScopeKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RouteResolver {
    Intrinsic,
    Dynamic { resolver_id: u16, scope: ScopeId },
}

impl RouteResolver {
    pub(crate) const fn is_dynamic(self) -> bool {
        matches!(self, Self::Dynamic { .. })
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
    pub(crate) const fn empty() -> Self {
        Self {
            scope: ScopeId::none(),
            resolver_id: INTRINSIC_ROUTE_RESOLVER_ID,
        }
    }

    pub(crate) const fn new(scope: ScopeId, resolver_id: u16) -> Self {
        if !matches!(scope.kind(), Some(ScopeKind::Route)) {
            panic!("route resolver marker scope");
        }
        Self { scope, resolver_id }
    }

    pub(crate) const fn resolver(self) -> RouteResolver {
        if self.resolver_id == INTRINSIC_ROUTE_RESOLVER_ID {
            RouteResolver::Intrinsic
        } else {
            RouteResolver::Dynamic {
                resolver_id: self.resolver_id,
                scope: self.scope,
            }
        }
    }
}
