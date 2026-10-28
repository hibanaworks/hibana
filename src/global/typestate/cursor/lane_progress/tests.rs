use super::super::CURRENT_STEP_UNLABELED_CODE;
use super::*;
use crate::{
    Endpoint,
    g::{self, Msg},
    global::{
        compiled::images::RoleDescriptorRef,
        role_program::{RoleImageRef, RoleLocalDirection, RoleProgram, project},
        typestate::cursor::EventCursorState,
    },
};
use core::mem::MaybeUninit;

fn compare_lane_heads<const ROLE: u8>(program: &RoleProgram<ROLE>) {
    let descriptor = RoleDescriptorRef::from_resident(program.role_image_ref());
    let rows = descriptor.local_event_rows();
    let mut state = MaybeUninit::<EventCursorState>::uninit();
    let mut cursor = MaybeUninit::<EventCursor>::uninit();
    let mut heads = [0u16; 256];
    let mut labels = [0u16; 256];
    let mut done = std::vec![0u32; rows.footprint().completion_bit_count().div_ceil(32)];
    // The backing columns outlive the unpublished cursor and are disjoint.
    unsafe {
        EventCursor::init_from_compiled(
            cursor.as_mut_ptr(),
            state.as_mut_ptr(),
            heads.as_mut_ptr(),
            labels.as_mut_ptr(),
            done.as_mut_ptr(),
            descriptor,
        );
    }
    let cursor = unsafe { cursor.assume_init_mut() };
    // Enumerate descriptor events independently of the compiled successor
    // column. Every lane covers the whole image, including sparse later rows.
    for lane in 0..cursor.logical_lane_count() {
        let expected: std::vec::Vec<_> = (0..rows.local_step_count())
            .filter(|&step| rows.local_step_lane(step) == Some(lane as u8))
            .collect();
        assert_eq!(cursor.step_index_at_lane(lane), expected.first().copied());
        for (ordinal, &step) in expected.iter().enumerate() {
            assert_eq!(cursor.step_index_at_lane(lane), Some(step));
            let previous = cursor.lane_cursors().to_vec();
            let previous_labels = cursor.current_step_label_codes().to_vec();
            let target = cursor
                .relocatable_resident_lane_step_at_index(step, lane)
                .expect("matching lane identity");
            cursor.advance_lane_to_relocatable_step(target);
            assert!(cursor.relocatable_step_done(target));
            assert_eq!(
                cursor.step_index_at_lane(lane),
                expected.get(ordinal + 1).copied()
            );
            assert_eq!(
                cursor.current_step_label_codes()[lane],
                expected
                    .get(ordinal + 1)
                    .map_or(CURRENT_STEP_UNLABELED_CODE, |&next| {
                        u16::from(cursor.event_row_at(next).unwrap().atom().label)
                    })
            );
            for (other, before) in previous.iter().enumerate() {
                if other != lane {
                    assert_eq!(cursor.lane_cursors()[other], *before);
                    assert_eq!(
                        cursor.current_step_label_codes()[other],
                        previous_labels[other]
                    );
                }
            }
            let advanced = cursor.lane_cursors()[lane];
            cursor.advance_lane_to_relocatable_step(target);
            assert_eq!(
                cursor.lane_cursors()[lane],
                advanced,
                "replay cannot rewind"
            );
            for foreign in 0..cursor.logical_lane_count() {
                if foreign != lane {
                    assert!(
                        cursor
                            .relocatable_resident_lane_step_at_index(step, foreign)
                            .is_err()
                    );
                }
            }
        }
    }
    assert!(
        cursor
            .relocatable_resident_lane_step_at_index(rows.local_step_count(), 0)
            .is_err()
    );
    // Explicit relocation must restore the same lane head, including backwards
    // relocation used by reentry; completion bits remain separate authority.
    for step in (0..rows.local_step_count()).rev() {
        let lane = usize::from(rows.local_step_lane(step).unwrap());
        let target = cursor
            .relocatable_resident_lane_step_at_index(step, lane)
            .unwrap();
        let previous = cursor.lane_cursors().to_vec();
        let previous_labels = cursor.current_step_label_codes().to_vec();
        let mutations = cursor.completion_mutations();
        cursor.set_lane_cursor_to_relocatable_step(target);
        assert_eq!(cursor.completion_mutations(), mutations);
        assert_eq!(cursor.step_index_at_lane(lane), Some(step));
        assert_eq!(cursor.node_index_for_relocatable_step(target), Some(step));
        assert_eq!(
            cursor.current_step_label_codes()[lane],
            u16::from(cursor.event_row_at(step).unwrap().atom().label)
        );
        for (other, before) in previous.iter().enumerate() {
            if other != lane {
                assert_eq!(cursor.lane_cursors()[other], *before);
                assert_eq!(
                    cursor.current_step_label_codes()[other],
                    previous_labels[other]
                );
            }
        }
    }
}

