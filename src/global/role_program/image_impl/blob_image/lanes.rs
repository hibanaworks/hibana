use super::super::super::{
    ColumnRange, PackedLaneRange, ROLE_IMAGE_LANE_STRIDE, RoleImageBytes, RouteArmLaneStepRow,
};
use super::super::projection::{LANE_BITMAP_BYTES, LocalLaneFacts};
use crate::global::const_dsl::EffList;

impl<const N: usize> RoleImageBytes<N> {
    const fn zero_extended_lane_bit_from_row(
        &self,
        column: ColumnRange,
        row: PackedLaneRange,
        index: usize,
    ) -> u8 {
        if index >= row.len() {
            return 0;
        }
        let source_row = row.start() + index;
        if source_row >= column.len as usize {
            crate::invariant();
        }
        self.bytes[Self::column_offset(column, source_row, ROLE_IMAGE_LANE_STRIDE)]
    }

    pub(super) const fn write_lane_bit_row(
        &mut self,
        column: ColumnRange,
        row_start: usize,
        facts: &LocalLaneFacts,
    ) -> PackedLaneRange {
        let len = facts.lane_bit_len();
        if len == 0 {
            return PackedLaneRange::new(0, 0);
        }
        let mut idx = 0usize;
        while idx < len {
            self.w8(
                column,
                row_start + idx,
                ROLE_IMAGE_LANE_STRIDE,
                facts.lane_bit(idx),
            );
            idx += 1;
        }
        PackedLaneRange::new(row_start, len)
    }

    pub(super) const fn write_lane_bit_union_row(
        &mut self,
        column: ColumnRange,
        row_start: usize,
        left: PackedLaneRange,
        right: PackedLaneRange,
    ) -> PackedLaneRange {
        let left_len = left.len();
        let right_len = right.len();
        let len = if left_len > right_len {
            left_len
        } else {
            right_len
        };
        if len == 0 {
            return PackedLaneRange::new(0, 0);
        }
        if row_start < left.end() || row_start < right.end() {
            crate::invariant();
        }
        let mut idx = 0usize;
        while idx < len {
            let left_bits = self.zero_extended_lane_bit_from_row(column, left, idx);
            let right_bits = self.zero_extended_lane_bit_from_row(column, right, idx);
            self.w8(
                column,
                row_start + idx,
                ROLE_IMAGE_LANE_STRIDE,
                left_bits | right_bits,
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
        facts: &LocalLaneFacts,
    ) -> usize {
        let (start_eff, end_eff) = facts.eff_range();
        let mut written = 0usize;
        let local_row = facts.local_row();
        let mut local_step = local_row.start();
        let mut emitted = [0u8; LANE_BITMAP_BYTES];
        let mut eff_idx = start_eff;
        while eff_idx < end_eff {
            let node = eff_list.node_at(eff_idx);
            if matches!(node.kind, crate::eff::EffKind::Atom) {
                let atom = node.atom_data();
                if atom.from == role || atom.to == role {
                    let lane = atom.lane as usize;
                    let byte_idx = lane / 8;
                    let bit = 1u8 << (lane % 8);
                    if emitted[byte_idx] & bit == 0 {
                        emitted[byte_idx] |= bit;
                        self.write_route_arm_lane_step(
                            column,
                            row_start + written,
                            RouteArmLaneStepRow::new(
                                atom.lane,
                                local_step,
                                facts.last_step(atom.lane),
                            ),
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
        if written != facts.relation_count() {
            crate::invariant();
        }
        written
    }
}
