use super::{ScopeFacts, binary_route_arm_index};
use crate::global::{
    const_dsl::ScopeId,
    typestate::{LocalConflict, PackedEventConflict},
};

/// Walk one route's commit ancestors once. The emitted rows are written from
/// the end toward the start, retaining outer-to-inner descriptor order without
/// rescanning each prefix or allocating a chain buffer.
pub(in crate::global::role_program::image_impl) struct RouteCommitCursor<'a> {
    scopes: &'a ScopeFacts,
    conflict: PackedEventConflict,
    remaining: usize,
}

impl<'a> RouteCommitCursor<'a> {
    #[inline(always)]
    pub(in crate::global::role_program::image_impl) const fn new(
        scopes: &'a ScopeFacts,
        scope: ScopeId,
        arm: u8,
    ) -> Self {
        let arm = binary_route_arm_index(arm) as u8;
        Self {
            scopes,
            conflict: PackedEventConflict::route_arm(scope, arm),
            remaining: scopes.route_count() + 1,
        }
    }

    #[inline(always)]
    pub(in crate::global::role_program::image_impl) const fn next(
        &mut self,
    ) -> Option<PackedEventConflict> {
        let Some(LocalConflict::RouteArm { scope, .. }) = self.conflict.to_conflict() else {
            return None;
        };
        if scope.is_none() || self.remaining == 0 {
            panic!("route commit ancestor chain invalid");
        }
        let row = self.conflict;
        self.remaining -= 1;
        self.conflict = PackedEventConflict::from_conflict(self.scopes.conflict(scope));
        Some(row)
    }
}

#[inline(always)]
pub(in crate::global::role_program::image_impl) const fn route_commit_row_count(
    scopes: &ScopeFacts,
    scope: ScopeId,
) -> usize {
    // Both arms have this scope's same ancestor chain; only their first row's
    // arm differs. Count once for the scope, not once for each arm.
    let mut rows = RouteCommitCursor::new(scopes, scope, 0);
    let mut count = 0usize;
    while rows.next().is_some() {
        count += 1;
    }
    count
}
