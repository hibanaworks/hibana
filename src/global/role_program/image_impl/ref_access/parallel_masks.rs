//! Mandatory source/row reconstruction for the immutable parallel lane table.
//! No live cursor, route selection or ownership state participates in a mask.
use super::{RoleImageRef, invalid_resident_descriptor};
use crate::global::role_program::{
    LaneSetView, PackedLaneRange,
    image_impl::{event_rows::RoleLocalDirection, parallel_masks::HEADER_BYTES},
};

impl RoleImageRef {
    /// Reset tokens are needed when a view can borrow a parallel fragment
    /// inside a repeated region. A participant may hold separate endpoints
    /// for the different fragments. Use source scopes and validated local rows.
    pub(super) const fn parallel_reentry_is_borrowable(&self) -> bool {
        if self.columns.roll_scopes.len == 0 {
            return false;
        }
        let mut marker = 0;
        while marker < self.program.source_scope_marker_count() {
            if let Some((start, _, end)) = self.program.source_parallel_at(marker)
                && self.mask_row_start(start) < self.mask_row_start(end)
            {
                let mut repeated = false;
                let mut roll_marker = 0;
                while roll_marker < self.program.source_scope_marker_count() {
                    if let Some((scope, body_start, body_end)) =
                        self.program.source_roll_at(roll_marker)
                        && body_start <= start
                        && end <= body_end
                    {
                        let mut slot = 0;
                        while let Some(row) = self.roll_scope_row(slot) {
                            if row.scope().raw() == scope.raw() {
                                repeated = true;
                                break;
                            }
                            slot += 1;
                        }
                    }
                    roll_marker += 1;
                }
                if repeated {
                    return true;
                }
            }
            marker += 1;
        }
        false
    }

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
        let Some(event) = self.lanes().local_step_event(row) else {
            invalid_resident_descriptor();
        };
        let eff = event.eff_index as usize;
        let atom = self.program.event_atom_at(eff);
        let view = match event.direction() {
            RoleLocalDirection::Recv => atom.to,
            RoleLocalDirection::Local | RoleLocalDirection::Send => atom.from,
        };
        (view, eff, atom.lane)
    }

    /// Resident validation has already established the complete source-ordered
    /// row projection and each row's direction, atom and lane. Locate the arm
    /// without decoding unrelated conflicts or scanning the entire local image.
    /// Equal source indices retain both facets of an actor-held communication.
    const fn mask_row_start(&self, source: usize) -> usize {
        let mut low = 0;
        let mut high = self.local_step_count();
        while low < high {
            let middle = low + (high - low) / 2;
            let Some(event) = self.lanes().local_step_event(middle) else {
                invalid_resident_descriptor();
            };
            if (event.eff_index as usize) < source {
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        low
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
                    let first = self.mask_row_start(start);
                    let end = self.mask_row_start(stop);
                    let mut views = [0u8; 32];
                    let mut event = first;
                    while event < end {
                        let (view, _, _) = self.mask_event(event);
                        views[view as usize / 8] |= 1 << (view % 8);
                        event += 1;
                    }
                    let mut view_byte = 0;
                    while view_byte < views.len() {
                        let mut remaining = views[view_byte];
                        while remaining != 0 {
                            let view = view_byte * 8 + remaining.trailing_zeros() as usize;
                            let mut bits = [0u8; 32];
                            let mut len = 0;
                            let mut event = first;
                            while event < end {
                                let (actual, _, lane) = self.mask_event(event);
                                if actual as usize == view {
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
                            remaining &= remaining - 1;
                        }
                        view_byte += 1;
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

    /// Complete candidate lanes for an existing borrow, or the actor image.
    /// Membership is static and includes Send/Local as well as receive rows.
    pub(crate) fn offer_candidate_lanes(&self, borrow_mask: Option<u16>) -> LaneSetView<'static> {
        let Some(mask) = borrow_mask else {
            return self.active_lane_set();
        };
        let offset = usize::from(mask);
        let len = usize::from(self.mask_byte(offset + 5));
        self.lanes().lane_bit_view(
            PackedLaneRange::new(offset + HEADER_BYTES, len),
            self.footprint().lane_word_count(),
        )
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
    fn reset_epochs_exist_only_for_projected_repeated_parallel_fragments() {
        type A = g::Msg<101, ()>;
        type B = g::Msg<102, ()>;
        let sequential = g::seq(g::send::<0, 1, A>(), g::send::<1, 0, B>()).roll();
        let separate = g::par(g::send::<0, 1, A>().roll(), g::send::<0, 2, B>().roll());
        let repeated = g::par(g::send::<0, 1, A>(), g::send::<0, 2, B>()).roll();
        let wrapped = g::route(
            g::par(g::send::<0, 1, A>(), g::send::<0, 2, B>()),
            g::par(
                g::send::<0, 1, g::Msg<103, ()>>(),
                g::send::<0, 2, g::Msg<104, ()>>(),
            ),
        )
        .resolve::<7>()
        .roll();
        let seq_program = project::<0>(&sequential);
        let seq = seq_program.role_image_ref().footprint();
        assert_eq!(seq.borrow_epoch_count, 0);
        assert_eq!(
            seq.completion_bit_count(),
            seq.local_step_count + seq.roll_scope_count
        );
        assert_eq!(
            project::<0>(&separate)
                .role_image_ref()
                .footprint()
                .borrow_epoch_count,
            0
        );
        assert_eq!(
            project::<0>(&repeated)
                .role_image_ref()
                .footprint()
                .borrow_epoch_count,
            1
        );
        assert_eq!(
            project::<1>(&repeated)
                .role_image_ref()
                .footprint()
                .borrow_epoch_count,
            1
        );
        assert_eq!(
            project::<2>(&repeated)
                .role_image_ref()
                .footprint()
                .borrow_epoch_count,
            1
        );
        let outside = g::seq(repeated, g::send::<0, 9, g::Msg<105, ()>>());
        assert_eq!(
            project::<9>(&outside)
                .role_image_ref()
                .footprint()
                .borrow_epoch_count,
            0
        );
        assert_eq!(
            project::<0>(&wrapped)
                .role_image_ref()
                .footprint()
                .borrow_epoch_count,
            1
        );
        assert_eq!(
            project::<1>(&wrapped)
                .role_image_ref()
                .footprint()
                .borrow_epoch_count,
            1
        );
    }

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
            let bytes = Box::into_raw(Box::new(bytes));
            assert!(
                catch_unwind(|| RoleImageRef::new(
                    rows.program,
                    rows.role,
                    rows.facts,
                    columns,
                    // SAFETY: this allocation stays live through validation.
                    // A rejected constructor cannot let its reference escape.
                    unsafe { &*bytes }
                ))
                .is_err()
            );
            // SAFETY: rejection has unwound every image borrowing this unique
            // allocation. No RoleImageRef escaped, so the test can reclaim it.
            unsafe { drop(Box::from_raw(bytes)) };
        }
    }

    #[test]
    fn candidate_lane_iteration_matches_raw_rows_for_coresident_views_and_local_actions() {
        use crate::Endpoint;
        use std::collections::BTreeSet;

        type Local = g::Msg<251, ()>;
        type Notice = g::Msg<252, ()>;
        type Loan<'r> = g::Msg<253, Endpoint<'r, 3>>;
        let global = g::seq(
            g::par(g::send::<1, 1, Local>(), g::send::<3, 0, Notice>()),
            g::send::<1, 2, Loan<'_>>(),
        );
        let owner = project::<1>(&global);
        let peer = project::<0>(&global);
        let owner_rows = owner.role_image_ref();
        let views: BTreeSet<_> = (0..owner_rows.local_step_count())
            .map(|step| owner_rows.local_step_logical_role(step))
            .collect();
        assert!(views.contains(&1) && views.contains(&3));
        assert!(
            (0..owner_rows.local_step_count())
                .any(|step| { owner_rows.local_step_direction(step) == RoleLocalDirection::Local })
        );
        let mut masks_checked = 0;
        for rows in [owner_rows, peer.role_image_ref()] {
            let lane_limit = rows.footprint().logical_lane_count;
            let raw = rows.lanes();
            let membership = |view: Option<u8>, first: usize, stop: usize| {
                let mut expected = BTreeSet::new();
                for step in 0..rows.local_step_count() {
                    let event = raw.local_step_event(step).unwrap();
                    let atom = rows.program().event_atom_at(event.eff_index as usize);
                    let actual_view = match event.direction() {
                        RoleLocalDirection::Recv => atom.to,
                        RoleLocalDirection::Local | RoleLocalDirection::Send => atom.from,
                    };
                    if view.is_none_or(|view| actual_view == view)
                        && first <= event.eff_index as usize
                        && (event.eff_index as usize) < stop
                    {
                        expected.insert(usize::from(atom.lane));
                    }
                }
                expected.into_iter().collect::<std::vec::Vec<_>>()
            };
            let collect = |mask| {
                let lanes = rows.offer_candidate_lanes(mask);
                let mut actual = std::vec::Vec::new();
                let mut start = 0;
                while let Some(lane) = lanes.next_set_from(start, lane_limit) {
                    actual.push(lane);
                    start = lane + 1;
                }
                actual
            };
            assert_eq!(collect(None), membership(None, 0, usize::MAX));
            for marker in 0..rows.program().source_scope_marker_count() {
                let Some((first, split, stop)) = rows.program().source_parallel_at(marker) else {
                    continue;
                };
                for (first, stop) in [(first, split), (split, stop)] {
                    for view in 0..=3 {
                        let expected = membership(Some(view), first, stop);
                        let mask = rows.parallel_borrow_mask(view, first, stop);
                        if expected.is_empty() {
                            assert!(mask.is_none());
                            continue;
                        }
                        let mask = mask.unwrap();
                        masks_checked += 1;
                        let actual = collect(Some(mask));
                        assert_eq!(actual, expected, "view={view} source={first}..{stop}");
                        for lane in 0..lane_limit {
                            assert_eq!(
                                actual.contains(&lane),
                                rows.parallel_borrow_owns_lane(mask, lane)
                            );
                        }
                    }
                }
            }
        }
        assert_eq!(masks_checked, 3);
        // Actor zero receives only the source-second parallel action. Its
        // lower lane is an actor-inactive hole, not a candidate to discover.
        let peer_rows = peer.role_image_ref();
        let peer_lanes = peer_rows.offer_candidate_lanes(None);
        assert!(!peer_lanes.contains(0));
        assert_eq!(
            peer_lanes.first_set(peer_rows.footprint().logical_lane_count),
            Some(1)
        );
    }
}
