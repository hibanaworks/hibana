use super::super::super::{
    OperationKernel, SelectedRouteCommitRowsRef, SendError, SendMeta, SendResult,
    SendRouteAuthority, Transport,
};
use crate::{global::const_dsl::DynamicRouteResolver, global::role_program::PackedLaneRange};

impl<'r, T> OperationKernel<'_, 'r, T>
where
    T: Transport + 'r,
{
    pub(crate) fn prepare_send_route_authority(
        &self,
        meta: &SendMeta,
        target_label: u8,
        preview_idx: usize,
    ) -> SendResult<SendRouteAuthority> {
        if meta.label != target_label {
            return Err(SendError::PhaseInvariant);
        }
        let rows = self.send_selected_route_rows_ref(meta, preview_idx)?;
        if rows.is_empty() {
            return Ok(SendRouteAuthority::none());
        }
        self.resolve_send_route_preview_rows(meta, rows, preview_idx)?;
        Ok(SendRouteAuthority::direct(meta.lane))
    }

    pub(crate) fn verify_send_route_authority(
        &self,
        meta: &SendMeta,
        target_label: u8,
        preview_idx: usize,
        authority: SendRouteAuthority,
    ) -> SendResult<()> {
        if meta.label != target_label {
            return Err(SendError::PhaseInvariant);
        }
        let current = self.send_selected_route_rows_ref(meta, preview_idx)?;
        match authority {
            SendRouteAuthority::None => {
                if current.is_empty() {
                    Ok(())
                } else {
                    Err(SendError::PhaseInvariant)
                }
            }
            SendRouteAuthority::Direct { lane } => {
                if lane != meta.lane || current.is_empty() || current.selected_lane() != Some(lane)
                {
                    return Err(SendError::PhaseInvariant);
                }
                Ok(())
            }
            SendRouteAuthority::MaterializedBranch => {
                if current.is_empty() {
                    return Err(SendError::PhaseInvariant);
                }
                self.verify_materialized_branch_route_rows(current)
            }
        }
    }

    fn send_selected_route_rows_ref(
        &self,
        meta: &SendMeta,
        preview_idx: usize,
    ) -> SendResult<SelectedRouteCommitRowsRef> {
        let Some(range) = self.send_route_commit_range(meta, preview_idx)? else {
            return Ok(SelectedRouteCommitRowsRef::EMPTY);
        };
        Ok(SelectedRouteCommitRowsRef::from_resident_range_for_lane(
            range, meta.lane,
        ))
    }

    fn send_route_commit_range(
        &self,
        meta: &SendMeta,
        preview_idx: usize,
    ) -> SendResult<Option<PackedLaneRange>> {
        let conflict = self.progress.cursor.event_conflict_for_index(preview_idx);
        if conflict.to_conflict().is_none() {
            return Ok(None);
        }
        if meta.selected_route_arm.is_none() {
            return Err(SendError::PhaseInvariant);
        }
        Ok(Some(
            self.progress
                .cursor
                .route_commit_range_for_conflict(conflict)
                .ok_or(SendError::PhaseInvariant)?,
        ))
    }

    fn resolve_send_route_preview_rows(
        &self,
        meta: &SendMeta,
        rows: SelectedRouteCommitRowsRef,
        preview_idx: usize,
    ) -> SendResult<()> {
        let fresh_start = self
            .fresh_route_start_for_rows(rows, preview_idx, Some(meta.lane))
            .map_err(|_| SendError::PhaseInvariant)?;
        let mut idx = 0usize;
        while idx < rows.len() {
            let route_row = rows
                .get(&self.progress.cursor, idx)
                .ok_or(SendError::PhaseInvariant)?;
            let scope_id = route_row.scope();
            let arm_index = route_row.selected_arm();
            // A completed containing arm does not start a new visit when the
            // candidate is a child's reentry rather than its own entry.
            if fresh_start.is_none_or(|start| idx < start) {
                if self.selected_arm_for_scope(scope_id) != Some(arm_index) {
                    return Err(SendError::PhaseInvariant);
                }
                idx += 1;
                continue;
            }
            let Some(resolver) = self.progress.cursor.route_scope_resolver(scope_id) else {
                idx += 1;
                continue;
            };
            self.resolve_dynamic_route_arm_for_send_preview(meta, arm_index, resolver)?;
            idx += 1;
        }
        Ok(())
    }

    fn resolve_dynamic_route_arm_for_send_preview(
        &self,
        meta: &SendMeta,
        arm_index: u8,
        resolver: DynamicRouteResolver,
    ) -> SendResult<()> {
        let scope_id = resolver.scope();
        let resolver_id = resolver.resolver_id();
        if let Some(selected) = self.selected_arm_for_scope(scope_id) {
            if self.reentrant_selected_arm_complete(scope_id, selected) {
                if meta.route_scope != scope_id {
                    let resolution =
                        self.resolve_dynamic_resolver_for_send_preview(meta.lane, resolver)?;
                    return if resolution.index() == arm_index {
                        Ok(())
                    } else {
                        Err(SendError::ResolverReject { resolver_id })
                    };
                }
                return if meta.selected_route_arm == Some(arm_index) {
                    Ok(())
                } else {
                    Err(SendError::PhaseInvariant)
                };
            }
            return if selected == arm_index {
                Ok(())
            } else {
                Err(SendError::ResolverReject { resolver_id })
            };
        }
        if meta.route_scope == scope_id {
            return if meta.selected_route_arm == Some(arm_index) {
                Ok(())
            } else {
                Err(SendError::PhaseInvariant)
            };
        }
        let resolution = self.resolve_dynamic_resolver_for_send_preview(meta.lane, resolver)?;
        if resolution.index() == arm_index {
            Ok(())
        } else {
            Err(SendError::ResolverReject { resolver_id })
        }
    }

    fn verify_materialized_branch_route_rows(
        &self,
        rows: SelectedRouteCommitRowsRef,
    ) -> SendResult<()> {
        let mut idx = 0usize;
        while idx < rows.len() {
            let route_row = rows
                .get(&self.progress.cursor, idx)
                .ok_or(SendError::PhaseInvariant)?;
            let scope_id = route_row.scope();
            let arm_index = route_row.selected_arm();
            if self
                .progress
                .cursor
                .route_scope_resolver(scope_id)
                .is_some()
                && let Some(selected) = self.selected_live_arm_for_scope(scope_id)
                && selected != arm_index
            {
                return Err(SendError::PhaseInvariant);
            }
            idx += 1;
        }
        Ok(())
    }

    pub(in crate::endpoint::kernel::core) fn resolve_dynamic_resolver_for_send_preview(
        &self,
        lane: u8,
        resolver: DynamicRouteResolver,
    ) -> SendResult<crate::session::cluster::core::DecisionArm> {
        let scope_id = resolver.scope();
        let cluster = self.context.session.cluster();
        self.context
            .session
            .rendezvous()
            .seal_session_membership(self.context.sid);
        let resolver_result = cluster.resolve_dynamic_resolver(
            self.context.public_rv,
            self.progress.cursor.program_ref(),
            resolver,
        );
        if let Some(kind) = self.session_fault() {
            return Err(SendError::SessionFault(kind));
        }
        match resolver_result {
            Ok(resolution) => {
                self.emit_dynamic_resolver_success_audit(
                    lane,
                    scope_id,
                    resolver.resolver_id(),
                    resolution,
                );
                Ok(resolution)
            }
            Err(crate::session::cluster::error::ClusterError::ResolverReject { resolver_id }) => {
                self.emit_dynamic_resolver_reject_audit(lane, scope_id, resolver_id);
                Err(SendError::ResolverReject { resolver_id })
            }
            Err(err) => Err(Self::send_error_from_cluster(err)),
        }
    }

    #[inline]
    fn send_error_from_cluster(err: crate::session::cluster::error::ClusterError) -> SendError {
        match err {
            crate::session::cluster::error::ClusterError::ResolverReject { resolver_id } => {
                SendError::ResolverReject { resolver_id }
            }
            crate::session::cluster::error::ClusterError::RendezvousMismatch { .. }
            | crate::session::cluster::error::ClusterError::RendezvousUnregistered { .. }
            | crate::session::cluster::error::ClusterError::RendezvousBusy { .. }
            | crate::session::cluster::error::ClusterError::SessionProgramMismatch { .. }
            | crate::session::cluster::error::ClusterError::SessionMembershipSealed { .. }
            | crate::session::cluster::error::ClusterError::ResourceExhausted { .. }
            | crate::session::cluster::error::ClusterError::DynamicResolverInvariant { .. } => {
                SendError::PhaseInvariant
            }
        }
    }
}
