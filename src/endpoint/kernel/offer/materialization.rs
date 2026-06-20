//! Route-branch materialization for `offer()`.

use super::{
    BranchKind, BranchMeta, CursorEndpoint, OfferScopeProfile, OfferScopeSelection,
    OfferStagedIngress, ResolvedRouteArm,
};
use crate::{
    endpoint::{RecvError, RecvResult},
    global::typestate::state_index_to_usize,
    transport::Transport,
};

use crate::endpoint::kernel::{
    core::{MaterializedRouteBranch, OfferedFrame},
    lane_port,
};

impl<'r, const ROLE: u8, T> CursorEndpoint<'r, ROLE, T>
where
    T: Transport + 'r,
{
    #[inline(never)]
    pub(super) fn produce_branch(
        &mut self,
        selection: OfferScopeSelection,
        resolved: ResolvedRouteArm,
        profile: OfferScopeProfile,
        ingress: &mut OfferStagedIngress<'r>,
    ) -> RecvResult<u8> {
        let scope_id = selection.scope_id;
        let route_token = resolved.route_token;
        let selected_arm = resolved.selected_arm;
        let preview_meta = match self.preview_selected_arm_meta(selection, selected_arm) {
            Ok(meta) => meta,
            Err(err) => {
                ingress.discard_terminal();
                return Err(err);
            }
        };
        if preview_meta.is_empty() {
            ingress.discard_terminal();
            return Err(RecvError::PhaseInvariant);
        }

        let lane_wire = preview_meta.lane;
        let transport_payload = ingress.take_transport();
        let branch_kind = self.materialized_branch_kind(&preview_meta);
        let transport_payload_for_branch = self.resolve_materialized_transport(
            branch_kind,
            lane_wire,
            preview_meta.peer,
            preview_meta.frame_label,
            transport_payload,
        )?;
        let branch_meta = BranchMeta {
            scope_id,
            selected_arm,
            lane_wire,
            cursor_index: preview_meta.cursor_index,
            eff_index: preview_meta.eff_index,
            label: preview_meta.label,
            origin: preview_meta.origin,
            frame_label: preview_meta.frame_label,
            kind: branch_kind,
            profile,
            route_token,
            route_arm_selection_commit_evidence: resolved.route_arm_selection_commit_evidence,
        };
        let offered_frame = transport_payload_for_branch.map(OfferedFrame::new);
        if self.public_route_branch.is_some() {
            if let Some(payload) = offered_frame {
                payload.discard_terminal();
            }
            crate::invariant();
        }
        self.public_route_branch = Some(MaterializedRouteBranch {
            offered_frame,
            branch_meta,
        });
        Ok(preview_meta.label)
    }

    fn materialized_branch_kind(&self, preview_meta: &super::CachedRecvMeta) -> BranchKind {
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

    #[inline(never)]
    fn resolve_materialized_transport(
        &mut self,
        branch_kind: BranchKind,
        lane_wire: u8,
        source_role: u8,
        frame_label: u8,
        transport_payload: Option<lane_port::PreambleFrame<'r>>,
    ) -> RecvResult<Option<lane_port::ReceivedFrame<'r>>> {
        let Some(payload) = transport_payload else {
            return Ok(None);
        };
        let observed_frame_label = payload.observed_frame_label_raw();
        let transport_payload_matches_branch_lane = payload.lane_wire() == lane_wire;
        if matches!(branch_kind, BranchKind::WireRecv) && transport_payload_matches_branch_lane {
            return Ok(Some(self.accept_materialized_transport_frame(
                payload.lane_idx(),
                lane_wire,
                source_role,
                frame_label,
                payload,
            )?));
        }
        let transport_payload_frame_mismatch = observed_frame_label != frame_label;
        if matches!(branch_kind, BranchKind::WireRecv) && transport_payload_frame_mismatch {
            payload.discard_uncommitted();
            return Err(RecvError::PhaseInvariant);
        }
        self.requeue_offer_transport_payload(payload)?;
        Ok(None)
    }
}
