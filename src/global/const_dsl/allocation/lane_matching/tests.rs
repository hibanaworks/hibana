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
            assert_eq!(
                graph.next_allowed(right, 0, 256, &[0; BYTE_DOMAIN_MASK_BYTES]),
                Some(left)
            );
            assert_eq!(
                graph.next_allowed(right, left, 256, &[0; BYTE_DOMAIN_MASK_BYTES]),
                Some(left)
            );
            assert_eq!(
                graph.next_allowed(right, left + 1, 256, &[0; BYTE_DOMAIN_MASK_BYTES]),
                None
            );
            assert_eq!(
                graph.next_allowed((right + 1) % 256, 0, 256, &[0; BYTE_DOMAIN_MASK_BYTES]),
                None
            );
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

#[test]
fn participant_rows_preserve_the_matching_for_every_three_by_three_graph() {
    use crate::eff::{EffAtom, EventOrigin, EventPayload};

    let atom = |role, lane| EffAtom {
        from: role,
        to: role,
        label: 0,
        payload: EventPayload::Wire(0),
        origin: EventOrigin::User,
        lane,
    };
    let left = EffList::<3>::new()
        .push(atom(0, 0))
        .push(atom(1, 1))
        .push(atom(2, 2));
    let left = LaneEndpointIndex::<1>::from_range(&left, None, 0, left.len());
    for edges in 0..512u16 {
        let rows = [
            edges as u8 & 7,
            (edges >> 3) as u8 & 7,
            (edges >> 6) as u8 & 7,
        ];
        let mut right = EffList::<12>::new();
        for r in 0..3u8 {
            right.push_event_mut(atom(3, r));
            for l in 0..3u8 {
                if rows[r as usize] & (1 << l) == 0 {
                    right.push_event_mut(atom(l, r));
                }
            }
        }
        let right = LaneEndpointIndex::<1>::from_range(&right, None, 0, right.len());
        let participants = maximum_lane_matching(
            LaneCompatibility::Parallel {
                left: &left,
                right: &right,
            },
            3,
            3,
        );
        let explicit = maximum_lane_matching::<0>(
            LaneCompatibility::Constraints {
                rows: &rows,
                row_bytes: 1,
            },
            3,
            3,
        );
        for r in 0..3 {
            assert_eq!(
                participants.left_for_right(r),
                explicit.left_for_right(r),
                "graph={edges}, right={r}"
            );
        }
    }
}

#[test]
fn constraint_neighbors_preserve_every_byte_pattern_and_partial_domain() {
    for byte in [0, 31] {
        for bits in 0..=u8::MAX {
            let mut rows = [0u8; BYTE_DOMAIN_MASK_BYTES];
            rows[byte] = bits;
            let graph: LaneCompatibility<'_, 0> = LaneCompatibility::Constraints {
                rows: &rows,
                row_bytes: 32,
            };
            for offset in 0..=8 {
                let start = byte as u16 * 8 + offset;
                for span in [start, (byte as u16 + 1) * 8, 256] {
                    for blocked in [0, bits, 0x55, 0xaa, u8::MAX] {
                        let mut seen = [0; BYTE_DOMAIN_MASK_BYTES];
                        seen[byte] = blocked;
                        let expected = (start..span).find(|&lane| {
                            graph.allows(0, lane)
                                && seen[lane as usize / 8] & (1 << (lane % 8)) == 0
                        });
                        assert_eq!(
                            graph.next_allowed(0, start, span, &seen),
                            expected,
                            "byte={byte}, bits={bits}, blocked={blocked}, start={start}, span={span}"
                        );
                    }
                }
            }
        }
    }
}
