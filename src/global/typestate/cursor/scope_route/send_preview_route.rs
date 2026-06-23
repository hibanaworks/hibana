use super::super::{EventCursor, ScopeId};

impl EventCursor {
    #[inline(never)]
    pub(super) fn send_preview_controller_scope_at_for_decision(
        &self,
        idx: usize,
    ) -> Option<ScopeId> {
        let mut selected = None;
        let mut selected_len = usize::MAX;
        let mut slot = 0usize;
        while let Some(region) = self.machine().route_scope_rows_by_slot(slot) {
            if idx >= region.start() && idx < region.end() {
                let scope = region.scope();
                let len = region.end() - region.start();
                if self.is_route_controller(scope) && len <= selected_len {
                    selected = Some(scope);
                    selected_len = len;
                }
            }
            slot += 1;
        }
        selected
    }
}
