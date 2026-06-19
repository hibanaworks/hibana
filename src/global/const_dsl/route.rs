use super::{INTRINSIC_ROUTE_RESOLVER_ID, ScopeId, ScopeKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RouteFrontierSummary {
    scope: ScopeId,
    controller_mask: u16,
    flags: u8,
}

impl RouteFrontierSummary {
    const FLAG_INVALID: u8 = 1 << 0;
    const FLAG_DUPLICATE_LABEL: u8 = 1 << 1;
    const FLAG_BRANCH_LABEL_OVERLAP: u8 = 1 << 2;

    pub(crate) const EMPTY: Self = Self {
        scope: ScopeId::none(),
        controller_mask: 0,
        flags: 0,
    };

    pub(crate) const fn new(
        scope: ScopeId,
        controller_mask: u16,
        invalid: bool,
        duplicate_label: bool,
        branch_label_overlap: bool,
    ) -> Self {
        if !matches!(scope.kind(), Some(ScopeKind::Route)) {
            panic!("route frontier summary scope");
        }
        let mut flags = 0u8;
        if invalid {
            flags |= Self::FLAG_INVALID;
        }
        if duplicate_label {
            flags |= Self::FLAG_DUPLICATE_LABEL;
        }
        if branch_label_overlap {
            flags |= Self::FLAG_BRANCH_LABEL_OVERLAP;
        }
        Self {
            scope,
            controller_mask,
            flags,
        }
    }

    pub(crate) const fn scope(self) -> ScopeId {
        self.scope
    }

    pub(crate) const fn rebase(self, offset: u16) -> Self {
        if self.scope.is_none() {
            self
        } else {
            Self {
                scope: self.scope.add_ordinal(offset),
                controller_mask: self.controller_mask,
                flags: self.flags,
            }
        }
    }

    pub(crate) const fn controller_mask(self) -> u16 {
        self.controller_mask
    }

    pub(crate) const fn is_invalid(self) -> bool {
        (self.flags & Self::FLAG_INVALID) != 0
    }

    pub(crate) const fn has_duplicate_label(self) -> bool {
        (self.flags & Self::FLAG_DUPLICATE_LABEL) != 0
    }

    pub(crate) const fn has_branch_label_overlap(self) -> bool {
        (self.flags & Self::FLAG_BRANCH_LABEL_OVERLAP) != 0
    }
}

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
