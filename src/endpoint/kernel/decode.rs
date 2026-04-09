//! Decode-path helpers for `RouteBranch`.

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
    transport::{Transport, wire::WireDecodeOwned},
};

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>
    CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: BindingSlot,
{
    pub async fn decode_branch<M>(
        &mut self,
        branch: &mut RouteBranch<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
    ) -> RecvResult<M::Payload>
    where
        M: MessageSpec,
        M::Payload: WireDecodeOwned,
    {
        let label = branch.label;
        let transport_payload_len = branch.transport_payload_len;
        let transport_payload_lane = branch.transport_payload_lane;
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
                let payload = M::Payload::decode_owned(&ZERO_BUF).map_err(RecvError::Codec)?;
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
                let payload = M::Payload::decode_owned(&ZERO_BUF).map_err(RecvError::Codec)?;
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
                let payload = M::Payload::decode_owned(&ZERO_BUF).map_err(RecvError::Codec)?;
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

        let meta = self
            .cursor
            .try_recv_meta()
            .ok_or_else(decode_phase_invariant)?;
        let control_handling = <M::ControlKind as ControlPayloadKind>::HANDLING;
        let expects_control = !matches!(control_handling, ControlHandling::None);
        if meta.is_control != expects_control {
            return Err(decode_phase_invariant());
        }

        if self
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

        enum PolicyRestash {
            None,
            Scratch { len: usize, lane: u8 },
            BindingScratch { len: usize, lane: u8 },
        }

        let (payload, policy_restash) = if let Some(channel) = binding_channel {
            let primary_lane = self.primary_lane;
            let n = {
                let binding = &mut self.binding;
                let port = self.ports[primary_lane]
                    .as_mut()
                    .ok_or_else(decode_phase_invariant)?;
                binding
                    .on_recv(channel, lane_port::scratch_mut(port))
                    .map_err(|_| decode_phase_invariant())?
            };

            let port = self.ports[primary_lane]
                .as_ref()
                .ok_or_else(decode_phase_invariant)?;
            match M::Payload::decode_owned(&lane_port::scratch(port)[..n]) {
                Ok(payload) => (
                    payload,
                    PolicyRestash::BindingScratch {
                        len: n,
                        lane: primary_lane as u8,
                    },
                ),
                Err(err) => {
                    branch.binding_channel = None;
                    branch.transport_payload_len = n;
                    branch.transport_payload_lane = primary_lane as u8;
                    return Err(RecvError::Codec(err));
                }
            }
        } else if transport_payload_len != 0 {
            let port = self.port_for_lane(transport_payload_lane as usize);
            (
                M::Payload::decode_owned(&lane_port::scratch(port)[..transport_payload_len])
                    .map_err(RecvError::Codec)?,
                PolicyRestash::None,
            )
        } else {
            let port = self.port_for_lane(meta.lane as usize);
            let payload = lane_port::recv_future(port)
                .await
                .map_err(RecvError::Transport)?;
            let n = lane_port::copy_payload_into_scratch(port, &payload)
                .map_err(|_| decode_phase_invariant())?;
            match M::Payload::decode_owned(&lane_port::scratch(port)[..n]) {
                Ok(payload) => (
                    payload,
                    PolicyRestash::Scratch {
                        len: n,
                        lane: meta.lane,
                    },
                ),
                Err(err) => {
                    branch.transport_payload_len = n;
                    branch.transport_payload_lane = meta.lane;
                    return Err(RecvError::Codec(err));
                }
            }
        };

        if let Err(err) = self.apply_branch_recv_policy(branch) {
            match policy_restash {
                PolicyRestash::None => {}
                PolicyRestash::Scratch { len, lane } => {
                    branch.transport_payload_len = len;
                    branch.transport_payload_lane = lane;
                }
                PolicyRestash::BindingScratch { len, lane } => {
                    branch.binding_channel = None;
                    branch.transport_payload_len = len;
                    branch.transport_payload_lane = lane;
                }
            }
            return Err(err);
        }

        let meta = self
            .commit_branch_preview(&branch)?
            .ok_or_else(decode_phase_invariant)?;

        self.cursor
            .try_advance_past_jumps_in_place()
            .map_err(|_| decode_phase_invariant())?;

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
    B: BindingSlot,
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
