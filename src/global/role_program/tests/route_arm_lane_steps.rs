use super::*;

type SparseMultiLaneLeft = g::Seq<
    g::Send<0, 1, Msg<109, ()>>,
    g::Par<g::Send<0, 2, Msg<110, ()>>, g::Send<0, 3, Msg<111, ()>>>,
>;
type SparseMultiLaneRight = g::Seq<
    g::Send<0, 1, Msg<112, ()>>,
    g::Par<g::Send<0, 2, Msg<113, ()>>, g::Send<0, 3, Msg<114, ()>>>,
>;
type SparseMultiLaneRoute = g::Route<SparseMultiLaneLeft, SparseMultiLaneRight>;

fn sparse_multi_lane_route_program() -> Program<SparseMultiLaneRoute> {
    g::route(
        g::seq(
            g::send::<0, 1, Msg<109, ()>>(),
            g::par(
                g::send::<0, 2, Msg<110, ()>>(),
                g::send::<0, 3, Msg<111, ()>>(),
            ),
        ),
        g::seq(
            g::send::<0, 1, Msg<112, ()>>(),
            g::par(
                g::send::<0, 2, Msg<113, ()>>(),
                g::send::<0, 3, Msg<114, ()>>(),
            ),
        ),
    )
}

#[test]
fn lane_resident_route_rows_do_not_restore_full_domain_copies() {
    let packed_route_lane_rows =
        2 * core::mem::size_of::<PackedRouteArmRow>() + core::mem::size_of::<PackedLaneRange>();
    let full_domain_route_lane_rows = 3 * LANE_SET_VIEW_WORDS * core::mem::size_of::<LaneWord>();

    assert!(
        packed_route_lane_rows < full_domain_route_lane_rows,
        "route lane rows must stay packed and must not restore full-domain lane-set copies: current={} full_domain={}",
        packed_route_lane_rows,
        full_domain_route_lane_rows
    );
    assert!(
        core::mem::size_of::<RouteArmLaneStepRow>()
            < LANE_SET_VIEW_WORDS * core::mem::size_of::<LaneWord>(),
        "one sparse first/last row must stay smaller than a full-domain lane-set row"
    );
}

#[test]
fn route_arm_row_keeps_exact_ranges_in_compact_scalar_limbs() {
    let separate_exact_range_columns =
        (core::mem::size_of::<PackedLaneRange>() * 2) + core::mem::size_of::<u8>();
    assert_eq!(
        core::mem::size_of::<PackedRouteArmRow>(),
        ROLE_IMAGE_ROUTE_ARM_STRIDE
    );
    assert!(
        ROLE_IMAGE_ROUTE_ARM_STRIDE < separate_exact_range_columns,
        "route arm row should keep the event range, absolute child slot, and lane-step length in one compact scalar row"
    );
}

#[test]
fn route_arm_row_crosses_the_former_twelve_bit_boundary() {
    for (start, len) in [(4094, 1), (4095, 1), (4096, 1), (u16::MAX as usize - 1, 1)] {
        let event_row = PackedLaneRange::new(start, len);
        let lane_step_row = PackedLaneRange::new(7, 1);
        let packed = PackedRouteArmRow::new(event_row, Some(3), lane_step_row);

        assert_eq!(packed.event_row().start(), start);
        assert_eq!(packed.event_row().len(), len);
        assert_eq!(packed.lane_step_len(), 1);
        assert_eq!(packed.child_slot(), Some(3));
    }
}

#[test]
fn route_arm_row_encodes_the_full_lane_domain_without_growing() {
    let packed = PackedRouteArmRow::new(
        PackedLaneRange::new(0, 256),
        None,
        PackedLaneRange::new(0, LANE_DOMAIN_SIZE),
    );

    assert_eq!(packed.lane_step_len(), LANE_DOMAIN_SIZE);
    assert_eq!(core::mem::size_of_val(&packed), ROLE_IMAGE_ROUTE_ARM_STRIDE);
}

