use super::super::{
    EventCursor, PackedEventConflict, ScopeId, SendMeta, SendPreviewError, StateIndex,
    state_index_to_usize,
};
use crate::global::typestate::EventCommitMeta;

#[derive(Clone, Copy)]
struct SendPreviewRouteArm {
    lane: u8,
    scope: ScopeId,
    arm: u8,
}

struct SendPreviewDecisionContext<'a> {
    target_label: u8,
    target_schema: u32,
    preview_route_arm: &'a mut Option<SendPreviewRouteArm>,
    committed_arm_for_scope: &'a mut dyn FnMut(ScopeId) -> Option<u8>,
    preview_controller_arm_for_scope: &'a mut dyn FnMut(ScopeId) -> Option<u8>,
    selected_arm_for_scope: &'a mut dyn FnMut(ScopeId) -> Option<u8>,
    lane_for_contract_or_offer: &'a mut dyn FnMut(ScopeId, u8, u32) -> u8,
}

impl EventCursor {
    fn send_preview_local_contract_lane_at(&self, idx: usize) -> Option<(u8, u32, u8)> {
        if let Some(meta) = self.try_recv_meta_at(idx) {
            return Some((meta.label, meta.payload_schema, meta.lane));
        }
        if let Some(meta) = self.try_send_meta_at(idx) {
            return Some((meta.label, meta.payload_schema, meta.lane));
        }
        if let Some(meta) = self.try_local_meta_at(idx) {
            return Some((meta.label, meta.payload_schema, meta.lane));
        }
        None
    }

    #[inline(never)]
    fn send_preview_route_arm_contract_index(
        &self,
        scope_id: ScopeId,
        arm: u8,
        target_label: u8,
        target_schema: u32,
    ) -> Option<usize> {
        let slot = self.route_scope_slot(scope_id)?;
        let row = self
            .machine()
            .event_program()
            .route_arm_event_row_by_slot(slot, arm)?;
        let mut completed = None;
        let mut idx = row.start();
        while idx < row.end() {
            if let Some((label, schema, lane)) = self.send_preview_local_contract_lane_at(idx)
                && label == target_label
                && schema == target_schema
            {
                if !self.node_event_done_for_lane(idx, lane) {
                    return Some(idx);
                }
                if completed.is_none() {
                    completed = Some(idx);
                }
            }
            idx += 1;
        }
        completed
    }

    fn send_preview_is_at_controller_arm_entry(&self, scope_id: ScopeId, idx: usize) -> bool {
        let mut arm = 0u8;
        while arm <= 1 {
            if self
                .controller_arm_entry_by_arm(scope_id, arm)
                .is_some_and(|(entry, _)| state_index_to_usize(entry) == idx)
                || self.route_arm_for_index(scope_id, idx) == Some(arm)
            {
                return true;
            }
            if arm == 1 {
                break;
            }
            arm += 1;
        }
        false
    }

    #[inline]
    pub(super) fn intrinsic_send_preview_controller_arm_entry_for_contract(
        &self,
        scope_id: ScopeId,
        target_label: u8,
        target_schema: u32,
    ) -> Option<(u8, usize)> {
        let mut arm = 0u8;
        while arm <= 1 {
            if let Some((entry, _)) = self.controller_arm_entry_by_arm(scope_id, arm)
                && let entry_idx = state_index_to_usize(entry)
                && self
                    .send_preview_local_contract_lane_at(entry_idx)
                    .is_some_and(|(label, schema, _)| {
                        label == target_label && schema == target_schema
                    })
            {
                return Some((arm, entry_idx));
            }
            if let Some(idx) = self.send_preview_route_arm_contract_index(
                scope_id,
                arm,
                target_label,
                target_schema,
            ) {
                return Some((arm, idx));
            }
            if arm == 1 {
                break;
            }
            arm += 1;
        }
        None
    }

