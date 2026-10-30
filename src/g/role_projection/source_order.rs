//! Const translation validation for the source and the emitted local guards.
//! This reads immutable projection data; it adds no runtime progress state.
use super::{ProgramProjection, ProgramShape};
use crate::{
    g::{
        ProgramSourceData, ProgramSourceNode, SourceArm, SourceContext, SourcePayload, SourceScope,
        SourceSelection,
    },
    global::{
        const_dsl::{ScopeId, ScopeKind},
        role_program::RoleImageRef,
        typestate::{LocalAction, LocalConflict, LocalDependency},
    },
};

mod presence;
#[cfg(all(test, hibana_repo_tests))]
mod reference;
use presence::SourcePresence;
#[cfg(kani)]
mod kani;

impl<Steps: ProgramShape, const E: usize> ProgramProjection<Steps, E> {
    const SOURCE_PRESENCE: SourcePresence<E> = SourcePresence::new(&Steps::SOURCE_NODE);
}

pub(super) const fn check<Steps: ProgramShape, const E: usize>(image: &RoleImageRef) -> bool {
    let result = word_check::<Steps, E>(image);
    #[cfg(all(test, hibana_repo_tests))]
    if result != reference::check::<Steps, E>(image) {
        panic!("word and pairwise source-order validation disagree");
    }
    result
}

