use super::super::{dependency_conflict_for_scope, parallel_exit_for_enter};
use super::*;
use crate::global::const_dsl::parallel_arm_ranges_from_enter;
use crate::global::typestate::LocalConflict;
use crate::{
    g::{Msg, Par, ProgramSourceData, Roll, Route, Send, Seq},
    global::const_dsl::EffList,
};

type OtherArm = Seq<Send<0, 1, Msg<237, u8>>, Send<1, 0, Msg<238, u8>>>;
type ParallelArm = Roll<Par<Send<0, 1, Msg<235, u8>>, Send<0, 1, Msg<236, u8>>>>;
type ReentrantRoute = Roll<Route<OtherArm, ParallelArm>>;

#[test]
fn source_marker_slots_keep_sparse_scope_kinds_distinct() {
    use crate::eff::{EffAtom, EventOrigin, EventPayload};
    use crate::global::const_dsl::{ReentryMark, ScopeId};
    let last = ScopeId::LOCAL_CAPACITY - 1;
    let mut checked = 0;
    for (route_id, parallel_id) in [(0, 1), (last, last)] {
        let mut source = EffList::<12>::new_partitioned(5, 7, 0);
        for (label, lane) in [0, 1, 2, 1, 0].into_iter().enumerate() {
            source.push_event_mut(EffAtom {
                from: 0,
                to: 1,
                label: label as u8,
                lane,
                payload: EventPayload::Wire(1),
                origin: EventOrigin::User,
            });
        }
        let route = ScopeId::route(route_id);
        source.push_route_scope_mut(route, 1, 3, 4, ReentryMark::SinglePass);
        source.push_parallel_scope_mut(ScopeId::parallel(parallel_id), 1, 2, 3);
        let scopes = ScopeFacts::new(&source);
        for role in [0, 1] {
            let role_rows = crate::global::role_program::RoleLocalRows::new(&source, role);
            let mut cursor = DependencyCursor::new(&source, &scopes, LocalRows::Role(&role_rows));
            let dependencies: [_; 5] =
                core::array::from_fn(|event| cursor.next(event, source.atom_at(event).lane, event));
            // Source: prefix; choice { par(left, right), other }; continuation.
            // Compare allowed operations at every legal cut. Guard encodings
            // may differ while imposing the same order on the selected branch.
            for selected in 0..2 {
                let active = [true, selected == 0, selected == 0, selected == 1, true];
                for mask in 0u8..32 {
                    let done = |event: usize| mask & (1u8 << event) != 0;
                    let body_done = if selected == 0 {
                        done(1) && done(2)
                    } else {
                        done(3)
                    };
                    let started =
                        (selected == 0 && (done(1) || done(2))) || (selected == 1 && done(3));
                    if (started && !done(0)) || (done(4) && !body_done) {
                        continue;
                    }
                    for event in 0..5 {
                        if !active[event] || done(event) {
                            continue;
                        }
                        let expected = match event {
                            0 => true,
                            1..=3 => done(0),
                            4 => body_done,
                            _ => unreachable!(),
                        };
                        let actual =
                            dependencies[event]
                                .to_dependency(5)
                                .is_none_or(|dependency| {
                                    assert!(
                                        dependency.start() <= dependency.end()
                                            && dependency.end() <= 5
                                    );
                                    let applies = match dependency.conflict() {
                                        LocalConflict::Unconditional => true,
                                        LocalConflict::SharedRoute => false,
                                        LocalConflict::RouteArm { scope, arm } => {
                                            assert_eq!(scope, route);
                                            arm == selected
                                        }
                                    };
                                    !applies
                                        || (dependency.start()..dependency.end())
                                            .all(|prior| !active[prior] || done(prior))
                                });
                        assert_eq!(
                            actual, expected,
                            "route={route_id}, par={parallel_id}, role={role}, choice={selected}, mask={mask}, event={event}"
                        );
                        checked += 1;
                    }
                }
            }
        }
    }
    assert_eq!(checked, 192);
}

const fn reference_lane_present<const E: usize>(
    eff_list: &EffList<E>,
    role: u8,
    start_eff: usize,
    end_eff: usize,
    lane: u8,
) -> bool {
    let mut eff_idx = start_eff;
    while eff_idx < end_eff {
        let atom = eff_list.atom_at(eff_idx);
        if (atom.from == role || atom.to == role) && atom.lane == lane {
            return true;
        }
        eff_idx += 1;
    }
    false
}

