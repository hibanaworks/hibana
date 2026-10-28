use super::*;
use crate::global::{const_dsl::ReentryMark, typestate::cursor::scope_route::ConflictAdmission};

fn admission_key(admission: ConflictAdmission) -> (u8, ScopeId, u8) {
    match admission {
        ConflictAdmission::Allowed => (0, ScopeId::none(), 0),
        ConflictAdmission::Excluded { scope, arm } => (1, scope, arm),
        ConflictAdmission::Invalid => (2, ScopeId::none(), 0),
    }
}

// Frozen parent-link interpreter. It does not read the compiled commit range.
fn walked_admission(
    cursor: &EventCursor,
    mut conflict: PackedEventConflict,
    effective: &mut dyn FnMut(ScopeId) -> Option<u8>,
) -> (u8, ScopeId, u8) {
    for _ in 0..cursor.route_chain_bound() {
        let Some(LocalConflict::RouteArm { scope, arm }) = conflict.to_conflict() else {
            return (0, ScopeId::none(), 0);
        };
        if let Some(selected) = effective(scope)
            && selected != arm
        {
            return (1, scope, arm);
        }
        conflict = cursor.route_scope_conflict_row(scope);
    }
    (2, ScopeId::none(), 0)
}

fn walked_offer(
    cursor: &EventCursor,
    index: usize,
    selected: &mut dyn FnMut(ScopeId) -> Option<u8>,
) -> ScopeId {
    let node_scope = cursor.node_scope_id_at(index);
    if node_scope.is_none() {
        return node_scope;
    }
    let mut conflict = cursor.event_conflict_for_index(index);
    let mut pending = ScopeId::none();
    for _ in 0..cursor.route_chain_bound() {
        let Some(LocalConflict::RouteArm { scope, arm }) = conflict.to_conflict() else {
            break;
        };
        if selected(scope) != Some(arm) {
            pending = scope;
        }
        conflict = cursor.route_scope_conflict_row(scope);
    }
    if pending.is_none() {
        node_scope
    } else {
        pending
    }
}

fn selected(choices: &[(ScopeId, Option<u8>)], scope: ScopeId) -> Option<u8> {
    choices
        .iter()
        .find(|(candidate, _)| *candidate == scope)
        .and_then(|(_, arm)| *arm)
}

pub(super) fn compare(rows: &'static RoleImageRef) {
    let count = rows.footprint().route_scope_count;
    assert!(count <= 4);
    with_rows(rows, |cursor| {
        let mut scopes: std::vec::Vec<_> = (0..count)
            .map(|slot| rows.route_scope_by_slot(slot).unwrap())
            .collect();
        let absent = (0..ScopeId::LOCAL_CAPACITY)
            .rev()
            .map(ScopeId::route)
            .find(|scope| !scopes.contains(scope))
            .unwrap();
        // Arbitrary syntactically valid route decisions remain meaningful even
        // when projection omitted their source scope from this actor's image.
        scopes.push(absent);
        let mut conflicts = std::vec![
            PackedEventConflict::none(),
            PackedEventConflict::none().with_route_reentry(ReentryMark::Reentrant),
        ];
        for &scope in &scopes {
            for arm in 0..2 {
                let conflict = PackedEventConflict::route_arm(scope, arm);
                conflicts.push(conflict);
                conflicts.push(conflict.with_route_reentry(ReentryMark::Reentrant));
            }
        }
        for code in 0..3usize.pow(scopes.len() as u32) {
            let mut digits = code;
            let choices: std::vec::Vec<_> = scopes
                .iter()
                .map(|&scope| {
                    let arm = match digits % 3 {
                        0 => None,
                        1 => Some(0),
                        2 => Some(1),
                        _ => unreachable!(),
                    };
                    digits /= 3;
                    (scope, arm)
                })
                .collect();
            for &conflict in &conflicts {
                for preview_scope in scopes.iter().copied().chain([ScopeId::none()]) {
                    for preview_arm in [None, Some(0), Some(1), Some(2)] {
                        let mut actual_calls = std::vec::Vec::new();
                        let actual = cursor.event_conflict_row_admission(
                            conflict,
                            preview_scope,
                            preview_arm,
                            &mut |scope| {
                                actual_calls.push(scope);
                                selected(&choices, scope)
                            },
                        );
                        let mut expected_calls = std::vec::Vec::new();
                        let expected = walked_admission(cursor, conflict, &mut |scope| {
                            expected_calls.push(scope);
                            match selected(&choices, scope) {
                                Some(arm) => Some(arm),
                                None if scope == preview_scope => preview_arm,
                                None => None,
                            }
                        });
                        assert_eq!(admission_key(actual), expected);
                        assert_eq!(actual_calls, expected_calls);
                    }
                }
                for &preview in &conflicts {
                    let mut actual_calls = std::vec::Vec::new();
                    let actual = cursor.event_conflict_row_admission_with_preview(
                        conflict,
                        preview,
                        &mut |scope| {
                            actual_calls.push(scope);
                            selected(&choices, scope)
                        },
                    );
                    let mut expected_calls = std::vec::Vec::new();
                    let expected = walked_admission(cursor, conflict, &mut |scope| {
                        expected_calls.push(scope);
                        selected(&choices, scope).or_else(|| walk(cursor, preview, scope))
                    });
                    assert_eq!(admission_key(actual), expected);
                    assert_eq!(actual_calls, expected_calls);
                }
            }
            for index in 0..=rows.local_step_count() {
                let mut actual_calls = std::vec::Vec::new();
                let actual = cursor.offer_scope_id_at(index, &mut |scope| {
                    actual_calls.push(scope);
                    selected(&choices, scope)
                });
                let mut expected_calls = std::vec::Vec::new();
                let expected = walked_offer(cursor, index, &mut |scope| {
                    expected_calls.push(scope);
                    selected(&choices, scope)
                });
                assert_eq!(actual, expected);
                assert_eq!(actual_calls, expected_calls);
            }
        }
    });
}

#[test]
fn raw_nonlocal_conflicts_keep_the_zero_route_depth_bound() {
    let rows = project::<0>(&g::send::<0, 1, Msg<1, ()>>()).role_image_ref();
    assert_eq!(rows.footprint().route_scope_count, 0);
    compare(rows);
    with_rows(rows, |cursor| {
        let scope = ScopeId::route(ScopeId::LOCAL_CAPACITY - 1);
        let conflict = PackedEventConflict::route_arm(scope, 0);
        assert_eq!(
            admission_key(cursor.event_conflict_row_admission(
                conflict,
                ScopeId::none(),
                None,
                &mut |_| None,
            )),
            (2, ScopeId::none(), 0),
        );
        assert_eq!(
            admission_key(cursor.event_conflict_row_admission(
                conflict,
                ScopeId::none(),
                None,
                &mut |_| Some(1),
            )),
            (1, scope, 0),
        );
    });
}
