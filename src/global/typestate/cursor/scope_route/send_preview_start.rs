use super::super::{EventCursor, ScopeId, SendPreviewError};
use super::EventAdmissionError;
use crate::global::typestate::{EventArmView, EventCommitMeta};

impl EventCursor {
    #[inline(never)]
    fn send_preview_outbound_event_at(
        &self,
        idx: usize,
    ) -> Option<(EventCommitMeta, crate::eff::EventPayload)> {
        if let Some(meta) = self.try_send_meta_at(idx) {
            return Some((meta.into(), meta.payload_kind));
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
                meta.payload_kind,
            )
        })
    }

    #[inline(never)]
    fn send_preview_progress_start_index_for_contract(
        &self,
        role: u8,
        target_label: u8,
        target_payload: crate::eff::EventPayload,
        committed_arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> Result<Option<usize>, SendPreviewError> {
        let mut other_view_pending = false;
        for idx in 0..self.local_steps_len() {
            let Some((event, schema)) = self.send_preview_outbound_event_at(idx) else {
                continue;
            };
            if self.logical_role_at(idx) != Some(role)
                || event.label != target_label
                || schema != target_payload
            {
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
            match self.event_enabled(idx, event, &mut arms) {
                Ok(_) => return Ok(Some(idx)),
                Err(EventAdmissionError::OtherViewPending) => other_view_pending = true,
                Err(EventAdmissionError::Invariant | EventAdmissionError::PredecessorPending) => {}
            }
        }
        if other_view_pending {
            Err(SendPreviewError::OtherViewPending)
        } else {
            Ok(None)
        }
    }

    #[inline]
    pub(super) fn send_preview_start_index_for_contract(
        &self,
        role: u8,
        target_label: u8,
        target_payload: crate::eff::EventPayload,
        selected_arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> Result<usize, SendPreviewError> {
        if let Some(idx) = self.send_preview_progress_start_index_for_contract(
            role,
            target_label,
            target_payload,
            selected_arm_for_scope,
        )? {
            return Ok(idx);
        }
        if self.enclosing_route_scope_rows_at(self.index()).is_some() {
            return Ok(self.index());
        }
        if let Some(idx) = self.first_pending_step_index() {
            return Ok(idx);
        }
        Ok(self.index())
    }
}
