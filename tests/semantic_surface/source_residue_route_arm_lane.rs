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
        "route arm membership must not contain per-event conflict walks for first/last lookup"
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

#[test]
fn compact_bucket_backing_stays_byte_only_and_program_ref_shared() {
    let program_blob = read("src/global/compiled/images/image/blob_storage.rs");
    let role_types = read("src/global/role_program/image_types.rs");
    let role_projection = read("src/g/role_projection.rs");
    let per_role_program_ref = concat!("RoleProjection::<ROLE, Steps>::", "PROGRAM_REF");

    let program_bytes = program_blob
        .split("pub(crate) struct ProgramImageBytes<const N: usize> {")
        .nth(1)
        .and_then(|tail| tail.split("}").next())
        .expect("program byte bucket owner");
    assert!(
        program_bytes.contains("bytes: [u8; N],")
            && !program_bytes.contains("facts")
            && !program_bytes.contains("columns")
            && !program_bytes.contains("len"),
        "ProgramImageBytes must own only packed bytes; facts and columns live in CompiledProgramRef"
    );

    let role_bytes = role_types
        .split("pub(crate) struct RoleImageBytes<const N: usize> {")
        .nth(1)
        .and_then(|tail| tail.split("}").next())
        .expect("role byte bucket owner");
    assert!(
        role_bytes.contains("bytes: [u8; N],")
            && !role_bytes.contains("columns")
            && !role_bytes.contains("len")
            && !role_bytes.contains("active_lane_row")
            && !role_bytes.contains("first_active_lane"),
        "RoleImageBytes must own only packed bytes; columns and lane metadata live in RoleImageRef"
    );

    let role_ref = role_types
        .split("pub(crate) struct RoleImageRef {")
        .nth(1)
        .and_then(|tail| tail.split("}").next())
        .expect("role image ref");
    assert!(
        role_ref.contains("program: &'static CompiledProgramRef")
            && role_ref.contains("blob: BlobPtr")
            && !role_ref.contains("blob: &'static [u8]")
            && !role_ref.contains("program: CompiledProgramRef"),
        "RoleImageRef must point at the shared program descriptor and keep only a thin blob pointer"
    );

    assert!(
        role_projection.contains("impl<Steps> ProgramProjection<Steps>")
            && role_projection.contains("const PROGRAM_REF:")
            && role_projection.contains("&ProgramProjection::<Steps>::PROGRAM_REF")
            && !role_projection.contains(per_role_program_ref),
        "program-wide compact metadata must be owned once by ProgramProjection<Steps> and referenced by each role projection"
    );
}
