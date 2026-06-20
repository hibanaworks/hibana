use super::super::super::{
    CursorEndpoint, SelectedRouteCommitRowsRef, SendError, SendMeta, SendResult,
    SendRouteAuthority, Transport,
};
use crate::{
    global::{const_dsl::RouteResolver, role_program::PackedLaneRange},
    global::{const_dsl::ScopeId, typestate::PackedEventConflict},
};

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
        let Some(selected_arm) = meta.selected_route_arm else {
            return Ok(None);
        };
        let conflict = if meta.route_scope.is_none() {
            self.cursor.event_conflict_for_index(preview_idx)
        } else {
            PackedEventConflict::route_arm(meta.route_scope, selected_arm)
        };
        Ok(Some(
            self.cursor
                .route_commit_range_for_conflict(conflict, Some(selected_arm))
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
            let Some((RouteResolver::Dynamic { resolver_id, scope }, _)) =
                self.cursor.route_scope_controller_resolver(scope_id)
            else {
                idx += 1;
                continue;
            };
            if self.selected_arm_for_scope(scope_id).is_none() && audit_start as usize == rows.len()
            {
                audit_start = idx as u16;
            }
            self.resolve_dynamic_route_arm_for_send_preview(
                meta,
                scope_id,
                arm_index,
                scope,
                resolver_id,
            )?;
            idx += 1;
        }
        Ok(audit_start)
    }

    fn resolve_dynamic_route_arm_for_send_preview(
        &self,
        meta: &SendMeta,
        scope_id: ScopeId,
        arm_index: u8,
        resolver_scope: ScopeId,
        resolver_id: u16,
    ) -> SendResult<()> {
        if scope_id.is_none() || scope_id != resolver_scope {
            return Err(SendError::PhaseInvariant);
        }
        if let Some(selected) = self.selected_arm_for_scope(scope_id) {
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
        let resolution =
            self.resolve_dynamic_resolver_for_send_preview(meta.lane, scope_id, resolver_id)?;
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
            if let Some((RouteResolver::Dynamic { scope, .. }, _)) =
                self.cursor.route_scope_controller_resolver(scope_id)
            {
                if scope_id.is_none() || scope_id != scope {
                    return Err(SendError::PhaseInvariant);
                }
                if let Some(selected) = self.selected_arm_for_scope(scope_id)
                    && selected != arm_index
                {
                    return Err(SendError::PhaseInvariant);
                }
            }
            idx += 1;
        }
        Ok(())
    }

    pub(in crate::endpoint::kernel::core) fn resolve_dynamic_resolver_for_send_preview(
        &self,
        lane: u8,
        scope_id: ScopeId,
        resolver_id: u16,
    ) -> SendResult<crate::session::cluster::core::DecisionArm> {
        let cluster = self.session.cluster();
        match cluster.resolve_dynamic_resolver(self.rendezvous_id(), scope_id, resolver_id) {
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
            | crate::session::cluster::error::ClusterError::ResourceExhausted { .. }
            | crate::session::cluster::error::ClusterError::DynamicResolverInvariant { .. } => {
                SendError::PhaseInvariant
            }
        }
    }
}
