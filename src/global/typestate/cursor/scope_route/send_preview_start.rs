use super::super::{EventCursor, ScopeId};

impl EventCursor {
    #[inline(never)]
    fn send_preview_outbound_contract_lane_at(&self, idx: usize) -> Option<(u8, u32, u8)> {
        if let Some(meta) = self.try_send_meta_at(idx) {
            return Some((meta.label, meta.payload_schema, meta.lane));
        }
        self.try_local_meta_at(idx)
            .map(|meta| (meta.label, meta.payload_schema, meta.lane))
    }

    #[inline(never)]
    fn send_preview_progress_start_index_for_contract(
        &self,
        target_label: u8,
        target_schema: u32,
        selected_arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> Option<usize> {
        let mut idx = 0usize;
        while self.contains_node_index(idx) {
            let Some((label, schema, lane)) = self.send_preview_outbound_contract_lane_at(idx)
            else {
                idx += 1;
                continue;
            };
            if label != target_label || schema != target_schema {
                idx += 1;
                continue;
            }
            let progress_step =
                match self.relocatable_resident_lane_step_at_index(idx, lane as usize) {
                    Ok(step) => step,
                    Err(_) => crate::invariant(),
                };
            let preview_conflict = self.machine().event_conflict_for_index(idx);
            let mut arm_for_scope = |scope| {
                self.selected_arm_for_reentry_preview_conflict(
                    scope,
                    preview_conflict,
                    selected_arm_for_scope,
                )
            };
            if !self.event_conflict_row_allows_with_preview(
                preview_conflict,
                preview_conflict,
                &mut arm_for_scope,
            ) {
                idx += 1;
                continue;
            }
            let mut arm_for_scope = |scope| {
                self.selected_arm_for_reentry_preview_conflict(
                    scope,
                    preview_conflict,
                    selected_arm_for_scope,
                )
            };
            if self.event_lane_head_allows(progress_step, preview_conflict, &mut arm_for_scope) {
                return Some(idx);
            }
            idx += 1;
        }
        None
    }

    #[inline]
    pub(super) fn send_preview_start_index_for_contract(
        &self,
        target_label: u8,
        target_schema: u32,
        selected_arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> Option<usize> {
        if let Some(idx) = self.send_preview_progress_start_index_for_contract(
            target_label,
            target_schema,
            selected_arm_for_scope,
        ) {
            return Some(idx);
        }
        if self.enclosing_route_scope_rows_at(self.index()).is_some() {
            return Some(self.index());
        }
        if let Some(idx) = self.first_pending_step_index(usize::MAX) {
            return Some(idx);
        }
        Some(self.index())
    }
}
