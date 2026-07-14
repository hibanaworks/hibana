use super::*;

#[test]
fn projection_accepts_role_16_and_full_u8_role_domain() {
    let program = g::seq(
        g::send::<16, 17, Msg<210, u8>>(),
        g::send::<254, 255, Msg<211, u8>>(),
    );
    let role16: RoleProgram<16> = project(&program);
    let role255: RoleProgram<255> = project(&program);

    assert_eq!(role16.role_image_ref().program.role_count(), 256);
    assert_eq!(role255.role_image_ref().program.role_count(), 256);
    assert!(
        role16
            .role_image_ref()
            .program
            .same_image(role255.role_image_ref().program)
    );
    assert_eq!(role16.role_image_ref().role, 16);
    assert_eq!(role255.role_image_ref().role, u8::MAX);
}

#[test]
fn high_role_parallel_projection_uses_disjoint_derived_lanes() {
    let program = g::par(
        g::send::<16, 17, Msg<212, u8>>(),
        g::send::<254, 255, Msg<213, u8>>(),
    );
    let role17: RoleProgram<17> = project(&program);
    let role255: RoleProgram<255> = project(&program);

    assert_eq!(role17.role_image_ref().program.role_count(), 256);
    assert_eq!(role255.role_image_ref().program.role_count(), 256);
    assert_eq!(
        role17.role_image_ref().facts.footprint().active_lane_count,
        1
    );
    assert_eq!(
        role255.role_image_ref().facts.footprint().active_lane_count,
        1
    );
}

#[test]
fn high_role_route_participants_are_canonical_sorted_lists() {
    let program = g::route(
        g::seq(
            g::send::<255, 16, Msg<214, u8>>(),
            g::send::<255, 254, Msg<215, u8>>(),
        ),
        g::seq(
            g::send::<255, 254, Msg<216, u8>>(),
            g::send::<255, 16, Msg<217, u8>>(),
        ),
    );
    let controller: RoleProgram<255> = project(&program);
    let descriptor = controller.role_image_ref().program;
    let scope = descriptor
        .route_resolver_scope_at_row(0)
        .expect("high-role route scope");

    assert_eq!(descriptor.role_count(), 256);
    assert_eq!(descriptor.route_controller_role(scope), u8::MAX);
    assert_eq!(descriptor.route_participant_count(scope, 0), 3);
    assert_eq!(descriptor.route_participant_at(scope, 0, 0), Some(16));
    assert_eq!(descriptor.route_participant_at(scope, 0, 1), Some(254));
    assert_eq!(descriptor.route_participant_at(scope, 0, 2), Some(255));
    assert_eq!(descriptor.route_participant_count(scope, 1), 3);
    assert_eq!(descriptor.route_participant_at(scope, 1, 0), Some(16));
    assert_eq!(descriptor.route_participant_at(scope, 1, 1), Some(254));
    assert_eq!(descriptor.route_participant_at(scope, 1, 2), Some(255));
}

#[test]
fn high_role_roll_projection_keeps_role_identity_without_runtime_epoch() {
    let program = g::send::<255, 254, Msg<218, u8>>().roll();
    let sender: RoleProgram<255> = project(&program);
    let receiver: RoleProgram<254> = project(&program);

    assert_eq!(sender.role_image_ref().program.role_count(), 256);
    assert_eq!(receiver.role_image_ref().program.role_count(), 256);
    assert_eq!(sender.role_image_ref().role, u8::MAX);
    assert_eq!(receiver.role_image_ref().role, 254);
}
