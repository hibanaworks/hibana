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
    RouteBranch<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: BindingSlot,
{
    pub async fn decode<M>(
        self,
    ) -> RecvResult<(
        CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
        M::Payload,
    )>
    where
        M: MessageSpec,
        M::Payload: WireDecodeOwned,
    {
        let RouteBranch {
            label,
            transport_payload_len,
            transport_payload_lane,
            mut endpoint,
            binding_channel,
            branch_meta,
        } = self;

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

                let route_arm = Some(branch_meta.selected_arm);
                let lane_idx = branch_meta.lane_wire as usize;
                let progress_eff = if let Some(eff) = endpoint.cursor.scope_lane_last_eff_for_arm(
                    branch_meta.scope_id,
                    branch_meta.selected_arm,
                    branch_meta.lane_wire,
                ) {
                    eff
                } else if let Some(eff) = endpoint
                    .cursor
                    .scope_lane_last_eff(branch_meta.scope_id, branch_meta.lane_wire)
                {
                    eff
                } else {
                    branch_meta.eff_index
                };
                endpoint.advance_lane_cursor(lane_idx, progress_eff);
                if branch_meta.selected_arm > 0
                    && endpoint
                        .cursor
                        .scope_region_by_id(branch_meta.scope_id)
                        .map(|region| region.linger)
                        .unwrap_or(false)
                    && let Some(scope_last_eff) = endpoint
                        .cursor
                        .scope_lane_last_eff(branch_meta.scope_id, branch_meta.lane_wire)
                {
                    endpoint.advance_lane_cursor(lane_idx, scope_last_eff);
                }
                if !endpoint.align_cursor_to_lane_progress(lane_idx) {
                    endpoint.set_cursor(
                        endpoint
                            .cursor
                            .try_advance_past_jumps()
                            .map_err(|_| RecvError::PhaseInvariant)?,
                    );
                }
                endpoint.maybe_skip_remaining_route_arm(
                    branch_meta.scope_id,
                    branch_meta.lane_wire,
                    Some(branch_meta.selected_arm),
                    progress_eff,
                );
                endpoint.settle_scope_after_action(
                    branch_meta.scope_id,
                    route_arm,
                    None,
                    branch_meta.lane_wire,
                );
                endpoint.maybe_advance_phase();

                return Ok((endpoint, payload));
            }

            super::offer::BranchKind::EmptyArmTerminal => {
                static ZERO_BUF: [u8; 64] = [0u8; 64];
                let payload = M::Payload::decode_owned(&ZERO_BUF).map_err(RecvError::Codec)?;

                let route_arm = Some(branch_meta.selected_arm);

                endpoint.set_cursor(
                    endpoint
                        .cursor
                        .try_follow_jumps()
                        .map_err(|_| RecvError::PhaseInvariant)?,
                );

                let lane_idx = branch_meta.lane_wire as usize;
                if let Some(eff_index) = endpoint
                    .cursor
                    .scope_lane_last_eff(branch_meta.scope_id, branch_meta.lane_wire)
                {
                    endpoint.advance_lane_cursor(lane_idx, eff_index);
                } else {
                    endpoint.advance_lane_cursor(lane_idx, branch_meta.eff_index);
                }
                endpoint.settle_scope_after_action(
                    branch_meta.scope_id,
                    route_arm,
                    None,
                    branch_meta.lane_wire,
                );
                endpoint.maybe_advance_phase();

                return Ok((endpoint, payload));
            }

            super::offer::BranchKind::ArmSendHint => {
                static ZERO_BUF: [u8; 64] = [0u8; 64];
                let payload = M::Payload::decode_owned(&ZERO_BUF).map_err(RecvError::Codec)?;

                let route_arm = Some(branch_meta.selected_arm);
                let lane_idx = branch_meta.lane_wire as usize;
                let progress_eff = if let Some(eff) = endpoint.cursor.scope_lane_last_eff_for_arm(
                    branch_meta.scope_id,
                    branch_meta.selected_arm,
                    branch_meta.lane_wire,
                ) {
                    eff
                } else if let Some(eff) = endpoint
                    .cursor
                    .scope_lane_last_eff(branch_meta.scope_id, branch_meta.lane_wire)
                {
                    eff
                } else {
                    branch_meta.eff_index
                };
                endpoint.advance_lane_cursor(lane_idx, progress_eff);
                if branch_meta.selected_arm > 0
                    && endpoint
                        .cursor
                        .scope_region_by_id(branch_meta.scope_id)
                        .map(|region| region.linger)
                        .unwrap_or(false)
                    && let Some(scope_last_eff) = endpoint
                        .cursor
                        .scope_lane_last_eff(branch_meta.scope_id, branch_meta.lane_wire)
                {
                    endpoint.advance_lane_cursor(lane_idx, scope_last_eff);
                }
                if !endpoint.align_cursor_to_lane_progress(lane_idx) {
                    endpoint.set_cursor(
                        endpoint
                            .cursor
                            .try_advance_past_jumps()
                            .map_err(|_| RecvError::PhaseInvariant)?,
                    );
                }
                endpoint.maybe_skip_remaining_route_arm(
                    branch_meta.scope_id,
                    branch_meta.lane_wire,
                    Some(branch_meta.selected_arm),
                    progress_eff,
                );
                endpoint.settle_scope_after_action(
                    branch_meta.scope_id,
                    route_arm,
                    None,
                    branch_meta.lane_wire,
                );
                endpoint.maybe_advance_phase();

                return Ok((endpoint, payload));
            }

            super::offer::BranchKind::WireRecv => {}
        }

        let meta = endpoint
            .cursor
            .try_recv_meta()
            .ok_or_else(decode_phase_invariant)?;
        let control_handling = <M::ControlKind as ControlPayloadKind>::HANDLING;
        let expects_control = !matches!(control_handling, ControlHandling::None);
        if meta.is_control != expects_control {
            return Err(decode_phase_invariant());
        }

        if endpoint
            .control_semantic_kind(meta.label, meta.resource)
            .is_loop()
        {
            if let Some(LoopMetadata {
                scope: scope_id,
                controller,
                target,
                role,
                ..
            }) = endpoint.cursor.loop_metadata_inner()
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
                let port = endpoint.port_for_lane(meta.lane as usize);
                let lane = port.lane();
                port.loop_table().acknowledge(lane, ROLE, idx);
                let has_local_decision = port.loop_table().has_decision(lane, idx);
                if has_local_decision {
                    port.ack_loop_decision(idx, ROLE);
                }
            }
        }

        let payload = if let Some(channel) = binding_channel {
            let primary_lane = endpoint.primary_lane;
            let n = {
                let binding = &mut endpoint.binding;
                let port = endpoint.ports[primary_lane]
                    .as_mut()
                    .ok_or_else(decode_phase_invariant)?;
                binding
                    .on_recv(channel, lane_port::scratch_mut(port))
                    .map_err(|_| decode_phase_invariant())?
            };

            let port = endpoint.ports[primary_lane]
                .as_ref()
                .ok_or_else(decode_phase_invariant)?;
            M::Payload::decode_owned(&lane_port::scratch(port)[..n]).map_err(RecvError::Codec)?
        } else if transport_payload_len != 0 {
            let port = endpoint.port_for_lane(transport_payload_lane as usize);
            M::Payload::decode_owned(&lane_port::scratch(port)[..transport_payload_len])
                .map_err(RecvError::Codec)?
        } else {
            M::Payload::decode_owned(&[]).map_err(RecvError::Codec)?
        };

        endpoint.set_cursor(
            endpoint
                .cursor
                .try_advance_past_jumps()
                .map_err(|_| decode_phase_invariant())?,
        );

        let decode_lane_idx = meta.lane as usize;
        endpoint.advance_lane_cursor(decode_lane_idx, meta.eff_index);
        endpoint.maybe_skip_remaining_route_arm(
            meta.scope,
            meta.lane,
            meta.route_arm,
            meta.eff_index,
        );
        endpoint.settle_scope_after_action(
            meta.scope,
            meta.route_arm,
            Some(meta.eff_index),
            meta.lane,
        );
        if branch_meta.scope_id != meta.scope {
            endpoint.settle_scope_after_action(
                branch_meta.scope_id,
                Some(branch_meta.selected_arm),
                Some(meta.eff_index),
                branch_meta.lane_wire,
            );
        }
        let mut linger_scope = meta.scope;
        loop {
            if endpoint.is_linger_route(linger_scope) {
                let mut arm = endpoint.route_arm_for(meta.lane, linger_scope);
                if arm.is_none() {
                    arm = endpoint
                        .cursor
                        .first_recv_target_evidence(linger_scope, label)
                        .map(|(arm, _)| if arm == ARM_SHARED { 0 } else { arm });
                    if let Some(selected) = arm {
                        endpoint.set_route_arm(meta.lane, linger_scope, selected)?;
                    }
                }
                if let Some(arm) = arm
                    && arm == 0
                    && let Some(last_eff) =
                        endpoint
                            .cursor
                            .scope_lane_last_eff_for_arm(linger_scope, arm, meta.lane)
                    && last_eff == meta.eff_index
                    && let Some(first_eff) = endpoint
                        .cursor
                        .scope_lane_first_eff(linger_scope, meta.lane)
                {
                    endpoint.set_lane_cursor_to_eff_index(meta.lane as usize, first_eff);
                    break;
                }
            }
            let Some(parent) = endpoint.cursor.scope_parent(linger_scope) else {
                break;
            };
            linger_scope = parent;
        }
        if let Some(region) = endpoint.cursor.scope_region()
            && region.kind == ScopeKind::Route
            && region.linger
        {
            let at_scope_start = endpoint.cursor.index() == region.start;
            let at_passive_branch = endpoint.cursor.jump_reason()
                == Some(JumpReason::PassiveObserverBranch)
                && endpoint
                    .cursor
                    .scope_region()
                    .map(|scope_region| scope_region.scope_id == region.scope_id)
                    .unwrap_or(false);
            if (at_scope_start || at_passive_branch)
                && let Some(arm) = endpoint.route_arm_for(meta.lane, region.scope_id)
                && arm == 0
                && let Some(first_eff) = endpoint
                    .cursor
                    .scope_lane_first_eff(region.scope_id, meta.lane)
            {
                endpoint.set_lane_cursor_to_eff_index(meta.lane as usize, first_eff);
            }
        }
        endpoint.maybe_advance_phase();
        Ok((endpoint, payload))
    }

    #[inline]
    pub fn label(&self) -> u8 {
        self.label
    }

    #[inline]
    pub fn into_endpoint(self) -> CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B> {
        self.endpoint
    }
}

#[inline]
pub(super) fn decode_phase_invariant() -> RecvError {
    RecvError::PhaseInvariant
}
