use super::{ROLE_IMAGE_EVENT_STRIDE, ROLE_IMAGE_U16_STRIDE, RoleImageBytes, RoleImageColumns};
use crate::global::const_dsl::EffList;

impl<const N: usize> RoleImageBytes<N> {
    const fn receive_index_step(&self, columns: RoleImageColumns, slot: usize) -> usize {
        let offset = Self::column_offset(columns.receive_index(), slot, ROLE_IMAGE_U16_STRIDE);
        (self.bytes[offset] as u16 | (self.bytes[offset + 1] as u16) << 8) as usize
    }

    const fn receive_key<const E: usize>(
        &self,
        source: &EffList<E>,
        columns: RoleImageColumns,
        step: usize,
    ) -> u32 {
        let offset = Self::column_offset(columns.events, step, ROLE_IMAGE_EVENT_STRIDE);
        let effect = self.bytes[offset] as u16 | (self.bytes[offset + 1] as u16) << 8;
        let atom = source.atom_at(effect as usize);
        super::super::event_rows::receive_index_key(self.bytes[offset + 9], atom.to, atom.label)
    }

    const fn receive_step_less<const E: usize>(
        &self,
        source: &EffList<E>,
        columns: RoleImageColumns,
        left: usize,
        right: usize,
    ) -> bool {
        let left_key = self.receive_key(source, columns, left);
        let right_key = self.receive_key(source, columns, right);
        left_key < right_key || (left_key == right_key && left < right)
    }

    const fn sift_receive_index<const E: usize>(
        &mut self,
        source: &EffList<E>,
        columns: RoleImageColumns,
        mut root: usize,
        end: usize,
    ) {
        let index = columns.receive_index();
        loop {
            let mut child = root * 2 + 1;
            if child >= end {
                return;
            }
            let root_step = self.receive_index_step(columns, root);
            let mut child_step = self.receive_index_step(columns, child);
            if child + 1 < end {
                let right_step = self.receive_index_step(columns, child + 1);
                if self.receive_step_less(source, columns, child_step, right_step) {
                    child += 1;
                    child_step = right_step;
                }
            }
            if !self.receive_step_less(source, columns, root_step, child_step) {
                return;
            }
            self.w16(index, root, ROLE_IMAGE_U16_STRIDE, child_step as u16);
            self.w16(index, child, ROLE_IMAGE_U16_STRIDE, root_step as u16);
            root = child;
        }
    }

    /// Sort in place with constant scratch space and O(n log n) comparisons.
    /// Each existing local row contributes exactly one two-byte ordinal.
    pub(super) const fn write_receive_index<const E: usize>(
        &mut self,
        source: &EffList<E>,
        columns: RoleImageColumns,
    ) {
        let index = columns.receive_index();
        let count = index.len as usize;
        let mut slot = 0;
        while slot < count {
            self.w16(index, slot, ROLE_IMAGE_U16_STRIDE, slot as u16);
            slot += 1;
        }
        let mut root = count / 2;
        while root > 0 {
            root -= 1;
            self.sift_receive_index(source, columns, root, count);
        }
        let mut end = count;
        while end > 1 {
            end -= 1;
            let first = self.receive_index_step(columns, 0);
            let last = self.receive_index_step(columns, end);
            self.w16(index, 0, ROLE_IMAGE_U16_STRIDE, last as u16);
            self.w16(index, end, ROLE_IMAGE_U16_STRIDE, first as u16);
            self.sift_receive_index(source, columns, 0, end);
        }
    }
}
