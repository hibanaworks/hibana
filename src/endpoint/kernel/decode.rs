//! Decode-path helpers for `RouteBranch`.

use core::task::Poll;

use super::{
    core::{CursorEndpoint, MaterializedRouteBranch, RouteBranch},
    inbox::PackedIncomingClassification,
    lane_port,
};
use crate::{
    binding::BindingSlot,
    control::cap::mint::{EpochTable, MintConfigMarker},
    endpoint::{RecvError, RecvResult},
    global::{
        const_dsl::ScopeKind,
        typestate::{ARM_SHARED, JumpReason, LoopMetadata, LoopRole},
    },
    runtime::{config::Clock, consts::LabelUniverse},
    transport::{
        Transport,
        wire::{CodecError, Payload},
    },
};

#[derive(Clone, Copy)]
pub(crate) struct DecodeDesc {
    label: u8,
    expects_control: bool,
    validate_payload: for<'a> fn(Payload<'a>) -> Result<(), CodecError>,
    synthetic_payload: for<'a> fn(&'a mut [u8]) -> Result<Payload<'a>, CodecError>,
}

impl DecodeDesc {
    #[inline]
    pub(crate) const fn new(
        label: u8,
        expects_control: bool,
        validate_payload: for<'a> fn(Payload<'a>) -> Result<(), CodecError>,
        synthetic_payload: for<'a> fn(&'a mut [u8]) -> Result<Payload<'a>, CodecError>,
    ) -> Self {
        Self {
            label,
            expects_control,
            validate_payload,
            synthetic_payload,
        }
    }
}

pub(crate) struct DecodeState<'r> {
    pub(crate) branch: Option<MaterializedRouteBranch<'r>>,
    prepared_meta: Option<crate::global::typestate::RecvMeta>,
    pending_recv: lane_port::PendingRecv,
    pub(crate) restore_on_drop: bool,
}

impl<'r> DecodeState<'r> {
    #[inline]
    pub(crate) const fn empty() -> Self {
        Self {
            branch: None,
            prepared_meta: None,
            pending_recv: lane_port::PendingRecv::new(),
            restore_on_drop: false,
        }
    }

