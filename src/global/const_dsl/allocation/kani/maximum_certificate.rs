use super::super::lane_matching::{
    LaneEndpointIndex, maximum_lane_matching, validate_maximum_certificate,
};

// Projection runs this exact production const function. Exhaust all 512
// matrices during const evaluation, including all of its bounds, arithmetic
// and certificate checks. Kani independently checks the compact results rather
// than expanding the 256-entry work arrays into a solver formula each time.
const fn production_result(conflicts: u16) -> [u16; 3] {
    let left = LaneEndpointIndex::<2>::from_three_role_masks([
        0b000_000_111,
        0b000_111_000,
        0b111_000_000,
    ]);
    let right = LaneEndpointIndex::<2>::from_three_role_masks([
        conflicts & 0b001_001_001,
        conflicts & 0b010_010_010,
        conflicts & 0b100_100_100,
    ]);
    let matching = maximum_lane_matching(&left, &right, 3, 3);
    validate_maximum_certificate(&left, &right, 3, 3, &matching);
    let mut result = [u16::MAX; 3];
    let mut index = 0;
    while index < 3 {
        result[index] = match matching.left_for_right(index as u16) {
            Some(left) => left,
            None => u16::MAX,
        };
        index += 1;
    }
    result
}

const PRODUCTION_RESULTS: [[u16; 3]; 512] = {
    let mut results = [[u16::MAX; 3]; 512];
    let mut conflicts = 0;
    while conflicts < results.len() {
        results[conflicts] = production_result(conflicts as u16);
        conflicts += 1;
    }
    results
};

#[kani::proof]
#[kani::unwind(8)]
fn three_by_three_parallel_lane_matching_certificate_is_maximum() {
    let conflicts = kani::any::<u16>() & 0x01ff;
    let matching = PRODUCTION_RESULTS[conflicts as usize];
    let mut actual = 0u16;
    let mut right = 0;
    while right < 3 {
        let left = matching[right];
        if left != u16::MAX {
            assert!(left < 3);
            assert!(conflicts & (1u16 << (left as usize * 3 + right)) == 0);
            let mut earlier = 0;
            while earlier < right {
                assert!(matching[earlier] != left);
                earlier += 1;
            }
            actual += 1;
        }
        right += 1;
    }

    // Every partial injection is universally quantified. Value 3 is unmatched.
    // A valid production matching at least as large as every valid candidate
    // has exactly the exhaustive maximum. No input or safety check is removed.
    let candidate = [
        kani::any::<u8>() & 3,
        kani::any::<u8>() & 3,
        kani::any::<u8>() & 3,
    ];
    let mut valid = true;
    let mut size = 0u16;
    let mut right = 0;
    while right < 3 {
        let left = candidate[right] as usize;
        if left < 3 {
            if conflicts & (1u16 << (left * 3 + right)) != 0 {
                valid = false;
            }
            let mut earlier = 0;
            while earlier < right {
                if candidate[earlier] == candidate[right] {
                    valid = false;
                }
                earlier += 1;
            }
            size += 1;
        }
        right += 1;
    }
    assert!(!valid || size <= actual);
}
