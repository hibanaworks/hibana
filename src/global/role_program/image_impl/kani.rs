use super::{
    super::{
        ColumnRange, PackedLaneRange, PackedRouteArmRow, ROLE_IMAGE_EVENT_STRIDE, RoleImageColumns,
        RoleImagePlan, RouteArmLaneStepRow, RuntimeRoleFacts,
    },
    decode_binary_route_arm_index,
    event_rows::{
        RoleLocalDirection, decode_resident_event_header, encode_optional_event_fact_row,
        role_local_direction,
    },
    lane_image::{
        decode_resident_local_step_lane, decode_resident_roll_scope,
        decode_resident_route_arm_lane_step, decode_resident_route_scope,
        passive_child_parent_matches, route_commit_decisions_match,
    },
    metadata::{
        derive_active_lane_metadata, lane_columns_are_coherent, roll_scope_columns_are_coherent,
        route_commit_capacity_is_exact,
    },
};
use crate::global::{
    compiled::lowering::RoleCompiledCounts,
    const_dsl::{ReentryMark, ScopeId},
    typestate::PackedEventConflict,
};

fn empty_role_facts() -> RuntimeRoleFacts {
    RuntimeRoleFacts::from_counts(RoleCompiledCounts {
        max_route_commit_count: 0,
        local_step_count: 0,
        route_scope_count: 0,
        active_lane_count: 0,
        endpoint_lane_slot_count: 0,
        logical_lane_count: 0,
    })
}

fn one_event_role_image_plan() -> RoleImagePlan {
    let empty = ColumnRange::new(0, 0, 1);
    RoleImagePlan {
        columns: RoleImageColumns {
            events: ColumnRange::new(0, 1, ROLE_IMAGE_EVENT_STRIDE),
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
        },
    }
}

#[kani::proof]
fn role_image_fit_probe_rejects_undersized_storage() {
    let source = crate::global::const_dsl::EffList::<1>::new();
    let facts = empty_role_facts();
    let plan = one_event_role_image_plan();
    assert!(plan.blob_len() == ROLE_IMAGE_EVENT_STRIDE);
    assert!(
        plan.build_if_fits::<{ ROLE_IMAGE_EVENT_STRIDE - 1 }, 1>(&source, facts, 0)
            .is_none()
    );
}

#[kani::proof]
fn optional_event_fact_row_reserves_only_the_absent_u16_value() {
    let row: usize = kani::any();
    let encoded = encode_optional_event_fact_row(row);
    assert_eq!(encoded.is_some(), row < u16::MAX as usize);
    assert!(encoded.is_none_or(|encoded| usize::from(encoded) == row));
}

#[kani::proof]
#[kani::should_panic]
fn role_image_fit_probe_rejects_plan_mismatch() {
    let source = crate::global::const_dsl::EffList::<1>::new();
    let facts = RuntimeRoleFacts::from_counts(RoleCompiledCounts {
        max_route_commit_count: 0,
        local_step_count: 0,
        route_scope_count: 0,
        active_lane_count: 0,
        endpoint_lane_slot_count: 1,
        logical_lane_count: 1,
    });
    let plan = one_event_role_image_plan();
    let _ = plan.build_if_fits::<ROLE_IMAGE_EVENT_STRIDE, 1>(&source, facts, 0);
}

#[kani::proof]
fn packed_lane_range_checked_constructor_domain_is_exact() {
    let start: usize = kani::any();
    let len: usize = kani::any();

    let encodable = start <= u16::MAX as usize && len <= u16::MAX as usize - start;
    let checked = PackedLaneRange::try_new(start, len);
    assert_eq!(checked.is_some(), encodable);
    kani::cover!(encodable);
    kani::cover!(!encodable);
    kani::cover!(start == u16::MAX as usize - 1 && len == 1);
    kani::cover!(start == u16::MAX as usize && len == 1);
    kani::cover!(start == u16::MAX as usize + 1 && len == 0);
    if let Some(range) = checked {
        assert!(!range.is_empty());
        assert!(range.raw() == ((start as u32) << 16) | len as u32);
        assert!(range.start() == start);
        assert!(range.len() == len);
    }
}

#[kani::proof]
fn packed_lane_optional_range_canonicality_is_exact() {
    let raw: u32 = kani::any();
    let range = PackedLaneRange::from_raw(raw);
    let start = raw >> 16;
    let len = raw & u16::MAX as u32;

    assert!(range.is_canonical_optional_range() == (raw != u32::MAX && (len != 0 || start == 0)));
}

