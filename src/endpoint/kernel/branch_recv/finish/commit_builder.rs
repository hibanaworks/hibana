use super::{
    BranchCommitPlan, BranchKind, BranchPreviewView, BranchRecvCommitBuilder,
    BranchRecvCommitInput, CursorEndpoint, EndpointRxEventPlan, Payload, RecvCommitPayload,
    RecvError, RecvMeta, RecvResult, StateIndex, Transport, branch_recv_phase_invariant, lane_port,
    scope_slot_for_route_from_cursor, state_index_to_usize,
};
use crate::endpoint::kernel::core::{CommitDelta, CommitRow};

pub(super) struct WireBranchRecvCommitInput<'r> {
    pub(super) branch_plan: BranchCommitPlan,
    pub(super) branch: BranchPreviewView,
    pub(super) meta: RecvMeta,
    pub(super) branch_meta: RecvMeta,
    pub(super) event: EndpointRxEventPlan,
    pub(super) frame: lane_port::ReceivedFrame<'r>,
}

impl<'build, 'r, const ROLE: u8, T> BranchRecvCommitBuilder<'build, 'r, ROLE, T>
where
    T: Transport + 'r,
{
    pub(super) fn build_wire_branch_recv_commit_input(
        &mut self,
        input: WireBranchRecvCommitInput<'r>,
    ) -> RecvResult<BranchRecvCommitInput<'r>> {
        let WireBranchRecvCommitInput {
            branch_plan,
            branch,
            meta,
            branch_meta,
            event,
            frame,
        } = input;
        let mut frame = Some(frame);
        let result = (|| {
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
            let reentry_cursor =
                CursorEndpoint::<ROLE, T>::branch_recv_reentry_cursor_step_from_parts(
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
            let frame = crate::invariant_some(frame.take());
            Ok(BranchRecvCommitInput {
                branch: branch_plan,
                event,
                delta,
                payload: RecvCommitPayload::wire(frame),
            })
        })();
        if result.is_err()
            && let Some(frame) = frame
        {
            frame.discard_uncommitted();
        }
        result
    }

    pub(super) fn build_non_wire_branch_recv_commit_input(
        &mut self,
        branch_plan: BranchCommitPlan,
        event: EndpointRxEventPlan,
        branch: BranchPreviewView,
        kind: BranchKind,
        payload: Payload<'r>,
    ) -> RecvResult<BranchRecvCommitInput<'r>> {
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
                CommitDelta::from_event_row(
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
                )
            }
            BranchKind::TerminalArm => {
                let next_index = StateIndex::from_usize(self.cursor.index());
                CommitDelta::route_rows(
                    route_rows.as_route_only_commit_rows(branch_meta.lane_wire),
                    next_index,
                )
            }
            BranchKind::WireRecv | BranchKind::ArmSendHint => {
                return Err(branch_recv_phase_invariant());
            }
        };
        Ok(BranchRecvCommitInput {
            branch: branch_plan,
            event,
            delta: progress,
            payload: RecvCommitPayload::non_wire(payload),
        })
    }
}
