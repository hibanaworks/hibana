use super::{
    Arm, CursorEndpoint, ResolverDecisionProof, ResolverDecisionProofs, ScopeId, SendError,
    SendResult, Transport,
};
use crate::global::const_dsl::RouteResolver;

impl<'r, const ROLE: u8, T> CursorEndpoint<'r, ROLE, T>
where
    T: Transport + 'r,
{
    #[inline]
    fn preview_dynamic_resolver_arm_for_scope(
        &self,
        scope_id: ScopeId,
        lane: u8,
        proofs: &mut ResolverDecisionProofs,
    ) -> SendResult<Option<u8>> {
        let Some((resolver, _)) = self.cursor.route_scope_controller_resolver(scope_id) else {
            return Ok(None);
        };
        let RouteResolver::Dynamic {
            resolver_id,
            scope: resolver_scope,
        } = resolver
        else {
            return Ok(None);
        };
        if scope_id.is_none() || resolver_scope != scope_id {
            return Err(SendError::PhaseInvariant);
        }
        let arm = self.resolve_dynamic_resolver_for_send_preview(lane, scope_id, resolver_id)?;
        let arm_index = arm.index();
        proofs.push(ResolverDecisionProof::new(
            scope_id,
            resolver_id,
            arm_index,
            lane,
        ))?;
        Ok(Some(arm_index))
    }

    #[inline]
    pub(in crate::endpoint::kernel::core) fn preview_controller_send_arm_for_scope(
        &self,
        scope_id: ScopeId,
        lane: u8,
        proofs: &mut ResolverDecisionProofs,
    ) -> SendResult<Option<u8>> {
        if scope_id.is_none() {
            return Ok(None);
        }
        if let Some(arm) = self.selected_arm_for_scope(scope_id) {
            return Ok(Some(arm));
        }
        self.preview_dynamic_resolver_arm_for_scope(scope_id, lane, proofs)
    }

    #[inline]
    pub(in crate::endpoint::kernel::core) fn preview_send_arm_for_scope(
        &self,
        scope_id: ScopeId,
    ) -> SendResult<Option<u8>> {
        if scope_id.is_none() {
            return Ok(None);
        }
        if let Some(arm) = self.selected_arm_for_scope(scope_id) {
            return Ok(Some(arm));
        }
        if self
            .cursor
            .route_scope_controller_resolver(scope_id)
            .is_some_and(|(resolver, _)| resolver.is_dynamic())
        {
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
            .preview_scope_ack_token_non_consuming(scope_id, offer_lanes)
            .map(|token| token.arm().as_u8())
            .or_else(|| self.poll_arm_from_ready_mask(scope_id).map(Arm::as_u8)))
    }
}
