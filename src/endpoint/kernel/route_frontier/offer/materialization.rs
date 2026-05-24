//! Route-branch materialization for `offer()`.

use core::marker::PhantomData;

use super::{
    BranchKind, BranchMeta, LaneIngressEvidence, OfferScopeSelection, ResolvedRouteDecision,
    RouteFrontierMachine,
};
use crate::{
    binding::BindingSlot,
    control::cap::mint::{EpochTable, MintConfigMarker},
    endpoint::{RecvError, RecvResult},
    global::typestate::state_index_to_usize,
    runtime::{config::Clock, consts::LabelUniverse},
    transport::Transport,
};

use crate::endpoint::kernel::{
    core::{RouteBranch, StagedPayload},
    inbox::PackedIngressEvidence,
    lane_port,
};

impl<'endpoint, 'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>
    RouteFrontierMachine<'endpoint, 'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: BindingSlot + 'r,
{
    pub(in crate::endpoint::kernel) fn produce_branch(
        &mut self,
        selection: OfferScopeSelection,
        resolved: ResolvedRouteDecision,
        is_route_controller: bool,
        binding_evidence: Option<LaneIngressEvidence>,
        transport_payload: Option<lane_port::ReceivedFrame<'r>>,
    ) -> RecvResult<RouteBranch<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>> {
        let mut transport_payload = transport_payload;
        let scope_id = selection.scope_id;
        let route_token = resolved.route_token;
        let selected_arm = resolved.selected_arm;
        let resolved_hint_frame_label = resolved.resolved_hint_frame_label;
        let preview_meta = match self.endpoint.preview_selected_arm_meta(
            selection,
            selected_arm,
            resolved_hint_frame_label,
        ) {
            Ok(meta) => meta,
            Err(err) => {
                if let Some(payload) = transport_payload.take() {
                    payload.discard_uncommitted();
                }
                return Err(err);
            }
        };
        let (_cursor_index, meta) = match preview_meta.recv_meta() {
            Some(meta) => meta,
            None => {
                if let Some(payload) = transport_payload.take() {
                    payload.discard_uncommitted();
                }
                return Err(RecvError::PhaseInvariant);
            }
        };

        let lane_wire = meta.lane;
        let branch_kind = self.materialized_branch_kind(
            selection,
            scope_id,
            selected_arm,
            is_route_controller,
            meta,
        );
        let binding_evidence = self.resolve_materialized_binding(
            selection,
            branch_kind,
            selected_arm,
            meta,
            binding_evidence,
        );
        let binding_staged_payload = binding_evidence.and_then(|lane_evidence| {
            let (lane_idx, evidence) = lane_evidence.into_parts();
            self.endpoint
                .take_restored_binding_payload(lane_idx, evidence)
                .map(|payload| (lane_idx as u8, payload))
        });
        let transport_payload_for_branch = self.resolve_materialized_transport(
            branch_kind,
            lane_wire,
            meta.frame_label,
            resolved_hint_frame_label,
            binding_evidence.is_some(),
            transport_payload,
        )?;
        let branch_progress_eff = self
            .endpoint
            .cursor
            .scope_lane_last_eff_for_arm(scope_id, selected_arm, lane_wire)
            .or_else(|| {
                self.endpoint
                    .cursor
                    .scope_lane_last_eff(scope_id, lane_wire)
            })
            .unwrap_or(meta.eff_index);
        let branch_meta = BranchMeta {
            scope_id,
            selected_arm,
            lane_wire,
            eff_index: branch_progress_eff,
            frame_label: meta.frame_label,
            kind: branch_kind,
            route_source: route_token.source(),
            poll_route_decision_authority: resolved.poll_route_decision_authority,
        };
        self.endpoint
            .set_cursor_index(state_index_to_usize(preview_meta.cursor_index));
        Ok(RouteBranch {
            label: meta.label,
            binding_evidence: PackedIngressEvidence::from_option(
                binding_evidence.map(|lane_evidence| lane_evidence.evidence),
            ),
            binding_evidence_lane: binding_evidence
                .map(|lane_evidence| lane_evidence.lane_idx as u8)
                .unwrap_or(u8::MAX),
            staged_payload: binding_staged_payload
                .map(|(lane, payload)| StagedPayload::Binding { lane, payload })
                .or_else(|| {
                    transport_payload_for_branch
                        .map(|payload| StagedPayload::Transport { frame: payload })
                }),
            branch_meta,
            _cfg: PhantomData,
        })
    }

    fn materialized_branch_kind(
        &self,
        selection: OfferScopeSelection,
        scope_id: crate::global::const_dsl::ScopeId,
        selected_arm: u8,
        is_route_controller: bool,
        meta: crate::global::typestate::RecvMeta,
    ) -> BranchKind {
        let passive_linger_loop_label = !is_route_controller
            && self.endpoint.is_linger_route(scope_id)
            && self.endpoint.control_semantic_kind(meta.semantic).is_loop();
        if self.endpoint.cursor.is_recv() {
            if passive_linger_loop_label
                || (!is_route_controller
                    && self.endpoint.control_semantic_kind(meta.semantic).is_loop()
                    && self.endpoint.selection_non_wire_loop_control_recv(
                        selection,
                        is_route_controller,
                        selected_arm,
                        meta.label,
                    ))
            {
                BranchKind::LocalControl
            } else {
                BranchKind::WireRecv
            }
        } else if self.endpoint.cursor.is_send() {
            BranchKind::ArmSendHint
        } else if self.endpoint.cursor.is_local_action() || self.endpoint.cursor.is_jump() {
            BranchKind::LocalControl
        } else {
            BranchKind::EmptyArmTerminal
        }
    }

    fn resolve_materialized_binding(
        &mut self,
        selection: OfferScopeSelection,
        branch_kind: BranchKind,
        selected_arm: u8,
        meta: crate::global::typestate::RecvMeta,
        mut binding_evidence: Option<LaneIngressEvidence>,
    ) -> Option<LaneIngressEvidence> {
        if !matches!(branch_kind, BranchKind::WireRecv) {
            if let Some(lane_evidence) = binding_evidence {
                let (lane_idx, evidence) = lane_evidence.into_parts();
                self.endpoint.put_back_binding_for_lane(lane_idx, evidence);
            }
            return None;
        }

        let mut selected_evidence = None;
        let lane_idx = meta.lane as usize;
        if let Some(carried) = binding_evidence.as_ref()
            && carried.lane_idx != lane_idx
        {
            let (carried_lane, carried_evidence) = binding_evidence.take().unwrap().into_parts();
            self.endpoint
                .put_back_binding_for_lane(carried_lane, carried_evidence);
        }

        let frame_label_meta = self.endpoint.selection_frame_label_meta(selection);
        if let Some(expected_frame_label) =
            frame_label_meta.preferred_binding_frame_label(Some(selected_arm))
        {
            if let Some(carried) = binding_evidence.take() {
                let (carried_lane, carried) = carried.into_parts();
                if carried.frame_label.raw() == expected_frame_label
                    && carried.frame_label.raw() == meta.frame_label
                {
                    selected_evidence = Some(carried);
                } else {
                    self.endpoint
                        .put_back_binding_for_lane(carried_lane, carried);
                }
            }
            if selected_evidence.is_none() && expected_frame_label == meta.frame_label {
                selected_evidence = self
                    .endpoint
                    .take_matching_binding_for_lane(lane_idx, expected_frame_label);
            }
        } else {
            if let Some(carried) = binding_evidence.take() {
                let (carried_lane, carried) = carried.into_parts();
                if carried.frame_label.raw() == meta.frame_label {
                    selected_evidence = Some(carried);
                } else {
                    self.endpoint
                        .put_back_binding_for_lane(carried_lane, carried);
                }
            }
            if selected_evidence.is_none() {
                selected_evidence = self
                    .endpoint
                    .take_matching_binding_for_lane(lane_idx, meta.frame_label);
            }
        }
        selected_evidence.map(|evidence| LaneIngressEvidence::new(lane_idx, evidence))
    }

    fn resolve_materialized_transport(
        &mut self,
        branch_kind: BranchKind,
        lane_wire: u8,
        frame_label: u8,
        resolved_hint_frame_label: Option<u8>,
        binding_selected: bool,
        transport_payload: Option<lane_port::ReceivedFrame<'r>>,
    ) -> RecvResult<Option<lane_port::ReceivedFrame<'r>>> {
        let Some(payload) = transport_payload else {
            return Ok(None);
        };
        let transport_payload_matches_branch = payload.lane_wire() == lane_wire
            && resolved_hint_frame_label
                .map(|hint_frame_label| hint_frame_label == frame_label)
                .unwrap_or(true);
        if matches!(branch_kind, BranchKind::WireRecv)
            && !binding_selected
            && transport_payload_matches_branch
        {
            return Ok(Some(payload));
        }
        let transport_payload_frame_mismatch = resolved_hint_frame_label
            .map(|hint_frame_label| hint_frame_label != frame_label)
            .unwrap_or(false);
        if matches!(branch_kind, BranchKind::WireRecv)
            && !binding_selected
            && transport_payload_frame_mismatch
        {
            payload.discard_uncommitted();
            return Err(RecvError::PhaseInvariant);
        }
        self.requeue_offer_transport_payload(payload);
        Ok(None)
    }
}
