use super::{RoleImageBytes, RoleImageColumns};

impl<const N: usize> RoleImageBytes<N> {
    /// Source IDs index immutable local slots. Absent IDs use the reserved u16
    /// sentinel; the table ends immediately after the greatest retained ID.
    pub(super) const fn write_route_index(&mut self, columns: RoleImageColumns) {
        let index = columns.route_index();
        let mut raw = 0;
        while raw < columns.route_index_len as usize {
            self.write_u16(index.start + raw * 2, u16::MAX);
            raw += 1;
        }
        let mut slot = 0;
        while slot < columns.route_scopes.len as usize {
            let at = columns.route_scopes.offset as usize + slot * 2;
            let raw = self.bytes[at] as u16 | (self.bytes[at + 1] as u16) << 8;
            if raw >= columns.route_index_len {
                panic!("route index does not cover local route");
            }
            self.write_u16(index.start + raw as usize * 2, slot as u16);
            slot += 1;
        }
    }
}
