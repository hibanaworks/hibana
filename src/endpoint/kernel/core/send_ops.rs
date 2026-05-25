use super::*;

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>
    CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: BindingSlot,
{
    pub(crate) fn map_cp_error(err: CpError) -> SendError {
        match err {
            CpError::PolicyAbort { reason } => SendError::PolicyAbort { reason },
            _ => SendError::PhaseInvariant,
        }
    }

    #[inline(never)]
    fn commit_send_after_emit(
        &mut self,
        preview_cursor_index: Option<StateIndex>,
        meta: SendMeta,
    ) -> SendResult<()> {
        self.commit_send_preview(preview_cursor_index, meta)?;
        self.commit_send_progress(meta);
        Ok(())
    }

    #[inline(never)]
    fn commit_send_route_selection(&mut self, meta: SendMeta) -> SendResult<()> {
        let Some(selected_arm) = meta.route_arm else {
            return Ok(());
        };
        let scope_id = meta.scope;
        let lane_wire = meta.lane;
        let route_source = self.peek_scope_ack(scope_id).map(|token| token.source());
        let is_route_controller = self.cursor.is_route_controller(scope_id);

        let parent_route_decision_plan = if !is_route_controller {
            self.build_recvless_parent_route_decision_plan(scope_id)
        } else {
            None
        };
        let route_arm_proof = if self.selected_arm_for_scope(scope_id) != Some(selected_arm) {
            self.preflight_route_arm_commit_after_clearing_other_lanes(
                lane_wire,
                scope_id,
                selected_arm,
            )
        } else {
            self.preflight_route_arm_commit(lane_wire, scope_id, selected_arm)
        };
        let route_arm_proof = route_arm_proof.ok_or(SendError::PhaseInvariant)?;

        if let Some(plan) = parent_route_decision_plan {
            self.publish_recvless_parent_route_decision(plan);
        }
        match route_source {
            Some(RouteDecisionSource::Ack) if is_route_controller => {
                self.record_route_decision_for_lane(lane_wire as usize, scope_id, selected_arm);
                self.emit_route_decision(
                    scope_id,
                    selected_arm,
                    RouteDecisionSource::Ack,
                    lane_wire,
                );
            }
            Some(RouteDecisionSource::Poll) => {
                self.emit_route_decision(
                    scope_id,
                    selected_arm,
                    RouteDecisionSource::Poll,
                    self.offer_lane_for_scope(scope_id),
                );
            }
            _ => {}
        }

        if self.selected_arm_for_scope(scope_id) != Some(selected_arm) {
            self.clear_scope_route_state_for_other_lanes(scope_id, lane_wire);
        }
        self.skip_unselected_arm_lanes(scope_id, selected_arm, lane_wire);
        self.commit_route_arm_after_preflight(route_arm_proof);
        if self.arm_has_recv(scope_id, selected_arm) {
            self.consume_scope_ready_arm(scope_id, selected_arm);
        }
        self.clear_scope_evidence(scope_id);
        self.port_for_lane(lane_wire as usize).clear_route_hints();
        Ok(())
    }

    #[inline(never)]
    fn commit_send_preview(
        &mut self,
        preview_cursor_index: Option<StateIndex>,
        meta: SendMeta,
    ) -> SendResult<()> {
        self.commit_send_route_selection(meta)?;
        if let Some(preview_cursor_index) = preview_cursor_index {
            self.set_cursor_index(state_index_to_usize(preview_cursor_index));
        }
        self.advance_cursor_after_send()
    }

    #[inline(never)]
    fn advance_cursor_after_send(&mut self) -> SendResult<()> {
        self.cursor
            .try_advance_past_jumps_in_place()
            .map_err(|_| SendError::PhaseInvariant)
    }

    #[inline(never)]
    fn commit_send_progress(&mut self, meta: SendMeta) {
        let lane_idx = meta.lane as usize;
        if self
            .cursor
            .current_phase_contains_eff_index(lane_idx, meta.eff_index)
        {
            self.advance_lane_cursor(lane_idx, meta.eff_index);
        } else {
            self.complete_lane_phase(lane_idx);
        }
        self.maybe_skip_remaining_route_arm(meta.scope, meta.lane, meta.route_arm, meta.eff_index);
        self.publish_scope_settlement(meta.scope, meta.route_arm, Some(meta.eff_index), meta.lane);
        self.maybe_advance_phase();
    }

    #[inline(never)]
    fn stage_send_payload(
        plan: SendPayloadPlan,
        payload: Option<lane_port::RawSendPayload>,
        scratch: &mut [u8],
    ) -> SendResult<(StagedSendPayload, Option<DescriptorDispatch>)> {
        match plan {
            SendPayloadPlan::Data => {
                let data = payload.ok_or(SendError::PhaseInvariant)?;
                Ok((
                    StagedSendPayload {
                        encoded_len: data.encode_into(scratch)?,
                        control: StagedControlEmission::None,
                    },
                    None,
                ))
            }
            SendPayloadPlan::LocalControl { token } => {
                if payload.is_some() {
                    return Err(SendError::PhaseInvariant);
                }
                let dispatch = token.dispatch;
                let bytes = token.token.bytes();
                scratch[..CAP_TOKEN_LEN].copy_from_slice(&bytes);
                Ok((
                    StagedSendPayload {
                        encoded_len: CAP_TOKEN_LEN,
                        control: StagedControlEmission::Registered(StagedDispatchToken {
                            token: token.token,
                            rollback: token.rollback,
                        }),
                    },
                    Some(dispatch),
                ))
            }
            SendPayloadPlan::ExplicitWireControl { dispatch } => {
                let data = payload.ok_or(SendError::PhaseInvariant)?;
                let encoded_len = data.encode_into(scratch)?;
                if encoded_len != CAP_TOKEN_LEN {
                    return Err(SendError::PhaseInvariant);
                }
                let mut bytes = [0u8; CAP_TOKEN_LEN];
                bytes.copy_from_slice(&scratch[..CAP_TOKEN_LEN]);
                let token = GenericCapToken::<()>::from_bytes(bytes);
                if matches!(
                    token
                        .control_header()
                        .map_err(|_| SendError::PhaseInvariant)?
                        .shot(),
                    CapShot::One
                ) {
                    return Err(SendError::PhaseInvariant);
                }
                Ok((
                    StagedSendPayload {
                        encoded_len,
                        control: StagedControlEmission::Emitted {
                            dispatch_token: StagedDispatchToken {
                                token: RawEmittedCapToken::new(bytes),
                                rollback: PendingCapRelease::inert(),
                            },
                            return_emitted: true,
                        },
                    },
                    Some(dispatch),
                ))
            }
            SendPayloadPlan::EmittedWireControl { token } => {
                if payload.is_some() {
                    return Err(SendError::PhaseInvariant);
                }
                let dispatch = token.dispatch;
                let bytes = token.token.bytes();
                scratch[..CAP_TOKEN_LEN].copy_from_slice(&bytes);
                Ok((
                    StagedSendPayload {
                        encoded_len: CAP_TOKEN_LEN,
                        control: StagedControlEmission::Emitted {
                            dispatch_token: StagedDispatchToken {
                                token: token.token,
                                rollback: token.rollback,
                            },
                            return_emitted: false,
                        },
                    },
                    Some(dispatch),
                ))
            }
        }
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
    ) -> SendResult<MintedControlToken>
    where
        <Mint as MintConfigMarker>::Policy: crate::control::cap::mint::AllowsEndpointMint,
    {
        let cluster = self.control.cluster().ok_or(SendError::PhaseInvariant)?;
        let rendezvous = cluster
            .get_local(&self.rendezvous_id())
            .ok_or(SendError::PhaseInvariant)?;
        let strategy = self.mint.as_config().strategy();
        let nonce = strategy.derive_nonce(rendezvous.next_nonce_seed());
        let rollback = PendingCapRelease::new(nonce, rendezvous.cap_release_ctx(lane));
        rendezvous
            .caps()
            .insert_entry(CapEntry {
                sid: self.sid,
                lane_raw: lane.as_wire(),
                kind_tag: control.resource_tag(),
                shot_state: shot.as_u8(),
                role: peer,
                mint_revision: rendezvous.next_cap_revision(),
                consumed_revision: 0,
                released_revision: 0,
                nonce,
                handle: handle_bytes,
            })
            .map_err(|_| SendError::PhaseInvariant)?;

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
            token: RawEmittedCapToken::new(token_bytes),
            dispatch: DescriptorDispatch::new(control, scope, epoch),
            rollback,
        })
    }

    #[inline(never)]
    fn mint_send_control(
        &mut self,
        meta: SendMeta,
        descriptor: SendRuntimeDesc,
    ) -> SendResult<Option<MintedControlToken>>
    where
        <Mint as MintConfigMarker>::Policy: crate::control::cap::mint::AllowsEndpointMint,
    {
        let Some(control) = descriptor.control() else {
            return Ok(None);
        };
        if matches!(control.path(), crate::control::cap::mint::ControlPath::Wire)
            && !control.auto_mint_wire()
        {
            return Err(SendError::PhaseInvariant);
        }

        let lane = self.port_for_lane(meta.lane as usize).lane();
        let shot = meta.shot.ok_or(SendError::PhaseInvariant)?;
        let minted = match control.op() {
            ControlOp::LoopContinue => self.mint_local_loop_continue_control(&meta, shot, lane)?,
            ControlOp::LoopBreak => self.mint_local_loop_break_control(&meta, shot, lane)?,
            ControlOp::CapDelegate => {
                let cp_lane = Lane::new(lane.raw());
                let src_rv = RendezvousId::new(self.rendezvous_id().raw());
                self.mint_local_reroute_control(&meta, shot, lane, src_rv, cp_lane, control)?
            }
            ControlOp::RouteDecision => {
                let cp_lane = Lane::new(lane.raw());
                let src_rv = RendezvousId::new(self.rendezvous_id().raw());
                self.mint_local_route_decision_control(&meta, shot, lane, src_rv, cp_lane, control)?
            }
            ControlOp::TopologyBegin => {
                let cp_sid = SessionId::new(self.sid.raw());
                let cp_lane = Lane::new(lane.raw());
                let src_rv = RendezvousId::new(self.rendezvous_id().raw());
                let encode_control_handle = descriptor
                    .encode_control_handle()
                    .ok_or(SendError::PhaseInvariant)?;
                self.mint_local_topology_begin_control(
                    &meta,
                    shot,
                    lane,
                    src_rv,
                    cp_lane,
                    control,
                    encode_control_handle(cp_sid, cp_lane, meta.scope),
                )?
            }
            ControlOp::TopologyAck => {
                let cp_sid = SessionId::new(self.sid.raw());
                let encode_control_handle = descriptor
                    .encode_control_handle()
                    .ok_or(SendError::PhaseInvariant)?;
                self.mint_local_topology_ack_control(
                    &meta,
                    shot,
                    lane,
                    cp_sid,
                    control,
                    encode_control_handle(cp_sid, lane, meta.scope),
                )?
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
                    encode_control_handle(self.sid, lane, meta.scope),
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
    fn dispatch_send_token(
        &self,
        dispatch: Option<DescriptorDispatch>,
        mut token: StagedDispatchToken,
    ) -> SendResult<DispatchSendTokenResult<'r>> {
        let Some(dispatch) = dispatch else {
            return Ok(DispatchSendTokenResult::None);
        };
        let cluster = self.control.cluster().ok_or(SendError::PhaseInvariant)?;
        cluster
            .dispatch_descriptor_control_frame(
                self.rendezvous_id(),
                token.bytes(),
                dispatch.desc,
                dispatch.scope_id,
                dispatch.epoch,
                None,
            )
            .map_err(|_| SendError::PhaseInvariant)?;

        match token.rollback.take_registered_token(token.bytes()) {
            Some(token) => Ok(DispatchSendTokenResult::Registered(token)),
            None => Ok(DispatchSendTokenResult::Emitted),
        }
    }

    #[inline(never)]
    fn preflight_send_control_dispatch(
        &self,
        meta: SendMeta,
        emission: &SendTransportEmission,
    ) -> SendResult<()> {
        let (Some(dispatch), Some(bytes)) =
            (emission.dispatch, emission.control.dispatch_token_bytes())
        else {
            return Ok(());
        };
        let cluster = self.control.cluster().ok_or(SendError::PhaseInvariant)?;
        cluster
            .validate_send_bound_descriptor_control_frame(
                self.rendezvous_id(),
                bytes,
                dispatch.desc,
                self.sid,
                Lane::new(meta.lane as u32),
                meta.peer,
                dispatch.scope_id,
                dispatch.epoch,
            )
            .map_err(|_| SendError::PhaseInvariant)
    }

    #[inline(never)]
    fn prepare_send_payload_plan(
        &mut self,
        meta: SendMeta,
        descriptor: SendRuntimeDesc,
        has_payload: bool,
    ) -> SendResult<SendPayloadPlan>
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
                    if has_payload {
                        return Err(SendError::PhaseInvariant);
                    }
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
                        let token = self
                            .mint_send_control(meta, descriptor)?
                            .ok_or(SendError::PhaseInvariant)?;
                        Ok(SendPayloadPlan::EmittedWireControl { token })
                    }
                }
            },
        }
    }

    #[inline(never)]
    fn begin_send_transport(
        &mut self,
        meta: SendMeta,
        payload: Option<lane_port::RawSendPayload>,
        plan: SendPayloadPlan,
    ) -> SendResult<SendTransportStep<'r>> {
        let scratch_ptr = {
            let port = self.port_for_lane(meta.lane as usize);
            lane_port::scratch_ptr(port)
        };
        let (staged_send, dispatch) = {
            let scratch = /* SAFETY: the pointer comes from pinned owner storage and this path holds the unique mutable access for the borrow. */ unsafe { &mut *scratch_ptr };
            Self::stage_send_payload(plan, payload, scratch)?
        };
        if let (Some(dispatch), Some(bytes)) =
            (dispatch, staged_send.control.dispatch_token_bytes())
        {
            let cluster = self.control.cluster().ok_or(SendError::PhaseInvariant)?;
            cluster
                .validate_send_bound_descriptor_control_frame(
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
        }
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
                    direction: if meta.peer == ROLE {
                        crate::transport::LocalDirection::Local
                    } else {
                        crate::transport::LocalDirection::Send
                    },
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
                lane_idx: meta.lane as usize,
                control: Some(staged_send.control),
                dispatch,
            }))
        } else {
            Ok(SendTransportStep::Immediate(SendTransportEmission {
                control: staged_send.control,
                dispatch,
            }))
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
        let step = match self.begin_send_transport(meta, payload, plan) {
            Ok(step) => step,
            Err(err) => return SendInitOutcome::Ready(Err(err)),
        };
        match step {
            SendTransportStep::Immediate(emission) => SendInitOutcome::Commit {
                meta,
                preview_cursor_index,
                emission,
            },
            SendTransportStep::Pending(pending) => SendInitOutcome::Pending {
                meta,
                preview_cursor_index,
                pending,
            },
        }
    }

    #[inline(never)]
    fn poll_send_transport(
        &mut self,
        pending: &mut PendingSendIo<'r>,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<SendResult<()>> {
        let port = self.port_for_lane(pending.lane_idx);
        lane_port::poll_send_outgoing(&mut pending.transport, port, cx)
            .map_err(SendError::Transport)
    }

    #[inline(never)]
    pub(crate) fn finish_send_after_transport_runtime(
        &mut self,
        meta: SendMeta,
        preview_cursor_index: Option<StateIndex>,
        emission: SendTransportEmission,
    ) -> SendResult<SendControlOutcome<'r>> {
        self.preflight_send_control_dispatch(meta, &emission)?;
        self.commit_send_after_emit(preview_cursor_index, meta)?;
        self.emit_send_after_transport_event(meta);
        self.resolve_send_control_outcome(emission)
    }

    #[inline(never)]
    fn emit_send_after_transport_event(&mut self, meta: SendMeta) {
        let lane_wire = self.port_for_lane(meta.lane as usize).lane().as_wire();
        let logical_meta = TapFrameMeta::new(
            self.sid.raw(),
            lane_wire,
            ROLE,
            meta.label,
            FrameFlags::empty(),
        );
        let scope_trace = self.scope_trace(meta.scope);
        let event_id = if meta.is_control {
            ids::ENDPOINT_CONTROL
        } else {
            ids::ENDPOINT_SEND
        };
        self.emit_endpoint_event(event_id, logical_meta, scope_trace, meta.lane);
    }

    #[inline(never)]
    fn resolve_send_control_outcome(
        &mut self,
        emission: SendTransportEmission,
    ) -> SendResult<SendControlOutcome<'r>> {
        match emission.control {
            StagedControlEmission::None => Ok(SendControlOutcome::None),
            StagedControlEmission::Registered(token) => {
                self.resolve_registered_send_control_outcome(emission.dispatch, token)
            }
            StagedControlEmission::Emitted {
                dispatch_token,
                return_emitted,
            } => self.resolve_emitted_send_control_outcome(
                emission.dispatch,
                dispatch_token,
                return_emitted,
            ),
        }
    }

    #[inline(never)]
    fn resolve_registered_send_control_outcome(
        &self,
        dispatch: Option<DescriptorDispatch>,
        token: StagedDispatchToken,
    ) -> SendResult<SendControlOutcome<'r>> {
        match self.dispatch_send_token(dispatch, token)? {
            DispatchSendTokenResult::Registered(token) => Ok(SendControlOutcome::Registered(token)),
            DispatchSendTokenResult::None | DispatchSendTokenResult::Emitted => {
                Err(SendError::PhaseInvariant)
            }
        }
    }

    #[inline(never)]
    fn resolve_emitted_send_control_outcome(
        &self,
        dispatch: Option<DescriptorDispatch>,
        dispatch_token: StagedDispatchToken,
        return_emitted: bool,
    ) -> SendResult<SendControlOutcome<'r>> {
        let emitted = dispatch_token.token;
        match self.dispatch_send_token(dispatch, dispatch_token)? {
            DispatchSendTokenResult::Registered(token) => {
                if return_emitted {
                    drop(token);
                    Ok(SendControlOutcome::Emitted(emitted))
                } else {
                    Ok(SendControlOutcome::Registered(token))
                }
            }
            DispatchSendTokenResult::Emitted => {
                if return_emitted {
                    Ok(SendControlOutcome::Emitted(emitted))
                } else {
                    Err(SendError::PhaseInvariant)
                }
            }
            DispatchSendTokenResult::None => Err(SendError::PhaseInvariant),
        }
    }

    #[inline(never)]
    pub(crate) fn poll_send_pending(
        &mut self,
        pending: &mut PendingSendIo<'r>,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<SendResult<SendTransportEmission>> {
        match self.poll_send_transport(pending, cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(())) => {
                let emission = SendTransportEmission {
                    control: pending
                        .control
                        .take()
                        .expect("send transport control must remain until completion"),
                    dispatch: pending.dispatch,
                };
                Poll::Ready(Ok(emission))
            }
            Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
        }
    }

    #[inline(never)]
    #[cfg(test)]
    pub(crate) fn poll_send_state(
        &mut self,
        state: &mut SendState<'r>,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<SendResult<SendControlOutcome<'r>>>
    where
        <Mint as MintConfigMarker>::Policy: crate::control::cap::mint::AllowsEndpointMint,
    {
        kernel_send(self, state, cx)
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
