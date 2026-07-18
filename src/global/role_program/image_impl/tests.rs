use super::super::{
    BlobPtr, ColumnRange, PackedLaneRange, ROLE_IMAGE_CONFLICT_STRIDE,
    ROLE_IMAGE_DEPENDENCY_STRIDE, ROLE_IMAGE_EVENT_STRIDE, ROLE_IMAGE_LANE_RANGE_STRIDE,
    ROLE_IMAGE_ROLL_SCOPE_STRIDE, ROLE_IMAGE_ROUTE_ARM_STRIDE, ROLE_IMAGE_ROUTE_SCOPE_STRIDE,
    ROLE_IMAGE_U16_STRIDE, RoleImageBytes, RoleImageColumns, RoleImagePlan, RoleImageRef,
    RoleLaneImage, RoleProgram, RuntimeRoleFacts, RuntimeRoleFootprint, project,
};
use super::decode_binary_route_arm_index;
use super::metadata::{
    derive_active_lane_metadata, lane_columns_are_coherent, roll_scope_columns_are_coherent,
    route_commit_capacity_is_exact,
};
use crate::{
    g::{self, Msg},
    global::{
        compiled::lowering::RoleCompiledCounts,
        const_dsl::ScopeId,
        typestate::{LocalAction, LocalConflict},
    },
};

const BLOB_LEN: usize = 40;

#[test]
fn active_lane_metadata_is_derived_from_one_canonical_bitmap() {
    let bytes = [0b1001_0010u8, 0b0000_0010];
    let metadata = derive_active_lane_metadata(
        &bytes,
        ColumnRange::new(0, bytes.len(), 1),
        PackedLaneRange::new(0, bytes.len()),
    )
    .expect("canonical sparse active-lane bitmap");

    assert_eq!(metadata.active_lane_count, 4);
    assert_eq!(metadata.first_active_lane, 1);
    assert_eq!(metadata.logical_lane_count, 10);
}

#[test]
fn inactive_role_has_one_canonical_empty_active_bitmap() {
    let bytes: [u8; 0] = [];
    let metadata = derive_active_lane_metadata(
        &bytes,
        ColumnRange::new(0, 0, 1),
        PackedLaneRange::new(0, 0),
    )
    .expect("canonical inactive-role bitmap");

    assert_eq!(metadata.active_lane_count, 0);
    assert_eq!(metadata.first_active_lane, u16::MAX);
    assert_eq!(metadata.logical_lane_count, 1);
    assert!(
        derive_active_lane_metadata(&bytes, ColumnRange::new(0, 0, 1), PackedLaneRange::EMPTY,)
            .is_none()
    );
}

#[test]
fn active_lane_metadata_rejects_trailing_zero_alias() {
    let bytes = [1u8, 0];
    assert!(
        derive_active_lane_metadata(
            &bytes,
            ColumnRange::new(0, bytes.len(), 1),
            PackedLaneRange::new(0, bytes.len()),
        )
        .is_none()
    );
}

fn lane_column_fixture() -> ([u8; 64], RoleImageColumns, RuntimeRoleFootprint) {
    let mut bytes = [0u8; 64];
    bytes[20] = 0;
    bytes[21] = 1;
    bytes[22] = 0b11;
    bytes[23] = 0b01;
    bytes[24] = 0b10;
    bytes[25] = 0b11;
    write_u32(&mut bytes, 26, (1 << 16) | 1);
    write_u32(&mut bytes, 30, (2 << 16) | 1);
    write_u32(&mut bytes, 34, (3 << 16) | 1);
    write_u32(&mut bytes, 38, 1);
    write_u32(&mut bytes, 42, u16::MAX as u32);
    write_u32(&mut bytes, 46, (1 << 16) | 1);
    write_u32(&mut bytes, 50, u16::MAX as u32);
    bytes[54] = 0;
    bytes[59] = 1;
    write_u16(&mut bytes, 60, 1);
    write_u16(&mut bytes, 62, 1);

    let empty = ColumnRange::new(bytes.len(), 0, 1);
    let columns = RoleImageColumns {
        events: ColumnRange::new(0, 2, ROLE_IMAGE_EVENT_STRIDE),
        lanes: ColumnRange::new(20, 2, super::super::ROLE_IMAGE_LANE_STRIDE),
        dependencies: empty,
        conflicts: empty,
        route_scopes: empty,
        route_scope_conflicts: empty,
        route_arms: ColumnRange::new(38, 2, ROLE_IMAGE_ROUTE_ARM_STRIDE),
        resident_boundaries: empty,
        lane_bits: ColumnRange::new(22, 4, 1),
        route_arm_lane_rows: ColumnRange::new(26, 2, ROLE_IMAGE_LANE_RANGE_STRIDE),
        route_offer_lane_rows: ColumnRange::new(34, 1, ROLE_IMAGE_LANE_RANGE_STRIDE),
        route_arm_lane_step_rows: ColumnRange::new(
            54,
            2,
            super::super::ROLE_IMAGE_ROUTE_ARM_LANE_STEP_STRIDE,
        ),
        route_commit_ranges: empty,
        route_commit_rows: empty,
        roll_scopes: empty,
    };
    let footprint = RuntimeRoleFootprint {
        max_route_commit_count: 0,
        route_arm_state_capacity: 2,
        local_step_count: 2,
        route_scope_count: 1,
        active_lane_count: 2,
        endpoint_lane_slot_count: 2,
        logical_lane_count: 2,
    };
    (bytes, columns, footprint)
}

#[test]
fn lane_columns_bind_partition_arm_steps_and_offer_union() {
    let (bytes, columns, footprint) = lane_column_fixture();
    let active = PackedLaneRange::new(0, 1);
    assert!(lane_columns_are_coherent(
        &bytes, columns, active, footprint
    ));

    let mut wrong_offer = bytes;
    wrong_offer[25] = 0b01;
    assert!(!lane_columns_are_coherent(
        &wrong_offer,
        columns,
        active,
        footprint,
    ));

    let mut wrong_step = bytes;
    wrong_step[59] = 0;
    assert!(!lane_columns_are_coherent(
        &wrong_step,
        columns,
        active,
        footprint,
    ));

    let mut wrong_first_step = bytes;
    write_u16(&mut wrong_first_step, 55, 1);
    assert!(!lane_columns_are_coherent(
        &wrong_first_step,
        columns,
        active,
        footprint,
    ));

    let mut wrong_last_step = bytes;
    write_u16(&mut wrong_last_step, 62, 0);
    assert!(!lane_columns_are_coherent(
        &wrong_last_step,
        columns,
        active,
        footprint,
    ));

    let mut orphan_columns = columns;
    orphan_columns.lane_bits.len += 1;
    assert!(!lane_columns_are_coherent(
        &bytes,
        orphan_columns,
        active,
        footprint,
    ));

    let mut inactive_event_lane = bytes;
    inactive_event_lane[21] = 0;
    assert!(!lane_columns_are_coherent(
        &inactive_event_lane,
        columns,
        active,
        footprint,
    ));
}

