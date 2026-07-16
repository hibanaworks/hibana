use super::{
    CursorEndpoint, CursorInvariantError, PendingSendIo, Poll, SendCommitOutcome, SendCommitPlan,
    SendCommitProof, SendError, SendInitOutcome, SendMeta, SendProgressCommitPlan, SendResult,
    SendRouteAudit, SendRouteAuthority, SendRuntimeDesc, StateIndex, TapFrameMeta, Transport, ids,
    lane_port,
};
use crate::global::typestate::state_index_to_usize;

mod route_evidence;

impl<'r, const ROLE: u8, T> CursorEndpoint<'r, ROLE, T>
where
    T: Transport + 'r,
{
    #[inline(never)]
    fn build_send_progress_commit_plan(
        &mut self,
        preview_cursor_index: Option<StateIndex>,
        meta: SendMeta,
        route_authority: SendRouteAuthority,
    ) -> SendResult<SendProgressCommitPlan> {
        let preview_idx = match preview_cursor_index {
            Some(index) => state_index_to_usize(index),
            None => self.cursor.index(),
        };
        let preview_conflict = self.cursor.event_conflict_for_index(preview_idx);
        let route_rows = match route_authority {
            SendRouteAuthority::None => super::SelectedRouteCommitRowsRef::EMPTY,
            SendRouteAuthority::Direct {
                lane,
                audit_start: _,
            } => {
                let selected_routes = self.build_send_selected_route_rows(preview_idx, meta)?;
                if lane != meta.lane || selected_routes.selected_lane() != Some(lane) {
                    return Err(SendError::PhaseInvariant);
                }
                selected_routes
            }
            SendRouteAuthority::MaterializedBranch => {
                self.build_send_selected_route_rows(preview_idx, meta)?
            }
        };
        let route_audit = route_authority.route_audit();
        let mut selected_arm = |scope| {
            let mut row_idx = 0usize;
            while row_idx < route_rows.len() {
                if let Some(row) = route_rows.get(&self.cursor, row_idx)
                    && row.scope() == scope
                {
                    return Some(row.selected_arm());
                }
                row_idx += 1;
            }
            if scope == meta.route_scope {
                meta.selected_route_arm
            } else {
                let mut committed = |candidate| self.selected_arm_for_scope(candidate);
                self.cursor.selected_arm_for_reentry_preview_conflict(
                    scope,
                    preview_conflict,
                    &mut committed,
                )
            }
        };
        let enabled = match self.cursor.event_enabled(
            preview_idx,
            crate::global::typestate::EventCommitMeta::from(meta),
            &mut selected_arm,
        ) {
            Ok(enabled) => enabled,
            Err(CursorInvariantError::INVARIANT) => return Err(SendError::PhaseInvariant),
        };
        let current_route_scope = meta.route_scope;
        let current_route_arm = meta.selected_route_arm;
        let reentry_cursor =
            self.cursor
                .send_reentry_cursor_step(meta, enabled.cursor_after(), |scope| {
                    let mut row_idx = 0usize;
                    while row_idx < route_rows.len() {
                        if let Some(row) = route_rows.get(&self.cursor, row_idx)
                            && row.scope() == scope
                        {
                            return Some(row.selected_arm());
                        }
                        row_idx += 1;
                    }
                    if scope == current_route_scope {
                        return current_route_arm;
                    }
                    let mut committed = |candidate| self.selected_arm_for_scope(candidate);
                    self.cursor.selected_arm_for_reentry_preview_conflict(
                        scope,
                        preview_conflict,
                        &mut committed,
                    )
                });
        let delta = super::CommitDelta::from_meta(
            meta,
            route_rows,
            enabled.cursor_after(),
            enabled.progress_step(),
        )
        .with_lane_relocation(reentry_cursor);
        let delta = match self.prepare_enabled_event_commit_delta(delta, enabled) {
            Ok(delta) => delta,
            Err(CursorInvariantError::INVARIANT) => return Err(SendError::PhaseInvariant),
        };
        Ok(SendProgressCommitPlan { delta, route_audit })
    }

    #[inline(never)]
    fn publish_send_progress_commit_plan(&mut self, plan: SendProgressCommitPlan) {
        let committed = self.commit_prepared_delta(plan.delta);
        match plan.route_audit {
            SendRouteAudit::DirectPreview { start } => {
                self.publish_send_resolver_success_audits_from(&committed, start)
            }
            SendRouteAudit::None => {}
        }
        self.commit_send_route_evidence_delta(&committed, plan.route_audit.fresh_route_start());
        self.emit_send_after_transport_event(&committed);
    }

    #[inline(never)]
    fn build_send_commit_plan(
        &mut self,
        preview_cursor_index: Option<StateIndex>,
        meta: SendMeta,
        route_authority: SendRouteAuthority,
    ) -> SendResult<SendCommitPlan<'r>> {
        let progress =
            self.build_send_progress_commit_plan(preview_cursor_index, meta, route_authority)?;
        Ok(SendCommitPlan {
            proof: SendCommitProof {
                progress,
                _borrow: core::marker::PhantomData,
            },
        })
    }

    #[inline(never)]
    fn preflight_send_descriptor(
        &self,
        meta: SendMeta,
        descriptor: SendRuntimeDesc,
    ) -> SendResult<()> {
        if meta.origin.is_session()
            || descriptor.logical_label() != meta.label
            || descriptor.frame_label() != crate::transport::FrameLabel::new(meta.frame_label)
        {
            Err(SendError::PhaseInvariant)
        } else {
            Ok(())
        }
    }

    #[inline(never)]
    fn begin_send_transport(
        &mut self,
        preview_cursor_index: Option<StateIndex>,
        meta: SendMeta,
        payload: Option<lane_port::RawSendPayload>,
        route_authority: SendRouteAuthority,
    ) -> SendResult<PendingSendIo<'r>> {
        let commit_plan =
            self.build_send_commit_plan(preview_cursor_index, meta, route_authority)?;
        let payload = payload.ok_or(SendError::PhaseInvariant)?;
        let port = self.port_for_lane(meta.lane as usize);
        let lane = port.lane();
        let carrier = if meta.peer == ROLE {
            lane_port::SendCarrier::Local
        } else {
            lane_port::SendCarrier::Transport
        };
        let transport = lane_port::PendingSend::new(
            payload,
            crate::transport::SendMeta {
                eff_index: meta.eff_index,
                logical_label: crate::transport::LogicalLabel::new(meta.label),
                frame_label: crate::transport::FrameLabel::new(meta.frame_label),
                target_role: meta.peer,
                lane: lane.as_wire(),
            },
            carrier,
        );
        Ok(PendingSendIo {
            lane,
            transport,
            commit_plan: Some(commit_plan),
        })
    }

    #[inline(never)]
    fn validate_send_payload(
        &mut self,
        meta: SendMeta,
        descriptor: SendRuntimeDesc,
        preview_cursor_index: Option<StateIndex>,
        route_authority: SendRouteAuthority,
    ) -> SendResult<()> {
        self.preflight_send_descriptor(meta, descriptor)?;

        let preview_idx = match preview_cursor_index {
            Some(index) => state_index_to_usize(index),
            None => self.cursor.index(),
        };
        self.verify_send_route_authority(
            &meta,
            descriptor.logical_label(),
            preview_idx,
            route_authority,
        )?;

        Ok(())
    }

    #[inline(never)]
    pub(crate) fn poll_send_init(
        &mut self,
        descriptor: SendRuntimeDesc,
        meta: SendMeta,
        preview_cursor_index: Option<StateIndex>,
        route_authority: SendRouteAuthority,
        payload: Option<lane_port::RawSendPayload>,
    ) -> SendInitOutcome<'r> {
        if descriptor.frame_label() != crate::transport::FrameLabel::new(meta.frame_label) {
            return SendInitOutcome::Ready(Err(SendError::PhaseInvariant));
        }
        if let Err(err) =
            self.validate_send_payload(meta, descriptor, preview_cursor_index, route_authority)
        {
            return SendInitOutcome::Ready(Err(err));
        }
        let pending =
            match self.begin_send_transport(preview_cursor_index, meta, payload, route_authority) {
                Ok(pending) => pending,
                Err(err) => return SendInitOutcome::Ready(Err(err)),
            };
        SendInitOutcome::Pending { pending }
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
        let transport_poll = lane_port::poll_send_outgoing(&mut pending.transport, port, cx);
        if let Some(kind) = self.session_fault() {
            lane_port::cancel_send_outgoing(&mut pending.transport, port);
            return Poll::Ready(Err(SendError::SessionFault(kind)));
        }
        match transport_poll {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(())) => Poll::Ready(Ok(())),
            Poll::Ready(Err(err)) => {
                if let SendError::Transport(transport_error) = err {
                    self.emit_transport_fault_event(lane_idx, lane_wire, transport_error);
                }
                Poll::Ready(Err(err))
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