const fn word_check<Steps: ProgramShape, const E: usize>(image: &RoleImageRef) -> bool {
    let data = &ProgramProjection::<Steps, E>::SOURCE;
    if data.actors().is_none() {
        return true;
    }
    let mut check = SourceOrder {
        source: &Steps::SOURCE_NODE,
        data,
        image,
        rows: [[None; 2]; E],
        presence: &ProgramProjection::<Steps, E>::SOURCE_PRESENCE,
        expected: RowBits::empty(),
        covers: RowBits::empty(),
        excluded: RowBits::empty(),
        lanes: LaneBits::empty(),
        source_rows: [0; E],
        guard_rows: [u16::MAX; E],
    };
    if !check.same_source() {
        return false;
    }
    let row_count = image.local_step_count();
    if row_count > E * 2 {
        return false;
    }
    let mut starts = [[u16::MAX; 2]; E];
    let mut ends = [[u16::MAX; 2]; E];
    let words = row_count.div_ceil(64);
    let mut row = 0;
    let mut source = 0;
    while row < row_count {
        let Some(node) = image.local_step_node(row) else {
            crate::invariant()
        };
        let coordinates = SourceOrder::<E>::coordinates(node.action());
        while source <= coordinates.0 {
            check.source_rows[source] = row as u16;
            source += 1;
        }
        let Some(actors) = data.actors() else {
            crate::invariant()
        };
        let Some(references) = actors.event_references(coordinates.0) else {
            crate::invariant()
        };
        let actor = if matches!(node.action(), LocalAction::Recv { .. }) {
            references[1]
        } else {
            references[0]
        };
        let dependency = image.dependency_for_index(row);
        // Both representations are forward-only. Once these bounds hold,
        // every reversed row pair has expected=false and actual=false.
        if row != 0 && check.row(row - 1).coordinates.0 > coordinates.0 {
            return false;
        }
        if let Some(dependency) = dependency
            && dependency.end() > row
        {
            return false;
        }
        let guard_scope = match dependency {
            Some(dependency) => match dependency.conflict() {
                LocalConflict::RouteArm { scope, .. } => Some(check.source_scope(scope)),
                LocalConflict::Unconditional | LocalConflict::SharedRoute => None,
            },
            None => None,
        };
        let (next_start, next_end) = match dependency {
            Some(edge)
                if edge.start() < edge.end()
                    && !matches!(edge.conflict(), LocalConflict::SharedRoute) =>
            {
                let first = edge.start();
                let last = edge.end();
                let next = (starts[first / 2][first % 2], ends[last / 2][last % 2]);
                starts[first / 2][first % 2] = row as u16;
                ends[last / 2][last % 2] = row as u16;
                next
            }
            _ => (u16::MAX, u16::MAX),
        };
        let next_guard = match (dependency, guard_scope) {
            (Some(edge), Some(scope)) => match edge.conflict() {
                LocalConflict::RouteArm { .. }
                    if check.presence.required_arm(coordinates.0, scope).is_none() =>
                {
                    let head = &mut check.guard_rows[scope.split as usize];
                    let next = *head;
                    *head = row as u16;
                    next
                }
                LocalConflict::RouteArm { .. } => u16::MAX,
                LocalConflict::Unconditional | LocalConflict::SharedRoute => crate::invariant(),
            },
            _ => u16::MAX,
        };
        let lane_word = coordinates.1 as usize * words + row / 64;
        check.lanes.insert(lane_word, row % 64);
        check.rows[row / 2][row % 2] = Some(SourceRow {
            action: node.action(),
            dependency,
            guard_scope,
            coordinates,
            actor,
            next_start,
            next_end,
            next_guard,
        });
        row += 1;
    }
    while source < check.source.event_count() {
        check.source_rows[source] = row_count as u16;
        source += 1;
    }
    let mut possible = RowBits::<E>::empty();
    let mut certain = RowBits::<E>::empty();
    let mut selections = [None; E];
    let mut known_true = RowBits::<E>::empty();
    let mut known_false = RowBits::<E>::empty();
    let mut before = 0;
    while before < row_count {
        let mut target = ends[before / 2][before % 2];
        while target != u16::MAX {
            possible.set(target as usize, false);
            certain.set(target as usize, false);
            target = check.row(target as usize).next_end;
        }
        let mut target = starts[before / 2][before % 2];
        while target != u16::MAX {
            let row = check.row(target as usize);
            // Bounds need only hold when the target occurs. Its source path
            // can settle a guarded dependency before any row-pair comparison.
            let activation = match row.dependency {
                Some(edge) => match edge.conflict() {
                    LocalConflict::Unconditional => Some(true),
                    LocalConflict::SharedRoute => Some(false),
                    LocalConflict::RouteArm { arm, .. } => match row.guard_scope {
                        Some(scope) => {
                            match check.presence.required_arm(row.coordinates.0, scope) {
                                Some(required) => Some(required as u8 == arm),
                                None => None,
                            }
                        }
                        None => crate::invariant(),
                    },
                },
                None => crate::invariant(),
            };
            possible.set(target as usize, !matches!(activation, Some(false)));
            certain.set(target as usize, matches!(activation, Some(true)));
            target = row.next_start;
        }
        check.expected.clear(words);
        check.covers.clear(words);
        check.excluded.clear(words);
        let (first, lane) = check.row(before).coordinates;
        known_true.clear(words);
        known_false.clear(words);
        let mut condition = check.presence.first_choice(first);
        while condition != u16::MAX {
            let (choice, parent) = check.presence.choice(condition);
            selections[choice.scope.split as usize] = Some(choice);
            let mut target = check.guard_rows[choice.scope.split as usize];
            while target != u16::MAX {
                let row = check.row(target as usize);
                let Some(edge) = row.dependency else {
                    crate::invariant();
                };
                let LocalConflict::RouteArm { arm, .. } = edge.conflict() else {
                    crate::invariant();
                };
                if arm == choice.arm as u8 {
                    known_true.set(target as usize, true);
                } else {
                    known_false.set(target as usize, true);
                }
                target = row.next_guard;
            }
            condition = parent;
        }
        check.mark_successors(check.source, 0, first, before);
        if matches!(check.operation(before), LocalAction::Send { .. })
            && before + 1 < row_count
            && check.row(before + 1).coordinates.0 == first
            && matches!(check.operation(before + 1), LocalAction::Recv { .. })
        {
            check.expected.set(before + 1, true);
        }
        #[cfg(all(test, hibana_repo_tests))]
        reference::assert_cover_candidates(&check, before);
        let mut word = before / 64;
        while word < words {
            let lane_word = lane as usize * words + word;
            let same_lane = check.lanes.get(lane_word)
                & if word != before / 64 {
                    u64::MAX
                } else if before % 64 == 63 {
                    0
                } else {
                    u64::MAX << (before % 64 + 1)
                };
            let expected = check.expected.words[word];
            let possible_bound = possible.words[word];
            let possible = same_lane | (possible_bound & !known_false.words[word]);
            let certain =
                same_lane | certain.words[word] | (possible_bound & known_true.words[word]);
            #[cfg(all(test, hibana_repo_tests))]
            {
                assert!(known_true.words[word] & known_false.words[word] == 0);
                assert!(certain & !possible == 0);
            }
            // Source covers suffice for positive obligations on a closed
            // prefix. All negative obligations remain: extra direct order
            // could otherwise hide a missing cover behind an intermediary.
            let mut pending =
                pair_candidates(expected, possible, certain, check.excluded.words[word])
                    & (!expected | check.covers.words[word]);
            if word == before / 64 {
                pending &= u64::MAX << (before % 64);
            }
            while pending != 0 {
                let after = word * 64 + pending.trailing_zeros() as usize;
                if after >= row_count {
                    return false;
                }
                let second = check.row(after).coordinates.0;
                // Both rows must occur for their order to matter. Seed those
                // source choices before inspecting their predecessors, so an
                // already selected arm is not scanned as an unknown choice.
                if check.presence.merge_path(second, &mut selections)
                    && !check.every_selection(before, after, &mut selections)
                {
                    return false;
                }
                check
                    .presence
                    .clear_path(second, Some(first), &mut selections);
                pending &= pending - 1;
            }
            word += 1;
        }
        check.presence.clear_path(first, None, &mut selections);
        before += 1;
    }
    true
}

