mod audit;
mod select;

use super::super::{
    CursorEndpoint, SelectedRouteCommitRow, SendError, SendMeta, SendResult, Transport,
};
use crate::global::typestate::PackedEventConflict;

impl<'r, const ROLE: u8, T> CursorEndpoint<'r, ROLE, T>
where
    T: Transport + 'r,
{
    pub(crate) fn decide_dynamic_resolvers_for_send(
        &mut self,
        meta: &SendMeta,
        target_label: u8,
        preview_idx: usize,
    ) -> SendResult<()> {
        if meta.label != target_label {
            return Err(SendError::PhaseInvariant);
        }
        let Some(selected_arm) = meta.selected_route_arm else {
            return Ok(());
        };
        let conflict = if meta.route_scope.is_none() {
            self.cursor.event_conflict_for_index(preview_idx)
        } else {
            PackedEventConflict::route_arm(meta.route_scope, selected_arm)
        };
        let range = self
            .cursor
            .route_commit_range_for_conflict(conflict, Some(selected_arm))
            .ok_or(SendError::PhaseInvariant)?;
        let mut idx = 0usize;
        while idx < range.len() {
            let route_row = self
                .cursor
                .route_commit_row_at(range, idx)
                .and_then(SelectedRouteCommitRow::from_resident_conflict)
                .ok_or(SendError::PhaseInvariant)?;
            let scope_id = route_row.scope();
            let arm_index = route_row.selected_arm();
            let resolver = self
                .cursor
                .route_scope_controller_resolver(scope_id)
                .map_or(
                    crate::global::const_dsl::RouteResolver::Intrinsic,
                    |(resolver, _)| resolver,
                );
            if let crate::global::const_dsl::RouteResolver::Dynamic { .. } = resolver {
                self.decide_dynamic_route_arm(meta.lane, scope_id, arm_index, resolver)?;
            }
            idx += 1;
        }
        Ok(())
    }

    fn decide_dynamic_route_arm(
        &mut self,
        lane: u8,
        scope_id: crate::global::const_dsl::ScopeId,
        arm_index: u8,
        resolver: crate::global::const_dsl::RouteResolver,
    ) -> SendResult<()> {
        let crate::global::const_dsl::RouteResolver::Dynamic {
            resolver_id,
            scope: resolver_scope,
        } = resolver
        else {
            return Err(SendError::PhaseInvariant);
        };

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

        let cluster = self.session.cluster();
        let resolution =
            match cluster.resolve_dynamic_resolver(self.rendezvous_id(), scope_id, resolver_id) {
                Ok(resolution) => {
                    self.emit_dynamic_resolver_success_audit(
                        lane,
                        scope_id,
                        resolver_id,
                        resolution,
                    );
                    resolution
                }
                Err(crate::session::cluster::error::ClusterError::ResolverReject {
                    resolver_id,
                }) => {
                    self.emit_dynamic_resolver_reject_audit(lane, scope_id, resolver_id);
                    return Err(SendError::ResolverReject { resolver_id });
                }
                Err(err) => return Err(Self::send_error_from_cluster(err)),
            };

        if resolution.index() == arm_index {
            Ok(())
        } else {
            Err(SendError::ResolverReject { resolver_id })
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