#[test]
fn route_commit_capacity_requires_the_exact_contiguous_partition_and_maximum() {
    let bytes = [1, 0, 0, 0, 2, 0, 1, 0];
    let ranges = ColumnRange::new(0, 2, ROLE_IMAGE_LANE_RANGE_STRIDE);

    assert!(route_commit_capacity_is_exact(&bytes, ranges, 3, 2));
    assert!(!route_commit_capacity_is_exact(&bytes, ranges, 3, 1));
    assert!(!route_commit_capacity_is_exact(&bytes, ranges, 3, 3));
    assert!(!route_commit_capacity_is_exact(&bytes, ranges, 4, 2));
}

#[test]
fn roll_scope_columns_require_unique_laminar_ranges() {
    let rows = ColumnRange::new(0, 2, ROLE_IMAGE_ROLL_SCOPE_STRIDE);
    let encode = |left_scope: u16,
                  left_start: u32,
                  left_len: u32,
                  right_scope: u16,
                  right_start: u32,
                  right_len: u32| {
        let mut bytes = [0u8; 2 * ROLE_IMAGE_ROLL_SCOPE_STRIDE];
        write_u16(&mut bytes, 0, left_scope);
        write_u32(&mut bytes, 2, (left_start << 16) | left_len);
        write_u16(&mut bytes, ROLE_IMAGE_ROLL_SCOPE_STRIDE, right_scope);
        write_u32(
            &mut bytes,
            ROLE_IMAGE_ROLL_SCOPE_STRIDE + 2,
            (right_start << 16) | right_len,
        );
        bytes
    };

    let nested = encode(0, 0, 4, 1, 1, 2);
    assert!(roll_scope_columns_are_coherent(&nested, rows, 4));
    let equal_projection = encode(0, 0, 4, 1, 0, 4);
    assert!(roll_scope_columns_are_coherent(&equal_projection, rows, 4));
    let disjoint = encode(0, 0, 2, 1, 2, 2);
    assert!(roll_scope_columns_are_coherent(&disjoint, rows, 4));

    let duplicate_scope = encode(0, 0, 4, 0, 1, 2);
    assert!(!roll_scope_columns_are_coherent(&duplicate_scope, rows, 4));
    let crossing = encode(0, 0, 3, 1, 2, 2);
    assert!(!roll_scope_columns_are_coherent(&crossing, rows, 4));
}

const fn write_u16<const N: usize>(bytes: &mut [u8; N], offset: usize, value: u16) {
    bytes[offset] = value as u8;
    bytes[offset + 1] = (value >> 8) as u8;
}

const fn write_u32<const N: usize>(bytes: &mut [u8; N], offset: usize, value: u32) {
    write_u16(bytes, offset, value as u16);
    write_u16(bytes, offset + 2, (value >> 16) as u16);
}

const fn route_arm_bytes(event_row: u32, lane_step_len_and_child_slot: u32) -> [u8; BLOB_LEN] {
    let mut bytes = [0; BLOB_LEN];
    write_u32(&mut bytes, 0, event_row);
    write_u32(&mut bytes, 4, lane_step_len_and_child_slot);
    bytes
}

const fn roll_scope_bytes(scope: u16, event_row: u32) -> [u8; BLOB_LEN] {
    let mut bytes = [0; BLOB_LEN];
    write_u16(&mut bytes, 0, scope);
    write_u32(&mut bytes, 2, event_row);
    bytes
}

const fn route_scope_bytes(scope: u16) -> [u8; BLOB_LEN] {
    let mut bytes = [0; BLOB_LEN];
    write_u16(&mut bytes, 0, scope);
    bytes
}

const fn noncanonical_empty_lane_range_bytes() -> [u8; BLOB_LEN] {
    let mut bytes = [0; BLOB_LEN];
    write_u32(&mut bytes, 0, 1 << 16);
    bytes
}

const fn event_bytes(dependency_row: u16, conflict_row: u16, flags: u8) -> [u8; BLOB_LEN] {
    let mut bytes = [0; BLOB_LEN];
    write_u16(&mut bytes, 2, dependency_row);
    write_u16(&mut bytes, 4, conflict_row);
    write_u16(&mut bytes, 6, u16::MAX);
    bytes[9] = flags;
    bytes
}

const fn event_header_bytes(eff_index: u16, scope: u16) -> [u8; BLOB_LEN] {
    let mut bytes = event_bytes(u16::MAX, u16::MAX, 0);
    write_u16(&mut bytes, 0, eff_index);
    write_u16(&mut bytes, 6, scope);
    bytes
}

const fn local_event_with_lane_bytes(lane: u8) -> [u8; BLOB_LEN] {
    let mut bytes = event_header_bytes(0, u16::MAX);
    bytes[ROLE_IMAGE_EVENT_STRIDE] = lane;
    bytes
}

const fn missing_program_event_with_lane_bytes(lane: u8) -> [u8; BLOB_LEN] {
    let mut bytes = event_header_bytes(1, u16::MAX);
    bytes[ROLE_IMAGE_EVENT_STRIDE] = lane;
    bytes
}

const fn event_with_empty_dependency_bytes() -> [u8; BLOB_LEN] {
    let mut bytes = event_bytes(0, u16::MAX, 0);
    let mut offset = ROLE_IMAGE_EVENT_STRIDE;
    while offset < ROLE_IMAGE_EVENT_STRIDE + ROLE_IMAGE_DEPENDENCY_STRIDE {
        bytes[offset] = u8::MAX;
        offset += 1;
    }
    bytes
}

const fn event_with_empty_conflict_bytes() -> [u8; BLOB_LEN] {
    let mut bytes = event_bytes(u16::MAX, 0, 0);
    write_u16(&mut bytes, ROLE_IMAGE_EVENT_STRIDE, u16::MAX);
    bytes
}

const fn event_with_out_of_domain_conflict_bytes() -> [u8; BLOB_LEN] {
    let mut bytes = event_bytes(u16::MAX, 0, 0);
    write_u16(&mut bytes, ROLE_IMAGE_EVENT_STRIDE, 0x8000);
    bytes
}

const fn event_with_out_of_domain_dependency_bytes() -> [u8; BLOB_LEN] {
    let mut bytes = event_bytes(0, u16::MAX, 0);
    write_u16(&mut bytes, ROLE_IMAGE_EVENT_STRIDE, 0);
    write_u16(&mut bytes, ROLE_IMAGE_EVENT_STRIDE + 2, 1);
    write_u16(&mut bytes, ROLE_IMAGE_EVENT_STRIDE + 4, 0);
    write_u16(&mut bytes, ROLE_IMAGE_EVENT_STRIDE + 6, 0x8002);
    bytes
}

