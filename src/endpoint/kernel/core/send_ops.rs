use super::{
    CursorEndpoint, CursorInvariantError, Payload, PendingSendIo, Poll, RouteArmToken,
    SendCommitOutcome, SendCommitPlan, SendCommitProof, SendError, SendInitOutcome, SendMeta,
    SendProgressCommitPlan, SendResolverAuthority, SendResult, SendRuntimeDesc, SendTransportStep,
    StagedSendPayload, StateIndex, TapFrameMeta, Transport, ids, lane_port,
    prepare_event_selected_route_commit_rows_from_resident_route_commit_range,
    prepare_route_site_materialization_rows_from_resident_route_commit_range,
};
use crate::global::typestate::state_index_to_usize;

impl<'r, const ROLE: u8, T> CursorEndpoint<'r, ROLE, T>
where
    T: Transport + 'r,
{
    #[inline(never)]
    fn build_send_selected_route_rows(
        &mut self,
        event_idx: usize,
        meta: SendMeta,
    ) -> SendResult<super::SelectedRouteCommitRowsRef> {
        let Some(selected_arm) = meta.selected_route_arm else {
            return Ok(super::SelectedRouteCommitRowsRef::EMPTY);
        };
        let Self {
            cursor,
            decision_state,
            route_commit_rows,
            ..
        } = self;
        let mut rows = route_commit_rows.begin();
        if meta.route_scope.is_none() {
            prepare_event_selected_route_commit_rows_from_resident_route_commit_range(
                decision_state,
                cursor,
                meta.lane,
                event_idx,
                selected_arm,
                &mut rows,
            )
        } else {
            prepare_route_site_materialization_rows_from_resident_route_commit_range(
                decision_state,
                cursor,
                meta.lane,
                meta.route_scope,
                selected_arm,
                &mut rows,
            )
        }
        .map_err(|_| SendError::PhaseInvariant)?;
        Ok(rows.as_commit_rows(meta.lane))
    }

    #[inline(never)]
    fn publish_send_route_row_evidence(
        &mut self,
        route_row: super::SelectedRouteCommitRow,
        lane_wire: u8,
    ) {
        let scope_id = route_row.scope();
        let selected_arm = route_row.selected_arm();
        let route_token = self.peek_scope_ack(scope_id);
        match route_token {
            Some(RouteArmToken::Ack(_)) => {
                let Some(arm) = super::Arm::new(selected_arm) else {
                    crate::invariant();
                };
                self.record_route_arm_selection_for_lane(
                    lane_wire as usize,
                    scope_id,
                    selected_arm,
                );
                self.emit_route_arm_selection(scope_id, RouteArmToken::from_ack(arm), lane_wire);
            }
            Some(RouteArmToken::Poll(_)) => {
                let Some(arm) = super::Arm::new(selected_arm) else {
                    crate::invariant();
                };
                self.emit_route_arm_selection(
                    scope_id,
                    RouteArmToken::from_poll(arm),
                    self.offer_lane_for_scope(scope_id),
                );
            }
            Some(RouteArmToken::Resolver(arm)) => {
                self.record_route_arm_selection_for_lane(
                    lane_wire as usize,
                    scope_id,
                    selected_arm,
                );
                self.emit_route_arm_selection(
                    scope_id,
                    RouteArmToken::from_resolver(arm),
                    lane_wire,
                );
                if self.arm_has_recv(scope_id, selected_arm) {
                    self.consume_scope_ready_arm(scope_id, selected_arm);
                }
                self.clear_scope_evidence(scope_id);
                self.port_for_lane(lane_wire as usize).clear_route_hints();
                return;
            }
            None if self
                .cursor
                .route_scope_controller_resolver(scope_id)
                .is_some_and(|(resolver, _)| resolver.is_dynamic()) =>
            {
                let Some(arm) = super::Arm::new(selected_arm) else {
                    crate::invariant();
                };
                self.record_route_arm_selection_for_lane(
                    lane_wire as usize,
                    scope_id,
                    selected_arm,
                );
                self.emit_route_arm_selection(
                    scope_id,
                    RouteArmToken::from_resolver(arm),
                    lane_wire,
                );
                if self.arm_has_recv(scope_id, selected_arm) {
                    self.consume_scope_ready_arm(scope_id, selected_arm);
                }
                self.clear_scope_evidence(scope_id);
                self.port_for_lane(lane_wire as usize).clear_route_hints();
                return;
            }
            None => {
                if self.arm_has_recv(scope_id, selected_arm) {
                    self.consume_scope_ready_arm(scope_id, selected_arm);
                }
                self.clear_scope_evidence(scope_id);
                self.port_for_lane(lane_wire as usize).clear_route_hints();
                return;
            }
        }

        if self.arm_has_recv(scope_id, selected_arm) {
            self.consume_scope_ready_arm(scope_id, selected_arm);
        }
        self.clear_scope_evidence(scope_id);
        self.port_for_lane(lane_wire as usize).clear_route_hints();
    }

    #[inline(never)]
    fn publish_send_route_evidence_delta(&mut self, delta: &super::CommittedCommitDelta) {
        let routes = delta.selected_routes();
        let Some(route_lane) = delta.selected_route_lane() else {
            return;
        };
        let mut idx = 0usize;
        while idx < routes.len() {
            if let Some(route_row) = routes.get(&self.cursor, idx) {
                self.publish_send_route_row_evidence(route_row, route_lane);
            }
            idx += 1;
        }
    }

    #[inline(never)]
    fn build_send_progress_commit_plan(
        &mut self,
        preview_cursor_index: Option<StateIndex>,
        meta: SendMeta,
    ) -> SendResult<SendProgressCommitPlan> {
        let preview_idx = match preview_cursor_index {
            Some(index) => state_index_to_usize(index),
            None => self.cursor.index(),
        };
        let enabled = match self.cursor.event_enabled(
            preview_idx,
            crate::global::typestate::EventCommitMeta::from(meta),
            |scope| {
                if scope == meta.route_scope {
                    meta.selected_route_arm
                } else {
                    self.selected_arm_for_scope(scope)
                }
            },
        ) {
            Ok(enabled) => enabled,
            Err(CursorInvariantError::INVARIANT) => return Err(SendError::PhaseInvariant),
        };
        let route_rows = self.build_send_selected_route_rows(preview_idx, meta)?;
        let current_route_scope = meta.route_scope;
        let current_route_arm = meta.selected_route_arm;
        let reentry_cursor =
            self.cursor
                .send_reentry_cursor_step(meta, enabled.cursor_after(), |scope| {
                    if scope == current_route_scope {
                        current_route_arm
                    } else {
                        self.selected_arm_for_scope(scope)
                    }
                });
        let delta = super::CommitDelta::from_meta(
            meta,
            route_rows,
            enabled.cursor_after(),
            enabled.progress_step(),
        )
        .with_lane_relocation(reentry_cursor);
        let delta = match self.prepare_commit_delta(delta) {
            Ok(delta) => delta,
            Err(CursorInvariantError::INVARIANT) => return Err(SendError::PhaseInvariant),
        };
        Ok(SendProgressCommitPlan { delta })
    }

    #[inline(never)]
    fn publish_send_progress_commit_plan(&mut self, plan: SendProgressCommitPlan) {
        let committed = self.commit_prepared_delta(plan.delta);
        self.publish_send_route_evidence_delta(&committed);
        self.emit_send_after_transport_event(&committed);
    }

    #[inline(never)]
    fn stage_send_payload(
        &mut self,
        payload: Option<lane_port::RawSendPayload>,
        scratch: &mut [u8],
    ) -> SendResult<StagedSendPayload> {
        let data = payload.ok_or(SendError::PhaseInvariant)?;
        Ok(StagedSendPayload {
            encoded_len: data.encode_into(scratch)?,
        })
    }

    #[inline(never)]
    fn build_send_commit_plan(
        &mut self,
        preview_cursor_index: Option<StateIndex>,
        meta: SendMeta,
    ) -> SendResult<SendCommitPlan<'r>> {
        let progress = self.build_send_progress_commit_plan(preview_cursor_index, meta)?;
        Ok(SendCommitPlan {
            proof: SendCommitProof {
                progress,
                _borrow: core::marker::PhantomData,
            },
        })
    }

    #[inline(never)]
    fn validate_send_payload(
        &mut self,
        meta: SendMeta,
        descriptor: SendRuntimeDesc,
        preview_cursor_index: Option<StateIndex>,
        resolver_authority: SendResolverAuthority,
    ) -> SendResult<()> {
        if meta.origin.is_session() {
            return Err(SendError::PhaseInvariant);
        }
        if descriptor.frame_label() != crate::transport::FrameLabel::new(meta.frame_label) {
            return Err(SendError::PhaseInvariant);
        }

        let preview_idx = match preview_cursor_index {
            Some(index) => state_index_to_usize(index),
            None => self.cursor.index(),
        };
        self.verify_dynamic_resolver_send_preview(
            &meta,
            descriptor.logical_label(),
            preview_idx,
            resolver_authority,
        )?;

        Ok(())
    }

    #[inline(never)]
    fn begin_send_transport(
        &mut self,
        preview_cursor_index: Option<StateIndex>,
        meta: SendMeta,
        payload: Option<lane_port::RawSendPayload>,
    ) -> SendResult<SendTransportStep<'r>> {
        let scratch_ptr = {
            let port = self.port_for_lane(meta.lane as usize);
            lane_port::scratch_ptr(port)
        };
        let staged_send = {
            let scratch = /* SAFETY: `scratch_ptr` comes from the selected
            lane port's send scratch. This send operation owns the mutable
            endpoint borrow while staging the payload into that scratch buffer. */ unsafe { &mut *scratch_ptr };
            self.stage_send_payload(payload, scratch)?
        };
        let commit_plan = self.build_send_commit_plan(preview_cursor_index, meta)?;
        let encoded_len = staged_send.encoded_len;

        if meta.peer == ROLE {
            return Ok(SendTransportStep::Immediate(commit_plan));
        }

        let port = self.port_for_lane(meta.lane as usize);
        let lane = port.lane();
        let payload_view = {
            let scratch = /* SAFETY: the send payload was staged into this same
            lane scratch above, and `encoded_len` is the length returned by the
            encoder for that buffer. */
                unsafe { &*scratch_ptr };
            Payload::new(&scratch[..encoded_len])
        };
        let outgoing = crate::transport::Outgoing {
            meta: crate::transport::SendMeta {
                eff_index: meta.eff_index,
                logical_label: crate::transport::LogicalLabel::new(meta.label),
                frame_label: crate::transport::FrameLabel::new(meta.frame_label),
                target_role: meta.peer,
                lane: lane.as_wire(),
            },
            payload: payload_view,
        };

        let mut transport = lane_port::PendingSend::new();
        lane_port::begin_send_outgoing(&mut transport, outgoing);
        Ok(SendTransportStep::Pending(PendingSendIo {
            lane,
            transport,
            commit_plan: Some(commit_plan),
        }))
    }

    #[inline(never)]
    pub(crate) fn poll_send_init(
        &mut self,
        descriptor: SendRuntimeDesc,
        meta: SendMeta,
        preview_cursor_index: Option<StateIndex>,
        resolver_authority: SendResolverAuthority,
        payload: Option<lane_port::RawSendPayload>,
    ) -> SendInitOutcome<'r> {
        if descriptor.frame_label() != crate::transport::FrameLabel::new(meta.frame_label) {
            return SendInitOutcome::Ready(Err(SendError::PhaseInvariant));
        }
        if let Err(err) =
            self.validate_send_payload(meta, descriptor, preview_cursor_index, resolver_authority)
        {
            return SendInitOutcome::Ready(Err(err));
        }
        let step = match self.begin_send_transport(preview_cursor_index, meta, payload) {
            Ok(step) => step,
            Err(err) => return SendInitOutcome::Ready(Err(err)),
        };
        match step {
            SendTransportStep::Immediate(commit_plan) => SendInitOutcome::Commit { commit_plan },
            SendTransportStep::Pending(pending) => SendInitOutcome::Pending { pending },
        }
    }

    #[inline(never)]
    fn poll_send_transport(
        &mut self,
        pending: &mut PendingSendIo<'r>,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<SendResult<()>> {
        let lane_idx = pending.lane_idx();
        let port = self.port_for_lane(lane_idx);
        let lane_wire = pending.lane_wire();
        match lane_port::poll_send_outgoing(&mut pending.transport, port, cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(())) => Poll::Ready(Ok(())),
            Poll::Ready(Err(err)) => {
                self.emit_transport_fault_event(lane_idx, lane_wire, err);
                Poll::Ready(Err(SendError::Transport(err)))
            }
        }
    }

    #[inline(never)]
    pub(crate) fn finish_send_after_transport_runtime(
        &mut self,
        commit_plan: SendCommitPlan<'r>,
    ) -> SendCommitOutcome<'r> {
        let SendCommitPlan {
            proof:
                SendCommitProof {
                    progress,
                    _borrow: _,
                },
        } = commit_plan;
        self.publish_send_progress_commit_plan(progress);
        SendCommitOutcome {
            _borrow: core::marker::PhantomData,
        }
    }

    #[inline(never)]
    fn emit_send_after_transport_event(&mut self, delta: &super::CommittedCommitDelta) {
        let event = crate::invariant_some(delta.event());
        let lane = event.lane();
        let lane_wire = self.port_for_lane(lane as usize).lane().as_wire();
        let logical_meta = TapFrameMeta::new(self.sid.raw(), lane_wire, ROLE, event.event_label());
        let event_id = event.event_id(ids::ENDPOINT_SEND, ids::ENDPOINT_SESSION);
        self.emit_endpoint_event(event_id, logical_meta, lane);
    }

    #[inline(never)]
    pub(crate) fn poll_send_pending(
        &mut self,
        pending: &mut PendingSendIo<'r>,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<SendResult<SendCommitPlan<'r>>> {
        match self.poll_send_transport(pending, cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(())) => {
                let commit_plan = crate::invariant_some(pending.commit_plan.take());
                Poll::Ready(Ok(commit_plan))
            }
            Poll::Ready(Err(err)) => {
                pending.commit_plan = None;
                Poll::Ready(Err(err))
            }
        }
    }
}
