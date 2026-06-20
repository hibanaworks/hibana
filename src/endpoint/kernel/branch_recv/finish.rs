use super::{
    BranchKind, CursorEndpoint, EndpointRxEventPlan, EventCursor, Payload, Poll, RecvCommitPayload,
    RecvCommitPlan, RecvError, RecvMeta, RecvResult, RouteState, SelectedRouteCommitRows,
    StateIndex, Transport, branch_recv_phase_invariant, lane_port,
    prepare_descriptor_checked_recv_reentry_rows_from_resident_route_commit_range,
    scope_slot_for_route_from_cursor, state_index_to_usize,
};
use crate::{
    endpoint::kernel::core::{CommitDelta, CommitRow, OfferedFrame},
    global::typestate::RelocatableResidentLaneStep,
    transport::wire::CodecError,
};

impl<'r, const ROLE: u8, T> CursorEndpoint<'r, ROLE, T>
where
    T: Transport + 'r,
{
    fn prepare_branch_recv_transport_wait(
        &mut self,
        logical_label: u8,
    ) -> RecvResult<Option<crate::global::typestate::RecvMeta>> {
        let Some(branch) = self.public_route_branch.as_ref() else {
            return Err(branch_recv_phase_invariant());
        };
        let expected = logical_label;
        if branch.branch_meta.label != expected {
            return Err(RecvError::LabelMismatch {
                expected,
                actual: branch.branch_meta.label,
            });
        }
        if !matches!(branch.branch_meta.kind, BranchKind::WireRecv)
            || branch.offered_frame.is_some()
        {
            return Ok(None);
        }
        let meta = self
            .cursor
            .try_recv_meta_at(state_index_to_usize(branch.branch_meta.cursor_index))
            .ok_or_else(branch_recv_phase_invariant)?;
        if meta.origin.is_session() {
            return Err(branch_recv_phase_invariant());
        }
        if branch.branch_meta.frame_label != meta.frame_label {
            return Err(branch_recv_phase_invariant());
        }
        Ok(Some(meta))
    }

    fn non_wire_branch_payload() -> Payload<'r> {
        Payload::new(&[])
    }

    fn finish_route_branch_recv(
        &mut self,
        logical_label: u8,
        prepared_meta: Option<crate::global::typestate::RecvMeta>,
        validate: for<'a> fn(Payload<'a>) -> Result<(), CodecError>,
    ) -> RecvResult<Payload<'r>> {
        let Some(branch) = self.public_route_branch.as_ref() else {
            return Err(branch_recv_phase_invariant());
        };
        let branch_meta = branch.branch_meta;
        let label = branch_meta.label;

        let expected = logical_label;
        if label != expected {
            return Err(RecvError::LabelMismatch {
                expected,
                actual: label,
            });
        }
        match branch_meta.kind {
            BranchKind::LocalAction | BranchKind::TerminalArm => {
                let payload = Self::non_wire_branch_payload();
                let branch_plan = self.preflight_branch_preview_commit_plan(branch_meta)?;
                let event =
                    EndpointRxEventPlan::branch(branch_meta.lane_wire, label, branch_meta.origin);
                let route_seed_rows = branch_plan.route_seed_rows();
                let commit_plan = self.build_non_wire_branch_recv_commit_plan(
                    route_seed_rows,
                    branch_meta,
                    event,
                    payload,
                )?;
                self.publish_recv_commit_plan(commit_plan, validate)
            }

            BranchKind::ArmSendHint => Err(branch_recv_phase_invariant()),
            BranchKind::WireRecv => {
                let meta = if let Some(meta) = prepared_meta {
                    meta
                } else if let Some(meta) = self
                    .cursor
                    .try_recv_meta_at(state_index_to_usize(branch_meta.cursor_index))
                {
                    meta
                } else {
                    return Err(branch_recv_phase_invariant());
                };
                if meta.origin.is_session() {
                    return Err(branch_recv_phase_invariant());
                }

                {
                    let Some(branch) = self.public_route_branch.as_ref() else {
                        return Err(branch_recv_phase_invariant());
                    };
                    let Some(offered_frame) = branch.offered_frame.as_ref() else {
                        return Err(branch_recv_phase_invariant());
                    };
                    if offered_frame.lane() != meta.lane {
                        return Err(branch_recv_phase_invariant());
                    }
                    if offered_frame.transport_frame_label() != meta.frame_label {
                        return Err(branch_recv_phase_invariant());
                    }
                }
                let branch_plan = self.preflight_branch_preview_commit_plan(branch_meta)?;
                let branch_recv_meta =
                    branch_plan.meta().ok_or_else(branch_recv_phase_invariant)?;
                let route_seed_rows = branch_plan.route_seed_rows();
                let event =
                    EndpointRxEventPlan::branch(branch_meta.lane_wire, label, branch_meta.origin);
                let frame = {
                    let Some(branch) = self.public_route_branch.as_mut() else {
                        return Err(branch_recv_phase_invariant());
                    };
                    crate::invariant_some(branch.offered_frame.take()).into_frame()
                };
                let commit_plan = self.build_wire_branch_recv_commit_plan(
                    route_seed_rows,
                    meta,
                    branch_recv_meta,
                    branch_meta,
                    event,
                    frame,
                )?;
                self.publish_recv_commit_plan(commit_plan, validate)
            }
        }
    }

    fn build_wire_branch_recv_commit_plan(
        &mut self,
        route_seed_rows: super::SelectedRouteCommitRowsRef,
        meta: RecvMeta,
        branch_meta: RecvMeta,
        publish_branch: super::BranchMeta,
        event: EndpointRxEventPlan,
        frame: lane_port::ReceivedFrame<'r>,
    ) -> RecvResult<RecvCommitPlan<'r>> {
        let branch_scope = publish_branch.scope_id;
        let cursor_index = publish_branch.cursor_index;
        let mut frame = Some(frame);
        let result = (|| {
            let Self {
                cursor,
                decision_state,
                ..
            } = self;
            let route_rows = SelectedRouteCommitRows::from_seed(route_seed_rows)?;
            let mut route_rows = route_rows;
            Self::collect_branch_recv_reentry_route_rows_from_parts(
                cursor,
                decision_state,
                meta,
                branch_scope,
                &mut route_rows,
            )?;
            let mut selected_arm = |candidate| {
                Self::authorized_route_arm_for_branch_recv(
                    decision_state,
                    cursor,
                    &route_rows,
                    candidate,
                )
            };
            let enabled = cursor
                .event_enabled(
                    state_index_to_usize(cursor_index),
                    crate::global::typestate::EventCommitMeta::new(
                        branch_meta.eff_index,
                        branch_meta.label,
                        branch_meta.origin,
                        branch_meta.scope,
                        branch_meta.route_arm,
                        branch_meta.lane,
                    ),
                    &mut selected_arm,
                )
                .map_err(|_| branch_recv_phase_invariant())?;
            let reentry_cursor = Self::branch_recv_reentry_cursor_step_from_parts(
                cursor,
                decision_state,
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
            let delta = self
                .prepare_enabled_event_commit_delta(delta, enabled)
                .map_err(|_| branch_recv_phase_invariant())?;
            let frame = crate::invariant_some(frame.take());
            Ok(RecvCommitPlan::branch(
                publish_branch,
                event,
                delta,
                RecvCommitPayload::wire(frame),
            ))
        })();
        if result.is_err()
            && let Some(frame) = frame
        {
            frame.discard_uncommitted();
        }
        result
    }

    fn build_non_wire_branch_recv_commit_plan(
        &mut self,
        route_seed_rows: super::SelectedRouteCommitRowsRef,
        publish_branch: super::BranchMeta,
        event: EndpointRxEventPlan,
        payload: Payload<'r>,
    ) -> RecvResult<RecvCommitPlan<'r>> {
        let route_rows = SelectedRouteCommitRows::from_seed(route_seed_rows)?;
        let kind = publish_branch.kind;
        let event_commit = crate::global::typestate::EventCommitMeta::new(
            publish_branch.eff_index,
            publish_branch.label,
            publish_branch.origin,
            publish_branch.scope_id,
            Some(publish_branch.selected_arm),
            publish_branch.lane_wire,
        );
        let cursor_index = publish_branch.cursor_index;
        let lane_wire = event_commit.lane;
        let branch_scope = event_commit.scope;
        let delta = match kind {
            BranchKind::LocalAction => {
                let idx = state_index_to_usize(cursor_index);
                let mut selected_arm = |candidate| {
                    if let Some(slot) = scope_slot_for_route_from_cursor(&self.cursor, candidate) {
                        self.decision_state.selected_arm_for_scope_slot(slot)
                    } else {
                        None
                    }
                };
                let enabled = self
                    .cursor
                    .event_enabled(idx, event_commit, &mut selected_arm)
                    .map_err(|_| RecvError::PhaseInvariant)?;
                let delta = CommitDelta::from_event_row(
                    event_commit.eff_index,
                    event_commit.label,
                    event_commit.origin,
                    CommitRow::new(branch_scope, event_commit.route_arm, lane_wire),
                    route_rows.as_commit_rows(lane_wire),
                    enabled.cursor_after(),
                    enabled.progress_step(),
                );
                self.prepare_enabled_event_commit_delta(delta, enabled)
                    .map_err(|_| RecvError::PhaseInvariant)?
            }
            BranchKind::TerminalArm => {
                let next_index = StateIndex::from_usize(self.cursor.index());
                let delta = CommitDelta::route_rows(
                    route_rows.as_route_only_commit_rows(lane_wire),
                    next_index,
                );
                self.prepare_commit_delta(delta)
                    .map_err(|_| RecvError::PhaseInvariant)?
            }
            BranchKind::WireRecv | BranchKind::ArmSendHint => {
                return Err(branch_recv_phase_invariant());
            }
        };
        Ok(RecvCommitPlan::branch(
            publish_branch,
            event,
            delta,
            RecvCommitPayload::non_wire(payload),
        ))
    }

    fn collect_branch_recv_reentry_route_rows_from_parts(
        cursor: &EventCursor,
        decision_state: &RouteState,
        meta: RecvMeta,
        branch_scope: crate::global::const_dsl::ScopeId,
        plan: &mut SelectedRouteCommitRows,
    ) -> RecvResult<()> {
        let mut result = Ok(());
        let completed = cursor.visit_branch_recv_reentry_route_rows(
            meta.scope,
            branch_scope,
            |scope, selected| {
                if Self::selected_route_arm_from_parts(decision_state, cursor, scope).is_some()
                    || plan.arm_for_scope(cursor, scope).is_some()
                {
                    return true;
                }
                let Some(selected) = selected else {
                    result = Err(branch_recv_phase_invariant());
                    return false;
                };
                result =
                    prepare_descriptor_checked_recv_reentry_rows_from_resident_route_commit_range(
                        decision_state,
                        cursor,
                        meta.lane,
                        scope,
                        selected,
                        plan,
                    );
                result.is_ok()
            },
        );
        if completed {
            result
        } else {
            Err(branch_recv_phase_invariant())
        }
    }

    fn selected_route_arm_from_parts(
        decision_state: &RouteState,
        cursor: &EventCursor,
        scope: crate::global::const_dsl::ScopeId,
    ) -> Option<u8> {
        if scope.is_none() {
            return None;
        }
        if let Some(scope_slot) = scope_slot_for_route_from_cursor(cursor, scope)
            && let Some(arm) = decision_state.selected_arm_for_scope_slot(scope_slot)
        {
            return Some(arm);
        }
        None
    }

    fn authorized_route_arm_for_branch_recv(
        decision_state: &RouteState,
        cursor: &EventCursor,
        rows: &SelectedRouteCommitRows,
        scope: crate::global::const_dsl::ScopeId,
    ) -> Option<u8> {
        if let Some(arm) = rows.arm_for_scope(cursor, scope) {
            return Some(arm);
        }
        Self::selected_route_arm_from_parts(decision_state, cursor, scope)
    }

    fn branch_recv_reentry_cursor_step_from_parts(
        cursor: &EventCursor,
        decision_state: &RouteState,
        rows: &SelectedRouteCommitRows,
        meta: RecvMeta,
        next_index: StateIndex,
    ) -> Option<RelocatableResidentLaneStep> {
        cursor.branch_recv_reentry_cursor_step(meta, next_index, |scope| {
            Self::authorized_route_arm_for_branch_recv(decision_state, cursor, rows, scope)
        })
    }
}

