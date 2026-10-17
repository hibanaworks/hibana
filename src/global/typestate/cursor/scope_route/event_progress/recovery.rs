//! Read-only recovery admission over the projected resource receipt index.
use super::{EventAdmissionError, EventArmView, EventCursor, ScopeId};
use crate::global::{
    role_program::PackedLaneRange,
    typestate::{EventCommitMeta, state_index_to_usize},
};

impl EventCursor {
    fn uses_blocked_view(&self, index: usize, blocked: (u8, Option<u8>)) -> bool {
        self.uses_view_at(index, blocked.0)
            || blocked.1.is_some_and(|role| self.uses_view_at(index, role))
    }

    pub(super) fn reentry_view_wait(
        &self,
        index: usize,
        event: EventCommitMeta,
        arms: &mut dyn FnMut(ScopeId, EventArmView) -> Option<u8>,
    ) -> EventAdmissionError {
        let Some(scope) = self.complete_roll_body_scope_for_index(index, &mut |scope| {
            arms(scope, EventArmView::Committed)
        }) else {
            return EventAdmissionError::Invariant;
        };
        let role = crate::invariant_some(self.logical_role_at(index));
        let sent = if self.is_send_at(index) {
            match self
                .program_ref()
                .event_atom_at(event.eff_index.dense_ordinal())
                .payload
            {
                crate::eff::EventPayload::Endpoint(role) => Some(role),
                crate::eff::EventPayload::Wire(_) => None,
            }
        } else {
            None
        };
        let Some(row) = self.machine().roll_scope_row(scope) else {
            crate::invariant();
        };
        let conflict = self.machine().event_conflict_for_index(index);
        let mut selected = |scope| arms(scope, EventArmView::Preview);
        for prior in row.start()..index.min(row.end()) {
            if self.event_lane_at(prior) == Some(event.lane)
                && self.event_actor_is_local(prior, &mut selected) != Some(false)
                && self.event_conflict_row_allows_with_preview(
                    self.machine().event_conflict_for_index(prior),
                    conflict,
                    &mut selected,
                )
                && self.uses_blocked_view(prior, (role, sent))
            {
                return EventAdmissionError::PredecessorPending;
            }
        }
        EventAdmissionError::OtherViewPending
    }

    pub(crate) fn resource_receipt_wait(
        &self,
        receipts: PackedLaneRange,
        blocked: (u8, Option<u8>),
        selected: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> EventAdmissionError {
        for receipt in receipts.start()..receipts.end() {
            let step = self.machine().event_program().owned_receipt(receipt) as usize;
            let Some(index) = self
                .machine()
                .state_for_step_index(step)
                .map(state_index_to_usize)
            else {
                crate::invariant();
            };
            let Some(meta) = self.try_recv_meta_at(index) else {
                crate::invariant();
            };
            let progress =
                match self.relocatable_resident_lane_step_at_index(index, meta.lane as usize) {
                    Ok(progress) => progress,
                    Err(_) => crate::invariant(),
                };
            if self.relocatable_step_done(progress)
                || self.event_actor_is_local(index, selected) == Some(false)
            {
                continue;
            }
            let conflict = self.machine().event_conflict_for_index(index);
            if !self.event_conflict_row_allows_with_preview(conflict, conflict, selected) {
                continue;
            }
            let enabled = self.event_enabled(
                index,
                EventCommitMeta::from(meta),
                &mut |scope, _view: EventArmView| selected(scope),
            );
            if matches!(enabled, Err(EventAdmissionError::Invariant)) {
                continue;
            }
            let role = crate::invariant_some(self.logical_role_at(index));
            if role == blocked.0 || blocked.1 == Some(role) {
                return EventAdmissionError::PredecessorPending;
            }
            if self.pending_source_predecessor_uses(index, blocked, conflict, selected) {
                return EventAdmissionError::PredecessorPending;
            }
            return EventAdmissionError::OtherViewPending;
        }
        EventAdmissionError::Invariant
    }
}
