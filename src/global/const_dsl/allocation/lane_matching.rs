use super::{BYTE_DOMAIN, BYTE_DOMAIN_MASK_BYTES};
use crate::global::const_dsl::EffList;

#[cfg(kani)]
mod certificate;
mod index;

pub(super) use index::LaneEndpointIndex;

const fn validate_lane_span<const E: usize>(
    eff_list: &EffList<E>,
    start: usize,
    end: usize,
    lane_span: u16,
) {
    let mut idx = start;
    while idx < end {
        if eff_list.atom_at(idx).lane as u16 >= lane_span {
            panic!("parallel arm contains a lane outside its declared span");
        }
        idx += 1;
    }
}

const NO_MATCH: u16 = u16::MAX;

const fn validate_dense_lane_classes<const ROLE_BYTES: usize>(
    index: &LaneEndpointIndex<ROLE_BYTES>,
    lane_span: u16,
) {
    let mut lane = 0;
    while lane < lane_span {
        if !index.contains_lane(lane as u8) {
            panic!("parallel arm lane classes must be dense");
        }
        lane += 1;
    }
}

/// Extend a maximum bipartite matching by one right-hand lane class.
///
/// The alternating-path search is deterministic: right and left classes are
/// visited in ascending wire order. Every successful extension increases the
/// number of reused left lanes by exactly one.
const fn augment_lane_matching<const ROLE_BYTES: usize>(
    left_index: &LaneEndpointIndex<ROLE_BYTES>,
    right_index: &LaneEndpointIndex<ROLE_BYTES>,
    left_lane_span: u16,
    right_lane: u16,
    seen_left: &mut [bool; BYTE_DOMAIN],
    right_to_left: &mut [u16; BYTE_DOMAIN],
    left_to_right: &mut [u16; BYTE_DOMAIN],
) -> bool {
    let mut right_stack = [0u16; BYTE_DOMAIN];
    let mut next_left = [0u16; BYTE_DOMAIN];
    let mut parent_left = [NO_MATCH; BYTE_DOMAIN];
    let mut depth = 0usize;
    right_stack[0] = right_lane;

    loop {
        let active_right = right_stack[depth];
        let right_roles = right_index.endpoint_set(active_right as u8);
        let mut selected = None;
        while next_left[depth] < left_lane_span {
            let left_lane = next_left[depth];
            next_left[depth] += 1;
            let left_slot = left_lane as usize;
            if !seen_left[left_slot]
                && right_roles.is_disjoint(left_index.endpoint_set(left_lane as u8))
            {
                seen_left[left_slot] = true;
                selected = Some(left_lane);
                break;
            }
        }

        match selected {
            Some(left_lane) => {
                let occupant = left_to_right[left_lane as usize];
                if occupant == NO_MATCH {
                    let mut assigned_left = left_lane;
                    loop {
                        let assigned_right = right_stack[depth];
                        right_to_left[assigned_right as usize] = assigned_left;
                        left_to_right[assigned_left as usize] = assigned_right;
                        if depth == 0 {
                            return true;
                        }
                        assigned_left = parent_left[depth];
                        if assigned_left == NO_MATCH {
                            panic!("parallel lane matching path is incomplete");
                        }
                        depth -= 1;
                    }
                }
                if depth + 1 >= BYTE_DOMAIN {
                    panic!("parallel lane matching path exceeds wire domain");
                }
                depth += 1;
                right_stack[depth] = occupant;
                next_left[depth] = 0;
                parent_left[depth] = left_lane;
            }
            None if depth == 0 => return false,
            None => depth -= 1,
        }
    }
}

pub(super) struct LaneMatching {
    right_to_left: [u16; BYTE_DOMAIN],
}

impl LaneMatching {
    pub(super) const fn left_for_right(&self, right_lane: u16) -> Option<u16> {
        let left_lane = self.right_to_left[right_lane as usize];
        if left_lane == NO_MATCH {
            None
        } else {
            Some(left_lane)
        }
    }
}

