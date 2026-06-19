use super::{
    BranchCommitPlan, BranchKind, BranchPreviewView, BranchRecvCommitBuilder,
    BranchRecvProgressPlan, BranchRecvRuntimeDesc, CursorEndpoint, EndpointRxEventPlan,
    EventCursor, MaterializedRouteBranch, Payload, Poll, PreparedBranchRecvProgressPlan,
    PreparedBranchRecvPublishPlan, RecvCommitPlan, RecvError, RecvMeta, RecvResult, RouteState,
    SelectedRouteCommitRows, StateIndex, Transport, branch_recv_phase_invariant, lane_port,
    prepare_descriptor_checked_recv_reentry_rows_from_resident_route_commit_range,
    scope_slot_for_route_from_cursor, state_index_to_usize,
};
use crate::{global::typestate::RelocatableResidentLaneStep, transport::trace::TapFrameMeta};

mod commit_builder;
use commit_builder::WireBranchRecvCommitInput;

impl<'r, const ROLE: u8, T> CursorEndpoint<'r, ROLE, T>
where
    T: Transport + 'r,
{
    fn prepare_branch_recv_transport_wait(
        &mut self,
        branch: &MaterializedRouteBranch<'r>,
        desc: BranchRecvRuntimeDesc,
    ) -> RecvResult<Option<crate::global::typestate::RecvMeta>> {
        let expected = desc.logical_label();
        if branch.label != expected {
            return Err(RecvError::LabelMismatch {
                expected,
                actual: branch.label,
            });
        }
        if desc.frame_label() != crate::transport::FrameLabel::new(branch.branch_meta.frame_label) {
            return Err(branch_recv_phase_invariant());
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
        if desc.frame_label() != crate::transport::FrameLabel::new(meta.frame_label) {
            return Err(branch_recv_phase_invariant());
        }
        Ok(Some(meta))
    }

    fn non_wire_branch_payload() -> Payload<'r> {
        Payload::new(&[])
    }

    fn finish_route_branch_recv(
        &mut self,
        desc: BranchRecvRuntimeDesc,
        prepared_meta: Option<crate::global::typestate::RecvMeta>,
        branch: &mut MaterializedRouteBranch<'r>,
    ) -> RecvResult<Payload<'r>> {
        let label = branch.label;
        let branch_meta = branch.branch_meta;

        let expected = desc.logical_label();
        if label != expected {
            return Err(RecvError::LabelMismatch {
                expected,
                actual: label,
            });
        }
        match branch_meta.kind {
            BranchKind::LocalAction | BranchKind::TerminalArm => {
                let payload = Self::non_wire_branch_payload();
                let branch_view = BranchPreviewView::from_materialized(branch);
                let branch_plan = self.preflight_branch_preview_commit_plan(branch_view)?;
                let event = EndpointRxEventPlan::from_branch(branch_view);
                let route_seed_rows = branch_plan.route_seed_rows();
                let publish_plan =
                    self.with_branch_recv_commit_builder(route_seed_rows, |mut builder| {
                        builder.build_non_wire_branch_recv_commit_plan(
                            branch_plan,
                            event,
                            branch_view,
                            branch_meta.kind,
                            payload,
                            |payload| desc.validate_payload(payload),
                        )
                    })?;
                let committed_payload = self.publish_branch_recv_commit_plan(publish_plan);
                branch.offered_frame = None;
                Ok(committed_payload)
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

                let Some(offered_frame) = branch.offered_frame.as_ref() else {
                    return Err(branch_recv_phase_invariant());
                };
                if offered_frame.lane() != meta.lane {
                    return Err(branch_recv_phase_invariant());
                }
                if offered_frame.transport_frame_label() != meta.frame_label {
                    return Err(branch_recv_phase_invariant());
                }
                let branch_view = BranchPreviewView::from_materialized(branch);

                let branch_plan = self.preflight_branch_preview_commit_plan(branch_view)?;
                let branch_recv_meta =
                    branch_plan.meta().ok_or_else(branch_recv_phase_invariant)?;
                let route_seed_rows = branch_plan.route_seed_rows();
                let event = EndpointRxEventPlan::from_branch(branch_view);
                let frame = crate::invariant_some(branch.offered_frame.take()).into_frame();
                let publish_plan =
                    self.with_branch_recv_commit_builder(route_seed_rows, |mut builder| {
                        builder.build_branch_recv_commit_plan(
                            WireBranchRecvCommitInput {
                                branch_plan,
                                branch: branch_view,
                                meta,
                                branch_meta: branch_recv_meta,
                                event,
                                frame,
                            },
                            |payload| desc.validate_payload(payload),
                        )
                    })?;
                let committed_payload = self.publish_branch_recv_commit_plan(publish_plan);
                Ok(committed_payload)
            }
        }
    }

    fn with_branch_recv_commit_builder(
        &mut self,
        route_seed_rows: super::SelectedRouteCommitRowsRef,
        f: impl for<'build> FnOnce(
            BranchRecvCommitBuilder<'build, 'r, ROLE, T>,
        ) -> RecvResult<RecvCommitPlan<'r>>,
    ) -> RecvResult<PreparedBranchRecvPublishPlan<'r>> {
        let plan = {
            let Self {
                cursor,
                decision_state,
                ..
            } = self;
            let route_rows = SelectedRouteCommitRows::from_seed(route_seed_rows)?;
            f(BranchRecvCommitBuilder {
                cursor,
                decision_state,
                route_rows: Some(route_rows),
                _role: core::marker::PhantomData,
            })?
        };
        self.prepare_branch_recv_publish_plan(plan)
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

    fn prepare_branch_recv_publish_plan(
        &mut self,
        plan: RecvCommitPlan<'r>,
    ) -> RecvResult<PreparedBranchRecvPublishPlan<'r>> {
        let progress = match plan.progress {
            BranchRecvProgressPlan::Wire { delta } => match self.prepare_commit_delta(delta) {
                Ok(delta) => PreparedBranchRecvProgressPlan::Wire { delta },
                Err(_) => {
                    plan.payload.discard_uncommitted();
                    return Err(RecvError::PhaseInvariant);
                }
            },
            BranchRecvProgressPlan::NonWire { delta } => match self.prepare_commit_delta(delta) {
                Ok(delta) => PreparedBranchRecvProgressPlan::NonWire { delta },
                Err(_) => {
                    plan.payload.discard_uncommitted();
                    return Err(RecvError::PhaseInvariant);
                }
            },
        };
        Ok(PreparedBranchRecvPublishPlan {
            branch: plan.branch,
            event: plan.event,
            progress,
            payload: plan.payload,
        })
    }
}

