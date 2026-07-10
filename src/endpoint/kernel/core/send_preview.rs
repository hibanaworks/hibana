use core::cell::Cell;

use super::{
    BranchKind, CursorEndpoint, PublicActiveOp, SendError, SendPreviewError, SendResult, Transport,
    state_index_to_usize,
};
use crate::endpoint::carrier::WaiterTransfer;

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
    fn fail_branch_send_preview(
        &mut self,
        error: SendError,
        waiters: &mut WaiterTransfer,
    ) -> SendError {
        self.clear_endpoint_waiter(waiters);
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
        waiters: &mut WaiterTransfer,
    ) -> SendResult<crate::endpoint::kernel::SendPreview> {
        if let Some(kind) = self.session_fault() {
            self.clear_endpoint_waiter(waiters);
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
        let has_offered_frame = branch.offered_frame.is_some();

        if branch_meta.kind != BranchKind::ArmSend || has_offered_frame {
            return Err(self.fail_branch_send_preview(SendError::PhaseInvariant, waiters));
        }
        if target_label != branch_meta.label {
            return Err(self.fail_branch_send_preview(
                SendError::LabelMismatch {
                    expected: branch_meta.label,
                    actual: target_label,
                },
                waiters,
            ));
        }

        let cursor_index = state_index_to_usize(branch_meta.cursor_index);
        let Some(mut meta) = self.cursor.try_send_meta_at(cursor_index) else {
            return Err(self.fail_branch_send_preview(SendError::PhaseInvariant, waiters));
        };
        if meta.label != branch_meta.label
            || meta.frame_label != branch_meta.frame_label
            || meta.lane != branch_meta.lane_wire
            || meta.route_scope != branch_meta.scope_id
        {
            return Err(self.fail_branch_send_preview(SendError::PhaseInvariant, waiters));
        }
        if meta
            .route_arm
            .is_some_and(|arm| arm != branch_meta.selected_arm)
        {
            return Err(self.fail_branch_send_preview(SendError::PhaseInvariant, waiters));
        }
        meta.selected_route_arm = Some(branch_meta.selected_arm);

        Ok(crate::endpoint::kernel::SendPreview::materialized_branch(
            meta,
            branch_meta.cursor_index,
        ))
    }

    /// Preview the current send transition without mutating endpoint state.
    #[inline(never)]
    pub(crate) fn preview_send_meta(
        &mut self,
        target_label: u8,
        waiters: &mut WaiterTransfer,
    ) -> SendResult<crate::endpoint::kernel::SendPreview> {
        match self.public_active_op {
            PublicActiveOp::Idle => {
                if let Some(kind) = self.session_fault() {
                    return Err(SendError::SessionFault(kind));
                }
            }
            PublicActiveOp::RouteBranch => {
                return self.preview_branch_send_meta(target_label, waiters);
            }
            _ => {
                self.public_op_busy_fault();
                return Err(SendError::PhaseInvariant);
            }
        }
        let preview_error = Cell::new(None::<SendError>);
        let preview_result = {
            let endpoint = &*self;
            let mut committed_arm_for_scope = |scope| endpoint.selected_arm_for_scope(scope);
            let mut preview_controller_arm_for_scope = |scope| {
                let lane = endpoint.lane_for_label_or_offer(scope, target_label);
                match endpoint.preview_controller_send_arm_for_scope(scope, lane) {
                    Ok(arm) => arm,
                    Err(error) => {
                        preview_error.set(Some(error));
                        None
                    }
                }
            };
            let mut selected_arm_for_scope =
                |scope| match endpoint.preview_send_arm_for_scope(scope) {
                    Ok(arm) => arm,
                    Err(error) => {
                        preview_error.set(Some(error));
                        None
                    }
                };
            let mut lane_for_label_or_offer =
                |scope, label| endpoint.lane_for_label_or_offer(scope, label);
            endpoint.cursor.send_preview_meta_for_label::<ROLE>(
                target_label,
                &mut committed_arm_for_scope,
                &mut preview_controller_arm_for_scope,
                &mut selected_arm_for_scope,
                &mut lane_for_label_or_offer,
            )
        };
        if let Some(error) = preview_error.get() {
            return Err(error);
        }
        let (meta, cursor_index) = preview_result.map_err(Self::map_send_preview_error)?;
        let route_authority = self.prepare_send_route_authority(
            &meta,
            target_label,
            state_index_to_usize(cursor_index),
        )?;
        Ok(crate::endpoint::kernel::SendPreview::with_route_authority(
            meta,
            cursor_index,
            route_authority,
        ))
    }
}
