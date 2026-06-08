//! Route-branch materialization for `offer()`.

use core::marker::PhantomData;

use super::{
    BranchKind, BranchMeta, CursorEndpoint, OfferScopeProfile, OfferScopeSelection,
    ResolvedRouteArm,
};
use crate::{
    control::cap::mint::{EpochTable, MintConfigMarker},
    endpoint::{RecvError, RecvResult},
    global::typestate::state_index_to_usize,
    runtime::{config::Clock, consts::LabelUniverse},
    transport::Transport,
};

use crate::endpoint::kernel::{
    core::{RouteBranch, StagedPayload},
    lane_port,
};

enum MaterializedTransport<'r> {
    Accepted(Option<lane_port::ReceivedFrame<'r>>),
    DiscardedAndPending,
}

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint>
    CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
{
    pub(in crate::endpoint::kernel) fn produce_branch(
        &mut self,
        selection: OfferScopeSelection,
        resolved: ResolvedRouteArm,
        profile: OfferScopeProfile,
        transport_payload: Option<lane_port::PreambleFrame<'r>>,
    ) -> RecvResult<Option<RouteBranch<'r, ROLE, T, U, C, E, MAX_RV, Mint>>> {
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
        let transport_payload_for_branch = match self.resolve_materialized_transport(
            branch_kind,
            lane_wire,
            meta.peer,
            meta.frame_label,
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
            label: meta.label,
            is_control: meta.is_control,
            frame_label: meta.frame_label,
            kind: branch_kind,
            profile,
            route_token,
            route_arm_selection_commit_evidence: resolved.route_arm_selection_commit_evidence,
        };
        Ok(Some(RouteBranch {
            label: meta.label,
            staged_payload: transport_payload_for_branch
                .map(|payload| StagedPayload::Transport { frame: payload }),
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

    fn resolve_materialized_transport(
        &mut self,
        branch_kind: BranchKind,
        lane_wire: u8,
        source_role: u8,
        frame_label: u8,
        transport_payload: Option<lane_port::PreambleFrame<'r>>,
    ) -> RecvResult<MaterializedTransport<'r>> {
        let Some(payload) = transport_payload else {
            return Ok(MaterializedTransport::Accepted(None));
        };
        let observed_frame_label = payload.observed_frame_label_raw();
        let transport_payload_matches_branch_lane = payload.lane_wire() == lane_wire;
        if matches!(branch_kind, BranchKind::WireRecv) && transport_payload_matches_branch_lane {
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
        if matches!(branch_kind, BranchKind::WireRecv) && transport_payload_frame_mismatch {
            payload.discard_uncommitted();
            return Err(RecvError::PhaseInvariant);
        }
        self.requeue_offer_transport_payload(payload)?;
        Ok(MaterializedTransport::Accepted(None))
    }
}