const fn separates_parallel_siblings<const E: usize>(
    source: &EffList<E>,
    join_index: usize,
    current_eff: usize,
) -> bool {
    let markers = source.scope_markers();
    let join = markers.at(join_index);
    let stop = parallel_exit_for_enter(markers, join_index);
    let mut index = 0;
    while index < markers.len() {
        let marker = markers.at(index);
        if marker.event.is_primary_enter()
            && matches!(marker.scope_id.kind(), Some(ScopeKind::Parallel))
        {
            let Some((start, split, _, end)) = parallel_arm_ranges_from_enter(markers, index)
            else {
                crate::invariant()
            };
            if start <= join.offset() && stop <= split && split <= current_eff && current_eff < end
            {
                return true;
            }
        }
        index += 1;
    }
    false
}

fn reference_dependency<const E: usize>(
    eff_list: &EffList<E>,
    role: u8,
    current_eff: usize,
    current_lane: u8,
    target: usize,
) -> PackedLocalDependency {
    let markers = eff_list.scope_markers();
    let has_route = scope_markers_contain_kind(markers, ScopeKind::Route);
    let mut dependency = PackedLocalDependency::none();
    let mut marker_idx = 0usize;
    while marker_idx < markers.len() {
        let marker = markers.at(marker_idx);
        if marker.event.is_primary_enter()
            && matches!(marker.scope_id.kind(), Some(ScopeKind::Parallel))
        {
            let exit_eff = parallel_exit_for_enter(markers, marker_idx);
            if marker.offset() <= current_eff && current_eff < exit_eff {
                let mut floor = 0;
                for parent_index in 0..marker_idx {
                    let parent = markers.at(parent_index);
                    if parent.event.is_primary_enter()
                        && matches!(parent.scope_id.kind(), Some(ScopeKind::Parallel))
                    {
                        let (start, split, _, end) =
                            parallel_arm_ranges_from_enter(markers, parent_index).unwrap();
                        let arm_start = if start <= marker.offset() && exit_eff <= split {
                            Some(start)
                        } else if split <= marker.offset() && exit_eff <= end {
                            Some(split)
                        } else {
                            None
                        };
                        if let Some(start) = arm_start {
                            floor = floor.max(start);
                        }
                    }
                }
                let input = local_step_range_for_eff_range(
                    eff_list,
                    floor,
                    marker.offset(),
                    LocalRows::Role(&crate::global::role_program::RoleLocalRows::new(
                        eff_list, role,
                    )),
                );
                if !input.is_absent_or_zero_len()
                    && (dependency.is_none() || input.end() > dependency.end() as usize)
                {
                    dependency = PackedLocalDependency::from_dependency(
                        LocalDependency::with_conflict_range(
                            marker.scope_id,
                            dependency_conflict_for_scope(markers, eff_list.len(), marker.scope_id),
                            input.start(),
                            input.end(),
                        ),
                    );
                }
            }
            let row = local_step_range_for_eff_range(
                eff_list,
                marker.offset(),
                exit_eff,
                LocalRows::Role(&crate::global::role_program::RoleLocalRows::new(
                    eff_list, role,
                )),
            );
            let end = row.end();
            if row.start() < end && target >= end {
                let independent = separates_parallel_siblings(eff_list, marker_idx, current_eff);
                let applies =
                    reference_lane_present(eff_list, role, marker.offset(), exit_eff, current_lane)
                        || !independent;
                if applies && (dependency.is_none() || end > dependency.end() as usize) {
                    let conflict = if has_route {
                        dependency_conflict_for_scope(markers, eff_list.len(), marker.scope_id)
                    } else {
                        LocalConflict::Unconditional
                    };
                    dependency = PackedLocalDependency::from_dependency(
                        LocalDependency::with_conflict_range(
                            marker.scope_id,
                            conflict,
                            row.start(),
                            end,
                        ),
                    );
                }
            }
        }
        marker_idx += 1;
    }
    dependency
}

#[test]
fn sequential_prefix_is_retained_at_nested_forks_without_sibling_edges() {
    type Pair = Par<Send<0, 1, Msg<11, u32>>, Send<0, 1, Msg<20, u32>>>;
    type Body = Seq<Send<0, 1, Msg<10, u32>>, Par<Seq<Send<0, 1, Msg<12, u32>>, Pair>, Pair>>;
    let source = ProgramSourceData::<64>::lower::<Body>();
    for role in 0..=1 {
        assert_cursor_matches_reference(source.eff_list(), role);
    }
}