const fn event_with_out_of_bounds_dependency_range_bytes() -> [u8; BLOB_LEN] {
    let mut bytes = event_bytes(0, u16::MAX, 0);
    write_u16(&mut bytes, ROLE_IMAGE_EVENT_STRIDE, 0);
    write_u16(&mut bytes, ROLE_IMAGE_EVENT_STRIDE + 2, 2);
    write_u16(&mut bytes, ROLE_IMAGE_EVENT_STRIDE + 4, 0);
    write_u16(&mut bytes, ROLE_IMAGE_EVENT_STRIDE + 6, 0);
    bytes
}

const fn route_arm_with_foreign_lane_step_bytes() -> [u8; BLOB_LEN] {
    let mut bytes = route_arm_bytes(1, u16::MAX as u32);
    bytes[ROLE_IMAGE_ROUTE_ARM_STRIDE] = 0;
    write_u16(&mut bytes, ROLE_IMAGE_ROUTE_ARM_STRIDE + 1, 1);
    write_u16(&mut bytes, ROLE_IMAGE_ROUTE_ARM_STRIDE + 3, 1);
    bytes
}

const fn resident_boundary_bytes(end: u16) -> [u8; BLOB_LEN] {
    let mut bytes = [0; BLOB_LEN];
    write_u16(&mut bytes, ROLE_IMAGE_U16_STRIDE, end);
    bytes
}

const fn lane_range_bytes(raw: u32) -> [u8; BLOB_LEN] {
    let mut bytes = [0; BLOB_LEN];
    write_u32(&mut bytes, 0, raw);
    bytes
}

const fn duplicate_route_scope_bytes() -> [u8; BLOB_LEN] {
    let mut bytes = [0; BLOB_LEN];
    write_u16(&mut bytes, 0, ScopeId::route(0).raw());
    write_u16(
        &mut bytes,
        ROLE_IMAGE_ROUTE_SCOPE_STRIDE,
        ScopeId::route(0).raw(),
    );
    bytes
}

const fn route_commit_chain_bytes(len: u16, rows: [u16; 3]) -> [u8; BLOB_LEN] {
    let mut bytes = [0; BLOB_LEN];
    write_u32(&mut bytes, 0, len as u32);
    write_u16(&mut bytes, 4, rows[0]);
    write_u16(&mut bytes, 6, rows[1]);
    write_u16(&mut bytes, 8, rows[2]);
    write_u16(&mut bytes, 10, ScopeId::route(1).raw());
    write_u16(&mut bytes, 12, ScopeId::route(0).raw());
    write_u16(&mut bytes, 14, 0);
    write_u16(&mut bytes, 16, u16::MAX);
    bytes
}

const fn passive_child_without_parent_bytes() -> [u8; BLOB_LEN] {
    let mut bytes = [0; BLOB_LEN];
    write_u32(&mut bytes, 0, 1);
    write_u32(&mut bytes, 4, 1);
    write_u16(&mut bytes, 8, ScopeId::route(0).raw());
    write_u16(&mut bytes, 10, ScopeId::route(1).raw());
    write_u16(&mut bytes, 12, u16::MAX);
    write_u16(&mut bytes, 14, u16::MAX);
    bytes
}

fn route_commit_columns(row_len: usize) -> RoleImageColumns {
    let mut columns = empty_columns();
    columns.route_commit_ranges = ColumnRange::new(0, 1, ROLE_IMAGE_LANE_RANGE_STRIDE);
    columns.route_commit_rows = ColumnRange::new(4, row_len, ROLE_IMAGE_CONFLICT_STRIDE);
    columns.route_scopes = ColumnRange::new(10, 2, ROLE_IMAGE_ROUTE_SCOPE_STRIDE);
    columns.route_scope_conflicts = ColumnRange::new(14, 2, ROLE_IMAGE_CONFLICT_STRIDE);
    columns
}

static EMPTY_DESCRIPTOR_BYTES: [u8; BLOB_LEN] = [u8::MAX; BLOB_LEN];
static ROUTE_ARM_WITH_NONCANONICAL_EMPTY_LANE_STEP_BYTES: [u8; BLOB_LEN] =
    route_arm_bytes(0, (1 << 16) | u16::MAX as u32);
static ROUTE_ARM_WITH_NONCANONICAL_EMPTY_EVENT_RANGE_BYTES: [u8; BLOB_LEN] =
    route_arm_bytes(1 << 16, u16::MAX as u32);
static ROUTE_ARM_WITH_OUT_OF_BOUNDS_EVENT_BYTES: [u8; BLOB_LEN] =
    route_arm_bytes(1, u16::MAX as u32);
static ROUTE_ARM_WITH_OUT_OF_BOUNDS_LANE_STEP_BYTES: [u8; BLOB_LEN] =
    route_arm_bytes(1, (1 << 16) | u16::MAX as u32);
static ROLL_SCOPE_WITH_EMPTY_EVENT_BYTES: [u8; BLOB_LEN] = roll_scope_bytes(0, u32::MAX);
static ROLL_SCOPE_WITH_OUT_OF_BOUNDS_EVENT_BYTES: [u8; BLOB_LEN] = roll_scope_bytes(0, 1);
static NON_ROUTE_SCOPE_BYTES: [u8; BLOB_LEN] = route_scope_bytes(ScopeId::roll_scope(0).raw());
static ROUTE_SCOPE_OUT_OF_RANGE_BYTES: [u8; BLOB_LEN] = route_scope_bytes(0x8000);
static ROLL_SCOPE_OUT_OF_RANGE_BYTES: [u8; BLOB_LEN] = roll_scope_bytes(ScopeId::LOCAL_CAPACITY, 1);
static EVENT_WITH_INVALID_FLAGS_BYTES: [u8; BLOB_LEN] = event_bytes(u16::MAX, u16::MAX, 2);
static EVENT_WITH_OUT_OF_DOMAIN_INDEX_BYTES: [u8; BLOB_LEN] =
    event_header_bytes(u16::MAX, u16::MAX);
static EVENT_WITH_OUT_OF_DOMAIN_SCOPE_BYTES: [u8; BLOB_LEN] = event_header_bytes(0, 0x8000);
static EVENT_WITH_EMPTY_DEPENDENCY_BYTES: [u8; BLOB_LEN] = event_with_empty_dependency_bytes();
static EVENT_WITH_EMPTY_CONFLICT_BYTES: [u8; BLOB_LEN] = event_with_empty_conflict_bytes();
static EVENT_WITH_OUT_OF_DOMAIN_CONFLICT_BYTES: [u8; BLOB_LEN] =
    event_with_out_of_domain_conflict_bytes();
