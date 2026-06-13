use super::*;

#[macro_use]
#[path = "final_form_protocol_matrix.rs"]
mod final_form_protocol_matrix;

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
            "test fixture must keep route arm lane on a high logical lane"
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