/// Evaluate one row of the typed source's strict order in a single descent.
/// Seq marks its entire right side; par and route only descend into the side
/// containing the first event. Roll preserves its body's single-visit order.
#[cfg(all(test, hibana_repo_tests))]
const fn mark_source_successors<const E: usize>(
    source: &ProgramSourceNode,
    start: usize,
    first: usize,
    successors: &mut [bool; E],
) {
    match source {
        ProgramSourceNode::Send(_) => (),
        ProgramSourceNode::Roll(body, _) => mark_source_successors(body, start, first, successors),
        ProgramSourceNode::Seq(left, right, _)
        | ProgramSourceNode::Par(left, right, _)
        | ProgramSourceNode::Route { left, right, .. } => {
            let split = start + left.event_count();
            if first < split {
                mark_source_successors(left, start, first, successors);
                if matches!(source, ProgramSourceNode::Seq(..)) {
                    let mut event = split;
                    while event < split + right.event_count() {
                        successors[event] = true;
                        event += 1;
                    }
                }
            } else {
                mark_source_successors(right, split, first, successors);
            }
        }
    }
}

const fn pair_candidates(expected: u64, possible: u64, certain: u64, excluded: u64) -> u64 {
    ((expected & !certain) | (!expected & possible)) & !excluded
}

const fn last_common_before(first: u64, second: u64, end: usize) -> Option<usize> {
    let prefix = if end == 64 {
        u64::MAX
    } else {
        (1u64 << end) - 1
    };
    let common = first & second & prefix;
    if common == 0 {
        None
    } else {
        Some(63 - common.leading_zeros() as usize)
    }
}

struct RowBits<const E: usize> {
    words: [u64; E],
}
impl<const E: usize> RowBits<E> {
    const fn empty() -> Self {
        Self { words: [0; E] }
    }
    const fn get(&self, row: usize) -> bool {
        self.words[row / 64] & (1 << (row % 64)) != 0
    }

    const fn clear(&mut self, words: usize) {
        let mut word = 0;
        while word < words {
            self.words[word] = 0;
            word += 1;
        }
    }
    const fn set(&mut self, row: usize, present: bool) {
        let bit = 1 << (row % 64);
        if present {
            self.words[row / 64] |= bit;
        } else {
            self.words[row / 64] &= !bit;
        }
    }
    const fn insert_range(&mut self, start: usize, end: usize) {
        let mut first = start;
        while first < end {
            let stop = if end - first > 64 - first % 64 {
                first + 64 - first % 64
            } else {
                end
            };
            let high = if stop % 64 == 0 {
                u64::MAX
            } else {
                (1 << (stop % 64)) - 1
            };
            self.words[first / 64] |= high & (u64::MAX << (first % 64));
            first = stop;
        }
    }
}

