use super::{
    ScopeFacts, local_step_range_for_eff_range, parallel_exit_for_enter, scope_markers_contain_kind,
};
use crate::global::{
    const_dsl::{EffList, ScopeEvent, ScopeId, ScopeKind},
    role_program::LANE_DOMAIN_SIZE,
    typestate::{LocalDependency, PackedLocalDependency},
};

#[cfg(all(test, hibana_repo_tests))]
mod tests;

#[derive(Clone, Copy)]
struct CandidateKey {
    marker_index: u16,
    local_start: u16,
    local_end: u16,
}

impl CandidateKey {
    const NONE: Self = Self {
        marker_index: u16::MAX,
        local_start: u16::MAX,
        local_end: u16::MAX,
    };

    const fn new(marker_index: usize, local_start: usize, local_end: usize) -> Self {
        if marker_index >= u16::MAX as usize
            || local_start >= local_end
            || local_end > u16::MAX as usize
        {
            crate::invariant();
        }
        Self {
            marker_index: marker_index as u16,
            local_start: local_start as u16,
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
            || (candidate.local_end == self.local_end && candidate.marker_index < self.marker_index)
        {
            candidate
        } else {
            self
        }
    }
}

#[derive(Clone, Copy)]
struct ParallelInput {
    barrier: CandidateKey,
    outer_floor: usize,
}

pub(in crate::global::role_program::image_impl) struct DependencyCursor<'a, const E: usize> {
    eff_list: &'a EffList<E>,
    scopes: &'a ScopeFacts,
    role: u8,
    marker_index: usize,
    next_local_step: usize,
    previous_eff: Option<usize>,
    branch_barrier: CandidateKey,
    branch_floor: usize,
    parallel_inputs: [Option<ParallelInput>; ScopeId::LOCAL_CAPACITY as usize],
    lane_candidates: [CandidateKey; LANE_DOMAIN_SIZE],
    has_parallel: bool,
}

impl<'a, const E: usize> DependencyCursor<'a, E> {
    pub(in crate::global::role_program::image_impl) const fn new(
        eff_list: &'a EffList<E>,
        scopes: &'a ScopeFacts,
        role: u8,
    ) -> Self {
        let markers = eff_list.scope_markers();
        Self {
            eff_list,
            scopes,
            role,
            marker_index: 0,
            next_local_step: 0,
            previous_eff: None,
            branch_barrier: CandidateKey::NONE,
            branch_floor: 0,
            parallel_inputs: [None; ScopeId::LOCAL_CAPACITY as usize],
            lane_candidates: [CandidateKey::NONE; LANE_DOMAIN_SIZE],
            has_parallel: scope_markers_contain_kind(markers, ScopeKind::Parallel),
        }
    }

    const fn update_lane_candidates(
        &mut self,
        start_eff: usize,
        end_eff: usize,
        candidate: CandidateKey,
    ) {
        let mut eff_index = start_eff;
        while eff_index < end_eff {
            let atom = self.eff_list.atom_at(eff_index);
            if atom.from == self.role || atom.to == self.role {
                let lane = atom.lane as usize;
                self.lane_candidates[lane] = self.lane_candidates[lane].later(candidate);
            }
            eff_index += 1;
        }
    }

    const fn process_parallel_exit(&mut self, exit_index: usize) {
        let markers = self.eff_list.scope_markers();
        let enter_index = markers
            .first_enter_index(markers.at(exit_index).scope_id)
            .expect("parallel scope without entry");
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
        let candidate = CandidateKey::new(enter_index, row.start(), row.end());
        self.update_lane_candidates(enter.offset(), exit_eff, candidate);

        // A completed par contributes its whole local row, including both arms.
        // A subsequent sibling starts from the saved input at its Split marker.
        self.branch_barrier = candidate;
    }

    const fn process_boundaries_through(&mut self, current_eff: usize, local_step: usize) {
        let markers = self.eff_list.scope_markers();
        while self.marker_index < markers.len() {
            let offset = markers.at(self.marker_index).offset();
            if offset > current_eff {
                break;
            }
            let start = self.marker_index;
            while self.marker_index < markers.len()
                && markers.at(self.marker_index).offset() == offset
            {
                self.marker_index += 1;
            }
            // Lowering emits nested exits before their enclosing exit. At a
            // shared position, finish those joins before restoring a sibling's
            // input, then save that input for newly entered parallel scopes.
            // Storage places enters before splits, so its tie order cannot be
            // used as the execution order of these structural boundaries.
            let mut pass = 0;
            while pass < 3 {
                let mut index = start;
                while index < self.marker_index {
                    let marker = markers.at(index);
                    if matches!(marker.scope_id.kind(), Some(ScopeKind::Parallel)) {
                        let slot = marker.scope_id.local_ordinal() as usize;
                        match (pass, marker.event) {
                            (0, ScopeEvent::Exit) => {
                                let input = self.parallel_inputs[slot]
                                    .take()
                                    .expect("parallel exit without input");
                                self.branch_floor = input.outer_floor;
                                self.process_parallel_exit(index);
                            }
                            (1, ScopeEvent::Split) => {
                                self.branch_barrier = self.parallel_inputs[slot]
                                    .expect("parallel split without input")
                                    .barrier;
                                self.branch_floor = local_step;
                            }
                            (2, ScopeEvent::Enter(_)) => {
                                if self.parallel_inputs[slot].is_some()
                                    || self.branch_floor > local_step
                                {
                                    crate::invariant();
                                }
                                // Share only the sequential prefix, excluding enclosing siblings.
                                if self.branch_floor < local_step {
                                    self.branch_barrier = self.branch_barrier.later(
                                        CandidateKey::new(index, self.branch_floor, local_step),
                                    );
                                }
                                self.parallel_inputs[slot] = Some(ParallelInput {
                                    barrier: self.branch_barrier,
                                    outer_floor: self.branch_floor,
                                });
                                self.branch_floor = local_step;
                            }
                            _ => {}
                        }
                    }
                    index += 1;
                }
                pass += 1;
            }
        }
    }

    const fn dependency_for_candidate(&self, candidate: CandidateKey) -> PackedLocalDependency {
        if candidate.is_none() {
            return PackedLocalDependency::none();
        }
        let markers = self.eff_list.scope_markers();
        let marker_index = candidate.marker_index as usize;
        let marker = markers.at(marker_index);
        let conflict = self.scopes.conflict(marker.scope_id);
        PackedLocalDependency::from_dependency(LocalDependency::with_conflict_range(
            marker.scope_id,
            conflict,
            candidate.local_start as usize,
            candidate.local_end as usize,
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
        let atom = self.eff_list.atom_at(current_eff);
        if (atom.from != self.role && atom.to != self.role) || atom.lane != current_lane {
            crate::invariant();
        }
        self.previous_eff = Some(current_eff);
        self.next_local_step += 1;
        if !self.has_parallel {
            return PackedLocalDependency::none();
        }
        self.process_boundaries_through(current_eff, local_step);
        let candidate = self
            .branch_barrier
            .later(self.lane_candidates[current_lane as usize]);
        if !candidate.is_none() && candidate.local_end as usize > local_step {
            crate::invariant();
        }
        self.dependency_for_candidate(candidate)
    }
}
