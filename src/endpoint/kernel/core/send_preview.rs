use super::{
    Arm, BranchKind, CursorEndpoint, PublicActiveOp, ScopeId, SendError, SendPreviewError,
    SendResult, Transport, state_index_to_usize,
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

    #[inline]
    fn fail_branch_send_preview(&mut self, error: SendError) -> SendError {
        self.clear_session_waiter();
        if let Some(branch) = self.public_route_branch.take() {
            branch.discard_terminal();
        }
        self.public_active_op = PublicActiveOp::Poisoned;
        self.poison_for_send_error(&error);
        error
    }

    #[inline(never)]
    fn preview_branch_send_meta(
        &mut self,
        target_label: u8,
    ) -> SendResult<crate::endpoint::kernel::SendPreview> {
        if let Some(kind) = self.session_fault() {
            self.clear_session_waiter();
            if let Some(branch) = self.public_route_branch.take() {
                branch.discard_terminal();
            }
            self.clear_public_op_if_current(PublicActiveOp::RouteBranch);
            return Err(SendError::SessionFault(kind));
        }

        let Some(branch) = self.public_route_branch.as_ref() else {
            self.public_op_busy_fault();
            return Err(SendError::PhaseInvariant);
        };
        let branch_meta = branch.branch_meta;
        let branch_label = branch.label;
        let has_offered_frame = branch.offered_frame.is_some();

        if branch_meta.kind != BranchKind::ArmSendHint || has_offered_frame {
            return Err(self.fail_branch_send_preview(SendError::PhaseInvariant));
        }
        if target_label != branch_label {
            return Err(self.fail_branch_send_preview(SendError::LabelMismatch {
                expected: branch_label,
                actual: target_label,
            }));
        }

        let cursor_index = state_index_to_usize(branch_meta.cursor_index);
        let Some(mut meta) = self.cursor.try_send_meta_at(cursor_index) else {
            return Err(self.fail_branch_send_preview(SendError::PhaseInvariant));
        };
        if meta.label != branch_meta.label
            || meta.frame_label != branch_meta.frame_label
            || meta.lane != branch_meta.lane_wire
            || meta.route_scope != branch_meta.scope_id
        {
            return Err(self.fail_branch_send_preview(SendError::PhaseInvariant));
        }
        if meta
            .route_arm
            .is_some_and(|arm| arm != branch_meta.selected_arm)
        {
            return Err(self.fail_branch_send_preview(SendError::PhaseInvariant));
        }
        meta.selected_route_arm = Some(branch_meta.selected_arm);

        Ok(crate::endpoint::kernel::SendPreview::new(
            meta,
            branch_meta.cursor_index,
        ))
    }

    /// Preview the current send transition without mutating endpoint state.
    pub(crate) fn preview_send_meta(
        &mut self,
        target_label: u8,
    ) -> SendResult<crate::endpoint::kernel::SendPreview> {
        match self.public_active_op {
            PublicActiveOp::Idle => {
                if let Some(kind) = self.session_fault() {
                    return Err(SendError::SessionFault(kind));
                }
            }
            PublicActiveOp::RouteBranch => return self.preview_branch_send_meta(target_label),
            _ => {
                self.public_op_busy_fault();
                return Err(SendError::PhaseInvariant);
            }
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
