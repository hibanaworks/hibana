use super::{
    CAP_HANDLE_LEN, CAP_TOKEN_LEN, CapEntry, CapHeader, CapShot, ControlDesc, ControlOp, CpError,
    CursorEndpoint, DescriptorDispatch, EpochTable, FrameFlags, LabelUniverse, Lane, LoopCommitRow,
    MintConfigMarker, MintedControlToken, Payload, PendingCapRelease, PendingSendIo, PolicySlot,
    Poll, RouteArmToken, ScopeId, SendCommitOutcome, SendCommitPlan, SendCommitProof,
    SendDescriptorTerminal, SendError, SendInitOutcome, SendMeta, SendPayloadPlan,
    SendProgressCommitPlan, SendResult, SendRuntimeDesc, SendTransportStep, StagedControlEmission,
    StagedSendPayload, StateIndex, TapFrameMeta, Transport, ids, lane_port,
    prepare_event_selected_route_commit_rows_from_conflict_chain,
};
use crate::global::typestate::state_index_to_usize;

const SEND_ROUTE_SOURCE_NONE: u8 = 0;
const SEND_ROUTE_SOURCE_ACK: u8 = 1;
const SEND_ROUTE_SOURCE_POLL: u8 = 2;

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint>
    CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
{
    pub(crate) fn map_cp_error(err: CpError) -> SendError {
        match err {
            CpError::PolicyAbort { reason } => SendError::PolicyAbort { reason },
            _ => SendError::PhaseInvariant,
        }
    }

    #[inline(never)]
    fn build_send_selected_route_rows(
        &mut self,
        event_idx: usize,
        meta: SendMeta,
    ) -> SendResult<super::SelectedRouteCommitRowsRef> {
        let Some(selected_arm) = meta.route_arm else {
            return Ok(super::SelectedRouteCommitRowsRef::EMPTY);
        };
        let Self {
            cursor,
            decision_state,
            route_commit_rows,
            ..
        } = self;
        let mut rows = route_commit_rows
            .begin()
            .map_err(|_| SendError::PhaseInvariant)?;
        prepare_event_selected_route_commit_rows_from_conflict_chain(
            decision_state,
            cursor,
            meta.lane,
            event_idx,
            selected_arm,
            &mut rows,
        )
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
        let source = match route_token {
            Some(token) if token.is_ack() => SEND_ROUTE_SOURCE_ACK,
            Some(token) if token.is_poll() => SEND_ROUTE_SOURCE_POLL,
            Some(_) | None => SEND_ROUTE_SOURCE_NONE,
        };
        match source {
            SEND_ROUTE_SOURCE_ACK if self.cursor.is_route_controller(scope_id) => {
                let Some(arm) = super::Arm::new(selected_arm) else {
                    return;
                };
                self.record_route_arm_selection_for_lane(
                    lane_wire as usize,
                    scope_id,
                    selected_arm,
                );
                self.emit_route_arm_selection(scope_id, RouteArmToken::from_ack(arm), lane_wire);
            }
            SEND_ROUTE_SOURCE_POLL => {
                let Some(arm) = super::Arm::new(selected_arm) else {
                    return;
                };
                self.emit_route_arm_selection(
                    scope_id,
                    RouteArmToken::from_poll(arm),
                    self.offer_lane_for_scope(scope_id),
                );
            }
            _ => {}
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
            if let Some(route_row) = routes.get(idx) {
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
        loop_row: LoopCommitRow,
    ) -> SendResult<SendProgressCommitPlan> {
        let preview_idx = preview_cursor_index
            .map(state_index_to_usize)
            .unwrap_or_else(|| self.cursor.index());
        let enabled = match self.cursor.event_enabled(
            preview_idx,
            meta.eff_index,
            meta.label,
            meta.is_control,
            meta.scope,
            meta.route_arm,
            meta.lane,
            |scope| self.selected_arm_for_scope(scope),
        ) {
            Ok(enabled) => enabled,
            Err(_) => return Err(SendError::PhaseInvariant),
        };
        let route_rows = match self.build_send_selected_route_rows(preview_idx, meta) {
            Ok(rows) => rows,
            Err(err) => return Err(err),
        };
        let delta = super::CommitDelta::from_meta(
            meta,
            route_rows,
            enabled.cursor_after(),
            enabled.progress_step(),
        )
        .with_loop_row(loop_row);
        let delta = match self.prepare_commit_delta(delta) {
            Ok(delta) => delta,
            Err(_) => return Err(SendError::PhaseInvariant),
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
        descriptor: SendRuntimeDesc,
        plan: SendPayloadPlan<'r>,
        payload: Option<lane_port::RawSendPayload>,
        scratch: &mut [u8],
    ) -> SendResult<(StagedSendPayload<'r>, Option<DescriptorDispatch>)>
    where
        <Mint as MintConfigMarker>::Policy: crate::control::cap::mint::AllowsEndpointMint,
    {
        match plan {
            SendPayloadPlan::Data => {
                let data = payload.ok_or(SendError::PhaseInvariant)?;
                Ok((
                    StagedSendPayload {
                        encoded_len: descriptor.encode_payload(data, scratch)?,
                        control: StagedControlEmission::None,
                    },
                    None,
                ))
            }
            SendPayloadPlan::LocalControl { token } => {
                Self::validate_empty_local_control_payload(descriptor, payload, scratch)?;
                let dispatch = token.dispatch;
                scratch[..CAP_TOKEN_LEN].copy_from_slice(&token.token_bytes);
                Ok((
                    StagedSendPayload {
                        encoded_len: CAP_TOKEN_LEN,
                        control: StagedControlEmission::Registered(token.rollback),
                    },
                    Some(dispatch),
                ))
            }
            SendPayloadPlan::ExplicitWireControl { dispatch } => {
                let data = payload.ok_or(SendError::PhaseInvariant)?;
                let encoded_len = descriptor.encode_payload(data, scratch)?;
                Self::validate_explicit_wire_control_length(encoded_len)?;
                Ok((
                    StagedSendPayload {
                        encoded_len,
                        control: StagedControlEmission::WireOnly,
                    },
                    Some(dispatch),
                ))
            }
        }
    }

    #[inline(never)]
    fn validate_empty_local_control_payload(
        descriptor: SendRuntimeDesc,
        payload: Option<lane_port::RawSendPayload>,
        scratch: &mut [u8],
    ) -> SendResult<()> {
        let Some(data) = payload else {
            return Ok(());
        };
        let encoded_len = descriptor.encode_payload(data, scratch)?;
        if encoded_len == 0 {
            Ok(())
        } else {
            Err(SendError::PhaseInvariant)
        }
    }

    #[inline(never)]
    fn validate_explicit_wire_control_length(encoded_len: usize) -> SendResult<()> {
        if encoded_len != CAP_TOKEN_LEN {
            return Err(SendError::PhaseInvariant);
        }
        Ok(())
    }

    #[inline(never)]
    pub(crate) fn mint_descriptor_token_bytes(
        &mut self,
        peer: u8,
        shot: CapShot,
        lane: Lane,
        scope: ScopeId,
        epoch: u16,
        control: ControlDesc,
        handle_bytes: [u8; CAP_HANDLE_LEN],
    ) -> SendResult<MintedControlToken<'r>>
    where
        <Mint as MintConfigMarker>::Policy: crate::control::cap::mint::AllowsEndpointMint,
    {
        let cluster = self.control.cluster().ok_or(SendError::PhaseInvariant)?;
        let rendezvous = cluster
            .get_local(&self.rendezvous_id())
            .ok_or(SendError::PhaseInvariant)?;
        let strategy = self.mint.as_config().strategy();
        let mut minted_nonce = None;
        rendezvous
            .caps()
            .insert_entry_with(|| {
                let nonce = strategy.derive_nonce(rendezvous.next_nonce_seed());
                minted_nonce = Some(nonce);
                CapEntry::new(lane, rendezvous.next_cap_revision(), nonce)
            })
            .map_err(|_| SendError::PhaseInvariant)?;
        let nonce =
            minted_nonce.expect("cap insertion builder must run after vacant-slot preflight");
        let rollback = PendingCapRelease::new(nonce, rendezvous.cap_release_ctx(lane));

        let mut header = [0u8; crate::control::cap::mint::CAP_HEADER_LEN];
        CapHeader::new(
            self.sid,
            lane,
            peer,
            control.resource_tag(),
            control.op(),
            control.path(),
            shot,
            control.scope_kind(),
            control.header_flags(),
            scope.local_ordinal(),
            epoch,
            handle_bytes,
        )
        .encode(&mut header);
        let mut token_bytes = [0u8; crate::control::cap::mint::CAP_TOKEN_LEN];
        token_bytes[..crate::control::cap::mint::CAP_NONCE_LEN].copy_from_slice(&nonce);
        token_bytes[crate::control::cap::mint::CAP_NONCE_LEN
            ..crate::control::cap::mint::CAP_NONCE_LEN + crate::control::cap::mint::CAP_HEADER_LEN]
            .copy_from_slice(&header);
        Ok(MintedControlToken {
            token_bytes,
            dispatch: DescriptorDispatch::new(control, scope, epoch),
            rollback,
        })
    }

    #[inline(never)]
    fn mint_send_control(
        &mut self,
        meta: SendMeta,
        descriptor: SendRuntimeDesc,
    ) -> SendResult<Option<MintedControlToken<'r>>>
    where
        <Mint as MintConfigMarker>::Policy: crate::control::cap::mint::AllowsEndpointMint,
    {
        let Some(control) = descriptor.control() else {
            return Ok(None);
        };
        if matches!(control.path(), crate::control::cap::mint::ControlPath::Wire) {
            return Err(SendError::PhaseInvariant);
        }

        let lane = self.port_for_lane(meta.lane as usize).lane();
        let shot = meta.shot.ok_or(SendError::PhaseInvariant)?;
        let minted = match control.op() {
            ControlOp::LoopContinue => {
                self.mint_local_loop_continue_control(&meta, shot, lane, control)?
            }
            ControlOp::LoopBreak => {
                self.mint_local_loop_break_control(&meta, shot, lane, control)?
            }
            ControlOp::TopologyBegin | ControlOp::TopologyAck | ControlOp::TopologyCommit => {
                return Err(SendError::PhaseInvariant);
            }
            _ => {
                let encode_control_handle = descriptor
                    .encode_control_handle()
                    .ok_or(SendError::PhaseInvariant)?;
                let epoch = self.descriptor_send_epoch(control, lane)?;
                self.mint_descriptor_token_bytes(
                    meta.peer,
                    shot,
                    lane,
                    meta.scope,
                    epoch,
                    control,
                    encode_control_handle(self.sid, lane.as_wire(), meta.scope.raw()),
                )?
            }
        };
        Ok(Some(minted))
    }

    #[inline]
    fn descriptor_send_epoch(&self, control: ControlDesc, lane: Lane) -> SendResult<u16> {
        match control.op() {
            ControlOp::AbortAck | ControlOp::StateSnapshot => {
                let cluster = self.control.cluster().ok_or(SendError::PhaseInvariant)?;
                let rendezvous = cluster
                    .get_local(&self.rendezvous_id())
                    .ok_or(SendError::PhaseInvariant)?;
                Ok(rendezvous.lane_generation(lane).raw())
            }
            ControlOp::StateRestore | ControlOp::TxCommit | ControlOp::TxAbort => {
                let cluster = self.control.cluster().ok_or(SendError::PhaseInvariant)?;
                let rendezvous = cluster
                    .get_local(&self.rendezvous_id())
                    .ok_or(SendError::PhaseInvariant)?;
                rendezvous
                    .snapshot_generation(lane)
                    .map(|generation| generation.raw())
                    .ok_or(SendError::PhaseInvariant)
            }
            _ => Ok(0),
        }
    }

    #[inline(never)]
    fn reserve_descriptor_terminal_for_send(
        &self,
        meta: SendMeta,
        token_bytes: Option<[u8; CAP_TOKEN_LEN]>,
        dispatch: Option<DescriptorDispatch>,
    ) -> SendResult<SendDescriptorTerminal<'r>> {
        let (Some(dispatch), Some(bytes)) = (dispatch, token_bytes) else {
            return Ok(SendDescriptorTerminal::none());
        };
        let cluster = self.control.cluster().ok_or(SendError::PhaseInvariant)?;
        let ticket = cluster
            .prepare_send_bound_descriptor_terminal(
                self.rendezvous_id(),
                bytes,
                dispatch.desc,
                self.sid,
                Lane::new(meta.lane as u32),
                meta.peer,
                dispatch.scope_id,
                dispatch.epoch,
            )
            .map_err(|_| SendError::PhaseInvariant)?;
        Ok(SendDescriptorTerminal::terminal(ticket))
    }

    #[inline(never)]
    fn build_send_commit_plan(
        &mut self,
        preview_cursor_index: Option<StateIndex>,
        meta: SendMeta,
        control: StagedControlEmission<'r>,
        dispatch: Option<DescriptorDispatch>,
        token_bytes: Option<[u8; CAP_TOKEN_LEN]>,
    ) -> SendResult<SendCommitPlan<'r>> {
        let loop_row = self.build_send_loop_commit_row(meta, &control, dispatch)?;
        let progress =
            self.build_send_progress_commit_plan(preview_cursor_index, meta, loop_row)?;
        let descriptor = self.reserve_descriptor_terminal_for_send(meta, token_bytes, dispatch)?;
        Ok(SendCommitPlan {
            control,
            proof: SendCommitProof {
                descriptor,
                progress,
            },
        })
    }

    #[inline(never)]
    fn prepare_send_payload_plan(
        &mut self,
        meta: SendMeta,
        descriptor: SendRuntimeDesc,
        has_payload: bool,
    ) -> SendResult<SendPayloadPlan<'r>>
    where
        <Mint as MintConfigMarker>::Policy: crate::control::cap::mint::AllowsEndpointMint,
    {
        if meta.is_control != descriptor.expects_control() {
            return Err(SendError::PhaseInvariant);
        }
        if descriptor.frame_label() != crate::transport::FrameLabel::new(meta.frame_label) {
            return Err(SendError::PhaseInvariant);
        }

        let control = descriptor.control();
        self.evaluate_dynamic_policy(&meta, descriptor.logical_label(), control)?;

        let lane = Lane::new(meta.lane as u32);
        // EndpointTx policy audit is an attempt-side replay tuple for the
        // policy input that authorized this send attempt. The observable
        // ENDPOINT_SEND / ENDPOINT_CONTROL event is emitted only from the
        // post-transport commit path.
        self.emit_endpoint_policy_audit(
            PolicySlot::EndpointTx,
            ids::ENDPOINT_SEND,
            self.sid.raw(),
            Self::endpoint_policy_args(lane, meta.label, FrameFlags::empty()),
            lane,
        );

        match control {
            None => Ok(SendPayloadPlan::Data),
            Some(control) => match control.path() {
                crate::control::cap::mint::ControlPath::Local => {
                    let token = self
                        .mint_send_control(meta, descriptor)?
                        .ok_or(SendError::PhaseInvariant)?;
                    Ok(SendPayloadPlan::LocalControl { token })
                }
                crate::control::cap::mint::ControlPath::Wire => {
                    if has_payload {
                        Ok(SendPayloadPlan::ExplicitWireControl {
                            dispatch: DescriptorDispatch::new(
                                control,
                                meta.scope,
                                self.descriptor_send_epoch(control, lane)?,
                            ),
                        })
                    } else {
                        Err(SendError::PhaseInvariant)
                    }
                }
            },
        }
    }

    #[inline(never)]
    fn begin_send_transport(
        &mut self,
        descriptor: SendRuntimeDesc,
        preview_cursor_index: Option<StateIndex>,
        meta: SendMeta,
        payload: Option<lane_port::RawSendPayload>,
        plan: SendPayloadPlan<'r>,
    ) -> SendResult<SendTransportStep<'r>>
    where
        <Mint as MintConfigMarker>::Policy: crate::control::cap::mint::AllowsEndpointMint,
    {
        let scratch_ptr = {
            let port = self.port_for_lane(meta.lane as usize);
            lane_port::scratch_ptr(port)
        };
        let (staged_send, dispatch, token_bytes) = {
            let scratch = /* SAFETY: the pointer comes from pinned owner storage and this path holds the unique mutable access for the borrow. */ unsafe { &mut *scratch_ptr };
            let (staged_send, dispatch) =
                self.stage_send_payload(descriptor, plan, payload, scratch)?;
            let token_bytes = if dispatch.is_some() {
                let mut bytes = [0u8; CAP_TOKEN_LEN];
                bytes.copy_from_slice(&scratch[..CAP_TOKEN_LEN]);
                Some(bytes)
            } else {
                None
            };
            (staged_send, dispatch, token_bytes)
        };
        let commit_plan = self.build_send_commit_plan(
            preview_cursor_index,
            meta,
            staged_send.control,
            dispatch,
            token_bytes,
        )?;
        let encoded_len = staged_send.encoded_len;

        let mut pending_transport = None;
        let is_remote_send = if meta.peer == ROLE {
            false
        } else {
            let port = self.port_for_lane(meta.lane as usize);
            let payload_view = {
                let scratch = /* SAFETY: the pointer comes from pinned owner storage and this path only creates a shared borrow. */ unsafe { &*scratch_ptr };
                Payload::new(&scratch[..encoded_len])
            };
            let outgoing = crate::transport::Outgoing {
                meta: crate::transport::SendMeta {
                    eff_index: meta.eff_index,
                    logical_label: crate::transport::LogicalLabel::new(meta.label),
                    frame_label: crate::transport::FrameLabel::new(meta.frame_label),
                    peer: meta.peer,
                    lane: port.lane().as_wire(),
                    is_control: meta.is_control,
                },
                payload: payload_view,
            };

            let mut transport = lane_port::PendingSend::new();
            lane_port::begin_send_outgoing(&mut transport, outgoing);
            pending_transport = Some(transport);
            true
        };

        if is_remote_send {
            Ok(SendTransportStep::Pending(PendingSendIo {
                transport: pending_transport.ok_or(SendError::PhaseInvariant)?,
                commit_plan: Some(commit_plan),
            }))
        } else {
            Ok(SendTransportStep::Immediate(commit_plan))
        }
    }

    #[inline(never)]
    pub(crate) fn poll_send_init(
        &mut self,
        descriptor: SendRuntimeDesc,
        meta: SendMeta,
        preview_cursor_index: Option<StateIndex>,
        payload: Option<lane_port::RawSendPayload>,
    ) -> SendInitOutcome<'r>
    where
        <Mint as MintConfigMarker>::Policy: crate::control::cap::mint::AllowsEndpointMint,
    {
        if descriptor.frame_label() != crate::transport::FrameLabel::new(meta.frame_label) {
            return SendInitOutcome::Ready(Err(SendError::PhaseInvariant));
        }
        let plan = match self.prepare_send_payload_plan(meta, descriptor, payload.is_some()) {
            Ok(plan) => plan,
            Err(err) => return SendInitOutcome::Ready(Err(err)),
        };
        let step = match self.begin_send_transport(
            descriptor,
            preview_cursor_index,
            meta,
            payload,
            plan,
        ) {
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
        let port = self.port_for_lane(pending.lane_idx());
        match lane_port::poll_send_outgoing(&mut pending.transport, port, cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(())) => Poll::Ready(Ok(())),
            Poll::Ready(Err(err)) => {
                self.emit_transport_fault_event(pending.lane_idx(), port.lane().as_wire(), err);
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
            control,
            proof:
                SendCommitProof {
                    descriptor,
                    progress,
                },
        } = commit_plan;
        self.finish_send_control_outcome(control);
        self.publish_send_progress_commit_plan(progress);
        let descriptor = if descriptor.is_none() {
            super::SendDescriptorPublication::none()
        } else {
            let cluster = self
                .control
                .cluster()
                .expect("send descriptor publication requires its preparing cluster");
            super::SendDescriptorPublication::new(
                cluster.descriptor_publication_authority(),
                descriptor,
            )
        };
        SendCommitOutcome { descriptor }
    }

    #[inline(never)]
    fn emit_send_after_transport_event(&mut self, delta: &super::CommittedCommitDelta) {
        let event = delta
            .event()
            .expect("send progress delta must carry an enabled event row");
        let lane = event.lane();
        let lane_wire = self.port_for_lane(lane as usize).lane().as_wire();
        let logical_meta = TapFrameMeta::new(
            self.sid.raw(),
            lane_wire,
            ROLE,
            event.event_label(),
            FrameFlags::empty(),
        );
        let scope_trace = self.scope_trace(event.scope());
        let event_id = event.event_id(ids::ENDPOINT_SEND, ids::ENDPOINT_CONTROL);
        self.emit_endpoint_event(event_id, logical_meta, scope_trace, lane);
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
                let commit_plan = pending
                    .commit_plan
                    .take()
                    .expect("send commit proof must remain until transport completion");
                Poll::Ready(Ok(commit_plan))
            }
            Poll::Ready(Err(err)) => {
                self.rollback_send_commit_plan(pending.commit_plan.take());
                Poll::Ready(Err(err))
            }
        }
    }
}

#[cfg(all(test, hibana_repo_tests))]
#[path = "tests.rs"]
mod tests;
