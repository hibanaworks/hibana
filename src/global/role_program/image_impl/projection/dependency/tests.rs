use super::super::dependency_conflict_for_scope;
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
                let input = local_step_range_for_eff_range(eff_list, floor, marker.offset(), role);
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
            let row = local_step_range_for_eff_range(eff_list, marker.offset(), exit_eff, role);
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
    let mut cursor = DependencyCursor::new(eff_list, &scopes, role);
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
fn reentrant_route_parallel_dependencies_match_the_direct_definition() {
    let source = ProgramSourceData::<32>::lower::<ReentrantRoute>();
    assert_cursor_matches_reference(source.eff_list(), 0);
    assert_cursor_matches_reference(source.eff_list(), 1);
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
