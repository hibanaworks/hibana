use super::super::{EventCursor, ScopeId};
use crate::global::typestate::LocalConflict;

impl EventCursor {
    #[inline(never)]
    pub(super) fn send_preview_scope_at_depth_for_decision(
        &self,
        idx: usize,
        depth: usize,
    ) -> Option<ScopeId> {
        if idx >= self.local_steps_len() {
            return None;
        }
        let chain = self.route_commit_range_for_conflict(self.event_conflict_for_index(idx))?;
        let row = self.route_commit_rows(chain).nth(depth)?;
        let Some(LocalConflict::RouteArm { scope, .. }) = row.to_conflict() else {
            crate::invariant();
        };
        Some(scope)
    }
}