static EVENT_WITH_OUT_OF_DOMAIN_DEPENDENCY_BYTES: [u8; BLOB_LEN] =
    event_with_out_of_domain_dependency_bytes();
static EVENT_WITH_OUT_OF_BOUNDS_DEPENDENCY_RANGE_BYTES: [u8; BLOB_LEN] =
    event_with_out_of_bounds_dependency_range_bytes();
static ROUTE_ARM_WITH_FOREIGN_LANE_STEP_BYTES: [u8; BLOB_LEN] =
    route_arm_with_foreign_lane_step_bytes();
static ZERO_LENGTH_RESIDENT_BOUNDARY_BYTES: [u8; BLOB_LEN] = resident_boundary_bytes(0);
static OUT_OF_BOUNDS_RESIDENT_BOUNDARY_BYTES: [u8; BLOB_LEN] = resident_boundary_bytes(1);
static OUT_OF_BOUNDS_LANE_RANGE_BYTES: [u8; BLOB_LEN] = lane_range_bytes(1);
static ZERO_LENGTH_LANE_RANGE_BYTES: [u8; BLOB_LEN] = lane_range_bytes(0);
static NONCANONICAL_EMPTY_LANE_RANGE_BYTES: [u8; BLOB_LEN] = noncanonical_empty_lane_range_bytes();
static OUT_OF_DOMAIN_LOCAL_LANE_BYTES: [u8; BLOB_LEN] = [1; BLOB_LEN];
static LOCAL_EVENT_LANE_MISMATCH_BYTES: [u8; BLOB_LEN] = local_event_with_lane_bytes(1);
static MISSING_PROGRAM_EVENT_BYTES: [u8; BLOB_LEN] = missing_program_event_with_lane_bytes(0);
static DUPLICATE_ROUTE_SCOPE_BYTES: [u8; BLOB_LEN] = duplicate_route_scope_bytes();
static ROUTE_COMMIT_VALID_NESTED_BYTES: [u8; BLOB_LEN] = route_commit_chain_bytes(2, [0, 2, 0]);
static ROUTE_COMMIT_FOREIGN_CURRENT_BYTES: [u8; BLOB_LEN] = route_commit_chain_bytes(1, [0, 0, 0]);
static ROUTE_COMMIT_FOREIGN_PARENT_BYTES: [u8; BLOB_LEN] = route_commit_chain_bytes(2, [1, 2, 0]);
static ROUTE_COMMIT_TRUNCATED_PARENT_BYTES: [u8; BLOB_LEN] = route_commit_chain_bytes(1, [2, 0, 0]);
static ROUTE_COMMIT_LEADING_PARENT_BYTES: [u8; BLOB_LEN] = route_commit_chain_bytes(3, [0, 0, 2]);
static PASSIVE_CHILD_WITHOUT_PARENT_BYTES: [u8; BLOB_LEN] = passive_child_without_parent_bytes();

fn empty_columns() -> RoleImageColumns {
    let empty = ColumnRange::new(EMPTY_DESCRIPTOR_BYTES.len(), 0, 1);
    RoleImageColumns {
        events: empty,
        lanes: empty,
        dependencies: empty,
        conflicts: empty,
        route_scopes: empty,
        route_scope_conflicts: empty,
        route_arms: empty,
        resident_boundaries: empty,
        lane_bits: empty,
        route_arm_lane_rows: empty,
        route_offer_lane_rows: empty,
        route_arm_lane_step_rows: empty,
        route_commit_ranges: empty,
        route_commit_rows: empty,
        roll_scopes: empty,
    }
}

fn image<'a>(columns: &'a RoleImageColumns, bytes: &'static [u8; BLOB_LEN]) -> RoleLaneImage<'a> {
    RoleLaneImage::new(columns, BlobPtr::from_array(bytes, columns.blob_len()))
}

fn assert_invariant(action: impl FnOnce()) {
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(action)).is_err(),
        "malformed resident descriptor must fail closed"
    );
}

#[test]
fn role_image_column_range_rejects_stride_multiplication_overflow() {
    assert_invariant(|| {
        let _ = ColumnRange::new(0, 2, usize::MAX);
    });
}

#[test]
fn resident_role_image_fit_probe_rejects_undersized_storage() {
    let eff_list = crate::global::const_dsl::const_send_typed::<0, 1, crate::g::Msg<1, ()>, 0, 8>();
    let facts = RuntimeRoleFacts::from_counts(RoleCompiledCounts {
        max_route_commit_count: 0,
        local_step_count: 1,
        route_scope_count: 0,
        active_lane_count: 1,
        endpoint_lane_slot_count: 1,
        logical_lane_count: 1,
    });

    let plan = RoleImagePlan::from_program(&eff_list, facts, 0);
    assert!(plan.build_if_fits::<0, 8>(&eff_list, facts, 0).is_none());
}

#[test]
fn resident_parallel_role_image_plan_matches_lane_bit_storage() {
    type Parallel = g::Par<g::Send<0, 1, Msg<90, ()>>, g::Send<0, 1, Msg<91, ()>>>;

    let source = crate::g::ProgramSourceData::<8>::lower::<Parallel>();
    let eff_list = source.eff_list();
    let facts = RuntimeRoleFacts::from_counts(RoleCompiledCounts {
        max_route_commit_count: 0,
        local_step_count: 2,
        route_scope_count: 0,
        active_lane_count: 2,
        endpoint_lane_slot_count: 2,
        logical_lane_count: 2,
    });
    let plan = RoleImagePlan::from_program(eff_list, facts, 0);
    let build = plan
        .build_if_fits::<64, 8>(eff_list, facts, 0)
        .expect("planned parallel descriptor fits");

    assert_eq!(
        plan.columns.resident_boundaries.len,
        build.columns.resident_boundaries.len
    );
    assert_eq!(plan.columns.lane_bits.len, build.columns.lane_bits.len);
}

#[test]
fn resident_role_image_fit_probe_rejects_plan_drift() {
    let eff_list = crate::global::const_dsl::const_send_typed::<0, 1, crate::g::Msg<1, ()>, 0, 8>();
    let facts = RuntimeRoleFacts::from_counts(RoleCompiledCounts {
        max_route_commit_count: 0,
        local_step_count: 1,
        route_scope_count: 0,
        active_lane_count: 1,
        endpoint_lane_slot_count: 1,
        logical_lane_count: 1,
    });
    let mut plan = RoleImagePlan::from_program(&eff_list, facts, 0);
    plan.columns.lane_bits.len += 1;

    assert_invariant(|| {
        let _ = plan.build_if_fits::<64, 8>(&eff_list, facts, 0);
    });
}

