use crate::global::compiled::images::RoleDescriptorRef;
use std::{format, string::String, vec::Vec};

fn bytes(values: impl IntoIterator<Item = u8>) -> String {
    let entries = values
        .into_iter()
        .map(|value| format!("{value}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{entries}]")
}

/// Render the actual resident image, including mixed logical views. This
/// exports facts; it does not run projection or assert that Lean accepts it.
pub(crate) fn descriptor_source(descriptor: RoleDescriptorRef) -> String {
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
    format!(
        "{{\n  roleCount := {}\n  role := {}\n  logicalLaneCount := {}\n  activeLaneCount := {}\n  \
         endpointLaneSlotCount := {}\n  maxRouteCommitCount := {}\n  firstActiveLane := {}\n  \
         activeLaneStart := {}\n  activeLaneLength := {}\n  atomCount := {}\n  routeResolverCount := {}\n  \
         routeParticipantCount := {}\n  scopeMarkerCount := {}\n  eventCount := {}\n  dependencyRowCount := {}\n  \
         conflictRowCount := {}\n  routeScopeCount := {}\n  residentBoundaryCount := {}\n  \
         laneBitCount := {}\n  routeArmLaneStepCount := {}\n  routeCommitRowCount := {}\n  \
         rollScopeCount := {}\n  rollEntryViewCount := {}\n  programColumnBytes := {program_column_bytes}\n  \
         programBytes := {program_bytes}\n  roleBytes := {role_bytes}\n}}",
        program.role_count(),
        descriptor.role(),
        descriptor.logical_lane_count(),
        footprint.active_lane_count,
        descriptor.endpoint_lane_slot_count(),
        descriptor.max_route_commit_count(),
        first_active_lane,
        resident.active_lane_row().start(),
        resident.active_lane_row().len(),
        program.proof_atom_count(),
        program_columns.route_resolver_count(),
        program_columns.route_participant_count(),
        program.source_scope_marker_count(),
        descriptor.local_len(),
        columns.dependencies.len,
        columns.conflicts.len,
        columns.route_scopes.len,
        columns.resident_boundaries.len,
        columns.lane_bits.len,
        columns.route_arm_lane_step_rows.len,
        columns.route_commit_rows.len,
        columns.roll_scopes.len,
        columns.roll_entry_view_count,
    )
}
