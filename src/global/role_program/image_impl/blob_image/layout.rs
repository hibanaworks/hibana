use super::super::super::{RoleImageColumns, RuntimeRoleFacts};

pub(super) const fn validate_role_image_layout<const N: usize>(
    columns: RoleImageColumns,
    facts: RuntimeRoleFacts,
) {
    let footprint = facts.footprint();
    let local_len = footprint.local_step_count;
    let route_scope_len = footprint.route_scope_count;
    let route_arm_len = route_scope_len * 2;
    if columns.blob_len() > N
        || columns.events.len as usize != local_len
        || columns.lanes.len as usize != local_len
        || columns.route_scopes.len as usize != route_scope_len
        || columns.route_scope_conflicts.len as usize != route_scope_len
        || columns.route_arms.len as usize != route_arm_len
        || columns.route_arm_lane_rows.len as usize != route_arm_len
        || columns.route_offer_lane_rows.len as usize != route_scope_len
        || columns.route_commit_ranges.len as usize != route_arm_len
    {
        panic!("role image");
    }
}
