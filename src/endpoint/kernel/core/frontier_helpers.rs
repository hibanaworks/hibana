use super::{
    Arm, CursorEndpoint, EventCursor, FrameLabelMask, FrontierFacts, FrontierKind,
    FrontierReadiness, OfferScopeSelection, ScopeArmMaterializationMeta, ScopeFrameLabelMeta,
    ScopeFrameLabelScratch, ScopeId, ScopeReentryMeta, Transport, state_index_to_usize,
};
impl<'r, const ROLE: u8, T> CursorEndpoint<'r, ROLE, T>
where
    T: Transport + 'r,
{
    pub(crate) fn parallel_scope_root(cursor: &EventCursor, scope_id: ScopeId) -> Option<ScopeId> {
        cursor.parallel_scope_root(scope_id)
    }

    #[inline]
    pub(crate) fn frontier_kind_for_cursor(
        cursor: &EventCursor,
        scope_id: ScopeId,
        is_controller: bool,
    ) -> FrontierKind {
        Self::frontier_kind(cursor, scope_id, is_controller)
    }

    #[inline]
    fn frontier_kind(cursor: &EventCursor, scope_id: ScopeId, is_controller: bool) -> FrontierKind {
        let has_controller_entry = cursor.controller_arm_entry_by_arm(scope_id, 0).is_some()
            || cursor.controller_arm_entry_by_arm(scope_id, 1).is_some();
        if !is_controller && !has_controller_entry {
            return FrontierKind::PassiveObserver;
        }
        if cursor.route_scope_reentry(scope_id) {
            return FrontierKind::Reentry;
        }
        if Self::parallel_scope_root(cursor, scope_id).is_some() {
            return FrontierKind::Parallel;
        }
        FrontierKind::Route
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn scope_reentry_meta(
        cursor: &EventCursor,
        scope_id: ScopeId,
    ) -> ScopeReentryMeta {
        Self::scope_reentry_meta_at(cursor, scope_id, cursor.index())
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn scope_reentry_meta_at(
        cursor: &EventCursor,
        scope_id: ScopeId,
        idx: usize,
    ) -> ScopeReentryMeta {
        let mut flags = 0u8;
        if cursor.node_roll_scope(idx).is_some() {
            flags |= ScopeReentryMeta::FLAG_SCOPE_ACTIVE;
        }
        if cursor.route_scope_reentry(scope_id) {
            flags |= ScopeReentryMeta::FLAG_ROUTE_REENTRY;
        }
        if cursor.route_scope_arm_recv_index(scope_id, 0).is_some() {
            flags |= ScopeReentryMeta::FLAG_ARM0_HAS_RECV;
        }
        if cursor.route_scope_arm_recv_index(scope_id, 1).is_some() {
            flags |= ScopeReentryMeta::FLAG_ARM1_HAS_RECV;
        }
        ScopeReentryMeta { flags }
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn write_scope_frame_label_meta(
        cursor: &EventCursor,
        scope_id: ScopeId,
        reentry_meta: ScopeReentryMeta,
        out: &mut ScopeFrameLabelScratch,
    ) {
        Self::write_scope_frame_label_meta_at(cursor, scope_id, reentry_meta, cursor.index(), out)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn write_scope_frame_label_meta_at(
        cursor: &EventCursor,
        scope_id: ScopeId,
        reentry_meta: ScopeReentryMeta,
        idx: usize,
        out: &mut ScopeFrameLabelScratch,
    ) {
        let is_controller = cursor.is_route_controller(scope_id);
        out.clear();
        if let Some(recv_meta) = cursor.try_recv_meta_at(idx)
            && recv_meta.scope == scope_id
        {
            out.meta_mut().recv_frame_label = recv_meta.frame_label;
            out.meta_mut().flags |= ScopeFrameLabelMeta::FLAG_CURRENT_RECV_FRAME_LABEL;
            if let Some(arm) = recv_meta.route_arm {
                let arm = Arm::from_raw(arm);
                out.meta_mut().recv_arm = arm.as_u8();
                out.meta_mut().flags |= ScopeFrameLabelMeta::FLAG_CURRENT_RECV_ARM;
                out.record_arm_frame_label(arm, recv_meta.frame_label);
                if !cursor.current_recv_matches_scope_arm(
                    scope_id,
                    recv_meta.lane,
                    recv_meta.frame_label,
                    arm.as_u8(),
                ) {
                    out.meta_mut().flags |= ScopeFrameLabelMeta::FLAG_CURRENT_RECV_BINDING_EXCLUDED;
                }
            }
        }
        if let Some((_entry, label)) = cursor.controller_arm_entry_by_arm(scope_id, 0) {
            out.meta_mut().controller_frame_labels[0] = label;
            out.meta_mut().flags |= ScopeFrameLabelMeta::FLAG_CONTROLLER_ARM0;
            out.record_arm_frame_label(Arm::LEFT, label);
            if !is_controller {
                out.exclude_controller_arm_frame_label_from_evidence(Arm::LEFT, label);
            }
        }
        if let Some((_entry, label)) = cursor.controller_arm_entry_by_arm(scope_id, 1) {
            out.meta_mut().controller_frame_labels[1] = label;
            out.meta_mut().flags |= ScopeFrameLabelMeta::FLAG_CONTROLLER_ARM1;
            out.record_arm_frame_label(Arm::RIGHT, label);
            if !is_controller {
                out.exclude_controller_arm_frame_label_from_evidence(Arm::RIGHT, label);
            }
        }
        if reentry_meta.route_reentry() {
            if let Some((_entry, label)) = cursor.controller_arm_entry_by_arm(scope_id, 0) {
                out.record_arm_frame_label(Arm::LEFT, label);
            }
            if let Some((_entry, label)) = cursor.controller_arm_entry_by_arm(scope_id, 1) {
                out.record_arm_frame_label(Arm::RIGHT, label);
            }
        }
        let mut dispatch_arm_masks = [FrameLabelMask::EMPTY; 2];
        crate::invariant_some(cursor.visit_route_scope_first_recv_dispatch(
            scope_id,
            |arm, target| {
                let arm = Arm::from_raw(arm);
                if target.is_absent() {
                    return;
                }
                let recv =
                    crate::invariant_some(cursor.try_recv_meta_at(state_index_to_usize(target)));
                dispatch_arm_masks[arm.as_u8() as usize].insert_frame_label(recv.frame_label);
            },
        ));
        out.record_dispatch_arm_frame_label_mask(Arm::LEFT, dispatch_arm_masks[0]);
        out.record_dispatch_arm_frame_label_mask(Arm::RIGHT, dispatch_arm_masks[1]);
    }

    #[inline]
    fn write_offer_scope_frame_label_meta(
        &self,
        scope_id: ScopeId,
        offer_lane_idx: usize,
        out: &mut ScopeFrameLabelScratch,
    ) {
        if offer_lane_idx < self.cursor.logical_lane_count() {
            let info = self.decision_state.lane_offer_state(offer_lane_idx);
            if info.scope == scope_id {
                let entry_idx = state_index_to_usize(info.entry);
                if Self::write_offer_entry_frame_label_meta(self, scope_id, entry_idx, out) {
                    return;
                }
                let reentry_meta = Self::scope_reentry_meta_at(&self.cursor, scope_id, entry_idx);
                Self::write_scope_frame_label_meta_at(
                    &self.cursor,
                    scope_id,
                    reentry_meta,
                    entry_idx,
                    out,
                );
                return;
            }
        }
        if let Some(offer_entry) = self.cursor.route_scope_offer_entry(scope_id) {
            let entry_idx = if offer_entry.is_absent() {
                self.cursor.index()
            } else {
                state_index_to_usize(offer_entry)
            };
            if Self::write_offer_entry_frame_label_meta(self, scope_id, entry_idx, out) {
                return;
            }
            let reentry_meta = Self::scope_reentry_meta_at(&self.cursor, scope_id, entry_idx);
            Self::write_scope_frame_label_meta_at(
                &self.cursor,
                scope_id,
                reentry_meta,
                entry_idx,
                out,
            );
            return;
        }
        let reentry_meta = Self::scope_reentry_meta(&self.cursor, scope_id);
        Self::write_scope_frame_label_meta(&self.cursor, scope_id, reentry_meta, out);
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn offer_scope_materialization_meta(
        &self,
        scope_id: ScopeId,
        offer_lane_idx: usize,
    ) -> ScopeArmMaterializationMeta {
        if offer_lane_idx < self.cursor.logical_lane_count() {
            let info = self.decision_state.lane_offer_state(offer_lane_idx);
            if info.scope == scope_id
                && let Some(cached) = self
                    .offer_entry_materialization_meta(scope_id, state_index_to_usize(info.entry))
            {
                return cached;
            }
        }
        if let Some(offer_entry) = self.cursor.route_scope_offer_entry(scope_id) {
            let entry_idx = if offer_entry.is_absent() {
                self.cursor.index()
            } else {
                state_index_to_usize(offer_entry)
            };
            if let Some(cached) = self.offer_entry_materialization_meta(scope_id, entry_idx) {
                return cached;
            }
        }
        self.compute_scope_arm_materialization_meta(scope_id)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn write_selection_frame_label_meta(
        &self,
        selection: OfferScopeSelection,
        out: &mut ScopeFrameLabelScratch,
    ) {
        self.write_offer_scope_frame_label_meta(
            selection.scope_id,
            selection.offer_lane as usize,
            out,
        );
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn selection_materialization_meta(
        &self,
        selection: OfferScopeSelection,
    ) -> ScopeArmMaterializationMeta {
        self.offer_scope_materialization_meta(selection.scope_id, selection.offer_lane as usize)
    }

    pub(in crate::endpoint::kernel) fn frontier_facts_at(
        cursor: &EventCursor,
        scope_id: ScopeId,
        is_controller: bool,
        is_dynamic: bool,
        idx: usize,
    ) -> FrontierFacts {
        let reentry_meta = Self::scope_reentry_meta_at(cursor, scope_id, idx);
        let controller_local_ready =
            is_controller && Self::scope_has_controller_arm_entry(cursor, scope_id);
        let cursor_ready = cursor.is_recv_at(idx)
            || cursor.try_recv_meta_at(idx).is_some()
            || cursor.try_local_meta_at(idx).is_some();
        let readiness = if reentry_meta.recvless_arm_ready()
            || controller_local_ready
            || is_dynamic
            || cursor_ready
        {
            FrontierReadiness::Ready
        } else {
            FrontierReadiness::Waiting
        };
        FrontierFacts {
            frontier: Self::frontier_kind(cursor, scope_id, is_controller),
            readiness,
        }
    }
}
