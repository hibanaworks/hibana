use super::super::evidence_store::ReadyArmEvidence;
use super::select_candidates::ProjectedOfferFrontier;
use super::{
    Arm, FrontierDeferOutcome, FrontierVisitSet, IngressEvidenceState, ObservedIngressSelection,
    OfferEvidenceOutcome, OfferProgressState, OfferScopeSelection, OfferStagedIngress,
    OperationKernel, Poll, RecvError, RecvResult, ScopeId, Transport, lane_port,
    state_index_to_usize,
};
use crate::global::typestate::InboundFrameKey;

pub(super) struct FrontierDeferRequest {
    pub(super) scope_id: ScopeId,
    pub(super) entry_index: crate::global::typestate::StateIndex,
    pub(super) ingress: IngressEvidenceState,
}

impl<'r, T> OperationKernel<'_, 'r, T>
where
    T: Transport + 'r,
{
    pub(super) fn select_scope(
        &mut self,
        carried_lane: Option<u8>,
        carried_key: Option<InboundFrameKey>,
        carried_observation: Option<lane_port::FrameObservation>,
        frontier_visited: &mut FrontierVisitSet,
        projected: Option<ProjectedOfferFrontier>,
    ) -> RecvResult<Option<ObservedIngressSelection>> {
        if let Some(selection) = self.select_observed_ingress(carried_key, carried_observation)? {
            return Ok(Some(selection));
        }
        if let Some(observed) = carried_observation {
            let lane = carried_lane.ok_or(RecvError::PhaseInvariant)?;
            self.emit_materialization_mismatch_observation(
                usize::from(lane),
                lane,
                lane_port::FrameMismatch::label_mismatch(observed),
            );
            return Err(RecvError::PhaseInvariant);
        }
        if carried_lane.is_none()
            && let Some(frontier) = projected
        {
            let no_callback = frontier.incoming.iter().all(|word| *word == 0);
            match frontier.candidate {
                Some(candidate) if no_callback => {
                    return Ok(Some(ObservedIngressSelection::Branch(candidate)));
                }
                Some(candidate) => {
                    if let Some(candidate) =
                        self.recheck_projected_offer_candidate(candidate, frontier_visited)?
                    {
                        return Ok(Some(ObservedIngressSelection::Branch(candidate)));
                    }
                }
                None if no_callback => return Ok(None),
                None => {}
            }
        }
        // A callback can retire a view or replace parked branch knowledge.
        // Invalidated intent requires a fresh frontier, not a cached negative.
        self.read_projected_offer_frontier(frontier_visited, carried_lane)
            .map(|frontier| frontier.candidate.map(ObservedIngressSelection::Branch))
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn mark_scope_ready_arm(
        &mut self,
        scope_id: ScopeId,
        arm: Arm,
    ) {
        self.mark_scope_ready_arm_inner(scope_id, arm, ReadyArmEvidence::Poll);
    }

    #[inline]
    pub(super) fn mark_scope_materialization_ready_arm(&mut self, scope_id: ScopeId, arm: Arm) {
        self.mark_scope_ready_arm_inner(scope_id, arm, ReadyArmEvidence::Materialization);
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn mark_scope_ready_arm_from_frame_key(
        &mut self,
        scope_id: ScopeId,
        key: InboundFrameKey,
    ) -> RecvResult<bool> {
        let Some(arm) = self
            .progress
            .cursor
            .passive_descendant_dispatch_arm_for_key(scope_id, key)
            .map_err(|_| RecvError::PhaseInvariant)?
            .map(Arm::from_raw)
        else {
            return Ok(false);
        };
        self.mark_scope_ready_arm_from_exact_passive_arm(scope_id, arm);
        self.mark_intrinsic_passive_descendant_path_ready(scope_id, key)?;
        Ok(true)
    }

    #[inline]
    pub(super) fn mark_scope_ready_arm_from_exact_passive_arm(
        &mut self,
        scope_id: ScopeId,
        arm: Arm,
    ) {
        if self.intrinsic_passive_scope_evidence_materializes_poll(scope_id) {
            self.mark_scope_ready_arm(scope_id, arm);
        } else {
            self.mark_scope_materialization_ready_arm(scope_id, arm);
        }
    }

    #[inline]
    pub(super) fn mark_intrinsic_passive_descendant_path_ready(
        &mut self,
        scope_id: ScopeId,
        key: InboundFrameKey,
    ) -> RecvResult<()> {
        let mut current_scope = scope_id;
        let mut depth = 0usize;
        let depth_bound = self.progress.cursor.route_chain_bound();
        while depth < depth_bound {
            let Some(arm) = self
                .progress
                .cursor
                .passive_descendant_dispatch_arm_for_key(current_scope, key)
                .map_err(|_| RecvError::PhaseInvariant)?
            else {
                break;
            };
            let arm = Arm::from_raw(arm);
            self.mark_scope_ready_arm(current_scope, arm);
            let Some(child_scope) = self
                .progress
                .cursor
                .passive_child_scope(current_scope, arm.as_u8())
            else {
                break;
            };
            current_scope = child_scope;
            depth += 1;
        }
        Ok(())
    }

    pub(super) fn on_frontier_defer(
        &mut self,
        progress: &mut OfferProgressState,
        request: FrontierDeferRequest,
        visited: &mut FrontierVisitSet,
    ) -> Poll<RecvResult<FrontierDeferOutcome>> {
        let FrontierDeferRequest {
            scope_id,
            entry_index,
            ingress,
        } = request;
        let fingerprint = self.evidence_fingerprint(scope_id, ingress);
        let no_candidate = match progress.on_defer(fingerprint) {
            OfferEvidenceOutcome::NewEvidence => Poll::Ready(Ok(FrontierDeferOutcome::Continue)),
            OfferEvidenceOutcome::Pending => Poll::Pending,
        };
        let current_entry_idx = entry_index.as_usize();
        visited.record(current_entry_idx);
        if self
            .read_projected_offer_frontier(visited, None)?
            .candidate
            .is_none()
        {
            return no_candidate;
        }
        Poll::Ready(Ok(FrontierDeferOutcome::Yielded))
    }
    pub(super) fn await_intrinsic_passive_progress(
        &mut self,
        pending_recv: &mut lane_port::PendingRecv,
        selection: OfferScopeSelection,
        selected_arm: Option<u8>,
        ingress: &mut OfferStagedIngress<'r>,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<()>> {
        let materialization_meta = self.compute_scope_arm_materialization_meta(selection.scope_id);
        let selected_arm = selected_arm.map(Arm::from_raw);
        let progress_lane = match selected_arm {
            Some(arm) => self
                .route_scope_arm_lane_set_for_scope(selection.scope_id, arm.as_u8())
                .and_then(|lanes| lanes.first_set(self.progress.cursor.logical_lane_count()))
                .map(|lane_idx| lane_idx as u8),
            None => Some(selection.offer_lane),
        };
        if let Some(arm) = selected_arm
            && selection.entry_position.is_route_entry()
            && let Some(entry) = materialization_meta.passive_arm_entry(arm)
            && !self.progress.cursor.is_recv_at(state_index_to_usize(entry))
        {
            return Poll::Ready(Ok(()));
        }
        let Some(progress_lane) = progress_lane else {
            return Poll::Ready(Ok(()));
        };
        if !ingress.has_transport() {
            return self.await_transport_payload_for_offer_lane(
                pending_recv,
                progress_lane,
                ingress,
                cx,
            );
        }
        Poll::Ready(Ok(()))
    }
}
