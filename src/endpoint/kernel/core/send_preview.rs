use core::cell::Cell;

use super::{
    BranchKind, CursorEndpoint, PublicActiveOp, ResolverDecisionProofs, SendError,
    SendPreviewError, SendResult, Transport, state_index_to_usize,
};

impl<'r, const ROLE: u8, T> CursorEndpoint<'r, ROLE, T>
where
    T: Transport + 'r,
{
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

        Ok(crate::endpoint::kernel::SendPreview::materialized_branch(
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
        let preview_error = Cell::new(None::<SendError>);
        let mut resolver_decisions = ResolverDecisionProofs::empty();
        let preview_result = self.cursor.send_preview_meta_for_label::<ROLE>(
            target_label,
            |scope| self.selected_arm_for_scope(scope),
            |scope| {
                let lane = self.lane_for_label_or_offer(scope, target_label);
                match self.preview_controller_send_arm_for_scope(
                    scope,
                    lane,
                    &mut resolver_decisions,
                ) {
                    Ok(arm) => arm,
                    Err(error) => {
                        preview_error.set(Some(error));
                        None
                    }
                }
            },
            |scope| match self.preview_send_arm_for_scope(scope) {
                Ok(arm) => arm,
                Err(error) => {
                    preview_error.set(Some(error));
                    None
                }
            },
            |scope, label| self.lane_for_label_or_offer(scope, label),
        );
        if let Some(error) = preview_error.get() {
            return Err(error);
        }
        let (meta, cursor_index) = preview_result.map_err(Self::map_send_preview_error)?;
        self.collect_dynamic_resolver_send_preview(
            &meta,
            target_label,
            state_index_to_usize(cursor_index),
            &mut resolver_decisions,
        )?;
        Ok(
            crate::endpoint::kernel::SendPreview::with_resolver_decisions(
                meta,
                cursor_index,
                resolver_decisions,
            ),
        )
    }
}
