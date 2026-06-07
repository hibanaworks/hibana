use super::{
    BranchCommitPlan, BranchPreviewView, Clock, CommitDelta, CursorEndpoint, DecodeCommitPlan,
    DecodeCommitTxn, DecodeLingerCursorPlan, DecodeProgressPlan, DecodePublishPlan,
    EndpointRxAuditPlan, EpochTable, LabelUniverse, LoopCommitRow, MintConfigMarker, Payload,
    RecvMeta, RecvResult, SelectedRouteCommitRow, Transport, decode_phase_invariant,
    event_selected_route_scope_from_cursor, prepare_event_selected_route_commit_row_from_parts,
    scope_slot_for_route_from_cursor, state_index_to_usize,
};

impl<'txn, 'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint>
    DecodeCommitTxn<'txn, 'r, ROLE, T, U, C, E, MAX_RV, Mint>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
{
    pub(super) fn build_decode_commit_plan(
        &mut self,
        branch_plan: BranchCommitPlan,
        branch_route_row: Option<SelectedRouteCommitRow>,
        branch: BranchPreviewView,
        meta: RecvMeta,
        branch_meta: RecvMeta,
        loop_ack: LoopCommitRow,
        audit: EndpointRxAuditPlan,
        committed_payload: Payload<'r>,
    ) -> RecvResult<DecodeCommitPlan<'r>> {
        let mut route_rows = self.route_rows.take().ok_or_else(decode_phase_invariant)?;
        CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint>::collect_decode_linger_route_rows_from_parts(
            self.cursor,
            self.decision_state,
            branch_route_row,
            meta,
            branch.branch_meta.scope_id,
            &mut route_rows,
        )?;
        let enabled = self
            .cursor
            .enabled_event_commit(
                state_index_to_usize(branch.branch_meta.cursor_index),
                branch_meta.eff_index,
                branch_meta.label,
                branch_meta.is_control,
                branch_meta.scope,
                branch_meta.route_arm,
                branch_meta.lane,
                |candidate| {
                    if let Some(slot) = scope_slot_for_route_from_cursor(self.cursor, candidate) {
                        self.decision_state.selected_arm_for_scope_slot(slot)
                    } else {
                        None
                    }
                },
            )
            .map_err(|_| decode_phase_invariant())?;
        let linger_cursor = CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint>::build_decode_linger_cursor_plan_from_parts(
            self.cursor,
            self.decision_state,
            branch_route_row,
            &route_rows,
            meta,
            enabled.cursor_after(),
        )?;
        let delta = CommitDelta::from_recv_meta(
            branch_meta,
            enabled.cursor_after(),
            enabled.progress_step(),
        );
        if let Some(arm) = branch_meta.route_arm {
            match prepare_event_selected_route_commit_row_from_parts(
                self.decision_state,
                self.cursor,
                branch_meta.lane,
                branch_meta.scope,
                arm,
            ) {
                Some(row) => route_rows.push_unique(row)?,
                None => {
                    let route_scope =
                        event_selected_route_scope_from_cursor(self.cursor, branch_meta.scope, arm);
                    let selected = if let Some(slot) =
                        scope_slot_for_route_from_cursor(self.cursor, route_scope)
                    {
                        self.decision_state.selected_arm_for_scope_slot(slot)
                    } else {
                        None
                    };
                    if route_rows.arm_for_scope(route_scope) != Some(arm) && selected != Some(arm) {
                        return Err(decode_phase_invariant());
                    }
                }
            }
        }
        let delta = delta
            .with_selected_route_rows(route_rows.as_commit_rows())
            .with_loop_row(loop_ack)
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

    pub(super) fn build_synthetic_decode_commit_plan(
        &mut self,
        branch_plan: BranchCommitPlan,
        audit: EndpointRxAuditPlan,
        progress: DecodeProgressPlan,
        payload: Payload<'r>,
    ) -> RecvResult<DecodeCommitPlan<'r>> {
        let mut route_rows = self.route_rows.take().ok_or_else(decode_phase_invariant)?;
        let progress = match progress {
            DecodeProgressPlan::Wire { delta } => DecodeProgressPlan::Wire { delta },
            DecodeProgressPlan::Branch { delta } => DecodeProgressPlan::Branch {
                delta: {
                    let routes = delta.selected_routes();
                    let mut idx = 0usize;
                    while idx < routes.len() {
                        route_rows
                            .push_unique(routes.get(idx).ok_or_else(decode_phase_invariant)?)?;
                        idx += 1;
                    }
                    delta.with_selected_route_rows(route_rows.as_commit_rows())
                },
            },
            DecodeProgressPlan::Empty { delta } => DecodeProgressPlan::Empty {
                delta: {
                    let routes = delta.selected_routes();
                    let mut idx = 0usize;
                    while idx < routes.len() {
                        route_rows
                            .push_unique(routes.get(idx).ok_or_else(decode_phase_invariant)?)?;
                        idx += 1;
                    }
                    delta.with_selected_route_rows(route_rows.as_commit_rows())
                },
            },
        };
        Ok(DecodeCommitPlan {
            branch: branch_plan,
            audit,
            progress,
            committed_payload: payload,
        })
    }

    pub(super) fn publish_decode_commit_plan(
        self,
        plan: DecodeCommitPlan<'r>,
    ) -> DecodePublishPlan<'r> {
        DecodePublishPlan {
            branch: plan.branch,
            audit: plan.audit,
            progress: plan.progress,
            committed_payload: plan.committed_payload,
        }
    }
}
