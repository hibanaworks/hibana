use crate::{
    g,
    global::{role_program::project, typestate::cursor::validated_metadata_tests::with_rows},
};

#[test]
fn unfinished_entry_admission_matches_all_enclosing_rolls_and_choice_assignments() {
    fn compare(image: &'static crate::global::role_program::RoleImageRef) -> (usize, usize) {
        let mut live_entries = 0;
        let mut excluded_entries_with_completion = 0;
        with_rows(image, |cursor| {
            let count = cursor.local_steps_len();
            let scopes: std::vec::Vec<_> = (0..image.footprint().roll_scope_count)
                .map(|slot| cursor.machine().roll_scope_row_by_slot(slot).unwrap())
                .collect();
            let routes: std::vec::Vec<_> = (0..image.footprint().route_scope_count)
                .map(|slot| image.route_scope_by_slot(slot).unwrap())
                .collect();
            let credits = if image.footprint().borrow_epoch_count == 0 {
                0
            } else {
                scopes.len()
            };
            for choices in 0..3usize.pow(routes.len() as u32) {
                let selected = |scope| {
                    let slot = routes.iter().position(|&candidate| candidate == scope)?;
                    match choices / 3usize.pow(slot as u32) % 3 {
                        0 => None,
                        1 => Some(0),
                        _ => Some(1),
                    }
                };
                for bits in 0..1usize << count {
                    for index in 0..count {
                        if bits & (1 << index) == 0 {
                            cursor.clear_local_event_done(index);
                        } else {
                            cursor.mark_local_event_done(index);
                        }
                    }
                    for retained in 0..1usize << credits {
                        for (slot, &(scope, _)) in scopes.iter().take(credits).enumerate() {
                            cursor.set_completed_roll_credits(
                                scope,
                                u64::from(retained & (1 << slot) != 0),
                            );
                        }
                        for index in 0..count {
                            let expected: std::vec::Vec<_> = scopes
                                .iter()
                                .filter(|(_, row)| row.start() <= index && index < row.end())
                                .filter_map(|&(scope, _)| {
                                    cursor
                                        .roll_scope_complete_for_entry(scope, index, &mut |scope| {
                                            selected(scope)
                                        })
                                        .then_some(scope)
                                })
                                .collect();
                            let before = cursor.completed_event_words().to_vec();
                            let found = cursor
                                .complete_roll_body_scope_for_index(index, &mut |scope| {
                                    selected(scope)
                                });
                            assert_eq!(
                                found.is_some(),
                                !expected.is_empty(),
                                "index={index}, bits={bits}, credits={retained}, choices={choices}"
                            );
                            if let Some(scope) = found {
                                assert!(expected.contains(&scope));
                            }
                            assert_eq!(cursor.completed_event_words(), before);
                            if bits & (1 << index) == 0 {
                                if cursor.event_conflict_row_allows(
                                    cursor.event_conflict_for_index(index),
                                    crate::global::const_dsl::ScopeId::none(),
                                    None,
                                    &mut |scope| selected(scope),
                                ) && cursor
                                    .event_actor_is_local(index, &mut |scope| selected(scope))
                                    != Some(false)
                                {
                                    live_entries += 1;
                                    assert!(expected.is_empty());
                                } else if !expected.is_empty() {
                                    excluded_entries_with_completion += 1;
                                }
                            }
                        }
                    }
                }
            }
        });
        (live_entries, excluded_entries_with_completion)
    }

    let parallel = g::par(
        g::route(
            g::send::<0, 1, g::Msg<1, ()>>(),
            g::send::<0, 1, g::Msg<2, ()>>(),
        )
        .roll(),
        g::send::<0, 1, g::Msg<3, ()>>().roll(),
    )
    .roll();
    type Loan = g::Msg<4, crate::Endpoint<'static, 2>>;
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
    let results = [
        compare(project::<0>(&parallel).role_image_ref()),
        compare(project::<1>(&parallel).role_image_ref()),
        compare(project::<0>(&delegated).role_image_ref()),
        compare(project::<1>(&delegated).role_image_ref()),
        compare(project::<2>(&delegated).role_image_ref()),
        compare(project::<3>(&delegated).role_image_ref()),
    ];
    assert!(results.iter().all(|(live, _)| *live != 0));
    assert!(results.iter().map(|(_, excluded)| excluded).sum::<usize>() != 0);
}

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

