use super::{
    CommitDelta, CursorEndpoint, CursorInvariantError, ScopeId, SelectedRouteCommitRow,
    SelectedRouteCommitRowsRef, Transport, state_index_to_usize,
};

impl<'r, const ROLE: u8, T> CursorEndpoint<'r, ROLE, T>
where
    T: Transport + 'r,
{
    pub(in crate::endpoint::kernel::core) fn preflight_route_only_cursor_after(
        &self,
        delta: &CommitDelta,
    ) -> Result<(), CursorInvariantError> {
        let routes = delta.selected_routes();
        if routes.is_empty() {
            return Err(CursorInvariantError::INVARIANT);
        }
        if delta.selected_route_lane().is_none() {
            return Err(CursorInvariantError::INVARIANT);
        }
        let route_rows = delta.selected_route_rows_ref();
        let cursor_after = state_index_to_usize(delta.cursor_after());
        (cursor_after == self.cursor.index()
            || self.route_only_cursor_after_is_materialization_target(route_rows, cursor_after))
        .then_some(())
        .ok_or(CursorInvariantError::INVARIANT)
    }

    fn route_only_rows_arm_for_scope(
        &self,
        routes: SelectedRouteCommitRowsRef,
        scope: ScopeId,
    ) -> Option<u8> {
        let mut idx = 0usize;
        while idx < routes.len() {
            let row = routes.get(&self.cursor, idx)?;
            if row.scope() == scope {
                return Some(row.selected_arm());
            }
            idx += 1;
        }
        None
    }

    #[inline(never)]
    fn route_only_cursor_after_is_materialization_target(
        &self,
        routes: SelectedRouteCommitRowsRef,
        cursor_after: usize,
    ) -> bool {
        let mut idx = 0usize;
        while idx < routes.len() {
            let Some(row) = routes.get(&self.cursor, idx) else {
                return false;
            };
            if self.route_only_row_materializes_cursor_after(routes, row, cursor_after) {
                return true;
            }
            idx += 1;
        }
        false
    }

    fn route_only_row_materializes_cursor_after(
        &self,
        routes: SelectedRouteCommitRowsRef,
        row: SelectedRouteCommitRow,
        cursor_after: usize,
    ) -> bool {
        let scope = row.scope();
        let arm = row.selected_arm();
        let Some(nested_scope) = self.cursor.passive_child_scope(scope, arm) else {
            return false;
        };
        let Some(expected) = self.cursor.visit_passive_route_materialization_rows(
            scope,
            nested_scope,
            arm,
            |candidate| match self.route_only_rows_arm_for_scope(routes, candidate) {
                Some(prepared) => Some(prepared),
                None => self.selected_arm_for_scope(candidate),
            },
            |candidate, selected| {
                self.route_only_rows_arm_for_scope(routes, candidate) == Some(selected)
                    || self.selected_arm_for_scope(candidate) == Some(selected)
            },
        ) else {
            return false;
        };
        expected == cursor_after
    }
}
