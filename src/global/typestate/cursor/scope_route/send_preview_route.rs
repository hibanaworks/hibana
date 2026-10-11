use super::super::{EventCursor, ScopeId};

impl EventCursor {
    #[inline(never)]
    pub(super) fn send_preview_controller_scope_at_for_decision(
        &self,
        idx: usize,
    ) -> Option<ScopeId> {
        let mut selected = None;
        let mut slot = 0usize;
        while slot < self.machine().route_scope_slot_count() {
            if let Some(region) = self.machine().route_scope_rows_by_slot(slot)
                && idx >= region.start()
                && idx < region.end()
            {
                let scope = region.scope();
                if self.is_route_controller(scope) {
                    selected = Some(match selected {
                        Some(current) => self.deeper_route_scope(current, scope),
                        None => scope,
                    });
                }
            }
            slot += 1;
        }
        selected
    }
}
