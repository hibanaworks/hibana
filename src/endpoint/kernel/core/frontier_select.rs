use super::{OperationKernel, ScopeArmMaterializationMeta, ScopeId, Transport};
impl<'r, T> OperationKernel<'_, 'r, T>
where
    T: Transport + 'r,
{
    pub(in crate::endpoint::kernel) fn compute_scope_arm_materialization_meta(
        &self,
        scope_id: ScopeId,
    ) -> ScopeArmMaterializationMeta {
        if self
            .progress
            .cursor
            .route_scope_arm_count(scope_id)
            .is_none()
        {
            crate::invariant();
        }
        let mut meta = ScopeArmMaterializationMeta::EMPTY;
        let mut arm = 0u8;
        while arm <= 1 {
            let arm_idx = arm as usize;
            if let Some(entry) = self
                .progress
                .cursor
                .passive_observer_arm_entry(scope_id, arm)
            {
                meta.passive_arm_entry[arm_idx] = entry;
            }
            if let Some(scope) = self.progress.cursor.passive_child_scope(scope_id, arm) {
                meta.passive_child_scope[arm_idx] = scope;
            }
            if arm == 1 {
                break;
            }
            arm += 1;
        }
        if let Some(arm_mask) = self
            .progress
            .cursor
            .route_scope_first_recv_dispatch_arm_mask(scope_id)
        {
            meta.record_first_recv_dispatch(arm_mask);
        }
        meta
    }

    /// The caller established that this view controls an unresolved dynamic
    /// scope. A resolver can run without ingress only at an eligible projected
    /// Send/Local entry; this read neither invokes it nor advances progress.
    pub(in crate::endpoint::kernel) fn resolver_offer_entry_enabled(
        &self,
        scope: ScopeId,
        previews: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> bool {
        use crate::global::{role_program::RoleLocalDirection, typestate::EventArmView};

        for arm in 0..=1 {
            let Some((entry, _)) = self.progress.cursor.controller_arm_entry_by_arm(scope, arm)
            else {
                continue;
            };
            let idx = entry.as_usize();
            // This resolver selects only its own choice. Select any unresolved
            // ancestor before considering the descendant as runnable work.
            if self.offer_scope_id_at(idx, previews) != scope {
                continue;
            }
            let Some(row) = self.progress.cursor.event_row_at(idx) else {
                continue;
            };
            if row.logical_role() != self.public_header.role()
                || row.direction() == RoleLocalDirection::Recv
            {
                continue;
            }
            let mut selected = |candidate, view| match view {
                EventArmView::Committed => self.selected_arm_for_scope(candidate),
                EventArmView::Preview if candidate == scope => Some(arm),
                EventArmView::Preview => previews(candidate)
                    .or_else(|| self.selected_arm_for_recv_event(row.conflict(), candidate, view)),
            };
            if self.event_eligibility(row, &mut selected).is_ok() {
                return true;
            }
        }
        false
    }
}
