use super::{LocalRows, ScopeFacts, passive_route_child_enter};
use crate::global::{
    const_dsl::{EffList, ScopeId, ScopeKind},
    typestate::LocalConflict,
};

// Compiler workspace shared by counting and serialization. Global route IDs
// remain unchanged after unused local rows have been removed.
pub(in crate::global::role_program::image_impl) const fn finish_route_retention<const E: usize>(
    source: (&EffList<E>, &ScopeFacts<E>),
    rows: LocalRows<'_, E>,
    mut retained: [bool; ScopeId::LOCAL_CAPACITY as usize],
) -> [bool; ScopeId::LOCAL_CAPACITY as usize] {
    let (effects, scopes) = source;
    let markers = effects.scope_markers();
    let mut marker_idx = 0usize;
    while marker_idx < markers.len() {
        let marker = markers.at(marker_idx);
        if markers.is_first_enter(marker_idx)
            && matches!(marker.scope_id.kind(), Some(ScopeKind::Route))
        {
            if rows.retains_context(marker.scope_id) {
                retained[marker.scope_id.local_ordinal() as usize] = true;
            }
            let Some(arms) = crate::global::const_dsl::closed_route_arm_ranges_from_first_enter(
                markers, marker_idx,
            ) else {
                panic!("route scope missing binary arm ranges");
            };
            let mut arm = 0usize;
            while arm < 2 {
                let (start, end) = arms[arm];
                if !rows.range(effects, start, end).is_absent_or_zero_len() {
                    retained[marker.scope_id.local_ordinal() as usize] = true;
                }
                arm += 1;
            }
        }
        marker_idx += 1;
    }
    // Typed source is parent-before-child: reverse ancestors then forward
    // passive children close it in one sweep. Repeat monotonically for accepted
    // hand-built sources with other ordering, without imposing a new rule.
    let mut remaining = scopes.route_count() + 1;
    loop {
        let mut changed = false;
        marker_idx = markers.len();
        while marker_idx != 0 {
            marker_idx -= 1;
            let marker = markers.at(marker_idx);
            if markers.is_first_enter(marker_idx)
                && matches!(marker.scope_id.kind(), Some(ScopeKind::Route))
                && retained[marker.scope_id.local_ordinal() as usize]
                && let LocalConflict::RouteArm { scope, .. } = scopes.conflict(marker.scope_id)
                && !retained[scope.local_ordinal() as usize]
            {
                retained[scope.local_ordinal() as usize] = true;
                changed = true;
            }
        }
        marker_idx = 0;
        while marker_idx < markers.len() {
            let marker = markers.at(marker_idx);
            if markers.is_first_enter(marker_idx)
                && matches!(marker.scope_id.kind(), Some(ScopeKind::Route))
                && retained[marker.scope_id.local_ordinal() as usize]
            {
                let mut arm = 0u8;
                while arm < 2 {
                    if let Some(child) = passive_route_child_enter(markers, marker_idx, arm) {
                        let ordinal = markers.at(child).scope_id.local_ordinal() as usize;
                        if !retained[ordinal] {
                            retained[ordinal] = true;
                            changed = true;
                        }
                    }
                    arm += 1;
                }
            }
            marker_idx += 1;
        }
        if !changed {
            break;
        }
        if remaining == 0 {
            panic!("route retention closure exceeds scope domain");
        }
        remaining -= 1;
    }
    retained
}

pub(crate) const fn retained_route_counts<const E: usize>(
    source: (&EffList<E>, &ScopeFacts<E>),
    retained: &[bool],
) -> (usize, usize) {
    let markers = source.0.scope_markers();
    let mut count = 0usize;
    let mut max_depth = 0usize;
    let mut index = 0usize;
    while index < markers.len() {
        let marker = markers.at(index);
        if markers.is_first_enter(index)
            && matches!(marker.scope_id.kind(), Some(ScopeKind::Route))
            && retained[marker.scope_id.local_ordinal() as usize]
        {
            count += 1;
            let depth = super::route_commit_row_count(source.1, marker.scope_id);
            if depth > max_depth {
                max_depth = depth;
            }
        }
        index += 1;
    }
    (count, max_depth)
}
