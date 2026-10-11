use crate::global::{
    const_dsl::EffList,
    role_program::{LANE_DOMAIN_SIZE, PackedLaneRange, lane_byte_count, lane_byte_index},
};

#[cfg(all(test, hibana_repo_tests))]
mod tests;

pub(in crate::global::role_program::image_impl) const LANE_BITMAP_BYTES: usize =
    lane_byte_count(LANE_DOMAIN_SIZE);

struct LocalLaneAccumulator {
    lane_bits: [u8; LANE_BITMAP_BYTES],
    last_steps: [u16; LANE_DOMAIN_SIZE],
    relation_count: usize,
    lane_bit_len: usize,
}

impl LocalLaneAccumulator {
    const fn new() -> Self {
        Self {
            lane_bits: [0; LANE_BITMAP_BYTES],
            last_steps: [0; LANE_DOMAIN_SIZE],
            relation_count: 0,
            lane_bit_len: 0,
        }
    }

    const fn record(&mut self, lane: u8, local_step: usize) {
        if local_step > u16::MAX as usize {
            panic!("local lane step overflow");
        }
        let lane = lane as usize;
        let (byte_idx, bit) = lane_byte_index(lane);
        if self.lane_bits[byte_idx] & bit == 0 {
            self.lane_bits[byte_idx] |= bit;
            self.relation_count += 1;
            let len = byte_idx + 1;
            if len > self.lane_bit_len {
                self.lane_bit_len = len;
            }
        }
        self.last_steps[lane] = local_step as u16;
    }
}

pub(in crate::global::role_program::image_impl) struct LocalLaneFacts {
    lanes: LocalLaneAccumulator,
    eff_range: (usize, usize),
    local_row: PackedLaneRange,
}

impl LocalLaneFacts {
    pub(in crate::global::role_program::image_impl) const fn for_eff_range<const E: usize>(
        eff_list: &EffList<E>,
        role: u8,
        start_eff: usize,
        end_eff: usize,
    ) -> Self {
        if start_eff > end_eff || end_eff > eff_list.len() {
            crate::invariant();
        }
        let mut lanes = LocalLaneAccumulator::new();
        let mut local_step = 0usize;
        let mut local_start = None;
        let mut local_len = 0usize;
        let mut eff_idx = 0usize;
        while eff_idx < end_eff {
            let atom = eff_list.atom_at(eff_idx);
            if atom.from == role || atom.to == role {
                if eff_idx >= start_eff {
                    if local_start.is_none() {
                        local_start = Some(local_step);
                    }
                    lanes.record(atom.lane, local_step);
                    local_len += 1;
                }
                local_step += 1;
            }
            eff_idx += 1;
        }
        let local_row = match local_start {
            Some(start) => PackedLaneRange::new(start, local_len),
            None => PackedLaneRange::new(0, 0),
        };
        Self {
            lanes,
            eff_range: (start_eff, end_eff),
            local_row,
        }
    }

    #[inline(always)]
    pub(in crate::global::role_program::image_impl) const fn eff_range(&self) -> (usize, usize) {
        self.eff_range
    }

    #[inline(always)]
    pub(in crate::global::role_program::image_impl) const fn local_row(&self) -> PackedLaneRange {
        self.local_row
    }

    #[inline(always)]
    pub(in crate::global::role_program::image_impl) const fn lane_bit_len(&self) -> usize {
        self.lanes.lane_bit_len
    }

    #[inline(always)]
    pub(in crate::global::role_program::image_impl) const fn lane_bit(
        &self,
        byte_idx: usize,
    ) -> u8 {
        if byte_idx >= self.lanes.lane_bit_len {
            crate::invariant();
        }
        self.lanes.lane_bits[byte_idx]
    }

    #[inline(always)]
    pub(in crate::global::role_program::image_impl) const fn relation_count(&self) -> usize {
        self.lanes.relation_count
    }

    #[inline(always)]
    pub(in crate::global::role_program::image_impl) const fn last_step(&self, lane: u8) -> usize {
        let lane = lane as usize;
        let (byte_idx, bit) = lane_byte_index(lane);
        if self.lanes.lane_bits[byte_idx] & bit == 0 {
            crate::invariant();
        }
        self.lanes.last_steps[lane] as usize
    }
}

#[cfg(kani)]
mod kani;
