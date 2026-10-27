use super::{
    ColumnRange, ROLE_IMAGE_EVENT_STRIDE, ROLE_IMAGE_U16_STRIDE, RoleImageBytes, RoleImageColumns,
};
use crate::global::const_dsl::EffList;

impl<const N: usize> RoleImageBytes<N> {
    const fn operation_index_step(&self, index: ColumnRange, slot: usize) -> usize {
        let offset = Self::column_offset(index, slot, ROLE_IMAGE_U16_STRIDE);
        (self.bytes[offset] as u16 | (self.bytes[offset + 1] as u16) << 8) as usize
    }

    const fn operation_key<const E: usize>(
        &self,
        source: &EffList<E>,
        events: ColumnRange,
        step: usize,
    ) -> u32 {
        let offset = Self::column_offset(events, step, ROLE_IMAGE_EVENT_STRIDE);
        let effect = self.bytes[offset] as u16 | (self.bytes[offset + 1] as u16) << 8;
        let atom = source.atom_at(effect as usize);
        super::super::event_rows::operation_index_key(
            self.bytes[offset + 8],
            atom.from,
            atom.to,
            atom.label,
        )
    }

    /// Stable radix ordering of direction, role and label. Starting with source
    /// ordinals preserves their order among equal keys. An actor has at most
    /// two local facets per source event; scratch exists only during projection.
    pub(super) const fn write_operation_index<const E: usize>(
        &mut self,
        source: &EffList<E>,
        columns: RoleImageColumns,
    ) {
        let index = columns.operation_index();
        let count = index.len as usize;
        if count > 2 * E {
            crate::invariant();
        }
        let mut slot = 0;
        while slot < count {
            self.w16(index, slot, ROLE_IMAGE_U16_STRIDE, slot as u16);
            slot += 1;
        }
        if count < 2 {
            return;
        }
        let mut scratch = [[0u16; E]; 2];
        let mut shift = 0;
        while shift <= 16 {
            let buckets = if shift == 16 { 2 } else { 16 };
            let mut positions = [0usize; 16];
            slot = 0;
            while slot < count {
                let step = self.operation_index_step(index, slot);
                let bucket = (self.operation_key(source, columns.events, step) >> shift) & 15;
                positions[bucket as usize] += 1;
                slot += 1;
            }
            let mut bucket = 0;
            let mut start = 0;
            while bucket < buckets {
                let len = positions[bucket];
                positions[bucket] = start;
                start += len;
                bucket += 1;
            }
            slot = 0;
            while slot < count {
                let step = self.operation_index_step(index, slot);
                let bucket = (self.operation_key(source, columns.events, step) >> shift) & 15;
                let target = positions[bucket as usize];
                if target < E {
                    scratch[0][target] = step as u16;
                } else {
                    scratch[1][target - E] = step as u16;
                }
                positions[bucket as usize] += 1;
                slot += 1;
            }
            slot = 0;
            while slot < count {
                let step = if slot < E {
                    scratch[0][slot]
                } else {
                    scratch[1][slot - E]
                };
                self.w16(index, slot, ROLE_IMAGE_U16_STRIDE, step);
                slot += 1;
            }
            shift += 4;
        }
    }
}
