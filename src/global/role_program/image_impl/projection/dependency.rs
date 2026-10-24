use super::{LocalRows, ScopeFacts, local_step_range_for_eff_range, scope_markers_contain_kind};
use crate::global::{
    const_dsl::{EffList, ScopeEvent, ScopeId, ScopeKind, route_arm_ranges_from_first_enter},
    role_program::LANE_DOMAIN_SIZE,
    typestate::{LocalConflict, LocalDependency, PackedLocalDependency},
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

    const fn new(marker_index: Option<usize>, local_start: usize, local_end: usize) -> Self {
        if local_start >= local_end || local_end > u16::MAX as usize {
            crate::invariant();
        }
        let marker_index = match marker_index {
            Some(index) if index < u16::MAX as usize => index as u16,
            Some(_) => crate::invariant(),
            None => u16::MAX,
        };
        Self {
            marker_index,
            local_start: local_start as u16,
            local_end: local_end as u16,
        }
    }

    const fn is_none(self) -> bool {
        self.local_start == u16::MAX
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
struct CompositionInput {
    barrier: CandidateKey,
    outer_floor: u16,
    context: u16,
}

pub(in crate::global::role_program::image_impl) struct DependencyCursor<'a, const E: usize> {
    eff_list: &'a EffList<E>,
    scopes: &'a ScopeFacts,
    rows: LocalRows<'a, E>,
    marker_index: usize,
    next_local_step: usize,
    previous_eff: Option<(usize, usize)>,
    event_dependency: PackedLocalDependency,
    branch_barrier: CandidateKey,
    branch_floor: usize,
    context: u16,
    inputs: [[Option<CompositionInput>; ScopeId::LOCAL_CAPACITY as usize]; 2],
    lane_candidates: [CandidateKey; LANE_DOMAIN_SIZE],
    needs_dependencies: bool,
}

impl<'a, const E: usize> DependencyCursor<'a, E> {
    pub(in crate::global::role_program::image_impl) const fn new(
        eff_list: &'a EffList<E>,
        scopes: &'a ScopeFacts,
        rows: LocalRows<'a, E>,
    ) -> Self {
        let markers = eff_list.scope_markers();
        let mut cross_lane = false;
        let mut event = 1;
        while event < eff_list.len() && !cross_lane {
            cross_lane = eff_list.atom_at(event).lane != eff_list.atom_at(0).lane;
            event += 1;
        }
        Self {
            eff_list,
            scopes,
            rows,
            marker_index: 0,
            next_local_step: 0,
            previous_eff: None,
            event_dependency: PackedLocalDependency::none(),
            branch_barrier: CandidateKey::NONE,
            branch_floor: 0,
            context: u16::MAX,
            inputs: [[None; ScopeId::LOCAL_CAPACITY as usize]; 2],
            lane_candidates: [CandidateKey::NONE; LANE_DOMAIN_SIZE],
            needs_dependencies: cross_lane
                || scope_markers_contain_kind(markers, ScopeKind::Parallel),
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
            if self.rows.count(self.eff_list, eff_index) != 0 {
                let lane = atom.lane as usize;
                self.lane_candidates[lane] = self.lane_candidates[lane].later(candidate);
            }
            eff_index += 1;
        }
    }

    const fn process_join(&mut self, exit_index: usize) {
        let markers = self.eff_list.scope_markers();
        let enter_index = self.scopes.first_enter(markers.at(exit_index).scope_id);
        let enter = markers.at(enter_index);
        let exit_eff = markers.at(exit_index).offset();
        if matches!(enter.scope_id.kind(), Some(ScopeKind::Route)) {
            let [_, (_, end)] = route_arm_ranges_from_first_enter(markers, enter_index);
            // A route has an exit for each arm. The first exit is not a join
            // and must not become the other arm's sequential input.
            if exit_eff != end {
                return;
            }
        }
        let kind = Self::composition_kind(enter.scope_id);
        let input = self.inputs[kind][enter.scope_id.local_ordinal() as usize]
            .take()
            .expect("composition exit without input");
        self.branch_floor = input.outer_floor as usize;
        self.branch_barrier = input.barrier;
        self.context = input.context;
        let row =
            local_step_range_for_eff_range(self.eff_list, enter.offset(), exit_eff, self.rows);
        if row.is_absent_or_zero_len() {
            return;
        }
        let candidate = CandidateKey::new(Some(enter_index), row.start(), row.end());
        self.update_lane_candidates(enter.offset(), exit_eff, candidate);

        // The join covers both arms. Their selected rows supply the preceding
        // work; each arm starts from the composition's saved input.
        self.branch_barrier = candidate;
    }

    const fn composition_kind(scope: ScopeId) -> usize {
        match scope.kind() {
            Some(ScopeKind::Parallel) => 0,
            Some(ScopeKind::Route) => 1,
            _ => crate::invariant(),
        }
    }

    const fn enter_composition(&mut self, index: usize, local_step: usize) {
        let marker = self.eff_list.scope_markers().at(index);
        let kind = Self::composition_kind(marker.scope_id);
        let slot = marker.scope_id.local_ordinal() as usize;
        if self.inputs[kind][slot].is_some()
            || self.branch_floor > local_step
            || self.branch_floor > u16::MAX as usize
        {
            crate::invariant();
        }
        if self.branch_floor < local_step {
            self.branch_barrier = self.branch_barrier.later(CandidateKey::new(
                Some(index),
                self.branch_floor,
                local_step,
            ));
        }
        self.inputs[kind][slot] = Some(CompositionInput {
            barrier: self.branch_barrier,
            outer_floor: self.branch_floor as u16,
            context: self.context,
        });
        self.branch_floor = local_step;
        if index >= u16::MAX as usize {
            crate::invariant();
        }
        self.context = index as u16;
    }

    const fn start_sibling(&mut self, scope: ScopeId, local_step: usize) {
        self.branch_barrier = self.inputs[Self::composition_kind(scope)]
            [scope.local_ordinal() as usize]
            .expect("composition sibling without input")
            .barrier;
        self.branch_floor = local_step;
        let enter = self.scopes.first_enter(scope);
        if enter >= u16::MAX as usize {
            crate::invariant();
        }
        self.context = enter as u16;
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
                        match (pass, marker.event) {
                            (0, ScopeEvent::Exit) => self.process_join(index),
                            (1, ScopeEvent::Split) => {
                                self.start_sibling(marker.scope_id, local_step);
                            }
                            (2, ScopeEvent::Enter(_)) => {
                                self.enter_composition(index, local_step);
                            }
                            _ => {}
                        }
                    } else if matches!(marker.scope_id.kind(), Some(ScopeKind::Route)) {
                        match (pass, marker.event.route_arm(), marker.event) {
                            (0, _, ScopeEvent::Exit) => self.process_join(index),
                            (1, Some(1), _) => self.start_sibling(marker.scope_id, local_step),
                            (2, Some(0), _) => self.enter_composition(index, local_step),
                            _ => (),
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
        if candidate.marker_index == u16::MAX {
            return PackedLocalDependency::from_dependency(LocalDependency::with_conflict_range(
                ScopeId::none(),
                LocalConflict::Unconditional,
                candidate.local_start as usize,
                candidate.local_end as usize,
            ));
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

    const fn applicable_candidate(&self, candidate: CandidateKey, event: usize) -> CandidateKey {
        if candidate.is_none() || candidate.marker_index == u16::MAX {
            return candidate;
        }
        let markers = self.eff_list.scope_markers();
        let mut scope = markers.at(candidate.marker_index as usize).scope_id;
        loop {
            match self.scopes.conflict(scope) {
                LocalConflict::Unconditional => return candidate,
                LocalConflict::SharedRoute => return CandidateKey::NONE,
                LocalConflict::RouteArm { scope: parent, arm } => {
                    let enter = self.scopes.first_enter(parent);
                    let [(start, split), (_, end)] =
                        route_arm_ranges_from_first_enter(markers, enter);
                    if start <= event
                        && event < end
                        && ((event < split && arm != 0) || (event >= split && arm != 1))
                    {
                        return CandidateKey::NONE;
                    }
                    scope = parent;
                }
            }
        }
    }

    pub(in crate::global::role_program::image_impl) const fn next(
        &mut self,
        current_eff: usize,
        current_lane: u8,
        local_step: usize,
    ) -> PackedLocalDependency {
        let side = match self.previous_eff {
            Some((previous, side)) if previous == current_eff => side + 1,
            Some((previous, _)) if previous > current_eff => crate::invariant(),
            Some(_) | None => 0,
        };
        if local_step != self.next_local_step || side >= self.rows.count(self.eff_list, current_eff)
        {
            crate::invariant();
        }
        let atom = self.eff_list.atom_at(current_eff);
        if atom.lane != current_lane {
            crate::invariant();
        }
        let crosses_lane = match self.previous_eff {
            Some((previous, _)) => self.eff_list.atom_at(previous).lane != current_lane,
            None => false,
        };
        self.previous_eff = Some((current_eff, side));
        self.next_local_step += 1;
        // Both halves of one message have the same incoming source prefix.
        // Its local send can be absent on the selected ownership path, so a
        // receive cannot rely on that row to carry the prefix transitively.
        if side != 0 {
            return self.event_dependency;
        }
        if !self.needs_dependencies {
            return PackedLocalDependency::none();
        }
        self.process_boundaries_through(current_eff, local_step);
        let mut candidate = self
            .applicable_candidate(self.branch_barrier, current_eff)
            .later(
                self.applicable_candidate(self.lane_candidates[current_lane as usize], current_eff),
            );
        if crosses_lane && self.branch_floor < local_step {
            // Lane FIFO cannot order a sequential prefix on another lane.
            // The floor excludes parallel siblings; composition exits restore
            // the enclosing segment before its continuation starts.
            candidate = candidate.later(CandidateKey::new(
                if self.context == u16::MAX {
                    None
                } else {
                    Some(self.context as usize)
                },
                self.branch_floor,
                local_step,
            ));
        }
        if !candidate.is_none() && candidate.local_end as usize > local_step {
            crate::invariant();
        }
        let dependency = self.dependency_for_candidate(candidate);
        self.event_dependency = dependency;
        dependency
    }
}
