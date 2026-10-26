use super::*;
use crate::{
    g, global::typestate::cursor::validated_metadata_tests::with_rows, runtime::program::project,
};

#[test]
fn excluded_arm_jump_preserves_pending_successor_across_bitmap_words() {
    fn block<const LABEL: u8>() -> impl crate::global::program::Projectable {
        let one = || g::send::<0, 1, g::Msg<LABEL, ()>>();
        let two = || g::seq(one(), one());
        let four = || g::seq(two(), two());
        let eight = || g::seq(four(), four());
        let sixteen = || g::seq(eight(), eight());
        g::seq(sixteen(), sixteen())
    }
    let global = g::par(
        g::route(block::<1>(), block::<2>()).roll(),
        g::send::<0, 1, g::Msg<3, ()>>(),
    );
    let program = project::<1>(&global);
    let image = program.role_image_ref();
    assert_eq!(image.local_step_count(), 65);
    let scope = image.route_scope_by_slot(0).unwrap();
    with_rows(image, |cursor| {
        for step in 32..64 {
            cursor.mark_local_event_done(step);
        }
        for choice in [None, Some(0), Some(1)] {
            for start in [0, 1, 31, 32, 33, 63, 64, 65] {
                for end in [start, 64.max(start), 65] {
                    let expected = (start..end)
                        .all(|step| (32..64).contains(&step) || (step < 32 && choice == Some(1)));
                    assert_eq!(
                        cursor.event_row_set_live_events_done(
                            LocalEventRowSet::new(start, end),
                            &mut |requested| {
                                assert_eq!(requested, scope);
                                choice
                            },
                        ),
                        expected,
                        "choice={choice:?}, range={start}..{end}"
                    );
                }
            }
        }
    });
}

#[test]
fn excluded_arm_skips_match_source_choices_for_every_cut_and_subrange() {
    let global = g::par(
        g::route(
            g::route(
                g::send::<0, 1, g::Msg<1, ()>>(),
                g::send::<0, 1, g::Msg<2, ()>>(),
            ),
            g::route(
                g::send::<0, 1, g::Msg<3, ()>>(),
                g::send::<0, 1, g::Msg<4, ()>>(),
            ),
        )
        .roll(),
        g::route(
            g::send::<0, 1, g::Msg<5, ()>>(),
            g::send::<0, 1, g::Msg<6, ()>>(),
        ),
    );
    let program = project::<1>(&global);
    let image = program.role_image_ref();
    assert_eq!(image.local_step_count(), 6);
    assert_eq!(image.footprint().route_scope_count, 4);
    let scopes: [ScopeId; 4] =
        core::array::from_fn(|slot| image.route_scope_by_slot(slot).unwrap());
    for completed in 0u8..64 {
        with_rows(image, |cursor| {
            for step in 0..6 {
                assert_eq!(
                    cursor.try_recv_meta_at(step).unwrap().label as usize,
                    step + 1
                );
                if completed & (1 << step) != 0 {
                    cursor.mark_local_event_done(step);
                }
            }
            for choices in 0..81 {
                let mut code = choices;
                let selected: [Option<u8>; 4] = core::array::from_fn(|_| {
                    let digit = code % 3;
                    code /= 3;
                    match digit {
                        0 => None,
                        1 => Some(0),
                        2 => Some(1),
                        _ => unreachable!(),
                    }
                });
                let allows = |slot: usize, arm| selected[slot].is_none_or(|value| value == arm);
                // Source semantics, independent of packed conflict rows and their parent links.
                let live = [
                    allows(0, 0) && allows(1, 0),
                    allows(0, 0) && allows(1, 1),
                    allows(0, 1) && allows(2, 0),
                    allows(0, 1) && allows(2, 1),
                    allows(3, 0),
                    allows(3, 1),
                ];
                for start in 0..=6 {
                    for end in start..=6 {
                        let expected =
                            (start..end).all(|step| completed & (1 << step) != 0 || !live[step]);
                        let actual = cursor.event_row_set_live_events_done(
                            LocalEventRowSet::new(start, end),
                            &mut |scope| {
                                scopes
                                    .iter()
                                    .position(|&candidate| candidate == scope)
                                    .and_then(|slot| selected[slot])
                            },
                        );
                        assert_eq!(
                            actual, expected,
                            "completed={completed:06b}, choices={choices}, range={start}..{end}"
                        );
                    }
                }
            }
        });
    }
}
