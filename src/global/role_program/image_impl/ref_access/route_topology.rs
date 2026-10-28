use super::{RoleImageRef, invalid_resident_descriptor};
use crate::global::const_dsl::ScopeId;
use crate::global::role_program::RoleLocalDirection;

const fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

struct OfferAlternativeReader<'a> {
    image: &'a RoleImageRef,
    bytes: &'a [u8],
    count: usize,
    events: usize,
    lanes: usize,
    route_scopes: usize,
    route_index: usize,
    predecessors: usize,
    permutation: usize,
    heads: usize,
    forks: usize,
    outer: usize,
}

impl<'a> OfferAlternativeReader<'a> {
    const fn new(image: &'a RoleImageRef) -> Self {
        let count = image.columns.events.len as usize;
        let predecessors = image.topology_start() + image.columns.route_scopes.len as usize * 8;
        let permutation = predecessors + count * 2;
        let heads = permutation + count * 4;
        Self {
            image,
            bytes: image.blob.as_slice(),
            count,
            events: image.columns.events.offset as usize,
            lanes: image.columns.lanes.offset as usize,
            route_scopes: image.columns.route_scopes.offset as usize,
            route_index: image.route_index_start as usize,
            predecessors,
            permutation,
            heads,
            forks: heads + count * 2,
            outer: heads + count * 4,
        }
    }

    const fn effect(&self, step: usize) -> usize {
        read_u16(
            self.bytes,
            self.events + step * crate::global::role_program::ROLE_IMAGE_EVENT_STRIDE,
        ) as usize
    }

    const fn key(&self, step: usize) -> u16 {
        let at = self.events + step * crate::global::role_program::ROLE_IMAGE_EVENT_STRIDE;
        let atom = self
            .image
            .program()
            .event_atom_at(read_u16(self.bytes, at) as usize);
        let view = if matches!(
            RoleLocalDirection::from_validated_flags(self.bytes[at + 8]),
            RoleLocalDirection::Recv
        ) {
            atom.to
        } else {
            atom.from
        };
        ((view as u16) << 8) | self.bytes[self.lanes + step] as u16
    }

    const fn left_contains(&self, target: u16, step: usize) -> bool {
        let slot = read_u16(self.bytes, self.forks + target as usize * 2);
        let scope = match ScopeId::decode_raw(read_u16(
            self.bytes,
            self.route_scopes + slot as usize * 2,
        )) {
            Some(scope) => scope,
            None => invalid_resident_descriptor(),
        };
        self.image.program().source_route_separates(
            scope,
            self.effect(step),
            self.effect(target as usize),
        )
    }

    const fn expected_fork(&self, before: usize, after: usize) -> Option<usize> {
        let Some((scope, split)) = self
            .image
            .program()
            .source_separating_route(self.effect(before), self.effect(after))
        else {
            return None;
        };
        let prior = read_u16(self.bytes, self.predecessors + after * 2);
        if prior != u16::MAX && self.effect(prior as usize) >= split {
            return None;
        }
        if scope.raw() >= self.image.columns.route_index_len {
            invalid_resident_descriptor();
        }
        let slot = read_u16(self.bytes, self.route_index + scope.raw() as usize * 2);
        if slot == u16::MAX {
            invalid_resident_descriptor();
        }
        Some(slot as usize)
    }
}

impl RoleImageRef {
    /// The compact index starts at a constructor-checked offset. Auxiliary
    /// addresses use that base, without rebuilding the packed column layout.
    const fn topology_start(&self) -> usize {
        self.route_index_start as usize + self.columns.route_index_len as usize * 2
    }

    const fn predecessor_start(&self) -> usize {
        self.topology_start() + self.columns.route_scopes.len as usize * 8
    }

    const fn view_lane_index_start(&self) -> usize {
        self.predecessor_start()
            + if self.columns.route_scopes.len == 0 {
                0
            } else {
                self.local_step_count() * 2
            }
    }

    const fn lane_successor_start(&self) -> usize {
        self.view_lane_index_start()
            + if self.columns.route_scopes.len == 0 {
                0
            } else {
                self.local_step_count() * 2
            }
    }

    const fn alternative_head_start(&self) -> usize {
        self.lane_successor_start() + self.local_step_count() * 2
    }