fn assert_cursor_matches_reference<const E: usize>(eff_list: &EffList<E>, role: u8) {
    let scopes = ScopeFacts::new(eff_list);
    let role_rows = crate::global::role_program::RoleLocalRows::new(eff_list, role);
    let mut cursor = DependencyCursor::new(eff_list, &scopes, LocalRows::Role(&role_rows));
    let mut local_step = 0usize;
    let mut eff_idx = 0usize;
    while eff_idx < eff_list.len() {
        let atom = eff_list.atom_at(eff_idx);
        if atom.from == role || atom.to == role {
            assert_eq!(
                cursor.next(eff_idx, atom.lane, local_step),
                reference_dependency(eff_list, role, eff_idx, atom.lane, local_step),
                "dependency cursor diverged at role {role} event {eff_idx} local step {local_step}",
            );
            local_step += 1;
        }
        eff_idx += 1;
    }
}

#[test]
fn reentrant_route_parallel_dependencies_preserve_source_order_in_each_alternative() {
    use crate::g::ProgramShape;
    let source = ProgramSourceData::<32>::lower::<ReentrantRoute>();
    let effects = source.eff_list();
    let markers = effects.scope_markers();
    let enter = (0..markers.len())
        .find(|&index| {
            markers.at(index).event.is_primary_enter()
                && matches!(markers.at(index).scope_id.kind(), Some(ScopeKind::Route))
        })
        .unwrap();
    let scope = markers.at(enter).scope_id;
    let [(start, split), (_, end)] = route_arm_ranges_from_first_enter(markers, enter);
    for role in 0..=1 {
        let scopes = ScopeFacts::new(effects);
        let rows: std::vec::Vec<_> = (0..effects.len())
            .filter(|&event| {
                let atom = effects.atom_at(event);
                atom.from == role || atom.to == role
            })
            .collect();
        let role_rows = crate::global::role_program::RoleLocalRows::new(effects, role);
        let mut cursor = DependencyCursor::new(effects, &scopes, LocalRows::Role(&role_rows));
        for (target, &event) in rows.iter().enumerate() {
            let lane = effects.atom_at(event).lane;
            let actual = cursor.next(event, lane, target).to_dependency(rows.len());
            let reference =
                reference_dependency(effects, role, event, lane, target).to_dependency(rows.len());
            for (arm, (low, high)) in [(0, (start, split)), (1, (split, end))] {
                if !(low..high).contains(&event) {
                    continue;
                }
                let predecessors = |dependency: Option<LocalDependency>| {
                    rows[..target]
                        .iter()
                        .enumerate()
                        .filter_map(|(row, &before)| {
                            if !(low..high).contains(&before) {
                                return None;
                            }
                            let depends = dependency.is_some_and(|dependency| {
                                let enabled = match dependency.conflict() {
                                    LocalConflict::Unconditional => true,
                                    LocalConflict::SharedRoute => false,
                                    LocalConflict::RouteArm {
                                        scope: selected,
                                        arm: chosen,
                                    } => {
                                        assert_eq!(selected, scope);
                                        chosen == arm
                                    }
                                };
                                enabled && dependency.start() <= row && row < dependency.end()
                            });
                            (effects.atom_at(before).lane == lane || depends).then_some(before)
                        })
                        .collect::<std::vec::Vec<_>>()
                };
                let expected: std::vec::Vec<_> = rows[..target]
                    .iter()
                    .copied()
                    .filter(|&before| {
                        (low..high).contains(&before)
                            && ReentrantRoute::SOURCE_NODE.before(before, event)
                    })
                    .collect();
                // An inactive branch's rows may be omitted or retained behind
                // a guard. Compare their selected predecessors, not encodings.
                assert_eq!(predecessors(actual), expected);
                assert_eq!(predecessors(reference), expected);
            }
        }
    }
}

#[test]
fn joins_at_parallel_splits_do_not_contaminate_new_sibling_inputs() {
    type Pair = Par<Send<0, 1, Msg<1, ()>>, Send<0, 2, Msg<2, ()>>>;
    type Tree = Par<Par<Pair, Pair>, Par<Pair, Pair>>;
    let source = ProgramSourceData::<64>::lower::<Tree>();
    for role in 0..=2 {
        assert_cursor_matches_reference(source.eff_list(), role);
    }
}
