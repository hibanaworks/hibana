use super::super::{EventCursor, ScopeId, SendPreviewError};
use super::EventAdmissionError;
use crate::global::typestate::{EventArmView, EventCommitMeta, MessageSelector};

#[cfg(all(test, hibana_repo_tests))]
#[path = "send_preview_start_tests.rs"]
mod tests;

impl EventCursor {
    #[inline]
    fn send_preview_outbound_event_at(&self, idx: usize) -> Option<EventCommitMeta> {
        if let Some(meta) = self.try_send_meta_at(idx) {
            return Some(meta.into());
        }
        self.try_local_meta_at(idx).map(|meta| {
            EventCommitMeta::new(
                meta.eff_index,
                meta.label,
                meta.origin,
                meta.scope,
                meta.route_arm,
                meta.lane,
            )
        })
    }

    #[inline(never)]
    fn send_preview_progress_start_index_for_contract(
        &self,
        selector: MessageSelector,
        source: Option<core::ops::Range<usize>>,
        committed_arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> Result<Option<usize>, SendPreviewError> {
        let indexed = self
            .machine()
            .event_program()
            .outbound_candidates(selector.role, selector.label);
        // Reuse one immutable lookup for scope checking and live admission.
        // Check membership before completion filtering or resolver callbacks.
        if let Some(source) = &source
            && !self.source_has_outbound_contract(selector, source.clone(), indexed.clone())
        {
            return Err(SendPreviewError::OutOfOrder);
        }
        let mut blocked = None;
        for idx in self.outbound_candidates(selector, source.unwrap_or(0..usize::MAX), indexed) {
            let event = crate::invariant_some(self.send_preview_outbound_event_at(idx));
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
                Err(EventAdmissionError::OtherViewPending) => {
                    blocked = Some(SendPreviewError::OtherViewPending);
                }
                Err(
                    EventAdmissionError::PredecessorPending
                    | EventAdmissionError::PayloadPredecessorPending,
                ) if blocked.is_none() => {
                    blocked = Some(SendPreviewError::OutOfOrder);
                }
                Err(
                    EventAdmissionError::Invariant
                    | EventAdmissionError::PredecessorPending
                    | EventAdmissionError::PayloadPredecessorPending,
                ) => {}
            }
        }
        if let Some(error) = blocked {
            Err(error)
        } else {
            Ok(None)
        }
    }

    #[inline]
    pub(super) fn send_preview_start_index_for_contract(
        &self,
        selector: MessageSelector,
        source: Option<core::ops::Range<usize>>,
        selected_arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> Result<usize, SendPreviewError> {
        if let Some(idx) = self.send_preview_progress_start_index_for_contract(
            selector,
            source,
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
