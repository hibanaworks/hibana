//! Mandatory source/row reconstruction for the immutable parallel lane table.
//! No live cursor, route selection or ownership state participates in a mask.
use super::{RoleImageRef, invalid_resident_descriptor};
use crate::global::role_program::image_impl::parallel_masks::HEADER_BYTES;
use crate::global::{role_program::PackedLaneRange, typestate::LocalAction};

impl RoleImageRef {
    const fn mask_byte(&self, offset: usize) -> u8 {
        if offset >= self.columns.lane_bits.len as usize {
            invalid_resident_descriptor();
        }
        self.blob
            .byte_at(self.columns.lane_bits.offset as usize + offset)
    }

    const fn mask_u16(&self, offset: usize) -> u16 {
        self.mask_byte(offset) as u16 | (self.mask_byte(offset + 1) as u16) << 8
    }

    const fn mask_table_start(&self) -> usize {
        let mut end = self.facts.active_lane_row().end();
        let ranges = [
            self.columns.route_arm_lane_rows,
            self.columns.route_offer_lane_rows,
        ];
        let mut column = 0;
        while column < ranges.len() {
            let range = ranges[column];
            let mut row = 0;
            while row < range.len as usize {
                let offset = range.offset as usize + row * 4;
                let packed = self.blob.byte_at(offset) as u32
                    | (self.blob.byte_at(offset + 1) as u32) << 8
                    | (self.blob.byte_at(offset + 2) as u32) << 16
                    | (self.blob.byte_at(offset + 3) as u32) << 24;
                let bound = PackedLaneRange::from_raw(packed).end();
                if bound > end {
                    end = bound;
                }
                row += 1;
            }
            column += 1;
        }
        end
    }

    const fn mask_event(&self, row: usize) -> (u8, usize, u8) {
        let Some(node) = self.local_step_node(row) else {
            invalid_resident_descriptor();
        };
        let (eff, lane, receiving) = match node.action() {
            LocalAction::Send {
                eff_index, lane, ..
            }
            | LocalAction::Local {
                eff_index, lane, ..
            } => (eff_index.dense_ordinal(), lane, false),
            LocalAction::Recv {
                eff_index, lane, ..
            } => (eff_index.dense_ordinal(), lane, true),
            LocalAction::Terminate => invalid_resident_descriptor(),
        };
        let atom = self.program.event_atom_at(eff);
        (if receiving { atom.to } else { atom.from }, eff, lane)
    }

    pub(super) const fn validate_parallel_borrow_masks(&self) {
        let mut offset = self.mask_table_start();
        let mut marker = 0;
        while marker < self.program.source_scope_marker_count() {
            if let Some((start, split, stop)) = self.program.source_parallel_at(marker) {
                let ranges = [(start, split), (split, stop)];
                let mut arm = 0;
                while arm < 2 {
                    let (start, stop) = ranges[arm];
                    let mut views = [0u8; 32];
                    let mut event = 0;
                    while event < self.local_step_count() {
                        let (view, eff, _) = self.mask_event(event);
                        if start <= eff && eff < stop {
                            views[view as usize / 8] |= 1 << (view % 8);
                        }
                        event += 1;
                    }
                    let mut view = 0usize;
                    while view < 256 {
                        if views[view / 8] & (1 << (view % 8)) != 0 {
                            let mut bits = [0u8; 32];
                            let mut len = 0;
                            let mut event = 0;
                            while event < self.local_step_count() {
                                let (actual, eff, lane) = self.mask_event(event);
                                if actual as usize == view && start <= eff && eff < stop {
                                    bits[lane as usize / 8] |= 1 << (lane % 8);
                                    let bound = lane as usize / 8 + 1;
                                    if bound > len {
                                        len = bound;
                                    }
                                }
                                event += 1;
                            }
                            if self.mask_byte(offset) as usize != view
                                || self.mask_u16(offset + 1) as usize != start
                                || self.mask_u16(offset + 3) as usize != stop
                                || self.mask_byte(offset + 5) as usize != len
                            {
                                invalid_resident_descriptor();
                            }
                            let mut byte = 0;
                            while byte < len {
                                if self.mask_byte(offset + HEADER_BYTES + byte) != bits[byte] {
                                    invalid_resident_descriptor();
                                }
                                byte += 1;
                            }
                            offset += HEADER_BYTES + len;
                        }
                        view += 1;
                    }
                    arm += 1;
                }
            }
            marker += 1;
        }
        if offset != self.columns.lane_bits.len as usize {
            invalid_resident_descriptor();
        }
    }

