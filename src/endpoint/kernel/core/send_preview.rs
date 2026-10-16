use core::cell::Cell;

use super::{
    OperationKernel, PublicActiveOp, SendError, SendPreviewError, SendResult, Transport,
    state_index_to_usize,
};
use crate::endpoint::carrier::WaiterTransfer;

impl<'r, T> OperationKernel<'_, 'r, T>
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
            SendPreviewError::PayloadMismatch { expected, actual } => {
                SendError::PayloadMismatch { expected, actual }
            }
        }
    }

    /// Preview the current send transition without advancing the cursor.
    ///
    /// A failed public preview is terminal. In particular, resolver rejection
    /// cannot be retried as a different route decision for the same session.
    #[inline(never)]
    pub(crate) fn preview_send_meta(
        &mut self,
        target_label: u8,
        target_payload: crate::eff::EventPayload,
        waiters: &mut WaiterTransfer,
    ) -> SendResult<crate::endpoint::kernel::SendPreview> {
        let mut result = (|| {
            if let Some(kind) = self.session_fault() {
                return Err(SendError::SessionFault(kind));
            }
            match self.io.public_active_op {
                PublicActiveOp::Idle => {}
                PublicActiveOp::RouteBranch => {
                    return self.preview_branch_send_meta(target_label, target_payload);
                }
                PublicActiveOp::Poisoned
                | PublicActiveOp::Send
                | PublicActiveOp::Recv
                | PublicActiveOp::Offer
                | PublicActiveOp::RestoredRouteBranch
                | PublicActiveOp::BranchRecv
                | PublicActiveOp::BranchSend => return Err(SendError::PhaseInvariant),
            }
            let preview_error = Cell::new(None::<SendError>);
            let preview_result = {
                let endpoint = &*self;
                let mut committed_arm_for_scope = |scope| endpoint.selected_arm_for_scope(scope);
                let mut preview_controller_arm_for_scope = |scope| {
                    let lane =
                        endpoint.lane_for_contract_or_offer(scope, target_label, target_payload);
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
                let mut lane_for_contract_or_offer = |scope, label, schema| {
                    endpoint.lane_for_contract_or_offer(scope, label, schema)
                };
                endpoint.progress.cursor.send_preview_meta_for_contract(
                    crate::global::typestate::MessageSelector::new(
                        endpoint.public_header.role(),
                        target_label,
                        target_payload,
                    ),
                    &mut committed_arm_for_scope,
                    &mut preview_controller_arm_for_scope,
                    &mut selected_arm_for_scope,
                    &mut lane_for_contract_or_offer,
                )
            };
            if let Some(error) = preview_error.get() {
                return Err(error);
            }
            let (meta, cursor_index) = preview_result.map_err(Self::map_send_preview_error)?;
            if meta.payload_kind != target_payload {
                return Err(SendError::PayloadMismatch {
                    expected: meta.payload_kind,
                    actual: target_payload,
                });
            }
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
        })();
        self.reconcile_retired_views();
        if let Some(kind) = self.session_fault() {
            result = Err(SendError::SessionFault(kind));
        }
        if let Err(error) = &result {
            self.clear_endpoint_waiter(waiters);
            self.poison_for_send_error(error);
        }
        result
    }
}
