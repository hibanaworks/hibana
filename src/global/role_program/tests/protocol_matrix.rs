use super::*;
use crate::global::compiled::images::{
    PROGRAM_IMAGE_ATOM_STRIDE, PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE,
    PROGRAM_IMAGE_SCOPE_MARKER_STRIDE,
};
use std::println;

#[macro_use]
#[path = "final_form_protocol_measure_roles.rs"]
mod final_form_protocol_measure_roles;

#[derive(Clone, Copy)]
struct ProtocolMatrixMeasurement {
    program_blob_len: usize,
    role_blob_len: usize,
    endpoint_scratch_bytes: usize,
    largest_section_bytes: usize,
}

impl ProtocolMatrixMeasurement {
    const EMPTY: Self = Self {
        program_blob_len: 0,
        role_blob_len: 0,
        endpoint_scratch_bytes: 0,
        largest_section_bytes: 0,
    };

    fn max(self, other: Self) -> Self {
        Self {
            program_blob_len: self.program_blob_len.max(other.program_blob_len),
            role_blob_len: self.role_blob_len.max(other.role_blob_len),
            endpoint_scratch_bytes: self
                .endpoint_scratch_bytes
                .max(other.endpoint_scratch_bytes),
            largest_section_bytes: self.largest_section_bytes.max(other.largest_section_bytes),
        }
    }
}

fn endpoint_largest_section(layout: crate::endpoint::kernel::EndpointArenaLayout) -> usize {
    let mut largest = layout.event_cursor_state().bytes();
    largest = largest.max(layout.decision_state().bytes());
    largest = largest.max(layout.route_arm_history().bytes());
    largest = largest.max(layout.lane_offer_state_slots().bytes());
    largest = largest.max(layout.frontier_state().bytes());
    largest = largest.max(layout.frontier_root_rows().bytes());
    largest = largest.max(layout.frontier_root_active_slots().bytes());
    largest = largest.max(layout.frontier_visited_entries().bytes());
    largest = largest.max(layout.scope_evidence_slots().bytes());
    largest
}

fn rbl(column: ColumnRange, stride: usize) -> usize {
    column.byte_len(stride)
}

fn largest_program_section(
    program_ref: crate::global::compiled::images::CompiledProgramRef,
) -> usize {
    let columns = program_ref.columns;
    (columns.atom_count() * PROGRAM_IMAGE_ATOM_STRIDE)
        .max(columns.route_resolver_count() * PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE)
        .max(columns.scope_marker_count() * PROGRAM_IMAGE_SCOPE_MARKER_STRIDE)
}

fn largest_role_section(rows: &RoleImageRef) -> usize {
    let columns = rows.columns;
    rbl(columns.events, ROLE_IMAGE_EVENT_STRIDE)
        .max(rbl(columns.lanes, ROLE_IMAGE_LANE_STRIDE))
        .max(rbl(columns.dependencies, ROLE_IMAGE_DEPENDENCY_STRIDE))
        .max(rbl(columns.conflicts, ROLE_IMAGE_CONFLICT_STRIDE))
        .max(rbl(columns.route_scopes, ROLE_IMAGE_ROUTE_SCOPE_STRIDE))
        .max(rbl(
            columns.route_scope_conflicts,
            ROLE_IMAGE_CONFLICT_STRIDE,
        ))
        .max(rbl(columns.route_arms, ROLE_IMAGE_ROUTE_ARM_STRIDE))
        .max(rbl(columns.resident_boundaries, ROLE_IMAGE_U16_STRIDE))
        .max(rbl(columns.lane_bits, ROLE_IMAGE_LANE_STRIDE))
        .max(rbl(
            columns.route_arm_lane_rows,
            ROLE_IMAGE_LANE_RANGE_STRIDE,
        ))
        .max(rbl(
            columns.route_offer_lane_rows,
            ROLE_IMAGE_LANE_RANGE_STRIDE,
        ))
        .max(rbl(
            columns.route_arm_lane_step_rows,
            ROLE_IMAGE_ROUTE_ARM_LANE_STEP_STRIDE,
        ))
        .max(rbl(
            columns.route_commit_ranges,
            ROLE_IMAGE_LANE_RANGE_STRIDE,
        ))
        .max(rbl(columns.route_commit_rows, ROLE_IMAGE_CONFLICT_STRIDE))
}

fn measure_role<const ROLE: u8>(program: &RoleProgram<ROLE>) -> ProtocolMatrixMeasurement {
    let compiled = program.role_image_ref();
    let program_ref = *compiled.program;
    let descriptor = RoleDescriptorRef::from_resident(compiled);
    let rows = descriptor.local_event_rows();
    let endpoint_layout = descriptor.endpoint_arena_layout();
    ProtocolMatrixMeasurement {
        program_blob_len: program_ref.columns.blob_len(),
        role_blob_len: rows.columns.blob_len(),
        endpoint_scratch_bytes: endpoint_layout.total_bytes(),
        largest_section_bytes: largest_program_section(program_ref)
            .max(largest_role_section(rows))
            .max(endpoint_largest_section(endpoint_layout)),
    }
}

fn report_protocol_matrix(name: &str, measured: ProtocolMatrixMeasurement) {
    println!(
        "protocol-matrix name={name} program_blob_len={} role_blob_len={} endpoint_scratch_bytes={} largest_section_bytes={}",
        measured.program_blob_len,
        measured.role_blob_len,
        measured.endpoint_scratch_bytes,
        measured.largest_section_bytes
    );
    assert!(
        measured.program_blob_len <= u16::MAX as usize,
        "{name} program image must fit its compact offset domain"
    );
    assert!(
        measured.role_blob_len <= u16::MAX as usize,
        "{name} role image must fit its compact offset domain"
    );
}

#[test]
fn projected_protocol_matrix_reports_compact_resident_images() {
    macro_rules! report {
        ($name:ident) => {{
            let program = final_form_protocol!($name);
            report_protocol_matrix(
                stringify!($name),
                final_form_protocol_measure_roles!($name, &program),
            );
        }};
    }
    report!(minimal_send_recv);
    report!(nested_par_join);
    report!(route_with_unselected_nested_par);
    report!(triple_nested_route);
    report!(passive_nested_route_observer);
    report!(alternating_par_route);
    report!(huge_legal_choreography);
}
