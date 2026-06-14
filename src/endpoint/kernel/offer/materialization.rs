//! Route-branch materialization for `offer()`.

use core::marker::PhantomData;

use super::{
    BranchKind, BranchMeta, CursorEndpoint, OfferScopeProfile, OfferScopeSelection,
    ResolvedRouteArm,
};
use crate::{
    endpoint::{RecvError, RecvResult},
    global::typestate::state_index_to_usize,
    runtime_core::config::Clock,
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

impl<'r, const ROLE: u8, T, C, const MAX_RV: usize> CursorEndpoint<'r, ROLE, T, C, MAX_RV>
where
    T: Transport + 'r,
    C: Clock,
{
    pub(in crate::endpoint::kernel) fn produce_branch(
        &mut self,
        selection: OfferScopeSelection,
        resolved: ResolvedRouteArm,
        profile: OfferScopeProfile,
        transport_payload: Option<lane_port::PreambleFrame<'r>>,
    ) -> RecvResult<Option<RouteBranch<'r, ROLE, T, C, MAX_RV>>> {
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
        let branch_kind = self.materialized_branch_kind(preview_meta);
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
            origin: meta.origin,
            frame_label: meta.frame_label,
            kind: branch_kind,
            profile,
            route_token,
            route_arm_selection_commit_evidence: resolved.route_arm_selection_commit_evidence,
        };
        Ok(Some(RouteBranch {
            label: meta.label,
            staged_payload: transport_payload_for_branch.map(StagedPayload::new),
            branch_meta,
            _cfg: PhantomData,
        }))
    }

    fn materialized_branch_kind(&self, preview_meta: super::CachedRecvMeta) -> BranchKind {
        let cursor_index = state_index_to_usize(preview_meta.cursor_index);
        if preview_meta.is_recv_step() {
            BranchKind::WireRecv
        } else if self.cursor.is_send_at(cursor_index) {
            BranchKind::ArmSendHint
        } else if self.cursor.is_local_action_at(cursor_index) {
            BranchKind::LocalAction
        } else {
            BranchKind::TerminalArm
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
                Some(payload) => Ok(MaterializedTransport::Accepted(Some(payload))),
                None => Ok(MaterializedTransport::DiscardedAndPending),
            };
        }
        let transport_payload_frame_mismatch = observed_frame_label != frame_label;
        if matches!(branch_kind, BranchKind::WireRecv) && transport_payload_frame_mismatch {
            payload.discard_uncommitted();
            return Err(RecvError::PhaseInvariant);
        }
        self.requeue_offer_transport_payload(payload)?;
        Ok(MaterializedTransport::Accepted(None))
    }
}
