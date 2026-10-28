use super::{RoleImageBytes, RoleImageColumns, projection};
use crate::global::{
    const_dsl::{EffList, ScopeEvent},
    role_program::{ROLE_IMAGE_EVENT_STRIDE, RoleLocalDirection},
    typestate::{LocalConflict, PackedEventConflict},
};

struct OfferAlternativeColumns<'a> {
    columns: &'a RoleImageColumns,
    route_index: usize,
    predecessors: usize,
    permutation: usize,
    heads: usize,
    forks: usize,
    outer: usize,
}

impl<'a> OfferAlternativeColumns<'a> {
    const fn new(columns: &'a RoleImageColumns) -> Self {
        let count = columns.events.len as usize;
        let route_index = columns.route_index().start;
        let predecessors = route_index
            + columns.route_index_len as usize * 2
            + columns.route_scopes.len as usize * 8;
        let permutation = predecessors + count * 2;
        let heads = permutation + count * 4;
        Self {
            columns,
            route_index,
            predecessors,
            permutation,
            heads,
            forks: heads + count * 2,
            outer: heads + count * 4,
        }
    }
}

/// Strict order in one unfolding. Binary siblings are independent or exclusive;
/// a split between the two effects is the only boundary that can separate them.
const fn source_precedes<const E: usize>(
    source: &EffList<E>,
    scopes: &projection::ScopeFacts,
    before: usize,
    after: usize,
) -> bool {
    if before >= after {
        return false;
    }
    let markers = source.scope_markers();
    let mut marker = markers.offset_lower_bound(before + 1, 0);
    while marker < markers.len() {
        let boundary = markers.at(marker);
        if boundary.offset() > after {
            break;
        }
        if matches!(boundary.event, ScopeEvent::Split)
            || matches!(boundary.event.route_arm(), Some(1))
        {
            let enter = markers.at(scopes.first_enter(boundary.scope_id));
            let end = match enter.event.route_end() {
                Some(end) => end,
                None => enter.segment_end(),
            };
            if enter.offset() <= before && after < end {
                return false;
            }
        }
        marker += 1;
    }
    true
}

impl<const N: usize> RoleImageBytes<N> {
    const fn topology_u16(&self, offset: usize) -> u16 {
        u16::from_le_bytes([self.bytes[offset], self.bytes[offset + 1]])
    }

    const fn topology_effect(&self, columns: &RoleImageColumns, step: usize) -> usize {
        self.topology_u16(columns.events.offset as usize + step * ROLE_IMAGE_EVENT_STRIDE) as usize
    }

    const fn topology_direction(
        &self,
        columns: &RoleImageColumns,
        step: usize,
    ) -> RoleLocalDirection {
        RoleLocalDirection::from_validated_flags(
            self.bytes[columns.events.offset as usize + step * ROLE_IMAGE_EVENT_STRIDE + 8],
        )
    }

    const fn topology_view<const E: usize>(
        &self,
        source: &EffList<E>,
        columns: &RoleImageColumns,
        step: usize,
    ) -> u8 {
        let atom = source.atom_at(self.topology_effect(columns, step));
        if matches!(
            self.topology_direction(columns, step),
            RoleLocalDirection::Recv
        ) {
            atom.to
        } else {
            atom.from
        }
    }

    const fn view_lane_key<const E: usize>(
        &self,
        source: &EffList<E>,
        columns: &RoleImageColumns,
        step: usize,
    ) -> u16 {
        ((self.topology_view(source, columns, step) as u16) << 8)
            | self.bytes[columns.lanes.offset as usize + step] as u16
    }

    const fn topology_conflict(
        &self,
        columns: &RoleImageColumns,
        step: usize,
    ) -> PackedEventConflict {
        let row =
            self.topology_u16(columns.events.offset as usize + step * ROLE_IMAGE_EVENT_STRIDE + 4);
        if row == u16::MAX {
            PackedEventConflict::none()
        } else {
            PackedEventConflict::from_validated_raw(
                self.topology_u16(columns.conflicts.offset as usize + row as usize * 2),
            )
        }
    }