// At most 2E local rows and 256 lanes require at most 8E words,
// plus one partially used word per lane. This is compiler scratch only.
struct LaneBits<const E: usize> {
    words: [[u64; 8]; E],
    remainder: [u64; 256],
}
impl<const E: usize> LaneBits<E> {
    const fn empty() -> Self {
        Self {
            words: [[0; 8]; E],
            remainder: [0; 256],
        }
    }
    const fn get(&self, word: usize) -> u64 {
        if word < E * 8 {
            self.words[word / 8][word % 8]
        } else {
            self.remainder[word - E * 8]
        }
    }
    const fn insert(&mut self, word: usize, bit: usize) {
        if word < E * 8 {
            self.words[word / 8][word % 8] |= 1 << bit;
        } else {
            self.remainder[word - E * 8] |= 1 << bit;
        }
    }
}

struct SourceOrder<'a, const E: usize> {
    source: &'a ProgramSourceNode,
    data: &'a ProgramSourceData<E>,
    image: &'a RoleImageRef,
    // A source message contributes at most a send and a receive locally.
    // Decode the immutable image once in compiler scratch, not per edge.
    rows: [[Option<SourceRow>; 2]; E],
    presence: &'a SourcePresence<E>,
    expected: RowBits<E>,
    covers: RowBits<E>,
    excluded: RowBits<E>,
    lanes: LaneBits<E>,
    source_rows: [u16; E],
    guard_rows: [u16; E],
}

#[derive(Clone, Copy)]
struct SourceRow {
    action: LocalAction,
    dependency: Option<LocalDependency>,
    guard_scope: Option<SourceScope>,
    coordinates: (usize, u8),
    next_start: u16,
    next_end: u16,
    next_guard: u16,
    actor: u16,
}

