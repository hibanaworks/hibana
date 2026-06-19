use super::super::super::{
    CursorEndpoint, ResolverDecisionProof, ResolverDecisionProofs, SelectedRouteCommitRow,
    SendError, SendMeta, SendResult, Transport,
};
use crate::global::typestate::PackedEventConflict;

impl<'r, const ROLE: u8, T> CursorEndpoint<'r, ROLE, T>
where
    T: Transport + 'r,
{
    pub(crate) fn collect_dynamic_resolver_send_preview(
        &self,
        meta: &SendMeta,
        target_label: u8,
        preview_idx: usize,
        proofs: &mut ResolverDecisionProofs,
    ) -> SendResult<()> {
        if meta.label != target_label {
            return Err(SendError::PhaseInvariant);
        }
        let Some(range) = self.send_route_commit_range(meta, preview_idx)? else {
            return Ok(());
        };
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
                self.collect_dynamic_route_arm_preview(
                    meta.lane, scope_id, arm_index, resolver, proofs,
                )?;
            }
            idx += 1;
        }
        Ok(())
    }

    pub(crate) fn verify_dynamic_resolver_send_preview(
        &mut self,
        meta: &SendMeta,
        target_label: u8,
        preview_idx: usize,
        proofs: ResolverDecisionProofs,
    ) -> SendResult<()> {
        if meta.label != target_label {
            return Err(SendError::PhaseInvariant);
        }
        let Some(range) = self.send_route_commit_range(meta, preview_idx)? else {
            return if proofs.is_empty() {
                Ok(())
            } else {
                Err(SendError::PhaseInvariant)
            };
        };
        let mut consumed = 0u8;
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
            if let crate::global::const_dsl::RouteResolver::Dynamic { .. } = resolver
                && let Some(proof_index) = self.verify_dynamic_route_arm_preview(
                    meta.lane, scope_id, arm_index, resolver, proofs,
                )?
            {
                if proof_index < u8::BITS as usize {
                    consumed |= 1u8 << proof_index;
                } else {
                    return Err(SendError::PhaseInvariant);
                }
            }
            idx += 1;
        }
        if proofs.len() >= u8::BITS as usize {
            return Err(SendError::PhaseInvariant);
        }
        let expected = if proofs.is_empty() {
            0
        } else {
            (1u8 << proofs.len()) - 1
        };
        if consumed == expected {
            Ok(())
        } else {
            Err(SendError::PhaseInvariant)
        }
    }

    fn send_route_commit_range(
        &self,
        meta: &SendMeta,
        preview_idx: usize,
    ) -> SendResult<Option<crate::global::role_program::PackedLaneRange>> {
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

    fn collect_dynamic_route_arm_preview(
        &self,
        lane: u8,
        scope_id: crate::global::const_dsl::ScopeId,
        arm_index: u8,
        resolver: crate::global::const_dsl::RouteResolver,
        proofs: &mut ResolverDecisionProofs,
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
        if proofs
            .match_index(scope_id, resolver_id, arm_index, lane)
            .is_some()
        {
            return Ok(());
        }
        if proofs.site_index(scope_id, resolver_id).is_some() {
            return Err(SendError::PhaseInvariant);
        }
        let resolution =
            self.resolve_dynamic_resolver_for_send_preview(lane, scope_id, resolver_id)?;
        if resolution.index() == arm_index {
            proofs.push(ResolverDecisionProof::new(
                scope_id,
                resolver_id,
                arm_index,
                lane,
            ))
        } else {
            Err(SendError::ResolverReject { resolver_id })
        }
    }

    fn verify_dynamic_route_arm_preview(
        &mut self,
        lane: u8,
        scope_id: crate::global::const_dsl::ScopeId,
        arm_index: u8,
        resolver: crate::global::const_dsl::RouteResolver,
        proofs: ResolverDecisionProofs,
    ) -> SendResult<Option<usize>> {
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
                Ok(None)
            } else {
                Err(SendError::ResolverReject { resolver_id })
            };
        }
        let proof_index = proofs
            .match_index(scope_id, resolver_id, arm_index, lane)
            .ok_or(SendError::PhaseInvariant)?;
        let proof = proofs.get(proof_index).ok_or(SendError::PhaseInvariant)?;
        self.emit_dynamic_resolver_success_audit(
            proof.lane(),
            proof.scope(),
            proof.resolver_id(),
            proof.decision_arm(),
        );
        Ok(Some(proof_index))
    }

    pub(in crate::endpoint::kernel::core) fn resolve_dynamic_resolver_for_send_preview(
        &self,
        lane: u8,
        scope_id: crate::global::const_dsl::ScopeId,
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
