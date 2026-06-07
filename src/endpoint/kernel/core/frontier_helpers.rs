use super::{
    CachedRecvMeta, ControlSemanticKind, ControlSemanticsTable, CursorEndpoint, EpochTable,
    EventCursor, FrameLabelMask, FrontierKind, FrontierStaticFacts, JumpReason, LabelUniverse,
    MintConfigMarker, OfferScopeSelection, ScopeArmMaterializationMeta, ScopeFrameLabelMeta,
    ScopeId, ScopeLoopMeta, Transport, controller_arm_semantic_kind, state_index_to_usize,
};
impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint>
    CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
{
    fn is_loop_control_scope(
        cursor: &EventCursor,
        semantics: &ControlSemanticsTable,
        scope_id: ScopeId,
    ) -> bool {
        matches!(
            (
                controller_arm_semantic_kind(cursor, semantics, scope_id, 0),
                controller_arm_semantic_kind(cursor, semantics, scope_id, 1)
            ),
            (
                Some(ControlSemanticKind::LoopContinue),
                Some(ControlSemanticKind::LoopBreak)
            ) | (
                Some(ControlSemanticKind::LoopBreak),
                Some(ControlSemanticKind::LoopContinue)
            )
        )
    }

    pub(crate) fn parallel_scope_root(cursor: &EventCursor, scope_id: ScopeId) -> Option<ScopeId> {
        cursor.parallel_scope_root(scope_id)
    }

    #[inline]
    pub(crate) fn frontier_kind_for_cursor(
        cursor: &EventCursor,
        scope_id: ScopeId,
        is_controller: bool,
    ) -> FrontierKind {
        Self::frontier_kind_for_index(cursor, scope_id, is_controller, cursor.index())
    }

    #[inline]
    fn frontier_kind_for_index(
        cursor: &EventCursor,
        scope_id: ScopeId,
        is_controller: bool,
        idx: usize,
    ) -> FrontierKind {
        if cursor.jump_reason_at(idx) == Some(JumpReason::PassiveObserverBranch) {
            return FrontierKind::PassiveObserver;
        }
        let has_controller_entry = cursor.controller_arm_entry_by_arm(scope_id, 0).is_some()
            || cursor.controller_arm_entry_by_arm(scope_id, 1).is_some();
        if !is_controller && !has_controller_entry {
            return FrontierKind::PassiveObserver;
        }
        if cursor.route_scope_linger(scope_id) {
            return FrontierKind::Loop;
        }
        if Self::parallel_scope_root(cursor, scope_id).is_some() {
            return FrontierKind::Parallel;
        }
        FrontierKind::Route
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn scope_loop_meta(
        cursor: &EventCursor,
        semantics: &ControlSemanticsTable,
        scope_id: ScopeId,
    ) -> ScopeLoopMeta {
        Self::scope_loop_meta_at(cursor, semantics, scope_id, cursor.index())
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn scope_loop_meta_at(
        cursor: &EventCursor,
        semantics: &ControlSemanticsTable,
        scope_id: ScopeId,
        idx: usize,
    ) -> ScopeLoopMeta {
        let mut flags = 0u8;
        if cursor.node_loop_scope(idx).is_some() {
            flags |= ScopeLoopMeta::FLAG_SCOPE_ACTIVE;
        }
        if cursor.route_scope_linger(scope_id) {
            flags |= ScopeLoopMeta::FLAG_SCOPE_LINGER;
        }
        if Self::is_loop_control_scope(cursor, semantics, scope_id) {
            flags |= ScopeLoopMeta::FLAG_CONTROL_SCOPE;
        }
        if cursor.route_scope_arm_recv_index(scope_id, 0).is_some() {
            flags |= ScopeLoopMeta::FLAG_CONTINUE_HAS_RECV;
        }
        if cursor.route_scope_arm_recv_index(scope_id, 1).is_some() {
            flags |= ScopeLoopMeta::FLAG_BREAK_HAS_RECV;
        }
        ScopeLoopMeta { flags }
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn scope_frame_label_meta(
        cursor: &EventCursor,
        semantics: &ControlSemanticsTable,
        scope_id: ScopeId,
        loop_meta: ScopeLoopMeta,
    ) -> ScopeFrameLabelMeta {
        Self::scope_frame_label_meta_at(cursor, semantics, scope_id, loop_meta, cursor.index())
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn scope_frame_label_meta_at(
        cursor: &EventCursor,
        semantics: &ControlSemanticsTable,
        scope_id: ScopeId,
        loop_meta: ScopeLoopMeta,
        idx: usize,
    ) -> ScopeFrameLabelMeta {
        let is_controller = cursor.is_route_controller(scope_id);
        let mut meta = ScopeFrameLabelMeta {
            loop_meta,
            ..ScopeFrameLabelMeta::EMPTY
        };
        if let Some(recv_meta) = cursor.try_recv_meta_at(idx)
            && recv_meta.scope == scope_id
        {
            meta.recv_frame_label = recv_meta.frame_label;
            meta.flags |= ScopeFrameLabelMeta::FLAG_CURRENT_RECV_FRAME_LABEL;
            if let Some(arm) = recv_meta.route_arm {
                meta.recv_arm = arm;
                meta.flags |= ScopeFrameLabelMeta::FLAG_CURRENT_RECV_ARM;
                meta.record_arm_frame_label(arm, recv_meta.frame_label);
                if !Self::current_recv_is_scope_local(
                    cursor,
                    semantics,
                    scope_id,
                    loop_meta,
                    recv_meta.lane,
                    recv_meta.frame_label,
                    recv_meta.semantic,
                    arm,
                ) {
                    meta.flags |= ScopeFrameLabelMeta::FLAG_CURRENT_RECV_BINDING_EXCLUDED;
                }
            }
        }
        if let Some((_entry, label)) = cursor.controller_arm_entry_by_arm(scope_id, 0) {
            meta.controller_frame_labels[0] = label;
            meta.flags |= ScopeFrameLabelMeta::FLAG_CONTROLLER_ARM0;
            meta.record_arm_frame_label(0, label);
            if !is_controller {
                meta.clear_evidence_arm_frame_label(0, label);
            }
        }
        if let Some((_entry, label)) = cursor.controller_arm_entry_by_arm(scope_id, 1) {
            meta.controller_frame_labels[1] = label;
            meta.flags |= ScopeFrameLabelMeta::FLAG_CONTROLLER_ARM1;
            meta.record_arm_frame_label(1, label);
            if !is_controller {
                meta.clear_evidence_arm_frame_label(1, label);
            }
        }
        if loop_meta.loop_label_scope() {
            if let Some((_entry, label)) = cursor.controller_arm_entry_by_arm(scope_id, 0) {
                meta.record_arm_frame_label(0, label);
            }
            if let Some((_entry, label)) = cursor.controller_arm_entry_by_arm(scope_id, 1) {
                meta.record_arm_frame_label(1, label);
            }
        }
        if let Some((dispatch, len)) = cursor.route_scope_first_recv_dispatch_table(scope_id) {
            let mut dispatch_arm_masks = [FrameLabelMask::EMPTY; 2];
            let mut dispatch_idx = 0usize;
            while dispatch_idx < len as usize {
                let entry = dispatch[dispatch_idx];
                if entry.arm() < 2 && !entry.target().is_max() {
                    if cursor
                        .try_recv_meta_at(state_index_to_usize(entry.target()))
                        .is_some()
                    {
                        meta.record_dispatch_arm_frame_label_mask(
                            entry.arm(),
                            FrameLabelMask::from_frame_label(entry.frame_label()),
                        );
                    } else {
                        dispatch_arm_masks[entry.arm() as usize]
                            .insert_frame_label(entry.frame_label());
                    }
                }
                dispatch_idx += 1;
            }
            meta.record_dispatch_arm_frame_label_mask(0, dispatch_arm_masks[0]);
            meta.record_dispatch_arm_frame_label_mask(1, dispatch_arm_masks[1]);
        }
        meta
    }

    #[inline]
    fn offer_scope_frame_label_meta(
        &self,
        scope_id: ScopeId,
        offer_lane_idx: usize,
    ) -> ScopeFrameLabelMeta {
        if offer_lane_idx < self.cursor.logical_lane_count() {
            let info = self.decision_state.lane_offer_state(offer_lane_idx);
            if info.scope == scope_id {
                let entry_idx = state_index_to_usize(info.entry);
                if let Some(cached) = Self::offer_entry_frame_label_meta(self, scope_id, entry_idx)
                {
                    return cached;
                }
                let loop_meta = Self::scope_loop_meta_at(
                    &self.cursor,
                    &self.control_semantics(),
                    scope_id,
                    entry_idx,
                );
                return Self::scope_frame_label_meta_at(
                    &self.cursor,
                    &self.control_semantics(),
                    scope_id,
                    loop_meta,
                    entry_idx,
                );
            }
        }
        if let Some(offer_entry) = self.cursor.route_scope_offer_entry(scope_id) {
            let entry_idx = if offer_entry.is_max() {
                self.cursor.index()
            } else {
                state_index_to_usize(offer_entry)
            };
            if let Some(cached) = Self::offer_entry_frame_label_meta(self, scope_id, entry_idx) {
                return cached;
            }
            let loop_meta = Self::scope_loop_meta_at(
                &self.cursor,
                &self.control_semantics(),
                scope_id,
                entry_idx,
            );
            return Self::scope_frame_label_meta_at(
                &self.cursor,
                &self.control_semantics(),
                scope_id,
                loop_meta,
                entry_idx,
            );
        }
        let loop_meta = Self::scope_loop_meta(&self.cursor, &self.control_semantics(), scope_id);
        Self::scope_frame_label_meta(&self.cursor, &self.control_semantics(), scope_id, loop_meta)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn offer_scope_materialization_meta(
        &self,
        scope_id: ScopeId,
        offer_lane_idx: usize,
    ) -> ScopeArmMaterializationMeta {
        if offer_lane_idx < self.cursor.logical_lane_count() {
            let info = self.decision_state.lane_offer_state(offer_lane_idx);
            if info.scope == scope_id {
                if let Some(cached) = self
                    .offer_entry_materialization_meta(scope_id, state_index_to_usize(info.entry))
                {
                    return cached;
                }
            }
        }
        if let Some(offer_entry) = self.cursor.route_scope_offer_entry(scope_id) {
            let entry_idx = if offer_entry.is_max() {
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
    pub(in crate::endpoint::kernel) fn selection_frame_label_meta(
        &self,
        selection: OfferScopeSelection,
    ) -> ScopeFrameLabelMeta {
        self.offer_scope_frame_label_meta(selection.scope_id, selection.offer_lane as usize)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn selection_materialization_meta(
        &self,
        selection: OfferScopeSelection,
    ) -> ScopeArmMaterializationMeta {
        self.offer_scope_materialization_meta(selection.scope_id, selection.offer_lane as usize)
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn selection_passive_recv_meta(
        &self,
        selection: OfferScopeSelection,
        materialization_meta: ScopeArmMaterializationMeta,
    ) -> [CachedRecvMeta; 2] {
        self.compute_scope_passive_recv_meta(
            materialization_meta,
            selection.scope_id,
            selection.offer_lane,
        )
    }

    pub(in crate::endpoint::kernel) fn frontier_static_facts_at(
        cursor: &EventCursor,
        semantics: &ControlSemanticsTable,
        scope_id: ScopeId,
        is_controller: bool,
        is_dynamic: bool,
        idx: usize,
    ) -> FrontierStaticFacts {
        let loop_meta = Self::scope_loop_meta_at(cursor, semantics, scope_id, idx);
        let controller_local_ready =
            is_controller && Self::scope_has_controller_arm_entry(cursor, scope_id);
        let cursor_ready = cursor.is_recv_at(idx)
            || cursor.try_recv_meta_at(idx).is_some()
            || cursor.try_local_meta_at(idx).is_some();
        FrontierStaticFacts {
            frontier: Self::frontier_kind_for_index(cursor, scope_id, is_controller, idx),
            ready: loop_meta.recvless_ready()
                || controller_local_ready
                || is_dynamic
                || cursor_ready,
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn ack_is_progress_evidence(
        loop_meta: ScopeLoopMeta,
        has_ack: bool,
    ) -> bool {
        has_ack && !loop_meta.control_scope()
    }
}
