use super::super::{EventCursor, ScopeId};
use crate::global::typestate::LocalConflict;

impl EventCursor {
    #[inline(never)]
    pub(super) fn send_preview_controller_scope_at_for_decision(
        &self,
        idx: usize,
    ) -> Option<ScopeId> {
        if idx >= self.local_steps_len() {
            return None;
        }
        let chain = self.route_commit_range_for_conflict(self.event_conflict_for_index(idx))?;
        for row in self.route_commit_rows(chain).rev() {
            let Some(LocalConflict::RouteArm { scope, .. }) = row.to_conflict() else {
                crate::invariant();
            };
            if self.contains_route_controller(scope) {
                return Some(scope);
            }
        }
        None
    }
}
