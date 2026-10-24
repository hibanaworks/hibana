use core::{cell::Cell, task::Poll};

use super::{
    CommitEventRow, CommitRow, OperationKernel, PublicActiveOp, SendError, SendMeta,
    SendPreviewError, SendResult, StateIndex, Transport, state_index_to_usize,
};

#[cfg(all(test, hibana_repo_tests))]
#[path = "send_preview_tests.rs"]
mod tests;

impl<'r, T> OperationKernel<'_, 'r, T>
where
    T: Transport + 'r,
{
    fn poll_send_entry_views(&self, meta: SendMeta, index: StateIndex) -> Poll<SendResult<()>> {
        let step = self
            .progress
            .cursor
            .relocatable_resident_lane_step_at_index(
                state_index_to_usize(index),
                usize::from(meta.lane),
            )
            .map_err(|_| SendError::PhaseInvariant)?;
        let event = CommitEventRow::new(
            meta.eff_index,
            meta.label,
            meta.origin,
            CommitRow::from_send_meta(meta),
            step,
        );
        match self.preflight_roll_entry_views(Some(event)) {
            Ok(_) => Poll::Ready(Ok(())),
            Err(crate::global::typestate::EventAdmissionError::OtherViewPending) => Poll::Pending,
            Err(crate::global::typestate::EventAdmissionError::Invariant) => {
                Poll::Ready(Err(SendError::PhaseInvariant))
            }
            Err(crate::global::typestate::EventAdmissionError::PredecessorPending) => {
                Poll::Ready(Err(SendError::OutOfOrder))
            }
        }
    }

    #[inline]
    fn map_send_preview_error(error: SendPreviewError) -> Poll<SendError> {
        match error {
            SendPreviewError::OtherViewPending => Poll::Pending,
            SendPreviewError::Invariant => Poll::Ready(SendError::PhaseInvariant),
            SendPreviewError::OutOfOrder => Poll::Ready(SendError::OutOfOrder),
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
            match self.operation.public_active_op {
                PublicActiveOp::Send => {}
                PublicActiveOp::BranchSend => {
                    let preview = self.preview_branch_send_meta(target_label, target_payload)?;
                    let (meta, index, _) = preview.into_parts();
                    return self.poll_send_entry_views(meta, index).map_ok(|()| preview);
                }
                PublicActiveOp::Poisoned
                | PublicActiveOp::Idle
                | PublicActiveOp::Recv
                | PublicActiveOp::Offer
                | PublicActiveOp::RouteBranch
                | PublicActiveOp::RestoredRouteBranch
                | PublicActiveOp::BranchRecv
                | PublicActiveOp::SendPayload => {
                    return Poll::Ready(Err(SendError::PhaseInvariant));
                }
            }
            let selector = crate::global::typestate::MessageSelector::new(
                self.public_header.role(),
                target_label,
                target_payload,
            );
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
                    selector,
                    (!endpoint.region.is_whole()).then(|| endpoint.region.bounds()),
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
            if !self.region.contains(meta.eff_index.dense_ordinal()) {
                return Poll::Ready(Err(SendError::PhaseInvariant));
            }
            match self.poll_send_entry_views(meta, cursor_index) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(result) => result?,
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
        self.reconcile_retirements();
        if let Some(kind) = self.session_fault() {
            result = Poll::Ready(Err(SendError::SessionFault(kind)));
        }
        result
    }
}