#[test]
fn whole_image_lane_heads_match_descriptor_order_across_sparse_rows_and_relocation() {
    let choreography = g::seq(
        g::send::<0, 1, Msg<1, u8>>(),
        g::seq(
            g::par(
                g::seq(g::send::<0, 1, Msg<2, u8>>(), g::send::<1, 0, Msg<3, u8>>()),
                g::seq(g::send::<0, 2, Msg<4, u8>>(), g::send::<2, 0, Msg<5, u8>>()),
            ),
            g::send::<0, 1, Msg<6, u8>>(),
        ),
    );
    let role0: RoleProgram<0> = project(&choreography);
    let role1: RoleProgram<1> = project(&choreography);
    let role2: RoleProgram<2> = project(&choreography);
    compare_lane_heads(&role0);
    compare_lane_heads(&role1);
    compare_lane_heads(&role2);
}

#[test]
fn whole_image_lane_heads_match_descriptor_order_for_routes_and_roll() {
    let choreography = g::seq(
        g::route(g::send::<0, 1, Msg<7, u8>>(), g::send::<0, 1, Msg<8, u8>>()),
        g::par(
            g::send::<0, 1, Msg<9, u8>>(),
            g::send::<0, 2, Msg<10, u8>>(),
        )
        .roll(),
    );
    let role0: RoleProgram<0> = project(&choreography);
    let role1: RoleProgram<1> = project(&choreography);
    let role2: RoleProgram<2> = project(&choreography);
    compare_lane_heads(&role0);
    compare_lane_heads(&role1);
    compare_lane_heads(&role2);
}

#[test]
fn every_completion_order_preserves_each_lanes_first_unfinished_row() {
    let global = g::seq(
        g::send::<0, 1, Msg<61, ()>>(),
        g::seq(
            g::par(
                g::seq(
                    g::send::<0, 1, Msg<62, ()>>(),
                    g::send::<1, 0, Msg<63, ()>>(),
                ),
                g::seq(
                    g::send::<0, 2, Msg<64, ()>>(),
                    g::send::<2, 0, Msg<65, ()>>(),
                ),
            ),
            g::send::<0, 1, Msg<66, ()>>(),
        ),
    );
    let rows = project::<0>(&global).role_image_ref();
    let count = rows.local_step_count();
    assert_eq!(count, 6);
    let orders: usize = (1..=count).product();
    for order in 0..orders {
        super::super::validated_metadata_tests::with_rows(rows, |cursor| {
            let mut remaining: std::vec::Vec<_> = (0..count).collect();
            let mut rank = order;
            let mut completed = std::vec![false; count];
            cursor.set_index(count);
            while !remaining.is_empty() {
                let ordinal = rank % remaining.len();
                rank /= remaining.len();
                let step = remaining.remove(ordinal);
                let lane = usize::from(rows.local_step_lane(step).unwrap());
                let target = cursor
                    .relocatable_resident_lane_step_at_index(step, lane)
                    .unwrap();
                // This isolates completion bookkeeping from semantic admission:
                // every permutation must preserve the same derived-head law.
                cursor.advance_lane_to_relocatable_step(target);
                completed[step] = true;
                assert_eq!(
                    cursor.index(),
                    count,
                    "completion cannot move lookup context"
                );
                for candidate_lane in 0..cursor.logical_lane_count() {
                    // The oracle scans raw descriptor rows, never successors.
                    let expected = (0..count).find(|&candidate| {
                        !completed[candidate]
                            && rows.local_step_lane(candidate) == Some(candidate_lane as u8)
                    });
                    assert_eq!(
                        cursor.step_index_at_lane(candidate_lane),
                        expected,
                        "order={order} completed={step} lane={candidate_lane}"
                    );
                    assert_eq!(
                        cursor.current_step_label_codes()[candidate_lane],
                        expected.map_or(CURRENT_STEP_UNLABELED_CODE, |next| {
                            u16::from(cursor.event_row_at(next).unwrap().atom().label)
                        })
                    );
                }
                let heads = cursor.lane_cursors().to_vec();
                let labels = cursor.current_step_label_codes().to_vec();
                cursor.advance_lane_to_relocatable_step(target);
                assert_eq!(cursor.lane_cursors(), heads);
                assert_eq!(cursor.current_step_label_codes(), labels);
            }
        });
    }
}