#[kani::proof]
fn active_lane_metadata_is_exact_for_every_first_byte_bitmap() {
    let bits: u8 = kani::any();
    let bytes = [bits];
    let decoded = derive_active_lane_metadata(
        &bytes,
        ColumnRange::new(0, 1, 1),
        PackedLaneRange::new(0, 1),
    );
    if bits == 0 {
        assert!(decoded.is_none());
        return;
    }
    let metadata = decoded.expect("nonzero first-byte bitmap is canonical");

    let expected_count = usize::from(bits & 0x01 != 0)
        + usize::from(bits & 0x02 != 0)
        + usize::from(bits & 0x04 != 0)
        + usize::from(bits & 0x08 != 0)
        + usize::from(bits & 0x10 != 0)
        + usize::from(bits & 0x20 != 0)
        + usize::from(bits & 0x40 != 0)
        + usize::from(bits & 0x80 != 0);
    assert_eq!(metadata.active_lane_count, expected_count);
    let expected_logical = if bits & 0x80 != 0 {
        8
    } else if bits & 0x40 != 0 {
        7
    } else if bits & 0x20 != 0 {
        6
    } else if bits & 0x10 != 0 {
        5
    } else if bits & 0x08 != 0 {
        4
    } else if bits & 0x04 != 0 {
        3
    } else if bits & 0x02 != 0 {
        2
    } else {
        1
    };
    let expected_first = if bits & 0x01 != 0 {
        0
    } else if bits & 0x02 != 0 {
        1
    } else if bits & 0x04 != 0 {
        2
    } else if bits & 0x08 != 0 {
        3
    } else if bits & 0x10 != 0 {
        4
    } else if bits & 0x20 != 0 {
        5
    } else if bits & 0x40 != 0 {
        6
    } else {
        7
    };
    assert_eq!(metadata.logical_lane_count, expected_logical);
    assert_eq!(metadata.first_active_lane, expected_first);
}

#[kani::proof]
fn inactive_lane_metadata_has_one_empty_representation() {
    let bytes: [u8; 0] = [];
    let decoded = derive_active_lane_metadata(
        &bytes,
        ColumnRange::new(0, 0, 1),
        PackedLaneRange::new(0, 0),
    )
    .expect("zero-width active row is canonical");

    assert_eq!(decoded.active_lane_count, 0);
    assert_eq!(decoded.logical_lane_count, 1);
    assert_eq!(decoded.first_active_lane, u16::MAX);
}

#[kani::proof]
fn active_lane_metadata_preserves_the_high_lane_boundary() {
    let bits: u8 = kani::any();
    let mut bytes = [0u8; 32];
    bytes[31] = bits;
    let decoded = derive_active_lane_metadata(
        &bytes,
        ColumnRange::new(0, 32, 1),
        PackedLaneRange::new(0, 32),
    );
    if bits == 0 {
        assert!(decoded.is_none());
        return;
    }
    let metadata = decoded.expect("canonical high-byte bitmap");

    assert_eq!(metadata.active_lane_count, bits.count_ones() as usize);
    assert_eq!(
        metadata.logical_lane_count,
        248 + 8 - bits.leading_zeros() as usize
    );
    assert_eq!(
        metadata.first_active_lane,
        248 + bits.trailing_zeros() as u16
    );
}