impl<'r, const ROLE: u8, T> CursorEndpoint<'r, ROLE, T>
where
    T: Transport + 'r,
{
    fn publish_branch_recv_commit_plan(
        &mut self,
        plan: PreparedBranchRecvPublishPlan<'r>,
    ) -> Payload<'r> {
        match plan.progress {
            PreparedBranchRecvProgressPlan::Wire { delta, .. } => {
                self.commit_prepared_delta(delta);
            }
            PreparedBranchRecvProgressPlan::NonWire { delta } => {
                self.commit_prepared_delta(delta);
            }
        }
        self.publish_branch_preview_commit_plan(plan.branch);
        let endpoint_meta =
            TapFrameMeta::new(self.sid.raw(), plan.event.lane, ROLE, plan.event.label);
        self.emit_endpoint_event(plan.event.event_id, endpoint_meta, plan.event.lane);
        plan.payload.into_payload()
    }
}

impl<'r, const ROLE: u8, T> crate::endpoint::kernel::core::BranchRecvKernelEndpoint<'r>
    for CursorEndpoint<'r, ROLE, T>
where
    T: Transport + 'r,
{
    #[inline]
    fn prepare_branch_recv_kernel_transport_wait(
        &mut self,
        desc: BranchRecvRuntimeDesc,
        branch: &MaterializedRouteBranch<'r>,
    ) -> RecvResult<Option<crate::global::typestate::RecvMeta>> {
        self.prepare_branch_recv_transport_wait(branch, desc)
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
    fn finish_branch_recv_kernel(
        &mut self,
        desc: BranchRecvRuntimeDesc,
        prepared_meta: Option<crate::global::typestate::RecvMeta>,
        branch: &mut MaterializedRouteBranch<'r>,
    ) -> RecvResult<Payload<'r>> {
        self.finish_route_branch_recv(desc, prepared_meta, branch)
    }
}
