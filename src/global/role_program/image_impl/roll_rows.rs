use super::super::{MAX_LOCAL_STEP_LANES, PackedRollScopeRow, RoleLaneScratch, ScopeKind};
use crate::global::const_dsl::EffList;

impl RoleLaneScratch {
    pub(super) const fn push_roll_scope_rows(&mut self, eff_list: &EffList, role: u8) {
        let markers = eff_list.scope_markers();
        if !Self::scope_markers_contain_kind(markers, ScopeKind::Roll) {
            return;
        }
        let mut marker_idx = 0usize;
        while marker_idx < markers.len() {
            let marker = markers[marker_idx];
            if Self::first_enter_for_scope(markers, marker_idx)
                && matches!(marker.scope_id.kind(), Some(ScopeKind::Roll))
            {
                let end_eff = Self::scope_segment_end(markers, marker_idx, eff_list.len());
                let row =
                    Self::local_step_range_for_eff_range(eff_list, marker.offset(), end_eff, role);
                if !row.is_absent_or_zero_len() {
                    let idx = self.roll_scope_row_len as usize;
                    if idx >= MAX_LOCAL_STEP_LANES {
                        panic!("roll scope row overflow");
                    }
                    self.roll_scope_rows[idx] = PackedRollScopeRow::new(marker.scope_id, row);
                    self.roll_scope_row_len += 1;
                }
            }
            marker_idx += 1;
        }
    }
}
