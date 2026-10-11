use super::super::{EventCursor, ScopeId};
use crate::global::typestate::{EventArmView, EventCommitMeta};

impl EventCursor {
    #[inline(never)]
    fn send_preview_outbound_event_at(&self, idx: usize) -> Option<(EventCommitMeta, u32)> {
        if let Some(meta) = self.try_send_meta_at(idx) {
            return Some((meta.into(), meta.payload_schema));
        }
        self.try_local_meta_at(idx).map(|meta| {
            (
                EventCommitMeta::new(
                    meta.eff_index,
                    meta.label,
                    meta.origin,
                    meta.scope,
                    meta.route_arm,
                    meta.lane,
                ),
                meta.payload_schema,
            )
        })
    }

    #[inline(never)]
    fn send_preview_progress_start_index_for_contract(
        &self,
        target_label: u8,
        target_schema: u32,
        committed_arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> Option<usize> {
        for idx in 0..self.local_steps_len() {
            let Some((event, schema)) = self.send_preview_outbound_event_at(idx) else {
                continue;
            };
            if event.label != target_label || schema != target_schema {
                continue;
            }
            let preview_conflict = self.machine().event_conflict_for_index(idx);
            let mut arms = |scope, view| match view {
                EventArmView::Committed => committed_arm_for_scope(scope),
                EventArmView::Preview => self.selected_arm_for_reentry_preview_conflict(
                    scope,
                    preview_conflict,
                    committed_arm_for_scope,
                ),
            };
            if self.event_enabled(idx, event, &mut arms).is_ok() {
                return Some(idx);
            }
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
        if let Some(idx) = self.first_pending_step_index() {
            return Some(idx);
        }
        Some(self.index())
    }
}