pub(super) const fn maximum_lane_matching<const ROLE_BYTES: usize>(
    left_index: &LaneEndpointIndex<ROLE_BYTES>,
    right_index: &LaneEndpointIndex<ROLE_BYTES>,
    left_lane_span: u16,
    right_lane_span: u16,
) -> LaneMatching {
    if left_lane_span as usize > BYTE_DOMAIN || right_lane_span as usize > BYTE_DOMAIN {
        panic!("parallel lane matching span exceeds wire domain");
    }

    let mut right_to_left = [NO_MATCH; BYTE_DOMAIN];
    let mut left_to_right = [NO_MATCH; BYTE_DOMAIN];
    let mut right_lane = 0u16;
    while right_lane < right_lane_span {
        let mut seen_left = [false; BYTE_DOMAIN];
        let _ = augment_lane_matching(
            left_index,
            right_index,
            left_lane_span,
            right_lane,
            &mut seen_left,
            &mut right_to_left,
            &mut left_to_right,
        );
        right_lane += 1;
    }
    LaneMatching { right_to_left }
}

#[cfg(kani)]
pub(in super::super) const fn validate_maximum_certificate<const ROLE_BYTES: usize>(
    left_index: &LaneEndpointIndex<ROLE_BYTES>,
    right_index: &LaneEndpointIndex<ROLE_BYTES>,
    left_lane_span: u16,
    right_lane_span: u16,
    matching: &LaneMatching,
) {
    certificate::validate_maximum(
        left_index,
        right_index,
        left_lane_span,
        right_lane_span,
        matching,
    );
}

/// Merge two already valid lane colorings at one parallel composition.
///
/// Right-hand color classes remain distinct, preserving every conflict already
/// established inside that arm. Maximum bipartite matching reuses the greatest
/// possible number of left-hand lanes whose endpoint-role sets are disjoint.
/// Fresh colors are allocated only for unmatched right-hand classes, so source
/// order cannot cause a false wire-domain rejection.
pub(crate) const fn merge_parallel_lanes<const E: usize>(
    eff_list: &mut EffList<E>,
    left_start: usize,
    left_end: usize,
    right_end: usize,
    left_lane_span: u16,
    right_lane_span: u16,
) -> u16 {
    if left_start >= left_end
        || left_end >= right_end
        || right_end > eff_list.len()
        || left_lane_span == 0
        || right_lane_span == 0
    {
        panic!("parallel lane merge requires two non-empty colored arms");
    }
    if left_lane_span > BYTE_DOMAIN as u16 || right_lane_span > BYTE_DOMAIN as u16 {
        panic!("parallel lane span exceeds wire domain");
    }

    validate_lane_span(eff_list, left_start, left_end, left_lane_span);
    validate_lane_span(eff_list, left_end, right_end, right_lane_span);

    let left_index =
        LaneEndpointIndex::<BYTE_DOMAIN_MASK_BYTES>::from_range(eff_list, left_start, left_end);
    let right_index =
        LaneEndpointIndex::<BYTE_DOMAIN_MASK_BYTES>::from_range(eff_list, left_end, right_end);
    validate_dense_lane_classes(&left_index, left_lane_span);
    validate_dense_lane_classes(&right_index, right_lane_span);

    let matching =
        maximum_lane_matching(&left_index, &right_index, left_lane_span, right_lane_span);

    let mut remap = [0u8; BYTE_DOMAIN];
    let mut result_span = left_lane_span;
    let mut right_lane = 0;
    while right_lane < right_lane_span {
        if let Some(matched) = matching.left_for_right(right_lane) {
            remap[right_lane as usize] = matched as u8;
        } else {
            if result_span >= BYTE_DOMAIN as u16 {
                panic!("parallel endpoint lane coloring exceeds wire domain");
            }
            remap[right_lane as usize] = result_span as u8;
            result_span += 1;
        }
        right_lane += 1;
    }

    let mut idx = left_end;
    while idx < right_end {
        let mut atom = eff_list.atom_at(idx);
        atom.lane = remap[atom.lane as usize];
        eff_list.replace_atom(idx, atom);
        idx += 1;
    }
    result_span
}
