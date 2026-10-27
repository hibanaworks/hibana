//! Const translation validation for the source and the emitted local guards.
//! This reads immutable projection data; it adds no runtime progress state.
use super::{ProgramProjection, ProgramShape};
use crate::{
    g::{
        ProgramSourceData, ProgramSourceNode, SourceArm, SourceContext, SourcePayload, SourceScope,
        SourceSelection,
    },
    global::{
        const_dsl::{ScopeId, route_arm_ranges_from_first_enter},
        role_program::RoleImageRef,
        typestate::{LocalAction, LocalConflict, LocalDependency},
    },
};

mod presence;
use presence::SourcePresence;

impl<Steps: ProgramShape, const E: usize> ProgramProjection<Steps, E> {
    const SOURCE_PRESENCE: SourcePresence<E> = SourcePresence::new(&Steps::SOURCE_NODE);
}

pub(super) const fn check<Steps: ProgramShape, const E: usize>(image: &RoleImageRef) -> bool {
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
        expected: [[false; 2]; E],
    };
    if !check.same_source() {
        return false;
    }
    if image.local_step_count() > E * 2 {
        return false;
    }
    let mut lane_heads = [u16::MAX; u8::MAX as usize + 1];
    let mut row = 0;
    while row < image.local_step_count() {
        let Some(node) = image.local_step_node(row) else {
            crate::invariant()
        };
        let coordinates = SourceOrder::<E>::coordinates(node.action());
        check.rows[row / 2][row % 2] = Some(SourceRow {
            action: node.action(),
            dependency: image.dependency_for_index(row),
            coordinates,
            previous_lane: lane_heads[coordinates.1 as usize],
        });
        lane_heads[coordinates.1 as usize] = row as u16;
        row += 1;
    }
    let mut selections = [None; E];
    let mut before = 0;
    while before < image.local_step_count() {
        let left = check.operation(before);
        let (first, _) = check.row(before).coordinates;
        let mut source_successors = [false; E];
        mark_source_successors(check.source, 0, first, &mut source_successors);
        let mut after = 0;
        while after < image.local_step_count() {
            let right = check.operation(after);
            let (second, _) = check.row(after).coordinates;
            let precedes = source_successors[second];
            if image.program().source_precedes(first, second) != precedes {
                return false;
            }
            check.expected[after / 2][after % 2] = if first == second {
                matches!(left, LocalAction::Send { .. })
                    && matches!(right, LocalAction::Recv { .. })
            } else {
                precedes
            };
            after += 1;
        }
        let mut after = 0;
        while after < image.local_step_count() {
            let (second, _) = check.row(after).coordinates;
            if check.presence.merge_path(first, &mut selections)
                && check.presence.merge_path(second, &mut selections)
            {
                // Inactive pairs impose no obligation. Check precisely the
                // common paths on which both source events can occur.
                if !check.every_selection(before, after, &SourceContext::from_index(&selections)) {
                    return false;
                }
            }
            check.presence.clear_path(first, &mut selections);
            check.presence.clear_path(second, &mut selections);
            after += 1;
        }
        before += 1;
    }
    true
}

/// Evaluate one row of the typed source's strict order in a single descent.
/// Seq marks its entire right side; par and route only descend into the side
/// containing the first event. Roll preserves its body's single-visit order.
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

struct SourceOrder<'a, const E: usize> {
    source: &'a ProgramSourceNode,
    data: &'a ProgramSourceData<E>,
    image: &'a RoleImageRef,
    // A source message contributes at most a send and a receive locally.
    // Decode the immutable image once in compiler scratch, not per edge.
    rows: [[Option<SourceRow>; 2]; E],
    presence: &'a SourcePresence<E>,
    expected: [[bool; 2]; E],
}

#[derive(Clone, Copy)]
struct SourceRow {
    action: LocalAction,
    dependency: Option<LocalDependency>,
    coordinates: (usize, u8),
    previous_lane: u16,
}

impl<const E: usize> SourceOrder<'_, E> {
    const fn source_scope(&self, scope: ScopeId) -> SourceScope {
        let markers = self.data.eff_list().scope_markers();
        let Some(enter) = markers.first_enter_index(scope) else {
            crate::invariant();
        };
        let [(start, split), (right, stop)] = route_arm_ranges_from_first_enter(markers, enter);
        if split != right {
            crate::invariant();
        }
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
            #[cfg(any(kani, all(test, hibana_repo_tests)))]
            LocalAction::Terminate => crate::invariant(),
        }
    }

    const fn active(&self, row: usize, path: &SourceContext<'_>) -> Result<bool, SourceScope> {
        let operation = self.operation(row);
        let (event, _) = self.row(row).coordinates;
        match self.presence.active(event, path) {
            Ok(true) => (),
            other => return other,
        }
        let Some(actors) = self.data.actors() else {
            crate::invariant()
        };
        let Some(references) = actors.event_references(event) else {
            crate::invariant()
        };
        let mut actor = if matches!(operation, LocalAction::Recv { .. }) {
            references[1]
        } else {
            references[0]
        };
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
        self.dependency_active(dependency, path)
    }

    const fn dependency_active(
        &self,
        dependency: LocalDependency,
        path: &SourceContext<'_>,
    ) -> Result<bool, SourceScope> {
        match dependency.conflict() {
            LocalConflict::Unconditional => Ok(true),
            LocalConflict::SharedRoute => Ok(false),
            LocalConflict::RouteArm { scope, arm } => {
                let scope = self.source_scope(scope);
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
        let expected = self.expected[after / 2][after % 2];
        let actual = self.actual(before, after, path);
        // Equal unconditional answers hold under every choice. Only a
        // missing or conditional edge needs path-specific activity checks.
        // A reversed local order must still be rejected when both rows occur.
        if (!expected && matches!(actual, Ok(false)))
            || (expected && before < after && matches!(actual, Ok(true)))
        {
            return Ok(true);
        }
        match self.active(after, path) {
            Ok(true) => (),
            Ok(false) => return Ok(true),
            Err(scope) => return Err(scope),
        }
        match self.active(before, path) {
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
            if self.expected[middle / 2][middle % 2] {
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
            let edge = self.dependency_active(dependency, path);
            if !matches!(edge, Ok(false)) {
                let mut middle = dependency.start();
                while middle < after && middle < dependency.end() {
                    if self.expected[middle / 2][middle % 2] {
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

    const fn every_selection(&self, before: usize, after: usize, path: &SourceContext<'_>) -> bool {
        let scope = match self.pair(before, after, path) {
            Ok(result) => return result,
            Err(scope) => scope,
        };
        self.every_selection(
            before,
            after,
            &path.select(SourceSelection {
                scope,
                arm: SourceArm::Left,
            }),
        ) && self.every_selection(
            before,
            after,
            &path.select(SourceSelection {
                scope,
                arm: SourceArm::Right,
            }),
        )
    }
}
