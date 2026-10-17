use core::{cell::Cell, task::Poll};

use super::{
    OperationKernel, PublicActiveOp, SendError, SendPreviewError, SendResult, Transport,
    state_index_to_usize,
};

impl<'r, T> OperationKernel<'_, 'r, T>
where
    T: Transport + 'r,
{
    #[inline]
    fn map_send_preview_error(error: SendPreviewError) -> Poll<SendError> {
        match error {
            SendPreviewError::OtherViewPending => Poll::Pending,
            SendPreviewError::Invariant => Poll::Ready(SendError::PhaseInvariant),
            SendPreviewError::LabelMismatch { expected, actual } => {
                Poll::Ready(SendError::LabelMismatch { expected, actual })
            }
            SendPreviewError::PayloadMismatch { expected, actual } => {
                Poll::Ready(SendError::PayloadMismatch { expected, actual })
            }
        }
    }

    /// Preview the current send transition without advancing the cursor.
    ///
    /// A failed public preview is terminal. In particular, resolver rejection
    /// cannot be retried as a different route decision for the same session.
    #[inline(never)]
    pub(crate) fn poll_send_preview(
        &mut self,
        target_label: u8,
        target_payload: crate::eff::EventPayload,
    ) -> Poll<SendResult<crate::endpoint::kernel::SendPreview>> {
        let mut result = (|| {
            if let Some(kind) = self.session_fault() {
                return Poll::Ready(Err(SendError::SessionFault(kind)));
            }
            match self.io.public_active_op {
                PublicActiveOp::Send => {}
                PublicActiveOp::BranchSend => {
                    return Poll::Ready(
                        self.preview_branch_send_meta(target_label, target_payload),
                    );
                }
                PublicActiveOp::Poisoned
                | PublicActiveOp::Idle
                | PublicActiveOp::Recv
                | PublicActiveOp::Offer
                | PublicActiveOp::RouteBranch
                | PublicActiveOp::RestoredRouteBranch
                | PublicActiveOp::BranchRecv => return Poll::Ready(Err(SendError::PhaseInvariant)),
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
                return Poll::Ready(Err(error));
            }
            let (meta, cursor_index) = match preview_result {
                Ok(preview) => preview,
                Err(error) => match Self::map_send_preview_error(error) {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(error) => return Poll::Ready(Err(error)),
                },
            };
            if meta.payload_kind != target_payload {
                return Poll::Ready(Err(SendError::PayloadMismatch {
                    expected: meta.payload_kind,
                    actual: target_payload,
                }));
            }
            let route_authority = self.prepare_send_route_authority(
                &meta,
                target_label,
                state_index_to_usize(cursor_index),
            )?;
            Poll::Ready(Ok(
                crate::endpoint::kernel::SendPreview::with_route_authority(
                    meta,
                    cursor_index,
                    route_authority,
                ),
            ))
        })();
        self.reconcile_retired_views();
        if let Some(kind) = self.session_fault() {
            result = Poll::Ready(Err(SendError::SessionFault(kind)));
        }
        result
    }
}
