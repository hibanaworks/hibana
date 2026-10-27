use super::*;
#[path = "tests/enclosing.rs"]
mod enclosing;
use crate::{
    Endpoint,
    g::{self, Msg},
    global::{
        role_program::{RoleImageRef, project},
        typestate::cursor::validated_metadata_tests::with_rows,
    },
};

fn walk(cursor: &EventCursor, mut conflict: PackedEventConflict, target: ScopeId) -> Option<u8> {
    for _ in 0..cursor.route_chain_bound() {
        let LocalConflict::RouteArm { scope, arm } = conflict.to_conflict()? else {
            return None;
        };
        if scope == target {
            return Some(arm);
        }
        conflict = cursor.route_scope_conflict_row(scope);
    }
    panic!("cyclic route ancestry");
}

fn compare(rows: &'static RoleImageRef) {
    with_rows(rows, |cursor| {
        let scopes: std::vec::Vec<_> = (0..cursor.machine().route_scope_slot_count())
            .map(|slot| rows.route_scope_by_slot(slot).unwrap())
            .collect();
        for &scope in &scopes {
            for arm in 0..=1 {
                let conflict = PackedEventConflict::route_arm(scope, arm);
                for ordinal in 0..ScopeId::LOCAL_CAPACITY {
                    let target = ScopeId::route(ordinal);
                    assert_eq!(
                        cursor.preview_conflict_arm(conflict, target),
                        walk(cursor, conflict, target),
                        "scope={scope:?}, arm={arm}, target={target:?}"
                    );
                }
                assert_eq!(cursor.preview_conflict_arm(conflict, ScopeId::none()), None);
            }
        }
        for &scope in &scopes {
            assert_eq!(
                cursor.preview_conflict_arm(PackedEventConflict::none(), scope),
                None
            );
        }
    });
}

#[test]
fn preview_ancestry_matches_parent_chains_through_parallel_and_repeated_routes() {
    let global = g::seq(
        g::route(
            g::seq(
                g::send::<0, 2, Msg<9, ()>>(),
                g::par(
                    g::route(g::send::<0, 1, Msg<1, ()>>(), g::send::<0, 1, Msg<2, ()>>()),
                    g::route(g::send::<2, 3, Msg<3, ()>>(), g::send::<2, 3, Msg<4, ()>>()),
                ),
            ),
            g::seq(
                g::send::<0, 2, Msg<10, ()>>(),
                g::par(g::send::<0, 1, Msg<5, ()>>(), g::send::<2, 3, Msg<6, ()>>()),
            ),
        )
        .resolve::<0>()
        .roll(),
        g::route(g::send::<0, 1, Msg<7, ()>>(), g::send::<0, 1, Msg<8, ()>>()),
    );
    compare(project::<0>(&global).role_image_ref());
    compare(project::<1>(&global).role_image_ref());
    compare(project::<2>(&global).role_image_ref());
    compare(project::<3>(&global).role_image_ref());
}

#[test]
fn preview_ancestry_matches_parent_chains_in_delegated_views() {
    type Loan<'r> = Msg<9, Endpoint<'r, 3>>;
    let global = g::seq(
        g::send::<0, 1, Loan<'_>>(),
        g::seq(
            g::route(
                g::route(g::send::<3, 2, Msg<1, ()>>(), g::send::<3, 2, Msg<2, ()>>()),
                g::send::<3, 2, Msg<3, ()>>(),
            )
            .roll(),
            g::send::<1, 0, Loan<'_>>(),
        ),
    );
    compare(project::<0>(&global).role_image_ref());
    compare(project::<1>(&global).role_image_ref());
    compare(project::<2>(&global).role_image_ref());
}
