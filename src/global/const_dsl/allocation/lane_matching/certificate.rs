use super::{LaneEndpointIndex, LaneMatching, NO_MATCH};
use crate::global::const_dsl::allocation::BYTE_DOMAIN;

pub(super) const fn validate_maximum<const ROLE_BYTES: usize>(
    left_index: &LaneEndpointIndex<ROLE_BYTES>,
    right_index: &LaneEndpointIndex<ROLE_BYTES>,
    left_lane_span: u16,
    right_lane_span: u16,
    matching: &LaneMatching,
) {
    let right_to_left = &matching.right_to_left;
    let mut left_to_right = [NO_MATCH; BYTE_DOMAIN];
    let mut right_lane = 0u16;
    while right_lane < right_lane_span {
        let matched_left = right_to_left[right_lane as usize];
        if matched_left != NO_MATCH {
            if matched_left >= left_lane_span || left_to_right[matched_left as usize] != NO_MATCH {
                panic!("parallel lane matching certificate is not one-to-one");
            }
            left_to_right[matched_left as usize] = right_lane;
        }
        right_lane += 1;
    }

    let mut reachable_right = [false; BYTE_DOMAIN];
    let mut right_queue = [0u16; BYTE_DOMAIN];
    let mut queue_len = 0usize;
    right_lane = 0;
    while right_lane < right_lane_span {
        if right_to_left[right_lane as usize] == NO_MATCH {
            reachable_right[right_lane as usize] = true;
            right_queue[queue_len] = right_lane;
            queue_len += 1;
        }
        right_lane += 1;
    }

    let mut reachable_left = [false; BYTE_DOMAIN];
    let mut queue_head = 0usize;
    while queue_head < queue_len {
        right_lane = right_queue[queue_head];
        queue_head += 1;
        let right_roles = right_index.endpoint_set(right_lane as u8);
        let mut left_lane = 0u16;
        while left_lane < left_lane_span {
            let is_matching_edge = right_to_left[right_lane as usize] == left_lane;
            let reusable = right_roles.is_disjoint(left_index.endpoint_set(left_lane as u8));
            if !is_matching_edge && reusable && !reachable_left[left_lane as usize] {
                reachable_left[left_lane as usize] = true;
                let matched_right = left_to_right[left_lane as usize];
                if matched_right != NO_MATCH && !reachable_right[matched_right as usize] {
                    if queue_len >= BYTE_DOMAIN {
                        panic!("parallel lane cover traversal exceeds wire domain");
                    }
                    reachable_right[matched_right as usize] = true;
                    right_queue[queue_len] = matched_right;
                    queue_len += 1;
                }
            }
            left_lane += 1;
        }
    }

    let mut matching_size = 0u16;
    let mut cover_size = 0u16;
    right_lane = 0;
    while right_lane < right_lane_span {
        let matched_left = right_to_left[right_lane as usize];
        if matched_left != NO_MATCH {
            if matched_left >= left_lane_span
                || left_to_right[matched_left as usize] != right_lane
                || !right_index
                    .endpoint_set(right_lane as u8)
                    .is_disjoint(left_index.endpoint_set(matched_left as u8))
            {
                panic!("parallel lane matching certificate contains an invalid edge");
            }
            matching_size += 1;
        }
        if !reachable_right[right_lane as usize] {
            cover_size += 1;
        }
        right_lane += 1;
    }

    let mut left_lane = 0u16;
    while left_lane < left_lane_span {
        if reachable_left[left_lane as usize] {
            cover_size += 1;
        }
        left_lane += 1;
    }

    right_lane = 0;
    while right_lane < right_lane_span {
        let right_roles = right_index.endpoint_set(right_lane as u8);
        left_lane = 0;
        while left_lane < left_lane_span {
            let reusable = right_roles.is_disjoint(left_index.endpoint_set(left_lane as u8));
            let covered =
                !reachable_right[right_lane as usize] || reachable_left[left_lane as usize];
            if reusable && !covered {
                panic!("parallel lane matching certificate leaves a reusable edge uncovered");
            }
            left_lane += 1;
        }
        right_lane += 1;
    }

    if matching_size != cover_size {
        panic!("parallel lane matching certificate is not maximum");
    }
}
