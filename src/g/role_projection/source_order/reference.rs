//! Pairwise oracle for the word-batched compiler check.
use super::*;

// Enumerate small typed sources independently of the cached presence paths and
// cover traversal. The pairwise oracle below remains enabled at every size.
pub(super) const fn assert_cover_candidates<const E: usize>(
    check: &SourceOrder<'_, E>,
    before: usize,
) {
    if check.source.event_count() > 12 {
        return;
    }
    let mut choices = [SourceSelection {
        scope: SourceScope {
            start: 0,
            split: 0,
            stop: 0,
        },
        arm: SourceArm::Left,
    }; E];
    let count = collect_choices(check.source, 0, &mut choices, 0);
    if count > 4 {
        return;
    }
    let mut assignment = 0;
    while assignment < 1 << count {
        let mut choice = 0;
        while choice < count {
            choices[choice].arm = if assignment & (1 << choice) == 0 {
                SourceArm::Left
            } else {
                SourceArm::Right
            };
            choice += 1;
        }
        let path = choices.split_at(count).0;
        if row_occurs(check, before, path) {
            let mut after = before + 1;
            while after < check.image.local_step_count() {
                let ordered = source_order(check, before, after);
                assert!(!check.covers.get(after) || ordered);
                if ordered && row_occurs(check, after, path) {
                    let mut middle = before + 1;
                    let mut cover = true;
                    while middle < after {
                        if source_order(check, before, middle)
                            && source_order(check, middle, after)
                            && row_occurs(check, middle, path)
                        {
                            cover = false;
                            break;
                        }
                        middle += 1;
                    }
                    assert!(!cover || check.covers.get(after));
                }
                after += 1;
            }
        }
        assignment += 1;
    }
}

const fn source_order<const E: usize>(check: &SourceOrder<'_, E>, a: usize, b: usize) -> bool {
    let first = check.row(a).coordinates.0;
    let second = check.row(b).coordinates.0;
    if first == second {
        matches!(check.operation(a), LocalAction::Send { .. })
            && matches!(check.operation(b), LocalAction::Recv { .. })
    } else {
        check.source.before(first, second)
    }
}

const fn row_occurs<const E: usize>(
    check: &SourceOrder<'_, E>,
    row: usize,
    path: &[SourceSelection],
) -> bool {
    let event = check.row(row).coordinates.0;
    if !event_occurs(check.source, 0, event, &SourceContext::new(path)) {
        return false;
    }
    let Some(actors) = check.data.actors() else {
        crate::invariant()
    };
    let Some((sender, receiver)) = actors.actors_at(event, path) else {
        crate::invariant()
    };
    let actor = if matches!(check.operation(row), LocalAction::Recv { .. }) {
        receiver
    } else {
        sender
    };
    actor == check.image.role()
}

const fn event_occurs(
    source: &ProgramSourceNode,
    start: usize,
    event: usize,
    path: &SourceContext<'_>,
) -> bool {
    match source {
        ProgramSourceNode::Send(_) => event == start,
        ProgramSourceNode::Roll(body, _) => event_occurs(body, start, event, path),
        ProgramSourceNode::Seq(left, right, _)
        | ProgramSourceNode::Par(left, right, _)
        | ProgramSourceNode::Route { left, right, .. } => {
            let split = start + left.event_count();
            if matches!(source, ProgramSourceNode::Route { .. }) {
                let scope = SourceScope {
                    start: start as u16,
                    split: split as u16,
                    stop: (split + right.event_count()) as u16,
                };
                match path.get(scope) {
                    Some(SourceArm::Left) if event >= split => return false,
                    Some(SourceArm::Right) if event < split => return false,
                    Some(_) => (),
                    None => crate::invariant(),
                }
            }
            if event < split {
                event_occurs(left, start, event, path)
            } else {
                event_occurs(right, split, event, path)
            }
        }
    }
}

const fn collect_choices<const E: usize>(
    source: &ProgramSourceNode,
    start: usize,
    choices: &mut [SourceSelection; E],
    mut count: usize,
) -> usize {
    match source {
        ProgramSourceNode::Send(_) => count,
        ProgramSourceNode::Roll(body, _) => collect_choices(body, start, choices, count),
        ProgramSourceNode::Seq(left, right, _)
        | ProgramSourceNode::Par(left, right, _)
        | ProgramSourceNode::Route { left, right, .. } => {
            let split = start + left.event_count();
            if matches!(source, ProgramSourceNode::Route { .. }) {
                choices[count].scope = SourceScope {
                    start: start as u16,
                    split: split as u16,
                    stop: (split + right.event_count()) as u16,
                };
                count += 1;
            }
            count = collect_choices(left, start, choices, count);
            collect_choices(right, split, choices, count)
        }
    }
}

pub(super) const fn previous_expected<const E: usize>(
    check: &SourceOrder<'_, E>,
    lane: u8,
    mut end: usize,
) -> Option<usize> {
    while end != 0 {
        end -= 1;
        if check.row(end).coordinates.1 == lane && check.expected.get(end) {
            return Some(end);
        }
    }
    None
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
    let order = image.program().source_order_index::<E>();
    let words = row_count.div_ceil(64);
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
        check
            .lanes
            .insert(coordinates.1 as usize * words + row / 64, row % 64);
        check.rows[row / 2][row % 2] = Some(SourceRow {
            action: node.action(),
            dependency,
            guard_scope,
            coordinates,
            actor,
            next_start: u16::MAX,
            next_end: u16::MAX,
            next_guard: u16::MAX,
        });
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
            check.presence.clear_path(first, None, &mut selections);
            check.presence.clear_path(second, None, &mut selections);
            after += 1;
        }
        before += 1;
    }
    true
}
