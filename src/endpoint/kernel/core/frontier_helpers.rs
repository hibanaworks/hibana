use super::{
    CursorEndpoint, EventCursor, FrontierFacts, FrontierKind, FrontierReadiness,
    OfferScopeSelection, ScopeArmMaterializationMeta, ScopeId, ScopeReentryMeta, Transport,
    state_index_to_usize,
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