#[test]
fn offer_head_normalization_never_consumes_unknown_or_revived_choices() {
    let global = g::seq(
        g::route(
            g::send::<0, 1, Msg<71, ()>>(),
            g::send::<0, 1, Msg<72, ()>>(),
        ),
        g::send::<0, 1, Msg<73, ()>>(),
    );
    let rows = project::<1>(&global).role_image_ref();
    super::super::validated_metadata_tests::with_rows(rows, |cursor| {
        let lane = usize::from(rows.local_step_lane(0).unwrap());
        assert!((0..3).all(|step| rows.local_step_lane(step) == Some(lane as u8)));
        let heads = cursor.lane_cursors().to_vec();
        let labels = cursor.current_step_label_codes().to_vec();
        let done = cursor.completed_event_words().to_vec();
        let mutations = cursor.completion_mutations();
        assert_eq!(cursor.offer_lane_head(1, lane, &mut |_| None), Some(0));
        assert_eq!(cursor.offer_lane_head(1, lane, &mut |_| Some(1)), Some(1));
        assert_eq!(cursor.offer_lane_head(1, lane, &mut |_| Some(0)), Some(0));
        assert_eq!(cursor.offer_lane_head(1, lane, &mut |_| None), Some(0));
        assert_eq!(cursor.lane_cursors(), heads);
        assert_eq!(cursor.current_step_label_codes(), labels);
        assert_eq!(cursor.completed_event_words(), done);
        assert_eq!(cursor.completion_mutations(), mutations);
        let target = cursor
            .relocatable_resident_lane_step_at_index(0, lane)
            .unwrap();
        cursor.advance_lane_to_relocatable_step(target);
        assert_eq!(cursor.step_index_at_lane(lane), Some(1));
        assert_eq!(cursor.offer_lane_head(1, lane, &mut |_| Some(0)), Some(2));
        assert_eq!(cursor.offer_lane_head(1, lane, &mut |_| None), Some(1));
        assert_eq!(cursor.step_index_at_lane(lane), Some(1));
        assert!(
            cursor
                .offer_lane_head(1, cursor.logical_lane_count(), &mut |_| None)
                .is_none()
        );
    });
}

#[test]
fn offer_heads_keep_live_and_completed_visit_contexts_separate() {
    let global = g::seq(
        g::route(
            g::seq(
                g::send::<0, 1, Msg<201, ()>>(),
                g::send::<1, 0, Msg<202, ()>>(),
            ),
            g::send::<0, 1, Msg<203, ()>>(),
        )
        .roll(),
        g::route(
            g::seq(
                g::send::<0, 1, Msg<204, ()>>(),
                g::send::<1, 0, Msg<205, ()>>(),
            ),
            g::seq(
                g::send::<0, 1, Msg<206, ()>>(),
                g::send::<1, 0, Msg<207, ()>>(),
            ),
        )
        .roll(),
    );
    let rows = project::<1>(&global).role_image_ref();
    super::super::validated_metadata_tests::with_rows(rows, |cursor| {
        assert_eq!(rows.local_step_count(), 7);
        for index in 0..7 {
            let label = match cursor.action_at(index) {
                super::super::LocalAction::Send { label, .. }
                | super::super::LocalAction::Recv { label, .. } => label,
                super::super::LocalAction::Local { .. } | super::super::LocalAction::Terminate => {
                    panic!("expected a message row")
                }
            };
            assert_eq!(label, 201 + index as u8);
        }
        let scopes: std::vec::Vec<_> = (0..2)
            .map(|slot| {
                cursor
                    .machine()
                    .route_scope_rows_by_slot(slot)
                    .unwrap()
                    .scope()
            })
            .collect();
        let phase = [0, 0, 0, 1, 1, 1, 1];
        let arm = [0, 0, 1, 0, 0, 1, 1];
        let arm_rows = [[0b0000011, 0b0000100], [0b0011000, 0b1100000]];
        for completion in 0..128u32 {
            for index in 0..7 {
                cursor.write_completion_bit(index, completion & (1 << index) != 0);
            }
            for choices in 0..9usize {
                let selected_arms = [choices % 3, choices / 3].map(|value| match value {
                    0 => None,
                    1 => Some(0),
                    _ => Some(1),
                });
                let mut selected = |scope| {
                    selected_arms[scopes.iter().position(|&candidate| candidate == scope)?]
                };
                for head in 0..7 {
                    let lane = usize::from(rows.local_step_lane(head).unwrap());
                    let target = cursor
                        .relocatable_resident_lane_step_at_index(head, lane)
                        .unwrap();
                    cursor.set_lane_cursor_to_relocatable_step(target);
                    let expected = (head..7).find(|&step| {
                        completion & (1 << step) == 0
                            && rows.local_step_lane(step) == Some(lane as u8)
                            && selected_arms[phase[step]]
                                .is_none_or(|selected| arm[step] == selected)
                    });
                    let heads = cursor.lane_cursors().to_vec();
                    let done = cursor.completed_event_words().to_vec();
                    assert_eq!(
                        cursor.offer_lane_head(1, lane, &mut selected),
                        expected,
                        "completion={completion:x} choices={choices} head={head} lane={lane}"
                    );
                    let mut live = |scope| {
                        let phase = scopes.iter().position(|&candidate| candidate == scope)?;
                        let selected = selected_arms[phase]?;
                        let selected_rows = arm_rows[phase][usize::from(selected)];
                        (completion & selected_rows != selected_rows).then_some(selected)
                    };
                    let expected_live = (head..7).find(|&step| {
                        completion & (1 << step) == 0
                            && rows.local_step_lane(step) == Some(lane as u8)
                            && selected_arms[phase[step]].is_none_or(|selected| {
                                let selected_rows = arm_rows[phase[step]][usize::from(selected)];
                                arm[step] == selected || completion & selected_rows == selected_rows
                            })
                    });
                    assert_eq!(
                        cursor.offer_lane_head(1, lane, &mut live),
                        expected_live,
                        "preview completion={completion:x} choices={choices} head={head} lane={lane}"
                    );
                    assert_eq!(cursor.lane_cursors(), heads);
                    assert_eq!(cursor.completed_event_words(), done);
                }
            }
        }
    });
}

