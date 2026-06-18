mod audit;
mod select;

use super::super::{CursorEndpoint, EventSemanticKind, SendError, SendMeta, SendResult, Transport};

impl<'r, const ROLE: u8, T> CursorEndpoint<'r, ROLE, T>
where
    T: Transport + 'r,
{
    pub(crate) fn decide_dynamic_resolver(
        &mut self,
        meta: &SendMeta,
        target_label: u8,
    ) -> SendResult<()> {
        let resolver = if meta.route_scope.is_none() {
            meta.resolver()
        } else {
            self.cursor
                .route_scope_controller_resolver(meta.route_scope)
                .map_or(
                    crate::global::const_dsl::RouteResolver::Intrinsic,
                    |(resolver, _)| resolver,
                )
        };
        let crate::global::const_dsl::RouteResolver::Dynamic { .. } = resolver else {
            return Ok(());
        };
        match meta.semantic {
            EventSemanticKind::DecisionArm => {
                self.decide_dynamic_route_arm(meta, target_label, resolver)
            }
            EventSemanticKind::ProtocolEvent => {
                if !meta.route_scope.is_none() {
                    self.cursor
                        .route_scope_controller_resolver(meta.route_scope)
                        .ok_or(SendError::PhaseInvariant)?;
                }
                self.decide_dynamic_route_arm(meta, target_label, resolver)
            }
        }
    }

    fn decide_dynamic_route_arm(
        &mut self,
        meta: &SendMeta,
        target_label: u8,
        resolver: crate::global::const_dsl::RouteResolver,
    ) -> SendResult<()> {
        let crate::global::const_dsl::RouteResolver::Dynamic {
            resolver_id,
            scope: resolver_scope,
        } = resolver
        else {
            return Err(SendError::PhaseInvariant);
        };

        if meta.label != target_label {
            return Err(SendError::PhaseInvariant);
        }
        let scope_id = meta.route_scope;
        let arm_index = meta.selected_route_arm.ok_or(SendError::PhaseInvariant)?;
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
                        meta.lane,
                        scope_id,
                        resolver_id,
                        resolution,
                    );
                    resolution
                }
                Err(crate::session::cluster::error::ClusterError::ResolverReject {
                    resolver_id,
                }) => {
                    self.emit_dynamic_resolver_reject_audit(meta.lane, scope_id, resolver_id);
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
