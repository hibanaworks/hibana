//! Pairwise oracle for the word-batched compiler check.
use super::*;

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
    let order = image.program().source_order_index::<E>();
    let mut lane_heads = [u16::MAX; u8::MAX as usize + 1];
    let mut row = 0;
    while row < row_count {
        let Some(node) = image.local_step_node(row) else {
            crate::invariant()
        };
        let coordinates = SourceOrder::<E>::coordinates(node.action());
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
        check.rows[row / 2][row % 2] = Some(SourceRow {
            action: node.action(),
            dependency,
            guard_scope,
            coordinates,
            actor,
            previous_lane: lane_heads[coordinates.1 as usize],
            next_start: u16::MAX,
            next_end: u16::MAX,
        });
        lane_heads[coordinates.1 as usize] = row as u16;
        row += 1;
    }
    let mut selections = [None; E];
    let mut before = 0;
    while before < row_count {
        let left = check.operation(before);
        let (first, _) = check.row(before).coordinates;
        let mut source_successors = [false; E];
        mark_source_successors(check.source, 0, first, &mut source_successors);
        let mut after = before;
        while after < row_count {
            let right = check.operation(after);
            let (second, _) = check.row(after).coordinates;
            let precedes = source_successors[second];
            if order.before(first, second) != precedes {
                return false;
            }
            check.expected.set(
                after,
                if first == second {
                    matches!(left, LocalAction::Send { .. })
                        && matches!(right, LocalAction::Recv { .. })
                } else {
                    precedes
                },
            );
            after += 1;
        }
        let mut after = before;
        while after < row_count {
            let (second, _) = check.row(after).coordinates;
            if !matches!(
                check.pair(before, after, &SourceContext::new(&[])),
                Ok(true)
            ) && check.presence.merge_path(first, &mut selections)
                && check.presence.merge_path(second, &mut selections)
            {
                // Inactive pairs impose no obligation. Check precisely the
                // common paths on which both source events can occur.
                if !check.every_selection(before, after, &mut selections) {
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
