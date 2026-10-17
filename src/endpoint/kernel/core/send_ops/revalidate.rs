//! Pending I/O retains bytes and a choice, not perpetual permission to commit.
//! Recheck live source facts before the next transport acceptance. Actor leases
//! prevent a second local commit between this check and that poll's publication.
use super::{OperationKernel, Poll, SendCommitPlan, SendError, SendResult, Transport};
use crate::endpoint::kernel::core::{CommitDelta, CommitRow};
use crate::global::typestate::{EventAdmissionError, EventArmView, EventCommitMeta};

impl<'r, T: Transport + 'r> OperationKernel<'_, 'r, T> {
    pub(super) fn revalidate_pending_send(
        &mut self,
        plan: &mut SendCommitPlan<'r>,
    ) -> Poll<SendResult<()>> {
        let prepared = &plan.proof.progress.delta;
        let event = prepared.event().ok_or(SendError::PhaseInvariant)?;
        match self.preflight_roll_entry_views(Some(event)) {
            Ok(()) => (),
            Err(EventAdmissionError::OtherViewPending) => return Poll::Pending,
            Err(EventAdmissionError::Invariant | EventAdmissionError::PredecessorPending) => {
                return Poll::Ready(Err(SendError::PhaseInvariant));
            }
        }
        let index = self
            .progress
            .cursor
            .node_index_for_relocatable_step(event.progress_step())
            .ok_or(SendError::PhaseInvariant)?;
        let rows = prepared.selected_routes().as_ref();
        let conflict = self.progress.cursor.event_conflict_for_index(index);
        // Retain the already chosen arms. Revalidation never calls a resolver
        // again and never changes the message already staged by the transport.
        let mut arm = |scope, view| {
            if matches!(view, EventArmView::Committed) {
                return self.selected_arm_for_scope(scope);
            }
            let mut row = 0;
            while row < rows.len() {
                if let Some(selected) = rows.get(&self.progress.cursor, row)
                    && selected.scope() == scope
                {
                    return Some(selected.selected_arm());
                }
                row += 1;
            }
            if scope == event.scope() {
                event.route_arm()
            } else {
                self.progress
                    .cursor
                    .selected_arm_for_reentry_preview_conflict(scope, conflict, &mut |scope| {
                        self.selected_arm_for_scope(scope)
                    })
            }
        };
        let enabled = match self.progress.cursor.event_enabled(
            index,
            EventCommitMeta::new(
                event.eff_index(),
                event.event_label(),
                event.event_origin(),
                event.scope(),
                event.route_arm(),
                event.lane(),
            ),
            &mut arm,
        ) {
            Ok(enabled) => enabled,
            Err(EventAdmissionError::OtherViewPending) => return Poll::Pending,
            Err(EventAdmissionError::Invariant | EventAdmissionError::PredecessorPending) => {
                return Poll::Ready(Err(SendError::PhaseInvariant));
            }
        };
        if enabled.progress_step() != event.progress_step()
            || enabled.cursor_after() != prepared.cursor_after()
        {
            return Poll::Ready(Err(SendError::PhaseInvariant));
        }
        let delta = CommitDelta::from_event_row(
            event.eff_index(),
            event.event_label(),
            event.event_origin(),
            CommitRow::new(event.scope(), event.route_arm(), event.lane()),
            rows,
            enabled.cursor_after(),
            enabled.progress_step(),
        )
        .with_lane_relocation(prepared.lane_relocation());
        let fresh = self
            .prepare_enabled_event_commit_delta(delta, enabled)
            .map_err(|_| SendError::PhaseInvariant)?;
        match (
            self.context
                .program
                .event_atom_at(event.eff_index().dense_ordinal())
                .payload,
            plan.proof.transfer,
        ) {
            (crate::eff::EventPayload::Wire(_), None) => (),
            (crate::eff::EventPayload::Endpoint(role), Some(transfer)) => {
                self.preflight_transfer_out(transfer, role)?;
            }
            _ => return Poll::Ready(Err(SendError::PhaseInvariant)),
        }
        plan.proof.progress.delta = fresh;
        Poll::Ready(Ok(()))
    }
}