    #[inline(never)]
    fn send_preview_selected_controller_arm_entry_for_contract(
        &self,
        scope_id: ScopeId,
        arm: u8,
        target_label: u8,
        target_schema: u32,
    ) -> Result<usize, SendPreviewError> {
        if let Some((entry, _)) = self.controller_arm_entry_by_arm(scope_id, arm) {
            let entry_idx = state_index_to_usize(entry);
            let Some((entry_label, entry_schema, _)) =
                self.send_preview_local_contract_lane_at(entry_idx)
            else {
                return Err(SendPreviewError::Invariant);
            };
            if entry_label == target_label && entry_schema == target_schema {
                return Ok(entry_idx);
            }
            if let Some(idx) = self.send_preview_route_arm_contract_index(
                scope_id,
                arm,
                target_label,
                target_schema,
            ) {
                return Ok(idx);
            }
            if entry_label == target_label {
                return Err(SendPreviewError::SchemaMismatch {
                    expected: entry_schema,
                    actual: target_schema,
                });
            }
            return Err(SendPreviewError::LabelMismatch {
                expected: entry_label,
                actual: target_label,
            });
        }
        self.send_preview_route_arm_contract_index(scope_id, arm, target_label, target_schema)
            .ok_or(SendPreviewError::Invariant)
    }

    #[inline]
    fn send_preview_lane_at(&self, idx: usize) -> Option<u8> {
        if let Some(meta) = self.try_send_meta_at(idx) {
            Some(meta.lane)
        } else {
            self.try_local_meta_at(idx).map(|meta| meta.lane)
        }
    }

