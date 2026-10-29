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
        excluded: RowBits::empty(),
        source_rows: [0; E],
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
    let mut lane_bits = LaneBits::<E>::empty();
    let words = row_count.div_ceil(64);
    let mut lane_heads = [u16::MAX; u8::MAX as usize + 1];
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
        let lane_word = coordinates.1 as usize * words + row / 64;
        lane_bits.insert(lane_word, row % 64);
        check.rows[row / 2][row % 2] = Some(SourceRow {
            action: node.action(),
            dependency,
            guard_scope,
            coordinates,
            actor,
            previous_lane: lane_heads[coordinates.1 as usize],
            next_start,
            next_end,
        });
        lane_heads[coordinates.1 as usize] = row as u16;
        row += 1;
    }
    while source < check.source.event_count() {
        check.source_rows[source] = row_count as u16;
        source += 1;
    }
    let mut possible = RowBits::<E>::empty();
    let mut certain = RowBits::<E>::empty();
    let mut selections = [None; E];
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
            possible.set(target as usize, true);
            if matches!(row.dependency, Some(edge) if matches!(edge.conflict(), LocalConflict::Unconditional))
            {
                certain.set(target as usize, true);
            }
            target = row.next_start;
        }
        check.expected.clear(words);
        check.excluded.clear(words);
        let (first, lane) = check.row(before).coordinates;
        check.mark_successors(check.source, 0, first);
        if matches!(check.operation(before), LocalAction::Send { .. })
            && before + 1 < row_count
            && check.row(before + 1).coordinates.0 == first
            && matches!(check.operation(before + 1), LocalAction::Recv { .. })
        {
            check.expected.set(before + 1, true);
        }
        let mut word = before / 64;
        while word < words {
            let lane_word = lane as usize * words + word;
            let same_lane = lane_bits.get(lane_word)
                & if word != before / 64 {
                    u64::MAX
                } else if before % 64 == 63 {
                    0
                } else {
                    u64::MAX << (before % 64 + 1)
                };
            let expected = check.expected.words[word];
            let possible = same_lane | possible.words[word];
            let certain = same_lane | certain.words[word];
            // Discharge unconditional equalities together. Every conditional
            // or missing edge still uses the choice/holder-sensitive check.
            let mut pending =
                pair_candidates(expected, possible, certain, check.excluded.words[word]);
            if word == before / 64 {
                pending &= u64::MAX << (before % 64);
            }
            while pending != 0 {
                let after = word * 64 + pending.trailing_zeros() as usize;
                if after >= row_count {
                    return false;
                }
                let second = check.row(after).coordinates.0;
                if !matches!(
                    check.pair(before, after, &SourceContext::new(&[])),
                    Ok(true)
                ) && check.presence.merge_path(first, &mut selections)
                    && check.presence.merge_path(second, &mut selections)
                    && !check.every_selection(before, after, &mut selections)
                {
                    return false;
                }
                check.presence.clear_path(first, &mut selections);
                check.presence.clear_path(second, &mut selections);
                pending &= pending - 1;
            }
            word += 1;
        }
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
    excluded: RowBits<E>,
    source_rows: [u16; E],
}

#[derive(Clone, Copy)]
struct SourceRow {
    action: LocalAction,
    dependency: Option<LocalDependency>,
    guard_scope: Option<SourceScope>,
    coordinates: (usize, u8),
    previous_lane: u16,
    next_start: u16,
    next_end: u16,
    actor: u16,
}

impl<const E: usize> SourceOrder<'_, E> {
    const fn row_start(&self, event: usize) -> usize {
        if event == self.source.event_count() {
            self.image.local_step_count()
        } else {
            self.source_rows[event] as usize
        }
    }

    const fn mark_successors(&mut self, source: &ProgramSourceNode, start: usize, first: usize) {
        match source {
            ProgramSourceNode::Send(_) => (),
            ProgramSourceNode::Roll(body, _) => self.mark_successors(body, start, first),
            ProgramSourceNode::Seq(left, right, _)
            | ProgramSourceNode::Par(left, right, _)
            | ProgramSourceNode::Route { left, right, .. } => {
                let split = start + left.event_count();
                if first < split {
                    self.mark_successors(left, start, first);
                    if matches!(source, ProgramSourceNode::Seq(..)) {
                        let start = self.row_start(split);
                        let stop = self.row_start(split + right.event_count());
                        self.expected.insert_range(start, stop);
                    } else if matches!(source, ProgramSourceNode::Route { .. }) {
                        self.excluded.insert_range(
                            self.row_start(split),
                            self.row_start(split + right.event_count()),
                        );
                    }
                } else {
                    self.mark_successors(right, split, first);
                    if matches!(source, ProgramSourceNode::Route { .. }) {
                        self.excluded
                            .insert_range(self.row_start(start), self.row_start(split));
                    }
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
        let mut scope = 0;
        if !self.same_scopes(self.source, 0, &mut scope) || scope != program.source_scope_count() {
            return false;
        }
        let Some(actors) = self.data.actors() else {
            crate::invariant()
        };
        if program.columns.atom_count() != self.source.event_count()
            || program.actor_decision_count() != actors.decision_count()
        {
            return false;
        }
        let mut event = 0;
        while event < self.source.event_count() {
            let expected = self.source.message_at(event);
            let actual = program.event_atom_at(event);
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
            match (
                actors.event_references(event),
                program.actor_binding_at(event),
            ) {
                (Some(expected), Some(actual))
                    if expected[0] == actual[0] && expected[1] == actual[1] => {}
                _ => return false,
            }
            event += 1;
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

    /// Scope kind and bounds completely determine the series/parallel order.
    /// Compare each typed node once; the program constructor already checks
    /// its event heads and parent links against these immutable intervals.
    const fn same_scopes(
        &self,
        source: &ProgramSourceNode,
        start: usize,
        next: &mut usize,
    ) -> bool {
        let kind = match source {
            ProgramSourceNode::Send(_) => return true,
            ProgramSourceNode::Seq(left, right, _) => {
                return self.same_scopes(left, start, next)
                    && self.same_scopes(right, start + left.event_count(), next);
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
                split == u16::MAX as usize && self.same_scopes(body, start, next)
            }
            ProgramSourceNode::Par(left, right, _)
            | ProgramSourceNode::Route { left, right, .. } => {
                split == start + left.event_count()
                    && self.same_scopes(left, start, next)
                    && self.same_scopes(right, split, next)
            }
            ProgramSourceNode::Send(_) | ProgramSourceNode::Seq(..) => crate::invariant(),
        }
    }

    const fn operation(&self, row: usize) -> LocalAction {
        self.row(row).action
    }

    const fn row(&self, row: usize) -> SourceRow {
        let Some(row) = self.rows[row / 2][row % 2] else {
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
        let Some(row) = self.rows[after / 2][after % 2] else {
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
        row: SourceRow,
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
        let mut previous = row.previous_lane;
        while previous != u16::MAX {
            let middle = previous as usize;
            if self.expected.get(middle) {
                match self.active(middle, path) {
                    Ok(true) => return Ok(true),
                    Ok(false) => (),
                    Err(scope) => {
                        if unknown.is_none() {
                            unknown = Some(scope);
                        }
                    }
                }
            }
            previous = self.row(middle).previous_lane;
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