    /// Next alternative for the exact logical view and lane of this row.
    /// Known forks are skipped through their sealed enclosing links.
    pub(crate) fn next_offer_lane_alternative(
        &self,
        step: usize,
        preview: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> Option<usize> {
        if step >= self.local_step_count() {
            invalid_resident_descriptor();
        }
        if self.columns.route_scopes.len == 0 {
            return None;
        }
        let heads = self.alternative_head_start();
        let forks = heads + self.columns.events.len as usize * 2;
        let outer = forks + self.columns.events.len as usize * 2;
        let scopes = self.columns.route_scopes.offset as usize;
        let bytes = self.blob.as_slice();
        let mut target = read_u16(bytes, heads + step * 2);
        while target != u16::MAX {
            let slot = read_u16(bytes, forks + target as usize * 2);
            let scope = match ScopeId::decode_raw(read_u16(bytes, scopes + slot as usize * 2)) {
                Some(scope) => scope,
                None => invalid_resident_descriptor(),
            };
            if preview(scope).is_none() {
                return Some(target as usize);
            }
            target = read_u16(bytes, outer + target as usize * 2);
        }
        None
    }

    /// Reconstruct every record and chain from source separation and the
    /// independently sealed key permutation and causal predecessors. Future
    /// targets are checked before their outer links are used by this sweep.
    const fn validate_offer_alternatives(&self) {
        let plan = OfferAlternativeReader::new(self);
        if plan.count == 0 {
            return;
        }
        let mut active = u16::MAX;
        let mut slot = plan.count;
        let mut current = read_u16(plan.bytes, plan.permutation + (slot - 1) * 2) as usize;
        let mut current_key = plan.key(current);
        while slot != 0 {
            slot -= 1;
            while active != u16::MAX && !plan.left_contains(active, current) {
                active = read_u16(plan.bytes, plan.outer + active as usize * 2);
            }
            if read_u16(plan.bytes, plan.heads + current * 2) != active {
                invalid_resident_descriptor();
            }
            let mut expected_fork = None;
            let previous = if slot == 0 {
                current
            } else {
                read_u16(plan.bytes, plan.permutation + (slot - 1) * 2) as usize
            };
            let previous_key = if slot == 0 {
                current_key
            } else {
                plan.key(previous)
            };
            if slot != 0 {
                if previous_key == current_key {
                    while active != u16::MAX && !plan.left_contains(active, previous) {
                        active = read_u16(plan.bytes, plan.outer + active as usize * 2);
                    }
                    expected_fork = plan.expected_fork(previous, current);
                } else {
                    active = u16::MAX;
                }
            }
            let fork = read_u16(plan.bytes, plan.forks + current * 2);
            let outer = read_u16(plan.bytes, plan.outer + current * 2);
            match expected_fork {
                Some(expected) => {
                    if fork as usize != expected
                        || outer != active
                        || (outer != u16::MAX && outer as usize <= current)
                    {
                        invalid_resident_descriptor();
                    }
                    active = current as u16;
                }
                None => {
                    if fork != u16::MAX || outer != u16::MAX {
                        invalid_resident_descriptor();
                    }
                }
            }
            current = previous;
            current_key = previous_key;
        }
    }

    #[inline(always)]
    pub(crate) const fn next_lane_step(&self, step: usize) -> usize {
        if step >= self.local_step_count() {
            invalid_resident_descriptor();
        }
        self.topology_u16(self.lane_successor_start() + step * 2) as usize
    }

    const fn topology_u16(&self, offset: usize) -> u16 {
        u16::from_le_bytes([self.blob.byte_at(offset), self.blob.byte_at(offset + 1)])
    }

    const fn view_lane_step_key(&self, step: usize) -> u32 {
        ((self.local_step_logical_role(step) as u32) << 24)
            | ((self.blob.byte_at(self.columns.lanes.offset as usize + step) as u32) << 16)
            | step as u32
    }

    const fn view_lane_step_at(&self, slot: usize) -> usize {
        let step = self.topology_u16(self.view_lane_index_start() + slot * 2) as usize;
        if step >= self.local_step_count() {
            invalid_resident_descriptor();
        }
        step
    }

    /// The latest causally preceding action of the same logical view. The
    /// compiler omits independent and alternative siblings from this link.
    const fn view_predecessor(&self, step: usize) -> Option<usize> {
        let prior = self.topology_u16(self.predecessor_start() + step * 2);
        if prior == u16::MAX {
            None
        } else {
            Some(prior as usize)
        }
    }

    pub(crate) const fn route_arm_first_operation(
        &self,
        slot: usize,
        arm: u8,
        receive: bool,
    ) -> Option<usize> {
        if slot >= self.columns.route_scopes.len as usize || arm > 1 {
            return None;
        }
        let at =
            self.topology_start() + (slot * 2 + arm as usize) * 4 + if receive { 0 } else { 2 };
        let step = self.topology_u16(at);
        if step == u16::MAX {
            None
        } else {
            Some(step as usize)
        }
    }

    pub(crate) fn route_arm_view_lane_entry(
        &self,
        slot: usize,
        arm: u8,
        view: u8,
        lane: u8,
    ) -> Option<usize> {
        let range = self.route_arm_event_row_by_slot(slot, arm);
        if range.is_absent_or_zero_len() {
            return None;
        }
        let step = self.view_lane_step_at_or_after(view, lane, range.start())?;
        if step >= range.end() {
            return None;
        }
        if self
            .view_predecessor(step)
            .is_some_and(|prior| prior >= range.start())
        {
            None
        } else {
            Some(step)
        }
    }

    /// Lower bound in the sealed `(logical view, lane, local row)` index.
    /// Images with no routes need no offer candidates and omit this index.
    pub(crate) fn view_lane_step_at_or_after(
        &self,
        view: u8,
        lane: u8,
        start: usize,
    ) -> Option<usize> {
        if self.columns.route_scopes.len == 0 || start >= self.local_step_count() {
            return None;
        }
        let prefix = (u32::from(view) << 24) | (u32::from(lane) << 16);
        let key = prefix | start as u32;
        let mut low = 0;
        let mut high = self.local_step_count();
        while low < high {
            let mid = low + (high - low) / 2;
            if self.view_lane_step_key(self.view_lane_step_at(mid)) < key {
                low = mid + 1;
            } else {
                high = mid;
            }
        }
        if low == self.local_step_count() {
            return None;
        }
        let step = self.view_lane_step_at(low);
        (self.view_lane_step_key(step) >> 16 == prefix >> 16).then_some(step)
    }

    pub(super) const fn validate_route_topology(&self) {
        let count = self.local_step_count();
        let mut next = [count; 256];
        let mut step = count;
        while step != 0 {
            step -= 1;
            let lane = self.blob.byte_at(self.columns.lanes.offset as usize + step) as usize;
            if self.next_lane_step(step) != next[lane] {
                invalid_resident_descriptor();
            }
            next[lane] = step;
        }
        if self.columns.route_scopes.len == 0 {
            return;
        }
        let mut last_view = [None; 256];
        let mut step = 0;
        let mut previous_key = None;
        while step < count {
            let indexed = self.view_lane_step_at(step);
            let key = self.view_lane_step_key(indexed);
            if let Some(previous) = previous_key
                && key <= previous
            {
                invalid_resident_descriptor();
            }
            previous_key = Some(key);
            let view = self.local_step_logical_role(step) as usize;
            let effect = self.local_step_effect(step).dense_ordinal();
            let mut expected = last_view[view];
            while let Some(prior) = expected {
                if self
                    .program()
                    .source_precedes(self.local_step_effect(prior).dense_ordinal(), effect)
                {
                    break;
                }
                // All earlier links were checked before they can be traversed.
                expected = self.view_predecessor(prior);
            }
            let actual = self.view_predecessor(step);
            if !matches!((actual, expected), (None, None))
                && !matches!((actual, expected), (Some(a), Some(b)) if a == b)
            {
                invalid_resident_descriptor();
            }
            last_view[view] = Some(step);
            step += 1;
        }
        self.validate_offer_alternatives();
        let mut slot = 0;
        while slot < self.columns.route_scopes.len as usize {
            let mut arm = 0;
            while arm < 2 {
                let row = self.route_arm_event_row_by_slot(slot, arm);
                let mut recv = None;
                let mut send = None;
                if !row.is_absent_or_zero_len() {
                    step = row.start();
                    while step < row.end() {
                        if matches!(self.local_step_direction(step), RoleLocalDirection::Recv) {
                            if recv.is_none() {
                                recv = Some(step);
                            }
                        } else if send.is_none() {
                            send = Some(step);
                        }
                        step += 1;
                    }
                }
                if !same_step(recv, self.route_arm_first_operation(slot, arm, true))
                    || !same_step(send, self.route_arm_first_operation(slot, arm, false))
                {
                    invalid_resident_descriptor();
                }
                arm += 1;
            }
            slot += 1;
        }
    }
}

const fn same_step(left: Option<usize>, right: Option<usize>) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => left == right,
        _ => false,
    }
}