#[test]
fn retained_predecessor_ranges_match_independent_source_intervals() {
    let global = g::seq(
        g::par(
            g::seq(
                g::send::<0, 1, g::Msg<1, ()>>().roll(),
                g::send::<0, 1, g::Msg<2, ()>>(),
            ),
            g::send::<0, 1, g::Msg<3, ()>>().roll(),
        )
        .roll(),
        g::send::<0, 1, g::Msg<4, ()>>(),
    );
    let source = project::<0>(&global);
    let incoming = project::<1>(&global);
    for (image, receiving) in [
        (source.role_image_ref(), false),
        (incoming.role_image_ref(), true),
    ] {
        with_rows(image, |cursor| {
            let count = cursor.local_steps_len();
            assert_eq!(count, 4);
            let scopes: std::vec::Vec<_> = (0..3)
                .map(|slot| cursor.machine().roll_scope_row_by_slot(slot).unwrap().0)
                .collect();
            let raw: std::vec::Vec<_> = (0..cursor.program_ref().source_scope_marker_count())
                .filter_map(|marker| cursor.program_ref().source_roll_at(marker))
                .collect();
            assert_eq!(raw.len(), scopes.len());
            // One ordinary facet per source effect in both images lets the
            // source interval end be the independent local-row end oracle.
            for index in 0..count {
                assert_eq!(
                    cursor
                        .machine()
                        .event_program()
                        .operation_at(index)
                        .unwrap()
                        .0
                        .dense_ordinal(),
                    index
                );
            }
            for retained in 0..1_u32 << scopes.len() {
                for slot in 0..scopes.len() {
                    cursor.write_completion_bit(count + slot, retained & (1 << slot) != 0);
                }
                let before = cursor.completed_event_words().to_vec();
                let expected = |predecessor: usize, target: usize| {
                    if !receiving || target == count {
                        return None;
                    }
                    raw.iter()
                        .filter(|(scope, start, end)| {
                            let slot = scopes
                                .iter()
                                .position(|candidate| candidate == scope)
                                .unwrap();
                            retained & (1 << slot) != 0
                                && *start <= predecessor
                                && predecessor < *end
                                && !(*start <= target && target < *end)
                        })
                        .map(|(_, _, end)| *end)
                        .max()
                };
                for predecessor in 0..count {
                    for target in 0..=count {
                        let actual = cursor.retained_roll_predecessor_end(predecessor, target);
                        assert_eq!(actual, expected(predecessor, target));
                        if let Some(end) = actual {
                            assert!(predecessor < end);
                            for skipped in predecessor..end {
                                assert!(expected(skipped, target).is_some());
                            }
                        }
                    }
                }
                assert_eq!(cursor.completed_event_words(), before);
            }
        });
    }
}

#[test]
fn retained_ranges_skip_only_covered_predecessors_for_both_admission_scans() {
    use crate::global::{
        const_dsl::ScopeId,
        typestate::{LocalConflict, LocalDependency, PackedEventConflict},
    };

    let global = g::seq(
        g::send::<0, 1, g::Msg<1, ()>>(),
        g::seq(
            g::seq(
                g::send::<0, 1, g::Msg<2, ()>>(),
                g::send::<0, 1, g::Msg<3, ()>>(),
            )
            .roll(),
            g::send::<0, 1, g::Msg<4, ()>>(),
        ),
    );
    let program = project::<1>(&global);
    with_rows(program.role_image_ref(), |cursor| {
        let count = cursor.local_steps_len();
        let (_, body) = cursor.machine().roll_scope_row_by_slot(0).unwrap();
        assert_eq!((body.start(), body.end()), (1, 3));
        cursor.write_completion_bit(count, true);
        assert_eq!(cursor.retained_roll_predecessor_end(1, 3), Some(3));
        assert_eq!(cursor.retained_roll_predecessor_end(1, 2), None);
        let target = cursor
            .relocatable_resident_lane_step_at_index(3, 0)
            .unwrap();
        for completed in 0..1_u32 << count {
            for index in 0..count {
                cursor.write_completion_bit(index, completed & (1 << index) != 0);
            }
            let before = cursor.completed_event_words().to_vec();
            // The prefix is outside the retained body. Its completion remains
            // mandatory even though both current body events are covered.
            assert_eq!(
                cursor.event_lane_head_allows(target, PackedEventConflict::none(), &mut |_| None),
                completed & 1 != 0
            );
            // Also clip a query inside the skipped source interval. This
            // exercises min(retained_end, dependency_end), including one row.
            for start in 0..3 {
                for end in start + 1..=3 {
                    let dependency = LocalDependency::with_conflict_range(
                        ScopeId::none(),
                        LocalConflict::Unconditional,
                        start,
                        end,
                    );
                    let expected = (start..end)
                        .all(|index| completed & (1 << index) != 0 || (1..3).contains(&index));
                    assert_eq!(
                        cursor.dependency_row_live_events_done(3, dependency, &mut |_| None),
                        expected,
                        "completed={completed:04b} range={start}..{end}"
                    );
                }
            }
            assert_eq!(cursor.completed_event_words(), before);
        }
    });
}
