use super::{
    super::{PackedLaneRange, RouteArmLaneStepRow},
    decode_binary_route_arm_index,
    event_rows::decode_resident_event_header,
    lane_image::{
        decode_resident_local_step_lane, decode_resident_roll_scope,
        decode_resident_route_arm_lane_step, decode_resident_route_scope,
        route_commit_decisions_match,
    },
};
use crate::global::{
    const_dsl::{ReentryMark, ScopeId},
    typestate::PackedEventConflict,
};

#[kani::proof]
fn packed_lane_range_encoding_avoids_reserved_sentinel() {
    let start: u16 = kani::any();
    let len: u16 = kani::any();

    if start != u16::MAX || len != u16::MAX {
        let range = PackedLaneRange::new(start as usize, len as usize);
        assert!(!range.is_empty());
        assert!(range.raw() == ((start as u32) << 16) | len as u32);
        assert!(range.start() == start as usize);
        assert!(range.len() == len as usize);
    }
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
    let expected = (raw as usize) < crate::eff::meta::MAX_EFF_NODES;

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
    let expected = (raw_ordinal as usize) < crate::eff::meta::MAX_EFF_NODES;

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
    let scope_ordinal = scope_raw & 0x1fff;
    let scope_kind = (scope_raw >> 13) & 0b11;
    let scope_valid = scope_raw == u16::MAX
        || ((scope_raw & 0x8000) == 0
            && scope_kind <= 2
            && (scope_ordinal as usize) < crate::eff::meta::MAX_EFF_NODES);
    let expected =
        (eff_index as usize) < crate::eff::meta::MAX_EFF_NODES && flags <= 1 && scope_valid;
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
fn resident_route_commit_decision_match_is_exact() {
    let left_scope: u16 = kani::any();
    let right_scope: u16 = kani::any();
    let left_arm: u8 = kani::any();
    let right_arm: u8 = kani::any();
    let left_mark_raw: u8 = kani::any();
    let right_mark_raw: u8 = kani::any();

    if (left_scope as usize) < crate::eff::meta::MAX_EFF_NODES
        && (right_scope as usize) < crate::eff::meta::MAX_EFF_NODES
        && left_arm <= 1
        && right_arm <= 1
        && left_mark_raw <= 1
        && right_mark_raw <= 1
    {
        let left_mark = if left_mark_raw == 0 {
            ReentryMark::SinglePass
        } else {
            ReentryMark::Reentrant
        };
        let right_mark = if right_mark_raw == 0 {
            ReentryMark::SinglePass
        } else {
            ReentryMark::Reentrant
        };
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
        && event_end <= crate::eff::meta::MAX_EFF_NODES
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
