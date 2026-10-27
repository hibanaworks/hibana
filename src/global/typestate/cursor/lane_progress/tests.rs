use super::super::CURRENT_STEP_UNLABELED_CODE;
use super::*;
use crate::{
    g::{self, Msg},
    global::{
        compiled::images::RoleDescriptorRef,
        role_program::{RoleProgram, project},
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
            assert_eq!(
                cursor.advance_lane_to_relocatable_step(target),
                CursorRefresh::Lane(lane as u8)
            );
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
        assert_eq!(
            cursor.set_lane_cursor_to_relocatable_step(target),
            CursorRefresh::Lane(lane as u8)
        );
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
        assert_eq!(
            cursor.advance_lane_to_relocatable_step(target),
            CursorRefresh::Lane(lane as u8)
        );
        assert_eq!(cursor.step_index_at_lane(lane), Some(0));
        assert_eq!(cursor.current_step_label_codes()[lane], head_label);
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
