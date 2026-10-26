//! Read-only recovery admission over the projected resource receipt index.
use super::{EventAdmissionError, EventArmView, EventCursor, ScopeId};
use crate::global::{
    role_program::PackedLaneRange,
    typestate::{EventCommitMeta, state_index_to_usize},
};

impl EventCursor {
    pub(super) fn reentry_view_wait(
        &self,
        index: usize,
        event: EventCommitMeta,
        arms: &mut dyn FnMut(ScopeId, EventArmView) -> Option<u8>,
    ) -> EventAdmissionError {
        let Some(scope) = (!event.body_reentry.is_none())
            .then_some(event.body_reentry)
            .or_else(|| {
                self.complete_roll_body_scope_for_index(index, &mut |scope| {
                    arms(scope, EventArmView::Committed)
                })
            })
        else {
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
        let mut wait = EventAdmissionError::OtherViewPending;
        for prior in row.start()..index.min(row.end()) {
            if !(self.uses_view_at(prior, role)
                || sent.is_some_and(|sent| self.uses_view_at(prior, sent)))
            {
                continue;
            }
            let (before, _) = self
                .machine()
                .event_program()
                .operation_at(prior)
                .unwrap_or_else(|| crate::invariant());
            // A fresh visit clears preceding actions on every lane. Waiting
            // cannot release this view to perform its own causal prefix.
            if (self.event_lane_at(prior) == Some(event.lane)
                || self
                    .program_ref()
                    .source_precedes(before.dense_ordinal(), event.eff_index.dense_ordinal()))
                && self.event_actor_is_local(prior, &mut selected) != Some(false)
                && self.event_conflict_row_allows_with_preview(
                    self.machine().event_conflict_for_index(prior),
                    conflict,
                    &mut selected,
                )
            {
                if self.uses_view_at(prior, role) {
                    return EventAdmissionError::PredecessorPending;
                }
                if sent.is_some_and(|sent| self.uses_view_at(prior, sent)) {
                    wait = EventAdmissionError::PayloadPredecessorPending;
                }
            }
        }
        wait
    }

    fn pending_resource_receipt(
        &self,
        receipt: usize,
        selected: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> Option<(usize, EventCommitMeta)> {
        let step = self.machine().event_program().owned_receipt(receipt) as usize;
        let index = state_index_to_usize(crate::invariant_some(
            self.machine().state_for_step_index(step),
        ));
        let meta = crate::invariant_some(self.try_recv_meta_at(index));
        let progress = crate::invariant_ok(
            self.relocatable_resident_lane_step_at_index(index, meta.lane as usize),
        );
        if self.relocatable_step_done(progress)
            || self.event_actor_is_local(index, selected) == Some(false)
        {
            return None;
        }
        let conflict = self.machine().event_conflict_for_index(index);
        if !self.event_conflict_row_allows_with_preview(conflict, conflict, selected) {
            return None;
        }
        let meta = EventCommitMeta::from(meta);
        if matches!(
            self.event_enabled(index, meta, &mut |scope, _view: EventArmView| selected(
                scope
            )),
            Err(EventAdmissionError::Invariant)
        ) {
            return None;
        }
        Some((index, meta))
    }

    /// A later receipt is not needed to restore this resource if another
    /// pending receipt must already return it to this actor first. Source
    /// causality, not catalog order, establishes that boundary.
    fn receipt_has_prior_return(
        &self,
        receipts: PackedLaneRange,
        event: EventCommitMeta,
        selected: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> bool {
        for receipt in receipts.start()..receipts.end() {
            let Some((index, prior)) = self.pending_resource_receipt(receipt, selected) else {
                continue;
            };
            if self.event_actor_is_local(index, selected) == Some(true)
                && self.program_ref().source_precedes(
                    prior.eff_index.dense_ordinal(),
                    event.eff_index.dense_ordinal(),
                )
            {
                return true;
            }
        }
        false
    }

    pub(crate) fn resource_receipt_wait(
        &self,
        receipts: PackedLaneRange,
        blocked: (u8, Option<u8>),
        selected: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> EventAdmissionError {
        let mut verdict = EventAdmissionError::Invariant;
        for receipt in receipts.start()..receipts.end() {
            let Some((index, meta)) = self.pending_resource_receipt(receipt, selected) else {
                continue;
            };
            let conflict = self.machine().event_conflict_for_index(index);
            let role = crate::invariant_some(self.logical_role_at(index));
            if !self.receipt_has_prior_return(receipts, meta, selected) {
                let wait = if role == blocked.0 {
                    EventAdmissionError::PredecessorPending
                } else {
                    let wait = self.source_predecessor_wait(index, blocked, conflict, selected);
                    if wait == EventAdmissionError::OtherViewPending && blocked.1 == Some(role) {
                        EventAdmissionError::PayloadPredecessorPending
                    } else {
                        wait
                    }
                };
                if wait == EventAdmissionError::PredecessorPending {
                    return wait;
                }
                if wait == EventAdmissionError::PayloadPredecessorPending {
                    verdict = wait;
                    continue;
                }
            }
            // An unknown branch can still require this borrowed view. Do not
            // let an earlier independent alternative hide that self-wait.
            if verdict == EventAdmissionError::Invariant {
                verdict = EventAdmissionError::OtherViewPending;
            }
        }
        verdict
    }
}
