use super::{
    Arm, CursorEndpoint, PublicActiveOp, ScopeId, SendError, SendPreviewError, SendResult,
    Transport,
};

impl<'r, const ROLE: u8, T> CursorEndpoint<'r, ROLE, T>
where
    T: Transport + 'r,
{
    #[inline]
    fn preview_send_arm_for_scope(&self, scope_id: ScopeId) -> Option<u8> {
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
    fn map_send_preview_error(error: SendPreviewError) -> SendError {
        match error {
            SendPreviewError::Invariant => SendError::PhaseInvariant,
            SendPreviewError::LabelMismatch { expected, actual } => {
                SendError::LabelMismatch { expected, actual }
            }
        }
    }

    /// Preview the current send transition without mutating endpoint state.
    pub(crate) fn preview_send_meta(
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
            .send_preview_meta_for_label::<ROLE>(
                target_label,
                |scope| self.selected_arm_for_scope(scope),
                |scope| self.preview_send_arm_for_scope(scope),
                |scope, label| self.lane_for_label_or_offer(scope, label),
            )
            .map_err(Self::map_send_preview_error)?;
        Ok(crate::endpoint::kernel::SendPreview::new(
            meta,
            cursor_index,
        ))
    }
}
