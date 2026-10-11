use super::parallel_exit_for_enter;
use crate::global::{
    const_dsl::{EffList, ScopeKind},
    role_program::PackedLaneRange,
};

struct LocalStepCursor<'a, const E: usize> {
    eff_list: &'a EffList<E>,
    role: u8,
    eff_index: usize,
    local_step: usize,
}

impl<'a, const E: usize> LocalStepCursor<'a, E> {
    const fn new(eff_list: &'a EffList<E>, role: u8) -> Self {
        Self {
            eff_list,
            role,
            eff_index: 0,
            local_step: 0,
        }
    }

    const fn advance_to(&mut self, end: usize) {
        let limit = if end < self.eff_list.len() {
            end
        } else {
            self.eff_list.len()
        };
        while self.eff_index < limit {
            let atom = self.eff_list.atom_at(self.eff_index);
            if atom.from == self.role || atom.to == self.role {
                self.local_step += 1;
            }
            self.eff_index += 1;
        }
    }

    const fn range(&mut self, start: usize, end: usize) -> PackedLaneRange {
        if start >= end {
            return PackedLaneRange::new(0, 0);
        }
        if start < self.eff_index {
            crate::invariant();
        }
        self.advance_to(start);
        let local_start = self.local_step;
        self.advance_to(end);
        let local_len = self.local_step - local_start;
        if local_len == 0 {
            PackedLaneRange::new(0, 0)
        } else {
            PackedLaneRange::new(local_start, local_len)
        }
    }
}

pub(in crate::global::role_program::image_impl) struct ResidentRowCursor<'a, const E: usize> {
    eff_list: &'a EffList<E>,
    marker_index: usize,
    current_eff: usize,
    pending: Option<PackedLaneRange>,
    emitted_rows: usize,
    finished: bool,
    local: LocalStepCursor<'a, E>,
}

impl<'a, const E: usize> ResidentRowCursor<'a, E> {
    pub(in crate::global::role_program::image_impl) const fn new(
        eff_list: &'a EffList<E>,
        role: u8,
    ) -> Self {
        Self {
            eff_list,
            marker_index: 0,
            current_eff: 0,
            pending: None,
            emitted_rows: 0,
            finished: false,
            local: LocalStepCursor::new(eff_list, role),
        }
    }

    const fn emit(&mut self, row: PackedLaneRange) -> Option<PackedLaneRange> {
        self.emitted_rows += 1;
        Some(row)
    }

    pub(in crate::global::role_program::image_impl) const fn next(
        &mut self,
    ) -> Option<PackedLaneRange> {
        if let Some(row) = self.pending {
            self.pending = None;
            return self.emit(row);
        }
        let markers = self.eff_list.scope_markers();
        while self.marker_index < markers.len() {
            let marker_index = self.marker_index;
            let marker = markers.at(marker_index);
            self.marker_index += 1;
            if !marker.event.is_primary_enter()
                || !matches!(marker.scope_id.kind(), Some(ScopeKind::Parallel))
            {
                continue;
            }
            let exit_eff = parallel_exit_for_enter(markers, marker_index);
            let before = self.local.range(self.current_eff, marker.offset());
            let parallel_start = if marker.offset() > self.current_eff {
                marker.offset()
            } else {
                self.current_eff
            };
            let parallel = self.local.range(parallel_start, exit_eff);
            if exit_eff > self.current_eff {
                self.current_eff = exit_eff;
            }
            let before_present = !before.is_absent_or_zero_len();
            let parallel_present = !parallel.is_absent_or_zero_len();
            if before_present {
                if parallel_present {
                    self.pending = Some(parallel);
                }
                return self.emit(before);
            }
            if parallel_present {
                return self.emit(parallel);
            }
        }
        if !self.finished {
            self.finished = true;
            let trailing = self.local.range(self.current_eff, self.eff_list.len());
            if !trailing.is_absent_or_zero_len() {
                return self.emit(trailing);
            }
            if self.emitted_rows == 0 && self.local.local_step != 0 {
                crate::invariant();
            }
        }
        None
    }
}
