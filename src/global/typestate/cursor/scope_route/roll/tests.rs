use super::*;
use crate::{
    Endpoint,
    g::{self, Msg},
    global::{
        role_program::{RoleImageRef, project},
        typestate::cursor::validated_metadata_tests::with_rows,
    },
};

fn compare_completion_domains(rows: &'static RoleImageRef) {
    with_rows(rows, |cursor| {
        let scopes: std::vec::Vec<_> = (0..cursor.machine().route_scope_slot_count())
            .map(|slot| {
                cursor
                    .machine()
                    .route_scope_rows_by_slot(slot)
                    .unwrap()
                    .scope()
            })
            .collect();
        let steps = cursor.local_steps_len();
        // Every completion pattern is checked; the fixtures deliberately have
        // few steps so this comparison does not select only reachable samples.
        assert!(steps <= 12);
        for assignment in 0..3usize.pow(scopes.len() as u32) {
            let mut selected = |scope| {
                let position = scopes.iter().position(|candidate| *candidate == scope)?;
                match assignment / 3usize.pow(position as u32) % 3 {
                    0 => None,
                    1 => Some(0),
                    2 => Some(1),
                    _ => unreachable!(),
                }
            };
            let mut slot = 0;
            while let Some((scope, row)) = cursor.machine().roll_scope_row_by_slot(slot) {
                // The selected local domain is independent of completion.
                // Its set inclusion in the completion word specifies completion
                // without depending on either iterator's traversal order.
                let members = (row.start()..row.end())
                    .filter(|&index| {
                        cursor.event_lane_at(index).is_some()
                            && cursor.event_actor_is_local(index, &mut selected) != Some(false)
                            && cursor.event_conflict_row_allows(
                                cursor.machine().event_conflict_for_index(index),
                                ScopeId::none(),
                                None,
                                &mut selected,
                            )
                    })
                    .fold(0usize, |mask, index| mask | 1 << index);
                for completed in 0..1usize << steps {
                    for index in 0..steps {
                        if completed & (1 << index) == 0 {
                            cursor.clear_local_event_done(index);
                        } else {
                            cursor.mark_local_event_done(index);
                        }
                    }
                    let before = cursor.completed_event_words().to_vec();
                    assert_eq!(
                        cursor.roll_scope_events_complete(scope, &mut selected),
                        members != 0 && completed & members == members,
                        "actor={} slot={slot} assignment={assignment} completed={completed:x}",
                        rows.role(),
                    );
                    assert_eq!(cursor.completed_event_words(), before);
                }
                slot += 1;
            }
        }
    });
}

#[test]
fn scope_completion_matches_selected_members_for_every_completion_pattern() {
    type Request = Msg<1, u32>;
    type Reply = Msg<2, u32>;
    type Notice = Msg<3, u32>;
    type Other = Msg<4, u32>;
    let exchange = g::seq(g::send::<0, 1, Request>(), g::send::<1, 0, Reply>()).roll();
    let choice = g::route(
        g::par(g::send::<0, 1, Request>(), g::send::<0, 2, Reply>()),
        g::par(g::send::<0, 1, Notice>(), g::send::<0, 2, Other>()),
    )
    .roll();
    let nested = g::seq(exchange, g::send::<0, 1, Notice>()).roll();
    let nested_choice = g::route(
        g::route(g::send::<0, 1, Request>(), g::send::<0, 1, Reply>()),
        g::par(g::send::<0, 1, Notice>(), g::send::<0, 1, Other>()),
    )
    .roll();
    type Loan = Msg<9, Endpoint<'static, 10>>;
    let owned = g::seq(
        g::send::<10, 2, Request>(),
        g::seq(
            g::send::<0, 1, Loan>(),
            g::seq(
                g::send::<10, 2, Request>(),
                g::seq(g::send::<1, 0, Loan>(), g::send::<10, 2, Request>()),
            ),
        ),
    )
    .roll();
    for rows in [
        project::<0>(&exchange).role_image_ref(),
        project::<1>(&exchange).role_image_ref(),
        project::<0>(&choice).role_image_ref(),
        project::<1>(&choice).role_image_ref(),
        project::<2>(&choice).role_image_ref(),
        project::<0>(&nested).role_image_ref(),
        project::<1>(&nested).role_image_ref(),
        project::<0>(&nested_choice).role_image_ref(),
        project::<1>(&nested_choice).role_image_ref(),
        project::<0>(&owned).role_image_ref(),
        project::<1>(&owned).role_image_ref(),
    ] {
        compare_completion_domains(rows);
    }
}

#[test]
fn roll_completion_skips_excluded_arms_without_skipping_the_continuation() {
    fn block<const LABEL: u8>() -> impl crate::global::program::Projectable {
        let one = || g::send::<0, 1, Msg<LABEL, ()>>();
        let two = || g::seq(one(), one());
        let four = || g::seq(two(), two());
        let eight = || g::seq(four(), four());
        let sixteen = || g::seq(eight(), eight());
        g::seq(sixteen(), sixteen())
    }
    let global = g::seq(
        g::route(block::<1>(), block::<2>()),
        g::send::<0, 1, Msg<3, ()>>(),
    )
    .roll();
    for image in [
        project::<0>(&global).role_image_ref(),
        project::<1>(&global).role_image_ref(),
    ] {
        assert_eq!(image.local_step_count(), 65);
        let choice = image.route_scope_by_slot(0).unwrap();
        with_rows(image, |cursor| {
            let (scope, _) = cursor.machine().roll_scope_row_by_slot(0).unwrap();
            for selected in [None, Some(0), Some(1)] {
                for left in [false, true] {
                    for right in [false, true] {
                        for continuation in [false, true] {
                            for index in 0..65 {
                                let done = match index {
                                    0..32 => left,
                                    32..64 => right,
                                    64 => continuation,
                                    _ => unreachable!(),
                                };
                                if done {
                                    cursor.mark_local_event_done(index);
                                } else {
                                    cursor.clear_local_event_done(index);
                                }
                            }
                            let expected = continuation
                                && (selected == Some(1) || left)
                                && (selected == Some(0) || right);
                            let before = cursor.completed_event_words().to_vec();
                            assert_eq!(
                                cursor.roll_scope_events_complete(scope, &mut |requested| {
                                    assert_eq!(requested, choice);
                                    selected
                                }),
                                expected,
                                "selected={selected:?}, left={left}, right={right}, continuation={continuation}"
                            );
                            assert_eq!(cursor.completed_event_words(), before);
                        }
                    }
                }
            }
        });
    }
}