#[test]
fn lookup_context_changes_preserve_every_lane_head_label_and_completion() {
    let global = g::seq(
        g::send::<0, 1, Msg<11, ()>>(),
        g::seq(
            g::par(
                g::send::<0, 1, Msg<12, ()>>(),
                g::send::<0, 2, Msg<13, ()>>(),
            ),
            g::send::<0, 1, Msg<14, ()>>(),
        ),
    );
    let program = project::<0>(&global);
    super::super::validated_metadata_tests::with_rows(program.role_image_ref(), |cursor| {
        let heads = cursor.lane_cursors().to_vec();
        let labels = cursor.current_step_label_codes().to_vec();
        let completions = cursor.completed_event_words().to_vec();
        let mutations = cursor.completion_mutations();
        for index in (0..=cursor.local_steps_len()).rev() {
            cursor.set_index(index);
            assert_eq!(cursor.index(), index);
            assert_eq!(cursor.lane_cursors(), heads);
            assert_eq!(cursor.current_step_label_codes(), labels);
            assert_eq!(cursor.completed_event_words(), completions);
            assert_eq!(cursor.completion_mutations(), mutations);
        }
    });
}

#[test]
fn roll_reset_rewinds_lanes_whose_first_event_is_in_a_later_parallel_group() {
    let global = g::seq(
        g::seq(
            g::par(
                g::send::<0, 1, Msg<31, ()>>(),
                g::send::<0, 2, Msg<32, ()>>(),
            ),
            g::par(
                g::par(
                    g::send::<0, 1, Msg<33, ()>>(),
                    g::send::<0, 2, Msg<34, ()>>(),
                ),
                g::send::<0, 3, Msg<35, ()>>(),
            ),
        )
        .roll(),
        g::send::<0, 1, Msg<36, ()>>(),
    );
    let program = project::<0>(&global);
    super::super::validated_metadata_tests::with_rows(program.role_image_ref(), |cursor| {
        assert_eq!(cursor.local_steps_len(), 6);
        let (scope, body) = cursor.machine().roll_scope_row_by_slot(0).unwrap();
        assert_eq!((body.start(), body.end()), (0, 5));
        assert!((0..cursor.logical_lane_count()).any(|lane| {
            (0..2).all(|step| cursor.event_lane_at(step) != Some(lane as u8))
                && (2..5).any(|step| cursor.event_lane_at(step) == Some(lane as u8))
        }));
        for step in 0..cursor.local_steps_len() {
            let lane = usize::from(cursor.event_lane_at(step).unwrap());
            let target = cursor
                .relocatable_resident_lane_step_at_index(step, lane)
                .unwrap();
            cursor.advance_lane_to_relocatable_step(target);
        }
        assert!(
            (0..cursor.logical_lane_count()).all(|lane| cursor.step_index_at_lane(lane).is_none())
        );
        let reset = cursor.prepare_roll_reset(scope, None, &mut |_| None);
        cursor.clear_reentry_scope_events(reset);
        for step in 0..5 {
            assert!(!cursor.local_event_done(step));
        }
        assert!(
            cursor.local_event_done(5),
            "the sequential tail stays completed"
        );
        for lane in 0..cursor.logical_lane_count() {
            let first = (0..5).find(|&step| cursor.event_lane_at(step) == Some(lane as u8));
            assert_eq!(cursor.step_index_at_lane(lane), first);
            assert_eq!(
                cursor.current_step_label_codes()[lane],
                first.map_or(CURRENT_STEP_UNLABELED_CODE, |step| {
                    u16::from(cursor.event_row_at(step).unwrap().atom().label)
                })
            );
        }
    });
}