#[kani::proof]
fn lane_column_coherence_is_exact_for_one_binary_route() {
    let active: u8 = kani::any();
    let left: u8 = kani::any();
    let right: u8 = kani::any();
    let offer: u8 = kani::any();
    let left_lane: u8 = kani::any();
    let right_lane: u8 = kani::any();
    let mut bytes = [0u8; 64];
    bytes[20] = 0;
    bytes[21] = 1;
    bytes[22] = active;
    bytes[23] = left;
    bytes[24] = right;
    bytes[25] = offer;
    bytes[26..30].copy_from_slice(&((1u32 << 16) | 1).to_le_bytes());
    bytes[30..34].copy_from_slice(&((2u32 << 16) | 1).to_le_bytes());
    bytes[34..38].copy_from_slice(&((3u32 << 16) | 1).to_le_bytes());
    bytes[38..42].copy_from_slice(&1u32.to_le_bytes());
    bytes[42..46].copy_from_slice(&(u16::MAX as u32).to_le_bytes());
    bytes[46..50].copy_from_slice(&((1u32 << 16) | 1).to_le_bytes());
    bytes[50..54].copy_from_slice(&(u16::MAX as u32).to_le_bytes());
    bytes[54] = left_lane;
    bytes[59] = right_lane;
    bytes[60..62].copy_from_slice(&1u16.to_le_bytes());
    bytes[62..64].copy_from_slice(&1u16.to_le_bytes());

    let empty = ColumnRange::new(bytes.len(), 0, 1);
    let columns = RoleImageColumns {
        events: ColumnRange::new(0, 2, ROLE_IMAGE_EVENT_STRIDE),
        lanes: ColumnRange::new(20, 2, super::super::ROLE_IMAGE_LANE_STRIDE),
        dependencies: empty,
        conflicts: empty,
        route_scopes: empty,
        route_scope_conflicts: empty,
        route_arms: ColumnRange::new(38, 2, super::super::ROLE_IMAGE_ROUTE_ARM_STRIDE),
        resident_boundaries: empty,
        lane_bits: ColumnRange::new(22, 4, 1),
        route_arm_lane_rows: ColumnRange::new(26, 2, super::super::ROLE_IMAGE_LANE_RANGE_STRIDE),
        route_offer_lane_rows: ColumnRange::new(34, 1, super::super::ROLE_IMAGE_LANE_RANGE_STRIDE),
        route_arm_lane_step_rows: ColumnRange::new(
            54,
            2,
            super::super::ROLE_IMAGE_ROUTE_ARM_LANE_STEP_STRIDE,
        ),
        route_commit_ranges: empty,
        route_commit_rows: empty,
        roll_scopes: empty,
    };
    let footprint = super::super::RuntimeRoleFootprint {
        max_route_commit_count: 0,
        route_arm_state_capacity: 2,
        local_step_count: 2,
        route_scope_count: 1,
        active_lane_count: active.count_ones() as usize,
        endpoint_lane_slot_count: 8,
        logical_lane_count: 8,
    };
    let accepted =
        lane_columns_are_coherent(&bytes, columns, PackedLaneRange::new(0, 1), footprint);
    let expected = active == 0b11
        && left != 0
        && right != 0
        && offer != 0
        && left & !active == 0
        && right & !active == 0
        && offer == left | right
        && left_lane == 0
        && right_lane == 1
        && left == 1
        && right == 2;

    assert_eq!(accepted, expected);
}

#[kani::proof]
fn route_commit_partition_requires_the_exact_builder_maximum() {
    let max_route_commit_count: u16 = kani::any();
    let bytes = [1, 0, 0, 0, 2, 0, 1, 0];
    let accepted = route_commit_capacity_is_exact(
        &bytes,
        ColumnRange::new(0, 2, 4),
        3,
        max_route_commit_count as usize,
    );

    assert_eq!(accepted, max_route_commit_count == 2);
}

#[kani::proof]
fn two_roll_scope_rows_are_accepted_exactly_when_unique_and_laminar() {
    let left_scope: u16 = kani::any();
    let right_scope: u16 = kani::any();
    let left_start: u8 = kani::any();
    let left_len: u8 = kani::any();
    let right_start: u8 = kani::any();
    let right_len: u8 = kani::any();
    let mut bytes = [0u8; 2 * super::super::ROLE_IMAGE_ROLL_SCOPE_STRIDE];
    bytes[0..2].copy_from_slice(&left_scope.to_le_bytes());
    bytes[2..6].copy_from_slice(&(((left_start as u32) << 16) | left_len as u32).to_le_bytes());
    let right_offset = super::super::ROLE_IMAGE_ROLL_SCOPE_STRIDE;
    bytes[right_offset..right_offset + 2].copy_from_slice(&right_scope.to_le_bytes());
    bytes[right_offset + 2..right_offset + 6]
        .copy_from_slice(&(((right_start as u32) << 16) | right_len as u32).to_le_bytes());

    let left_end = left_start as usize + left_len as usize;
    let right_end = right_start as usize + right_len as usize;
    let each_row_valid = left_scope < ScopeId::LOCAL_CAPACITY
        && right_scope < ScopeId::LOCAL_CAPACITY
        && left_len != 0
        && right_len != 0
        && left_end <= 256
        && right_end <= 256;
    let laminar = left_end <= right_start as usize
        || right_end <= left_start as usize
        || (left_start <= right_start && right_end <= left_end)
        || (right_start <= left_start && left_end <= right_end);
    let expected = each_row_valid && left_scope != right_scope && laminar;
    let accepted = roll_scope_columns_are_coherent(
        &bytes,
        ColumnRange::new(0, 2, super::super::ROLE_IMAGE_ROLL_SCOPE_STRIDE),
        256,
    );

    assert_eq!(accepted, expected);
}