    const fn alternative_left_contains(
        &self,
        columns: &OfferAlternativeColumns<'_>,
        target: u16,
        step: usize,
    ) -> bool {
        let slot = self.topology_u16(columns.forks + target as usize * 2) as usize;
        let at = columns.columns.route_arms.offset as usize + slot * 16;
        let len = self.topology_u16(at) as usize;
        let first = self.topology_u16(at + 2) as usize;
        first <= step && step < first + len
    }

    /// Adjacent rows of one key have at most one separating route. Its first
    /// right row owns the record, so the target needs no serialized field.
    const fn alternative_fork(
        &self,
        scopes: &projection::ScopeFacts,
        columns: &OfferAlternativeColumns<'_>,
        before: usize,
        after: usize,
    ) -> Option<usize> {
        let mut conflict = self.topology_conflict(columns.columns, before);
        while let Some(LocalConflict::RouteArm { scope, arm }) = conflict.to_conflict() {
            let slot = self.topology_u16(columns.route_index + scope.raw() as usize * 2);
            if slot == u16::MAX {
                crate::invariant();
            }
            let at = columns.columns.route_arms.offset as usize + slot as usize * 16;
            let left_len = self.topology_u16(at) as usize;
            let left_start = self.topology_u16(at + 2) as usize;
            let right_len = self.topology_u16(at + 8) as usize;
            let right_start = self.topology_u16(at + 10) as usize;
            if arm == 0
                && left_start <= before
                && before < left_start + left_len
                && right_start <= after
                && after < right_start + right_len
            {
                let prior = self.topology_u16(columns.predecessors + after * 2);
                if prior != u16::MAX && prior as usize >= right_start {
                    return None;
                }
                return Some(slot as usize);
            }
            conflict = PackedEventConflict::from_conflict(scopes.conflict(scope));
        }
        None
    }

    const fn write_offer_alternatives<const E: usize>(
        &mut self,
        source: &EffList<E>,
        scopes: &projection::ScopeFacts,
        columns: &RoleImageColumns,
    ) {
        let layout = OfferAlternativeColumns::new(columns);
        let heads = layout.heads;
        let forks = layout.forks;
        let outer = layout.outer;
        let count = columns.events.len as usize;
        if count == 0 {
            return;
        }
        let mut active = u16::MAX;
        let mut slot = count;
        let mut current = self.topology_u16(layout.permutation + (slot - 1) * 2) as usize;
        let mut current_key = self.view_lane_key(source, columns, current);
        while slot != 0 {
            slot -= 1;
            while active != u16::MAX && !self.alternative_left_contains(&layout, active, current) {
                active = self.topology_u16(outer + active as usize * 2);
            }
            self.write_u16(heads + current * 2, active);
            let previous = if slot == 0 {
                current
            } else {
                self.topology_u16(layout.permutation + (slot - 1) * 2) as usize
            };
            let previous_key = if slot == 0 {
                current_key
            } else {
                self.view_lane_key(source, columns, previous)
            };
            let mut fork = None;
            if slot != 0 && previous_key == current_key {
                // A record inside this fork's right arm cannot enclose its
                // left witness. Remove it before linking the new record.
                while active != u16::MAX
                    && !self.alternative_left_contains(&layout, active, previous)
                {
                    active = self.topology_u16(outer + active as usize * 2);
                }
                fork = self.alternative_fork(scopes, &layout, previous, current);
            } else {
                active = u16::MAX;
            }
            match fork {
                Some(fork) => {
                    self.write_u16(forks + current * 2, fork as u16);
                    self.write_u16(outer + current * 2, active);
                    active = current as u16;
                }
                None => {
                    self.write_u16(forks + current * 2, u16::MAX);
                    self.write_u16(outer + current * 2, u16::MAX);
                }
            }
            current = previous;
            current_key = previous_key;
        }
    }