#[test]
fn accepted_old_continuation_preserves_the_fresh_body_head_and_label() {
    let global = g::seq(
        g::send::<0, 1, Msg<41, ()>>().roll(),
        g::send::<0, 1, Msg<42, ()>>(),
    );
    let program = project::<1>(&global);
    super::super::validated_metadata_tests::with_rows(program.role_image_ref(), |cursor| {
        assert_eq!(cursor.local_steps_len(), 2);
        let lane = usize::from(cursor.event_lane_at(0).unwrap());
        assert_eq!(cursor.event_lane_at(1), Some(lane as u8));
        let body = cursor
            .relocatable_resident_lane_step_at_index(0, lane)
            .unwrap();
        cursor.advance_lane_to_relocatable_step(body);
        let scope = cursor.machine().roll_scope_row_by_slot(0).unwrap().0;
        let reset = cursor.prepare_roll_reset(scope, None, &mut |_| None);
        cursor.clear_reentry_scope_events(reset);
        assert_eq!(cursor.step_index_at_lane(lane), Some(0));
        let head_label = cursor.current_step_label_codes()[lane];
        let continuation = cursor.try_recv_meta_at(1).unwrap();
        assert!(
            cursor
                .event_enabled(1, continuation.into(), &mut |_, _| None)
                .is_ok()
        );
        let target = cursor
            .relocatable_resident_lane_step_at_index(1, lane)
            .unwrap();
        cursor.set_index(1);
        cursor.advance_lane_to_relocatable_step(target);
        assert_eq!(cursor.step_index_at_lane(lane), Some(0));
        assert_eq!(cursor.current_step_label_codes()[lane], head_label);
        assert_eq!(cursor.index(), 1);
        assert!(!cursor.local_event_done(0));
        assert!(cursor.local_event_done(1));
    });
}

#[test]
fn pending_prefix_preserves_rejection_for_every_completion_and_route_choice() {
    let global = g::seq(
        g::par(
            g::seq(
                g::send::<0, 1, Msg<21, u8>>(),
                g::route(
                    g::send::<0, 1, Msg<22, u8>>(),
                    g::send::<0, 1, Msg<23, u8>>(),
                ),
            ),
            g::seq(
                g::send::<0, 2, Msg<24, u8>>(),
                g::send::<2, 0, Msg<25, u8>>(),
            ),
        )
        .roll(),
        g::send::<0, 1, Msg<26, u8>>(),
    );
    let program: RoleProgram<0> = project(&global);
    let descriptor = RoleDescriptorRef::from_resident(program.role_image_ref());
    let rows = descriptor.local_event_rows();
    let len = rows.local_step_count();
    assert!(
        len < 12,
        "fixture exhaustively enumerates completion subsets"
    );
    let mut state = MaybeUninit::<EventCursorState>::uninit();
    let mut cursor = MaybeUninit::<EventCursor>::uninit();
    let mut heads = [0u16; 256];
    let mut labels = [0u16; 256];
    let mut done = std::vec![0u32; rows.footprint().completion_bit_count().div_ceil(32)];
    // As in the lane-head differential fixture: disjoint unpublished storage
    // outlives every cursor access, including deliberate completion resets.
    unsafe {
        EventCursor::init_from_compiled(
            cursor.as_mut_ptr(),
            state.as_mut_ptr(),
            heads.as_mut_ptr(),
            labels.as_mut_ptr(),
            done.as_mut_ptr(),
            descriptor,
        );
    }
    let cursor = unsafe { cursor.assume_init_mut() };
    for completion in 0..1u32 << len {
        cursor.completed_event_words_mut()[0] = completion;
        for choice in [None, Some(0), Some(1)] {
            for step in 0..len {
                let target = RelocatableResidentLaneStep(ResidentLaneStep {
                    step_idx: step as u16,
                    lane: rows.local_step_lane(step).unwrap(),
                });
                let preview = cursor.machine().event_conflict_for_index(step);
                // Independent per-step specification, including completions
                // cleared on reentry and unchosen route arms in another lane.
                let expected = (0..step).all(|previous| {
                    completion & (1 << previous) != 0
                        || rows.local_step_lane(previous) != Some(target.0.lane)
                        || !cursor.event_conflict_row_allows_with_preview(
                            cursor.machine().event_conflict_for_index(previous),
                            preview,
                            &mut |_| choice,
                        )
                });
                assert_eq!(
                    cursor.event_lane_head_allows(target, preview, &mut |_| choice),
                    expected,
                    "completion={completion:08x} choice={choice:?} step={step}"
                );
            }
        }
    }
}

#[test]
fn excluded_nested_arms_match_each_step_admission_without_repeated_queries() {
    let global = g::par(
        g::seq(
            g::route(
                g::seq(
                    g::send::<0, 1, Msg<1, ()>>(),
                    g::route(g::send::<0, 1, Msg<2, ()>>(), g::send::<0, 1, Msg<3, ()>>()),
                ),
                g::seq(g::send::<0, 1, Msg<4, ()>>(), g::send::<0, 1, Msg<5, ()>>()),
            ),
            g::send::<0, 1, Msg<6, ()>>(),
        ),
        g::send::<0, 2, Msg<7, ()>>(),
    )
    .roll();
    let program = project::<0>(&global);
    let rows = program.role_image_ref();
    super::super::validated_metadata_tests::with_rows(rows, |cursor| {
        let len = rows.local_step_count();
        let routes = rows.footprint().route_scope_count;
        assert!(len < 12 && routes < 4);
        let mut saved = 0;
        for completion in 0..1u32 << len {
            cursor.completed_event_words_mut()[0] = completion;
            for choices in 0..3usize.pow(routes as u32) {
                let selected = |scope| {
                    let slot = cursor.route_scope_slot(scope).unwrap();
                    match choices / 3usize.pow(slot as u32) % 3 {
                        0 => None,
                        1 => Some(0),
                        _ => Some(1),
                    }
                };
                for step in 0..len {
                    let target = RelocatableResidentLaneStep(ResidentLaneStep {
                        step_idx: step as u16,
                        lane: rows.local_step_lane(step).unwrap(),
                    });
                    let preview = cursor.machine().event_conflict_for_index(step);
                    let mut reference_queries = 0;
                    let expected = (0..step).all(|previous| {
                        completion & (1 << previous) != 0
                            || rows.local_step_lane(previous) != Some(target.0.lane)
                            || !cursor.event_conflict_row_allows_with_preview(
                                cursor.machine().event_conflict_for_index(previous),
                                preview,
                                &mut |scope| {
                                    reference_queries += 1;
                                    selected(scope)
                                },
                            )
                    });
                    let mut queries = 0;
                    let actual = cursor.event_lane_head_allows(target, preview, &mut |scope| {
                        queries += 1;
                        selected(scope)
                    });
                    assert_eq!(actual, expected, "{completion:x}/{choices}/{step}");
                    assert!(queries <= reference_queries);
                    saved += reference_queries - queries;
                }
            }
        }
        assert!(saved > 0, "excluded arms must avoid per-event queries");
    });
}

