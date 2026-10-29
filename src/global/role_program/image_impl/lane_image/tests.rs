use super::*;
use crate::{
    Endpoint, g,
    global::role_program::{RoleImageRef, project},
};

fn compare_event_facts(rows: &RoleImageRef) {
    let image = rows.lanes();
    for step in 0..rows.local_step_count() {
        let full = image.local_step_event(step).unwrap();
        let dependency = if full.dependency_row == u16::MAX {
            None
        } else {
            Some(
                image
                    .packed_dependency_row(full.dependency_row as usize)
                    .to_dependency(rows.local_step_count())
                    .unwrap(),
            )
        };
        assert_eq!(image.dependency_for_index(step), dependency);
        let conflict = if full.conflict_row == u16::MAX {
            PackedEventConflict::none()
        } else {
            PackedEventConflict::from_raw(
                image
                    .read_u16(
                        image.columns.conflicts,
                        full.conflict_row as usize,
                        ROLE_IMAGE_CONFLICT_STRIDE,
                    )
                    .unwrap(),
            )
        };
        assert_eq!(image.event_conflict_for_index(step).raw(), conflict.raw());
    }
}

#[test]
fn direct_event_facts_match_full_rows_across_owned_parallel_choices_and_repetition() {
    type Loan = g::Msg<1, Endpoint<'static, 10>>;
    type Read = g::Msg<3, u32>;
    type Value = g::Msg<4, u32>;
    let global = g::seq(
        g::send::<0, 1, Loan>(),
        g::seq(
            g::par(
                g::seq(g::send::<10, 2, Read>(), g::send::<2, 10, Value>()).roll(),
                g::route(g::send::<3, 4, Read>(), g::send::<3, 4, Value>()),
            ),
            g::send::<1, 0, Loan>(),
        ),
    );
    compare_event_facts(project::<0>(&global).role_image_ref());
    compare_event_facts(project::<1>(&global).role_image_ref());
    compare_event_facts(project::<2>(&global).role_image_ref());
    compare_event_facts(project::<3>(&global).role_image_ref());
    compare_event_facts(project::<4>(&global).role_image_ref());
    compare_event_facts(project::<10>(&global).role_image_ref());
}

#[test]
fn cumulative_arm_spans_match_prefix_sums_including_empty_arms() {
    const ARMS: usize = 256;
    const BYTES: usize =
        ARMS * (ROLE_IMAGE_ROUTE_ARM_STRIDE + ROLE_IMAGE_ROUTE_ARM_LANE_STEP_STRIDE);
    static DATA: ([u8; BYTES], [usize; ARMS], usize) = {
        let mut bytes = [0; BYTES];
        let mut ends = [0; ARMS];
        let mut end = 0;
        let mut row = 0;
        while row < ARMS {
            let length = if row % 3 == 0 { 0 } else { 1 };
            end += length;
            ends[row] = end;
            let offset = row * ROLE_IMAGE_ROUTE_ARM_STRIDE;
            bytes[offset] = length as u8;
            bytes[offset + 4] = u8::MAX;
            bytes[offset + 5] = u8::MAX;
            bytes[offset + 6] = end as u8;
            bytes[offset + 7] = (end >> 8) as u8;
            row += 1;
        }
        (bytes, ends, end)
    };
    let global = g::route(
        g::send::<0, 1, g::Msg<0, ()>>(),
        g::send::<0, 1, g::Msg<1, ()>>(),
    );
    let mut columns = *project::<0>(&global).role_image_ref().columns();
    columns.events = ColumnRange::new(0, 1, ROLE_IMAGE_EVENT_STRIDE);
    columns.route_arms = ColumnRange::new(0, ARMS, ROLE_IMAGE_ROUTE_ARM_STRIDE);
    columns.route_arm_lane_step_rows = ColumnRange::new(
        ARMS * ROLE_IMAGE_ROUTE_ARM_STRIDE,
        DATA.2,
        ROLE_IMAGE_ROUTE_ARM_LANE_STEP_STRIDE,
    );
    let image = RoleLaneImage::new(&columns, BlobPtr::from_array(&DATA.0, BYTES));
    let mut start = 0;
    for (row, end) in DATA.1.into_iter().enumerate() {
        let span = image.route_arm_lane_step_range(row);
        assert_eq!((span.start(), span.end()), (start, end));
        assert_eq!(image.route_arm_row(row).event_row().len(), end - start);
        start = end;
    }
}
