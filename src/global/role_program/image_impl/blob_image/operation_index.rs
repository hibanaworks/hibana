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
            self.bytes[offset + 9],
            atom.from,
            atom.to,
            atom.label,
        )
    }

    const fn operation_step_less<const E: usize>(
        &self,
        source: &EffList<E>,
        events: ColumnRange,
        left: usize,
        right: usize,
    ) -> bool {
        let left_key = self.operation_key(source, events, left);
        let right_key = self.operation_key(source, events, right);
        left_key < right_key || (left_key == right_key && left < right)
    }

    const fn sift_operation_index<const E: usize>(
        &mut self,
        source: &EffList<E>,
        index: ColumnRange,
        events: ColumnRange,
        mut root: usize,
        end: usize,
    ) {
        loop {
            let mut child = root * 2 + 1;
            if child >= end {
                return;
            }
            let root_step = self.operation_index_step(index, root);
            let mut child_step = self.operation_index_step(index, child);
            if child + 1 < end {
                let right_step = self.operation_index_step(index, child + 1);
                if self.operation_step_less(source, events, child_step, right_step) {
                    child += 1;
                    child_step = right_step;
                }
            }
            if !self.operation_step_less(source, events, root_step, child_step) {
                return;
            }
            self.w16(index, root, ROLE_IMAGE_U16_STRIDE, child_step as u16);
            self.w16(index, child, ROLE_IMAGE_U16_STRIDE, root_step as u16);
            root = child;
        }
    }

    /// Sort in place with constant scratch space and O(n log n) comparisons.
    /// Each existing local row contributes exactly one two-byte ordinal.
    pub(super) const fn write_operation_index<const E: usize>(
        &mut self,
        source: &EffList<E>,
        columns: RoleImageColumns,
    ) {
        let index = columns.operation_index();
        let count = index.len as usize;
        let mut slot = 0;
        while slot < count {
            self.w16(index, slot, ROLE_IMAGE_U16_STRIDE, slot as u16);
            slot += 1;
        }
        let mut root = count / 2;
        while root > 0 {
            root -= 1;
            self.sift_operation_index(source, index, columns.events, root, count);
        }
        let mut end = count;
        while end > 1 {
            end -= 1;
            let first = self.operation_index_step(index, 0);
            let last = self.operation_index_step(index, end);
            self.w16(index, 0, ROLE_IMAGE_U16_STRIDE, last as u16);
            self.w16(index, end, ROLE_IMAGE_U16_STRIDE, first as u16);
            self.sift_operation_index(source, index, columns.events, 0, end);
        }
    }
}