impl<const E: usize> SourceOrder<'_, E> {
    /// Enumerate only source successors on this lane, newest first. The
    /// lane masks are also used by the unconditional word check above.
    const fn previous_expected(&self, lane: u8, mut end: usize) -> Option<usize> {
        #[cfg(all(test, hibana_repo_tests))]
        let expected = reference::previous_expected(self, lane, end);
        let words = self.image.local_step_count().div_ceil(64);
        let mut found = None;
        while end != 0 {
            let word = (end - 1) / 64;
            if let Some(bit) = last_common_before(
                self.lanes.get(lane as usize * words + word),
                self.expected.words[word],
                end - word * 64,
            ) {
                found = Some(word * 64 + bit);
                break;
            }
            end = word * 64;
        }
        #[cfg(all(test, hibana_repo_tests))]
        match (found, expected) {
            (None, None) => (),
            (Some(first), Some(second)) if first == second => (),
            _ => panic!("indexed and scalar source predecessors disagree"),
        }
        found
    }

    const fn row_start(&self, event: usize) -> usize {
        if event == self.source.event_count() {
            self.image.local_step_count()
        } else {
            self.source_rows[event] as usize
        }
    }

    /// Add every possible minimal local row. A literal actor binding is
    /// present whenever its message occurs; a decision binding may be absent.
    /// The result says whether some path may contain no local row.
    const fn mark_minima(&mut self, source: &ProgramSourceNode, start: usize) -> bool {
        match source {
            ProgramSourceNode::Send(_) => {
                let mut row = self.row_start(start);
                let stop = self.row_start(start + 1);
                while row < stop {
                    self.covers.set(row, true);
                    if self.row(row).actor == self.image.role() as u16 {
                        return false;
                    }
                    row += 1;
                }
                true
            }
            ProgramSourceNode::Roll(body, _) => self.mark_minima(body, start),
            ProgramSourceNode::Seq(left, right, _) => {
                self.mark_minima(left, start) && self.mark_minima(right, start + left.event_count())
            }
            ProgramSourceNode::Par(left, right, _) => {
                let left_empty = self.mark_minima(left, start);
                let right_empty = self.mark_minima(right, start + left.event_count());
                left_empty && right_empty
            }
            ProgramSourceNode::Route { left, right, .. } => {
                let left_empty = self.mark_minima(left, start);
                let right_empty = self.mark_minima(right, start + left.event_count());
                left_empty || right_empty
            }
        }
    }

    /// Mark full source order and a conservative set containing every cover.
    /// Return whether `before` may remain maximal in this subtree, assuming
    /// that row occurs. Conditional rows are never used to prune a cover.
    const fn mark_successors(
        &mut self,
        source: &ProgramSourceNode,
        start: usize,
        first: usize,
        before: usize,
    ) -> bool {
        match source {
            ProgramSourceNode::Send(_) => {
                let mut row = before + 1;
                let stop = self.row_start(start + 1);
                while row < stop {
                    self.covers.set(row, true);
                    if self.row(row).actor == self.image.role() as u16 {
                        return false;
                    }
                    row += 1;
                }
                true
            }
            ProgramSourceNode::Roll(body, _) => self.mark_successors(body, start, first, before),
            ProgramSourceNode::Seq(left, right, _)
            | ProgramSourceNode::Par(left, right, _)
            | ProgramSourceNode::Route { left, right, .. } => {
                let split = start + left.event_count();
                if first < split {
                    let maximal = self.mark_successors(left, start, first, before);
                    if matches!(source, ProgramSourceNode::Seq(..)) {
                        let start = self.row_start(split);
                        let stop = self.row_start(split + right.event_count());
                        self.expected.insert_range(start, stop);
                        return maximal && self.mark_minima(right, split);
                    } else if matches!(source, ProgramSourceNode::Route { .. }) {
                        self.excluded.insert_range(
                            self.row_start(split),
                            self.row_start(split + right.event_count()),
                        );
                    }
                    maximal
                } else {
                    let maximal = self.mark_successors(right, split, first, before);
                    if matches!(source, ProgramSourceNode::Route { .. }) {
                        self.excluded
                            .insert_range(self.row_start(start), self.row_start(split));
                    }
                    maximal
                }
            }
        }
    }

    const fn source_scope(&self, scope: ScopeId) -> SourceScope {
        let Some((start, split, stop)) = self.image.program().source_scope_bounds(scope) else {
            crate::invariant();
        };
        SourceScope {
            start: start as u16,
            split: split as u16,
            stop: stop as u16,
        }
    }

    const fn same_source(&self) -> bool {
        let program = self.image.program();
        let Some(actors) = self.data.actors() else {
            crate::invariant()
        };
        if program.columns.atom_count() != self.source.event_count()
            || program.actor_decision_count() != actors.decision_count()
        {
            return false;
        }
        let mut scope = 0;
        if !self.same_source_tree(self.source, 0, &mut scope)
            || scope != program.source_scope_count()
        {
            return false;
        }
        let mut index = 0;
        while index < actors.decision_count() {
            let Some((expected, refs)) = actors.decision(index) else {
                crate::invariant()
            };
            let Some((scope, actual)) = program.actor_decision_at(index) else {
                return false;
            };
            let coordinates = self.source_scope(scope);
            if refs[0] != actual[0]
                || refs[1] != actual[1]
                || expected.start != coordinates.start
                || expected.split != coordinates.split
                || expected.stop != coordinates.stop
            {
                return false;
            }
            index += 1;
        }
        true
    }

    /// Compare every typed message and scope once. Scope kind and bounds
    /// determine series/parallel order; source messages supply each row's
    /// roles, payload and actor binding independently of local projection.
    const fn same_source_tree(
        &self,
        source: &ProgramSourceNode,
        start: usize,
        next: &mut usize,
    ) -> bool {
        let kind = match source {
            ProgramSourceNode::Send(expected) => {
                let program = self.image.program();
                let actual = program.event_atom_at(start);
                if expected.from.component != 0
                    || expected.to.component != 0
                    || expected.from.role != actual.from
                    || expected.to.role != actual.to
                    || expected.label != actual.label
                {
                    return false;
                }
                match (expected.payload, actual.payload) {
                    (SourcePayload::Wire { schema }, crate::eff::EventPayload::Wire(actual))
                        if schema == actual => {}
                    (SourcePayload::Endpoint(key), crate::eff::EventPayload::Endpoint(role))
                        if key.component == 0 && key.role == role => {}
                    _ => return false,
                }
                let Some(actors) = self.data.actors() else {
                    crate::invariant()
                };
                return match (
                    actors.event_references(start),
                    program.actor_binding_at(start),
                ) {
                    (Some(expected), Some(actual)) => {
                        expected[0] == actual[0] && expected[1] == actual[1]
                    }
                    _ => false,
                };
            }
            ProgramSourceNode::Seq(left, right, _) => {
                return self.same_source_tree(left, start, next)
                    && self.same_source_tree(right, start + left.event_count(), next);
            }
            ProgramSourceNode::Par(..) => ScopeKind::Parallel,
            ProgramSourceNode::Route { .. } => ScopeKind::Route,
            ProgramSourceNode::Roll(..) => ScopeKind::Roll,
        };
        if *next >= ScopeId::LOCAL_CAPACITY as usize {
            return false;
        }
        let scope = ScopeId::new(kind, *next as u16);
        *next += 1;
        let Some((first, split, stop)) = self.image.program().source_scope_bounds(scope) else {
            return false;
        };
        if first != start || stop != start + source.event_count() {
            return false;
        }
        match source {
            ProgramSourceNode::Roll(body, _) => {
                split == u16::MAX as usize && self.same_source_tree(body, start, next)
            }
            ProgramSourceNode::Par(left, right, _)
            | ProgramSourceNode::Route { left, right, .. } => {
                split == start + left.event_count()
                    && self.same_source_tree(left, start, next)
                    && self.same_source_tree(right, split, next)
            }
            ProgramSourceNode::Send(_) | ProgramSourceNode::Seq(..) => crate::invariant(),
        }
    }

    const fn operation(&self, row: usize) -> LocalAction {
        self.row(row).action
    }

    const fn row(&self, row: usize) -> &SourceRow {
        let Some(row) = &self.rows[row / 2][row % 2] else {
            crate::invariant()
        };
        row
    }

    const fn coordinates(action: LocalAction) -> (usize, u8) {
        match action {
            LocalAction::Send {
                eff_index, lane, ..
            }
            | LocalAction::Recv {
                eff_index, lane, ..
            }
            | LocalAction::Local {
                eff_index, lane, ..
            } => (eff_index.dense_ordinal(), lane),
            #[cfg(all(test, hibana_repo_tests))]
            LocalAction::Terminate => crate::invariant(),
        }
    }

    const fn active(&self, row: usize, path: &SourceContext<'_>) -> Result<bool, SourceScope> {
        let (event, _) = self.row(row).coordinates;
        match self.presence.active(event, path) {
            Ok(true) => self.holder_active(row, path),
            other => other,
        }
    }

    const fn holder_active(
        &self,
        row: usize,
        path: &SourceContext<'_>,
    ) -> Result<bool, SourceScope> {
        let Some(actors) = self.data.actors() else {
            crate::invariant()
        };
        let mut actor = self.row(row).actor;
        while actor >= 256 {
            let Some((scope, branches)) = actors.decision(actor as usize - 256) else {
                crate::invariant();
            };
            actor = match path.get(scope) {
                Some(SourceArm::Left) => branches[0],
                Some(SourceArm::Right) => branches[1],
                None => return Err(scope),
            };
        }
        Ok(actor == self.image.role() as u16)
    }

    const fn actual(
        &self,
        before: usize,
        after: usize,
        path: &SourceContext<'_>,
    ) -> Result<bool, SourceScope> {
        let (_, left_lane) = self.row(before).coordinates;
        let (_, right_lane) = self.row(after).coordinates;
        if before < after && left_lane == right_lane {
            return Ok(true);
        }
        let Some(row) = &self.rows[after / 2][after % 2] else {
            crate::invariant()
        };
        let Some(dependency) = row.dependency else {
            return Ok(false);
        };
        if before < dependency.start() || before >= dependency.end() {
            return Ok(false);
        }
        self.dependency_active(row, path)
    }

    const fn dependency_active(
        &self,
        row: &SourceRow,
        path: &SourceContext<'_>,
    ) -> Result<bool, SourceScope> {
        let Some(dependency) = row.dependency else {
            crate::invariant()
        };
        match dependency.conflict() {
            LocalConflict::Unconditional => Ok(true),
            LocalConflict::SharedRoute => Ok(false),
            LocalConflict::RouteArm { arm, .. } => {
                let Some(scope) = row.guard_scope else {
                    crate::invariant()
                };
                match path.get(scope) {
                    Some(SourceArm::Left) => Ok(arm == 0),
                    Some(SourceArm::Right) => Ok(arm == 1),
                    None => Err(scope),
                }
            }
        }
    }

    const fn pair(
        &self,
        before: usize,
        after: usize,
        path: &SourceContext<'_>,
    ) -> Result<bool, SourceScope> {
        let expected = self.expected.get(after);
        let actual = self.actual(before, after, path);
        // Equal unconditional answers hold under every choice. Only a
        // missing or conditional edge needs path-specific activity checks.
        // A reversed local order must still be rejected when both rows occur.
        if (!expected && matches!(actual, Ok(false)))
            || (expected && before < after && matches!(actual, Ok(true)))
        {
            return Ok(true);
        }
        match self.holder_active(after, path) {
            Ok(true) => (),
            Ok(false) => return Ok(true),
            Err(scope) => return Err(scope),
        }
        match self.holder_active(before, path) {
            Ok(true) => (),
            Ok(false) => return Ok(true),
            Err(scope) => return Err(scope),
        }
        if expected && before >= after {
            return Ok(false);
        }
        match actual {
            Ok(true) => return Ok(expected),
            Ok(false) if !expected => return Ok(true),
            Ok(false) => (),
            Err(scope) => return Err(scope),
        }
        // Source-closed progress makes a direct predecessor or one of its
        // source ancestors sufficient. Check each obligation separately, so
        // independent route scopes do not form a Cartesian path enumeration.
        let mut unknown = None;
        let row = self.row(after);
        let mut end = after;
        while let Some(middle) = self.previous_expected(row.coordinates.1, end) {
            match self.active(middle, path) {
                Ok(true) => return Ok(true),
                Ok(false) => (),
                Err(scope) => {
                    if unknown.is_none() {
                        unknown = Some(scope);
                    }
                }
            }
            end = middle;
        }
        if let Some(dependency) = row.dependency {
            let edge = self.dependency_active(row, path);
            if !matches!(edge, Ok(false)) {
                let mut middle = dependency.start();
                while middle < after && middle < dependency.end() {
                    if self.expected.get(middle) {
                        match self.active(middle, path) {
                            Ok(true) => match edge {
                                Ok(true) => return Ok(true),
                                Ok(false) => (),
                                Err(scope) => {
                                    if unknown.is_none() {
                                        unknown = Some(scope);
                                    }
                                }
                            },
                            Ok(false) => (),
                            Err(scope) => {
                                if unknown.is_none() {
                                    unknown = Some(scope);
                                }
                            }
                        }
                    }
                    middle += 1;
                }
            }
        }
        match unknown {
            Some(scope) => Err(scope),
            None => Ok(false),
        }
    }

    const fn every_selection(
        &self,
        before: usize,
        after: usize,
        selections: &mut [Option<SourceSelection>; E],
    ) -> bool {
        let scope = match self.pair(before, after, &SourceContext::from_index(selections)) {
            Ok(result) => return result,
            Err(scope) => scope,
        };
        let slot = scope.split as usize;
        if slot >= E || selections[slot].is_some() {
            return false;
        }
        selections[slot] = Some(SourceSelection {
            scope,
            arm: SourceArm::Left,
        });
        let left = self.every_selection(before, after, selections);
        let right = if left {
            selections[slot] = Some(SourceSelection {
                scope,
                arm: SourceArm::Right,
            });
            self.every_selection(before, after, selections)
        } else {
            false
        };
        selections[slot] = None;
        left && right
    }
}
