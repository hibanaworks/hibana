use super::{
    CAP_HANDLE_LEN, CAP_TOKEN_LEN, CapEntry, CapHeader, CapShot, ControlDesc, ControlOp, CpError,
    CursorEndpoint, DescriptorDispatch, EndpointSlot, EpochTable, FrameFlags, GenericCapToken,
    LabelUniverse, Lane, MintConfigMarker, MintedControlToken, ParentRouteDecisionPlan, Payload,
    PendingCapRelease, PendingSendIo, PolicySlot, Poll, RendezvousId, RouteDecisionSource, ScopeId,
    SendCommitMeta, SendCommitOutcome, SendCommitPlan, SendCommitProof, SendDescriptorTerminal,
    SendError, SendInitOutcome, SendMeta, SendPayloadPlan, SendProgressCommitPlan, SendResult,
    SendRouteCommitPlan, SendRuntimeDesc, SendTransportStep, SessionId, StagedControlEmission,
    StagedSendPayload, StateIndex, TapFrameMeta, Transport, ids, lane_port, state_index_to_usize,
};
#[cfg(test)]
use super::{SendState, kernel_send};
use crate::global::const_dsl::CompactScopeId;

const SEND_ROUTE_SOURCE_NONE: u8 = 0;
const SEND_ROUTE_SOURCE_ACK: u8 = 1;
const SEND_ROUTE_SOURCE_POLL: u8 = 2;
const SEND_ROUTE_WAS_SELECTED: u8 = 1 << 2;
const SEND_ROUTE_ARM_HAS_RECV: u8 = 1 << 3;
const SEND_ROUTE_PRESENT: u8 = 1 << 4;
const SEND_ROUTE_HAS_PARENT: u8 = 1 << 5;
const SEND_ROUTE_NO_SLOT: u16 = u16::MAX;

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>
    CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: EndpointSlot,
{
    pub(crate) fn map_cp_error(err: CpError) -> SendError {
        match err {
            CpError::PolicyAbort { reason } => SendError::PolicyAbort { reason },
            _ => SendError::PhaseInvariant,
        }
    }

    #[inline(never)]
    fn build_send_route_commit_plan(&mut self, meta: SendMeta) -> SendResult<SendRouteCommitPlan> {
        let Some(selected_arm) = meta.route_arm else {
            return Ok(SendRouteCommitPlan {
                parent_scope: CompactScopeId::none(),
                route_arm_slot: SEND_ROUTE_NO_SLOT,
                offer_lane: 0,
                flags: 0,
                parent_arm: 0,
                parent_lane: 0,
            });
        };
        let scope_id = meta.scope;
        let lane_wire = meta.lane;
        let route_source = self.peek_scope_ack(scope_id).map(|token| token.source());
        let is_route_controller = self.cursor.is_route_controller(scope_id);
        let was_selected = self.selected_arm_for_scope(scope_id) == Some(selected_arm);

        let parent_route_decision_plan = if !is_route_controller {
            self.build_recvless_parent_route_decision_plan(scope_id)
        } else {
            None
        };
        let route_arm_proof = if !was_selected {
            self.preflight_route_arm_commit_after_clearing_other_lanes(
                lane_wire,
                scope_id,
                selected_arm,
            )
        } else {
            self.preflight_route_arm_commit(lane_wire, scope_id, selected_arm)
        };
        let route_arm_proof = route_arm_proof.ok_or(SendError::PhaseInvariant)?;
        let source_flag = match route_source {
            Some(RouteDecisionSource::Ack) => SEND_ROUTE_SOURCE_ACK,
            Some(RouteDecisionSource::Poll) => SEND_ROUTE_SOURCE_POLL,
            Some(RouteDecisionSource::Resolver) | None => SEND_ROUTE_SOURCE_NONE,
        };
        let mut flags = source_flag | SEND_ROUTE_PRESENT;
        if was_selected {
            flags |= SEND_ROUTE_WAS_SELECTED;
        }
        if self.arm_has_recv(scope_id, selected_arm) {
            flags |= SEND_ROUTE_ARM_HAS_RECV;
        }
        if parent_route_decision_plan.is_some() {
            flags |= SEND_ROUTE_HAS_PARENT;
        }
        let route_arm_slot = self
            .route_commit_proofs
            .stage_send_commit_proof(route_arm_proof)
            .map_err(|_| SendError::PhaseInvariant)?;
        let (parent_scope, parent_arm, parent_lane) = if let Some(plan) = parent_route_decision_plan
        {
            (
                CompactScopeId::from_scope_id(plan.scope),
                plan.arm,
                plan.lane,
            )
        } else {
            (CompactScopeId::none(), 0, 0)
        };
        Ok(SendRouteCommitPlan {
            parent_scope,
            route_arm_slot,
            offer_lane: self.offer_lane_for_scope(scope_id),
            flags,
            parent_arm,
            parent_lane,
        })
    }

    #[inline(never)]
    fn publish_send_route_commit_plan(&mut self, plan: SendRouteCommitPlan) {
        if plan.flags & SEND_ROUTE_PRESENT == 0 {
            return;
        }
        let route_arm_proof = self
            .route_commit_proofs
            .staged_send_commit_proof(plan.route_arm_slot);
        if plan.flags & SEND_ROUTE_HAS_PARENT != 0 {
            self.publish_recvless_parent_route_decision(ParentRouteDecisionPlan {
                scope: plan.parent_scope.to_scope_id(),
                arm: plan.parent_arm,
                lane: plan.parent_lane,
            });
        }
        let scope_id = route_arm_proof.scope();
        let lane_wire = route_arm_proof.lane_idx() as u8;
        let selected_arm = route_arm_proof.arm();
        let source = plan.flags & 0b11;
        match source {
            SEND_ROUTE_SOURCE_ACK if self.cursor.is_route_controller(scope_id) => {
                self.record_route_decision_for_lane(lane_wire as usize, scope_id, selected_arm);
                self.emit_route_decision(
                    scope_id,
                    selected_arm,
                    RouteDecisionSource::Ack,
                    lane_wire,
                );
            }
            SEND_ROUTE_SOURCE_POLL => {
                self.emit_route_decision(
                    scope_id,
                    selected_arm,
                    RouteDecisionSource::Poll,
                    plan.offer_lane,
                );
            }
            _ => {}
        }

        if plan.flags & SEND_ROUTE_WAS_SELECTED == 0 {
            self.clear_scope_route_state_for_other_lanes(scope_id, lane_wire);
        }
        self.skip_unselected_arm_lanes(scope_id, selected_arm, lane_wire);
        self.commit_route_arm_after_preflight(route_arm_proof);
        if plan.flags & SEND_ROUTE_ARM_HAS_RECV != 0 {
            self.consume_scope_ready_arm(scope_id, selected_arm);
        }
        self.clear_scope_evidence(scope_id);
        self.port_for_lane(lane_wire as usize).clear_route_hints();
    }

    #[inline(never)]
    fn build_send_progress_commit_plan(
        &mut self,
        preview_cursor_index: Option<StateIndex>,
        meta: SendMeta,
    ) -> SendResult<SendProgressCommitPlan> {
        let route = self.build_send_route_commit_plan(meta)?;
        let cursor_after_send = self.preflight_send_cursor_after_preview(preview_cursor_index)?;
        Ok(SendProgressCommitPlan {
            route,
            cursor_after_send,
        })
    }

    #[inline(never)]
    fn preflight_send_cursor_after_preview(
        &self,
        preview_cursor_index: Option<StateIndex>,
    ) -> SendResult<StateIndex> {
        match preview_cursor_index {
            Some(preview_cursor_index) => self
                .cursor
                .try_next_index_past_jumps_from(preview_cursor_index)
                .map_err(|_| SendError::PhaseInvariant),
            None => self
                .cursor
                .try_next_index_past_jumps()
                .map_err(|_| SendError::PhaseInvariant),
        }
    }

    #[inline(never)]
    fn publish_send_progress_commit_plan(
        &mut self,
        meta: SendCommitMeta,
        plan: SendProgressCommitPlan,
    ) {
        self.publish_send_route_commit_plan(plan.route);
        self.set_cursor_index(state_index_to_usize(plan.cursor_after_send));
        self.commit_send_progress(meta);
    }

    #[inline(never)]
    fn commit_send_progress(&mut self, meta: SendCommitMeta) {
        let lane_idx = meta.lane as usize;
        let scope = meta.scope();
        if self
            .cursor
            .current_phase_contains_eff_index(lane_idx, meta.eff_index)
        {
            self.advance_lane_cursor(lane_idx, meta.eff_index);
        } else {
            self.complete_lane_phase(lane_idx);
        }
        self.maybe_skip_remaining_route_arm(scope, meta.lane, meta.route_arm, meta.eff_index);
        self.publish_scope_settlement(scope, meta.route_arm, Some(meta.eff_index), meta.lane);
        self.maybe_advance_phase();
    }

    #[inline(never)]
    fn stage_send_payload(
        &mut self,
        meta: SendMeta,
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
                        encoded_len: data.encode_into(scratch)?,
                        control: StagedControlEmission::None,
                    },
                    None,
                ))
            }
            SendPayloadPlan::LocalControl { token } => {
                Self::stage_auto_control_request(payload, scratch)?;
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
            SendPayloadPlan::WireControlWithAutoRequest { dispatch } => {
                let data = payload.ok_or(SendError::PhaseInvariant)?;
                let encoded_len = data.encode_into(scratch)?;
                if Self::encoded_auto_control_request(encoded_len, scratch) {
                    let token = self
                        .mint_send_control(meta, descriptor)?
                        .ok_or(SendError::PhaseInvariant)?;
                    let dispatch = token.dispatch;
                    scratch[..CAP_TOKEN_LEN].copy_from_slice(&token.token_bytes);
                    Ok((
                        StagedSendPayload {
                            encoded_len: CAP_TOKEN_LEN,
                            control: StagedControlEmission::Registered(token.rollback),
                        },
                        Some(dispatch),
                    ))
                } else {
                    Self::validate_explicit_wire_control_payload(encoded_len, scratch)?;
                    Ok((
                        StagedSendPayload {
                            encoded_len,
                            control: StagedControlEmission::WireOnly,
                        },
                        Some(dispatch),
                    ))
                }
            }
            SendPayloadPlan::ExplicitWireControl { dispatch } => {
                let data = payload.ok_or(SendError::PhaseInvariant)?;
                let encoded_len = data.encode_into(scratch)?;
                Self::validate_explicit_wire_control_payload(encoded_len, scratch)?;
                Ok((
                    StagedSendPayload {
                        encoded_len,
                        control: StagedControlEmission::WireOnly,
                    },
                    Some(dispatch),
                ))
            }
            SendPayloadPlan::EmittedWireControl { token } => {
                Self::stage_auto_control_request(payload, scratch)?;
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
        }
    }

    #[inline(never)]
    fn stage_auto_control_request(
        payload: Option<lane_port::RawSendPayload>,
        scratch: &mut [u8],
    ) -> SendResult<()> {
        let Some(data) = payload else {
            return Ok(());
        };
        let encoded_len = data.encode_into(scratch)?;
        if Self::encoded_auto_control_request(encoded_len, scratch) {
            Ok(())
        } else {
            Err(SendError::PhaseInvariant)
        }
    }

    #[inline(always)]
    fn encoded_auto_control_request(encoded_len: usize, scratch: &[u8]) -> bool {
        let _ = scratch;
        encoded_len == 0
    }

    #[inline(never)]
    fn validate_explicit_wire_control_payload(
        encoded_len: usize,
        scratch: &[u8],
    ) -> SendResult<()> {
        if encoded_len != CAP_TOKEN_LEN || Self::encoded_auto_control_request(encoded_len, scratch)
        {
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
        let progress = self.build_send_progress_commit_plan(preview_cursor_index, meta)?;
        let decision = self.build_send_control_decision_plan(meta, &control, dispatch)?;
        let descriptor = self.reserve_descriptor_terminal_for_send(meta, token_bytes, dispatch)?;
        Ok(SendCommitPlan {
            control,
            proof: SendCommitProof {
                meta: SendCommitMeta::from_send_meta(meta),
                descriptor,
                progress,
                decision,
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
                        if control.auto_mint_wire() {
                            Ok(SendPayloadPlan::WireControlWithAutoRequest {
                                dispatch: DescriptorDispatch::new(
                                    control,
                                    meta.scope,
                                    self.descriptor_send_epoch(control, lane)?,
                                ),
                            })
                        } else {
                            Ok(SendPayloadPlan::ExplicitWireControl {
                                dispatch: DescriptorDispatch::new(
                                    control,
                                    meta.scope,
                                    self.descriptor_send_epoch(control, lane)?,
                                ),
                            })
                        }
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
        preview_cursor_index: Option<StateIndex>,
        descriptor: SendRuntimeDesc,
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
                self.stage_send_payload(meta, descriptor, plan, payload, scratch)?;
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
                lane_idx: meta.lane as usize,
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
            preview_cursor_index,
            descriptor,
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
        lane_port::poll_send_outgoing(&mut pending.transport, port, cx)
            .map_err(SendError::Transport)
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
                    meta,
                    descriptor,
                    progress,
                    decision,
                },
        } = commit_plan;
        self.finish_send_control_outcome(control);
        self.publish_send_control_decision_plan(decision);
        self.publish_send_progress_commit_plan(meta, progress);
        self.emit_send_after_transport_event(meta);
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
    fn emit_send_after_transport_event(&mut self, meta: SendCommitMeta) {
        let lane_wire = self.port_for_lane(meta.lane as usize).lane().as_wire();
        let logical_meta = TapFrameMeta::new(
            self.sid.raw(),
            lane_wire,
            ROLE,
            meta.label,
            FrameFlags::empty(),
        );
        let scope_trace = self.scope_trace(meta.scope());
        let event_id = if meta.is_control {
            ids::ENDPOINT_CONTROL
        } else {
            ids::ENDPOINT_SEND
        };
        self.emit_endpoint_event(event_id, logical_meta, scope_trace, meta.lane);
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

    #[inline(never)]
    #[cfg(test)]
    pub(crate) fn poll_send_state(
        &mut self,
        state: &mut SendState<'r>,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<SendResult<SendCommitOutcome<'r>>>
    where
        <Mint as MintConfigMarker>::Policy: crate::control::cap::mint::AllowsEndpointMint,
    {
        kernel_send(self, state, cx)
    }
}

#[cfg(all(test, hibana_repo_tests))]
#[path = "tests.rs"]
mod tests;
