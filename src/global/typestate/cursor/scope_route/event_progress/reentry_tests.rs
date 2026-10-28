use super::*;
use crate::global::typestate::cursor::validated_metadata_tests::with_rows;
use crate::{Endpoint, g, runtime::program::project};

// The unfactored rule searches completion before checking the current path.
// Keep this interpreter separate from the admission function being compared.
fn unfactored(
    cursor: &EventCursor,
    index: usize,
    step: RelocatableResidentLaneStep,
    lane: u8,
    choices: &mut dyn FnMut(ScopeId, EventArmView) -> Option<u8>,
) -> Result<(), EventBlocker> {
    if !cursor.relocatable_step_done(step) {
        let completed = cursor.complete_roll_body_scope_for_index(index, &mut |scope| {
            choices(scope, EventArmView::Committed)
        });
        if let Some(scope) = completed {
            return (cursor.has_reentry_scopes()
                && cursor.roll_scope_head_allows_index(scope, index, lane, &mut |scope| {
                    choices(scope, EventArmView::Preview)
                }))
            .then_some(())
            .ok_or(EventBlocker::Reentry);
        }
        let conflict = cursor.event_conflict_for_index(index);
        return cursor
            .event_conflict_row_allows_with_preview(conflict, conflict, &mut |scope| {
                choices(scope, EventArmView::Committed)
            })
            .then_some(())
            .ok_or(EventBlocker::Invariant);
    }
    (cursor.has_reentry_scopes() && cursor.roll_reentry_event_allows_index(index, lane, choices))
        .then_some(())
        .ok_or(EventBlocker::Reentry)
}

#[test]
fn current_visit_admission_matches_unfactored_nested_and_owned_reentry() {
    let nested = g::par(
        g::route(
            g::send::<0, 1, g::Msg<1, ()>>(),
            g::send::<0, 1, g::Msg<2, ()>>(),
        )
        .roll(),
        g::send::<0, 1, g::Msg<3, ()>>().roll(),
    )
    .roll();
    let choices = g::route(
        g::route(
            g::send::<0, 1, g::Msg<1, ()>>(),
            g::send::<0, 1, g::Msg<2, ()>>(),
        ),
        g::send::<0, 1, g::Msg<3, ()>>(),
    )
    .roll();
    type Loan = g::Msg<4, Endpoint<'static, 2>>;
    let delegated = g::seq(
        g::send::<0, 1, Loan>(),
        g::seq(
            g::route(
                g::send::<2, 3, g::Msg<5, ()>>(),
                g::send::<2, 3, g::Msg<6, ()>>(),
            ),
            g::send::<1, 0, Loan>(),
        ),
    )
    .roll();
    let straight = g::seq(
        g::send::<0, 1, g::Msg<1, ()>>(),
        g::send::<1, 0, g::Msg<2, ()>>(),
    );
    let mut comparisons = 0;
    for image in [
        project::<0>(&nested).role_image_ref(),
        project::<1>(&nested).role_image_ref(),
        project::<0>(&choices).role_image_ref(),
        project::<1>(&choices).role_image_ref(),
        project::<0>(&delegated).role_image_ref(),
        project::<1>(&delegated).role_image_ref(),
        project::<2>(&delegated).role_image_ref(),
        project::<3>(&delegated).role_image_ref(),
        project::<0>(&straight).role_image_ref(),
        project::<1>(&straight).role_image_ref(),
    ] {
        with_rows(image, |cursor| {
            let count = cursor.local_steps_len();
            let route_count = image.footprint().route_scope_count;
            let credits = if image.footprint().borrow_epoch_count == 0 {
                0
            } else {
                image.footprint().roll_scope_count
            };
            assert!(count <= 6 && route_count <= 2 && credits <= 3);
            for committed in 0..3usize.pow(route_count as u32) {
                for preview in 0..3usize.pow(route_count as u32) {
                    let choice = |scope, view| {
                        let slot = image.route_scope_slot(scope)?;
                        let assignment = match view {
                            EventArmView::Committed => committed,
                            EventArmView::Preview => preview,
                        };
                        match assignment / 3usize.pow(slot as u32) % 3 {
                            0 => None,
                            1 => Some(0),
                            _ => Some(1),
                        }
                    };
                    for done in 0..1usize << count {
                        cursor.completed_event_words_mut()[0] = done as u32;
                        for retained in 0..1usize << credits {
                            let start = (count + image.footprint().roll_scope_count).div_ceil(32)
                                + 2 * image.footprint().borrow_epoch_count;
                            for slot in 0..credits {
                                let words = cursor.completed_event_words_mut();
                                words[start + 2 * slot] = u32::from(retained & (1 << slot) != 0);
                                words[start + 2 * slot + 1] = 0;
                            }
                            for index in 0..count {
                                let lane = cursor.event_lane_at(index).unwrap();
                                let step = cursor
                                    .relocatable_resident_lane_step_at_index(index, lane as usize)
                                    .unwrap();
                                let expected =
                                    unfactored(cursor, index, step, lane, &mut |scope, view| {
                                        choice(scope, view)
                                    });
                                let actual = cursor.validate_event_eligibility_reentry(
                                    index,
                                    step,
                                    ScopeId::none(),
                                    lane,
                                    &mut |scope, view| choice(scope, view),
                                );
                                assert_eq!(
                                    actual,
                                    expected,
                                    "role={}, index={index}, done={done}, retained={retained}, committed={committed}, preview={preview}",
                                    image.role()
                                );
                                assert_eq!(cursor.completed_event_words()[0], done as u32);
                                comparisons += 1;
                            }
                        }
                    }
                }
            }
        });
    }
    assert!(comparisons > 0);
    std::eprintln!("current-visit admission comparisons={comparisons}");
}