fn compare_offer_head_to_raw_rows(rows: &'static RoleImageRef) {
    super::super::validated_metadata_tests::with_rows(rows, |cursor| {
        let raw = rows.lanes();
        let count = rows.local_step_count();
        let scopes: std::vec::Vec<_> = (0..rows.footprint().route_scope_count)
            .map(|slot| {
                cursor
                    .machine()
                    .route_scope_rows_by_slot(slot)
                    .unwrap()
                    .scope()
            })
            .collect();
        assert!(count <= 10 && scopes.len() <= 3);
        for completion in 0..1u32 << count {
            cursor.completed_event_words_mut()[0] = completion;
            for choices in 0..3usize.pow(scopes.len() as u32) {
                let mut selected = |scope| {
                    let slot = scopes.iter().position(|&candidate| candidate == scope)?;
                    match choices / 3usize.pow(slot as u32) % 3 {
                        0 => None,
                        1 => Some(0),
                        _ => Some(1),
                    }
                };
                let before = cursor.completed_event_words().to_vec();
                let heads = cursor.lane_cursors().to_vec();
                for lane in 0..cursor.logical_lane_count() {
                    for view in [0, 1, 2, 3, 254, 255] {
                        // Test every raw source facet separately, without the
                        // indexed lower bound or sparse arm summaries.
                        let expected = (0..count).find(|&step| {
                            let event = raw.local_step_event(step).unwrap();
                            let atom = rows.program().event_atom_at(usize::from(event.eff_index));
                            let logical_role = if event.direction() == RoleLocalDirection::Recv {
                                atom.to
                            } else {
                                atom.from
                            };
                            logical_role == view
                                && completion & (1 << step) == 0
                                && raw.local_step_lane(step, rows.footprint().logical_lane_count)
                                    == Some(lane as u8)
                                && cursor.event_actor_is_local(step, &mut selected) != Some(false)
                                && cursor.event_conflict_row_allows(
                                    raw.event_conflict_for_index(step),
                                    ScopeId::none(),
                                    None,
                                    &mut selected,
                                )
                        });
                        assert_eq!(
                            cursor.offer_lane_head(view, lane, &mut selected),
                            expected,
                            "actor={} view={view} completion={completion:x} choices={choices} lane={lane}",
                            rows.role()
                        );
                    }
                }
                assert_eq!(cursor.lane_cursors(), heads);
                assert_eq!(cursor.completed_event_words(), before);
            }
        }
    });
}

#[test]
fn excluded_arm_jumps_match_raw_rows_across_nested_sparse_lanes_and_views() {
    let global = g::seq(
        g::route(
            g::seq(
                g::send::<0, 1, Msg<81, ()>>(),
                g::route(
                    g::par(
                        g::send::<0, 1, Msg<82, ()>>(),
                        g::send::<0, 2, Msg<83, ()>>(),
                    ),
                    g::par(
                        g::send::<0, 1, Msg<84, ()>>(),
                        g::send::<0, 2, Msg<85, ()>>(),
                    ),
                ),
            ),
            g::par(
                g::send::<0, 1, Msg<86, ()>>(),
                g::send::<0, 2, Msg<87, ()>>(),
            ),
        ),
        g::send::<0, 1, Msg<88, ()>>(),
    );
    compare_offer_head_to_raw_rows(project::<0>(&global).role_image_ref());
    compare_offer_head_to_raw_rows(project::<1>(&global).role_image_ref());
    compare_offer_head_to_raw_rows(project::<2>(&global).role_image_ref());

    type Loan<'r> = Msg<88, Endpoint<'r, 2>>;
    let delegated = g::seq(
        g::send::<0, 1, Loan<'_>>(),
        g::route(
            g::seq(
                g::send::<2, 1, Msg<89, ()>>(),
                g::send::<1, 2, Msg<90, ()>>(),
            ),
            g::seq(
                g::send::<2, 1, Msg<91, ()>>(),
                g::send::<1, 2, Msg<92, ()>>(),
            ),
        ),
    );
    let rows = project::<1>(&delegated).role_image_ref();
    assert!((0..rows.local_step_count()).any(|step| {
        rows.local_step_logical_role(step) == 2
            && (0..rows.local_step_count()).any(|other| {
                rows.local_step_logical_role(other) == 1
                    && rows.local_step_lane(step) == rows.local_step_lane(other)
            })
    }));
    compare_offer_head_to_raw_rows(rows);
}

