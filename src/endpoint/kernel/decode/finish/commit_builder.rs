use super::{
    BranchCommitPlan, BranchKind, BranchPreviewView, Clock, CursorEndpoint, DecodeCommitBuilder,
    DecodeCommitPlan, DecodeLingerCursorPlan, DecodeProgressPlan, EndpointRxAuditPlan, Payload,
    RecvError, RecvMeta, RecvResult, StateIndex, Transport, decode_phase_invariant,
    scope_slot_for_route_from_cursor, state_index_to_usize,
};
use crate::endpoint::kernel::core::{CommitDelta, CommitRow};

impl<'build, 'r, const ROLE: u8, T, C, const MAX_RV: usize>
    DecodeCommitBuilder<'build, 'r, ROLE, T, C, MAX_RV>
where
    T: Transport + 'r,
    C: Clock,
{
    pub(super) fn build_decode_commit_plan(
        &mut self,
        branch_plan: BranchCommitPlan,
        branch: BranchPreviewView,
        meta: RecvMeta,
        branch_meta: RecvMeta,
        audit: EndpointRxAuditPlan,
        committed_payload: Payload<'r>,
    ) -> RecvResult<DecodeCommitPlan<'r>> {
        let mut route_rows = self.route_rows.take().ok_or_else(decode_phase_invariant)?;
        CursorEndpoint::<ROLE, T, C, MAX_RV>::collect_decode_linger_route_rows_from_parts(
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
                    branch_meta.is_internal,
                    branch_meta.scope,
                    branch_meta.route_arm,
                    branch_meta.lane,
                ),
                |candidate| {
                    CursorEndpoint::<ROLE, T, C, MAX_RV>::authorized_route_arm_for_decode(
                        self.decision_state,
                        self.cursor,
                        &route_rows,
                        candidate,
                    )
                },
            )
            .map_err(|_| decode_phase_invariant())?;
        let linger_cursor =
            CursorEndpoint::<ROLE, T, C, MAX_RV>::build_decode_linger_cursor_plan_from_parts(
                self.cursor,
                self.decision_state,
                &route_rows,
                meta,
                enabled.cursor_after(),
            )?;
        let delta = CommitDelta::from_recv_meta(
            branch_meta,
            route_rows.as_commit_rows(branch_meta.lane),
            enabled.cursor_after(),
            enabled.progress_step(),
        )
        .with_lane_relocation(match linger_cursor {
            DecodeLingerCursorPlan::None => None,
            DecodeLingerCursorPlan::SetLane { step } => Some(step),
        });
        Ok(DecodeCommitPlan {
            branch: branch_plan,
            audit,
            progress: DecodeProgressPlan::Wire { delta },
            committed_payload,
        })
    }

    pub(super) fn build_non_wire_decode_commit_plan(
        &mut self,
        branch_plan: BranchCommitPlan,
        audit: EndpointRxAuditPlan,
        branch: BranchPreviewView,
        kind: BranchKind,
        payload: Payload<'r>,
    ) -> RecvResult<DecodeCommitPlan<'r>> {
        let route_rows = self.route_rows.take().ok_or_else(decode_phase_invariant)?;
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
                            branch_meta.is_internal,
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
                DecodeProgressPlan::NonWire {
                    delta: CommitDelta::from_event_row(
                        branch_meta.eff_index,
                        branch_meta.label,
                        branch_meta.is_internal,
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
            BranchKind::EmptyArmTerminal => {
                let next_index = StateIndex::from_usize(self.cursor.index());
                DecodeProgressPlan::NonWire {
                    delta: CommitDelta::route_rows(
                        route_rows.as_route_only_commit_rows(branch_meta.lane_wire),
                        next_index,
                    ),
                }
            }
            BranchKind::WireRecv | BranchKind::ArmSendHint => return Err(decode_phase_invariant()),
        };
        Ok(DecodeCommitPlan {
            branch: branch_plan,
            audit,
            progress,
            committed_payload: payload,
        })
    }
}
