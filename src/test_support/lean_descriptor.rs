use crate::global::compiled::images::RoleDescriptorRef;
use std::{
    format,
    string::{String, ToString},
    vec::Vec,
};

fn bytes(values: impl IntoIterator<Item = u8>) -> String {
    let entries = values
        .into_iter()
        .map(|value| format!("{value}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{entries}]")
}

fn descriptor_fields(descriptor: RoleDescriptorRef) -> Vec<(&'static str, String)> {
    let program = descriptor.program();
    let resident = descriptor.local_event_rows();
    let footprint = resident.footprint();
    let columns = resident.columns();
    let program_columns = program.columns;
    let program_column_bytes = bytes(program_columns.proof_header_bytes());
    let program_bytes =
        bytes((0..program.proof_blob_len()).map(|offset| program.proof_byte_at(offset)));
    let role_bytes =
        bytes((0..descriptor.proof_blob_len()).map(|offset| descriptor.proof_byte_at(offset)));
    // The descriptor uses u16::MAX for an absent active lane.
    let first_active_lane = match descriptor.first_active_lane() {
        Some(lane) => lane,
        None => u16::MAX as usize,
    };
    let mut fields = [
        ("roleCount", program.role_count()),
        ("role", usize::from(descriptor.role())),
        ("logicalLaneCount", descriptor.logical_lane_count()),
        ("activeLaneCount", footprint.active_lane_count),
        (
            "endpointLaneSlotCount",
            descriptor.endpoint_lane_slot_count(),
        ),
        ("maxRouteCommitCount", descriptor.max_route_commit_count()),
        ("firstActiveLane", first_active_lane),
        ("activeLaneStart", resident.active_lane_row().start()),
        ("activeLaneLength", resident.active_lane_row().len()),
        ("atomCount", program.proof_atom_count()),
        ("routeResolverCount", program_columns.route_resolver_count()),
        (
            "routeParticipantCount",
            program_columns.route_participant_count(),
        ),
        ("scopeMarkerCount", program.source_scope_marker_count()),
        ("eventCount", descriptor.local_len()),
        ("dependencyRowCount", usize::from(columns.dependencies.len)),
        ("conflictRowCount", usize::from(columns.conflicts.len)),
        ("routeScopeCount", usize::from(columns.route_scopes.len)),
        (
            "residentBoundaryCount",
            usize::from(columns.resident_boundaries.len),
        ),
        ("laneBitCount", usize::from(columns.lane_bits.len)),
        (
            "routeArmLaneStepCount",
            usize::from(columns.route_arm_lane_step_rows.len),
        ),
        (
            "routeCommitRowCount",
            usize::from(columns.route_commit_rows.len),
        ),
        ("rollScopeCount", usize::from(columns.roll_scopes.len)),
        (
            "rollEntryViewCount",
            usize::from(columns.roll_entry_view_count),
        ),
        (
            "ownedReceiptCount",
            usize::from(columns.owned_receipt_count),
        ),
    ]
    .into_iter()
    .map(|(name, value)| (name, value.to_string()))
    .collect::<Vec<_>>();
    fields.extend([
        ("programColumnBytes", program_column_bytes),
        ("programBytes", program_bytes),
        ("roleBytes", role_bytes),
    ]);
    fields
}

/// Render the actual resident image, including mixed logical views. This
/// exports facts; it does not run projection or assert that Lean accepts it.
pub(crate) fn descriptor_source(descriptor: RoleDescriptorRef) -> String {
    let fields = descriptor_fields(descriptor)
        .into_iter()
        .map(|(name, value)| format!("  {name} := {value}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!("{{\n{fields}\n}}")
}

#[cfg(hibana_generated_protocols)]
pub(crate) fn descriptor_json(descriptor: RoleDescriptorRef) -> String {
    let fields = descriptor_fields(descriptor)
        .into_iter()
        .map(|(name, value)| format!("\"{name}\":{value}"))
        .collect::<Vec<_>>()
        .join(",");
    format!("{{{fields}}}")
}
