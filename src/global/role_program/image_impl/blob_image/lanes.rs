use super::super::super::{
    ColumnRange, PackedLaneRange, ROLE_IMAGE_LANE_STRIDE, RoleImageBytes, RouteArmLaneStepRow,
    lane_byte_count, lane_byte_index,
};
use crate::global::const_dsl::EffList;

impl<const N: usize> RoleImageBytes<N> {
    const fn eff_lane_bit_byte<const E: usize>(
        eff_list: &EffList<E>,
        role: u8,
        start_eff: usize,
        end_eff: usize,
        byte_idx: usize,
    ) -> u8 {
        let mut bits = 0u8;
        let mut eff_idx = start_eff;
        while eff_idx < end_eff {
            let node = eff_list.node_at(eff_idx);
            if matches!(node.kind, crate::eff::EffKind::Atom) {
                let atom = node.atom_data();
                if atom.from == role || atom.to == role {
                    let (candidate_byte, bit) = lane_byte_index(atom.lane as usize);
                    if candidate_byte == byte_idx {
                        bits |= bit;
                    }
                }
            }
            eff_idx += 1;
        }
        bits
    }

    const fn eff_lane_bit_len<const E: usize>(
        eff_list: &EffList<E>,
        role: u8,
        start_eff: usize,
        end_eff: usize,
    ) -> usize {
        let mut max_lane_plus_one = 0usize;
        let mut eff_idx = start_eff;
        while eff_idx < end_eff {
            let node = eff_list.node_at(eff_idx);
            if matches!(node.kind, crate::eff::EffKind::Atom) {
                let atom = node.atom_data();
                if atom.from == role || atom.to == role {
                    let lane_plus_one = atom.lane as usize + 1;
                    if lane_plus_one > max_lane_plus_one {
                        max_lane_plus_one = lane_plus_one;
                    }
                }
            }
            eff_idx += 1;
        }
        lane_byte_count(max_lane_plus_one)
    }

    pub(super) const fn write_lane_bit_row<const E: usize>(
        &mut self,
        column: ColumnRange,
        row_start: usize,
        eff_list: &EffList<E>,
        role: u8,
        start_eff: usize,
        end_eff: usize,
    ) -> PackedLaneRange {
        let len = Self::eff_lane_bit_len(eff_list, role, start_eff, end_eff);
        if len == 0 {
            return PackedLaneRange::new(0, 0);
        }
        let mut idx = 0usize;
        while idx < len {
            self.w8(
                column,
                row_start + idx,
                ROLE_IMAGE_LANE_STRIDE,
                Self::eff_lane_bit_byte(eff_list, role, start_eff, end_eff, idx),
            );
            idx += 1;
        }
        PackedLaneRange::new(row_start, len)
    }

    pub(super) const fn write_lane_bit_union_row<const E: usize>(
        &mut self,
        column: ColumnRange,
        row_start: usize,
        eff_list: &EffList<E>,
        role: u8,
        left: (usize, usize),
        right: (usize, usize),
    ) -> PackedLaneRange {
        let left_len = Self::eff_lane_bit_len(eff_list, role, left.0, left.1);
        let right_len = Self::eff_lane_bit_len(eff_list, role, right.0, right.1);
        let len = if left_len > right_len {
            left_len
        } else {
            right_len
        };
        if len == 0 {
            return PackedLaneRange::new(0, 0);
        }
        let mut idx = 0usize;
        while idx < len {
            self.w8(
                column,
                row_start + idx,
                ROLE_IMAGE_LANE_STRIDE,
                Self::eff_lane_bit_byte(eff_list, role, left.0, left.1, idx)
                    | Self::eff_lane_bit_byte(eff_list, role, right.0, right.1, idx),
            );
            idx += 1;
        }
        PackedLaneRange::new(row_start, len)
    }

    pub(super) const fn write_route_arm_lane_steps<const E: usize>(
        &mut self,
        column: ColumnRange,
        row_start: usize,
        eff_list: &EffList<E>,
        role: u8,
        eff_range: (usize, usize),
        local_row: PackedLaneRange,
    ) -> usize {
        let (start_eff, end_eff) = eff_range;
        let mut written = 0usize;
        let mut local_step = local_row.start();
        let mut eff_idx = start_eff;
        while eff_idx < end_eff {
            let node = eff_list.node_at(eff_idx);
            if matches!(node.kind, crate::eff::EffKind::Atom) {
                let atom = node.atom_data();
                if atom.from == role || atom.to == role {
                    let mut seen = false;
                    let mut scan_eff = start_eff;
                    while scan_eff < eff_idx {
                        let candidate = eff_list.node_at(scan_eff);
                        if matches!(candidate.kind, crate::eff::EffKind::Atom) {
                            let candidate = candidate.atom_data();
                            if (candidate.from == role || candidate.to == role)
                                && candidate.lane == atom.lane
                            {
                                seen = true;
                                break;
                            }
                        }
                        scan_eff += 1;
                    }
                    if !seen {
                        let mut last = local_step;
                        let mut scan_local_step = local_step + 1;
                        scan_eff = eff_idx + 1;
                        while scan_eff < end_eff {
                            let candidate = eff_list.node_at(scan_eff);
                            if matches!(candidate.kind, crate::eff::EffKind::Atom) {
                                let candidate = candidate.atom_data();
                                if candidate.from == role || candidate.to == role {
                                    if candidate.lane == atom.lane {
                                        last = scan_local_step;
                                    }
                                    scan_local_step += 1;
                                }
                            }
                            scan_eff += 1;
                        }
                        self.write_route_arm_lane_step(
                            column,
                            row_start + written,
                            RouteArmLaneStepRow::new(atom.lane, local_step, last),
                        );
                        written += 1;
                    }
                    local_step += 1;
                }
            }
            eff_idx += 1;
        }
        if local_step != local_row.end() {
            crate::invariant();
        }
        written
    }
}
