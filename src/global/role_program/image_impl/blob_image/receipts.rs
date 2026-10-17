//! Construction-only index of actual owned receive rows, grouped by resource.
//! Entry rows share these immutable ranges; no holder or completion state is kept.
use super::{PackedLaneRange, ROLE_IMAGE_U16_STRIDE, RoleImageBytes, RoleImageColumns, projection};
use crate::{
    eff::EventPayload,
    global::{const_dsl::EffList, role_program::image_impl::event_rows::RoleLocalDirection},
};

impl<const N: usize> RoleImageBytes<N> {
    pub(super) const fn write_owned_receipts<const E: usize>(
        &mut self,
        effects: &EffList<E>,
        rows: projection::LocalRows<'_, E>,
        columns: RoleImageColumns,
    ) -> [PackedLaneRange; 256] {
        let mut ranges = [PackedLaneRange::new(0, 0); 256];
        if columns.owned_receipt_count == 0 {
            return ranges;
        }
        let mut counts = [0u16; 256];
        let mut event = 0;
        while event < effects.len() {
            if let EventPayload::Endpoint(role) = effects.atom_at(event).payload {
                let mut side = 0;
                while side < rows.count(effects, event) {
                    if matches!(
                        rows.direction(effects, event, side),
                        RoleLocalDirection::Recv
                    ) {
                        counts[role as usize] += 1;
                    }
                    side += 1;
                }
            }
            event += 1;
        }
        let mut offset = 0usize;
        let mut role = 0;
        while role < 256 {
            let count = counts[role] as usize;
            ranges[role] = PackedLaneRange::new(offset, count);
            counts[role] = 0;
            offset += count;
            role += 1;
        }
        if offset != columns.owned_receipt_count as usize {
            crate::invariant();
        }
        let mut local = 0;
        event = 0;
        while event < effects.len() {
            let mut side = 0;
            while side < rows.count(effects, event) {
                if matches!(
                    rows.direction(effects, event, side),
                    RoleLocalDirection::Recv
                ) && let EventPayload::Endpoint(role) = effects.atom_at(event).payload
                {
                    let index = ranges[role as usize].start() + counts[role as usize] as usize;
                    let byte =
                        Self::column_offset(columns.owned_receipts(), index, ROLE_IMAGE_U16_STRIDE);
                    self.write_u16(byte, local as u16);
                    counts[role as usize] += 1;
                }
                local += 1;
                side += 1;
            }
            event += 1;
        }
        ranges
    }
}