#[test]
#[should_panic(expected = "role image")]
fn resident_role_image_constructor_rejects_undersized_storage() {
    let eff_list = crate::global::const_dsl::const_send_typed::<0, 1, crate::g::Msg<1, ()>, 0, 8>();
    let facts = RuntimeRoleFacts::from_counts(RoleCompiledCounts {
        max_route_commit_count: 0,
        local_step_count: 1,
        route_scope_count: 0,
        active_lane_count: 1,
        endpoint_lane_slot_count: 1,
        logical_lane_count: 1,
    });
    let plan = RoleImagePlan::from_program(&eff_list, facts, 0);
    let _ = RoleImageBytes::<0>::emit(&eff_list, facts, 0, plan.columns);
}

fn assert_route_commit_fixture_decodes(image: &RoleLaneImage<'_>, expected_rows: &[(ScopeId, u8)]) {
    assert_eq!(image.route_scope_by_slot(0), Some(ScopeId::route(1)));
    assert_eq!(image.route_scope_by_slot(1), Some(ScopeId::route(0)));
    assert!(matches!(
        image.route_scope_conflict_by_slot(0).to_conflict(),
        Some(LocalConflict::RouteArm { scope, arm })
            if scope.same(ScopeId::route(0)) && arm == 0
    ));
    assert!(image.route_scope_conflict_by_slot(1).is_none());
    for (idx, (expected_scope, expected_arm)) in expected_rows.iter().copied().enumerate() {
        assert!(matches!(
            image.route_commit_row_at(idx).to_conflict(),
            Some(LocalConflict::RouteArm { scope, arm })
                if scope.same(expected_scope) && arm == expected_arm
        ));
    }
}

#[test]
fn resident_route_arm_index_decoder_accepts_exact_binary_domain() {
    assert_eq!(decode_binary_route_arm_index(0), Some(0));
    assert_eq!(decode_binary_route_arm_index(1), Some(1));
    for raw in 2..=u8::MAX {
        assert_eq!(decode_binary_route_arm_index(raw), None);
    }
}

#[test]
#[should_panic(expected = "lane range descriptor outside compact domain")]
fn lane_range_constructor_rejects_reserved_empty_encoding() {
    let _ = PackedLaneRange::new(u16::MAX as usize, u16::MAX as usize);
}

#[test]
#[should_panic(expected = "lane range descriptor outside compact domain")]
fn lane_range_constructor_rejects_end_outside_compact_domain() {
    let _ = PackedLaneRange::new(u16::MAX as usize, 1);
}

#[test]
#[should_panic(expected = "lane range descriptor outside compact domain")]
fn lane_range_constructor_rejects_limb_outside_compact_domain() {
    let _ = PackedLaneRange::new(u16::MAX as usize + 1, 0);
}

#[test]
fn resident_descriptor_rejects_absent_route_scope_row() {
    let mut columns = empty_columns();
    columns.route_scopes = ColumnRange::new(0, 1, ROLE_IMAGE_ROUTE_SCOPE_STRIDE);
    let image = image(&columns, &EMPTY_DESCRIPTOR_BYTES);

    assert_invariant(|| {
        let _ = image.route_scope_by_slot(0);
    });
}

#[test]
fn resident_descriptor_rejects_non_route_scope_row() {
    let mut columns = empty_columns();
    columns.route_scopes = ColumnRange::new(0, 1, ROLE_IMAGE_ROUTE_SCOPE_STRIDE);
    let image = image(&columns, &NON_ROUTE_SCOPE_BYTES);

    assert_invariant(|| {
        let _ = image.route_scope_by_slot(0);
    });
}

#[test]
fn resident_descriptor_rejects_route_scope_ordinal_out_of_range() {
    let mut columns = empty_columns();
    columns.route_scopes = ColumnRange::new(0, 1, ROLE_IMAGE_ROUTE_SCOPE_STRIDE);
    let image = image(&columns, &ROUTE_SCOPE_OUT_OF_RANGE_BYTES);

    assert_invariant(|| {
        let _ = image.route_scope_by_slot(0);
    });
}

#[test]
fn resident_descriptor_rejects_duplicate_route_scope_authority() {
    let mut columns = empty_columns();
    columns.route_scopes = ColumnRange::new(0, 2, ROLE_IMAGE_ROUTE_SCOPE_STRIDE);
    let image = image(&columns, &DUPLICATE_ROUTE_SCOPE_BYTES);

    assert_invariant(|| {
        let _ = image.route_scope_slot(ScopeId::route(0));
    });
}

#[test]
fn resident_descriptor_rejects_empty_route_arm_row() {
    let mut columns = empty_columns();
    columns.route_arms = ColumnRange::new(0, 1, ROLE_IMAGE_ROUTE_ARM_STRIDE);
    let image = image(&columns, &EMPTY_DESCRIPTOR_BYTES);

    assert_invariant(|| {
        let _ = image.route_arm_event_row_by_slot(0, 0);
    });
}

#[test]
fn resident_descriptor_rejects_noncanonical_empty_route_arm_lane_step_range() {
    let mut columns = empty_columns();
    columns.route_arms = ColumnRange::new(0, 1, ROLE_IMAGE_ROUTE_ARM_STRIDE);
    let image = image(&columns, &ROUTE_ARM_WITH_NONCANONICAL_EMPTY_LANE_STEP_BYTES);

    assert_invariant(|| {
        let _ = image.route_arm_event_row_by_slot(0, 0);
    });
}

#[test]
fn resident_descriptor_rejects_noncanonical_empty_route_arm_event_range() {
    let mut columns = empty_columns();
    columns.route_arms = ColumnRange::new(0, 1, ROLE_IMAGE_ROUTE_ARM_STRIDE);
    let image = image(
        &columns,
        &ROUTE_ARM_WITH_NONCANONICAL_EMPTY_EVENT_RANGE_BYTES,
    );

    assert_invariant(|| {
        let _ = image.route_arm_event_row_by_slot(0, 0);
    });
}

#[test]
fn resident_descriptor_rejects_empty_lane_range_row() {
    let mut columns = empty_columns();
    columns.route_commit_ranges = ColumnRange::new(0, 1, ROLE_IMAGE_LANE_RANGE_STRIDE);
    let image = image(&columns, &EMPTY_DESCRIPTOR_BYTES);

    assert_invariant(|| {
        let _ = image.route_commit_range_by_slot(0, 0);
    });
}

#[test]
fn resident_descriptor_rejects_noncanonical_empty_lane_range_row() {
    let mut columns = empty_columns();
    columns.route_arm_lane_rows = ColumnRange::new(0, 1, ROLE_IMAGE_LANE_RANGE_STRIDE);
    let image = image(&columns, &NONCANONICAL_EMPTY_LANE_RANGE_BYTES);

    assert_invariant(|| {
        let _ = image.route_scope_arm_lane_set_by_slot(0, 0, 1);
    });
}

