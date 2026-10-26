use super::{
    Arm, OperationKernel, RecvError, RecvResult, ScopeId, TapFrameMeta, Transport, emit, events,
    state_index_to_usize,
};

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::endpoint::kernel) enum IngressEvidenceState {
    Absent = 0,
    Ready = 1,
}

impl IngressEvidenceState {
    #[inline]
    pub(in crate::endpoint::kernel) const fn is_ready(self) -> bool {
        matches!(self, Self::Ready)
    }
}

impl<'r, T> OperationKernel<'_, 'r, T>
where
    T: Transport + 'r,
{
    /// Answers stay valid while this shared kernel borrow excludes progress
    /// updates. The scratch is discarded before any operation can commit.
    pub(in crate::endpoint::kernel) fn preview_queries(
        &self,
    ) -> impl FnMut(ScopeId) -> Option<u8> + '_ {
        let mut answers = [0u8; (ScopeId::LOCAL_CAPACITY as usize).div_ceil(4)];
        move |scope| {
            if !matches!(
                scope.kind(),
                Some(crate::global::const_dsl::ScopeKind::Route)
            ) {
                return self.preview_live_selected_arm_for_scope(scope);
            }
            let index = scope.local_ordinal() as usize;
            let shift = (index % 4) * 2;
            match (answers[index / 4] >> shift) & 3 {
                0 => {
                    let answer = self.preview_live_selected_arm_for_scope(scope);
                    let encoded = match answer {
                        None => 1,
                        Some(0) => 2,
                        Some(1) => 3,
                        Some(_) => crate::invariant(),
                    };
                    answers[index / 4] |= encoded << shift;
                    answer
                }
                1 => None,
                2 => Some(0),
                3 => Some(1),
                _ => crate::invariant(),
            }
        }
    }

    pub(crate) fn is_reentry_route(&self, scope: ScopeId) -> bool {
        self.progress.cursor.route_scope_reentry(scope)
    }

    pub(crate) fn selected_arm_for_scope(&self, scope: ScopeId) -> Option<u8> {
        if scope.is_none() {
            return None;
        }
        let scope_slot = self.scope_slot_for_route(scope)?;
        self.progress
            .decision_state
            .selected_arm_for_scope_slot(scope_slot)
    }

    pub(in crate::endpoint::kernel) fn selected_live_arm_for_scope(
        &self,
        scope: ScopeId,
    ) -> Option<u8> {
        let arm = self.selected_arm_for_scope(scope)?;
        if self.reentrant_selected_arm_complete(scope, arm) {
            None
        } else {
            Some(arm)
        }
    }

    pub(in crate::endpoint::kernel::core) fn reentrant_selected_arm_complete(
        &self,
        scope_id: ScopeId,
        arm: u8,
    ) -> bool {
        let mut selected_arm_for_scope = |scope| self.selected_arm_for_scope(scope);
        self.progress.cursor.reentrant_route_arm_event_row_done(
            scope_id,
            arm,
            &mut selected_arm_for_scope,
        )
    }

    pub(crate) fn route_scope_offer_entry_index(&self, scope_id: ScopeId) -> Option<usize> {
        let offer_entry = self.progress.cursor.route_scope_offer_entry(scope_id)?;
        Some(if offer_entry.is_absent() {
            self.progress.cursor.index()
        } else {
            state_index_to_usize(offer_entry)
        })
    }

    pub(crate) fn preview_passive_materialization_index_for_selected_arm(
        &self,
        scope_id: ScopeId,
        arm: u8,
    ) -> Option<usize> {
        self.progress
            .cursor
            .passive_materialization_index_for_selected_arm(scope_id, arm, |scope| {
                self.preview_live_selected_arm_for_scope(scope)
            })
    }

    pub(in crate::endpoint::kernel) fn preview_live_selected_arm_for_scope(
        &self,
        scope_id: ScopeId,
    ) -> Option<u8> {
        let slot = self.scope_slot_for_route(scope_id);
        let selected = slot
            .and_then(|slot| {
                self.progress
                    .decision_state
                    .selected_arm_for_scope_slot(slot)
            })
            .filter(|&arm| !self.reentrant_selected_arm_complete(scope_id, arm));
        let ready = slot
            .and_then(|slot| {
                Arm::from_single_ready_mask(
                    self.progress
                        .decision_state
                        .scope_evidence
                        .poll_ready_arm_mask(slot),
                )
            })
            .map(Arm::as_u8);
        match (selected, ready) {
            (Some(selected), Some(ready)) if selected != ready => crate::invariant(),
            (Some(selected), _) => Some(selected),
            (None, Some(ready)) => Some(ready),
            (None, None) => self.parked_preview_arm_for_scope(scope_id),
        }
    }

    pub(in crate::endpoint::kernel::core) fn parked_preview_arm_for_scope(
        &self,
        scope: ScopeId,
    ) -> Option<u8> {
        let from_branch = |branch: &super::BranchMeta| {
            if branch.scope_id == scope {
                Some(branch.selected_arm)
            } else {
                self.progress
                    .cursor
                    .route_scope_conflict_arm_for_scope(branch.scope_id, scope)
            }
        };
        let mut selected = self
            .operation
            .public_route_branch
            .as_ref()
            .and_then(|branch| from_branch(&branch.branch_meta));
        if self.region.is_whole() && self.views.as_slice().len() == 1 {
            return selected;
        }
        let bounds = self.region.bounds();
        if !self.region.is_whole()
            && !self.context.program.source_route_visible_from_range(
                scope,
                bounds.start,
                bounds.end,
            )
        {
            return selected;
        }
        // A received choice is known to the actor across its held views.
        // Reading parked previews leaves every communication step uncommitted.
        for view in self.views.as_slice() {
            let current_view = core::ptr::eq(&view.public_header, self.public_header);
            if (current_view && self.region.is_whole())
                || (!current_view && view.public_header.generation() == 0)
            {
                continue;
            }
            for operation in view.operations() {
                if core::ptr::eq(operation.state.get(), self.operation) {
                    continue;
                }
                // SAFETY: all views belong to this actor. Its progress borrow
                // excludes other kernels; the mutable current slot is excluded.
                let operation = unsafe { &*operation.state.get() };
                if matches!(
                    operation.public_active_op,
                    super::PublicActiveOp::Idle | super::PublicActiveOp::Poisoned
                ) {
                    continue;
                }
                if let Some(arm) = operation
                    .public_route_branch
                    .as_ref()
                    .and_then(|branch| from_branch(&branch.branch_meta))
                {
                    if selected.is_some_and(|selected| selected != arm) {
                        crate::invariant();
                    }
                    selected = Some(arm);
                }
            }
        }
        selected
    }

    #[inline]
    pub(crate) fn offer_scope_id_at(&self, index: usize) -> ScopeId {
        let mut selected_arm_for_scope = |scope| self.selected_live_arm_for_scope(scope);
        let mut preview_selected_arm_for_scope =
            |scope| self.preview_live_selected_arm_for_scope(scope);
        self.progress.cursor.offer_scope_id_at(
            index,
            &mut selected_arm_for_scope,
            &mut preview_selected_arm_for_scope,
        )
    }

    pub(crate) fn rebase_passive_descendant_scope(
        &self,
        stop_scope: ScopeId,
        initial_scope: ScopeId,
    ) -> ScopeId {
        self.progress
            .cursor
            .rebase_passive_descendant_scope(stop_scope, initial_scope, |scope| {
                self.preview_live_selected_arm_for_scope(scope)
            })
    }

    pub(crate) fn current_route_arm_authorized(&self) -> RecvResult<bool> {
        self.progress
            .cursor
            .current_route_arm_authorization(
                self.public_header.role(),
                |scope| self.selected_live_arm_for_scope(scope),
                |scope| self.preview_live_selected_arm_for_scope(scope),
            )
            .map(|authorization| authorization.authorizes_current_arm())
            .map_err(|_| RecvError::PhaseInvariant)
    }

    pub(crate) fn emit_endpoint_event(&self, id: u16, meta: TapFrameMeta, lane: u8) {
        let port = self.port_for_lane(lane as usize);
        let packed = ((self.public_header.role() as u32) << 24)
            | ((meta.lane as u32) << 16)
            | ((meta.label as u32) << 8);
        let event = events::raw_event(id).with_arg0(meta.sid).with_arg1(packed);
        emit(port.tap(), event);
    }
}
