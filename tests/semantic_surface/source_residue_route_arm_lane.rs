use super::common::*;

fn cursor_scope_route_source() -> String {
    let mut source = read("src/global/typestate/cursor/scope_route.rs");
    source.push_str(&read(
        "src/global/typestate/cursor/scope_route/event_flow.rs",
    ));
    source.push_str(&read(
        "src/global/typestate/cursor/scope_route/navigation.rs",
    ));
    source.push_str(&read(
        "src/global/typestate/cursor/scope_route/row_completion.rs",
    ));
    source
}

#[test]
fn route_arm_lane_first_last_use_resident_columns() {
    let cursor = cursor_scope_route_source();
    let first = cursor
        .split("fn route_arm_lane_first_step_inner")
        .nth(1)
        .and_then(|tail| tail.split("fn route_arm_lane_last_eff_inner").next())
        .expect("route arm lane first-step implementation must stay present");
    let last = cursor
        .split("fn route_arm_lane_last_eff_inner")
        .nth(1)
        .and_then(|tail| tail.split("fn controller_arm_entry_for_label_inner").next())
        .expect("route arm lane last-eff implementation must stay present");

    assert!(
        !cursor.contains("event_route_arm_for_scope("),
        "route arm membership must not reintroduce per-event conflict walks for first/last lookup"
    );
    assert!(
        first.contains("route_arm_lane_first_step_by_slot") && !first.contains("local_steps_len()"),
        "route arm lane first step must read the resident first-step column, not scan local steps"
    );
    assert!(
        last.contains("route_arm_lane_last_step_by_slot") && !last.contains("local_steps_len()"),
        "route arm lane last step must read the resident last-step column, not scan local steps"
    );
}
