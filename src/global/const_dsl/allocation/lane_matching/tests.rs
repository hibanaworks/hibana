use super::*;

#[test]
fn explicit_constraints_reach_every_wire_lane_pair_without_aliasing() {
    let mut rows = [0u8; BYTE_DOMAIN * BYTE_DOMAIN_MASK_BYTES];
    for right in 0..256u16 {
        for left in 0..256u16 {
            let offset = usize::from(right) * BYTE_DOMAIN_MASK_BYTES + usize::from(left) / 8;
            rows[offset] = 1 << (left % 8);
            let graph: LaneCompatibility<'_, 0> = LaneCompatibility::Constraints {
                rows: &rows,
                row_bytes: 32,
            };
            assert!(graph.allows(right, left));
            assert!(!graph.allows(right, (left + 1) % 256));
            assert!(!graph.allows((right + 1) % 256, left));
            rows[offset] = 0;
        }
    }
}

#[test]
fn constrained_matching_is_maximum_for_all_three_by_three_graphs() {
    for edges in 0..512u16 {
        let rows = [
            edges as u8 & 7,
            (edges >> 3) as u8 & 7,
            (edges >> 6) as u8 & 7,
        ];
        let actual = maximum_lane_matching::<0>(
            LaneCompatibility::Constraints {
                rows: &rows,
                row_bytes: 1,
            },
            3,
            3,
        );
        let mut size = 0;
        let mut selected = 0;
        for right in 0..3 {
            if let Some(left) = actual.left_for_right(right) {
                assert!(left < 3 && rows[right as usize] & (1 << left) != 0);
                assert_eq!(selected & (1 << left), 0);
                selected |= 1 << left;
                size += 1;
            }
        }
        let mut expected = 0;
        for a in 0..4 {
            for b in 0..4 {
                for c in 0..4 {
                    let candidate = [a, b, c];
                    let mut used = 0;
                    let mut count = 0;
                    let mut valid = true;
                    for right in 0..3 {
                        let left = candidate[right];
                        if left < 3 {
                            valid &= rows[right] & (1 << left) != 0 && used & (1 << left) == 0;
                            used |= 1 << left;
                            count += 1;
                        }
                    }
                    if valid {
                        expected = expected.max(count);
                    }
                }
            }
        }
        assert_eq!(size, expected, "graph={edges}");
    }
}
