use super::{RoleImageRef, invalid_resident_descriptor};
use crate::global::role_program::RoleLocalDirection;

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
        let prefix = (u32::from(view) << 24) | (u32::from(lane) << 16);
        let key = prefix | range.start() as u32;
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
        if self.view_lane_step_key(step) >> 16 != prefix >> 16 || step >= range.end() {
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
