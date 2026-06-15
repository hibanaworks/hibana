use super::{
    Arm, CursorEndpoint, FlowPreviewError, PublicActiveOp, ScopeId, SendError, SendResult,
    Transport,
};

impl<'r, const ROLE: u8, T, const MAX_RV: usize> CursorEndpoint<'r, ROLE, T, MAX_RV>
where
    T: Transport + 'r,
{
    #[inline]
    fn preview_flow_arm_for_scope(&self, scope_id: ScopeId) -> Option<u8> {
        if scope_id.is_none() {
            return None;
        }
        if let Some(arm) = self.selected_arm_for_scope(scope_id) {
            return Some(arm);
        }
        let offer_lanes = self.offer_lane_set_for_scope(scope_id);
        offer_lanes.first_set(self.cursor.logical_lane_count())?;
        self.preview_scope_ack_token_non_consuming(scope_id, offer_lanes)
            .map(|token| token.arm().as_u8())
            .or_else(|| self.poll_arm_from_ready_mask(scope_id).map(Arm::as_u8))
    }

    #[inline]
    fn map_flow_preview_error(error: FlowPreviewError) -> SendError {
        match error {
            FlowPreviewError::Invariant => SendError::PhaseInvariant,
            FlowPreviewError::LabelMismatch { expected, actual } => {
                SendError::LabelMismatch { expected, actual }
            }
        }
    }

    /// Preview the current send transition without mutating endpoint state.
    pub(crate) fn preview_flow_meta(
        &mut self,
        target_label: u8,
    ) -> SendResult<crate::endpoint::kernel::SendPreview> {
        if let Some(kind) = self.session_fault() {
            return Err(SendError::SessionFault(kind));
        }
        if self.public_active_op != PublicActiveOp::Idle {
            self.public_op_busy_fault();
            return Err(SendError::PhaseInvariant);
        }
        let (meta, cursor_index) = self
            .cursor
            .flow_preview_send_meta_for_label::<ROLE>(
                target_label,
                |scope| self.selected_arm_for_scope(scope),
                |scope| self.preview_flow_arm_for_scope(scope),
                |scope, label| self.lane_for_label_or_offer(scope, label),
            )
            .map_err(Self::map_flow_preview_error)?;
        Ok(crate::endpoint::kernel::SendPreview::new(
            meta,
            cursor_index,
        ))
    }
}
