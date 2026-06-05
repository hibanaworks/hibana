//! Route-branch materialization for `offer()`.

use core::marker::PhantomData;

use super::{
    BranchKind, BranchMeta, CursorEndpoint, LaneIngressEvidence, OfferScopeProfile,
    OfferScopeSelection, ResolvedRouteDecision,
};
use crate::{
    binding::EndpointSlot,
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

enum MaterializedTransport<'r> {
    Accepted(Option<lane_port::ReceivedFrame<'r>>),
    DiscardedAndPending,
}

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>
    CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: EndpointSlot + 'r,
{
    pub(in crate::endpoint::kernel) fn produce_branch(
        &mut self,
        selection: OfferScopeSelection,
        resolved: ResolvedRouteDecision,
        profile: OfferScopeProfile,
        binding_evidence: Option<LaneIngressEvidence>,
        transport_payload: Option<lane_port::PreambleFrame<'r>>,
    ) -> RecvResult<Option<RouteBranch<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>>> {
        let mut transport_payload = transport_payload;
        let scope_id = selection.scope_id;
        let route_token = resolved.route_token;
        let selected_arm = resolved.selected_arm;
        let preview_meta = match self.preview_selected_arm_meta(selection, selected_arm) {
            Ok(meta) => meta,
            Err(err) => {
                if let Some(payload) = transport_payload.take() {
                    payload.discard_uncommitted();
                }
                return Err(err);
            }
        };
        let (_, meta) = match preview_meta.recv_meta() {
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
            profile,
            preview_meta,
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
            self.take_restored_binding_payload(lane_idx, evidence)
                .map(|payload| (lane_idx as u8, payload))
        });
        let transport_payload_for_branch = match self.resolve_materialized_transport(
            branch_kind,
            lane_wire,
            meta.peer,
            meta.frame_label,
            binding_evidence.is_some(),
            transport_payload,
        )? {
            MaterializedTransport::Accepted(payload) => payload,
            MaterializedTransport::DiscardedAndPending => return Ok(None),
        };
        let branch_meta = BranchMeta {
            scope_id,
            selected_arm,
            lane_wire,
            cursor_index: preview_meta.cursor_index,
            eff_index: meta.eff_index,
            frame_label: meta.frame_label,
            kind: branch_kind,
            profile,
            route_source: route_token.source(),
            route_decision_commit_evidence: resolved.route_decision_commit_evidence,
        };
        self.set_cursor_index(state_index_to_usize(preview_meta.cursor_index));
        Ok(Some(RouteBranch {
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
        }))
    }

    fn materialized_branch_kind(
        &self,
        selection: OfferScopeSelection,
        scope_id: crate::global::const_dsl::ScopeId,
        selected_arm: u8,
        profile: OfferScopeProfile,
        preview_meta: super::CachedRecvMeta,
        meta: crate::global::typestate::RecvMeta,
    ) -> BranchKind {
        let passive_linger_loop_label = profile.is_passive()
            && self.is_linger_route(scope_id)
            && self.control_semantic_kind(meta.semantic).is_loop();
        let cursor_index = state_index_to_usize(preview_meta.cursor_index);
        if preview_meta.is_recv_step() {
            if passive_linger_loop_label
                || (profile.is_passive()
                    && self.control_semantic_kind(meta.semantic).is_loop()
                    && self.selection_non_wire_loop_control_recv(
                        selection,
                        profile.is_controller(),
                        selected_arm,
                        meta.label,
                    ))
            {
                BranchKind::LocalControl
            } else {
                BranchKind::WireRecv
            }
        } else if self.cursor.is_send_at(cursor_index) {
            BranchKind::ArmSendHint
        } else if self.cursor.is_local_action_at(cursor_index)
            || self.cursor.is_jump_at(cursor_index)
        {
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
                self.put_back_binding_for_lane(lane_idx, evidence);
            }
            return None;
        }

        let mut selected_evidence = None;
        let lane_idx = meta.lane as usize;
        if let Some(carried) = binding_evidence.as_ref()
            && carried.lane_idx != lane_idx
        {
            let (carried_lane, carried_evidence) = binding_evidence.take().unwrap().into_parts();
            self.put_back_binding_for_lane(carried_lane, carried_evidence);
        }

        let frame_label_meta = self.selection_frame_label_meta(selection);
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
                    self.put_back_binding_for_lane(carried_lane, carried);
                }
            }
            if selected_evidence.is_none() && expected_frame_label == meta.frame_label {
                selected_evidence =
                    self.take_matching_binding_for_lane(lane_idx, expected_frame_label);
            }
        } else {
            if let Some(carried) = binding_evidence.take() {
                let (carried_lane, carried) = carried.into_parts();
                if carried.frame_label.raw() == meta.frame_label {
                    selected_evidence = Some(carried);
                } else {
                    self.put_back_binding_for_lane(carried_lane, carried);
                }
            }
            if selected_evidence.is_none() {
                selected_evidence = self.take_matching_binding_for_lane(lane_idx, meta.frame_label);
            }
        }
        selected_evidence.map(|evidence| LaneIngressEvidence::new(lane_idx, evidence))
    }

    fn resolve_materialized_transport(
        &mut self,
        branch_kind: BranchKind,
        lane_wire: u8,
        source_role: u8,
        frame_label: u8,
        binding_selected: bool,
        transport_payload: Option<lane_port::PreambleFrame<'r>>,
    ) -> RecvResult<MaterializedTransport<'r>> {
        let Some(payload) = transport_payload else {
            return Ok(MaterializedTransport::Accepted(None));
        };
        let observed_frame_label = payload.observed_frame_label_raw();
        let transport_payload_matches_branch_lane = payload.lane_wire() == lane_wire;
        if matches!(branch_kind, BranchKind::WireRecv)
            && !binding_selected
            && transport_payload_matches_branch_lane
        {
            return match self.accept_materialized_transport_frame(
                payload.lane_idx(),
                lane_wire,
                source_role,
                frame_label,
                payload,
            ) {
                Ok(payload) => Ok(MaterializedTransport::Accepted(Some(payload))),
                Err(()) => Ok(MaterializedTransport::DiscardedAndPending),
            };
        }
        let transport_payload_frame_mismatch =
            observed_frame_label.is_some_and(|observed| observed != frame_label);
        if matches!(branch_kind, BranchKind::WireRecv)
            && !binding_selected
            && transport_payload_frame_mismatch
        {
            payload.discard_uncommitted();
            return Err(RecvError::PhaseInvariant);
        }
        self.requeue_offer_transport_payload(payload)?;
        Ok(MaterializedTransport::Accepted(None))
    }
}