#[test]
fn alternative_co_resident_views_each_have_their_own_potential_lane_head() {
    type Loan<'r> = Msg<112, Endpoint<'r, 3>>;
    let global = g::seq(
        g::route(
            g::send::<0, 1, Msg<110, ()>>(),
            g::send::<0, 3, Msg<111, ()>>(),
        ),
        g::seq(g::send::<1, 2, Loan<'_>>(), g::send::<0, 3, Msg<113, ()>>()),
    );
    let rows = project::<1>(&global).role_image_ref();
    super::super::validated_metadata_tests::with_rows(rows, |cursor| {
        let left = (0..rows.local_step_count())
            .find(|&step| {
                cursor
                    .try_recv_meta_at(step)
                    .is_some_and(|meta| meta.label == 110)
            })
            .unwrap();
        let right = (0..rows.local_step_count())
            .find(|&step| {
                cursor
                    .try_recv_meta_at(step)
                    .is_some_and(|meta| meta.label == 111)
            })
            .unwrap();
        let lane = usize::from(rows.local_step_lane(left).unwrap());
        assert_eq!(rows.local_step_lane(right), Some(lane as u8));
        assert_eq!(cursor.index_for_lane_step(lane), Some(left));
        let heads = cursor.lane_cursors().to_vec();
        let done = cursor.completed_event_words().to_vec();
        assert_eq!(cursor.offer_lane_head(1, lane, &mut |_| None), Some(left));
        assert_eq!(cursor.offer_lane_head(3, lane, &mut |_| None), Some(right));
        let right_meta = cursor.try_recv_meta_at(right).unwrap();
        assert!(
            cursor
                .event_enabled(right, right_meta.into(), &mut |_, _| None)
                .is_ok()
        );
        assert_eq!(cursor.offer_lane_head(3, lane, &mut |_| Some(0)), None);
        let loan = cursor.offer_lane_head(1, lane, &mut |_| Some(1)).unwrap();
        let loan_meta = cursor.try_send_meta_at(loan).unwrap();
        // A candidate can pass another view's prefix; it still cannot commit
        // the transfer before that view consumes its incoming choice.
        assert!(
            cursor
                .event_enabled(loan, loan_meta.into(), &mut |_, _| Some(1))
                .is_err()
        );
        assert_eq!(cursor.lane_cursors(), heads);
        assert_eq!(cursor.completed_event_words(), done);
    });
    compare_offer_head_to_raw_rows(rows);
}

fn compare_view_lane_lower_bounds(rows: &RoleImageRef) {
    let raw = rows.lanes();
    let count = rows.local_step_count();
    let view = |step| {
        let event = raw.local_step_event(step).unwrap();
        let atom = rows.program().event_atom_at(usize::from(event.eff_index));
        if event.direction() == RoleLocalDirection::Recv {
            atom.to
        } else {
            atom.from
        }
    };
    for requested_view in [0, 1, 2, 3, 254, 255] {
        for lane in (0..rows.footprint().logical_lane_count).chain([254, 255]) {
            for start in 0..=count + 1 {
                let expected = (rows.footprint().route_scope_count != 0)
                    .then(|| {
                        (start..count).find(|&step| {
                            view(step) == requested_view
                                && raw.local_step_lane(step, rows.footprint().logical_lane_count)
                                    == Some(lane as u8)
                        })
                    })
                    .flatten();
                assert_eq!(
                    rows.view_lane_step_at_or_after(requested_view, lane as u8, start),
                    expected,
                    "view={requested_view} lane={lane} start={start}"
                );
            }
        }
    }
}

#[test]
fn retained_view_lane_lower_bounds_match_raw_source_facets_and_omitted_indices() {
    type Loan<'r> = Msg<93, Endpoint<'r, 2>>;
    let global = g::seq(
        g::send::<0, 1, Loan<'_>>(),
        g::par(
            g::route(
                g::seq(
                    g::send::<2, 1, Msg<94, ()>>(),
                    g::send::<1, 2, Msg<95, ()>>(),
                ),
                g::seq(
                    g::send::<2, 1, Msg<96, ()>>(),
                    g::send::<1, 2, Msg<97, ()>>(),
                ),
            ),
            g::send::<1, 3, Msg<98, ()>>(),
        ),
    );
    compare_view_lane_lower_bounds(project::<1>(&global).role_image_ref());
    compare_view_lane_lower_bounds(project::<3>(&global).role_image_ref());
    let no_routes = g::seq(
        g::send::<0, 1, Msg<99, ()>>(),
        g::send::<1, 0, Msg<100, ()>>(),
    );
    compare_view_lane_lower_bounds(project::<0>(&no_routes).role_image_ref());
}

#[test]
fn nested_retained_anchors_match_raw_source_rolls_for_all_progress_and_choices() {
    let global = g::seq(
        g::seq(
            g::send::<0, 1, Msg<101, ()>>().roll(),
            g::route(
                g::send::<0, 1, Msg<102, ()>>(),
                g::send::<0, 1, Msg<103, ()>>(),
            ),
        )
        .roll(),
        g::route(
            g::send::<0, 1, Msg<104, ()>>(),
            g::send::<0, 1, Msg<105, ()>>(),
        ),
    );
    let rows = project::<1>(&global).role_image_ref();
    super::super::validated_metadata_tests::with_rows(rows, |cursor| {
        let raw = rows.lanes();
        let count = rows.local_step_count();
        let lane = rows.local_step_lane(0).unwrap();
        assert_eq!(count, 5);
        assert!((0..count).all(|step| rows.local_step_lane(step) == Some(lane)));
        let source = rows.program();
        let scopes: std::vec::Vec<_> = (0..rows.footprint().route_scope_count)
            .map(|slot| {
                cursor
                    .machine()
                    .route_scope_rows_by_slot(slot)
                    .unwrap()
                    .scope()
            })
            .collect();
        let mut rolls: std::vec::Vec<_> = (0..source.source_scope_marker_count())
            .filter_map(|marker| source.source_roll_at(marker))
            .collect();
        rolls.reverse();
        assert_eq!(rolls.len(), 2);
        for completion in 0..1u32 << count {
            for retained in 0..4 {
                for step in 0..count {
                    cursor.write_completion_bit(step, completion & (1 << step) != 0);
                }
                for (ordinal, &(scope, _, _)) in rolls.iter().enumerate() {
                    let slot = cursor
                        .machine()
                        .event_program()
                        .roll_scope_slot(scope)
                        .unwrap();
                    cursor.write_completion_bit(count + slot, retained & (1 << ordinal) != 0);
                }
                for choices in 0..9usize {
                    let mut selected = |scope| {
                        let slot = scopes.iter().position(|&candidate| candidate == scope)?;
                        match choices / 3usize.pow(slot as u32) % 3 {
                            0 => None,
                            1 => Some(0),
                            _ => Some(1),
                        }
                    };
                    for head in 0..count {
                        let effect = usize::from(raw.local_step_event(head).unwrap().eff_index);
                        let expected: std::vec::Vec<_> = rolls
                            .iter()
                            .enumerate()
                            .filter(|(ordinal, (_, start, end))| {
                                retained & (1 << *ordinal) != 0 && *start <= effect && effect < *end
                            })
                            .filter_map(|(_, (_, _, end))| {
                                (0..count).find(|&step| {
                                    usize::from(raw.local_step_event(step).unwrap().eff_index)
                                        >= *end
                                        && completion & (1 << step) == 0
                                        && cursor.event_conflict_row_allows(
                                            raw.event_conflict_for_index(step),
                                            ScopeId::none(),
                                            None,
                                            &mut selected,
                                        )
                                })
                            })
                            .collect();
                        let before = cursor.completed_event_words().to_vec();
                        assert_eq!(
                            cursor
                                .retained_offer_lane_candidates(head, 1, lane, &mut selected)
                                .collect::<std::vec::Vec<_>>(),
                            expected,
                            "completion={completion:x} retained={retained} choices={choices} head={head}"
                        );
                        assert_eq!(cursor.completed_event_words(), before);
                    }
                }
            }
        }
    });
}

#[test]
fn a_pending_send_stops_the_retained_receive_anchor() {
    let global = g::seq(
        g::send::<0, 1, Msg<106, ()>>().roll(),
        g::seq(
            g::route(
                g::send::<1, 0, Msg<107, ()>>(),
                g::send::<1, 0, Msg<108, ()>>(),
            ),
            g::send::<0, 1, Msg<109, ()>>(),
        ),
    );
    let rows = project::<1>(&global).role_image_ref();
    super::super::validated_metadata_tests::with_rows(rows, |cursor| {
        assert_eq!(rows.local_step_count(), 4);
        let lane = rows.local_step_lane(0).unwrap();
        cursor.write_completion_bit(4, true);
        let candidates = |cursor: &EventCursor| {
            cursor
                .retained_offer_lane_candidates(0, 1, lane, &mut |_| None)
                .collect::<std::vec::Vec<_>>()
        };
        assert!(candidates(cursor).is_empty());
        cursor.mark_local_event_done(1);
        assert!(candidates(cursor).is_empty());
        cursor.mark_local_event_done(2);
        assert_eq!(candidates(cursor), [3]);
        assert!(!cursor.local_event_done(0));
    });
}