#[test]
fn route_arm_child_slot_crosses_the_former_u8_delta_boundary() {
    for child_slot in [256usize, 4096, u16::MAX as usize - 1] {
        let packed = PackedRouteArmRow::new(
            PackedLaneRange::new(0, 1),
            Some(child_slot),
            PackedLaneRange::new(0, 1),
        );
        assert_eq!(packed.child_slot(), Some(child_slot as u16));
        assert_eq!(core::mem::size_of_val(&packed), ROLE_IMAGE_ROUTE_ARM_STRIDE);
    }
}

#[test]
fn route_arm_lane_steps_are_sparse_over_actual_lanes_not_logical_lanes() {
    let program: RoleProgram<0> = project(&sparse_route_high_lane_program());
    with_role_descriptor(&program, |descriptor| {
        let rows = descriptor.local_event_rows();
        let lanes = rows.lanes();
        let columns = lanes.columns;
        let logical_lane_count = descriptor.logical_lane_count();
        assert!(
            logical_lane_count >= 64,
            "test support must keep route arm lane on a high logical lane"
        );
        assert_eq!(columns.route_arms.len, 8);
        assert_eq!(columns.route_arm_lane_step_rows.len, 8);
        assert!(columns.route_arm_lane_step_rows.len < columns.route_arms.len * 64);
        let dense_first_last_bytes =
            columns.route_arms.len as usize * logical_lane_count * 2 * core::mem::size_of::<u16>();
        let role_blob_len = rows.columns.blob_len();
        assert!(
            role_blob_len < dense_first_last_bytes,
            "role blob must not scale as route_arm_len * logical_lane_count: blob={} dense={}",
            role_blob_len,
            dense_first_last_bytes
        );
        let mut route_arm = 0usize;
        while route_arm < columns.route_arms.len as usize {
            let slot = route_arm / 2;
            let arm = (route_arm - slot * 2) as u8;
            assert_eq!(
                rows.route_arm_lane_first_step_by_slot(slot, arm, 63),
                rows.route_arm_lane_last_step_by_slot(slot, arm, 63)
            );
            assert!(
                rows.route_arm_lane_first_step_by_slot(slot, arm, 0)
                    .is_none(),
                "absent sparse row must mean this lane has no selected arm step"
            );
            route_arm += 1;
        }
    });
}

#[test]
fn route_arm_lane_steps_store_one_row_per_actual_arm_lane_relation() {
    let program: RoleProgram<0> = project(&sparse_multi_lane_route_program());
    with_role_descriptor(&program, |descriptor| {
        let rows = descriptor.local_event_rows();
        let columns = rows.lanes().columns;
        assert_eq!(columns.route_arms.len, 2);
        assert_eq!(descriptor.logical_lane_count(), 2);
        assert_eq!(columns.route_arm_lane_step_rows.len, 4);
        for arm in 0..=1 {
            for lane in 0..=1 {
                assert!(
                    rows.route_arm_lane_first_step_by_slot(0, arm, lane)
                        .is_some(),
                    "each actually used arm/lane pair must own exactly one relation row"
                );
            }
        }
    });
}

#[test]
fn nested_route_lane_steps_are_relation_counted_not_event_counted() {
    let program: RoleProgram<0> = project(&final_form_protocol!(triple_nested_route));
    with_role_descriptor(&program, |descriptor| {
        let rows = descriptor.local_event_rows();
        let columns = rows.lanes().columns;
        assert_eq!(columns.route_arms.len, 6);
        assert!(
            columns.route_arm_lane_step_rows.len as usize > columns.events.len as usize,
            "nested route arms can duplicate lane-step summaries across ancestor arms"
        );
        assert_eq!(descriptor.logical_lane_count(), 1);
        assert_eq!(columns.route_arm_lane_step_rows.len, columns.route_arms.len);
    });
}
