use super::{DependencyCursor, LocalRows, ScopeFacts, passive_arm_child_scope, route_arm_ranges};
use crate::global::{
    const_dsl::{EffList, ScopeId, ScopeKind},
    typestate::LocalConflict,
};

// Temporary compiler workspace only. No retention data is emitted or observed
// by the runtime. Global route identities remain unchanged after compaction.
pub(in crate::global::role_program::image_impl) const fn retained_route_scopes<const E: usize>(
    source: (&EffList<E>, &ScopeFacts),
    rows: LocalRows<'_, E>,
) -> [bool; ScopeId::LOCAL_CAPACITY as usize] {
    let (effects, scopes) = source;
    let markers = effects.scope_markers();
    let mut retained = [false; ScopeId::LOCAL_CAPACITY as usize];
    let mut marker_idx = 0usize;
    while marker_idx < markers.len() {
        let marker = markers.at(marker_idx);
        if markers.is_first_enter(marker_idx)
            && matches!(marker.scope_id.kind(), Some(ScopeKind::Route))
        {
            if rows.retains_context(marker.scope_id) {
                retained[marker.scope_id.local_ordinal() as usize] = true;
            }
            let Some(arms) = route_arm_ranges(markers, marker.scope_id) else {
                panic!("route scope missing binary arm ranges");
            };
            let mut arm = 0usize;
            while arm < 2 {
                let (mut index, end) = arms[arm];
                while index < end {
                    if rows.count(effects, index) != 0 {
                        // The global controller is an arm's visible sender,
                        // and participants are exactly its effect endpoints.
                        retained[marker.scope_id.local_ordinal() as usize] = true;
                        break;
                    }
                    index += 1;
                }
                arm += 1;
            }
        }
        marker_idx += 1;
    }
    let mut dependencies = DependencyCursor::new(effects, scopes, rows);
    let local_len = rows.range(effects, 0, effects.len()).len();
    let mut index = 0usize;
    let mut local_step = 0usize;
    while index < effects.len() {
        let effect = effects.atom_at(index);
        let mut side = 0;
        while side < rows.count(effects, index) {
            let dependency = dependencies.next(index, effect.lane, local_step);
            // Local positions are bounded by the source effect domain.
            if let Some(dependency) = dependency.to_dependency(local_len)
                && let LocalConflict::RouteArm { scope, .. } = dependency.conflict()
            {
                retained[scope.local_ordinal() as usize] = true;
            }
            local_step += 1;
            side += 1;
        }
        index += 1;
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
                    if let Some(child) = passive_arm_child_scope(markers, marker.scope_id, arm)
                        && !retained[child.local_ordinal() as usize]
                    {
                        retained[child.local_ordinal() as usize] = true;
                        changed = true;
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
    source: (&EffList<E>, &ScopeFacts),
    rows: LocalRows<'_, E>,
) -> (usize, usize) {
    let retained = retained_route_scopes(source, rows);
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
