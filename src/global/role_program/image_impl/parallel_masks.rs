//! Construction-only lane summaries for lexical parallel borrows. Rows live
//! after route bitmaps in the same immutable lane-byte column.
use super::{event_rows::RoleLocalDirection, projection::LocalRows};
use crate::global::const_dsl::{EffList, parallel_arm_ranges_from_enter};

pub(super) const HEADER_BYTES: usize = 6;
const MASK_BYTES: usize = (u8::MAX as usize + 1).div_ceil(8);

pub(super) struct MaskRow {
    view: u8,
    start: u16,
    stop: u16,
    len: u8,
    bits: [u8; MASK_BYTES],
}

const fn logical_view<const E: usize>(
    effects: &EffList<E>,
    rows: LocalRows<'_, E>,
    event: usize,
    side: usize,
) -> u8 {
    let atom = effects.atom_at(event);
    match rows.direction(effects, event, side) {
        RoleLocalDirection::Local | RoleLocalDirection::Send => atom.from,
        RoleLocalDirection::Recv => atom.to,
    }
}

impl MaskRow {
    pub(super) const fn len(&self) -> usize {
        HEADER_BYTES + self.len as usize
    }

    pub(super) const fn byte(&self, index: usize) -> u8 {
        match index {
            0 => self.view,
            1 => self.start as u8,
            2 => (self.start >> 8) as u8,
            3 => self.stop as u8,
            4 => (self.stop >> 8) as u8,
            5 => self.len,
            _ => {
                if index >= self.len() {
                    crate::invariant();
                }
                self.bits[index - HEADER_BYTES]
            }
        }
    }
}

/// Compiler iterator workspace. Nothing here is attached to a live session.
pub(super) struct MaskRows {
    marker: usize,
    arm: u8,
    range: Option<(usize, usize)>,
    views: [u8; MASK_BYTES],
    lanes: [[u8; MASK_BYTES]; 256],
    lengths: [u8; 256],
}

impl MaskRows {
    pub(super) const fn new() -> Self {
        Self {
            marker: 0,
            arm: 0,
            range: None,
            views: [0; MASK_BYTES],
            lanes: [[0; MASK_BYTES]; 256],
            lengths: [0; 256],
        }
    }

    pub(super) const fn next<const E: usize>(
        &mut self,
        effects: &EffList<E>,
        rows: LocalRows<'_, E>,
    ) -> Option<MaskRow> {
        loop {
            if let Some((start, stop)) = self.range {
                let mut byte = 0;
                while byte < self.views.len() {
                    let pending = self.views[byte];
                    if pending != 0 {
                        let bit = pending.trailing_zeros() as usize;
                        self.views[byte] &= !(1 << bit);
                        let view = byte * 8 + bit;
                        let result = MaskRow {
                            view: view as u8,
                            start: start as u16,
                            stop: stop as u16,
                            len: self.lengths[view],
                            bits: self.lanes[view],
                        };
                        self.lanes[view] = [0; MASK_BYTES];
                        self.lengths[view] = 0;
                        return Some(result);
                    }
                    byte += 1;
                }
                self.range = None;
            }
            let markers = effects.scope_markers();
            if self.marker == markers.len() {
                return None;
            }
            let Some((left_start, left_stop, right_start, right_stop)) =
                parallel_arm_ranges_from_enter(markers, self.marker)
            else {
                self.marker += 1;
                self.arm = 0;
                continue;
            };
            let (start, stop) = if self.arm == 0 {
                self.arm = 1;
                (left_start, left_stop)
            } else {
                self.marker += 1;
                self.arm = 0;
                (right_start, right_stop)
            };
            if rows.range(effects, start, stop).is_absent_or_zero_len() {
                continue;
            }
            self.range = Some((start, stop));
            if start >= stop || stop > effects.len() || stop > u16::MAX as usize {
                crate::invariant();
            }
            let mut event = start;
            while event < stop {
                let mut side = 0;
                while side < rows.count(effects, event) {
                    let view = logical_view(effects, rows, event, side) as usize;
                    self.views[view / 8] |= 1 << (view % 8);
                    let lane = effects.atom_at(event).lane as usize;
                    self.lanes[view][lane / 8] |= 1 << (lane % 8);
                    let len = (lane / 8 + 1) as u8;
                    if len > self.lengths[view] {
                        self.lengths[view] = len;
                    }
                    side += 1;
                }
                event += 1;
            }
        }
    }

    pub(super) const fn byte_len<const E: usize>(
        effects: &EffList<E>,
        rows: LocalRows<'_, E>,
    ) -> usize {
        let mut cursor = Self::new();
        let mut bytes = 0;
        while let Some(row) = cursor.next(effects, rows) {
            bytes += row.len();
        }
        bytes
    }
}