    #[inline]
    fn send_preview_selected_arm_for_scope_with_route(
        &self,
        scope_id: ScopeId,
        preview_route_arm: Option<SendPreviewRouteArm>,
        arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> Option<u8> {
        if scope_id.is_none() {
            return None;
        }
        if let Some(preview) = preview_route_arm
            && preview.scope == scope_id
            && (preview.lane as usize) < self.logical_lane_count()
        {
            return Some(preview.arm);
        }
        arm_for_scope(scope_id)
    }

    fn route_scope_has_dynamic_resolver(&self, scope_id: ScopeId) -> bool {
        self.route_scope_resolver(scope_id).is_some()
    }

    #[inline(never)]
    fn reentry_committed_arm_complete(
        &self,
        scope_id: ScopeId,
        arm: u8,
        arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> bool {
        self.route_scope_reentry(scope_id)
            && self.selected_route_arm_event_row_done(scope_id, arm, arm_for_scope)
    }

    fn send_preview_arm_for_scope_with_reentry_path(
        &self,
        scope: ScopeId,
        preview_route_arm: Option<SendPreviewRouteArm>,
        preview_conflict: PackedEventConflict,
        committed_arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
        selected_arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> Option<u8> {
        if preview_route_arm.is_some_and(|preview| preview.scope == scope) {
            return preview_route_arm.map(|preview| preview.arm);
        }
        if let Some(committed_arm) = committed_arm_for_scope(scope) {
            if self
                .preview_conflict_arm(preview_conflict, scope)
                .is_some_and(|preview_arm| preview_arm != committed_arm)
                && self.reentry_committed_arm_complete(
                    scope,
                    committed_arm,
                    committed_arm_for_scope,
                )
            {
                return None;
            }
            return Some(committed_arm);
        }
        selected_arm_for_scope(scope)
    }

    #[inline(never)]
    fn send_preview_route_scope_end_if_complete(
        &self,
        scope_id: ScopeId,
        preview_route_arm: Option<SendPreviewRouteArm>,
        arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> Option<usize> {
        let arm = self.send_preview_selected_arm_for_scope_with_route(
            scope_id,
            preview_route_arm,
            arm_for_scope,
        )?;
        let mut selected_arm_for_scope = |scope| {
            self.send_preview_selected_arm_for_scope_with_route(
                scope,
                preview_route_arm,
                arm_for_scope,
            )
        };
        if !self.selected_route_arm_completes_scope(scope_id, arm, &mut selected_arm_for_scope) {
            return None;
        }
        self.route_scope_end_by_id(scope_id)
    }

    fn send_preview_contract_at_index(&self, idx: usize) -> Option<(u8, u32)> {
        if let Some(meta) = self.try_recv_meta_at(idx) {
            return Some((meta.label, meta.payload_schema));
        }
        if let Some(meta) = self.try_send_meta_at(idx) {
            return Some((meta.label, meta.payload_schema));
        }
        if let Some(meta) = self.try_local_meta_at(idx) {
            return Some((meta.label, meta.payload_schema));
        }
        None
    }

    fn send_preview_missing_start_error(
        &self,
        target_label: u8,
        target_schema: u32,
    ) -> SendPreviewError {
        if let Some(idx) = self.first_pending_step_index()
            && let Some((expected_label, expected_schema)) =
                self.send_preview_contract_at_index(idx)
        {
            if expected_label == target_label {
                return SendPreviewError::SchemaMismatch {
                    expected: expected_schema,
                    actual: target_schema,
                };
            }
            return SendPreviewError::LabelMismatch {
                expected: expected_label,
                actual: target_label,
            };
        }
        SendPreviewError::Invariant
    }

    #[inline(never)]
    fn send_preview_apply_controller_route_decision_for_contract(
        &self,
        idx: &mut usize,
        scope_id: ScopeId,
        at_decision: bool,
        ctx: &mut SendPreviewDecisionContext<'_>,
    ) -> Result<(), SendPreviewError> {
        let at_arm_entry = self.send_preview_is_at_controller_arm_entry(scope_id, *idx);
        let at_decision = at_arm_entry || at_decision;
        if at_decision
            && self.route_scope_reentry(scope_id)
            && !self.route_scope_has_dynamic_resolver(scope_id)
            && let Some((arm, entry_idx)) = self
                .intrinsic_send_preview_controller_arm_entry_for_contract(
                    scope_id,
                    ctx.target_label,
                    ctx.target_schema,
                )
            && let Some(committed) = (ctx.committed_arm_for_scope)(scope_id)
            && committed != arm
            && self.reentry_committed_arm_complete(scope_id, committed, ctx.committed_arm_for_scope)
        {
            *idx = entry_idx;
            if let Some(lane) = self.send_preview_lane_at(*idx) {
                *ctx.preview_route_arm = Some(SendPreviewRouteArm {
                    lane,
                    scope: scope_id,
                    arm,
                });
            }
            return Ok(());
        }
        if at_decision && let Some(selected) = (ctx.preview_controller_arm_for_scope)(scope_id) {
            let entry_idx = self.send_preview_selected_controller_arm_entry_for_contract(
                scope_id,
                selected,
                ctx.target_label,
                ctx.target_schema,
            )?;
            if let Some(committed) = (ctx.committed_arm_for_scope)(scope_id)
                && committed != selected
                && !self.route_scope_reentry(scope_id)
            {
                return Err(SendPreviewError::Invariant);
            }
            *idx = entry_idx;
            if let Some(lane) = self.send_preview_lane_at(*idx) {
                *ctx.preview_route_arm = Some(SendPreviewRouteArm {
                    lane,
                    scope: scope_id,
                    arm: selected,
                });
            }
        }
        if at_decision
            && ctx.preview_route_arm.is_none()
            && let Some((arm, entry_idx)) = self
                .intrinsic_send_preview_controller_arm_entry_for_contract(
                    scope_id,
                    ctx.target_label,
                    ctx.target_schema,
                )
        {
            if let Some(committed) = (ctx.committed_arm_for_scope)(scope_id)
                && committed != arm
                && !self.route_scope_reentry(scope_id)
            {
                return Err(SendPreviewError::Invariant);
            }
            *idx = entry_idx;
            if let Some(lane) = self.send_preview_lane_at(*idx) {
                *ctx.preview_route_arm = Some(SendPreviewRouteArm {
                    lane,
                    scope: scope_id,
                    arm,
                });
            }
        }
        if at_decision
            && ctx.preview_route_arm.is_none()
            && let Some(arm) = self.route_arm_for_index(scope_id, *idx)
            && let Some(lane) = self.send_preview_lane_at(*idx)
        {
            *ctx.preview_route_arm = Some(SendPreviewRouteArm {
                lane,
                scope: scope_id,
                arm,
            });
        }
        Ok(())
    }

    #[inline(never)]
    fn send_preview_apply_observer_route_decision_for_contract(
        &self,
        idx: &mut usize,
        scope_id: ScopeId,
        at_decision: bool,
        ctx: &mut SendPreviewDecisionContext<'_>,
    ) {
        if !at_decision {
            return;
        }
        let lane_wire =
            (ctx.lane_for_contract_or_offer)(scope_id, ctx.target_label, ctx.target_schema);
        let selected_arm = match (ctx.selected_arm_for_scope)(scope_id) {
            Some(arm) => Some(arm),
            None => (*ctx.preview_route_arm).and_then(|preview| {
                (preview.lane == lane_wire && preview.scope == scope_id).then_some(preview.arm)
            }),
        };
        if let Some(selected_arm) = selected_arm {
            *ctx.preview_route_arm = Some(SendPreviewRouteArm {
                lane: lane_wire,
                scope: scope_id,
                arm: selected_arm,
            });
            if let Some(entry_idx) = self.passive_observer_arm_entry_index(scope_id, selected_arm) {
                *idx = entry_idx;
            }
        }
    }

    #[inline(never)]
    fn send_preview_apply_route_decision_for_contract(
        &self,
        idx: &mut usize,
        ctx: &mut SendPreviewDecisionContext<'_>,
    ) -> Result<(), SendPreviewError> {
        let scope_id = match self.send_preview_controller_scope_at_for_decision(*idx) {
            Some(scope_id) => Some(scope_id),
            None => self
                .enclosing_route_scope_rows_at(*idx)
                .map(|region| region.scope()),
        };
        let Some(scope_id) = scope_id else {
            return Ok(());
        };

        let at_route_start = self
            .route_scope_rows(scope_id)
            .is_some_and(|region| *idx == region.start());
        let unlabeled =
            !self.is_send_at(*idx) && !self.is_recv_at(*idx) && !self.is_local_action_at(*idx);
        let at_decision = at_route_start || unlabeled;

        if self.is_route_controller(scope_id) {
            return self.send_preview_apply_controller_route_decision_for_contract(
                idx,
                scope_id,
                at_decision,
                ctx,
            );
        }
        self.send_preview_apply_observer_route_decision_for_contract(
            idx,
            scope_id,
            at_decision,
            ctx,
        );
        Ok(())
    }

    #[inline(never)]
    fn send_preview_skip_non_sender_index(
        &self,
        idx: &mut usize,
        preview_route_arm: Option<SendPreviewRouteArm>,
        selected_arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> Result<bool, SendPreviewError> {
        if self.is_send_at(*idx) || self.is_local_action_at(*idx) {
            return Ok(false);
        }
        if let Some(recv_meta) = self.try_recv_meta_at(*idx)
            && let Ok(progress_step) =
                self.relocatable_resident_lane_step_at_index(*idx, recv_meta.lane as usize)
            && self.relocatable_step_done(progress_step)
        {
            *idx = state_index_to_usize(self.node_next_index_at(*idx));
            return Ok(true);
        }
        if let Some(region) = self.enclosing_route_scope_rows_at(*idx)
            && let Some(end) = self.send_preview_route_scope_end_if_complete(
                region.scope(),
                preview_route_arm,
                selected_arm_for_scope,
            )
        {
            *idx = end;
            return Ok(true);
        }
        Err(SendPreviewError::Invariant)
    }

    #[inline(never)]
    fn send_preview_meta_at_index<const ROLE: u8>(
        &self,
        idx: usize,
        preview_route_arm: Option<SendPreviewRouteArm>,
    ) -> Result<SendMeta, SendPreviewError> {
        let mut current_meta = if self.is_local_action_at(idx) {
            let local = self
                .try_local_meta_at(idx)
                .ok_or(SendPreviewError::Invariant)?;
            SendMeta {
                eff_index: local.eff_index,
                peer: ROLE,
                label: local.label,
                payload_schema: local.payload_schema,
                frame_label: local.frame_label,
                semantic: local.semantic,
                origin: local.origin,
                next: local.next,
                scope: local.scope,
                route_scope: local.route_scope,
                route_arm: local.route_arm,
                selected_route_arm: local.route_arm,
                lane: local.lane,
            }
        } else {
            self.try_send_meta_at(idx)
                .ok_or(SendPreviewError::Invariant)?
        };
        if let Some(preview) = preview_route_arm {
            current_meta.route_scope = preview.scope;
            current_meta.selected_route_arm = Some(preview.arm);
        }
        Ok(current_meta)
    }

    #[inline(never)]
    fn send_preview_conflict_allows(
        &self,
        idx: usize,
        preview_route_arm: Option<SendPreviewRouteArm>,
        committed_arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
        selected_arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> bool {
        let preview_conflict = self.machine().event_conflict_for_index(idx);
        let mut arm_for_scope = |scope| {
            self.send_preview_arm_for_scope_with_reentry_path(
                scope,
                preview_route_arm,
                preview_conflict,
                committed_arm_for_scope,
                selected_arm_for_scope,
            )
        };
        self.event_conflict_row_allows_with_preview(
            preview_conflict,
            preview_conflict,
            &mut arm_for_scope,
        )
    }

    #[inline(never)]
    fn send_preview_step_for_contract<const ROLE: u8>(
        &self,
        idx: &mut usize,
        target_label: u8,
        target_schema: u32,
        preview_route_arm: Option<SendPreviewRouteArm>,
        committed_arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
        selected_arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> Result<Option<(SendMeta, StateIndex)>, SendPreviewError> {
        if !self.send_preview_conflict_allows(
            *idx,
            preview_route_arm,
            committed_arm_for_scope,
            selected_arm_for_scope,
        ) {
            *idx = state_index_to_usize(self.node_next_index_at(*idx));
            return Ok(None);
        }

        if self.send_preview_skip_non_sender_index(
            idx,
            preview_route_arm,
            selected_arm_for_scope,
        )? {
            return Ok(None);
        }

        let current_meta = self.send_preview_meta_at_index::<ROLE>(*idx, preview_route_arm)?;
        let progress_step = self
            .relocatable_resident_lane_step_at_index(*idx, current_meta.lane as usize)
            .map_err(|_| SendPreviewError::Invariant)?;
        let contract_matches =
            current_meta.label == target_label && current_meta.payload_schema == target_schema;
        if !contract_matches && self.relocatable_step_done(progress_step) {
            *idx = state_index_to_usize(self.node_next_index_at(*idx));
            return Ok(None);
        }

        if contract_matches {
            if self.relocatable_step_done(progress_step)
                && !self.roll_reentry_event_allows_index(
                    *idx,
                    current_meta.lane,
                    &mut *committed_arm_for_scope,
                )
            {
                *idx = state_index_to_usize(self.node_next_index_at(*idx));
                return Ok(None);
            }
            let preview_conflict = self.machine().event_conflict_for_index(*idx);
            let mut arm_for_scope = |scope| {
                self.send_preview_arm_for_scope_with_reentry_path(
                    scope,
                    preview_route_arm,
                    preview_conflict,
                    committed_arm_for_scope,
                    selected_arm_for_scope,
                )
            };
            self.event_enabled(
                *idx,
                EventCommitMeta::from(current_meta),
                &mut arm_for_scope,
            )
            .map_err(|_| SendPreviewError::Invariant)?;
            return Ok(Some((current_meta, StateIndex::from_usize(*idx))));
        }

        if let Some(region) = self.enclosing_route_scope_rows_at(*idx)
            && let Some(end) = self.send_preview_route_scope_end_if_complete(
                region.scope(),
                preview_route_arm,
                selected_arm_for_scope,
            )
        {
            *idx = end;
            return Ok(None);
        }

        if current_meta.label == target_label {
            Err(SendPreviewError::SchemaMismatch {
                expected: current_meta.payload_schema,
                actual: target_schema,
            })
        } else {
            Err(SendPreviewError::LabelMismatch {
                expected: current_meta.label,
                actual: target_label,
            })
        }
    }

    pub(crate) fn send_preview_meta_for_contract<const ROLE: u8>(
        &self,
        target_label: u8,
        target_schema: u32,
        committed_arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
        preview_controller_arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
        selected_arm_for_scope: &mut dyn FnMut(ScopeId) -> Option<u8>,
        lane_for_contract_or_offer: &mut dyn FnMut(ScopeId, u8, u32) -> u8,
    ) -> Result<(SendMeta, StateIndex), SendPreviewError> {
        let mut idx = self
            .send_preview_start_index_for_contract(
                target_label,
                target_schema,
                committed_arm_for_scope,
            )
            .ok_or_else(|| self.send_preview_missing_start_error(target_label, target_schema))?;
        let mut preview_route_arm: Option<SendPreviewRouteArm> = None;

        {
            let mut route_decision = SendPreviewDecisionContext {
                target_label,
                target_schema,
                preview_route_arm: &mut preview_route_arm,
                committed_arm_for_scope,
                preview_controller_arm_for_scope,
                selected_arm_for_scope,
                lane_for_contract_or_offer,
            };
            self.send_preview_apply_route_decision_for_contract(&mut idx, &mut route_decision)?;
        }

        let mut iter = 0usize;
        let bound = self.local_steps_len() + self.route_chain_bound();
        loop {
            iter += 1;
            if iter > bound {
                return Err(SendPreviewError::Invariant);
            }

            if let Some(result) = self.send_preview_step_for_contract::<ROLE>(
                &mut idx,
                target_label,
                target_schema,
                preview_route_arm,
                committed_arm_for_scope,
                selected_arm_for_scope,
            )? {
                return Ok(result);
            }
        }
    }
}
