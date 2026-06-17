use super::{
    BranchCommitPlan, BranchKind, BranchPreviewView, BranchRecvCommitBuilder, BranchRecvCommitPlan,
    BranchRecvProgressPlan, CursorEndpoint, EndpointRxEventPlan, Payload, RecvError, RecvMeta,
    RecvResult, StateIndex, Transport, branch_recv_phase_invariant,
    scope_slot_for_route_from_cursor, state_index_to_usize,
};
use crate::endpoint::kernel::core::{CommitDelta, CommitRow};

impl<'build, 'r, const ROLE: u8, T> BranchRecvCommitBuilder<'build, 'r, ROLE, T>
where
    T: Transport + 'r,
{
    pub(super) fn build_branch_recv_commit_plan(
        &mut self,
        branch_plan: BranchCommitPlan,
        branch: BranchPreviewView,
        meta: RecvMeta,
        branch_meta: RecvMeta,
        event: EndpointRxEventPlan,
        committed_payload: Payload<'r>,
    ) -> RecvResult<BranchRecvCommitPlan<'r>> {
        let mut route_rows = self
            .route_rows
            .take()
            .ok_or_else(branch_recv_phase_invariant)?;
        CursorEndpoint::<ROLE, T>::collect_branch_recv_reentry_route_rows_from_parts(
            self.cursor,
            self.decision_state,
            meta,
            branch.branch_meta.scope_id,
            &mut route_rows,
        )?;
        let enabled = self
            .cursor
            .event_enabled(
                state_index_to_usize(branch.branch_meta.cursor_index),
                crate::global::typestate::EventCommitMeta::new(
                    branch_meta.eff_index,
                    branch_meta.label,
                    branch_meta.origin,
                    branch_meta.scope,
                    branch_meta.route_arm,
                    branch_meta.lane,
                ),
                |candidate| {
                    CursorEndpoint::<ROLE, T>::authorized_route_arm_for_branch_recv(
                        self.decision_state,
                        self.cursor,
                        &route_rows,
                        candidate,
                    )
                },
            )
            .map_err(|_| branch_recv_phase_invariant())?;
        let reentry_cursor = CursorEndpoint::<ROLE, T>::branch_recv_reentry_cursor_step_from_parts(
            self.cursor,
            self.decision_state,
            &route_rows,
            meta,
            enabled.cursor_after(),
        );
        let delta = CommitDelta::from_recv_meta(
            branch_meta,
            route_rows.as_commit_rows(branch_meta.lane),
            enabled.cursor_after(),
            enabled.progress_step(),
        )
        .with_lane_relocation(reentry_cursor);
        Ok(BranchRecvCommitPlan {
            branch: branch_plan,
            event,
            progress: BranchRecvProgressPlan::Wire { delta },
            committed_payload,
        })
    }

    pub(super) fn build_non_wire_branch_recv_commit_plan(
        &mut self,
        branch_plan: BranchCommitPlan,
        event: EndpointRxEventPlan,
        branch: BranchPreviewView,
        kind: BranchKind,
        payload: Payload<'r>,
    ) -> RecvResult<BranchRecvCommitPlan<'r>> {
        let route_rows = self
            .route_rows
            .take()
            .ok_or_else(branch_recv_phase_invariant)?;
        let branch_meta = branch.branch_meta;
        let progress = match kind {
            BranchKind::LocalAction => {
                let idx = state_index_to_usize(branch_meta.cursor_index);
                let enabled = self
                    .cursor
                    .event_enabled(
                        idx,
                        crate::global::typestate::EventCommitMeta::new(
                            branch_meta.eff_index,
                            branch_meta.label,
                            branch_meta.origin,
                            branch_meta.scope_id,
                            Some(branch_meta.selected_arm),
                            branch_meta.lane_wire,
                        ),
                        |candidate| {
                            if let Some(slot) =
                                scope_slot_for_route_from_cursor(self.cursor, candidate)
                            {
                                self.decision_state.selected_arm_for_scope_slot(slot)
                            } else {
                                None
                            }
                        },
                    )
                    .map_err(|_| RecvError::PhaseInvariant)?;
                BranchRecvProgressPlan::NonWire {
                    delta: CommitDelta::from_event_row(
                        branch_meta.eff_index,
                        branch_meta.label,
                        branch_meta.origin,
                        CommitRow::new(
                            branch_meta.scope_id,
                            Some(branch_meta.selected_arm),
                            branch_meta.lane_wire,
                        ),
                        route_rows.as_commit_rows(branch_meta.lane_wire),
                        enabled.cursor_after(),
                        enabled.progress_step(),
                    ),
                }
            }
            BranchKind::TerminalArm => {
                let next_index = StateIndex::from_usize(self.cursor.index());
                BranchRecvProgressPlan::NonWire {
                    delta: CommitDelta::route_rows(
                        route_rows.as_route_only_commit_rows(branch_meta.lane_wire),
                        next_index,
                    ),
                }
            }
            BranchKind::WireRecv | BranchKind::ArmSendHint => {
                return Err(branch_recv_phase_invariant());
            }
        };
        Ok(BranchRecvCommitPlan {
            branch: branch_plan,
            event,
            progress,
            committed_payload: payload,
        })
    }
}
