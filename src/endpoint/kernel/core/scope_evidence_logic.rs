use super::super::evidence_store::ReadyArmEvidence;
use super::{
    Arm, EventCursor, EvidenceFingerprint, IngressEvidenceState, OperationKernel,
    ScopeArmMaterializationMeta, ScopeEvidence, ScopeId, Transport, state_index_to_usize,
};
impl<'r, T> OperationKernel<'_, 'r, T>
where
    T: Transport + 'r,
{
    #[inline]
    pub(in crate::endpoint::kernel) fn scope_slot_for_route(
        &self,
        scope_id: ScopeId,
    ) -> Option<usize> {
        self.progress.cursor.route_scope_slot(scope_id)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn mark_scope_ready_arm_inner(
        &mut self,
        scope_id: ScopeId,
        arm: Arm,
        evidence_kind: ReadyArmEvidence,
    ) {
        let selected = self
            .selected_live_arm_for_scope(scope_id)
            .map(Arm::from_raw);
        if let Some(slot) = self.scope_slot_for_route(scope_id) {
            self.progress.decision_state.scope_evidence.mark_ready_arm(
                slot,
                arm,
                selected,
                evidence_kind,
            );
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn intrinsic_passive_scope_evidence_materializes_poll(
        &self,
        scope_id: ScopeId,
    ) -> bool {
        self.progress
            .cursor
            .intrinsic_passive_scope_evidence_materializes_poll(scope_id)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn scope_ready_arm_mask(&self, scope_id: ScopeId) -> u8 {
        let Some(slot) = self.scope_slot_for_route(scope_id) else {
            return 0;
        };
        self.progress
            .decision_state
            .scope_evidence
            .ready_arm_mask(slot)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn scope_poll_ready_arm_mask(&self, scope_id: ScopeId) -> u8 {
        let Some(slot) = self.scope_slot_for_route(scope_id) else {
            return 0;
        };
        self.progress
            .decision_state
            .scope_evidence
            .poll_ready_arm_mask(slot)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn scope_has_ready_arm(
        &self,
        scope_id: ScopeId,
        arm: u8,
    ) -> bool {
        (self.scope_ready_arm_mask(scope_id) & ScopeEvidence::arm_bit(Arm::from_raw(arm))) != 0
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn scope_has_ready_arm_evidence(
        &self,
        scope_id: ScopeId,
    ) -> bool {
        self.scope_ready_arm_mask(scope_id) != 0
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn consume_scope_ready_arm(
        &mut self,
        scope_id: ScopeId,
        arm: u8,
    ) {
        let arm = Arm::from_raw(arm);
        if let Some(slot) = self.scope_slot_for_route(scope_id) {
            self.progress
                .decision_state
                .scope_evidence
                .consume_ready_arm(slot, arm);
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn clear_scope_evidence(&mut self, scope_id: ScopeId) {
        if let Some(slot) = self.scope_slot_for_route(scope_id) {
            self.progress.decision_state.scope_evidence.clear(slot);
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn scope_evidence_conflicted(&self, scope_id: ScopeId) -> bool {
        let Some(slot) = self.scope_slot_for_route(scope_id) else {
            return false;
        };
        self.progress.decision_state.scope_evidence.conflicted(slot)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn lane_for_contract_or_offer(
        &self,
        scope_id: ScopeId,
        target_label: u8,
        target_payload: crate::eff::EventPayload,
    ) -> u8 {
        if let Some((lane_idx, _)) = self.progress.cursor.pending_step_for_contract(
            self.public_header.role(),
            target_label,
            target_payload,
        ) {
            lane_idx as u8
        } else {
            self.offer_lane_for_scope(scope_id)
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn evidence_fingerprint(
        &self,
        scope_id: ScopeId,
        ingress: IngressEvidenceState,
    ) -> EvidenceFingerprint {
        EvidenceFingerprint::new(
            self.scope_has_ready_arm_evidence(scope_id),
            ingress.is_ready(),
        )
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn poll_arm_from_ready_mask(
        &self,
        scope_id: ScopeId,
    ) -> Option<Arm> {
        let mask = self.scope_poll_ready_arm_mask(scope_id);
        Arm::from_single_ready_mask(mask)
    }

    #[inline]
    fn scope_arm_has_recv_with_dispatch(
        cursor: &EventCursor,
        scope_id: ScopeId,
        arm: u8,
        dispatch_has_arm: bool,
    ) -> bool {
        if cursor.route_scope_arm_recv_index(scope_id, arm).is_some() {
            return true;
        }
        if let Some((entry, _)) = cursor.controller_arm_entry_by_arm(scope_id, arm)
            && cursor.is_recv_at(state_index_to_usize(entry))
        {
            return true;
        }
        if let Some(entry_idx) = cursor.passive_observer_arm_entry_index(scope_id, arm) {
            return cursor.is_recv_at(entry_idx);
        }
        dispatch_has_arm
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn arm_has_recv_with_materialization(
        &self,
        scope_id: ScopeId,
        arm: u8,
        materialization_meta: ScopeArmMaterializationMeta,
    ) -> bool {
        let decoded_arm = Arm::from_raw(arm);
        if Self::scope_arm_has_recv_with_dispatch(
            &self.progress.cursor,
            scope_id,
            arm,
            materialization_meta.arm_has_first_recv_dispatch(decoded_arm),
        ) {
            return true;
        }
        self.preview_passive_materialization_index_for_selected_arm(scope_id, arm)
            .is_some_and(|target_idx| self.progress.cursor.try_recv_meta_at(target_idx).is_some())
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn arm_requires_materialization_ready_evidence_at(
        &self,
        scope_id: ScopeId,
        arm: u8,
        entry_index: usize,
    ) -> bool {
        let at_scope_offer_entry = crate::invariant_some(
            self.progress
                .cursor
                .route_scope_offer_entry(scope_id)
                .map(|entry| entry.is_absent() || entry_index == state_index_to_usize(entry)),
        );
        if self.is_route_controller(scope_id)
            && at_scope_offer_entry
            && let Some((entry, _)) = self
                .progress
                .cursor
                .controller_arm_entry_by_arm(scope_id, arm)
        {
            if let Some(recv_meta) = self
                .progress
                .cursor
                .try_recv_meta_at(state_index_to_usize(entry))
            {
                return recv_meta.peer != self.public_header.role();
            }
            return false;
        }
        if let Some(entry_idx) = self
            .progress
            .cursor
            .passive_observer_arm_entry_index(scope_id, arm)
        {
            let Some(recv_meta) = self.progress.cursor.try_recv_meta_at(entry_idx) else {
                return false;
            };
            if recv_meta.peer == self.public_header.role() {
                return false;
            }
            if recv_meta.origin.is_session()
                && let Some((_controller_entry, controller_label)) = self
                    .progress
                    .cursor
                    .controller_arm_entry_by_arm(scope_id, arm)
                && recv_meta.label == controller_label
            {
                return false;
            }
            return true;
        }
        self.progress
            .cursor
            .route_scope_arm_recv_index(scope_id, arm)
            .is_some()
    }

    pub(in crate::endpoint::kernel) fn arm_has_recv(&self, scope_id: ScopeId, arm: u8) -> bool {
        let materialization_meta = self.compute_scope_arm_materialization_meta(scope_id);
        self.arm_has_recv_with_materialization(scope_id, arm, materialization_meta)
    }
}