#[kani::proof]
#[kani::should_panic]
fn packed_lane_range_infallible_constructor_rejects_first_end_overflow() {
    let _ = PackedLaneRange::new(u16::MAX as usize, 1);
}

#[kani::proof]
fn resident_route_arm_index_decoding_accepts_exact_binary_domain() {
    let raw: u8 = kani::any();
    let decoded = decode_binary_route_arm_index(raw);

    assert!(decoded.is_some() == (raw <= 1));
    if let Some(index) = decoded {
        assert!(index == raw as usize);
    }
}

#[kani::proof]
fn resident_route_scope_decoding_accepts_exact_domain() {
    let raw: u16 = kani::any();
    let decoded = decode_resident_route_scope(raw);
    let expected = raw < ScopeId::LOCAL_CAPACITY;

    assert!(decoded.is_some() == expected);
    if let Some(scope) = decoded {
        assert!(scope.raw() == raw);
        assert!(scope.local_ordinal() == raw);
    }
}

#[kani::proof]
fn resident_roll_scope_decoding_accepts_exact_domain() {
    let raw_ordinal: u16 = kani::any();
    let decoded = decode_resident_roll_scope(raw_ordinal);
    let expected = raw_ordinal < ScopeId::LOCAL_CAPACITY;

    assert!(decoded.is_some() == expected);
    if let Some(scope) = decoded {
        assert!(scope.local_ordinal() == raw_ordinal);
    }
}

#[kani::proof]
fn resident_event_header_decoding_accepts_exact_domain() {
    let eff_index: u16 = kani::any();
    let scope_raw: u16 = kani::any();
    let flags: u8 = kani::any();
    let scope_kind = (scope_raw >> 13) & 0b11;
    let scope_valid = scope_raw == u16::MAX || ((scope_raw & 0x8000) == 0 && scope_kind <= 2);
    let expected = (eff_index as usize) < crate::eff::meta::COMPACT_EVENT_IDENTITY_CAPACITY
        && flags <= 1
        && scope_valid;
    let decoded = decode_resident_event_header(eff_index, scope_raw, flags);

    assert!(decoded.is_some() == expected);
    if let Some(scope) = decoded {
        assert!(scope.raw() == scope_raw);
    }
}

#[kani::proof]
fn resident_local_step_lane_decoding_accepts_exact_domain() {
    let raw: u8 = kani::any();
    let logical_lane_count: u16 = kani::any();
    let expected = logical_lane_count != 0
        && logical_lane_count as usize <= super::super::LANE_DOMAIN_SIZE
        && (raw as u16) < logical_lane_count;
    let decoded = decode_resident_local_step_lane(raw, logical_lane_count as usize);

    assert!(decoded.is_some() == expected);
    if let Some(lane) = decoded {
        assert!(lane == raw);
    }
}

#[kani::proof]
fn resident_role_local_direction_is_exact_over_the_full_role_domain() {
    let role: u8 = kani::any();
    let from: u8 = kani::any();
    let to: u8 = kani::any();
    let direction = role_local_direction(role, from, to);

    assert!(direction.is_some() == (from == role || to == role));
    match direction {
        Some(RoleLocalDirection::Local) => {
            assert!(from == role && to == role);
        }
        Some(RoleLocalDirection::Send) => {
            assert!(from == role && to != role);
        }
        Some(RoleLocalDirection::Recv) => {
            assert!(from != role && to == role);
        }
        None => {
            assert!(from != role && to != role);
        }
    }
}

#[kani::proof]
fn resident_route_commit_decision_match_is_exact() {
    let left_scope: u16 = kani::any();
    let right_scope: u16 = kani::any();
    let left_arm: u8 = kani::any();
    let right_arm: u8 = kani::any();
    let left_mark: ReentryMark = kani::any();
    let right_mark: ReentryMark = kani::any();

    let valid = left_scope < ScopeId::LOCAL_CAPACITY
        && right_scope < ScopeId::LOCAL_CAPACITY
        && left_arm <= 1
        && right_arm <= 1;
    kani::cover!(valid && left_scope == right_scope && left_arm == right_arm);
    kani::cover!(valid && left_scope != right_scope);
    kani::cover!(valid && left_scope == right_scope && left_arm != right_arm);
    kani::cover!(
        valid && left_scope == right_scope && left_arm == right_arm && left_mark != right_mark
    );
    if valid {
        let left = PackedEventConflict::route_arm(ScopeId::route(left_scope), left_arm)
            .with_route_reentry(left_mark);
        let right = PackedEventConflict::route_arm(ScopeId::route(right_scope), right_arm)
            .with_route_reentry(right_mark);

        assert!(
            route_commit_decisions_match(left, right)
                == (left_scope == right_scope && left_arm == right_arm)
        );
    }
}