    #[inline]
    pub(crate) fn new(branch: MaterializedRouteBranch<'r>) -> Self {
        Self {
            branch: Some(branch),
            prepared_meta: None,
            pending_recv: lane_port::PendingRecv::new(),
            restore_on_drop: true,
        }
    }
}

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>
    CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: BindingSlot + 'r,
{
    pub(crate) fn poll_decode_state<'a>(
        &mut self,
        desc: DecodeDesc,
        state: &mut DecodeState<'r>,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<Payload<'a>>> {
        let branch = match state.branch.as_mut() {
            Some(branch) => branch,
            None => return Poll::Ready(Err(decode_phase_invariant())),
        };
        if state.prepared_meta.is_none() {
            state.prepared_meta = match self.prepare_decode_transport_wait(branch, desc) {
                Ok(meta) => meta,
                Err(err) => return Poll::Ready(Err(err)),
            };
        }
        if let Some(meta) = state.prepared_meta
            && branch.staged_payload.is_none()
            && !branch.binding_classification.is_present()
        {
            let port = self.port_for_lane(meta.lane as usize);
            let payload = match lane_port::poll_recv(&mut state.pending_recv, port, cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Ok(payload)) => payload,
                Poll::Ready(Err(err)) => {
                    state.prepared_meta = None;
                    return Poll::Ready(Err(RecvError::Transport(err)));
                }
            };
            branch.staged_payload = Some(super::core::StagedPayload::Transport {
                lane: meta.lane,
                payload,
            });
        }
        match self.finish_route_branch_decode(desc, state.prepared_meta, branch) {
            Ok(payload) => {
                let _ = state.branch.take();
                state.restore_on_drop = false;
                Poll::Ready(Ok(lane_port::shrink_payload(payload)))
            }
            Err(err) => Poll::Ready(Err(err)),
        }
    }

    fn prepare_decode_transport_wait(
        &mut self,
        branch: &MaterializedRouteBranch<'r>,
        desc: DecodeDesc,
    ) -> RecvResult<Option<crate::global::typestate::RecvMeta>> {
        let expected = desc.label;
        if branch.label != expected {
            return Err(RecvError::LabelMismatch {
                expected,
                actual: branch.label,
            });
        }
        if !matches!(branch.branch_meta.kind, super::offer::BranchKind::WireRecv)
            || branch.binding_classification.is_present()
            || branch.staged_payload.is_some()
        {
            return Ok(None);
        }
        let meta = self
            .cursor
            .try_recv_meta()
            .ok_or_else(decode_phase_invariant)?;
        if meta.is_control != desc.expects_control {
            return Err(decode_phase_invariant());
        }
        if self.control_semantic_kind(meta.semantic).is_loop()
            && let Some(LoopMetadata {
                scope: scope_id,
                controller,
                target,
                role,
                ..
            }) = self.cursor.loop_metadata_inner()
        {
            if role != LoopRole::Target || target != ROLE {
                return Err(decode_phase_invariant());
            }

            if meta.peer != controller {
                return Err(RecvError::PeerMismatch {
                    expected: controller,
                    actual: meta.peer,
                });
            }

            let idx = CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint>::loop_index(scope_id)
                .ok_or_else(decode_phase_invariant)?;
            let port = self.port_for_lane(meta.lane as usize);
            let lane = port.lane();
            port.loop_table().acknowledge(lane, ROLE, idx);
            let has_local_decision = port.loop_table().has_decision(lane, idx);
            if has_local_decision {
                port.ack_loop_decision(idx, ROLE);
            }
        }
        Ok(Some(meta))
    }

    fn synthetic_branch_payload(
        &mut self,
        lane_idx: u8,
        desc: DecodeDesc,
    ) -> RecvResult<Payload<'r>> {
        let scratch_ptr = {
            let port = self.port_for_lane(lane_idx as usize);
            lane_port::scratch_ptr(port)
        };
        let payload = {
            let scratch = unsafe { &mut *scratch_ptr };
            (desc.synthetic_payload)(scratch).map_err(RecvError::Codec)?
        };
        Ok(lane_port::shrink_payload(payload))
    }

    fn finish_route_branch_decode(
        &mut self,
        desc: DecodeDesc,
        prepared_meta: Option<crate::global::typestate::RecvMeta>,
        branch: &mut MaterializedRouteBranch<'r>,
    ) -> RecvResult<Payload<'r>> {
        let label = branch.label;
        let binding_classification = branch.binding_classification.into_option();
        let branch_meta = branch.branch_meta;

        let expected = desc.label;
        if label != expected {
            return Err(RecvError::LabelMismatch {
                expected,
                actual: label,
            });
        }

        match branch_meta.kind {
            super::offer::BranchKind::LocalControl => {
                let payload = self.synthetic_branch_payload(branch_meta.lane_wire, desc)?;
                (desc.validate_payload)(payload).map_err(RecvError::Codec)?;
                let typed_branch: RouteBranch<'r, ROLE, T, U, C, E, MAX_RV, Mint, B> =
                    (*branch).into();
                self.apply_branch_recv_policy(&typed_branch)?;
                let _ = self.commit_branch_preview(&typed_branch)?;

                let route_arm = Some(branch_meta.selected_arm);
                let lane_idx = branch_meta.lane_wire as usize;
                let progress_eff = if let Some(eff) = self.cursor.scope_lane_last_eff_for_arm(
                    branch_meta.scope_id,
                    branch_meta.selected_arm,
                    branch_meta.lane_wire,
                ) {
                    eff
                } else if let Some(eff) = self
                    .cursor
                    .scope_lane_last_eff(branch_meta.scope_id, branch_meta.lane_wire)
                {
                    eff
                } else {
                    branch_meta.eff_index
                };
                self.advance_lane_cursor(lane_idx, progress_eff);
                if branch_meta.selected_arm > 0
                    && self
                        .cursor
                        .scope_region_by_id(branch_meta.scope_id)
                        .map(|region| region.linger)
                        .unwrap_or(false)
                    && let Some(scope_last_eff) = self
                        .cursor
                        .scope_lane_last_eff(branch_meta.scope_id, branch_meta.lane_wire)
                {
                    self.advance_lane_cursor(lane_idx, scope_last_eff);
                }
                if !self.align_cursor_to_lane_progress(lane_idx) {
                    self.cursor
                        .try_advance_past_jumps_in_place()
                        .map_err(|_| RecvError::PhaseInvariant)?;
                }
                self.maybe_skip_remaining_route_arm(
                    branch_meta.scope_id,
                    branch_meta.lane_wire,
                    Some(branch_meta.selected_arm),
                    progress_eff,
                );
                self.settle_scope_after_action(
                    branch_meta.scope_id,
                    route_arm,
                    None,
                    branch_meta.lane_wire,
                );
                self.maybe_advance_phase();

                return Ok(payload);
            }

            super::offer::BranchKind::EmptyArmTerminal => {
                let payload = self.synthetic_branch_payload(branch_meta.lane_wire, desc)?;
                (desc.validate_payload)(payload).map_err(RecvError::Codec)?;
                let typed_branch: RouteBranch<'r, ROLE, T, U, C, E, MAX_RV, Mint, B> =
                    (*branch).into();
                self.apply_branch_recv_policy(&typed_branch)?;
                let _ = self.commit_branch_preview(&typed_branch)?;

                let route_arm = Some(branch_meta.selected_arm);

                self.cursor
                    .try_follow_jumps_in_place()
                    .map_err(|_| RecvError::PhaseInvariant)?;

                let lane_idx = branch_meta.lane_wire as usize;
                if let Some(eff_index) = self
                    .cursor
                    .scope_lane_last_eff(branch_meta.scope_id, branch_meta.lane_wire)
                {
                    self.advance_lane_cursor(lane_idx, eff_index);
                } else {
                    self.advance_lane_cursor(lane_idx, branch_meta.eff_index);
                }
                self.settle_scope_after_action(
                    branch_meta.scope_id,
                    route_arm,
                    None,
                    branch_meta.lane_wire,
                );
                self.maybe_advance_phase();

                return Ok(payload);
            }

            super::offer::BranchKind::ArmSendHint => {
                let payload = self.synthetic_branch_payload(branch_meta.lane_wire, desc)?;
                (desc.validate_payload)(payload).map_err(RecvError::Codec)?;
                let typed_branch: RouteBranch<'r, ROLE, T, U, C, E, MAX_RV, Mint, B> =
                    (*branch).into();
                self.apply_branch_recv_policy(&typed_branch)?;
                let _ = self.commit_branch_preview(&typed_branch)?;

                let route_arm = Some(branch_meta.selected_arm);
                let lane_idx = branch_meta.lane_wire as usize;
                let progress_eff = if let Some(eff) = self.cursor.scope_lane_last_eff_for_arm(
                    branch_meta.scope_id,
                    branch_meta.selected_arm,
                    branch_meta.lane_wire,
                ) {
                    eff
                } else if let Some(eff) = self
                    .cursor
                    .scope_lane_last_eff(branch_meta.scope_id, branch_meta.lane_wire)
                {
                    eff
                } else {
                    branch_meta.eff_index
                };
                self.advance_lane_cursor(lane_idx, progress_eff);
                if branch_meta.selected_arm > 0
                    && self
                        .cursor
                        .scope_region_by_id(branch_meta.scope_id)
                        .map(|region| region.linger)
                        .unwrap_or(false)
                    && let Some(scope_last_eff) = self
                        .cursor
                        .scope_lane_last_eff(branch_meta.scope_id, branch_meta.lane_wire)
                {
                    self.advance_lane_cursor(lane_idx, scope_last_eff);
                }
                if !self.align_cursor_to_lane_progress(lane_idx) {
                    self.cursor
                        .try_advance_past_jumps_in_place()
                        .map_err(|_| RecvError::PhaseInvariant)?;
                }
                self.maybe_skip_remaining_route_arm(
                    branch_meta.scope_id,
                    branch_meta.lane_wire,
                    Some(branch_meta.selected_arm),
                    progress_eff,
                );
                self.settle_scope_after_action(
                    branch_meta.scope_id,
                    route_arm,
                    None,
                    branch_meta.lane_wire,
                );
                self.maybe_advance_phase();

                return Ok(payload);
            }

            super::offer::BranchKind::WireRecv => {}
        }

        let meta = if let Some(meta) = prepared_meta {
            meta
        } else if let Some(meta) = self.cursor.try_recv_meta() {
            meta
        } else {
            return Err(decode_phase_invariant());
        };
        if meta.is_control != desc.expects_control {
            return Err(decode_phase_invariant());
        }

        if prepared_meta.is_none() && self.control_semantic_kind(meta.semantic).is_loop() {
            if let Some(LoopMetadata {
                scope: scope_id,
                controller,
                target,
                role,
                ..
            }) = self.cursor.loop_metadata_inner()
            {
                if role != LoopRole::Target || target != ROLE {
                    return Err(decode_phase_invariant());
                }

                if meta.peer != controller {
                    return Err(RecvError::PeerMismatch {
                        expected: controller,
                        actual: meta.peer,
                    });
                }

                let idx = CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint>::loop_index(scope_id)
                    .ok_or_else(decode_phase_invariant)?;
                let port = self.port_for_lane(meta.lane as usize);
                let lane = port.lane();
                port.loop_table().acknowledge(lane, ROLE, idx);
                let has_local_decision = port.loop_table().has_decision(lane, idx);
                if has_local_decision {
                    port.ack_loop_decision(idx, ROLE);
                }
            }
        }

        let mut staged_payload = branch.staged_payload;
        if staged_payload.is_none()
            && let Some(classification) = binding_classification
        {
            let primary_lane = self.primary_lane;
            let scratch_ptr = {
                let port = self.ports[primary_lane]
                    .as_ref()
                    .ok_or_else(decode_phase_invariant)?;
                lane_port::scratch_ptr(port)
            };
            let payload = lane_port::recv_from_binding(
                core::ptr::from_mut(&mut self.binding),
                classification.channel,
                scratch_ptr,
            )
            .map_err(|_| decode_phase_invariant())?;
            staged_payload = Some(super::core::StagedPayload::Binding {
                lane: primary_lane as u8,
                payload,
            });
        } else if staged_payload.is_none() {
            return Err(decode_phase_invariant());
        }

        let staged_payload = staged_payload.ok_or_else(decode_phase_invariant)?;
        let payload = staged_payload.payload();
        if let Err(err) = (desc.validate_payload)(lane_port::shrink_payload(payload)) {
            branch.binding_classification =
                PackedIncomingClassification::from_option(binding_classification);
            branch.staged_payload = Some(staged_payload);
            return Err(RecvError::Codec(err));
        }

        let typed_branch: RouteBranch<'r, ROLE, T, U, C, E, MAX_RV, Mint, B> = (*branch).into();
        if let Err(err) = self.apply_branch_recv_policy(&typed_branch) {
            branch.binding_classification =
                PackedIncomingClassification::from_option(binding_classification);
            branch.staged_payload = Some(staged_payload);
            return Err(err);
        }

        let meta = match self.commit_branch_preview(&typed_branch) {
            Ok(Some(meta)) => meta,
            Ok(None) => return Err(decode_phase_invariant()),
            Err(err) => return Err(err),
        };

        if self.cursor.try_advance_past_jumps_in_place().is_err() {
            return Err(decode_phase_invariant());
        }

        let decode_lane_idx = meta.lane as usize;
        self.advance_lane_cursor(decode_lane_idx, meta.eff_index);
        self.maybe_skip_remaining_route_arm(meta.scope, meta.lane, meta.route_arm, meta.eff_index);
        self.settle_scope_after_action(meta.scope, meta.route_arm, Some(meta.eff_index), meta.lane);
        if branch_meta.scope_id != meta.scope {
            self.settle_scope_after_action(
                branch_meta.scope_id,
                Some(branch_meta.selected_arm),
                Some(meta.eff_index),
                branch_meta.lane_wire,
            );
        }
        let mut linger_scope = meta.scope;
        loop {
            if self.is_linger_route(linger_scope) {
                let mut arm = self.route_arm_for(meta.lane, linger_scope);
                if arm.is_none() {
                    arm = self
                        .cursor
                        .first_recv_target_evidence(linger_scope, label)
                        .map(|(arm, _)| if arm == ARM_SHARED { 0 } else { arm });
                    if let Some(selected) = arm {
                        self.set_route_arm(meta.lane, linger_scope, selected)?;
                    }
                }
                if let Some(arm) = arm
                    && arm == 0
                    && let Some(last_eff) =
                        self.cursor
                            .scope_lane_last_eff_for_arm(linger_scope, arm, meta.lane)
                    && last_eff == meta.eff_index
                    && let Some(first_eff) =
                        self.cursor.scope_lane_first_eff(linger_scope, meta.lane)
                {
                    self.set_lane_cursor_to_eff_index(meta.lane as usize, first_eff);
                    break;
                }
            }
            let Some(parent) = self.cursor.scope_parent(linger_scope) else {
                break;
            };
            linger_scope = parent;
        }
        if let Some(region) = self.cursor.scope_region()
            && region.kind == ScopeKind::Route
            && region.linger
        {
            let at_scope_start = self.cursor.index() == region.start;
            let at_passive_branch = self.cursor.jump_reason()
                == Some(JumpReason::PassiveObserverBranch)
                && self
                    .cursor
                    .scope_region()
                    .map(|scope_region| scope_region.scope_id == region.scope_id)
                    .unwrap_or(false);
            if (at_scope_start || at_passive_branch)
                && let Some(arm) = self.route_arm_for(meta.lane, region.scope_id)
                && arm == 0
                && let Some(first_eff) =
                    self.cursor.scope_lane_first_eff(region.scope_id, meta.lane)
            {
                self.set_lane_cursor_to_eff_index(meta.lane as usize, first_eff);
            }
        }
        self.maybe_advance_phase();
        Ok(payload)
    }

    fn apply_branch_recv_policy(
        &self,
        branch: &RouteBranch<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
    ) -> RecvResult<()> {
        let lane = crate::control::types::Lane::new(branch.branch_meta.lane_wire as u32);
        self.emit_endpoint_policy_audit(
            crate::policy_runtime::PolicySlot::EndpointRx,
            crate::observe::ids::ENDPOINT_RECV,
            self.sid.raw(),
            Self::endpoint_policy_args(
                lane,
                branch.label,
                crate::transport::wire::FrameFlags::empty(),
            ),
            lane,
        );
        Ok(())
    }
}

#[inline]
pub(super) fn decode_phase_invariant() -> RecvError {
    RecvError::PhaseInvariant
}
