use super::{
    Arm, BranchKind, CursorEndpoint, ScopeId, SendError, SendResult, Transport,
    state_index_to_usize,
};

impl<'r, const ROLE: u8, T> CursorEndpoint<'r, ROLE, T>
where
    T: Transport + 'r,
{
    #[inline(never)]
    pub(in crate::endpoint::kernel::core) fn preview_branch_send_meta(
        &mut self,
        target_label: u8,
        target_schema: u32,
    ) -> SendResult<crate::endpoint::kernel::SendPreview> {
        let Some(branch) = self.public_route_branch.as_ref() else {
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
        let Some(mut meta) = self.cursor.try_send_meta_at(cursor_index) else {
            return Err(SendError::PhaseInvariant);
        };
        if meta.payload_schema != target_schema {
            return Err(SendError::SchemaMismatch {
                expected: meta.payload_schema,
                actual: target_schema,
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
        let Some(resolver) = self.cursor.route_scope_resolver(scope_id) else {
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
        if scope_id.is_none() {
            return Ok(None);
        }
        if let Some(arm) = self.selected_live_arm_for_scope(scope_id) {
            return Ok(Some(arm));
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
        if self.cursor.route_scope_resolver(scope_id).is_some() {
            return Ok(None);
        }
        let offer_lanes = self.offer_lane_set_for_scope(scope_id);
        if offer_lanes
            .first_set(self.cursor.logical_lane_count())
            .is_none()
        {
            return Ok(None);
        }
        Ok(self
            .peek_live_scope_ack(scope_id)
            .map(|token| token.arm().as_u8())
            .or_else(|| self.poll_arm_from_ready_mask(scope_id).map(Arm::as_u8)))
    }
}