    /// The source-order permutation makes one binary lookup sufficient for the
    /// first action of a logical view on a lane inside any projected arm.
    const fn write_view_lane_index<const E: usize>(
        &mut self,
        source: &EffList<E>,
        columns: &RoleImageColumns,
    ) {
        let start = columns.view_lane_index().start;
        let count = columns.events.len as usize;
        if count > 2 * E {
            crate::invariant();
        }
        let mut slot = 0;
        while slot < count {
            self.write_u16(start + slot * 2, slot as u16);
            slot += 1;
        }
        let mut scratch = [[0u16; E]; 2];
        let mut shift = 0;
        while shift < 16 {
            let mut positions = [0usize; 16];
            slot = 0;
            while slot < count {
                let step = self.topology_u16(start + slot * 2) as usize;
                let bucket = (self.view_lane_key(source, columns, step) >> shift) & 15;
                positions[bucket as usize] += 1;
                slot += 1;
            }
            let mut bucket = 0;
            let mut prefix = 0;
            while bucket < positions.len() {
                let len = positions[bucket];
                positions[bucket] = prefix;
                prefix += len;
                bucket += 1;
            }
            slot = 0;
            while slot < count {
                let step = self.topology_u16(start + slot * 2);
                let bucket = (self.view_lane_key(source, columns, step as usize) >> shift) & 15;
                let target = positions[bucket as usize];
                scratch[target / E][target % E] = step;
                positions[bucket as usize] += 1;
                slot += 1;
            }
            slot = 0;
            while slot < count {
                self.write_u16(start + slot * 2, scratch[slot / E][slot % E]);
                slot += 1;
            }
            shift += 4;
        }
    }

    pub(super) const fn write_route_topology<const E: usize>(
        &mut self,
        source: &EffList<E>,
        scopes: &projection::ScopeFacts,
        columns: RoleImageColumns,
    ) {
        let columns = &columns;
        let count = columns.events.len as usize;
        let successors = columns.lane_successors().start;
        let mut next = [count as u16; 256];
        let mut step = count;
        while step != 0 {
            step -= 1;
            let lane = self.bytes[columns.lanes.offset as usize + step] as usize;
            self.write_u16(successors + step * 2, next[lane]);
            next[lane] = step as u16;
        }
        if columns.route_scopes.len == 0 {
            return;
        }
        let mut last_view = [u16::MAX; 256];
        let predecessors = columns.view_predecessors().start;
        let mut step = 0;
        while step < columns.events.len as usize {
            let view = self.topology_view(source, columns, step) as usize;
            let effect = self.topology_effect(columns, step);
            let mut prior = last_view[view];
            while prior != u16::MAX
                && !source_precedes(
                    source,
                    scopes,
                    self.topology_effect(columns, prior as usize),
                    effect,
                )
            {
                prior = self.topology_u16(predecessors + prior as usize * 2);
            }
            self.write_u16(predecessors + step * 2, prior);
            last_view[view] = step as u16;
            step += 1;
        }
        self.write_view_lane_index(source, columns);
        self.write_offer_alternatives(source, scopes, columns);
        let entries = columns.route_entry_steps().start;
        let mut arm = 0;
        while arm < columns.route_arms.len as usize {
            let at = columns.route_arms.offset as usize + arm * 8;
            let len = self.topology_u16(at) as usize;
            let first = self.topology_u16(at + 2) as usize;
            let mut recv = u16::MAX;
            let mut send = u16::MAX;
            step = first;
            while step < first + len {
                if matches!(
                    self.topology_direction(columns, step),
                    RoleLocalDirection::Recv
                ) {
                    if recv == u16::MAX {
                        recv = step as u16;
                    }
                } else if send == u16::MAX {
                    send = step as u16;
                }
                step += 1;
            }
            self.write_u16(entries + arm * 4, recv);
            self.write_u16(entries + arm * 4 + 2, send);
            arm += 1;
        }
    }
}