impl<'r, const ROLE: u8, T> crate::endpoint::kernel::core::BranchRecvKernelEndpoint<'r>
    for CursorEndpoint<'r, ROLE, T>
where
    T: Transport + 'r,
{
    #[inline]
    fn has_branch_recv_kernel_branch(&self) -> bool {
        self.public_route_branch.is_some()
    }

    #[inline]
    fn prepare_branch_recv_kernel_transport_wait(
        &mut self,
        logical_label: u8,
    ) -> RecvResult<Option<crate::global::typestate::RecvMeta>> {
        self.prepare_branch_recv_transport_wait(logical_label)
    }

    #[inline]
    fn poll_branch_recv_kernel_transport_payload(
        &mut self,
        meta: crate::global::typestate::RecvMeta,
        pending_recv: &mut lane_port::PendingRecv,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<lane_port::ReceivedFrame<'r>>> {
        let lane_idx = meta.lane as usize;
        self.poll_accepted_transport_frame(
            pending_recv,
            lane_idx,
            lane_port::FrameExpectation {
                session_raw: self.sid.raw(),
                lane_wire: meta.lane,
                source_role: meta.peer,
                target_role: ROLE,
                label: meta.frame_label,
            },
            cx,
        )
    }

    #[inline]
    fn stage_branch_recv_kernel_transport_payload(
        &mut self,
        frame: lane_port::ReceivedFrame<'r>,
    ) -> RecvResult<()> {
        let Some(branch) = self.public_route_branch.as_mut() else {
            frame.discard_uncommitted();
            return Err(branch_recv_phase_invariant());
        };
        if branch.offered_frame.is_some() {
            frame.discard_uncommitted();
            return Err(branch_recv_phase_invariant());
        }
        branch.offered_frame = Some(OfferedFrame::new(frame));
        Ok(())
    }

    #[inline]
    fn finish_branch_recv_kernel(
        &mut self,
        logical_label: u8,
        prepared_meta: Option<crate::global::typestate::RecvMeta>,
        validate: for<'a> fn(Payload<'a>) -> Result<(), CodecError>,
    ) -> RecvResult<Payload<'r>> {
        self.finish_route_branch_recv(logical_label, prepared_meta, validate)
    }
}