#[kani::proof]
fn resident_passive_child_parent_binding_is_exact() {
    let parent_ordinal: u16 = kani::any();
    let child_ordinal: u16 = kani::any();
    let arm: u8 = kani::any();
    let recorded_parent_ordinal: u16 = kani::any();
    let recorded_arm: u8 = kani::any();

    let valid = parent_ordinal < ScopeId::LOCAL_CAPACITY
        && child_ordinal < ScopeId::LOCAL_CAPACITY
        && arm <= 1
        && recorded_parent_ordinal < ScopeId::LOCAL_CAPACITY
        && recorded_arm <= 1;
    if valid {
        let parent = ScopeId::route(parent_ordinal);
        let child = ScopeId::route(child_ordinal);
        let recorded =
            PackedEventConflict::route_arm(ScopeId::route(recorded_parent_ordinal), recorded_arm);
        assert!(
            passive_child_parent_matches(parent, arm, child, recorded)
                == (child_ordinal != parent_ordinal
                    && recorded_parent_ordinal == parent_ordinal
                    && recorded_arm == arm)
        );
    }
}

#[kani::proof]
fn resident_route_arm_lane_step_decoding_accepts_exact_domain() {
    let lane: u8 = kani::any();
    let first_step: u16 = kani::any();
    let last_step: u16 = kani::any();
    let logical_lane_count: u16 = kani::any();
    let event_raw: u32 = kani::any();
    let event_start = (event_raw >> 16) as u16;
    let event_len = event_raw as u16;
    let event_end = event_start as usize + event_len as usize;
    let expected = logical_lane_count != 0
        && logical_lane_count as usize <= super::super::LANE_DOMAIN_SIZE
        && (lane as u16) < logical_lane_count
        && event_raw != u32::MAX
        && event_len != 0
        && event_start <= first_step
        && first_step <= last_step
        && (last_step as usize) < event_end;
    let decoded = decode_resident_route_arm_lane_step(
        RouteArmLaneStepRow::from_packed_parts(lane, first_step, last_step),
        logical_lane_count as usize,
        PackedLaneRange::from_raw(event_raw),
    );

    assert!(decoded.is_some() == expected);
    if let Some(row) = decoded {
        assert!(row.lane() == lane);
        assert!(row.first_step() == first_step);
        assert!(row.last_step() == last_step);
    }
}

#[kani::proof]
fn packed_route_arm_row_round_trips_full_compact_domains() {
    let event_start: u16 = kani::any();
    let event_len: u16 = kani::any();
    let lane_step_start: u16 = kani::any();
    let lane_step_len: u16 = kani::any();
    let child_slot: u16 = kani::any();
    let event_raw = ((event_start as u32) << 16) | event_len as u32;

    let representable = event_raw != u32::MAX
        && (event_len != 0 || event_start == 0)
        && u32::from(event_start) + u32::from(event_len) <= u32::from(u16::MAX)
        && u32::from(lane_step_start) + u32::from(lane_step_len) <= u32::from(u16::MAX)
        && lane_step_len as usize <= super::super::LANE_DOMAIN_SIZE
        && (event_len == 0) == (lane_step_len == 0);
    kani::cover!(representable);
    kani::cover!(!representable);
    if representable {
        let event_row = PackedLaneRange::new(event_start as usize, event_len as usize);
        let lane_step_row = PackedLaneRange::new(lane_step_start as usize, lane_step_len as usize);
        let child = if child_slot == u16::MAX {
            None
        } else {
            Some(child_slot as usize)
        };
        let packed = PackedRouteArmRow::new(event_row, child, lane_step_row);

        assert!(packed.event_row_raw() == event_raw);
        assert!(packed.event_row().start() == event_start as usize);
        assert!(packed.event_row().len() == event_len as usize);
        assert!(packed.lane_step_len() == lane_step_len as usize);
        let expected_child = if child_slot == u16::MAX {
            None
        } else {
            Some(child_slot)
        };
        assert!(packed.child_slot() == expected_child);
        kani::cover!(event_start == 4095 && event_len == 1);
        kani::cover!(event_start == 4096 && event_len == 1);
        kani::cover!(lane_step_len as usize == super::super::LANE_DOMAIN_SIZE);
    }
}
