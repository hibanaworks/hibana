use super::super::super::{
    CursorEndpoint, SelectedRouteCommitRowsRef, SendError, SendMeta, SendResult,
    SendRouteAuthority, Transport,
};
use crate::{global::const_dsl::DynamicRouteResolver, global::role_program::PackedLaneRange};

impl<'r, const ROLE: u8, T> CursorEndpoint<'r, ROLE, T>
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
        let audit_start = self.resolve_send_route_preview_rows(meta, rows)?;
        Ok(SendRouteAuthority::direct(meta.lane, audit_start))
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
            SendRouteAuthority::Direct {
                lane,
                audit_start: _,
            } => {
                if lane != meta.lane
                    || current.is_empty()
                    || current.packed_selected_lane() != Some(lane)
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
        let conflict = self.cursor.event_conflict_for_index(preview_idx);
        if conflict.to_conflict().is_none() {
            return Ok(None);
        }
        if meta.selected_route_arm.is_none() {
            return Err(SendError::PhaseInvariant);
        }
        Ok(Some(
            self.cursor
                .route_commit_range_for_conflict(conflict)
                .ok_or(SendError::PhaseInvariant)?,
        ))
    }

    fn resolve_send_route_preview_rows(
        &self,
        meta: &SendMeta,
        rows: SelectedRouteCommitRowsRef,
    ) -> SendResult<u16> {
        if rows.len() > u16::MAX as usize {
            return Err(SendError::PhaseInvariant);
        }
        let mut audit_start = rows.len() as u16;
        let mut idx = 0usize;
        while idx < rows.len() {
            let route_row = rows
                .get(&self.cursor, idx)
                .ok_or(SendError::PhaseInvariant)?;
            let scope_id = route_row.scope();
            let arm_index = route_row.selected_arm();
            let Some(resolver) = self.cursor.route_scope_resolver(scope_id) else {
                idx += 1;
                continue;
            };
            if self.selected_arm_for_scope(scope_id).is_none() && audit_start as usize == rows.len()
            {
                audit_start = idx as u16;
            }
            if let Some(selected) = self.selected_arm_for_scope(scope_id)
                && self.reentrant_selected_arm_complete(scope_id, selected)
                && audit_start as usize == rows.len()
            {
                audit_start = idx as u16;
            }
            self.resolve_dynamic_route_arm_for_send_preview(meta, arm_index, resolver)?;
            idx += 1;
        }
        Ok(audit_start)
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
                .get(&self.cursor, idx)
                .ok_or(SendError::PhaseInvariant)?;
            let scope_id = route_row.scope();
            let arm_index = route_row.selected_arm();
            if self.cursor.route_scope_resolver(scope_id).is_some()
                && let Some(selected) = self.selected_arm_for_scope(scope_id)
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
        let cluster = self.session.cluster();
        let resolver_result = cluster.resolve_dynamic_resolver(
            self.rendezvous_id(),
            self.cursor.program_ref(),
            resolver,
        );
        if let Some(kind) = self.session_fault() {
            return Err(SendError::SessionFault(kind));
        }
        match resolver_result {
            Ok(resolution) => Ok(resolution),
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
            | crate::session::cluster::error::ClusterError::ResourceExhausted { .. }
            | crate::session::cluster::error::ClusterError::DynamicResolverInvariant { .. } => {
                SendError::PhaseInvariant
            }
        }
    }
}