#[test]
fn resident_descriptor_rejects_empty_roll_scope_row() {
    let mut columns = empty_columns();
    columns.roll_scopes = ColumnRange::new(0, 1, ROLE_IMAGE_ROLL_SCOPE_STRIDE);
    let image = image(&columns, &EMPTY_DESCRIPTOR_BYTES);

    assert_invariant(|| {
        let _ = image.roll_scope_row(0);
    });
}

#[test]
fn resident_descriptor_rejects_empty_roll_scope_event_range() {
    let mut columns = empty_columns();
    columns.roll_scopes = ColumnRange::new(0, 1, ROLE_IMAGE_ROLL_SCOPE_STRIDE);
    let image = image(&columns, &ROLL_SCOPE_WITH_EMPTY_EVENT_BYTES);

    assert_invariant(|| {
        let _ = image.roll_scope_row(0);
    });
}

#[test]
fn resident_descriptor_rejects_roll_scope_ordinal_out_of_range() {
    let mut columns = empty_columns();
    columns.roll_scopes = ColumnRange::new(0, 1, ROLE_IMAGE_ROLL_SCOPE_STRIDE);
    columns.events = ColumnRange::new(ROLE_IMAGE_ROLL_SCOPE_STRIDE, 1, ROLE_IMAGE_EVENT_STRIDE);
    let image = image(&columns, &ROLL_SCOPE_OUT_OF_RANGE_BYTES);

    assert_invariant(|| {
        let _ = image.roll_scope_row(0);
    });
}

#[test]
fn resident_descriptor_rejects_empty_local_event_row() {
    let mut columns = empty_columns();
    columns.events = ColumnRange::new(0, 1, ROLE_IMAGE_EVENT_STRIDE);
    let image = image(&columns, &EMPTY_DESCRIPTOR_BYTES);

    assert_invariant(|| {
        let _ = image.local_step_event(0);
    });
}

#[test]
fn resident_descriptor_rejects_reserved_local_event_flags() {
    let mut columns = empty_columns();
    columns.events = ColumnRange::new(0, 1, ROLE_IMAGE_EVENT_STRIDE);
    let image = image(&columns, &EVENT_WITH_INVALID_FLAGS_BYTES);

    assert_invariant(|| {
        let _ = image.local_step_event(0);
    });
}

#[test]
fn resident_descriptor_rejects_out_of_domain_local_event_index() {
    let mut columns = empty_columns();
    columns.events = ColumnRange::new(0, 1, ROLE_IMAGE_EVENT_STRIDE);
    let image = image(&columns, &EVENT_WITH_OUT_OF_DOMAIN_INDEX_BYTES);

    assert_invariant(|| {
        let _ = image.local_step_event(0);
    });
}

#[test]
fn resident_descriptor_rejects_out_of_domain_local_event_scope() {
    let mut columns = empty_columns();
    columns.events = ColumnRange::new(0, 1, ROLE_IMAGE_EVENT_STRIDE);
    let image = image(&columns, &EVENT_WITH_OUT_OF_DOMAIN_SCOPE_BYTES);

    assert_invariant(|| {
        let _ = image.local_step_event(0);
    });
}

#[test]
fn resident_descriptor_rejects_local_step_lane_outside_logical_domain() {
    let mut columns = empty_columns();
    columns.lanes = ColumnRange::new(0, 1, super::super::ROLE_IMAGE_LANE_STRIDE);
    let image = image(&columns, &OUT_OF_DOMAIN_LOCAL_LANE_BYTES);

    assert_invariant(|| {
        let _ = image.local_step_lane(0, 1);
    });
}

#[test]
fn resident_descriptor_rejects_program_lane_mismatch() {
    let program: RoleProgram<0> = project(&g::send::<0, 1, Msg<1, ()>>());
    let compiled = program.role_image_ref().program;
    assert_eq!(compiled.atom_at(0).expect("compiled atom").lane, 0);

    let mut columns = empty_columns();
    columns.events = ColumnRange::new(0, 1, ROLE_IMAGE_EVENT_STRIDE);
    columns.lanes = ColumnRange::new(
        ROLE_IMAGE_EVENT_STRIDE,
        1,
        super::super::ROLE_IMAGE_LANE_STRIDE,
    );
    let image = RoleImageRef {
        program: compiled,
        role: 0,
        facts: RuntimeRoleFacts::from_counts(RoleCompiledCounts {
            max_route_commit_count: 0,
            local_step_count: 1,
            route_scope_count: 0,
            active_lane_count: 1,
            endpoint_lane_slot_count: 2,
            logical_lane_count: 2,
        }),
        columns,
        blob: BlobPtr::from_array(&LOCAL_EVENT_LANE_MISMATCH_BYTES, columns.blob_len()),
        active_lane_row: PackedLaneRange::new(0, 0),
        first_active_lane: 1,
    };

    let raw_node = image
        .lanes()
        .local_step_node(0, image.role, image.program)
        .expect("event and compiled atom rows must decode before lane validation");
    assert!(matches!(
        raw_node.action(),
        LocalAction::Send { lane: 0, .. }
    ));
    assert_eq!(image.local_step_lane(0), Some(1));

    assert_invariant(|| {
        let _ = image.local_step_node(0);
    });
}

#[test]
fn resident_descriptor_rejects_foreign_role_event() {
    let program: RoleProgram<2> = project(&g::seq(
        g::send::<0, 1, Msg<1, ()>>(),
        g::send::<0, 2, Msg<2, ()>>(),
    ));
    let compiled = program.role_image_ref().program;

    let mut columns = empty_columns();
    columns.events = ColumnRange::new(0, 1, ROLE_IMAGE_EVENT_STRIDE);
    columns.lanes = ColumnRange::new(
        ROLE_IMAGE_EVENT_STRIDE,
        1,
        super::super::ROLE_IMAGE_LANE_STRIDE,
    );
    let image = RoleImageRef {
        program: compiled,
        role: 2,
        facts: RuntimeRoleFacts::from_counts(RoleCompiledCounts {
            max_route_commit_count: 0,
            local_step_count: 1,
            route_scope_count: 0,
            active_lane_count: 1,
            endpoint_lane_slot_count: 2,
            logical_lane_count: 2,
        }),
        columns,
        blob: BlobPtr::from_array(&LOCAL_EVENT_LANE_MISMATCH_BYTES, columns.blob_len()),
        active_lane_row: PackedLaneRange::new(0, 0),
        first_active_lane: 1,
    };

    assert_invariant(|| {
        let _ = image.local_step_node(0);
    });
}

