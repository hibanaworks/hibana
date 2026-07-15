use super::*;
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
        let node = eff_list.node_at(eff_idx);
        if matches!(node.kind, EffKind::Atom) {
            let atom = node.atom_data();
            if (atom.from == role || atom.to == role) && atom.lane == lane {
                return true;
            }
        }
        eff_idx += 1;
    }
    false
}

const fn reference_dependency<const E: usize>(
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
        if matches!(marker.event, ScopeEvent::Enter)
            && matches!(marker.scope_id.kind(), Some(ScopeKind::Parallel))
        {
            let exit_eff = parallel_exit_for_enter(markers, marker_idx);
            let row = local_step_range_for_eff_range(eff_list, marker.offset(), exit_eff, role);
            let end = row.end();
            if row.start() < end && target >= end {
                let parent_end = nearest_parent_parallel_end(markers, marker_idx, exit_eff);
                let applies =
                    reference_lane_present(eff_list, role, marker.offset(), exit_eff, current_lane)
                        || current_eff >= parent_end;
                if applies && (dependency.is_none() || end >= dependency.end() as usize) {
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

fn assert_cursor_matches_reference<const E: usize>(eff_list: &EffList<E>, role: u8) {
    let mut cursor = DependencyCursor::new(eff_list, role);
    let mut local_step = 0usize;
    let mut eff_idx = 0usize;
    while eff_idx < eff_list.len() {
        let node = eff_list.node_at(eff_idx);
        if matches!(node.kind, EffKind::Atom) {
            let atom = node.atom_data();
            if atom.from == role || atom.to == role {
                assert_eq!(
                    cursor.next(eff_idx, atom.lane, local_step),
                    reference_dependency(eff_list, role, eff_idx, atom.lane, local_step),
                    "dependency cursor diverged at role {role} event {eff_idx} local step {local_step}",
                );
                local_step += 1;
            }
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
