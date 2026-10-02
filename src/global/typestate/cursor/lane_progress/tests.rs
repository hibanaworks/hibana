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
    let mut done = std::vec![0u32; rows.local_step_count().div_ceil(32)];
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
    let mut row = 0;
    while cursor.resident_row_bounds(row).is_some() {
        cursor.select_resident_row_for_lane(row, 0);
        for lane in 0..cursor.logical_lane_count() {
            assert_eq!(
                cursor.step_index_at_lane(lane),
                rows.reference_resident_lane_step_at(row, lane, 0)
                    .map(usize::from)
            );
        }
        for lane in 0..cursor.logical_lane_count() {
            let mut ordinal = 0;
            while let Some(step) = rows.reference_resident_lane_step_at(row, lane, ordinal) {
                assert_eq!(cursor.step_index_at_lane(lane), Some(usize::from(step)));
                let previous = cursor.lane_cursors().to_vec();
                let target = cursor
                    .relocatable_resident_lane_step_at_index(usize::from(step), lane)
                    .expect("matching lane identity");
                cursor.advance_lane_to_relocatable_step(target);
                assert!(cursor.relocatable_step_done(target));
                ordinal += 1;
                assert_eq!(
                    cursor.step_index_at_lane(lane),
                    rows.reference_resident_lane_step_at(row, lane, ordinal)
                        .map(usize::from)
                );
                for (other, before) in previous.iter().enumerate() {
                    if other != lane {
                        assert_eq!(cursor.lane_cursors()[other], *before);
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
                                .relocatable_resident_lane_step_at_index(usize::from(step), foreign)
                                .is_err()
                        );
                    }
                }
            }
        }
        row += 1;
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
        cursor.set_lane_cursor_to_relocatable_step(target);
        assert_eq!(cursor.step_index_at_lane(lane), Some(step));
        assert_eq!(cursor.node_index_for_relocatable_step(target), Some(step));
    }
}

#[test]
fn direct_lane_heads_match_ordinal_oracle_across_sparse_rows_and_relocation() {
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
fn direct_lane_heads_match_ordinal_oracle_for_routes_and_roll() {
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