#[test]
fn resident_descriptor_rejects_missing_program_event() {
    let program: RoleProgram<0> = project(&g::send::<0, 1, Msg<1, ()>>());
    let compiled = program.role_image_ref().program;

    let mut columns = empty_columns();
    columns.events = ColumnRange::new(0, 1, ROLE_IMAGE_EVENT_STRIDE);
    columns.lanes = ColumnRange::new(
        ROLE_IMAGE_EVENT_STRIDE,
        1,
        super::super::ROLE_IMAGE_LANE_STRIDE,
    );
    let image = RoleImageRef {
        program: compiled,
        role: 0,
        facts: RuntimeRoleFacts::from_counts(RoleCompiledCounts {
            max_route_commit_count: 0,
            local_step_count: 1,
            route_scope_count: 0,
            active_lane_count: 1,
            endpoint_lane_slot_count: 1,
            logical_lane_count: 1,
        }),
        columns,
        blob: BlobPtr::from_array(&MISSING_PROGRAM_EVENT_BYTES, columns.blob_len()),
        active_lane_row: PackedLaneRange::new(0, 0),
        first_active_lane: 0,
    };

    assert_invariant(|| {
        let _ = image.local_step_node(0);
    });
}

#[test]
fn resident_descriptor_rejects_in_range_missing_event_and_lane_columns() {
    let program: RoleProgram<0> = project(&g::send::<0, 1, Msg<1, ()>>());
    let compiled = program.role_image_ref().program;
    let columns = empty_columns();
    let image = RoleImageRef {
        program: compiled,
        role: 0,
        facts: RuntimeRoleFacts::from_counts(RoleCompiledCounts {
            max_route_commit_count: 0,
            local_step_count: 1,
            route_scope_count: 0,
            active_lane_count: 1,
            endpoint_lane_slot_count: 1,
            logical_lane_count: 1,
        }),
        columns,
        blob: BlobPtr::from_array(&EMPTY_DESCRIPTOR_BYTES, columns.blob_len()),
        active_lane_row: PackedLaneRange::new(0, 0),
        first_active_lane: 0,
    };

    assert_invariant(|| {
        let _ = image.local_step_lane(0);
    });
    assert_invariant(|| {
        let _ = image.local_step_node(0);
    });
}

#[test]
fn resident_descriptor_rejects_empty_referenced_dependency_row() {
    let mut columns = empty_columns();
    columns.events = ColumnRange::new(0, 1, ROLE_IMAGE_EVENT_STRIDE);
    columns.dependencies =
        ColumnRange::new(ROLE_IMAGE_EVENT_STRIDE, 1, ROLE_IMAGE_DEPENDENCY_STRIDE);
    let image = image(&columns, &EVENT_WITH_EMPTY_DEPENDENCY_BYTES);

    assert_invariant(|| {
        let _ = image.dependency_for_index(0);
    });
}

#[test]
fn resident_descriptor_rejects_empty_referenced_conflict_row() {
    let mut columns = empty_columns();
    columns.events = ColumnRange::new(0, 1, ROLE_IMAGE_EVENT_STRIDE);
    columns.conflicts = ColumnRange::new(ROLE_IMAGE_EVENT_STRIDE, 1, ROLE_IMAGE_CONFLICT_STRIDE);
    let image = image(&columns, &EVENT_WITH_EMPTY_CONFLICT_BYTES);

    assert_invariant(|| {
        let _ = image.event_conflict_for_index(0);
    });
}

#[test]
fn resident_descriptor_rejects_out_of_domain_conflict_route_scope() {
    let mut columns = empty_columns();
    columns.events = ColumnRange::new(0, 1, ROLE_IMAGE_EVENT_STRIDE);
    columns.conflicts = ColumnRange::new(ROLE_IMAGE_EVENT_STRIDE, 1, ROLE_IMAGE_CONFLICT_STRIDE);
    let image = image(&columns, &EVENT_WITH_OUT_OF_DOMAIN_CONFLICT_BYTES);

    assert_invariant(|| {
        let _ = image.event_conflict_for_index(0);
    });
}

#[test]
fn resident_descriptor_rejects_out_of_domain_dependency_route_scope() {
    let mut columns = empty_columns();
    columns.events = ColumnRange::new(0, 1, ROLE_IMAGE_EVENT_STRIDE);
    columns.dependencies =
        ColumnRange::new(ROLE_IMAGE_EVENT_STRIDE, 1, ROLE_IMAGE_DEPENDENCY_STRIDE);
    let image = image(&columns, &EVENT_WITH_OUT_OF_DOMAIN_DEPENDENCY_BYTES);

    assert_invariant(|| {
        let _ = image.dependency_for_index(0);
    });
}

#[test]
fn resident_descriptor_rejects_dependency_range_beyond_events() {
    let mut columns = empty_columns();
    columns.events = ColumnRange::new(0, 1, ROLE_IMAGE_EVENT_STRIDE);
    columns.dependencies =
        ColumnRange::new(ROLE_IMAGE_EVENT_STRIDE, 1, ROLE_IMAGE_DEPENDENCY_STRIDE);
    let image = image(&columns, &EVENT_WITH_OUT_OF_BOUNDS_DEPENDENCY_RANGE_BYTES);

    assert_invariant(|| {
        let _ = image.dependency_for_index(0);
    });
}

#[test]
fn resident_descriptor_rejects_zero_length_resident_boundary_row() {
    let mut columns = empty_columns();
    columns.resident_boundaries = ColumnRange::new(0, 2, ROLE_IMAGE_U16_STRIDE);
    let image = image(&columns, &ZERO_LENGTH_RESIDENT_BOUNDARY_BYTES);

    assert_invariant(|| {
        let _ = image.resident_row_min_start(0);
    });
}

#[test]
fn resident_descriptor_rejects_resident_boundary_beyond_lane_rows() {
    let mut columns = empty_columns();
    columns.resident_boundaries = ColumnRange::new(0, 2, ROLE_IMAGE_U16_STRIDE);
    let image = image(&columns, &OUT_OF_BOUNDS_RESIDENT_BOUNDARY_BYTES);

    assert_invariant(|| {
        let _ = image.resident_row_min_start(0);
    });
}

#[test]
fn resident_descriptor_rejects_route_arm_event_range_beyond_events() {
    let mut columns = empty_columns();
    columns.route_arms = ColumnRange::new(0, 1, ROLE_IMAGE_ROUTE_ARM_STRIDE);
    let image = image(&columns, &ROUTE_ARM_WITH_OUT_OF_BOUNDS_EVENT_BYTES);

    assert_invariant(|| {
        let _ = image.route_arm_event_row_by_slot(0, 0);
    });
}

