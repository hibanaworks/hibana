use super::*;

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>
    CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: BindingSlot + 'r,
{
    pub(crate) fn poll_offer_state(
        &mut self,
        state: &mut OfferState<'r>,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<crate::endpoint::kernel::MaterializedRouteBranch<'r>>> {
        let (
            frontier_visited,
            carried_binding_evidence,
            carried_transport_payload,
            run_stage,
            pending_recv,
        ) = state.into_machine_parts();
        let mut machine = RouteFrontierMachine {
            endpoint: self,
            frontier_visited,
            carried_binding_evidence,
            carried_transport_payload,
            run_stage,
            pending_recv,
        };
        let poll = machine.poll_run(cx).map(|result| result.map(Into::into));
        state.store_machine_parts(
            machine.frontier_visited.take(),
            machine.carried_binding_evidence.take(),
            machine.carried_transport_payload.take(),
            machine.run_stage.take(),
            machine.pending_recv,
        );
        poll
    }

    pub(in crate::endpoint::kernel) fn preflight_branch_preview_commit_plan(
        &mut self,
        branch: BranchPreviewView,
    ) -> RecvResult<BranchCommitPlan> {
        RouteFrontierMachine::new(self).preflight_route_branch_commit(branch)
    }

    pub(in crate::endpoint::kernel) fn publish_branch_preview_commit_plan(
        &mut self,
        plan: BranchCommitPlan,
    ) {
        RouteFrontierMachine::new(self).publish_route_branch_commit_plan(plan)
    }

    pub(in crate::endpoint::kernel) fn ingest_binding_scope_evidence(
        &mut self,
        scope_id: ScopeId,
        lane: u8,
        frame_label: u8,
        suppress_hint: bool,
        frame_label_meta: ScopeFrameLabelMeta,
    ) {
        RouteFrontierMachine::new(self).ingest_binding_scope_evidence(
            scope_id,
            lane,
            frame_label,
            suppress_hint,
            frame_label_meta,
        )
    }

    pub(in crate::endpoint::kernel) fn ingest_scope_evidence_for_offer_lanes(
        &mut self,
        scope_id: ScopeId,
        summary_lane_idx: usize,
        offer_lanes: LaneSetView,
        suppress_hint: bool,
        frame_label_meta: ScopeFrameLabelMeta,
    ) {
        RouteFrontierMachine::new(self).ingest_scope_evidence_for_offer(
            scope_id,
            summary_lane_idx,
            offer_lanes,
            suppress_hint,
            frame_label_meta,
        )
    }

    pub(in crate::endpoint::kernel) fn recover_scope_evidence_conflict(
        &mut self,
        scope_id: ScopeId,
        is_dynamic_scope: bool,
        is_route_controller: bool,
    ) -> bool {
        RouteFrontierMachine::new(self).recover_scope_evidence_conflict(
            scope_id,
            is_dynamic_scope,
            is_route_controller,
        )
    }

    #[cfg(test)]
    pub(in crate::endpoint::kernel) fn cache_binding_evidence_for_offer(
        &mut self,
        scope_id: ScopeId,
        offer_lane_idx: usize,
        frame_label_meta: ScopeFrameLabelMeta,
        materialization_meta: ScopeArmMaterializationMeta,
        binding_evidence: &mut Option<LaneIngressEvidence>,
    ) {
        RouteFrontierMachine::new(self).cache_binding_evidence_for_offer(
            scope_id,
            offer_lane_idx,
            frame_label_meta,
            materialization_meta,
            binding_evidence,
        )
    }

    pub(in crate::endpoint::kernel) fn record_scope_ack(
        &mut self,
        scope_id: ScopeId,
        token: RouteDecisionToken,
    ) {
        RouteFrontierMachine::new(self).record_scope_ack(scope_id, token)
    }

    pub(in crate::endpoint::kernel) fn mark_scope_ready_arm(&mut self, scope_id: ScopeId, arm: u8) {
        RouteFrontierMachine::new(self).mark_scope_ready_arm(scope_id, arm)
    }

    pub(in crate::endpoint::kernel) fn mark_scope_ready_arm_from_frame_label(
        &mut self,
        scope_id: ScopeId,
        lane: u8,
        frame_label: u8,
        frame_label_meta: ScopeFrameLabelMeta,
    ) {
        RouteFrontierMachine::new(self).mark_scope_ready_arm_from_frame_label(
            scope_id,
            lane,
            frame_label,
            frame_label_meta,
        )
    }

    pub(in crate::endpoint::kernel) fn mark_scope_ready_arm_from_binding_frame_label(
        &mut self,
        scope_id: ScopeId,
        lane: u8,
        frame_label: u8,
        frame_label_meta: ScopeFrameLabelMeta,
    ) {
        RouteFrontierMachine::new(self).mark_scope_ready_arm_from_binding_frame_label(
            scope_id,
            lane,
            frame_label,
            frame_label_meta,
        )
    }

    pub(in crate::endpoint::kernel) fn refresh_frontier_observation_cache_for_scope(
        &mut self,
        scope_id: ScopeId,
    ) {
        RouteFrontierMachine::new(self).refresh_frontier_observation_cache_for_scope(scope_id)
    }

    pub(in crate::endpoint::kernel) fn refresh_frontier_observation_cache_for_binding_lane(
        &mut self,
        lane_idx: usize,
        previous_nonempty: bool,
    ) {
        RouteFrontierMachine::new(self)
            .refresh_frontier_observation_cache_for_binding_lane(lane_idx, previous_nonempty)
    }

    pub(in crate::endpoint::kernel) fn refresh_frontier_observation_cache_for_route_lane(
        &mut self,
        lane_idx: usize,
        previous_change_epoch: u16,
    ) {
        RouteFrontierMachine::new(self)
            .refresh_frontier_observation_cache_for_route_lane(lane_idx, previous_change_epoch)
    }

    #[cfg(test)]
    pub(in crate::endpoint::kernel) fn cached_frontier_changed_entry_slot_mask(
        &mut self,
        domain: FrontierObservationDomain,
        observation_key: FrontierObservationKey,
        cached_key: FrontierObservationKey,
    ) -> Option<u8> {
        RouteFrontierMachine::new(self).cached_frontier_changed_entry_slot_mask(
            domain,
            observation_key,
            cached_key,
        )
    }

    #[cfg(test)]
    pub(in crate::endpoint::kernel) fn refresh_frontier_observed_entries_from_cache(
        &mut self,
        domain: FrontierObservationDomain,
        active_entries: ActiveEntrySet,
        observation_key: FrontierObservationKey,
        cached_key: FrontierObservationKey,
        cached_observed_entries: ObservedEntrySet,
    ) -> Option<ObservedEntrySet> {
        RouteFrontierMachine::new(self).refresh_frontier_observed_entries_from_cache(
            domain,
            active_entries,
            observation_key,
            cached_key,
            cached_observed_entries,
        )
    }

    #[cfg(test)]
    pub(in crate::endpoint::kernel) fn refresh_cached_frontier_observation_entry(
        &mut self,
        domain: FrontierObservationDomain,
        entry_idx: usize,
    ) -> bool {
        RouteFrontierMachine::new(self).refresh_cached_frontier_observation_entry(domain, entry_idx)
    }

    #[cfg(test)]
    pub(in crate::endpoint::kernel) fn clear_lane_offer_state(&mut self, lane_idx: usize) {
        RouteFrontierMachine::new(self).clear_lane_offer_state(lane_idx)
    }

    pub(crate) fn sync_lane_offer_state(&mut self) {
        RouteFrontierMachine::new(self).sync_lane_offer_state()
    }

    pub(in crate::endpoint::kernel) fn refresh_lane_offer_state(&mut self, lane_idx: usize) {
        RouteFrontierMachine::new(self).refresh_lane_offer_state(lane_idx)
    }
}
