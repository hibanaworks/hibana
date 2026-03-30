//! Scope registry owners for typestate lowering.

use super::facts::{
    MAX_STATES, RouteRecvIndex, SCOPE_ORDINAL_INDEX_CAPACITY, SCOPE_ORDINAL_INDEX_EMPTY,
    StateIndex, state_index_to_usize,
};
use crate::{
    eff::{self, EffIndex},
    global::{
        const_dsl::{PolicyMode, ScopeId, ScopeKind},
        role_program::MAX_LANES,
    },
};

/// Marker for dispatch entries where label -> continuation is arm-agnostic.
pub(crate) const ARM_SHARED: u8 = 0xFF;
pub(crate) const MAX_FIRST_RECV_DISPATCH: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ScopeEntry {
    pub scope_id: ScopeId,
    pub kind: ScopeKind,
    pub start: StateIndex,
    pub end: StateIndex,
    pub range: u16,
    pub nest: u16,
    pub linger: bool,
    pub parent: ScopeId,
    pub route_recv_head: RouteRecvIndex,
    pub route_recv_tail: RouteRecvIndex,
    pub route_recv_len: u16,
    pub route_recv_offset: RouteRecvIndex,
    pub route_send_len: u16,
    pub route_policy: PolicyMode,
    pub route_policy_eff: EffIndex,
    pub route_policy_tag: u8,
    pub has_route_policy: bool,
    pub passive_arm_jump: [StateIndex; 2],
    pub offer_lanes: u8,
    pub offer_entry: StateIndex,
    pub lane_first_eff: [EffIndex; MAX_LANES],
    pub lane_last_eff: [EffIndex; MAX_LANES],
    pub arm_lane_last_eff: [[EffIndex; MAX_LANES]; 2],
    pub controller_arm_entry: [StateIndex; 2],
    pub controller_arm_label: [u8; 2],
    pub passive_arm_entry: [StateIndex; 2],
    pub passive_arm_scope: [ScopeId; 2],
    pub controller_role: Option<u8>,
    pub first_recv_dispatch: [(u8, u8, StateIndex); MAX_FIRST_RECV_DISPATCH],
    pub first_recv_len: u8,
    pub mergeable: bool,
}

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
pub(super) struct ScopeRecord {
    pub scope_id: ScopeId,
    pub kind: ScopeKind,
    pub start: usize,
    pub end: usize,
    pub range: u16,
    pub nest: u16,
    pub linger: bool,
    pub parent: ScopeId,
    pub route_recv_offset: RouteRecvIndex,
    pub route_recv_len: u16,
    pub present: bool,
    pub route_policy: PolicyMode,
    pub route_policy_eff: EffIndex,
    pub route_policy_tag: u8,
    pub has_route_policy: bool,
    pub passive_arm_jump: [StateIndex; 2],
    pub offer_lanes: u8,
    pub offer_lane_list: [u8; MAX_LANES],
    pub offer_lane_len: u8,
    pub offer_entry: StateIndex,
    pub lane_first_eff: [EffIndex; MAX_LANES],
    pub lane_last_eff: [EffIndex; MAX_LANES],
    pub arm_lane_last_eff: [[EffIndex; MAX_LANES]; 2],
    pub controller_arm_entry: [StateIndex; 2],
    pub controller_arm_label: [u8; 2],
    pub passive_arm_entry: [StateIndex; 2],
    pub passive_arm_scope: [ScopeId; 2],
    pub controller_role: Option<u8>,
    pub first_recv_dispatch: [(u8, u8, StateIndex); MAX_FIRST_RECV_DISPATCH],
    pub first_recv_len: u8,
    pub mergeable: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ScopeRegistry {
    pub(super) records: [ScopeRecord; eff::meta::MAX_EFF_NODES],
    pub(super) len: usize,
    pub(super) ordinal_index: [u16; SCOPE_ORDINAL_INDEX_CAPACITY],
    pub(super) route_recv_indices: [StateIndex; MAX_STATES],
    pub(super) route_recv_len: usize,
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

impl ScopeEntry {
    pub(super) const EMPTY: Self = Self {
        scope_id: ScopeId::none(),
        kind: ScopeKind::Generic,
        start: StateIndex::MAX,
        end: StateIndex::MAX,
        range: 0,
        nest: 0,
        linger: false,
        parent: ScopeId::none(),
        route_recv_head: RouteRecvIndex::MAX,
        route_recv_tail: RouteRecvIndex::MAX,
        route_recv_len: 0,
        route_recv_offset: RouteRecvIndex::ZERO,
        route_send_len: 0,
        route_policy: PolicyMode::Static,
        route_policy_eff: EffIndex::MAX,
        route_policy_tag: 0,
        has_route_policy: false,
        passive_arm_jump: [StateIndex::MAX, StateIndex::MAX],
        offer_lanes: 0,
        offer_entry: StateIndex::MAX,
        lane_first_eff: [EffIndex::MAX; MAX_LANES],
        lane_last_eff: [EffIndex::MAX; MAX_LANES],
        arm_lane_last_eff: [[EffIndex::MAX; MAX_LANES]; 2],
        controller_arm_entry: [StateIndex::MAX, StateIndex::MAX],
        controller_arm_label: [0, 0],
        passive_arm_entry: [StateIndex::MAX, StateIndex::MAX],
        passive_arm_scope: [ScopeId::none(), ScopeId::none()],
        controller_role: None,
        first_recv_dispatch: [(0, 0, StateIndex::MAX); MAX_FIRST_RECV_DISPATCH],
        first_recv_len: 0,
        mergeable: false,
    };
}

impl ScopeRecord {
    const EMPTY: Self = Self {
        scope_id: ScopeId::none(),
        kind: ScopeKind::Generic,
        start: 0,
        end: 0,
        range: 0,
        nest: 0,
        linger: false,
        parent: ScopeId::none(),
        route_recv_offset: RouteRecvIndex::ZERO,
        route_recv_len: 0,
        present: false,
        route_policy: PolicyMode::Static,
        route_policy_eff: EffIndex::MAX,
        route_policy_tag: 0,
        has_route_policy: false,
        passive_arm_jump: [StateIndex::MAX, StateIndex::MAX],
        offer_lanes: 0,
        offer_lane_list: [0; MAX_LANES],
        offer_lane_len: 0,
        offer_entry: StateIndex::MAX,
        lane_first_eff: [EffIndex::MAX; MAX_LANES],
        lane_last_eff: [EffIndex::MAX; MAX_LANES],
        arm_lane_last_eff: [[EffIndex::MAX; MAX_LANES]; 2],
        controller_arm_entry: [StateIndex::MAX, StateIndex::MAX],
        controller_arm_label: [0, 0],
        passive_arm_entry: [StateIndex::MAX, StateIndex::MAX],
        passive_arm_scope: [ScopeId::none(), ScopeId::none()],
        controller_role: None,
        first_recv_dispatch: [(0, 0, StateIndex::MAX); MAX_FIRST_RECV_DISPATCH],
        first_recv_len: 0,
        mergeable: false,
    };

    const fn from_entry(entry: ScopeEntry) -> Self {
        if entry.scope_id.is_none() {
            panic!("scope registry entry missing scope id");
        }
        if entry.start.is_max() || entry.end.is_max() {
            panic!("scope registry entry missing bounds");
        }
        let (offer_lane_list, offer_lane_len) = offer_lane_list_from_mask(entry.offer_lanes);
        Self {
            scope_id: entry.scope_id,
            kind: entry.kind,
            start: state_index_to_usize(entry.start),
            end: state_index_to_usize(entry.end),
            range: entry.range,
            nest: entry.nest,
            linger: entry.linger,
            parent: entry.parent,
            route_recv_offset: entry.route_recv_offset,
            route_recv_len: entry.route_recv_len,
            present: true,
            route_policy: entry.route_policy,
            route_policy_eff: entry.route_policy_eff,
            route_policy_tag: entry.route_policy_tag,
            has_route_policy: entry.has_route_policy,
            passive_arm_jump: entry.passive_arm_jump,
            offer_lanes: entry.offer_lanes,
            offer_lane_list,
            offer_lane_len,
            offer_entry: entry.offer_entry,
            lane_first_eff: entry.lane_first_eff,
            lane_last_eff: entry.lane_last_eff,
            arm_lane_last_eff: entry.arm_lane_last_eff,
            controller_arm_entry: entry.controller_arm_entry,
            controller_arm_label: entry.controller_arm_label,
            passive_arm_entry: entry.passive_arm_entry,
            passive_arm_scope: entry.passive_arm_scope,
            controller_role: entry.controller_role,
            first_recv_dispatch: entry.first_recv_dispatch,
            first_recv_len: entry.first_recv_len,
            mergeable: entry.mergeable,
        }
    }

    const fn region(&self) -> ScopeRegion {
        ScopeRegion {
            scope_id: self.scope_id,
            kind: self.kind,
            start: self.start,
            end: self.end,
            range: self.range,
            nest: self.nest,
            linger: self.linger,
            controller_role: self.controller_role,
        }
    }
}

impl ScopeRegistry {
    pub(super) const fn from_scope_entries(
        entries: [ScopeEntry; eff::meta::MAX_EFF_NODES],
        len: usize,
        route_recv_indices: [StateIndex; MAX_STATES],
        route_recv_len: usize,
    ) -> Self {
        let mut registry = Self {
            records: [ScopeRecord::EMPTY; eff::meta::MAX_EFF_NODES],
            len: 0,
            ordinal_index: [SCOPE_ORDINAL_INDEX_EMPTY; SCOPE_ORDINAL_INDEX_CAPACITY],
            route_recv_indices,
            route_recv_len,
        };
        let mut idx = 0usize;
        while idx < len {
            registry = registry.insert_entry(entries[idx]);
            idx += 1;
        }
        registry
    }

    const fn insert_entry(mut self, entry: ScopeEntry) -> Self {
        if entry.scope_id.is_none() {
            return self;
        }
        let ordinal = entry.scope_id.local_ordinal() as usize;
        if ordinal >= SCOPE_ORDINAL_INDEX_CAPACITY {
            panic!("scope ordinal exceeds registry capacity");
        }
        if self.len >= eff::meta::MAX_EFF_NODES {
            panic!("scope registry exhausted");
        }
        if self.ordinal_index[ordinal] != SCOPE_ORDINAL_INDEX_EMPTY {
            panic!("duplicate scope ordinal recorded");
        }
        self.records[self.len] = ScopeRecord::from_entry(entry);
        self.ordinal_index[ordinal] = self.len as u16;
        self.len += 1;
        self
    }

    const fn lookup_record(&self, scope_id: ScopeId) -> Option<&ScopeRecord> {
        if scope_id.is_none() {
            return None;
        }
        let canonical = scope_id.canonical();
        let ordinal = canonical.local_ordinal() as usize;
        if ordinal >= SCOPE_ORDINAL_INDEX_CAPACITY {
            return None;
        }
        let slot = self.ordinal_index[ordinal];
        if slot == SCOPE_ORDINAL_INDEX_EMPTY {
            return None;
        }
        let record = &self.records[slot as usize];
        if !record.present || record.scope_id.raw() != canonical.raw() {
            return None;
        }
        Some(record)
    }

    #[inline]
    fn lookup_slot(&self, scope_id: ScopeId) -> Option<usize> {
        if scope_id.is_none() {
            return None;
        }
        let canonical = scope_id.canonical();
        let ordinal = canonical.local_ordinal() as usize;
        if ordinal >= SCOPE_ORDINAL_INDEX_CAPACITY {
            return None;
        }
        let slot = self.ordinal_index[ordinal];
        if slot == SCOPE_ORDINAL_INDEX_EMPTY {
            return None;
        }
        let slot_idx = slot as usize;
        let record = &self.records[slot_idx];
        if !record.present || record.scope_id != canonical {
            return None;
        }
        Some(slot_idx)
    }

    pub(super) fn parent_of(&self, scope_id: ScopeId) -> Option<ScopeId> {
        self.lookup_record(scope_id).and_then(|record| {
            if record.parent.is_none() {
                None
            } else {
                Some(record.parent)
            }
        })
    }

    pub(super) fn lookup_region(&self, scope_id: ScopeId) -> Option<ScopeRegion> {
        self.lookup_record(scope_id).map(ScopeRecord::region)
    }

    pub(super) fn route_recv_state(&self, scope_id: ScopeId, arm: u8) -> Option<StateIndex> {
        let record = self.lookup_record(scope_id)?;
        if record.route_recv_len == 0 {
            return None;
        }
        let arm_idx = arm as u16;
        if arm_idx >= record.route_recv_len {
            return None;
        }
        let offset = record.route_recv_offset.as_usize() + arm as usize;
        if offset >= self.route_recv_len {
            return None;
        }
        Some(self.route_recv_indices[offset])
    }

    pub(super) fn route_arm_count(&self, scope_id: ScopeId) -> Option<u16> {
        let record = self.lookup_record(scope_id)?;
        Some(record.route_recv_len)
    }

    pub(super) fn route_offer_lane_list(
        &self,
        scope_id: ScopeId,
    ) -> Option<([u8; MAX_LANES], usize)> {
        let record = self.lookup_record(scope_id)?;
        Some((record.offer_lane_list, record.offer_lane_len as usize))
    }

    pub(super) fn route_offer_entry(&self, scope_id: ScopeId) -> Option<StateIndex> {
        let record = self.lookup_record(scope_id)?;
        Some(record.offer_entry)
    }

    #[inline]
    pub(super) fn route_scope_slot(&self, scope_id: ScopeId) -> Option<usize> {
        let slot = self.lookup_slot(scope_id)?;
        let record = &self.records[slot];
        if !record.present || record.kind != ScopeKind::Route {
            return None;
        }
        Some(slot)
    }

    pub(super) fn scope_lane_first_eff(&self, scope_id: ScopeId, lane: u8) -> Option<EffIndex> {
        let record = self.lookup_record(scope_id)?;
        let lane_idx = lane as usize;
        if lane_idx >= MAX_LANES {
            return None;
        }
        let eff_index = record.lane_first_eff[lane_idx];
        if eff_index == EffIndex::MAX {
            None
        } else {
            Some(eff_index)
        }
    }

    pub(super) fn scope_lane_last_eff(&self, scope_id: ScopeId, lane: u8) -> Option<EffIndex> {
        let record = self.lookup_record(scope_id)?;
        let lane_idx = lane as usize;
        if lane_idx >= MAX_LANES {
            return None;
        }
        let eff_index = record.lane_last_eff[lane_idx];
        if eff_index == EffIndex::MAX {
            None
        } else {
            Some(eff_index)
        }
    }

    pub(super) fn scope_lane_last_eff_for_arm(
        &self,
        scope_id: ScopeId,
        arm: u8,
        lane: u8,
    ) -> Option<EffIndex> {
        let record = self.lookup_record(scope_id)?;
        if arm >= 2 {
            return None;
        }
        let lane_idx = lane as usize;
        if lane_idx >= MAX_LANES {
            return None;
        }
        let eff_index = record.arm_lane_last_eff[arm as usize][lane_idx];
        if eff_index == EffIndex::MAX {
            None
        } else {
            Some(eff_index)
        }
    }

    pub(super) fn controller_arm_entry_for_label(
        &self,
        scope_id: ScopeId,
        label: u8,
    ) -> Option<StateIndex> {
        let record = self.lookup_record(scope_id)?;
        for i in 0..2 {
            if record.controller_arm_entry[i] != StateIndex::MAX
                && record.controller_arm_label[i] == label
            {
                return Some(record.controller_arm_entry[i]);
            }
        }
        None
    }

    pub(super) fn is_at_controller_arm_entry(&self, scope_id: ScopeId, idx: StateIndex) -> bool {
        let Some(record) = self.lookup_record(scope_id) else {
            return false;
        };
        for i in 0..2 {
            if record.controller_arm_entry[i] != StateIndex::MAX
                && record.controller_arm_entry[i] == idx
            {
                return true;
            }
        }
        false
    }

    pub(super) const fn controller_arm_entry_by_arm(
        &self,
        scope_id: ScopeId,
        arm: u8,
    ) -> Option<(StateIndex, u8)> {
        let record = match self.lookup_record(scope_id) {
            Some(record) => record,
            None => return None,
        };
        if arm < 2 && record.controller_arm_entry[arm as usize].raw() != StateIndex::MAX.raw() {
            Some((
                record.controller_arm_entry[arm as usize],
                record.controller_arm_label[arm as usize],
            ))
        } else {
            None
        }
    }

    pub(super) fn route_controller(&self, scope_id: ScopeId) -> Option<(PolicyMode, EffIndex, u8)> {
        let record = self.lookup_record(scope_id)?;
        if !record.has_route_policy {
            return None;
        }
        Some((
            record.route_policy,
            record.route_policy_eff,
            record.route_policy_tag,
        ))
    }

    pub(super) fn passive_arm_jump(&self, scope_id: ScopeId, arm: u8) -> Option<StateIndex> {
        if arm >= 2 {
            return None;
        }
        let record = self.lookup_record(scope_id)?;
        let target = record.passive_arm_jump[arm as usize];
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
        let target = record.passive_arm_entry[arm as usize];
        if target == StateIndex::MAX {
            None
        } else {
            Some(target)
        }
    }

    pub(super) fn passive_arm_scope(&self, scope_id: ScopeId, arm: u8) -> Option<ScopeId> {
        if arm >= 2 {
            return None;
        }
        let record = self.lookup_record(scope_id)?;
        let target = record.passive_arm_scope[arm as usize];
        (!target.is_none()).then_some(target)
    }

    pub(super) fn first_recv_target(
        &self,
        scope_id: ScopeId,
        label: u8,
    ) -> Option<(u8, StateIndex)> {
        let record = self.lookup_record(scope_id)?;
        let len = record.first_recv_len as usize;
        for i in 0..len {
            let (entry_label, arm, target) = record.first_recv_dispatch[i];
            if entry_label == label {
                return Some((arm, target));
            }
        }
        None
    }

    #[inline]
    pub(super) const fn first_recv_dispatch_entry(
        &self,
        scope_id: ScopeId,
        idx: usize,
    ) -> Option<(u8, u8, StateIndex)> {
        let record = match self.lookup_record(scope_id) {
            Some(record) => record,
            None => return None,
        };
        if idx >= record.first_recv_len as usize {
            return None;
        }
        Some(record.first_recv_dispatch[idx])
    }
}
