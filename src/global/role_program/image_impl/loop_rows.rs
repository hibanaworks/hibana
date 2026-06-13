use super::super::{MAX_LOCAL_STEP_LANES, PackedLoopScopeRow, RoleLaneScratch, ScopeKind};
use crate::global::compiled::lowering::CompiledProgramImage;

impl RoleLaneScratch {
    #[inline(always)]
    pub(super) const fn push_loop_scope_rows<const ROLE: u8>(
        &mut self,
        program: &CompiledProgramImage,
    ) {
        let view = program.view();
        let markers = view.scope_markers();
        let mut marker_idx = 0usize;
        while marker_idx < markers.len() {
            let marker = markers[marker_idx];
            if Self::first_enter_for_scope(markers, marker_idx)
                && matches!(marker.scope_kind, ScopeKind::Loop)
            {
                let end_eff = Self::scope_segment_end(markers, marker_idx, view.len());
                let row =
                    Self::local_step_range_for_eff_range::<ROLE>(program, marker.offset, end_eff);
                if !row.is_absent_or_zero_len() {
                    let idx = self.loop_scope_row_len as usize;
                    if idx >= MAX_LOCAL_STEP_LANES {
                        panic!("roll scope row overflow");
                    }
                    self.loop_scope_rows[idx] = PackedLoopScopeRow::new(marker.scope_id, row);
                    self.loop_scope_row_len += 1;
                }
            }
            marker_idx += 1;
        }
    }
}
