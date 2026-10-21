use super::{
    Arm, BranchKind, OperationKernel, ScopeId, SendError, SendResult, Transport,
    state_index_to_usize,
};

impl<'r, T> OperationKernel<'_, 'r, T>
where
    T: Transport + 'r,
{
    #[inline(never)]
    pub(in crate::endpoint::kernel::core) fn preview_branch_send_meta(
        &mut self,
        target_label: u8,
        target_payload: crate::eff::EventPayload,
    ) -> SendResult<crate::endpoint::kernel::SendPreview> {
        let Some(branch) = self.operation.public_route_branch.as_ref() else {
            return Err(SendError::PhaseInvariant);
        };
        let branch_meta = branch.branch_meta;
        let has_offered_frame = branch.offered_frame.is_some();

        if branch_meta.kind != BranchKind::ArmSend || has_offered_frame {
            return Err(SendError::PhaseInvariant);
        }
        if target_label != branch_meta.label {
            return Err(SendError::LabelMismatch {
                expected: branch_meta.label,
                actual: target_label,
            });
        }

        let cursor_index = state_index_to_usize(branch_meta.cursor_index);
        if self.progress.cursor.logical_role_at(cursor_index) != Some(self.public_header.role())
            || !self.permits_event(cursor_index)
        {
            return Err(SendError::PhaseInvariant);
        }
        let Some(mut meta) = self.progress.cursor.try_send_meta_at(cursor_index) else {
            return Err(SendError::PhaseInvariant);
        };
        if meta.payload_kind != target_payload {
            return Err(SendError::PayloadMismatch {
                expected: meta.payload_kind,
                actual: target_payload,
            });
        }
        if meta.label != branch_meta.label
            || meta.frame_label != branch_meta.frame_label
            || meta.lane != branch_meta.lane_wire
            || meta.route_scope != branch_meta.scope_id
        {
            return Err(SendError::PhaseInvariant);
        }
        if meta
            .route_arm
            .is_some_and(|arm| arm != branch_meta.selected_arm)
        {
            return Err(SendError::PhaseInvariant);
        }
        meta.selected_route_arm = Some(branch_meta.selected_arm);

        Ok(crate::endpoint::kernel::SendPreview::materialized_branch(
            meta,
            branch_meta.cursor_index,
        ))
    }

    #[inline]
    fn preview_dynamic_resolver_arm_for_scope(
        &self,
        scope_id: ScopeId,
        lane: u8,
    ) -> SendResult<Option<u8>> {
        let Some(resolver) = self.progress.cursor.route_scope_resolver(scope_id) else {
            return Ok(None);
        };
        let arm = self.resolve_dynamic_resolver_for_send_preview(lane, resolver)?;
        Ok(Some(arm.index()))
    }

    #[inline]
    pub(in crate::endpoint::kernel::core) fn preview_controller_send_arm_for_scope(
        &self,
        scope_id: ScopeId,
        lane: u8,
    ) -> SendResult<Option<u8>> {
        if !self.is_route_controller(scope_id) {
            return Ok(None);
        }
        if let Some(arm) = self.selected_live_arm_for_scope(scope_id) {
            return Ok(Some(arm));
        }
        // A parked offer retains the decision before its first event commits.
        // The enclosing actor borrow keeps every operation slot stable here.
        let mut materialized = None;
        for slot in self.operations.iter().flatten() {
            let operation = if core::ptr::eq(slot.state.get(), self.operation) {
                &*self.operation
            } else {
                // SAFETY: this distinct slot is read under the actor's exclusive
                // kernel borrow. Child futures retain no slot borrow while Pending.
                unsafe { &*slot.state.get() }
            };
            let Some(branch) = operation.public_route_branch.as_ref() else {
                continue;
            };
            let selected = if branch.branch_meta.scope_id == scope_id {
                Some(branch.branch_meta.selected_arm)
            } else {
                self.progress
                    .cursor
                    .route_scope_conflict_arm_for_scope(branch.branch_meta.scope_id, scope_id)
            };
            if let Some(arm) = selected {
                if materialized.is_some_and(|selected| selected != arm) {
                    return Err(SendError::PhaseInvariant);
                }
                materialized = Some(arm);
            }
        }
        if materialized.is_some() {
            return Ok(materialized);
        }
        self.preview_dynamic_resolver_arm_for_scope(scope_id, lane)
    }

    #[inline]
    pub(in crate::endpoint::kernel::core) fn preview_send_arm_for_scope(
        &self,
        scope_id: ScopeId,
    ) -> SendResult<Option<u8>> {
        if scope_id.is_none() {
            return Ok(None);
        }
        if let Some(arm) = self.selected_arm_for_scope(scope_id)
            && !self.reentrant_selected_arm_complete(scope_id, arm)
        {
            return Ok(Some(arm));
        }
        if self
            .progress
            .cursor
            .route_scope_resolver(scope_id)
            .is_some()
        {
            return Ok(None);
        }
        let offer_lanes = self.offer_lane_set_for_scope(scope_id);
        if offer_lanes
            .first_set(self.progress.cursor.logical_lane_count())
            .is_none()
        {
            return Ok(None);
        }
        Ok(self.poll_arm_from_ready_mask(scope_id).map(Arm::as_u8))
    }
}
