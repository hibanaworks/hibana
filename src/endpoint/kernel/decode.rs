//! Decode-path helpers for `RouteBranch`.

use core::future::poll_fn;

use super::{
    core::{CursorEndpoint, RouteBranch},
    lane_port,
};
use crate::{
    binding::BindingSlot,
    control::cap::mint::{EpochTable, MintConfigMarker},
    endpoint::{RecvError, RecvResult},
    global::{
        ControlHandling, ControlPayloadKind, MessageSpec,
        const_dsl::ScopeKind,
        typestate::{ARM_SHARED, JumpReason, LoopMetadata, LoopRole},
    },
    runtime::{config::Clock, consts::LabelUniverse},
    transport::{Transport, wire::{Payload, WirePayload}},
};

type DecodedPayload<'a, M> =
    <<M as MessageSpec>::Payload as WirePayload>::Decoded<'a>;

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
    fn prepare_decode_transport_wait<M>(
        &mut self,
        branch: &mut RouteBranch<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
    ) -> RecvResult<Option<crate::global::typestate::RecvMeta>>
    where
        M: MessageSpec,
        M::Payload: WirePayload,
    {
        let expected = <M as MessageSpec>::LABEL;
        if branch.label != expected {
            return Err(RecvError::LabelMismatch {
                expected,
                actual: branch.label,
            });
        }
        if !matches!(branch.branch_meta.kind, super::offer::BranchKind::WireRecv)
            || branch.binding_channel.is_some()
            || branch.transport_payload_len != 0
        {
            return Ok(None);
        }
        let meta = self.cursor.try_recv_meta().ok_or_else(decode_phase_invariant)?;
        let control_handling = <M::ControlKind as ControlPayloadKind>::HANDLING;
        let expects_control = !matches!(control_handling, ControlHandling::None);
        if meta.is_control != expects_control {
            return Err(decode_phase_invariant());
        }
        if self.control_semantic_kind(meta.label, meta.resource).is_loop()
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

    pub(crate) fn decode_branch_ptr<'a, M>(
        &'a mut self,
        branch_ptr: *mut RouteBranch<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
    ) -> impl core::future::Future<Output = RecvResult<DecodedPayload<'a, M>>> + 'a
    where
        M: MessageSpec,
        M::Payload: WirePayload,
    {
        let mut prepared_meta = None;
        let mut staged_payload = None;
        let mut pending_recv = lane_port::PendingRecv::new();
        poll_fn(move |cx| {
            let branch = lane_port::deref_mut(branch_ptr);
            if prepared_meta.is_none() {
                prepared_meta = self.prepare_decode_transport_wait::<M>(branch)?;
            }
            if let Some(meta) = prepared_meta
                && staged_payload.is_none()
                && branch.staged_payload.is_none()
                && branch.transport_payload_len == 0
                && branch.binding_channel.is_none()
            {
                let port = self.port_for_lane(meta.lane as usize);
                let payload = match lane_port::poll_recv(&mut pending_recv, port, cx) {
                    core::task::Poll::Pending => return core::task::Poll::Pending,
                    core::task::Poll::Ready(Ok(payload)) => payload,
                    core::task::Poll::Ready(Err(err)) => {
                        prepared_meta = None;
                        return core::task::Poll::Ready(Err(RecvError::Transport(err)));
                    }
                };
                staged_payload = Some(super::core::StagedPayload::Transport {
                    lane: meta.lane,
                    payload,
                });
            }
            core::task::Poll::Ready(self.finish_decode_branch::<M>(
                branch,
                prepared_meta,
                staged_payload,
            ))
        })
    }

    pub fn decode_branch<'a, M>(
        &'a mut self,
        branch: &mut RouteBranch<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
    ) -> impl core::future::Future<Output = RecvResult<DecodedPayload<'a, M>>> + 'a
    where
        M: MessageSpec,
        M::Payload: WirePayload,
    {
        self.decode_branch_ptr::<M>(core::ptr::from_mut(branch))
    }

    fn finish_decode_branch<'a, M>(
        &mut self,
        branch: &mut RouteBranch<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
        prepared_meta: Option<crate::global::typestate::RecvMeta>,
        staged_payload: Option<super::core::StagedPayload<'r>>,
    ) -> RecvResult<DecodedPayload<'a, M>>
    where
        M: MessageSpec,
        M::Payload: WirePayload,
    {
        let label = branch.label;
        let transport_payload_len = branch.transport_payload_len;
        let _transport_payload_lane = branch.transport_payload_lane;
        let binding_channel = branch.binding_channel;
        let branch_meta = branch.branch_meta;

        let expected = <M as MessageSpec>::LABEL;
        if label != expected {
            return Err(RecvError::LabelMismatch {
                expected,
                actual: label,
            });
        }

        match branch_meta.kind {
            super::offer::BranchKind::LocalControl => {
                static ZERO_BUF: [u8; 64] = [0u8; 64];
                let payload =
                    <<M as MessageSpec>::Payload as WirePayload>::decode_payload(Payload::new(
                        &ZERO_BUF,
                    ))
                    .map_err(RecvError::Codec)?;
                self.apply_branch_recv_policy(branch)?;
                let _ = self.commit_branch_preview(&branch)?;

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
                static ZERO_BUF: [u8; 64] = [0u8; 64];
                let payload =
                    <<M as MessageSpec>::Payload as WirePayload>::decode_payload(Payload::new(
                        &ZERO_BUF,
                    ))
                    .map_err(RecvError::Codec)?;
                self.apply_branch_recv_policy(branch)?;
                let _ = self.commit_branch_preview(&branch)?;

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
                static ZERO_BUF: [u8; 64] = [0u8; 64];
                let payload =
                    <<M as MessageSpec>::Payload as WirePayload>::decode_payload(Payload::new(
                        &ZERO_BUF,
                    ))
                    .map_err(RecvError::Codec)?;
                self.apply_branch_recv_policy(branch)?;
                let _ = self.commit_branch_preview(&branch)?;

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
        let control_handling = <M::ControlKind as ControlPayloadKind>::HANDLING;
        let expects_control = !matches!(control_handling, ControlHandling::None);
        if meta.is_control != expects_control {
            return Err(decode_phase_invariant());
        }

        if prepared_meta.is_none()
            && self
            .control_semantic_kind(meta.label, meta.resource)
            .is_loop()
        {
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

        let mut staged_payload = staged_payload.or(branch.staged_payload);
        if staged_payload.is_none()
            && let Some(channel) = binding_channel
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
                channel,
                scratch_ptr,
            )
            .map_err(|_| decode_phase_invariant())?;
            staged_payload = Some(super::core::StagedPayload::Binding {
                lane: primary_lane as u8,
                payload,
            });
        } else if staged_payload.is_none() && transport_payload_len == 0 {
            return Err(decode_phase_invariant());
        }

        let staged_payload = staged_payload.ok_or_else(decode_phase_invariant)?;
        let payload_view: Payload<'a> = lane_port::shrink_payload(staged_payload.payload());
        let payload = match <<M as MessageSpec>::Payload as WirePayload>::decode_payload(
            payload_view,
        ) {
            Ok(payload) => payload,
            Err(err) => {
                branch.binding_channel = None;
                branch.staged_payload = Some(staged_payload);
                branch.transport_payload_len = staged_payload.payload().as_bytes().len();
                branch.transport_payload_lane = staged_payload.lane();
                return Err(RecvError::Codec(err));
            }
        };

        if let Err(err) = self.apply_branch_recv_policy(branch) {
            branch.binding_channel = None;
            branch.staged_payload = Some(staged_payload);
            branch.transport_payload_len = staged_payload.payload().as_bytes().len();
            branch.transport_payload_lane = staged_payload.lane();
            return Err(err);
        }

        let meta = match self.commit_branch_preview(&branch) {
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
        let action = self.eval_endpoint_policy(
            crate::epf::vm::Slot::EndpointRx,
            crate::observe::ids::ENDPOINT_RECV,
            self.sid.raw(),
            Self::endpoint_policy_args(
                lane,
                branch.label,
                crate::transport::wire::FrameFlags::empty(),
            ),
            lane,
        );
        self.apply_recv_policy(action, branch.branch_meta.scope_id, lane)
    }
}

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>
    RouteBranch<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: BindingSlot + 'r,
{
    #[inline]
    pub fn label(&self) -> u8 {
        self.label
    }

    #[cfg(test)]
    #[inline]
    pub(super) fn scope_id(&self) -> crate::global::const_dsl::ScopeId {
        self.branch_meta.scope_id
    }
}

#[inline]
pub(super) fn decode_phase_invariant() -> RecvError {
    RecvError::PhaseInvariant
}
