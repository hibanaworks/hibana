use crate::{
    g,
    global::{role_program::project, typestate::cursor::validated_metadata_tests::with_rows},
};

#[test]
fn parallel_completion_observations_cover_every_bit_credit_and_entry() {
    use std::io::Write;
    type Prefix = g::Msg<1, ()>;
    type A = g::Msg<2, ()>;
    type B = g::Msg<3, ()>;
    // All three sends are parallel. Only A and B have their own roll.
    let global = g::par(
        g::send::<0, 1, Prefix>(),
        g::par(g::send::<0, 1, A>().roll(), g::send::<0, 1, B>().roll()),
    )
    .roll();
    let program = project::<0>(&global);
    let mut observations = std::vec::Vec::new();
    with_rows(program.role_image_ref(), |cursor| {
        let scopes: std::vec::Vec<_> = (0..3)
            .map(|slot| cursor.machine().roll_scope_row_by_slot(slot).unwrap())
            .collect();
        let scope = |start, end| {
            scopes
                .iter()
                .find(|(_, row)| row.start() == start && row.end() == end)
                .unwrap()
                .0
        };
        let [parent, a, b] = [scope(0, 3), scope(1, 2), scope(2, 3)];
        for bits in 0..8 {
            for credits in 0..4 {
                for entry in 0usize..4 {
                    for index in 0..3 {
                        if bits & (1 << index) == 0 {
                            cursor.clear_local_event_done(index);
                        } else {
                            cursor.mark_local_event_done(index);
                        }
                    }
                    cursor.set_completed_roll_credits(parent, 0);
                    cursor.set_completed_roll_credits(a, credits & 1);
                    cursor.set_completed_roll_credits(b, (credits >> 1) & 1);
                    let target = entry.checked_sub(1);
                    let before = cursor.completed_event_words().to_vec();
                    let complete =
                        cursor.roll_scope_events_complete_at(parent, target, &mut |_| None);
                    let retain_a = cursor.roll_reset_retains_credit(a, target, &mut |_| None);
                    let retain_b = cursor.roll_reset_retains_credit(b, target, &mut |_| None);
                    assert_eq!(cursor.completed_event_words(), before);
                    writeln!(observations,
                        "{{\"bits\":{bits},\"credits\":{credits},\"entry\":{entry},\"complete\":{complete},\"retainA\":{retain_a},\"retainB\":{retain_b}}}"
                    ).unwrap();
                }
            }
        }
    });
    if let Some(path) = std::env::var_os("HIBANA_ROLL_COMPLETION_OBSERVATIONS") {
        std::fs::write(path, observations).unwrap();
    }
}

#[test]
fn child_credit_accounts_for_a_siblings_retained_visit_at_the_actual_entry() {
    type Prefix = g::Msg<1, ()>;
    type A = g::Msg<2, ()>;
    type B = g::Msg<3, ()>;
    let global = g::par(
        g::send::<0, 1, Prefix>(),
        g::par(g::send::<0, 1, A>().roll(), g::send::<0, 1, B>().roll()),
    )
    .roll();
    let program = project::<0>(&global);
    with_rows(program.role_image_ref(), |cursor| {
        let scopes: std::vec::Vec<_> = (0..3)
            .map(|slot| cursor.machine().roll_scope_row_by_slot(slot).unwrap())
            .collect();
        let parent = scopes
            .iter()
            .find(|(_, row)| row.start() == 0 && row.end() == 3)
            .unwrap()
            .0;
        let a = scopes
            .iter()
            .find(|(_, row)| row.start() == 1 && row.end() == 2)
            .unwrap()
            .0;
        let b = scopes
            .iter()
            .find(|(_, row)| row.start() == 2 && row.end() == 3)
            .unwrap()
            .0;
        for bits in 0..8 {
            for retained in [false, true] {
                for index in 0..3 {
                    if bits & (1 << index) != 0 {
                        cursor.mark_local_event_done(index);
                    } else {
                        cursor.clear_local_event_done(index);
                    }
                }
                cursor.set_completed_roll_credits(a, 0);
                cursor.set_completed_roll_credits(b, u64::from(retained));
                cursor.set_completed_roll_credits(parent, 0);
                let expected = bits & 3 == 3 && (bits & 4 != 0 || retained);
                let before = cursor.completed_event_words().to_vec();
                assert_eq!(
                    cursor.roll_reset_retains_credit(a, Some(1), &mut |_| None),
                    expected,
                    "bits={bits} retained={retained}"
                );
                assert_eq!(
                    cursor.roll_reset_retains_credit(a, None, &mut |_| None),
                    bits == 7
                );
                assert_eq!(cursor.completed_event_words(), before);
                let reset = cursor.prepare_roll_reset(a, Some(1), &mut |_| None);
                cursor.clear_reentry_scope_events(reset);
                assert_eq!(cursor.completed_roll_credits(a), u64::from(expected));
                if expected {
                    // The send in the fresh child visit commits before a
                    // delayed parallel sibling starts the next parent visit.
                    cursor.mark_local_event_done(1);
                    let reset = cursor.prepare_roll_reset(parent, Some(0), &mut |_| None);
                    cursor.clear_reentry_scope_events(reset);
                    assert!(cursor.node_event_done_for_lane(1, cursor.event_lane_at(1).unwrap()));
                    assert_eq!(cursor.completed_roll_credits(a), 0);
                }
            }
        }
    });
}