#[test]
fn resident_descriptor_rejects_route_arm_lane_step_range_beyond_rows() {
    let mut columns = empty_columns();
    columns.route_arms = ColumnRange::new(0, 1, ROLE_IMAGE_ROUTE_ARM_STRIDE);
    let image = image(&columns, &ROUTE_ARM_WITH_OUT_OF_BOUNDS_LANE_STEP_BYTES);

    assert_invariant(|| {
        let _ = image.route_arm_event_row_by_slot(0, 0);
    });
}

#[test]
fn resident_descriptor_rejects_passive_child_without_parent_authority() {
    let mut columns = empty_columns();
    columns.route_arms = ColumnRange::new(0, 1, ROLE_IMAGE_ROUTE_ARM_STRIDE);
    columns.route_scopes = ColumnRange::new(8, 2, ROLE_IMAGE_ROUTE_SCOPE_STRIDE);
    columns.route_scope_conflicts = ColumnRange::new(12, 2, ROLE_IMAGE_CONFLICT_STRIDE);
    columns.events = ColumnRange::new(16, 1, ROLE_IMAGE_EVENT_STRIDE);
    columns.route_arm_lane_step_rows = ColumnRange::new(
        16 + ROLE_IMAGE_EVENT_STRIDE,
        1,
        super::super::ROLE_IMAGE_ROUTE_ARM_LANE_STEP_STRIDE,
    );
    let image = image(&columns, &PASSIVE_CHILD_WITHOUT_PARENT_BYTES);

    assert_eq!(image.route_scope_by_slot(0), Some(ScopeId::route(0)));
    assert_eq!(image.route_scope_by_slot(1), Some(ScopeId::route(1)));
    let event_row = image.route_arm_event_row_by_slot(0, 0);
    assert_eq!((event_row.start(), event_row.len()), (0, 1));
    assert!(image.route_scope_conflict_by_slot(1).is_none());

    assert_invariant(|| {
        let _ = image.passive_arm_child_ordinal_by_slot(0, 0);
    });
}

#[test]
fn resident_descriptor_rejects_route_arm_lane_step_outside_own_arm() {
    let mut columns = empty_columns();
    columns.route_arms = ColumnRange::new(0, 1, ROLE_IMAGE_ROUTE_ARM_STRIDE);
    columns.route_arm_lane_step_rows = ColumnRange::new(
        ROLE_IMAGE_ROUTE_ARM_STRIDE,
        1,
        super::super::ROLE_IMAGE_ROUTE_ARM_LANE_STEP_STRIDE,
    );
    columns.events = ColumnRange::new(
        ROLE_IMAGE_ROUTE_ARM_STRIDE + super::super::ROLE_IMAGE_ROUTE_ARM_LANE_STEP_STRIDE,
        2,
        ROLE_IMAGE_EVENT_STRIDE,
    );
    let image = image(&columns, &ROUTE_ARM_WITH_FOREIGN_LANE_STEP_BYTES);

    assert_invariant(|| {
        let _ = image.route_arm_lane_first_step_by_slot(0, 0, 0, 1);
    });
}

#[test]
fn resident_descriptor_rejects_route_commit_range_beyond_rows() {
    let mut columns = empty_columns();
    columns.route_commit_ranges = ColumnRange::new(0, 1, ROLE_IMAGE_LANE_RANGE_STRIDE);
    let image = image(&columns, &OUT_OF_BOUNDS_LANE_RANGE_BYTES);

    assert_invariant(|| {
        let _ = image.route_commit_range_by_slot(0, 0);
    });
}

#[test]
fn resident_descriptor_rejects_zero_length_route_commit_range() {
    let mut columns = empty_columns();
    columns.route_commit_ranges = ColumnRange::new(0, 1, ROLE_IMAGE_LANE_RANGE_STRIDE);
    let image = image(&columns, &ZERO_LENGTH_LANE_RANGE_BYTES);

    assert_invariant(|| {
        let _ = image.route_commit_range_by_slot(0, 0);
    });
}

#[test]
fn resident_route_commit_chain_accepts_ancestor_first_order() {
    let columns = route_commit_columns(2);
    let image = image(&columns, &ROUTE_COMMIT_VALID_NESTED_BYTES);

    assert_route_commit_fixture_decodes(&image, &[(ScopeId::route(0), 0), (ScopeId::route(1), 0)]);
    let row = image.route_commit_range_by_slot(0, 0);
    assert_eq!((row.start(), row.len()), (0, 2));
}

#[test]
fn resident_descriptor_rejects_foreign_route_commit_current() {
    let columns = route_commit_columns(1);
    let image = image(&columns, &ROUTE_COMMIT_FOREIGN_CURRENT_BYTES);

    assert_route_commit_fixture_decodes(&image, &[(ScopeId::route(0), 0)]);
    assert_invariant(|| {
        let _ = image.route_commit_range_by_slot(0, 0);
    });
}

#[test]
fn resident_descriptor_rejects_foreign_route_commit_parent() {
    let columns = route_commit_columns(2);
    let image = image(&columns, &ROUTE_COMMIT_FOREIGN_PARENT_BYTES);

    assert_route_commit_fixture_decodes(&image, &[(ScopeId::route(0), 1), (ScopeId::route(1), 0)]);
    assert_invariant(|| {
        let _ = image.route_commit_range_by_slot(0, 0);
    });
}

#[test]
fn resident_descriptor_rejects_truncated_route_commit_parent_chain() {
    let columns = route_commit_columns(1);
    let image = image(&columns, &ROUTE_COMMIT_TRUNCATED_PARENT_BYTES);

    assert_route_commit_fixture_decodes(&image, &[(ScopeId::route(1), 0)]);
    assert_invariant(|| {
        let _ = image.route_commit_range_by_slot(0, 0);
    });
}

#[test]
fn resident_descriptor_rejects_route_commit_rows_before_terminal_parent() {
    let columns = route_commit_columns(3);
    let image = image(&columns, &ROUTE_COMMIT_LEADING_PARENT_BYTES);

    assert_route_commit_fixture_decodes(
        &image,
        &[
            (ScopeId::route(0), 0),
            (ScopeId::route(0), 0),
            (ScopeId::route(1), 0),
        ],
    );
    assert_invariant(|| {
        let _ = image.route_commit_range_by_slot(0, 0);
    });
}

#[test]
fn resident_descriptor_rejects_roll_scope_event_range_beyond_events() {
    let mut columns = empty_columns();
    columns.roll_scopes = ColumnRange::new(0, 1, ROLE_IMAGE_ROLL_SCOPE_STRIDE);
    let image = image(&columns, &ROLL_SCOPE_WITH_OUT_OF_BOUNDS_EVENT_BYTES);

    assert_invariant(|| {
        let _ = image.roll_scope_row(0);
    });
}
