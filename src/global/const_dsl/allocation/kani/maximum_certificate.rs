use super::super::lane_matching::{
    LaneEndpointIndex, maximum_lane_matching, validate_maximum_certificate,
};

#[kani::proof]
#[kani::unwind(8)]
fn three_by_three_parallel_lane_matching_certificate_is_maximum() {
    let conflicts = kani::any::<u16>() & 0x01ff;
    let left_index = LaneEndpointIndex::<2>::from_three_role_masks([
        0b000_000_111,
        0b000_111_000,
        0b111_000_000,
    ]);
    let right_index = LaneEndpointIndex::<2>::from_three_role_masks([
        (conflicts & 0b001) | (conflicts & 0b001_000) | (conflicts & 0b001_000_000),
        (conflicts & 0b010) | (conflicts & 0b010_000) | (conflicts & 0b010_000_000),
        (conflicts & 0b100) | (conflicts & 0b100_000) | (conflicts & 0b100_000_000),
    ]);

    let matching = maximum_lane_matching(&left_index, &right_index, 3, 3);
    validate_maximum_certificate(&left_index, &right_index, 3, 3, &matching);
    let actual = matching.left_for_right(0).is_some() as u16
        + matching.left_for_right(1).is_some() as u16
        + matching.left_for_right(2).is_some() as u16;

    // Independently enumerate every partial injection. Value 3 is unmatched.
    let mut expected = 0u16;
    let mut first = 0u16;
    while first <= 3 {
        let mut second = 0u16;
        while second <= 3 {
            let mut third = 0u16;
            while third <= 3 {
                let assignments = [first, second, third];
                let mut valid = true;
                let mut size = 0u16;
                let mut right = 0usize;
                while right < 3 {
                    let left = assignments[right];
                    if left < 3 {
                        let conflict_bit = 1u16 << (left as usize * 3 + right);
                        if conflicts & conflict_bit != 0 {
                            valid = false;
                        }
                        let mut earlier = 0usize;
                        while earlier < right {
                            if assignments[earlier] == left {
                                valid = false;
                            }
                            earlier += 1;
                        }
                        size += 1;
                    }
                    right += 1;
                }
                if valid && size > expected {
                    expected = size;
                }
                third += 1;
            }
            second += 1;
        }
        first += 1;
    }

    assert!(actual == expected);
}
