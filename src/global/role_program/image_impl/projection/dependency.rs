use super::{
    dependency_conflict_for_scope, local_step_range_for_eff_range, nearest_parent_parallel_end,
    parallel_exit_for_enter, scope_markers_contain_kind,
};
use crate::{
    eff::EffKind,
    global::{
        const_dsl::{EffList, ScopeEvent, ScopeKind, ScopeMarkerView},
        role_program::LANE_DOMAIN_SIZE,
        typestate::{LocalConflict, LocalDependency, PackedLocalDependency},
    },
};

#[cfg(all(test, hibana_repo_tests))]
mod tests;

#[derive(Clone, Copy)]
struct CandidateKey {
    marker_index: u16,
    local_end: u16,
}

impl CandidateKey {
    const NONE: Self = Self {
        marker_index: u16::MAX,
        local_end: u16::MAX,
    };

    const fn new(marker_index: usize, local_end: usize) -> Self {
        if marker_index >= u16::MAX as usize || local_end > u16::MAX as usize {
            crate::invariant();
        }
        Self {
            marker_index: marker_index as u16,
            local_end: local_end as u16,
        }
    }

    const fn is_none(self) -> bool {
        self.marker_index == u16::MAX
    }

    const fn later(self, candidate: Self) -> Self {
        if candidate.is_none() {
            return self;
        }
        if self.is_none()
            || candidate.local_end > self.local_end
            || (candidate.local_end == self.local_end && candidate.marker_index > self.marker_index)
        {
            candidate
        } else {
            self
        }
    }
}

pub(in crate::global::role_program::image_impl) struct DependencyCursor<'a, const E: usize> {
    eff_list: &'a EffList<E>,
    role: u8,
    marker_index: usize,
    next_local_step: usize,
    previous_eff: Option<usize>,
    latest_completed: CandidateKey,
    globally_enabled: CandidateKey,
    lane_candidates: [CandidateKey; LANE_DOMAIN_SIZE],
    has_route: bool,
    has_parallel: bool,
}

impl<'a, const E: usize> DependencyCursor<'a, E> {
    pub(in crate::global::role_program::image_impl) const fn new(
        eff_list: &'a EffList<E>,
        role: u8,
    ) -> Self {
        let markers = eff_list.scope_markers();
        Self {
            eff_list,
            role,
            marker_index: 0,
            next_local_step: 0,
            previous_eff: None,
            latest_completed: CandidateKey::NONE,
            globally_enabled: CandidateKey::NONE,
            lane_candidates: [CandidateKey::NONE; LANE_DOMAIN_SIZE],
            has_route: scope_markers_contain_kind(markers, ScopeKind::Route),
            has_parallel: scope_markers_contain_kind(markers, ScopeKind::Parallel),
        }
    }

    const fn parallel_enter_index(markers: ScopeMarkerView<'_>, exit_index: usize) -> usize {
        let exit = markers.at(exit_index);
        let mut index = 0usize;
        while index < markers.len() {
            let marker = markers.at(index);
            if matches!(marker.event, ScopeEvent::Enter)
                && matches!(marker.scope_id.kind(), Some(ScopeKind::Parallel))
                && marker.scope_id.same(exit.scope_id)
            {
                return index;
            }
            index += 1;
        }
        crate::invariant()
    }

    const fn update_lane_candidates(
        &mut self,
        start_eff: usize,
        end_eff: usize,
        candidate: CandidateKey,
    ) {
        let mut eff_index = start_eff;
        while eff_index < end_eff {
            let node = self.eff_list.node_at(eff_index);
            if matches!(node.kind, EffKind::Atom) {
                let atom = node.atom_data();
                if atom.from == self.role || atom.to == self.role {
                    let lane = atom.lane as usize;
                    self.lane_candidates[lane] = self.lane_candidates[lane].later(candidate);
                }
            }
            eff_index += 1;
        }
    }

    const fn process_parallel_exit(&mut self, exit_index: usize) {
        let markers = self.eff_list.scope_markers();
        let enter_index = Self::parallel_enter_index(markers, exit_index);
        let enter = markers.at(enter_index);
        let exit_eff = parallel_exit_for_enter(markers, enter_index);
        if markers.at(exit_index).offset() != exit_eff {
            crate::invariant();
        }
        let row =
            local_step_range_for_eff_range(self.eff_list, enter.offset(), exit_eff, self.role);
        if row.is_absent_or_zero_len() {
            return;
        }
        let candidate = CandidateKey::new(enter_index, row.end());
        self.latest_completed = self.latest_completed.later(candidate);
        self.update_lane_candidates(enter.offset(), exit_eff, candidate);

        if nearest_parent_parallel_end(markers, enter_index, exit_eff) == exit_eff {
            self.globally_enabled = self.globally_enabled.later(self.latest_completed);
        }
    }

    const fn process_boundaries_through(&mut self, current_eff: usize) {
        let markers = self.eff_list.scope_markers();
        while self.marker_index < markers.len() {
            let marker = markers.at(self.marker_index);
            if marker.offset() > current_eff {
                break;
            }
            if matches!(marker.event, ScopeEvent::Exit)
                && matches!(marker.scope_id.kind(), Some(ScopeKind::Parallel))
            {
                self.process_parallel_exit(self.marker_index);
            }
            self.marker_index += 1;
        }
    }

    const fn dependency_for_candidate(&self, candidate: CandidateKey) -> PackedLocalDependency {
        if candidate.is_none() {
            return PackedLocalDependency::none();
        }
        let markers = self.eff_list.scope_markers();
        let marker_index = candidate.marker_index as usize;
        let marker = markers.at(marker_index);
        let exit_eff = parallel_exit_for_enter(markers, marker_index);
        let row =
            local_step_range_for_eff_range(self.eff_list, marker.offset(), exit_eff, self.role);
        if row.is_absent_or_zero_len() || row.end() != candidate.local_end as usize {
            crate::invariant();
        }
        let conflict = if self.has_route {
            dependency_conflict_for_scope(markers, self.eff_list.len(), marker.scope_id)
        } else {
            LocalConflict::Unconditional
        };
        PackedLocalDependency::from_dependency(LocalDependency::with_conflict_range(
            marker.scope_id,
            conflict,
            row.start(),
            row.end(),
        ))
    }

    pub(in crate::global::role_program::image_impl) const fn next(
        &mut self,
        current_eff: usize,
        current_lane: u8,
        local_step: usize,
    ) -> PackedLocalDependency {
        let ordered = match self.previous_eff {
            Some(previous) => previous < current_eff,
            None => true,
        };
        if local_step != self.next_local_step || !ordered {
            crate::invariant();
        }
        let node = self.eff_list.node_at(current_eff);
        if !matches!(node.kind, EffKind::Atom) {
            crate::invariant();
        }
        let atom = node.atom_data();
        if (atom.from != self.role && atom.to != self.role) || atom.lane != current_lane {
            crate::invariant();
        }
        self.previous_eff = Some(current_eff);
        self.next_local_step += 1;
        if !self.has_parallel {
            return PackedLocalDependency::none();
        }
        self.process_boundaries_through(current_eff);
        let candidate = self
            .globally_enabled
            .later(self.lane_candidates[current_lane as usize]);
        if !candidate.is_none() && candidate.local_end as usize > local_step {
            crate::invariant();
        }
        self.dependency_for_candidate(candidate)
    }
}