    /// Lookup is construction-time work, once for the final flattened borrow.
    pub(crate) fn parallel_borrow_mask(&self, view: u8, start: usize, stop: usize) -> Option<u16> {
        let mut offset = self.mask_table_start();
        while offset < self.columns.lane_bits.len as usize {
            if self.mask_byte(offset) == view
                && self.mask_u16(offset + 1) as usize == start
                && self.mask_u16(offset + 3) as usize == stop
            {
                return u16::try_from(offset).ok();
            }
            offset += HEADER_BYTES + self.mask_byte(offset + 5) as usize;
        }
        None
    }

    /// One checked immutable byte lookup; no event or scope scan on the hot path.
    #[inline]
    pub(crate) fn parallel_borrow_owns_lane(&self, mask: u16, lane: usize) -> bool {
        let offset = usize::from(mask);
        lane / 8 < self.mask_byte(offset + 5) as usize
            && self.mask_byte(offset + HEADER_BYTES + lane / 8) & (1 << (lane % 8)) != 0
    }
}

#[cfg(all(test, hibana_repo_tests))]
mod tests {
    use super::*;
    use crate::{g, global::role_program::project};
    use std::{boxed::Box, panic::catch_unwind};

    #[test]
    fn parallel_masks_are_bound_to_their_source_even_when_raw_columns_are_coherent() {
        let global = g::par(
            g::send::<0, 1, g::Msg<1, u32>>(),
            g::send::<1, 0, g::Msg<2, u32>>(),
        );
        let program = project::<0>(&global);
        let rows = program.role_image_ref();
        let first = rows.mask_table_start();
        let mut original = [0u8; 2048];
        assert!(rows.columns.blob_len() <= original.len());
        for (at, byte) in original
            .iter_mut()
            .enumerate()
            .take(rows.columns.blob_len())
        {
            *byte = rows.blob.byte_at(at);
        }
        for mutation in 0..4 {
            let mut bytes = original;
            let mut columns = rows.columns;
            let base = columns.lane_bits.offset as usize + first;
            match mutation {
                0 => bytes[base] ^= 1,                     // another valid role identifier
                1 => bytes[base + HEADER_BYTES] ^= 3,      // valid active lanes, wrong arm
                2 => bytes[base + 1] ^= 1,                 // well-formed but wrong source interval
                3 => columns.lane_bits.len = first as u16, // entire cache omitted
                _ => unreachable!(),
            }
            if mutation == 2 {
                // Keep an ordered interval so the raw syntax validator cannot
                // reject this case before the source correspondence check.
                bytes[base + 1..base + 3].copy_from_slice(&1u16.to_le_bytes());
                bytes[base + 3..base + 5].copy_from_slice(&2u16.to_le_bytes());
            }
            assert!(
                crate::global::role_program::image_impl::metadata::lane_columns_are_coherent(
                    &bytes,
                    columns,
                    rows.active_lane_row(),
                    rows.footprint(),
                ),
                "mutation {mutation} must reach the independent semantic validator"
            );
            let bytes = Box::leak(Box::new(bytes));
            assert!(
                catch_unwind(|| RoleImageRef::new(
                    rows.program,
                    rows.role,
                    rows.facts,
                    columns,
                    bytes
                ))
                .is_err()
            );
        }
    }
}
