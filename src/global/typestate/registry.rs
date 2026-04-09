//! Scope registry owners for typestate lowering.

use super::facts::{StateIndex, state_index_to_usize};
use crate::{
    eff,
    eff::EffIndex,
    global::{
        const_dsl::{CompactScopeId, PolicyMode, ScopeId, ScopeKind},
        role_program::MAX_LANES,
    },
};

/// Marker for dispatch entries where label -> continuation is arm-agnostic.
pub(crate) const ARM_SHARED: u8 = 0xFF;
pub(crate) const MAX_FIRST_RECV_DISPATCH: usize = 16;
pub(crate) const CONTROLLER_ROLE_NONE: u8 = u8::MAX;
pub(crate) const SCOPE_LINK_NONE: u16 = u16::MAX;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ScopeRegion {
    pub scope_id: ScopeId,
    pub kind: ScopeKind,
    pub start: usize,
    pub end: usize,
    pub range: u16,
    pub nest: u16,
    pub linger: bool,
    pub controller_role: Option<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ScopeRecord {
    pub scope_id: CompactScopeId,
    pub kind: ScopeKind,
    pub linger: bool,
    pub route_policy_tag: u8,
    pub controller_role: u8,
    pub start: StateIndex,
    pub end: StateIndex,
    pub range: u16,
    pub nest: u16,
    pub parent: u16,
    pub route_policy_id: u16,
    pub route_policy_eff: EffIndex,
    pub lane_first_eff: [EffIndex; MAX_LANES],
    pub lane_last_eff: [EffIndex; MAX_LANES],
    pub arm_entry: [StateIndex; 2],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RouteScopeRecord {
    pub route_recv: [StateIndex; 2],
    pub passive_arm_jump: [StateIndex; 2],
    pub offer_lanes: u8,
    pub offer_entry: StateIndex,
    // Arm 1 always ends at the enclosing scope's per-lane last eff, so the
    // route tail only needs explicit arm-local last-eff storage for arm 0 plus
    // a presence mask for arm 1.
    pub arm0_lane_last_eff: [EffIndex; MAX_LANES],
    pub arm1_lane_mask: u8,
    pub first_recv_dispatch: [(u8, u8, StateIndex); MAX_FIRST_RECV_DISPATCH],
    pub first_recv_len: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ScopeRegistry {
    pub(super) records: *const ScopeRecord,
    pub(super) len: u16,
    pub(super) slots_by_scope: *const u16,
    pub(super) route_dense_by_slot: *const u16,
    pub(super) route_records: *const RouteScopeRecord,
    pub(super) route_scope_len: u16,
}

#[inline]
pub(super) const fn offer_lane_bit(lane: u8) -> u8 {
    if lane >= MAX_LANES as u8 {
        panic!("offer lane exceeds MAX_LANES");
    }
    1u8 << (lane as u32)
}

const fn offer_lane_list_from_mask(mask: u8) -> ([u8; MAX_LANES], u8) {
    let mut lanes = [0u8; MAX_LANES];
    let mut len = 0u8;
    let mut lane = 0u8;
    while (lane as usize) < MAX_LANES {
        if (mask & (1u8 << (lane as u32))) != 0 {
            lanes[len as usize] = lane;
            len = len + 1;
        }
        lane = lane + 1;
    }
    (lanes, len)
}

impl ScopeRecord {
    pub(crate) const EMPTY: Self = Self {
        scope_id: CompactScopeId::none(),
        kind: ScopeKind::Generic,
        linger: false,
        route_policy_tag: 0,
        controller_role: CONTROLLER_ROLE_NONE,
        start: StateIndex::ZERO,
        end: StateIndex::ZERO,
        range: 0,
        nest: 0,
        parent: SCOPE_LINK_NONE,
        route_policy_id: u16::MAX,
        route_policy_eff: EffIndex::MAX,
        lane_first_eff: [EffIndex::MAX; MAX_LANES],
        lane_last_eff: [EffIndex::MAX; MAX_LANES],
        arm_entry: [StateIndex::MAX, StateIndex::MAX],
    };

    const fn region(&self) -> ScopeRegion {
        ScopeRegion {
            scope_id: self.scope_id.to_scope_id(),
            kind: self.kind,
            start: state_index_to_usize(self.start),
            end: state_index_to_usize(self.end),
            range: self.range,
            nest: self.nest,
            linger: self.linger,
            controller_role: if self.controller_role == CONTROLLER_ROLE_NONE {
                None
            } else {
                Some(self.controller_role)
            },
        }
    }
}

impl RouteScopeRecord {
    pub(crate) const EMPTY: Self = Self {
        route_recv: [StateIndex::MAX, StateIndex::MAX],
        passive_arm_jump: [StateIndex::MAX, StateIndex::MAX],
        offer_lanes: 0,
        offer_entry: StateIndex::MAX,
        arm0_lane_last_eff: [EffIndex::MAX; MAX_LANES],
        arm1_lane_mask: 0,
        first_recv_dispatch: [(0, 0, StateIndex::MAX); MAX_FIRST_RECV_DISPATCH],
        first_recv_len: 0,
    };

    #[inline(always)]
    pub(crate) const fn route_recv_count(&self) -> u8 {
        if self.route_recv[0].is_max() {
            0
        } else if self.route_recv[1].is_max() {
            1
        } else {
            2
        }
    }

    #[inline(always)]
    const fn route_recv_state(&self, arm: u8) -> Option<StateIndex> {
        if arm >= 2 {
            return None;
        }
        if arm >= self.route_recv_count() {
            return None;
        }
        Some(self.route_recv[arm as usize])
    }
}

impl ScopeRegistry {
    #[inline(always)]
    fn records(&self) -> &[ScopeRecord] {
        if self.len == 0 {
            &[]
        } else {
            unsafe { core::slice::from_raw_parts(self.records, self.len as usize) }
        }
    }

    #[inline(always)]
    fn slots(&self) -> &[u16] {
        if self.len == 0 {
            &[]
        } else {
            unsafe { core::slice::from_raw_parts(self.slots_by_scope, self.len as usize) }
        }
    }

    #[inline(always)]
    fn route_dense_by_slot(&self) -> &[u16] {
        if self.len == 0 {
            &[]
        } else {
            unsafe { core::slice::from_raw_parts(self.route_dense_by_slot, self.len as usize) }
        }
    }

    #[inline(always)]
    fn route_records(&self) -> &[RouteScopeRecord] {
        if self.route_scope_len == 0 {
            &[]
        } else {
            unsafe {
                core::slice::from_raw_parts(self.route_records, self.route_scope_len as usize)
            }
        }
    }

    #[inline(always)]
    fn linked_record(&self, link: u16) -> Option<&ScopeRecord> {
        if link == SCOPE_LINK_NONE {
            None
        } else {
            self.records().get(link as usize)
        }
    }

    #[inline(always)]
    fn linked_scope_id(&self, link: u16) -> Option<ScopeId> {
        self.linked_record(link)
            .map(|record| record.scope_id.to_scope_id())
    }

    #[inline(always)]
    pub(super) fn record_count(&self) -> usize {
        self.len as usize
    }

    #[inline(always)]
    pub(super) fn record_at(&self, idx: usize) -> &ScopeRecord {
        &self.records()[idx]
    }

    fn lookup_slot(&self, scope_id: ScopeId) -> Option<usize> {
        if scope_id.is_none() {
            return None;
        }
        let canonical = scope_id.canonical();
        let target_raw = canonical.raw();
        let mut lo = 0usize;
        let mut hi = self.len as usize;
        while lo < hi {
            let mid = lo + ((hi - lo) / 2);
            let slot = self.slots()[mid];
            let raw = self.records()[slot as usize].scope_id.canonical().raw();
            if raw < target_raw {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        if lo >= self.len as usize {
            return None;
        }
        let slot = self.slots()[lo];
        let slot_idx = slot as usize;
        let record = &self.records()[slot_idx];
        if record.scope_id.canonical().raw() != target_raw {
            return None;
        }
        Some(slot_idx)
    }

    #[inline]
    fn lookup_record(&self, scope_id: ScopeId) -> Option<&ScopeRecord> {
        match self.lookup_slot(scope_id) {
            Some(slot) => Some(&self.records()[slot]),
            None => None,
        }
    }

    #[inline]
    pub(super) fn route_payload_at_slot(&self, slot: usize) -> Option<&RouteScopeRecord> {
        if slot >= self.len as usize {
            return None;
        }
        let dense = self.route_dense_by_slot()[slot];
        if dense == u16::MAX || dense as usize >= self.route_scope_len as usize {
            return None;
        }
        Some(&self.route_records()[dense as usize])
    }

    #[inline]
    fn lookup_route_record(&self, scope_id: ScopeId) -> Option<(&ScopeRecord, &RouteScopeRecord)> {
        let slot = self.lookup_slot(scope_id)?;
        let record = &self.records()[slot];
        if record.kind != ScopeKind::Route {
            return None;
        }
        Some((record, self.route_payload_at_slot(slot)?))
    }

    pub(super) fn parent_of(&self, scope_id: ScopeId) -> Option<ScopeId> {
        self.lookup_record(scope_id)
            .and_then(|record| self.linked_scope_id(record.parent))
    }

    pub(super) fn lookup_region(&self, scope_id: ScopeId) -> Option<ScopeRegion> {
        self.lookup_record(scope_id).map(ScopeRecord::region)
    }

    pub(super) fn route_recv_state(&self, scope_id: ScopeId, arm: u8) -> Option<StateIndex> {
        let (_record, route) = self.lookup_route_record(scope_id)?;
        route.route_recv_state(arm)
    }

    pub(super) fn route_arm_count(&self, scope_id: ScopeId) -> Option<u16> {
        let (_record, route) = self.lookup_route_record(scope_id)?;
        Some(route.route_recv_count() as u16)
    }

    pub(super) fn route_offer_lane_list(
        &self,
        scope_id: ScopeId,
    ) -> Option<([u8; MAX_LANES], usize)> {
        let (_record, route) = self.lookup_route_record(scope_id)?;
        let (lanes, len) = offer_lane_list_from_mask(route.offer_lanes);
        Some((lanes, len as usize))
    }

    pub(super) fn route_offer_entry(&self, scope_id: ScopeId) -> Option<StateIndex> {
        let (_record, route) = self.lookup_route_record(scope_id)?;
        Some(route.offer_entry)
    }

    #[inline]
    pub(super) fn route_scope_slot(&self, scope_id: ScopeId) -> Option<usize> {
        let slot = self.lookup_slot(scope_id)?;
        let record = &self.records()[slot];
        if record.kind != ScopeKind::Route {
            return None;
        }
        Some(slot)
    }

    #[inline]
    pub(super) fn route_scope_dense_ordinal(&self, slot: usize) -> Option<usize> {
        if slot >= self.len as usize {
            return None;
        }
        let dense = self.route_dense_by_slot()[slot];
        if dense == u16::MAX {
            None
        } else {
            Some(dense as usize)
        }
    }

    pub(super) fn route_scope_count(&self) -> usize {
        self.route_scope_len as usize
    }

    pub(super) fn max_offer_entries(&self) -> usize {
        let records = self.records();
        let mut max_entries = 0usize;
        let mut slot = 0usize;
        while slot < self.len as usize {
            let record = &records[slot];
            if record.kind != ScopeKind::Route {
                slot += 1;
                continue;
            }
            let route = match self.route_payload_at_slot(slot) {
                Some(route) => route,
                None => {
                    slot += 1;
                    continue;
                }
            };

            let mut unique = [StateIndex::MAX; eff::meta::MAX_EFF_NODES];
            let mut unique_len = 0usize;
            let mut push_unique = |state: StateIndex| {
                if state == StateIndex::MAX {
                    return;
                }
                let mut idx = 0usize;
                while idx < unique_len {
                    if unique[idx] == state {
                        return;
                    }
                    idx += 1;
                }
                unique[unique_len] = state;
                unique_len += 1;
            };

            push_unique(record.start);
            push_unique(route.offer_entry);
            let mut parent = record.parent;
            while let Some(parent_record) = self.linked_record(parent) {
                if parent_record.kind == ScopeKind::Route {
                    let Some(parent_route) = self.route_payload_at_slot(parent as usize) else {
                        break;
                    };
                    push_unique(parent_record.start);
                    push_unique(parent_route.offer_entry);
                }
                parent = parent_record.parent;
            }

            if unique_len > max_entries {
                max_entries = unique_len;
            }
            slot += 1;
        }
        max_entries
    }

    pub(super) fn max_route_stack_depth(&self) -> usize {
        let mut max_depth = 0usize;
        let records = self.records();
        let mut slot = 0usize;
        while slot < self.len as usize {
            let record = &records[slot];
            if record.kind != ScopeKind::Route {
                slot += 1;
                continue;
            }

            let mut depth = 1usize;
            let mut parent = record.parent;
            while let Some(parent_record) = self.linked_record(parent) {
                if parent_record.kind == ScopeKind::Route {
                    depth += 1;
                }
                if parent_record.parent == SCOPE_LINK_NONE {
                    break;
                }
                parent = parent_record.parent;
            }
            if depth > max_depth {
                max_depth = depth;
            }
            slot += 1;
        }
        max_depth
    }

    pub(super) fn max_loop_stack_depth(&self) -> usize {
        let mut max_depth = 0usize;
        let records = self.records();
        let mut slot = 0usize;
        while slot < self.len as usize {
            let record = &records[slot];
            if record.kind != ScopeKind::Loop {
                slot += 1;
                continue;
            }

            let mut depth = 1usize;
            let mut parent = record.parent;
            while let Some(parent_record) = self.linked_record(parent) {
                if parent_record.kind == ScopeKind::Loop {
                    depth += 1;
                }
                if parent_record.parent == SCOPE_LINK_NONE {
                    break;
                }
                parent = parent_record.parent;
            }
            if depth > max_depth {
                max_depth = depth;
            }
            slot += 1;
        }
        max_depth
    }

    pub(super) fn scope_lane_first_eff_in_record(
        &self,
        record: &ScopeRecord,
        lane: u8,
    ) -> Option<EffIndex> {
        if lane as usize >= MAX_LANES {
            return None;
        }
        let eff_index = record.lane_first_eff[lane as usize];
        if eff_index == EffIndex::MAX {
            None
        } else {
            Some(eff_index)
        }
    }

    pub(super) fn scope_lane_last_eff_in_record(
        &self,
        record: &ScopeRecord,
        lane: u8,
    ) -> Option<EffIndex> {
        if lane as usize >= MAX_LANES {
            return None;
        }
        let eff_index = record.lane_last_eff[lane as usize];
        if eff_index == EffIndex::MAX {
            None
        } else {
            Some(eff_index)
        }
    }

    pub(super) fn scope_lane_first_eff(&self, scope_id: ScopeId, lane: u8) -> Option<EffIndex> {
        let record = self.lookup_record(scope_id)?;
        self.scope_lane_first_eff_in_record(record, lane)
    }

    pub(super) fn scope_lane_last_eff(&self, scope_id: ScopeId, lane: u8) -> Option<EffIndex> {
        let record = self.lookup_record(scope_id)?;
        self.scope_lane_last_eff_in_record(record, lane)
    }

    pub(super) fn scope_lane_last_eff_for_arm(
        &self,
        scope_id: ScopeId,
        arm: u8,
        lane: u8,
    ) -> Option<EffIndex> {
        let (record, route) = self.lookup_route_record(scope_id)?;
        if arm >= 2 {
            return None;
        }
        let lane_idx = lane as usize;
        if lane_idx >= MAX_LANES {
            return None;
        }
        if arm == 0 {
            let eff_index = route.arm0_lane_last_eff[lane_idx];
            if eff_index == EffIndex::MAX {
                None
            } else {
                Some(eff_index)
            }
        } else if (route.arm1_lane_mask & offer_lane_bit(lane)) == 0 {
            None
        } else {
            self.scope_lane_last_eff_in_record(record, lane)
        }
    }

    pub(super) fn controller_arm_entry(&self, scope_id: ScopeId, arm: u8) -> Option<StateIndex> {
        let record = match self.lookup_record(scope_id) {
            Some(record) => record,
            None => return None,
        };
        if arm < 2 && record.arm_entry[arm as usize].raw() != StateIndex::MAX.raw() {
            Some(record.arm_entry[arm as usize])
        } else {
            None
        }
    }
    pub(super) fn route_controller(&self, scope_id: ScopeId) -> Option<(PolicyMode, EffIndex, u8)> {
        let record = self.lookup_record(scope_id)?;
        if record.route_policy_eff == EffIndex::MAX {
            return None;
        }
        let policy = if record.route_policy_id == u16::MAX {
            PolicyMode::Static
        } else {
            PolicyMode::Dynamic {
                policy_id: record.route_policy_id,
                scope: record.scope_id,
            }
        };
        Some((policy, record.route_policy_eff, record.route_policy_tag))
    }

    pub(super) fn passive_arm_jump(&self, scope_id: ScopeId, arm: u8) -> Option<StateIndex> {
        if arm >= 2 {
            return None;
        }
        let (_record, route) = self.lookup_route_record(scope_id)?;
        let target = route.passive_arm_jump[arm as usize];
        if target == StateIndex::MAX {
            None
        } else {
            Some(target)
        }
    }

    pub(super) fn passive_arm_entry(&self, scope_id: ScopeId, arm: u8) -> Option<StateIndex> {
        if arm >= 2 {
            return None;
        }
        let record = self.lookup_record(scope_id)?;
        let target = record.arm_entry[arm as usize];
        if target == StateIndex::MAX {
            None
        } else {
            Some(target)
        }
    }

    #[inline]
    pub(super) fn first_recv_dispatch_entry(
        &self,
        scope_id: ScopeId,
        idx: usize,
    ) -> Option<(u8, u8, StateIndex)> {
        let route = match self.lookup_route_record(scope_id) {
            Some((_record, route)) => route,
            None => return None,
        };
        if idx >= route.first_recv_len as usize {
            return None;
        }
        Some(route.first_recv_dispatch[idx])
    }

    pub(super) fn first_recv_dispatch_target_for_label(
        &self,
        scope_id: ScopeId,
        label: u8,
    ) -> Option<(u8, StateIndex)> {
        let route = match self.lookup_route_record(scope_id) {
            Some((_record, route)) => route,
            None => return None,
        };
        let mut idx = 0usize;
        while idx < route.first_recv_len as usize {
            let (entry_label, arm, target) = route.first_recv_dispatch[idx];
            if entry_label == label {
                return Some((arm, target));
            }
            idx += 1;
        }
        None
    }
}
