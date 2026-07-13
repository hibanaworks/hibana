use super::*;

#[test]
fn lane_resident_route_rows_do_not_restore_full_domain_copies() {
    let packed_route_lane_rows = MAX_ROUTE_ARM_LANE_ROWS
        * core::mem::size_of::<PackedRouteArmRow>()
        + MAX_ROUTE_SCOPE_LANE_ROWS * core::mem::size_of::<PackedLaneRange>();
    let full_domain_route_lane_rows = (MAX_ROUTE_ARM_LANE_ROWS + MAX_ROUTE_SCOPE_LANE_ROWS)
        * LANE_SET_VIEW_WORDS
        * core::mem::size_of::<LaneWord>();

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
        "route arm row should keep event range, child delta, and lane-step range in one compact scalar row"
    );
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
fn route_arm_lane_steps_keep_multi_lane_arms_sparse() {
    let program: RoleProgram<0> = project(&sparse_multi_lane_route_program());
    with_role_descriptor(&program, |descriptor| {
        let rows = descriptor.local_event_rows();
        let columns = rows.lanes().columns;
        assert_eq!(columns.route_arms.len, 2);
        assert!(
            columns.route_arm_lane_step_rows.len > columns.route_arms.len,
            "route arms with multiple actual lanes must keep more than one sparse row per arm"
        );
        assert!(
            (columns.route_arm_lane_step_rows.len as usize)
                < columns.route_arms.len as usize * descriptor.logical_lane_count()
        );
    });
}

#[test]
fn nested_route_lane_steps_are_not_capped_by_local_step_count() {
    let program: RoleProgram<0> = project(&final_form_protocol!(triple_nested_route));
    with_role_descriptor(&program, |descriptor| {
        let rows = descriptor.local_event_rows();
        let columns = rows.lanes().columns;
        assert_eq!(columns.route_arms.len, 6);
        assert!(
            columns.route_arm_lane_step_rows.len as usize > columns.events.len as usize,
            "nested route arms can duplicate lane-step summaries across ancestor arms"
        );
        assert!(
            (columns.route_arm_lane_step_rows.len as usize)
                < columns.route_arms.len as usize * descriptor.logical_lane_count(),
            "nested route sparse rows must still avoid route_arm * logical_lane_count scaling"
        );
    });
}
