use super::{
    BranchCommitPlan, BranchKind, BranchPreviewView, Clock, CursorEndpoint, DecodeCommitPlan,
    DecodeCommitTxn, DecodeLingerCursorPlan, DecodeProgressPlan, DecodeRuntimeDesc,
    EndpointRxAuditPlan, EpochTable, EventCursor, LabelUniverse, LoopCommitRow, LoopMetadata,
    LoopRole, MaterializedRouteBranch, MintConfigMarker, Payload, Poll, PreparedDecodeProgressPlan,
    PreparedDecodePublishPlan, RecvError, RecvMeta, RecvResult, RouteState,
    SelectedRouteCommitRows, StateIndex, Transport, decode_phase_invariant, lane_port,
    prepare_descriptor_checked_recv_linger_rows_from_resident_route_commit_range,
    scope_slot_for_route_from_cursor, state_index_to_usize,
};

mod commit_txn;

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint>
    CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
{
    fn prepare_decode_transport_wait(
        &mut self,
        branch: &MaterializedRouteBranch<'r>,
        desc: DecodeRuntimeDesc,
    ) -> RecvResult<Option<crate::global::typestate::RecvMeta>> {
        let expected = desc.logical_label();
        if branch.label != expected {
            return Err(RecvError::LabelMismatch {
                expected,
                actual: branch.label,
            });
        }
        if desc.frame_label() != crate::transport::FrameLabel::new(branch.branch_meta.frame_label) {
            return Err(decode_phase_invariant());
        }
        if !matches!(branch.branch_meta.kind, BranchKind::WireRecv)
            || branch.staged_payload.is_some()
        {
            return Ok(None);
        }
        let meta = self
            .cursor
            .try_recv_meta_at(state_index_to_usize(branch.branch_meta.cursor_index))
            .ok_or_else(decode_phase_invariant)?;
        if meta.is_control != desc.expects_control() {
            return Err(decode_phase_invariant());
        }
        if desc.frame_label() != crate::transport::FrameLabel::new(meta.frame_label) {
            return Err(decode_phase_invariant());
        }
        let _ = self.preflight_decode_loop_ack(meta)?;
        Ok(Some(meta))
    }

    fn preflight_decode_loop_ack(&self, meta: RecvMeta) -> RecvResult<LoopCommitRow> {
        if !self.control_semantic_kind(meta.semantic).is_loop() {
            return Ok(LoopCommitRow::EMPTY);
        }
        let Some(LoopMetadata {
            scope: scope_id,
            controller,
            target,
            role,
            ..
        }) = self.cursor.loop_metadata_inner()
        else {
            return Ok(LoopCommitRow::EMPTY);
        };
        if role != LoopRole::Target || target != ROLE {
            return Err(decode_phase_invariant());
        }

        if meta.peer != controller {
            return Err(RecvError::PeerMismatch {
                expected: controller,
                actual: meta.peer,
            });
        }

        let lane_idx = meta.lane as usize;
        if lane_idx >= self.cursor.logical_lane_count() {
            return Err(decode_phase_invariant());
        }
        let idx = CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint>::loop_index(scope_id)
            .ok_or_else(decode_phase_invariant)?;
        let port = self.port_for_lane(lane_idx);
        let lane = port.lane();
        Ok(LoopCommitRow::ack(
            scope_id,
            idx,
            meta.lane,
            ROLE,
            port.loop_table().has_decision(lane, idx),
        ))
    }

    fn synthetic_branch_payload(
        &mut self,
        lane_idx: u8,
        desc: DecodeRuntimeDesc,
    ) -> RecvResult<Payload<'r>> {
        let scratch_ptr = {
            let port = self.port_for_lane(lane_idx as usize);
            lane_port::scratch_ptr(port)
        };
        let payload = {
            let scratch = /* SAFETY: the pointer comes from pinned owner storage and this path holds the unique mutable access for the borrow. */ unsafe { &mut *scratch_ptr };
            desc.synthetic_payload(scratch).map_err(RecvError::Codec)?
        };
        Ok(unsafe {
            // SAFETY: synthetic branch payloads borrow from the lane scratch owned
            // by this endpoint for the whole endpoint lifetime.
            lane_port::endpoint_resident_payload(payload)
        })
    }

    fn finish_route_branch_decode(
        &mut self,
        desc: DecodeRuntimeDesc,
        control: Option<crate::global::ControlDesc>,
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
            BranchKind::LocalControl | BranchKind::EmptyArmTerminal => {
                let payload = self.synthetic_branch_payload(branch_meta.lane_wire, desc)?;
                desc.validate_payload(payload).map_err(RecvError::Codec)?;
                let branch_view = BranchPreviewView::from_materialized(branch);
                let branch_plan = self.preflight_branch_preview_commit_plan(branch_view)?;
                let audit = self.build_endpoint_rx_audit_plan(branch_view);
                let route_seed_rows = branch_plan.route_seed_rows();
                let publish_plan = self.with_decode_commit_txn(route_seed_rows, |mut txn| {
                    txn.build_synthetic_decode_commit_plan(
                        branch_plan,
                        audit,
                        branch_view,
                        branch_meta.kind,
                        payload,
                    )
                })?;
                let committed_payload = self.publish_decode_commit_plan(publish_plan);
                branch.staged_payload = None;
                return Ok(committed_payload);
            }

            BranchKind::ArmSendHint => return Err(decode_phase_invariant()),
            BranchKind::WireRecv => {}
        }

        let meta = if let Some(meta) = prepared_meta {
            meta
        } else if let Some(meta) = self
            .cursor
            .try_recv_meta_at(state_index_to_usize(branch_meta.cursor_index))
        {
            meta
        } else {
            return Err(decode_phase_invariant());
        };
        if meta.is_control != desc.expects_control() {
            return Err(decode_phase_invariant());
        }

        let loop_ack_plan = self.preflight_decode_loop_ack(meta)?;

        let staged_payload = branch
            .staged_payload
            .take()
            .ok_or_else(decode_phase_invariant)?;
        if staged_payload.lane() != meta.lane {
            branch.staged_payload = Some(staged_payload);
            return Err(decode_phase_invariant());
        }
        if staged_payload
            .transport_frame_label()
            .is_some_and(|frame_label| frame_label != meta.frame_label)
        {
            branch.staged_payload = Some(staged_payload);
            return Err(decode_phase_invariant());
        }
        let committed_payload = staged_payload;
        let payload =
            match committed_payload.validated_payload(|payload| desc.validate_payload(payload)) {
                Ok(payload) => payload,
                Err(err) => {
                    branch.staged_payload = Some(committed_payload);
                    return Err(RecvError::Codec(err));
                }
            };
        let recv_desc = crate::endpoint::kernel::recv::RecvDescriptor {
            meta,
            cursor_index: branch_meta.cursor_index,
            sid_raw: self.sid.raw(),
            lane_idx: meta.lane as usize,
            lane_wire: meta.lane,
        };
        if let Err(err) = self.validate_inbound_explicit_wire_control(recv_desc, control, payload) {
            branch.staged_payload = Some(committed_payload);
            return Err(err);
        }

        let branch_view = BranchPreviewView::from_materialized(branch);

        branch.staged_payload = Some(committed_payload);
        let branch_plan = self.preflight_branch_preview_commit_plan(branch_view)?;
        let branch_recv_meta = branch_plan.meta().ok_or_else(decode_phase_invariant)?;
        let route_seed_rows = branch_plan.route_seed_rows();
        let audit = self.build_endpoint_rx_audit_plan(branch_view);
        let publish_plan = self.with_decode_commit_txn(route_seed_rows, |mut txn| {
            txn.build_decode_commit_plan(
                branch_plan,
                branch_view,
                meta,
                branch_recv_meta,
                loop_ack_plan,
                audit,
                payload,
            )
        })?;
        let _ = branch
            .staged_payload
            .take()
            .expect("committed wire decode must retain staged payload until explicit frame commit")
            .commit();
        let committed_payload = self.publish_decode_commit_plan(publish_plan);
        Ok(committed_payload)
    }

    fn with_decode_commit_txn(
        &mut self,
        route_seed_rows: super::SelectedRouteCommitRowsRef,
        f: impl for<'txn> FnOnce(
            DecodeCommitTxn<'txn, 'r, ROLE, T, U, C, E, MAX_RV, Mint>,
        ) -> RecvResult<DecodeCommitPlan<'r>>,
    ) -> RecvResult<PreparedDecodePublishPlan<'r>> {
        let plan = {
            let Self {
                cursor,
                decision_state,
                ..
            } = self;
            let route_rows = SelectedRouteCommitRows::from_seed(route_seed_rows)?;
            f(DecodeCommitTxn {
                cursor,
                decision_state,
                route_rows: Some(route_rows),
                _role: core::marker::PhantomData,
            })?
        };
        self.prepare_decode_publish_plan(plan)
    }

    fn collect_decode_linger_route_rows_from_parts(
        cursor: &EventCursor,
        decision_state: &RouteState,
        meta: RecvMeta,
        branch_scope: crate::global::const_dsl::ScopeId,
        plan: &mut SelectedRouteCommitRows,
    ) -> RecvResult<()> {
        let mut result = Ok(());
        let completed =
            cursor.visit_decode_linger_route_rows(meta.scope, branch_scope, |scope, selected| {
                if Self::selected_route_arm_from_parts(decision_state, cursor, scope).is_some()
                    || plan.arm_for_scope(cursor, scope).is_some()
                {
                    return true;
                }
                let Some(selected) = selected else {
                    result = Err(decode_phase_invariant());
                    return false;
                };
                result =
                    prepare_descriptor_checked_recv_linger_rows_from_resident_route_commit_range(
                        decision_state,
                        cursor,
                        meta.lane,
                        scope,
                        selected,
                        plan,
                    );
                result.is_ok()
            });
        if completed {
            result
        } else {
            Err(decode_phase_invariant())
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
        if let Some(scope_slot) = scope_slot_for_route_from_cursor(cursor, scope) {
            if let Some(arm) = decision_state.selected_arm_for_scope_slot(scope_slot) {
                return Some(arm);
            }
        }
        None
    }

    fn authorized_route_arm_for_decode(
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

    fn build_decode_linger_cursor_plan_from_parts(
        cursor: &EventCursor,
        decision_state: &RouteState,
        rows: &SelectedRouteCommitRows,
        meta: RecvMeta,
        next_index: StateIndex,
    ) -> RecvResult<DecodeLingerCursorPlan> {
        Ok(cursor
            .decode_linger_cursor_step(meta, next_index, |scope| {
                Self::authorized_route_arm_for_decode(decision_state, cursor, rows, scope)
            })
            .map(|step| DecodeLingerCursorPlan::SetLane { step })
            .unwrap_or(DecodeLingerCursorPlan::None))
    }

    fn prepare_decode_publish_plan(
        &mut self,
        plan: DecodeCommitPlan<'r>,
    ) -> RecvResult<PreparedDecodePublishPlan<'r>> {
        let progress = match plan.progress {
            DecodeProgressPlan::Wire { delta } => PreparedDecodeProgressPlan::Wire {
                delta: self
                    .prepare_commit_delta(delta)
                    .map_err(|_| RecvError::PhaseInvariant)?,
            },
            DecodeProgressPlan::Branch { delta } => PreparedDecodeProgressPlan::Branch {
                delta: self
                    .prepare_commit_delta(delta)
                    .map_err(|_| RecvError::PhaseInvariant)?,
            },
            DecodeProgressPlan::Empty { delta } => PreparedDecodeProgressPlan::Empty {
                delta: self
                    .prepare_commit_delta(delta)
                    .map_err(|_| RecvError::PhaseInvariant)?,
            },
        };
        Ok(PreparedDecodePublishPlan {
            branch: plan.branch,
            audit: plan.audit,
            progress,
            committed_payload: plan.committed_payload,
        })
    }
}

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint>
    CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
{
    fn publish_decode_commit_plan(&mut self, plan: PreparedDecodePublishPlan<'r>) -> Payload<'r> {
        match plan.progress {
            PreparedDecodeProgressPlan::Wire { delta, .. } => {
                self.commit_prepared_delta(delta);
            }
            PreparedDecodeProgressPlan::Branch { delta } => {
                self.commit_prepared_delta(delta);
            }
            PreparedDecodeProgressPlan::Empty { delta } => {
                self.commit_prepared_delta(delta);
            }
        }
        self.publish_branch_preview_commit_plan(plan.branch);
        self.publish_endpoint_rx_audit(plan.audit);
        plan.committed_payload
    }
}

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint>
    crate::endpoint::kernel::core::DecodeKernelEndpoint<'r>
    for CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
{
    #[inline]
    fn prepare_decode_kernel_transport_wait(
        &mut self,
        desc: DecodeRuntimeDesc,
        branch: &MaterializedRouteBranch<'r>,
    ) -> RecvResult<Option<crate::global::typestate::RecvMeta>> {
        self.prepare_decode_transport_wait(branch, desc)
    }

    #[inline]
    fn poll_decode_kernel_transport_payload(
        &mut self,
        meta: crate::global::typestate::RecvMeta,
        pending_recv: &mut lane_port::PendingRecv,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<lane_port::ReceivedFrame<'r>>> {
        let lane_idx = meta.lane as usize;
        self.poll_accepted_transport_frame(
            pending_recv,
            lane_idx,
            self.sid.raw(),
            meta.lane,
            meta.peer,
            ROLE,
            meta.frame_label,
            cx,
        )
    }

    #[inline]
    fn finish_decode_kernel(
        &mut self,
        desc: DecodeRuntimeDesc,
        control: Option<crate::global::ControlDesc>,
        prepared_meta: Option<crate::global::typestate::RecvMeta>,
        branch: &mut MaterializedRouteBranch<'r>,
    ) -> RecvResult<Payload<'r>> {
        self.finish_route_branch_decode(desc, control, prepared_meta, branch)
    }
}
