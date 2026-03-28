//! Typestate lowering and builder internals.

use super::facts::{
    JumpReason, LocalAction, LocalNode, MAX_STATES, RouteRecvIndex, SCOPE_ORDINAL_INDEX_CAPACITY,
    SCOPE_ORDINAL_INDEX_EMPTY, StateIndex, as_eff_index, as_state_index, state_index_to_usize,
};
use crate::{
    eff::{self, EffIndex, EffStruct},
    global::{
        LoopControlMeaning,
        compiled::{LoweringSummary, LoweringView},
        const_dsl::{PolicyMode, ScopeEvent, ScopeId, ScopeKind},
        role_program::{MAX_LANES, MAX_PHASES, MAX_STEPS},
    },
};

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
    /// Jump node indices for passive observers in route scopes (both linger and non-linger).
    /// passive_arm_jump[arm] = Jump node index for that arm.
    /// Set to u16::MAX if no passive observer Jump exists for that arm.
    pub passive_arm_jump: [StateIndex; 2],
    /// Lane bitmask for the first recv nodes in this route scope.
    /// Used by offer() to determine which lanes to poll without O(n) scan.
    /// Set to 0 if no recv node exists in the scope.
    pub offer_lanes: u8,
    /// Entry index where offer() is expected to run for this scope.
    /// u16::MAX disables the entry check (e.g., linger routes).
    pub offer_entry: StateIndex,
    /// First eff_index observed in this scope for each lane.
    /// EffIndex::MAX means no steps for that lane within this scope.
    pub lane_first_eff: [EffIndex; MAX_LANES],
    /// Last eff_index observed in this scope for each lane.
    /// EffIndex::MAX means no steps for that lane within this scope.
    pub lane_last_eff: [EffIndex; MAX_LANES],
    /// Last eff_index observed in this scope for each lane within each route arm.
    /// EffIndex::MAX means no steps for that lane within that arm.
    pub arm_lane_last_eff: [[EffIndex; MAX_LANES]; 2],
    /// Controller arm entry indices for route/loop scopes.
    /// Each arm's first self-send (CanonicalControl) decision node index.
    /// u16::MAX = arm not present.
    pub controller_arm_entry: [StateIndex; 2],
    /// Controller arm labels for O(1) lookup in flow().
    /// Stores the label of each arm's entry point.
    pub controller_arm_label: [u8; 2],
    /// Passive observer arm entry indices for route/loop scopes.
    /// Each arm's first cross-role node (Send or Recv) index.
    /// u16::MAX = arm not present or not yet set.
    pub passive_arm_entry: [StateIndex; 2],
    /// First nested route scope containing the passive arm entry.
    /// ScopeId::none() means the arm materializes directly to a node in this scope.
    pub passive_arm_scope: [ScopeId; 2],
    /// Controller role for Route scopes.
    /// Propagated from ScopeMarker::controller_role (derived from the route arm entry).
    /// `None` if this role is a passive observer or the scope is not a Route.
    pub controller_role: Option<u8>,
    /// FIRST-recv dispatch table for passive observers.
    /// Maps recv label → (arm, target_idx) for O(1) nested route resolution.
    /// `first_recv_dispatch[i] = (label, arm, target_idx)` where:
    /// - `label` is the recv label
    /// - `arm` is the route arm (0 or 1), or ARM_SHARED when label maps to the same continuation
    /// - `target_idx` is the StateIndex of the leaf recv node
    /// Entries with label=0 and target=u16::MAX are unused.
    pub first_recv_dispatch: [(u8, u8, StateIndex); MAX_FIRST_RECV_DISPATCH],
    /// Number of valid entries in first_recv_dispatch.
    pub first_recv_len: u8,
    /// True when this role's arms are mergeable and the route can be elided locally.
    pub mergeable: bool,
}

/// Marker for dispatch entries where label → continuation is arm-agnostic.
pub(crate) const ARM_SHARED: u8 = 0xFF;
pub(crate) const MAX_FIRST_RECV_DISPATCH: usize = 16;

const fn offer_lane_bit(lane: u8) -> u8 {
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
    const EMPTY: Self = Self {
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

/// Two route policies are considered the same if they share policy_id and scope.
const fn route_policy_differs(existing: PolicyMode, new_policy: PolicyMode) -> bool {
    match (existing, new_policy) {
        (
            PolicyMode::Dynamic {
                policy_id: existing_policy,
                scope: existing_scope,
                ..
            },
            PolicyMode::Dynamic {
                policy_id: new_policy,
                scope: new_scope,
                ..
            },
        ) => existing_policy != new_policy || existing_scope.raw() != new_scope.raw(),
        _ => true,
    }
}

const fn actions_equivalent(
    left: LocalAction,
    right: LocalAction,
    left_next: StateIndex,
    right_next: StateIndex,
) -> bool {
    match (left, right) {
        (
            LocalAction::Send {
                peer: left_peer,
                label: left_label,
                lane: left_lane,
                ..
            },
            LocalAction::Send {
                peer: right_peer,
                label: right_label,
                lane: right_lane,
                ..
            },
        ) => left_peer == right_peer && left_label == right_label && left_lane == right_lane,
        (
            LocalAction::Recv {
                peer: left_peer,
                label: left_label,
                lane: left_lane,
                ..
            },
            LocalAction::Recv {
                peer: right_peer,
                label: right_label,
                lane: right_lane,
                ..
            },
        ) => left_peer == right_peer && left_label == right_label && left_lane == right_lane,
        (
            LocalAction::Local {
                label: left_label,
                lane: left_lane,
                ..
            },
            LocalAction::Local {
                label: right_label,
                lane: right_lane,
                ..
            },
        ) => left_label == right_label && left_lane == right_lane,
        (LocalAction::Terminate, LocalAction::Terminate) => true,
        (
            LocalAction::Jump {
                reason: left_reason,
            },
            LocalAction::Jump {
                reason: right_reason,
            },
        ) => jump_reason_eq(left_reason, right_reason) && left_next.raw() == right_next.raw(),
        _ => false,
    }
}

const fn jump_reason_eq(left: JumpReason, right: JumpReason) -> bool {
    matches!(
        (left, right),
        (JumpReason::RouteArmEnd, JumpReason::RouteArmEnd)
            | (JumpReason::LoopContinue, JumpReason::LoopContinue)
            | (JumpReason::LoopBreak, JumpReason::LoopBreak)
            | (
                JumpReason::PassiveObserverBranch,
                JumpReason::PassiveObserverBranch
            )
    )
}

const fn arm_sequences_equal(
    nodes: &[LocalNode; MAX_STATES],
    scope_end: StateIndex,
    arm0_entry: StateIndex,
    arm1_entry: StateIndex,
) -> bool {
    if arm0_entry.is_max() || arm1_entry.is_max() {
        return false;
    }
    let end = state_index_to_usize(scope_end);
    let mut idx0 = state_index_to_usize(arm0_entry);
    let mut idx1 = state_index_to_usize(arm1_entry);
    let mut steps = 0usize;
    while steps < MAX_STATES {
        if idx0 < end {
            let node0 = nodes[idx0];
            if matches!(
                node0.action(),
                LocalAction::Jump {
                    reason: JumpReason::RouteArmEnd | JumpReason::LoopBreak
                }
            ) {
                idx0 = end;
            }
        }
        if idx1 < end {
            let node1 = nodes[idx1];
            if matches!(
                node1.action(),
                LocalAction::Jump {
                    reason: JumpReason::RouteArmEnd | JumpReason::LoopBreak
                }
            ) {
                idx1 = end;
            }
        }
        let at_end0 = idx0 >= end;
        let at_end1 = idx1 >= end;
        if at_end0 && at_end1 {
            return true;
        }
        if at_end0 || at_end1 {
            return false;
        }
        let node0 = nodes[idx0];
        let node1 = nodes[idx1];
        if !actions_equivalent(node0.action(), node1.action(), node0.next(), node1.next()) {
            return false;
        }
        let next0 = node0.next();
        let next1 = node1.next();
        idx0 = if next0.is_max() {
            end
        } else {
            state_index_to_usize(next0)
        };
        idx1 = if next1.is_max() {
            end
        } else {
            state_index_to_usize(next1)
        };
        steps += 1;
    }
    false
}

const fn continuations_equivalent(
    nodes: &[LocalNode; MAX_STATES],
    scope_end: StateIndex,
    left_entry: StateIndex,
    right_entry: StateIndex,
) -> bool {
    if left_entry.raw() == right_entry.raw() {
        return true;
    }
    arm_sequences_equal(nodes, scope_end, left_entry, right_entry)
}

const fn arm_common_prefix_end(
    nodes: &[LocalNode; MAX_STATES],
    scope: ScopeId,
    scope_end: StateIndex,
    arm0_entry: StateIndex,
    arm1_entry: StateIndex,
) -> (StateIndex, StateIndex, usize) {
    if arm0_entry.is_max() || arm1_entry.is_max() {
        return (arm0_entry, arm1_entry, 0);
    }
    let end = state_index_to_usize(scope_end);
    let scope_raw = scope.raw();
    let mut worklist = [(StateIndex::MAX, StateIndex::MAX); MAX_PREFIX_ACTIONS];
    worklist[0] = (arm0_entry, arm1_entry);
    let mut work_len = 1usize;
    let mut prefix_len = 0usize;
    let mut end0 = arm0_entry;
    let mut end1 = arm1_entry;

    while work_len > 0 {
        work_len -= 1;
        let (mut idx0, mut idx1) = worklist[work_len];
        let mut idx0_us = state_index_to_usize(idx0);
        let mut idx1_us = state_index_to_usize(idx1);

        if idx0_us < end {
            let node0 = nodes[idx0_us];
            if matches!(
                node0.action(),
                LocalAction::Jump {
                    reason: JumpReason::RouteArmEnd | JumpReason::LoopBreak
                }
            ) {
                idx0_us = end;
                idx0 = scope_end;
            }
        }
        if idx1_us < end {
            let node1 = nodes[idx1_us];
            if matches!(
                node1.action(),
                LocalAction::Jump {
                    reason: JumpReason::RouteArmEnd | JumpReason::LoopBreak
                }
            ) {
                idx1_us = end;
                idx1 = scope_end;
            }
        }

        let at_end0 = idx0_us >= end;
        let at_end1 = idx1_us >= end;
        if at_end0 || at_end1 {
            end0 = if at_end0 { scope_end } else { idx0 };
            end1 = if at_end1 { scope_end } else { idx1 };
            continue;
        }

        let node0 = nodes[idx0_us];
        let node1 = nodes[idx1_us];
        if node0.scope().raw() != scope_raw || node1.scope().raw() != scope_raw {
            end0 = idx0;
            end1 = idx1;
            continue;
        }
        if !actions_equivalent(node0.action(), node1.action(), node0.next(), node1.next()) {
            end0 = idx0;
            end1 = idx1;
            continue;
        }

        let next0 = node0.next();
        let next1 = node1.next();
        end0 = if next0.is_max() { scope_end } else { next0 };
        end1 = if next1.is_max() { scope_end } else { next1 };
        prefix_len += 1;

        if work_len >= worklist.len() {
            panic!("prefix merge worklist overflow");
        }
        worklist[work_len] = (end0, end1);
        work_len += 1;
    }

    (end0, end1, prefix_len)
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
    /// Controller role for Route scopes (derived from the route arm entry).
    /// `None` for non-Route scopes or when controller info is unavailable.
    pub controller_role: Option<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RouteRecvNode {
    state: StateIndex,
    next: RouteRecvIndex,
}

impl RouteRecvNode {
    const EMPTY: Self = Self {
        state: StateIndex::ZERO,
        next: RouteRecvIndex::MAX,
    };
}

#[derive(Clone, Copy, Debug)]
struct PrefixAction {
    kind: u8,
    peer: u8,
    label: u8,
    lane: u8,
}

impl PrefixAction {
    const EMPTY: Self = Self {
        kind: 0,
        peer: 0,
        label: 0,
        lane: 0,
    };
}

const MAX_PREFIX_ACTIONS: usize = eff::meta::MAX_EFF_NODES;

const fn prefix_action_eq(left: PrefixAction, right: PrefixAction) -> bool {
    left.kind == right.kind
        && left.peer == right.peer
        && left.label == right.label
        && left.lane == right.lane
}

const PREFIX_KIND_SEND: u8 = 0;
const PREFIX_KIND_LOCAL: u8 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ScopeRecord {
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
    /// PassiveObserverBranch Jump targets for each arm (0 and 1).
    /// u16::MAX means no Jump is registered for that arm.
    pub passive_arm_jump: [StateIndex; 2],
    /// Lane bitmask for the first recv nodes in this route scope.
    pub offer_lanes: u8,
    /// Lane list for the first recv nodes in this route scope.
    pub offer_lane_list: [u8; MAX_LANES],
    /// Number of lanes stored in offer_lane_list.
    pub offer_lane_len: u8,
    /// Entry index where offer() is expected to run for this scope.
    /// u16::MAX disables the entry check (e.g., linger routes).
    pub offer_entry: StateIndex,
    /// First eff_index observed in this scope for each lane.
    /// EffIndex::MAX means no steps for that lane within this scope.
    pub lane_first_eff: [EffIndex; MAX_LANES],
    /// Last eff_index observed in this scope for each lane.
    /// EffIndex::MAX means no steps for that lane within this scope.
    pub lane_last_eff: [EffIndex; MAX_LANES],
    /// Last eff_index observed in this scope for each lane within each route arm.
    /// EffIndex::MAX means no steps for that lane within that arm.
    pub arm_lane_last_eff: [[EffIndex; MAX_LANES]; 2],
    /// Controller arm entry indices.
    pub controller_arm_entry: [StateIndex; 2],
    /// Controller arm labels.
    pub controller_arm_label: [u8; 2],
    /// Passive observer arm entry indices.
    pub passive_arm_entry: [StateIndex; 2],
    /// First nested route scope containing the passive arm entry.
    pub passive_arm_scope: [ScopeId; 2],
    /// Controller role for Route scopes (derived from the route arm entry).
    /// `None` for non-Route scopes or when controller info is unavailable.
    pub controller_role: Option<u8>,
    /// FIRST-recv dispatch table for passive observers.
    /// Maps recv label → (arm, target_idx) for O(1) nested route resolution.
    /// `first_recv_dispatch[i] = (label, arm, target_idx)`.
    pub first_recv_dispatch: [(u8, u8, StateIndex); MAX_FIRST_RECV_DISPATCH],
    /// Number of valid entries in first_recv_dispatch.
    pub first_recv_len: u8,
    /// True when this role's arms are mergeable and the route can be elided locally.
    pub mergeable: bool,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ScopeRegistry {
    records: [ScopeRecord; eff::meta::MAX_EFF_NODES],
    len: usize,
    ordinal_index: [u16; SCOPE_ORDINAL_INDEX_CAPACITY],
    route_recv_indices: [StateIndex; MAX_STATES],
    route_recv_len: usize,
}

impl ScopeRegistry {
    const fn from_scope_entries(
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

    fn parent_of(&self, scope_id: ScopeId) -> Option<ScopeId> {
        self.lookup_record(scope_id).and_then(|record| {
            if record.parent.is_none() {
                None
            } else {
                Some(record.parent)
            }
        })
    }

    fn lookup_region(&self, scope_id: ScopeId) -> Option<ScopeRegion> {
        self.lookup_record(scope_id).map(ScopeRecord::region)
    }

    fn route_recv_state(&self, scope_id: ScopeId, arm: u8) -> Option<StateIndex> {
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

    fn route_arm_count(&self, scope_id: ScopeId) -> Option<u16> {
        let record = self.lookup_record(scope_id)?;
        Some(record.route_recv_len)
    }

    fn route_offer_lane_list(&self, scope_id: ScopeId) -> Option<([u8; MAX_LANES], usize)> {
        let record = self.lookup_record(scope_id)?;
        Some((record.offer_lane_list, record.offer_lane_len as usize))
    }

    fn route_offer_entry(&self, scope_id: ScopeId) -> Option<StateIndex> {
        let record = self.lookup_record(scope_id)?;
        Some(record.offer_entry)
    }

    #[inline]
    fn route_scope_slot(&self, scope_id: ScopeId) -> Option<usize> {
        let slot = self.lookup_slot(scope_id)?;
        let record = &self.records[slot];
        if !record.present || record.kind != ScopeKind::Route {
            return None;
        }
        Some(slot)
    }

    fn scope_lane_first_eff(&self, scope_id: ScopeId, lane: u8) -> Option<EffIndex> {
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

    fn scope_lane_last_eff(&self, scope_id: ScopeId, lane: u8) -> Option<EffIndex> {
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

    fn scope_lane_last_eff_for_arm(
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

    /// Get the controller arm entry index for a given label.
    /// Returns the StateIndex of the arm whose label matches, or None if not found.
    fn controller_arm_entry_for_label(&self, scope_id: ScopeId, label: u8) -> Option<StateIndex> {
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

    /// Check if a given state index is at a controller arm entry for this scope.
    /// Returns true if the index matches controller_arm_entry[0] or controller_arm_entry[1].
    fn is_at_controller_arm_entry(&self, scope_id: ScopeId, idx: StateIndex) -> bool {
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

    /// Get the controller arm entry (index, label) for a given arm number.
    /// Returns (StateIndex, label) if the arm exists, None otherwise.
    const fn controller_arm_entry_by_arm(
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

    fn route_controller(&self, scope_id: ScopeId) -> Option<(PolicyMode, EffIndex, u8)> {
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

    /// Get the PassiveObserverBranch Jump target for the specified arm.
    ///
    /// Returns the StateIndex of the Jump node's target for the given arm (0 or 1),
    /// or `None` if no PassiveObserverBranch Jump is registered for that arm.
    fn passive_arm_jump(&self, scope_id: ScopeId, arm: u8) -> Option<StateIndex> {
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

    /// Get the passive arm entry index for the specified arm.
    ///
    /// Returns the StateIndex of the first cross-role node (Send or Recv) in the arm,
    /// or `None` if not set.
    fn passive_arm_entry(&self, scope_id: ScopeId, arm: u8) -> Option<StateIndex> {
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

    fn passive_arm_scope(&self, scope_id: ScopeId, arm: u8) -> Option<ScopeId> {
        if arm >= 2 {
            return None;
        }
        let record = self.lookup_record(scope_id)?;
        let target = record.passive_arm_scope[arm as usize];
        (!target.is_none()).then_some(target)
    }

    /// FIRST-recv dispatch lookup for passive observers.
    ///
    /// Given a recv label, returns the leaf recv StateIndex that handles that label.
    /// This flattens nested routes: the returned index points directly to the
    /// innermost recv node, not to intermediate route scope entries.
    ///
    /// Returns `(arm, target_idx)` where `arm` is the route arm (0 or 1) and
    /// `target_idx` is the StateIndex of the recv node.
    ///
    /// Returns `None` if:
    /// - Label not found in dispatch table
    ///
    /// O(n) scan where n ≤ 8 (fixed dispatch table; bounded and no-alloc friendly).
    fn first_recv_target(&self, scope_id: ScopeId, label: u8) -> Option<(u8, StateIndex)> {
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
    const fn first_recv_dispatch_entry(
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

/// Role-specific typestate graph synthesized from a global effect list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoleTypestate<const ROLE: u8> {
    nodes: [LocalNode; MAX_STATES],
    len: usize,
    scope_registry: ScopeRegistry,
}

pub(crate) type RoleTypestateValue = RoleTypestate<0>;

const MAX_LOOP_TRACKED: usize = eff::meta::MAX_EFF_NODES;

const fn find_loop_entry_state(
    ids: &[ScopeId; MAX_LOOP_TRACKED],
    states: &[Option<StateIndex>; MAX_LOOP_TRACKED],
    len: usize,
    scope_id: ScopeId,
) -> Option<StateIndex> {
    let mut idx = 0usize;
    while idx < len {
        if ids[idx].raw() == scope_id.raw() {
            return states[idx];
        }
        idx += 1;
    }
    None
}

const fn store_loop_entry_if_absent(
    ids: &mut [ScopeId; MAX_LOOP_TRACKED],
    states: &mut [Option<StateIndex>; MAX_LOOP_TRACKED],
    len: &mut usize,
    scope_id: ScopeId,
    state: StateIndex,
) {
    let mut idx = 0usize;
    while idx < *len {
        if ids[idx].raw() == scope_id.raw() {
            if states[idx].is_none() {
                states[idx] = Some(state);
            }
            return;
        }
        idx += 1;
    }
    if *len >= MAX_LOOP_TRACKED {
        panic!("loop entry table capacity exceeded");
    }
    ids[*len] = scope_id;
    states[*len] = Some(state);
    *len += 1;
}

impl<const ROLE: u8> RoleTypestate<ROLE> {
    const fn new(
        nodes: [LocalNode; MAX_STATES],
        len: usize,
        scope_registry: ScopeRegistry,
    ) -> Self {
        Self {
            nodes,
            len,
            scope_registry,
        }
    }

    #[inline(always)]
    pub(crate) const fn into_value(self) -> RoleTypestateValue {
        RoleTypestate::<0>::new(self.nodes, self.len, self.scope_registry)
    }

    /// Number of nodes present in the typestate (including the terminal node).
    #[inline(always)]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Access a node by index.
    #[inline(always)]
    pub(crate) const fn node(&self, index: usize) -> LocalNode {
        self.nodes[index]
    }

    pub(in crate::global::typestate) fn scope_region_for(
        &self,
        scope_id: ScopeId,
    ) -> Option<ScopeRegion> {
        self.scope_registry.lookup_region(scope_id)
    }

    pub(in crate::global::typestate) fn scope_parent(&self, scope_id: ScopeId) -> Option<ScopeId> {
        self.scope_registry.parent_of(scope_id)
    }

    /// Get the PassiveObserverBranch Jump target for the specified arm in a scope.
    ///
    /// Returns the StateIndex of the Jump's target node for the given arm (0 or 1),
    /// or `None` if no PassiveObserverBranch Jump is registered for that arm.
    pub(in crate::global::typestate) fn passive_arm_jump(
        &self,
        scope_id: ScopeId,
        arm: u8,
    ) -> Option<StateIndex> {
        self.scope_registry.passive_arm_jump(scope_id, arm)
    }

    /// Get the passive arm entry index for the specified arm.
    ///
    /// Returns the StateIndex of the first cross-role node (Send or Recv) in the arm,
    /// or `None` if not set.
    pub(in crate::global::typestate) fn passive_arm_entry(
        &self,
        scope_id: ScopeId,
        arm: u8,
    ) -> Option<StateIndex> {
        self.scope_registry.passive_arm_entry(scope_id, arm)
    }

    pub(in crate::global::typestate) fn passive_arm_scope(
        &self,
        scope_id: ScopeId,
        arm: u8,
    ) -> Option<ScopeId> {
        self.scope_registry.passive_arm_scope(scope_id, arm)
    }

    /// FIRST-recv dispatch lookup for passive observers.
    ///
    /// Given a recv label, returns the route arm and leaf recv StateIndex.
    /// Returns `(arm, target_idx)` where:
    /// - `arm` is the route arm (0 or 1)
    /// - `target_idx` is the StateIndex of the recv node
    ///
    /// Returns `None` if label not found.
    /// Flattens nested routes for O(1) dispatch.
    pub(crate) fn first_recv_target(
        &self,
        scope_id: ScopeId,
        label: u8,
    ) -> Option<(u8, StateIndex)> {
        self.scope_registry.first_recv_target(scope_id, label)
    }

    #[inline]
    pub(in crate::global::typestate) fn route_recv_state(
        &self,
        scope_id: ScopeId,
        arm: u8,
    ) -> Option<StateIndex> {
        self.scope_registry.route_recv_state(scope_id, arm)
    }

    #[inline]
    pub(in crate::global::typestate) fn route_arm_count(&self, scope_id: ScopeId) -> Option<u16> {
        self.scope_registry.route_arm_count(scope_id)
    }

    #[inline]
    pub(in crate::global::typestate) fn route_offer_lane_list(
        &self,
        scope_id: ScopeId,
    ) -> Option<([u8; MAX_LANES], usize)> {
        self.scope_registry.route_offer_lane_list(scope_id)
    }

    #[inline]
    pub(in crate::global::typestate) fn route_offer_entry(
        &self,
        scope_id: ScopeId,
    ) -> Option<StateIndex> {
        self.scope_registry.route_offer_entry(scope_id)
    }

    #[inline]
    pub(in crate::global) const fn first_recv_dispatch_entry(
        &self,
        scope_id: ScopeId,
        idx: usize,
    ) -> Option<(u8, u8, StateIndex)> {
        self.scope_registry.first_recv_dispatch_entry(scope_id, idx)
    }

    #[inline]
    pub(in crate::global::typestate) fn route_scope_slot(
        &self,
        scope_id: ScopeId,
    ) -> Option<usize> {
        self.scope_registry.route_scope_slot(scope_id)
    }

    #[inline]
    pub(in crate::global::typestate) fn scope_lane_first_eff(
        &self,
        scope_id: ScopeId,
        lane: u8,
    ) -> Option<EffIndex> {
        self.scope_registry.scope_lane_first_eff(scope_id, lane)
    }

    #[inline]
    pub(in crate::global::typestate) fn scope_lane_last_eff(
        &self,
        scope_id: ScopeId,
        lane: u8,
    ) -> Option<EffIndex> {
        self.scope_registry.scope_lane_last_eff(scope_id, lane)
    }

    #[inline]
    pub(in crate::global::typestate) fn scope_lane_last_eff_for_arm(
        &self,
        scope_id: ScopeId,
        arm: u8,
        lane: u8,
    ) -> Option<EffIndex> {
        self.scope_registry
            .scope_lane_last_eff_for_arm(scope_id, arm, lane)
    }

    #[inline]
    pub(in crate::global::typestate) fn controller_arm_entry_for_label(
        &self,
        scope_id: ScopeId,
        label: u8,
    ) -> Option<StateIndex> {
        self.scope_registry
            .controller_arm_entry_for_label(scope_id, label)
    }

    #[inline]
    pub(in crate::global::typestate) fn is_at_controller_arm_entry(
        &self,
        scope_id: ScopeId,
        idx: StateIndex,
    ) -> bool {
        self.scope_registry
            .is_at_controller_arm_entry(scope_id, idx)
    }

    #[inline]
    pub(in crate::global) const fn controller_arm_entry_by_arm(
        &self,
        scope_id: ScopeId,
        arm: u8,
    ) -> Option<(StateIndex, u8)> {
        self.scope_registry
            .controller_arm_entry_by_arm(scope_id, arm)
    }

    #[inline]
    pub(in crate::global::typestate) fn route_controller(
        &self,
        scope_id: ScopeId,
    ) -> Option<(PolicyMode, EffIndex, u8)> {
        self.scope_registry.route_controller(scope_id)
    }

    #[inline(always)]
    pub(in crate::global) const fn has_parallel_phase_scope(&self) -> bool {
        let mut idx = 0usize;
        while idx < self.scope_registry.len {
            let record = self.scope_registry.records[idx];
            if record.present
                && matches!(record.kind, ScopeKind::Parallel)
                && Self::parallel_phase_eff_range(record).is_some()
            {
                return true;
            }
            idx += 1;
        }
        false
    }

    #[inline(always)]
    pub(in crate::global) const fn parallel_phase_range_at(
        &self,
        ordinal: usize,
    ) -> Option<(usize, usize)> {
        let mut idx = 0usize;
        let mut seen = 0usize;
        while idx < self.scope_registry.len {
            let record = self.scope_registry.records[idx];
            if record.present
                && matches!(record.kind, ScopeKind::Parallel)
                && let Some(range) = Self::parallel_phase_eff_range(record)
            {
                if seen == ordinal {
                    return Some(range);
                }
                seen += 1;
            }
            idx += 1;
        }
        None
    }

    #[inline(always)]
    pub(in crate::global) const fn phase_route_guard_for_state(
        &self,
        state: StateIndex,
    ) -> Option<(ScopeId, u8)> {
        if state.is_max() {
            return None;
        }
        let state_idx = state_index_to_usize(state);
        let mut best_scope = ScopeId::none();
        let mut best_arm = 0u8;
        let mut best_nest = u16::MAX;
        let mut idx = 0usize;
        while idx < self.scope_registry.len {
            let record = self.scope_registry.records[idx];
            if record.present
                && matches!(record.kind, ScopeKind::Route)
                && record.start <= state_idx
                && state_idx < record.end
                && record.nest < best_nest
                && let Some(arm) = Self::phase_route_arm_for_record(record, state_idx)
            {
                best_scope = record.scope_id;
                best_arm = arm;
                best_nest = record.nest;
            }
            idx += 1;
        }
        if best_scope.is_none() {
            None
        } else {
            Some((best_scope, best_arm))
        }
    }

    #[inline(always)]
    const fn parallel_phase_eff_range(record: ScopeRecord) -> Option<(usize, usize)> {
        let mut min_eff = usize::MAX;
        let mut max_eff = 0usize;
        let mut have_lane = false;
        let mut lane_idx = 0usize;
        while lane_idx < MAX_LANES {
            let first = record.lane_first_eff[lane_idx];
            if first.raw() != EffIndex::MAX.raw() {
                let first_idx = first.as_usize();
                let last = record.lane_last_eff[lane_idx];
                if last.raw() == EffIndex::MAX.raw() {
                    panic!("parallel scope lane missing last eff index");
                }
                let last_idx = last.as_usize();
                if !have_lane || first_idx < min_eff {
                    min_eff = first_idx;
                }
                if !have_lane || last_idx > max_eff {
                    max_eff = last_idx;
                }
                have_lane = true;
            }
            lane_idx += 1;
        }
        if !have_lane {
            None
        } else {
            Some((min_eff, max_eff + 1))
        }
    }

    #[inline(always)]
    const fn phase_route_arm_for_record(record: ScopeRecord, state_idx: usize) -> Option<u8> {
        let arm0_entry = Self::phase_route_entry_for_arm(record, 0);
        let arm1_entry = Self::phase_route_entry_for_arm(record, 1);
        let mut selected_arm = None;
        let mut selected_entry = 0usize;

        if !arm0_entry.is_max() {
            let arm0_idx = state_index_to_usize(arm0_entry);
            if arm0_idx <= state_idx {
                selected_arm = Some(0);
                selected_entry = arm0_idx;
            }
        }

        if !arm1_entry.is_max() {
            let arm1_idx = state_index_to_usize(arm1_entry);
            if arm1_idx <= state_idx && (selected_arm.is_none() || arm1_idx > selected_entry) {
                selected_arm = Some(1);
            }
        }

        selected_arm
    }

    #[inline(always)]
    const fn phase_route_entry_for_arm(record: ScopeRecord, arm: usize) -> StateIndex {
        let is_controller = match record.controller_role {
            Some(role) => role == ROLE,
            None => true,
        };
        if is_controller {
            record.controller_arm_entry[arm]
        } else {
            record.passive_arm_entry[arm]
        }
    }

    pub(crate) const fn from_summary(summary: &LoweringSummary) -> Self {
        let view = summary.view();
        Self::build(view, view.as_slice())
    }

    pub(crate) const fn validate_compiled_layout(&self) {
        self.validate_phase_capacity();
        self.validate_controller_arm_table_capacity();
        self.validate_first_recv_dispatch_capacity();
    }

    const fn validate_phase_capacity(&self) {
        if self.compiled_phase_count() > MAX_PHASES {
            panic!("compiled role phase capacity exceeded");
        }
    }

    const fn validate_controller_arm_table_capacity(&self) {
        if self.compiled_controller_arm_entry_count() > ScopeId::ORDINAL_CAPACITY as usize * 2 {
            panic!("controller arm table capacity exceeded");
        }
    }

    const fn compiled_controller_arm_entry_count(&self) -> usize {
        let mut count = 0usize;
        let mut ordinal = 0usize;
        while ordinal < ScopeId::ORDINAL_CAPACITY as usize {
            let route_scope = ScopeId::route(ordinal as u16);
            let mut arm = 0u8;
            while arm <= 1 {
                if self.controller_arm_entry_by_arm(route_scope, arm).is_some() {
                    count += 1;
                }
                if arm == 1 {
                    break;
                }
                arm += 1;
            }

            let loop_scope = ScopeId::loop_scope(ordinal as u16);
            let mut loop_arm = 0u8;
            while loop_arm <= 1 {
                if self
                    .controller_arm_entry_by_arm(loop_scope, loop_arm)
                    .is_some()
                {
                    count += 1;
                }
                if loop_arm == 1 {
                    break;
                }
                loop_arm += 1;
            }

            ordinal += 1;
        }
        count
    }

    const fn validate_first_recv_dispatch_capacity(&self) {
        let mut count = 0usize;
        let mut idx = 0usize;
        while idx < self.scope_registry.len {
            let record = self.scope_registry.records[idx];
            if record.present && matches!(record.kind, ScopeKind::Route) {
                count += record.first_recv_len as usize;
                if count > ScopeId::ORDINAL_CAPACITY as usize * MAX_FIRST_RECV_DISPATCH {
                    panic!("first recv dispatch table capacity exceeded");
                }
            }
            idx += 1;
        }
    }

    const fn compiled_phase_count(&self) -> usize {
        let mut present = [false; MAX_STEPS];
        let mut local_len = 0usize;
        let mut node_idx = 0usize;
        while node_idx < self.len() {
            match self.node(node_idx).action() {
                LocalAction::Send { eff_index, .. }
                | LocalAction::Recv { eff_index, .. }
                | LocalAction::Local { eff_index, .. } => {
                    let idx = eff_index.as_usize();
                    if idx >= MAX_STEPS {
                        panic!("local step eff_index exceeds MAX_STEPS");
                    }
                    if !present[idx] {
                        present[idx] = true;
                        local_len += 1;
                    }
                }
                LocalAction::None | LocalAction::Terminate | LocalAction::Jump { .. } => {}
            }
            node_idx += 1;
        }

        if local_len == 0 {
            return 0;
        }
        if !self.has_parallel_phase_scope() {
            return 1;
        }

        let mut phase_count = 0usize;
        let mut current_eff = 0usize;
        let mut ordinal = 0usize;
        loop {
            let Some((enter_eff, exit_eff)) = self.parallel_phase_range_at(ordinal) else {
                break;
            };
            if Self::has_local_step_in_range(&present, current_eff, enter_eff) {
                phase_count += 1;
            }
            if Self::has_local_step_in_range(&present, enter_eff, exit_eff) {
                phase_count += 1;
            }
            current_eff = exit_eff;
            ordinal += 1;
        }

        if Self::has_local_step_in_range(&present, current_eff, MAX_STEPS) {
            phase_count += 1;
        }

        if phase_count == 0 { 1 } else { phase_count }
    }

    const fn has_local_step_in_range(
        present: &[bool; MAX_STEPS],
        start: usize,
        end: usize,
    ) -> bool {
        let mut idx = start;
        while idx < end && idx < MAX_STEPS {
            if present[idx] {
                return true;
            }
            idx += 1;
        }
        false
    }

    const fn build(program: LoweringView<'_>, slice: &[EffStruct]) -> Self {
        let mut loop_entry_ids = [ScopeId::generic(0); MAX_LOOP_TRACKED];
        let mut loop_entry_states = [None::<StateIndex>; MAX_LOOP_TRACKED];
        let mut loop_entry_len = 0usize;

        // Track the last node index of each arm for linger (loop) scopes.
        // Used to insert Jump nodes at arm ends.
        // Index 0 = arm 0 (Continue), Index 1 = arm 1 (Break).
        // Use usize::MAX as sentinel for "no node yet" to distinguish from node index 0.
        // Capacity = MAX_EFF_NODES (can have at most one linger scope per effect node).
        const MAX_LINGER_ARM_TRACK: usize = eff::meta::MAX_EFF_NODES;
        const LINGER_ARM_NO_NODE: usize = usize::MAX;
        let mut linger_arm_last_node = [[LINGER_ARM_NO_NODE; 2]; MAX_LINGER_ARM_TRACK];
        let mut linger_arm_scope_ids = [ScopeId::generic(0); MAX_LINGER_ARM_TRACK];
        let mut linger_arm_current = [0u8; MAX_LINGER_ARM_TRACK]; // current arm (0 or 1)
        let mut linger_arm_len = 0usize;

        // Track passive observer arm boundaries for linger (loop) scopes.
        // When another role's self-send defines an arm, passive observers need Jump targets.
        // linger_passive_arm_start[li][arm] = node_len when arm boundary was detected.
        // This allows inserting PassiveObserverBranch Jump nodes at scope exit.
        // Use usize::MAX as sentinel for "not set" to distinguish from node_len == 0.
        const PASSIVE_ARM_UNSET: usize = usize::MAX;
        let mut linger_passive_arm_start = [[PASSIVE_ARM_UNSET; 2]; MAX_LINGER_ARM_TRACK];
        // Flag indicating this scope has passive arm tracking (ROLE != controller).
        let mut linger_is_passive = [false; MAX_LINGER_ARM_TRACK];

        // Non-linger Route arm tracking for RouteArmEnd Jump generation.
        // Uses "Scope-as-Block" strategy: treat nested scopes as opaque blocks.
        // - last_step_was_scope[stack_idx]: true if last step was a scope exit
        // - route_arm_last_node[stack_idx][arm]: last node index for each arm

        // Backpatch list for Jump nodes that need their target resolved.
        // Records (node_index, scope, kind) where kind:
        // - 0 = loop_start (LoopContinue)
        // - 1 = scope_end (LoopBreak)
        // - 2 = scope_end (RouteArmEnd)
        // Capacity = MAX_STATES (at most one backpatch per node).
        const MAX_JUMP_BACKPATCH: usize = MAX_STATES;
        let mut jump_backpatch_indices = [0usize; MAX_JUMP_BACKPATCH];
        let mut jump_backpatch_scopes = [ScopeId::generic(0); MAX_JUMP_BACKPATCH];
        let mut jump_backpatch_kinds = [0u8; MAX_JUMP_BACKPATCH];
        let mut jump_backpatch_len = 0usize;

        let mut nodes = [LocalNode::EMPTY; MAX_STATES];
        let mut node_len = 0usize;
        let mut eff_idx = 0usize;

        let scope_markers = program.scope_markers();
        let mut scope_marker_idx = 0usize;
        let mut scope_stack = [ScopeId::none(); eff::meta::MAX_EFF_NODES];
        let mut scope_stack_kinds = [ScopeKind::Generic; eff::meta::MAX_EFF_NODES];
        let mut scope_stack_entries = [0usize; eff::meta::MAX_EFF_NODES];
        // Track current arm number for each route scope in the stack.
        // Starts at 0 (no arm yet), incremented when a dynamic control recv is found.
        let mut route_current_arm = [0u8; eff::meta::MAX_EFF_NODES];
        // Scope-as-Block: Track whether the last step was a scope exit (for nested route handling).
        let mut last_step_was_scope = [false; eff::meta::MAX_EFF_NODES];
        // Scope-as-Block: Track the last node index for each arm in non-linger Route scopes.
        // route_arm_last_node[stack_idx][arm] = last node index for that arm.
        let mut route_arm_last_node = [[StateIndex::MAX; 2]; eff::meta::MAX_EFF_NODES];
        // Non-linger Route passive observer tracking using is_immediate_reenter method.
        // The arm boundary is detected via Exit→Enter pairs in ScopeEvent, not via
        // other roles' self-send messages (which passive observers don't see).
        //
        // route_enter_count[stack_idx] = number of Enter events for this scope.
        // arm number = enter_count - 1 (arm 0 at first Enter, arm 1 at second Enter).
        let mut route_enter_count = [0u8; eff::meta::MAX_EFF_NODES];
        // route_passive_arm_start[stack_idx][arm] = node_len at arm start.
        // Use usize::MAX as sentinel for "not set".
        const ROUTE_PASSIVE_ARM_UNSET: usize = usize::MAX;
        let mut route_passive_arm_start = [[ROUTE_PASSIVE_ARM_UNSET; 2]; eff::meta::MAX_EFF_NODES];
        // Flag indicating this non-linger Route scope has passive tracking (ROLE != controller).
        let mut route_is_passive = [false; eff::meta::MAX_EFF_NODES];
        let mut scope_stack_len = 0usize;
        let mut scope_entries = [ScopeEntry::EMPTY; eff::meta::MAX_EFF_NODES];
        let mut scope_entries_len = 0usize;
        let mut scope_entry_index_by_ordinal =
            [SCOPE_ORDINAL_INDEX_EMPTY; SCOPE_ORDINAL_INDEX_CAPACITY];
        let mut scope_range_counter: u16 = 0;
        let mut route_recv_nodes = [RouteRecvNode::EMPTY; MAX_STATES];
        let mut route_recv_nodes_len = 0usize;

        while eff_idx <= slice.len() {
            while scope_marker_idx < scope_markers.len()
                && scope_markers[scope_marker_idx].offset == eff_idx
            {
                let marker = scope_markers[scope_marker_idx];
                let scope = marker.scope_id;
                match marker.event {
                    ScopeEvent::Enter => {
                        if scope_stack_len >= eff::meta::MAX_EFF_NODES {
                            panic!("structured scope stack overflow");
                        }
                        let parent_scope = if scope_stack_len == 0 {
                            ScopeId::none()
                        } else {
                            scope_stack[scope_stack_len - 1]
                        };
                        let ordinal = scope.local_ordinal() as usize;
                        if ordinal >= SCOPE_ORDINAL_INDEX_CAPACITY {
                            panic!("scope ordinal exceeds typestate capacity");
                        }
                        let (entry_idx, is_new_ordinal) = match scope_entry_index_by_ordinal
                            [ordinal]
                        {
                            SCOPE_ORDINAL_INDEX_EMPTY => {
                                if scope_entries_len >= eff::meta::MAX_EFF_NODES {
                                    panic!("structured scope metadata overflow");
                                }
                                if scope_range_counter == u16::MAX {
                                    panic!("scope range ordinal overflow");
                                }
                                scope_entry_index_by_ordinal[ordinal] = scope_entries_len as u16;
                                let idx = scope_entries_len;
                                scope_entries[idx] = ScopeEntry::EMPTY;
                                scope_entries[idx].scope_id = scope;
                                scope_entries[idx].kind = marker.scope_kind;
                                scope_entries[idx].linger = marker.linger;
                                scope_entries[idx].parent = parent_scope;
                                scope_entries[idx].range = scope_range_counter;
                                scope_entries[idx].nest = scope_stack_len as u16;
                                scope_range_counter = scope_range_counter.wrapping_add(1);
                                scope_entries_len += 1;
                                (idx, true)
                            }
                            existing => (existing as usize, false),
                        };
                        scope_stack[scope_stack_len] = scope;
                        scope_stack_kinds[scope_stack_len] = marker.scope_kind;
                        scope_stack_entries[scope_stack_len] = entry_idx;
                        // Initialize route tracking arrays only for NEW scope ordinals.
                        // This ensures seq(ROUTE1, ROUTE2) starts ROUTE2 at arm 0,
                        // while preserving arm count when re-entering the same route
                        // scope (e.g., different arms within the same binary route).
                        if is_new_ordinal {
                            route_current_arm[scope_stack_len] = 0;
                            route_enter_count[scope_stack_len] = 0;
                            route_passive_arm_start[scope_stack_len] =
                                [ROUTE_PASSIVE_ARM_UNSET, ROUTE_PASSIVE_ARM_UNSET];
                            route_is_passive[scope_stack_len] = false;
                            route_arm_last_node[scope_stack_len] =
                                [StateIndex::MAX, StateIndex::MAX];
                            last_step_was_scope[scope_stack_len] = false;
                        }
                        scope_stack_len += 1;

                        // Update entry fields (short borrow scope)
                        {
                            let entry = &mut scope_entries[entry_idx];
                            if marker.linger {
                                entry.linger = true;
                            }
                            if !entry.parent.is_none() && entry.parent.raw() != parent_scope.raw() {
                                panic!("scope parent mismatch for ordinal");
                            }
                            if entry.start.is_max() {
                                entry.start = as_state_index(node_len);
                            }
                            // Propagate controller_role from ScopeMarker to ScopeEntry.
                            // This allows type-level controller detection instead of runtime inference.
                            if marker.controller_role.is_some() && entry.controller_role.is_none() {
                                entry.controller_role = marker.controller_role;
                            }
                        }

                        // Linger scope tracking for Jump insertion
                        if marker.linger && is_new_ordinal {
                            if linger_arm_len >= MAX_LINGER_ARM_TRACK {
                                panic!("linger arm tracking capacity exceeded");
                            }
                            linger_arm_scope_ids[linger_arm_len] = scope;
                            linger_arm_last_node[linger_arm_len] =
                                [LINGER_ARM_NO_NODE, LINGER_ARM_NO_NODE];
                            linger_arm_current[linger_arm_len] = 0;
                            linger_passive_arm_start[linger_arm_len] =
                                [PASSIVE_ARM_UNSET, PASSIVE_ARM_UNSET];
                            linger_is_passive[linger_arm_len] = false;
                            linger_arm_len += 1;
                        }

                        // Nested scope passive_arm_entry propagation
                        // Note: scope_stack_len was already incremented above, so the parent
                        // is at scope_stack_len - 2, not scope_stack_len - 1 (which is "self").
                        if scope_stack_len >= 2 {
                            let parent_idx = scope_stack_len - 2;
                            if matches!(scope_stack_kinds[parent_idx], ScopeKind::Route) {
                                let parent_entry_idx = scope_stack_entries[parent_idx];
                                let arm = route_current_arm[parent_idx] as usize;
                                if arm < 2
                                    && scope_entries[parent_entry_idx].passive_arm_entry[arm]
                                        .is_max()
                                    && scope_entries[parent_entry_idx].passive_arm_scope[arm]
                                        .is_none()
                                    && matches!(marker.scope_kind, ScopeKind::Route)
                                {
                                    scope_entries[parent_entry_idx].passive_arm_scope[arm] = scope;
                                }
                                if arm < 2
                                    && scope_entries[parent_entry_idx].passive_arm_entry[arm]
                                        .is_max()
                                {
                                    scope_entries[parent_entry_idx].passive_arm_entry[arm] =
                                        as_state_index(node_len);
                                }
                            }
                        }

                        // Route arm tracking via ScopeMarker Enter events (binary route invariant)
                        if matches!(marker.scope_kind, ScopeKind::Route) {
                            let stack_idx = scope_stack_len - 1;
                            route_enter_count[stack_idx] = route_enter_count[stack_idx]
                                .checked_add(1)
                                .expect("route enter count overflow");
                            if route_enter_count[stack_idx] > 2 {
                                panic!("route must have exactly 2 arms (Enter count > 2)");
                            }
                            route_current_arm[stack_idx] = route_enter_count[stack_idx] - 1;
                            let arm = route_current_arm[stack_idx] as usize;
                            route_arm_last_node[stack_idx][arm] = StateIndex::MAX;
                            last_step_was_scope[stack_idx] = false;

                            // At first Enter (enter_count == 1), set route policy from EffList.
                            // This keeps route policy metadata independent of role projection.
                            if route_enter_count[stack_idx] == 1
                                && !scope_entries[entry_idx].has_route_policy
                            {
                                let scope_start = marker.offset;
                                let mut scope_end = slice.len();
                                let mut scan_idx = scope_marker_idx + 1;
                                let mut nest_depth = 1usize;
                                while scan_idx < scope_markers.len() {
                                    let scan_marker = scope_markers[scan_idx];
                                    if scan_marker.scope_id.local_ordinal() == scope.local_ordinal()
                                    {
                                        match scan_marker.event {
                                            ScopeEvent::Enter => nest_depth += 1,
                                            ScopeEvent::Exit => {
                                                nest_depth -= 1;
                                                if nest_depth == 0 {
                                                    scope_end = scan_marker.offset;
                                                    break;
                                                }
                                            }
                                        }
                                    }
                                    scan_idx += 1;
                                }
                                if let Some((policy, eff_offset, tag)) =
                                    program.first_dynamic_policy_in_range(scope_start, scope_end)
                                {
                                    scope_entries[entry_idx].route_policy =
                                        policy.with_scope(scope);
                                    scope_entries[entry_idx].route_policy_eff =
                                        as_eff_index(eff_offset);
                                    scope_entries[entry_idx].route_policy_tag = tag;
                                    scope_entries[entry_idx].has_route_policy = true;
                                }
                            }
                        }
                    }
                    ScopeEvent::Exit => {
                        if scope_stack_len == 0 {
                            panic!("structured scope stack underflow");
                        }
                        scope_stack_len -= 1;
                        let expected = scope_stack[scope_stack_len];
                        if expected.local_ordinal() != scope.local_ordinal() {
                            panic!("structured scope stack mismatch");
                        }
                        let entry_idx = scope_stack_entries[scope_stack_len];
                        let is_linger = scope_entries[entry_idx].linger;
                        let mut offer_entry_locked = false;

                        // Check if the next scope marker is an Enter for the same scope.
                        // If so, this is an intermediate Exit between arms in the same binary route.
                        // We need to insert arm 0's Jump HERE, not at the final Exit.
                        let next_marker_idx = scope_marker_idx + 1;
                        let is_immediate_reenter = next_marker_idx < scope_markers.len()
                            && scope_markers[next_marker_idx].offset
                                == scope_markers[scope_marker_idx].offset
                            && matches!(scope_markers[next_marker_idx].event, ScopeEvent::Enter)
                            && scope_markers[next_marker_idx].scope_id.local_ordinal()
                                == scope.local_ordinal();

                        // For linger (loop) scopes, insert Jump nodes at arm ends.
                        // We need to do this BEFORE setting scope_entries[entry_idx].end
                        // because the Jump nodes become part of the scope.
                        //
                        // With a binary route, we get multiple Exit/Enter pairs for the same scope:
                        // - Intermediate Exit (is_immediate_reenter=true): Insert arm 0's Jump
                        // - Final Exit (is_immediate_reenter=false): Insert arm 1's Jump
                        if is_linger {
                            // Find the linger tracking entry for this scope
                            let mut linger_idx = 0usize;
                            while linger_idx < linger_arm_len {
                                if linger_arm_scope_ids[linger_idx].local_ordinal()
                                    == scope.local_ordinal()
                                {
                                    break;
                                }
                                linger_idx += 1;
                            }

                            if linger_idx < linger_arm_len {
                                let arm_last = linger_arm_last_node[linger_idx];
                                let loop_start = scope_entries[entry_idx].start;
                                // Passive observer detection using type-level controller_role.
                                // controller_role is propagated from the route arm entry via ScopeMarker.
                                let is_passive = match scope_entries[entry_idx].controller_role {
                                    Some(ctrl_role) => ctrl_role != ROLE,
                                    None => false, // No controller_role = not a route scope
                                };
                                // For passive observers, use passive_arm_entry for arm start positions.
                                // passive_arm_entry tracks the first cross-role node (Send or Recv)
                                // of each arm, which is more reliable than route_recv_indices
                                // (which only tracks Recv nodes).
                                let passive_starts = if is_passive {
                                    let arm0_start = if !scope_entries[entry_idx].passive_arm_entry
                                        [0]
                                    .is_max()
                                    {
                                        state_index_to_usize(
                                            scope_entries[entry_idx].passive_arm_entry[0],
                                        )
                                    } else {
                                        PASSIVE_ARM_UNSET
                                    };
                                    let arm1_start = if !scope_entries[entry_idx].passive_arm_entry
                                        [1]
                                    .is_max()
                                    {
                                        state_index_to_usize(
                                            scope_entries[entry_idx].passive_arm_entry[1],
                                        )
                                    } else {
                                        PASSIVE_ARM_UNSET
                                    };
                                    [arm0_start, arm1_start]
                                } else {
                                    [PASSIVE_ARM_UNSET, PASSIVE_ARM_UNSET]
                                };

                                // At intermediate Exit: Insert Jump for arm 0 (Continue)
                                // At final Exit: Insert Jump for arm 1 (Break)
                                if is_immediate_reenter {
                                    // Insert Jump for Continue arm (arm 0).
                                    // For controller: LoopContinue Jump (rewinding flow)
                                    // For passive observer: PassiveObserverBranch Jump (arm entry navigation)
                                    if is_passive && passive_starts[0] != PASSIVE_ARM_UNSET {
                                        // Passive observer: insert PassiveObserverBranch Jump FIRST
                                        // This takes priority because passive observers don't control
                                        // the loop - they need arm entry navigation, not rewind logic.
                                        if node_len >= MAX_STATES {
                                            panic!(
                                                "node capacity exceeded inserting PassiveObserverBranch Jump for arm 0"
                                            );
                                        }
                                        let continue_target = as_state_index(passive_starts[0]);
                                        let jump_node = LocalNode::jump(
                                            continue_target,
                                            JumpReason::PassiveObserverBranch,
                                            scope,
                                            Some(scope),
                                            Some(0),
                                        );
                                        nodes[node_len] = jump_node;
                                        scope_entries[entry_idx].passive_arm_jump[0] =
                                            as_state_index(node_len);
                                        node_len += 1;
                                        // Also insert LoopContinue Jump if there are nodes to connect
                                        if arm_last[0] != LINGER_ARM_NO_NODE {
                                            if node_len >= MAX_STATES {
                                                panic!(
                                                    "node capacity exceeded inserting LoopContinue Jump for passive"
                                                );
                                            }
                                            let jump_node = LocalNode::jump(
                                                loop_start,
                                                JumpReason::LoopContinue,
                                                scope,
                                                Some(scope),
                                                Some(0),
                                            );
                                            let prev_idx = arm_last[0];
                                            nodes[prev_idx] =
                                                nodes[prev_idx].with_next(as_state_index(node_len));
                                            nodes[node_len] = jump_node;
                                            node_len += 1;
                                        }
                                    } else if arm_last[0] != LINGER_ARM_NO_NODE {
                                        // Controller: LoopContinue Jump
                                        if node_len >= MAX_STATES {
                                            panic!(
                                                "node capacity exceeded inserting LoopContinue Jump"
                                            );
                                        }
                                        // Create Jump node for LoopContinue
                                        // Target = loop_start (known at this point)
                                        let jump_node = LocalNode::jump(
                                            loop_start,
                                            JumpReason::LoopContinue,
                                            scope,
                                            Some(scope), // loop_scope is this scope
                                            Some(0),     // arm 0 = Continue
                                        );
                                        // Update the previous node's `next` to point to this Jump
                                        let prev_idx = arm_last[0];
                                        nodes[prev_idx] =
                                            nodes[prev_idx].with_next(as_state_index(node_len));
                                        nodes[node_len] = jump_node;
                                        node_len += 1;
                                    } else if passive_starts[0] != PASSIVE_ARM_UNSET {
                                        if node_len >= MAX_STATES {
                                            panic!(
                                                "node capacity exceeded inserting PassiveObserverBranch Jump for arm 0"
                                            );
                                        }
                                        // Passive observer: insert PassiveObserverBranch Jump for arm 0
                                        // The target should be the start of arm 0's body, which is
                                        // recorded in passive_starts[0]. This is the index where
                                        // the first node of arm 0 was created (e.g., Recv BodyMsg).
                                        //
                                        // Note: We use passive_starts[0] directly instead of
                                        // find_loop_entry_state because:
                                        // 1. Passive observers have nodes inside the scope (arm body)
                                        // 2. passive_starts[0] was set when the arm boundary was
                                        //    detected, which is the position where the body starts
                                        let continue_target = as_state_index(passive_starts[0]);
                                        let jump_node = LocalNode::jump(
                                            continue_target,
                                            JumpReason::PassiveObserverBranch,
                                            scope,
                                            Some(scope),
                                            Some(0),
                                        );
                                        nodes[node_len] = jump_node;
                                        scope_entries[entry_idx].passive_arm_jump[0] =
                                            as_state_index(node_len);
                                        node_len += 1;
                                    }
                                } else {
                                    // Final Exit: Insert Jump for Break arm (arm 1) if it has nodes
                                    if arm_last[1] != LINGER_ARM_NO_NODE {
                                        if node_len >= MAX_STATES {
                                            panic!(
                                                "node capacity exceeded inserting LoopBreak Jump"
                                            );
                                        }
                                        // Create Jump node for LoopBreak
                                        // Target = scope_end (needs backpatch)
                                        let jump_node = LocalNode::jump(
                                            StateIndex::ZERO, // Sentinel, will be backpatched
                                            JumpReason::LoopBreak,
                                            scope,
                                            Some(scope), // loop_scope is this scope
                                            Some(1),     // arm 1 = Break
                                        );
                                        // Update the previous node's `next` to point to this Jump
                                        let prev_idx = arm_last[1];
                                        nodes[prev_idx] =
                                            nodes[prev_idx].with_next(as_state_index(node_len));
                                        nodes[node_len] = jump_node;
                                        // Record for backpatch
                                        if jump_backpatch_len >= MAX_JUMP_BACKPATCH {
                                            panic!(
                                                "jump backpatch capacity exceeded for LoopBreak"
                                            );
                                        }
                                        jump_backpatch_indices[jump_backpatch_len] = node_len;
                                        jump_backpatch_scopes[jump_backpatch_len] = scope;
                                        jump_backpatch_kinds[jump_backpatch_len] = 1; // scope_end
                                        jump_backpatch_len += 1;
                                        node_len += 1;
                                    } else if is_passive && passive_starts[1] != PASSIVE_ARM_UNSET {
                                        if node_len >= MAX_STATES {
                                            panic!(
                                                "node capacity exceeded inserting PassiveObserverBranch Jump for arm 1"
                                            );
                                        }
                                        // Passive observer: insert PassiveObserverBranch Jump for arm 1
                                        // Target = arm 1 body start (passive_starts[1]), similar to arm 0.
                                        // This handles protocols where the break arm has cross-role
                                        // messages for the passive observer (e.g., ExitMsg send).
                                        //
                                        // If passive_starts[1] == node_len, the break arm is EMPTY
                                        // (no cross-role content). In that case, the Jump should point
                                        // directly to scope_end (terminal), not to itself. We use
                                        // backpatch to set the target to scope_end.

                                        // Determine if the break arm has content for passive observer
                                        let arm_is_empty = passive_starts[1] == node_len;

                                        // IMPORTANT: Before inserting the PassiveObserverBranch, record the
                                        // arm's last node for backpatch. This node's `next` currently points
                                        // to where we're about to insert the PassiveObserverBranch. We need
                                        // to patch it to point to scope_end instead, so that after completing
                                        // the break arm, the cursor moves to scope_end (terminal) rather than
                                        // looping back through the PassiveObserverBranch.
                                        //
                                        // The arm's last action is at (node_len - 1) because node_len is
                                        // where we're about to insert the PassiveObserverBranch.
                                        if node_len > 0 && passive_starts[1] < node_len {
                                            let arm_last_node = node_len - 1;
                                            // Only patch if this is an actual action node (not a Jump)
                                            if !nodes[arm_last_node].action().is_jump() {
                                                if jump_backpatch_len >= MAX_JUMP_BACKPATCH {
                                                    panic!(
                                                        "jump backpatch capacity exceeded for arm last node"
                                                    );
                                                }
                                                jump_backpatch_indices[jump_backpatch_len] =
                                                    arm_last_node;
                                                jump_backpatch_scopes[jump_backpatch_len] = scope;
                                                jump_backpatch_kinds[jump_backpatch_len] = 1; // scope_end
                                                jump_backpatch_len += 1;
                                            }
                                        }

                                        // Target: if arm is empty, use sentinel for backpatch to scope_end
                                        // Otherwise, use the arm body start
                                        let break_target = if arm_is_empty {
                                            StateIndex::ZERO // Sentinel, will be backpatched to scope_end
                                        } else {
                                            as_state_index(passive_starts[1])
                                        };
                                        let jump_node = LocalNode::jump(
                                            break_target,
                                            JumpReason::PassiveObserverBranch,
                                            scope,
                                            Some(scope),
                                            Some(1),
                                        );
                                        nodes[node_len] = jump_node;
                                        scope_entries[entry_idx].passive_arm_jump[1] =
                                            as_state_index(node_len);

                                        // If arm is empty, backpatch the Jump target to scope_end
                                        if arm_is_empty {
                                            if jump_backpatch_len >= MAX_JUMP_BACKPATCH {
                                                panic!(
                                                    "jump backpatch capacity exceeded for empty arm"
                                                );
                                            }
                                            jump_backpatch_indices[jump_backpatch_len] = node_len;
                                            jump_backpatch_scopes[jump_backpatch_len] = scope;
                                            jump_backpatch_kinds[jump_backpatch_len] = 1; // scope_end
                                            jump_backpatch_len += 1;
                                        }

                                        node_len += 1;
                                    }
                                }
                            }
                        }
                        // Non-linger Route Jump generation using is_immediate_reenter.
                        // Arm boundaries are visible via Exit→Enter pairs in ScopeEvent (generated by
                        // binary route wrapping each arm with with_scope()).
                        //
                        // CFG-pure design: arm 0 ends with RouteArmEnd Jump → scope_end, NOT fall-through to arm 1.
                        // This eliminates sequential layout dependency and runtime arm repositioning.
                        //
                        // At intermediate Exit (is_immediate_reenter=true):
                        //   - Controller: RouteArmEnd Jump → scope_end
                        //   - Passive observer: PassiveObserverBranch Jump → arm entry
                        // At final Exit (is_immediate_reenter=false):
                        //   - Passive observer: PassiveObserverBranch Jump → arm entry
                        //
                        // Passive observer detection using type-level controller_role.
                        // controller_role is propagated from the route arm entry via ScopeMarker.
                        // If controller_role matches this role, we're the controller.
                        let _is_passive_observer = match scope_entries[entry_idx].controller_role {
                            Some(ctrl_role) => ctrl_role != ROLE,
                            None => false, // No controller_role = not a route scope
                        };

                        // Generate RouteArmEnd Jump at arm 0's end (intermediate Exit).
                        // This explicitly exits arm 0 to scope_end, purifying the CFG.
                        // Both controller and passive observer roles get RouteArmEnd to ensure
                        // arm completion leads directly to scope_end without passing through
                        // PassiveObserverBranch nodes (which are decision points, not terminators).
                        if !is_linger
                            && matches!(scope_entries[entry_idx].kind, ScopeKind::Route)
                            && is_immediate_reenter
                        {
                            // For τ-eliminated arm 0 (passive observer has no nodes in arm 0),
                            // this RouteArmEnd also serves as the arm entry placeholder.
                            let arm0_is_tau_eliminated =
                                scope_entries[entry_idx].passive_arm_entry[0].is_max();

                            if node_len >= MAX_STATES {
                                panic!(
                                    "node capacity exceeded inserting RouteArmEnd Jump for arm 0"
                                );
                            }
                            // Target is scope_end, which will be backpatched after scope closes.
                            let jump_node = LocalNode::jump(
                                StateIndex::ZERO, // Sentinel, will be backpatched to scope_end
                                JumpReason::RouteArmEnd,
                                scope,
                                None, // Not a loop
                                Some(0),
                            );
                            nodes[node_len] = jump_node;

                            // For τ-eliminated arm 0, set passive_arm_entry to this RouteArmEnd.
                            // This ensures follow_passive_observer_arm_for_scope always returns
                            // a valid entry (ArmEmpty placeholder).
                            if arm0_is_tau_eliminated {
                                scope_entries[entry_idx].passive_arm_entry[0] =
                                    as_state_index(node_len);
                            }

                            // Record for backpatch to scope_end
                            if jump_backpatch_len >= MAX_JUMP_BACKPATCH {
                                panic!("jump backpatch capacity exceeded for RouteArmEnd Jump");
                            }
                            jump_backpatch_indices[jump_backpatch_len] = node_len;
                            jump_backpatch_scopes[jump_backpatch_len] = scope;
                            jump_backpatch_kinds[jump_backpatch_len] = 2; // scope_end via RouteArmEnd
                            jump_backpatch_len += 1;

                            node_len += 1;
                        }

                        // Generate RouteArmEnd Jump at arm 1's end (final Exit).
                        // This removes reliance on sequential layout for the last arm and
                        // ensures both arms explicitly exit to scope_end.
                        if !is_linger
                            && matches!(scope_entries[entry_idx].kind, ScopeKind::Route)
                            && !is_immediate_reenter
                        {
                            let arm1_last = route_arm_last_node[scope_stack_len][1];
                            let last_was_scope = last_step_was_scope[scope_stack_len];
                            if !arm1_last.is_max() {
                                if last_was_scope {
                                    // Arm ended with a nested scope; insert RouteArmEnd at scope exit.
                                    if node_len >= MAX_STATES {
                                        panic!(
                                            "node capacity exceeded inserting RouteArmEnd Jump for arm 1 (scope exit)"
                                        );
                                    }
                                    let jump_node = LocalNode::jump(
                                        StateIndex::ZERO, // Sentinel, will be backpatched to scope_end
                                        JumpReason::RouteArmEnd,
                                        scope,
                                        None, // Not a loop
                                        Some(1),
                                    );
                                    nodes[node_len] = jump_node;
                                    if jump_backpatch_len >= MAX_JUMP_BACKPATCH {
                                        panic!(
                                            "jump backpatch capacity exceeded for RouteArmEnd Jump (arm 1 scope exit)"
                                        );
                                    }
                                    jump_backpatch_indices[jump_backpatch_len] = node_len;
                                    jump_backpatch_scopes[jump_backpatch_len] = scope;
                                    jump_backpatch_kinds[jump_backpatch_len] = 2; // scope_end via RouteArmEnd
                                    jump_backpatch_len += 1;
                                    node_len += 1;
                                } else {
                                    if node_len >= MAX_STATES {
                                        panic!(
                                            "node capacity exceeded inserting RouteArmEnd Jump for arm 1"
                                        );
                                    }
                                    let jump_node = LocalNode::jump(
                                        StateIndex::ZERO, // Sentinel, will be backpatched to scope_end
                                        JumpReason::RouteArmEnd,
                                        scope,
                                        None, // Not a loop
                                        Some(1),
                                    );
                                    // Patch last node in arm 1 to jump to RouteArmEnd
                                    let prev_idx = state_index_to_usize(arm1_last);
                                    nodes[prev_idx] =
                                        nodes[prev_idx].with_next(as_state_index(node_len));
                                    nodes[node_len] = jump_node;
                                    if jump_backpatch_len >= MAX_JUMP_BACKPATCH {
                                        panic!(
                                            "jump backpatch capacity exceeded for RouteArmEnd Jump (arm 1)"
                                        );
                                    }
                                    jump_backpatch_indices[jump_backpatch_len] = node_len;
                                    jump_backpatch_scopes[jump_backpatch_len] = scope;
                                    jump_backpatch_kinds[jump_backpatch_len] = 2; // scope_end via RouteArmEnd
                                    jump_backpatch_len += 1;
                                    node_len += 1;
                                }
                            }
                        }

                        // Generate ArmEmpty placeholder for τ-eliminated arm 1 (final Exit).
                        // This ensures passive observers always have a valid arm entry,
                        // eliminating the need for runtime ScopeExited recovery.
                        //
                        // CFG-pure design: All τ-eliminated arms have ArmEmpty placeholder.
                        // For both linger (loop) and non-linger routes, passive_arm_entry must be set.
                        //
                        // Note: For non-linger routes, ArmEmpty is a RouteArmEnd Jump → scope_end.
                        // For linger routes, ArmEmpty is a LoopBreak Jump (handled differently).
                        if matches!(scope_entries[entry_idx].kind, ScopeKind::Route)
                            && !is_immediate_reenter
                        {
                            let arm1_has_content =
                                !scope_entries[entry_idx].passive_arm_entry[1].is_max();
                            if !arm1_has_content {
                                // τ-eliminated arm 1: insert ArmEmpty placeholder
                                if node_len >= MAX_STATES {
                                    panic!(
                                        "node capacity exceeded inserting ArmEmpty placeholder for arm 1"
                                    );
                                }

                                let jump_node = if is_linger {
                                    // Linger scope: ArmEmpty is a LoopBreak Jump → scope start (for loop back)
                                    // Actually for break arm, target is scope_end (exit loop).
                                    LocalNode::jump(
                                        as_state_index(node_len + 1), // scope_end
                                        JumpReason::LoopBreak,
                                        scope,
                                        Some(scope), // loop scope
                                        Some(1),
                                    )
                                } else {
                                    // Non-linger: ArmEmpty is a RouteArmEnd Jump → scope_end
                                    LocalNode::jump(
                                        as_state_index(node_len + 1), // scope_end
                                        JumpReason::RouteArmEnd,
                                        scope,
                                        None,
                                        Some(1),
                                    )
                                };
                                nodes[node_len] = jump_node;
                                // Update passive_arm_entry to point to this placeholder
                                scope_entries[entry_idx].passive_arm_entry[1] =
                                    as_state_index(node_len);
                                node_len += 1;
                            }
                        }

                        // Scope-as-Block: Mark parent scope as "last step was a scope exit".
                        // This enables correct Jump insertion when the parent scope's arm boundary
                        // is detected - if this flag is true, we insert a Jump node at the current
                        // position (Inner.end) instead of patching the previous node's next field.
                        if scope_stack_len > 0 {
                            last_step_was_scope[scope_stack_len - 1] = true;
                        }

                        // FIRST-recv dispatch computation for Route scopes (final Exit only).
                        // Computes label → (arm, target_idx) mapping for passive observers.
                        // This enables O(1) nested route resolution in offer().
                        if matches!(scope_entries[entry_idx].kind, ScopeKind::Route)
                            && !is_immediate_reenter
                        {
                            let is_controller = match scope_entries[entry_idx].controller_role {
                                Some(role) => role == ROLE,
                                None => false,
                            };
                            let scope_end = as_state_index(node_len);
                            if !is_linger {
                                let arm0_entry = if is_controller {
                                    scope_entries[entry_idx].controller_arm_entry[0]
                                } else {
                                    scope_entries[entry_idx].passive_arm_entry[0]
                                };
                                let arm1_entry = if is_controller {
                                    scope_entries[entry_idx].controller_arm_entry[1]
                                } else {
                                    scope_entries[entry_idx].passive_arm_entry[1]
                                };
                                if !arm0_entry.is_max() && !arm1_entry.is_max() {
                                    let (prefix_end0, prefix_end1, prefix_len) =
                                        arm_common_prefix_end(
                                            &nodes,
                                            scope_entries[entry_idx].scope_id,
                                            scope_end,
                                            arm0_entry,
                                            arm1_entry,
                                        );
                                    if prefix_len > 0 {
                                        let parent_scope = scope_entries[entry_idx].parent;
                                        let mut arm = 0u8;
                                        while arm < 2 {
                                            let mut steps = 0usize;
                                            let mut idx =
                                                if arm == 0 { arm0_entry } else { arm1_entry };
                                            while steps < prefix_len {
                                                if idx.is_max() {
                                                    break;
                                                }
                                                let node_idx = state_index_to_usize(idx);
                                                if node_idx >= node_len {
                                                    break;
                                                }
                                                let node = nodes[node_idx];
                                                nodes[node_idx] = node
                                                    .with_scope(parent_scope)
                                                    .with_route_arm(None);
                                                let next = node.next();
                                                if next.is_max() {
                                                    break;
                                                }
                                                idx = next;
                                                steps += 1;
                                            }
                                            arm += 1;
                                        }

                                        let min_start = if prefix_end0.raw() < prefix_end1.raw() {
                                            prefix_end0
                                        } else {
                                            prefix_end1
                                        };
                                        if !min_start.is_max() {
                                            scope_entries[entry_idx].start = min_start;
                                        }
                                        if is_controller {
                                            scope_entries[entry_idx].controller_arm_entry[0] =
                                                prefix_end0;
                                            scope_entries[entry_idx].controller_arm_entry[1] =
                                                prefix_end1;

                                            let mut arm = 0u8;
                                            while arm < 2 {
                                                let entry = scope_entries[entry_idx]
                                                    .controller_arm_entry
                                                    [arm as usize];
                                                if !entry.is_max() {
                                                    let node_idx = state_index_to_usize(entry);
                                                    if node_idx < node_len {
                                                        match nodes[node_idx].action() {
                                                            LocalAction::Local {
                                                                label, ..
                                                            } => {
                                                                scope_entries[entry_idx]
                                                                    .controller_arm_label
                                                                    [arm as usize] = label;
                                                            }
                                                            _ => {
                                                                scope_entries[entry_idx]
                                                                    .controller_arm_entry
                                                                    [arm as usize] =
                                                                    StateIndex::MAX;
                                                                scope_entries[entry_idx]
                                                                    .controller_arm_label
                                                                    [arm as usize] = 0;
                                                            }
                                                        }
                                                    } else {
                                                        scope_entries[entry_idx]
                                                            .controller_arm_entry
                                                            [arm as usize] = StateIndex::MAX;
                                                        scope_entries[entry_idx]
                                                            .controller_arm_label
                                                            [arm as usize] = 0;
                                                    }
                                                } else {
                                                    scope_entries[entry_idx].controller_arm_label
                                                        [arm as usize] = 0;
                                                }
                                                arm += 1;
                                            }

                                            scope_entries[entry_idx].route_recv_head =
                                                RouteRecvIndex::MAX;
                                            scope_entries[entry_idx].route_recv_tail =
                                                RouteRecvIndex::MAX;
                                            scope_entries[entry_idx].route_recv_len = 0;
                                            scope_entries[entry_idx].offer_lanes = 0;
                                            if prefix_end0.raw() != prefix_end1.raw() {
                                                let mut arm = 0u8;
                                                while arm < 2 {
                                                    let arm_entry = if arm == 0 {
                                                        prefix_end0
                                                    } else {
                                                        prefix_end1
                                                    };
                                                    if (arm as u16)
                                                        == scope_entries[entry_idx].route_recv_len
                                                        && !arm_entry.is_max()
                                                    {
                                                        let node_idx =
                                                            state_index_to_usize(arm_entry);
                                                        if node_idx < node_len {
                                                            if let LocalAction::Recv {
                                                                lane, ..
                                                            } = nodes[node_idx].action()
                                                            {
                                                                if route_recv_nodes_len
                                                                    >= MAX_STATES
                                                                {
                                                                    panic!(
                                                                        "route recv node capacity exceeded"
                                                                    );
                                                                }
                                                                route_recv_nodes
                                                                    [route_recv_nodes_len] =
                                                                    RouteRecvNode {
                                                                        state: arm_entry,
                                                                        next: RouteRecvIndex::MAX,
                                                                    };
                                                                if scope_entries[entry_idx]
                                                                    .route_recv_head
                                                                    .is_max()
                                                                {
                                                                    scope_entries[entry_idx]
                                                                        .route_recv_head =
                                                                        RouteRecvIndex::from_usize(
                                                                            route_recv_nodes_len,
                                                                        );
                                                                } else {
                                                                    let tail_idx = scope_entries
                                                                        [entry_idx]
                                                                        .route_recv_tail
                                                                        .as_usize();
                                                                    route_recv_nodes[tail_idx]
                                                                        .next =
                                                                        RouteRecvIndex::from_usize(
                                                                            route_recv_nodes_len,
                                                                        );
                                                                }
                                                                scope_entries[entry_idx]
                                                                    .route_recv_tail =
                                                                    RouteRecvIndex::from_usize(
                                                                        route_recv_nodes_len,
                                                                    );
                                                                scope_entries[entry_idx]
                                                                    .route_recv_len += 1;
                                                                route_recv_nodes_len += 1;
                                                                scope_entries[entry_idx]
                                                                    .offer_lanes |=
                                                                    offer_lane_bit(lane);
                                                            }
                                                        }
                                                    }
                                                    arm += 1;
                                                }
                                            }
                                        } else {
                                            scope_entries[entry_idx].passive_arm_entry[0] =
                                                prefix_end0;
                                            scope_entries[entry_idx].passive_arm_entry[1] =
                                                prefix_end1;
                                        }
                                        scope_entries[entry_idx].offer_entry =
                                            if prefix_end0.raw() == prefix_end1.raw() {
                                                prefix_end0
                                            } else {
                                                StateIndex::MAX
                                            };
                                        offer_entry_locked = true;
                                    }
                                }
                            }
                            let mut arm = 0usize;
                            while arm < 2 {
                                if scope_entries[entry_idx].passive_arm_scope[arm].is_none() {
                                    let arm_entry = scope_entries[entry_idx].passive_arm_entry[arm];
                                    if !arm_entry.is_max() {
                                        let arm_entry_idx = state_index_to_usize(arm_entry);
                                        if arm_entry_idx < node_len {
                                            let arm_scope = nodes[arm_entry_idx].scope();
                                            if !arm_scope.is_none()
                                                && arm_scope.raw()
                                                    != scope_entries[entry_idx].scope_id.raw()
                                                && matches!(arm_scope.kind(), ScopeKind::Route)
                                            {
                                                scope_entries[entry_idx].passive_arm_scope[arm] =
                                                    arm_scope;
                                            }
                                        }
                                    }
                                }
                                arm += 1;
                            }
                            if is_controller {
                                scope_entries[entry_idx].first_recv_dispatch =
                                    [(0, 0, StateIndex::MAX); MAX_FIRST_RECV_DISPATCH];
                                scope_entries[entry_idx].first_recv_len = 0;
                                scope_entries[entry_idx].mergeable = false;
                            } else {
                                let mut dispatch_len = 0u8;
                                let mut dispatch_table: [(u8, u8, StateIndex);
                                    MAX_FIRST_RECV_DISPATCH] =
                                    [(0, 0, StateIndex::MAX); MAX_FIRST_RECV_DISPATCH];
                                let mut dispatch_functional = true;
                                let mut prefix_actions =
                                    [[PrefixAction::EMPTY; MAX_PREFIX_ACTIONS]; 2];
                                let mut prefix_lens = [0usize; 2];
                                let mut arm_seen_recv = [false; 2];

                                // Process both arms
                                let mut arm = 0u8;
                                while arm < 2 {
                                    let arm_idx = arm as usize;
                                    let arm_entry =
                                        scope_entries[entry_idx].passive_arm_entry[arm as usize];
                                    if !arm_entry.is_max() {
                                        // Collect FIRST labels for this arm, flattening nested routes.
                                        // Use a stack-based approach to avoid recursion in const fn.
                                        let mut scan_stack: [StateIndex; eff::meta::MAX_EFF_NODES] =
                                            [StateIndex::MAX; eff::meta::MAX_EFF_NODES];
                                        let mut visited = [false; MAX_STATES];
                                        let mut scan_len = 1usize;
                                        scan_stack[0] = arm_entry;

                                        while scan_len > 0 {
                                            scan_len -= 1;
                                            let scan_idx =
                                                state_index_to_usize(scan_stack[scan_len]);
                                            if scan_idx >= node_len {
                                                // Out of bounds, skip
                                                arm += 1;
                                                continue;
                                            }
                                            if visited[scan_idx] {
                                                continue;
                                            }
                                            visited[scan_idx] = true;
                                            let node = nodes[scan_idx];
                                            let scan_scope = node.scope();
                                            let scan_outer_scope =
                                                scope_entries[entry_idx].scope_id;
                                            if matches!(scan_scope.kind(), ScopeKind::Route)
                                                && !scan_scope.is_none()
                                                && scan_scope.local_ordinal()
                                                    != scan_outer_scope.local_ordinal()
                                            {
                                                let nested_ordinal = scan_scope.local_ordinal();
                                                let mut nested_entry_idx = 0usize;
                                                while nested_entry_idx < scope_entries_len {
                                                    if scope_entries[nested_entry_idx]
                                                        .scope_id
                                                        .local_ordinal()
                                                        == nested_ordinal
                                                    {
                                                        let nested =
                                                            &scope_entries[nested_entry_idx];
                                                        let mut ni = 0usize;
                                                        while ni < nested.first_recv_len as usize {
                                                            let (nlabel, _narm, ntarget) =
                                                                nested.first_recv_dispatch[ni];
                                                            let mut nconflict = false;
                                                            let mut nfound = false;
                                                            let mut ei = 0usize;
                                                            while ei < dispatch_len as usize {
                                                                let (
                                                                    existing_label,
                                                                    existing_arm,
                                                                    existing_target,
                                                                ) = dispatch_table[ei];
                                                                if existing_label == nlabel {
                                                                    nfound = true;
                                                                    let same_continuation =
                                                                    existing_target.raw()
                                                                        == ntarget.raw()
                                                                        || continuations_equivalent(
                                                                            &nodes,
                                                                            scope_end,
                                                                            existing_target,
                                                                            ntarget,
                                                                        );
                                                                    if same_continuation {
                                                                        if existing_arm != arm
                                                                            && existing_arm
                                                                                != ARM_SHARED
                                                                        {
                                                                            dispatch_table[ei] = (
                                                                                nlabel,
                                                                                ARM_SHARED,
                                                                                existing_target,
                                                                            );
                                                                        }
                                                                    } else {
                                                                        nconflict = true;
                                                                    }
                                                                    break;
                                                                }
                                                                ei += 1;
                                                            }
                                                            if nconflict {
                                                                dispatch_functional = false;
                                                            } else if !nfound {
                                                                if dispatch_len
                                                                    >= MAX_FIRST_RECV_DISPATCH as u8
                                                                {
                                                                    panic!(
                                                                        "FIRST-recv dispatch table overflow from nested"
                                                                    );
                                                                }
                                                                dispatch_table
                                                                    [dispatch_len as usize] =
                                                                    (nlabel, arm, ntarget);
                                                                dispatch_len += 1;
                                                            }
                                                            ni += 1;
                                                        }
                                                        break;
                                                    }
                                                    nested_entry_idx += 1;
                                                }
                                                continue;
                                            }
                                            match node.action() {
                                                LocalAction::Recv { label, .. } => {
                                                    // Found a recv - add to dispatch table
                                                    let target_idx = as_state_index(scan_idx);
                                                    arm_seen_recv[arm_idx] = true;

                                                    // Check for conflict with existing entries
                                                    let mut conflict = false;
                                                    let mut found = false;
                                                    let mut check_i = 0usize;
                                                    while check_i < dispatch_len as usize {
                                                        let (
                                                            existing_label,
                                                            existing_arm,
                                                            existing_target,
                                                        ) = dispatch_table[check_i];
                                                        if existing_label == label {
                                                            found = true;
                                                            let same_continuation = existing_target
                                                                .raw()
                                                                == target_idx.raw()
                                                                || continuations_equivalent(
                                                                    &nodes,
                                                                    scope_end,
                                                                    existing_target,
                                                                    target_idx,
                                                                );
                                                            if same_continuation {
                                                                // Same label maps to the same continuation
                                                                if existing_arm != arm
                                                                    && existing_arm != ARM_SHARED
                                                                {
                                                                    dispatch_table[check_i] = (
                                                                        label,
                                                                        ARM_SHARED,
                                                                        existing_target,
                                                                    );
                                                                }
                                                            } else {
                                                                // Same label maps to different continuation → non-functional
                                                                conflict = true;
                                                            }
                                                            break;
                                                        }
                                                        check_i += 1;
                                                    }

                                                    if conflict {
                                                        dispatch_functional = false;
                                                    } else if !found {
                                                        if dispatch_len
                                                            >= MAX_FIRST_RECV_DISPATCH as u8
                                                        {
                                                            panic!(
                                                                "FIRST-recv dispatch table overflow"
                                                            );
                                                        }
                                                        dispatch_table[dispatch_len as usize] =
                                                            (label, arm, target_idx);
                                                        dispatch_len += 1;
                                                    }

                                                    // Check if this recv is inside a nested Route scope.
                                                    // If so, merge that nested route's FIRST entries as well.
                                                    let recv_scope = node.scope();
                                                    let outer_scope =
                                                        scope_entries[entry_idx].scope_id;
                                                    if matches!(recv_scope.kind(), ScopeKind::Route)
                                                        && !recv_scope.is_none()
                                                        && recv_scope.local_ordinal()
                                                            != outer_scope.local_ordinal()
                                                    {
                                                        // This recv is inside a nested route - merge its FIRST
                                                        let nested_ordinal =
                                                            recv_scope.local_ordinal();
                                                        let mut nested_entry_idx = 0usize;
                                                        while nested_entry_idx < scope_entries_len {
                                                            if scope_entries[nested_entry_idx]
                                                                .scope_id
                                                                .local_ordinal()
                                                                == nested_ordinal
                                                            {
                                                                let nested = &scope_entries
                                                                    [nested_entry_idx];
                                                                let mut ni = 0usize;
                                                                while ni
                                                                    < nested.first_recv_len as usize
                                                                {
                                                                    let (nlabel, _narm, ntarget) =
                                                                        nested.first_recv_dispatch
                                                                            [ni];
                                                                    // Check for conflict/duplicate with existing entries
                                                                    let mut nconflict = false;
                                                                    let mut nfound = false;
                                                                    let mut ei = 0usize;
                                                                    while ei < dispatch_len as usize
                                                                    {
                                                                        let (
                                                                            existing_label,
                                                                            existing_arm,
                                                                            existing_target,
                                                                        ) = dispatch_table[ei];
                                                                        if existing_label == nlabel
                                                                        {
                                                                            nfound = true;
                                                                            let same_continuation =
                                                                                existing_target
                                                                                    .raw()
                                                                                    == ntarget.raw()
                                                                            || continuations_equivalent(
                                                                                &nodes,
                                                                                scope_end,
                                                                                existing_target,
                                                                                ntarget,
                                                                            );
                                                                            if same_continuation {
                                                                                // Same label maps to same continuation
                                                                                if existing_arm != arm && existing_arm != ARM_SHARED {
                                                                                dispatch_table[ei] =
                                                                                    (nlabel, ARM_SHARED, existing_target);
                                                                                }
                                                                            } else {
                                                                                nconflict = true;
                                                                            }
                                                                            break;
                                                                        }
                                                                        ei += 1;
                                                                    }
                                                                    if nconflict {
                                                                        dispatch_functional = false;
                                                                    } else if !nfound {
                                                                        if dispatch_len
                                                                            >= MAX_FIRST_RECV_DISPATCH as u8
                                                                        {
                                                                            panic!(
                                                                                "FIRST-recv dispatch table overflow from nested recv scope"
                                                                            );
                                                                        }
                                                                        // Nested entries inherit the outer arm value
                                                                        dispatch_table[dispatch_len
                                                                            as usize] =
                                                                            (nlabel, arm, ntarget);
                                                                        dispatch_len += 1;
                                                                    }
                                                                    ni += 1;
                                                                }
                                                                break;
                                                            }
                                                            nested_entry_idx += 1;
                                                        }
                                                    }
                                                }
                                                LocalAction::Send {
                                                    peer, label, lane, ..
                                                } => {
                                                    if !arm_seen_recv[arm_idx] {
                                                        if prefix_lens[arm_idx]
                                                            >= MAX_PREFIX_ACTIONS
                                                        {
                                                            panic!("route prefix action overflow");
                                                        }
                                                        let prefix_idx = prefix_lens[arm_idx];
                                                        prefix_actions[arm_idx][prefix_idx] =
                                                            PrefixAction {
                                                                kind: PREFIX_KIND_SEND,
                                                                peer,
                                                                label,
                                                                lane,
                                                            };
                                                        prefix_lens[arm_idx] += 1;
                                                    }
                                                    // Continue scan forward (decision frontier).
                                                    let next_state = node.next();
                                                    let next_idx = state_index_to_usize(next_state);
                                                    let mut nested_merged = false;
                                                    if next_idx < node_len && next_idx != scan_idx {
                                                        let next_node = nodes[next_idx];
                                                        let next_scope = next_node.scope();
                                                        let current_scope = node.scope();

                                                        if matches!(
                                                            next_scope.kind(),
                                                            ScopeKind::Route
                                                        ) && !next_scope.is_none()
                                                            && next_scope.local_ordinal()
                                                                != current_scope.local_ordinal()
                                                        {
                                                            let nested_ordinal =
                                                                next_scope.local_ordinal();
                                                            let mut nested_entry_idx = 0usize;
                                                            while nested_entry_idx
                                                                < scope_entries_len
                                                            {
                                                                if scope_entries[nested_entry_idx]
                                                                    .scope_id
                                                                    .local_ordinal()
                                                                    == nested_ordinal
                                                                {
                                                                    let nested = &scope_entries
                                                                        [nested_entry_idx];
                                                                    let mut ni = 0usize;
                                                                    while ni
                                                                        < nested.first_recv_len
                                                                            as usize
                                                                    {
                                                                        let (
                                                                            nlabel,
                                                                            _narm,
                                                                            ntarget,
                                                                        ) = nested
                                                                            .first_recv_dispatch
                                                                            [ni];
                                                                        let mut nconflict = false;
                                                                        let mut nfound = false;
                                                                        let mut ci = 0usize;
                                                                        while ci
                                                                            < dispatch_len as usize
                                                                        {
                                                                            let (
                                                                                existing_label,
                                                                                existing_arm,
                                                                                existing_target,
                                                                            ) = dispatch_table[ci];
                                                                            if existing_label
                                                                                == nlabel
                                                                            {
                                                                                nfound = true;
                                                                                let same_continuation =
                                                                                existing_target.raw()
                                                                                    == ntarget.raw()
                                                                                    || continuations_equivalent(
                                                                                        &nodes,
                                                                                        scope_end,
                                                                                        existing_target,
                                                                                        ntarget,
                                                                                    );
                                                                                if same_continuation
                                                                                {
                                                                                    if existing_arm != arm
                                                                                    && existing_arm != ARM_SHARED
                                                                                {
                                                                                    dispatch_table[ci] =
                                                                                        (nlabel, ARM_SHARED, existing_target);
                                                                                }
                                                                                } else {
                                                                                    nconflict =
                                                                                        true;
                                                                                }
                                                                                break;
                                                                            }
                                                                            ci += 1;
                                                                        }
                                                                        if nconflict {
                                                                            dispatch_functional =
                                                                                false;
                                                                        } else if !nfound {
                                                                            if dispatch_len
                                                                                >= MAX_FIRST_RECV_DISPATCH as u8
                                                                            {
                                                                                panic!(
                                                                                    "FIRST-recv dispatch table overflow from nested"
                                                                                );
                                                                            }
                                                                            dispatch_table
                                                                                [dispatch_len
                                                                                    as usize] = (
                                                                                nlabel, arm,
                                                                                ntarget,
                                                                            );
                                                                            dispatch_len += 1;
                                                                        }
                                                                        ni += 1;
                                                                    }
                                                                    nested_merged = true;
                                                                    break;
                                                                }
                                                                nested_entry_idx += 1;
                                                            }
                                                        }
                                                    }
                                                    if !nested_merged
                                                        && !next_state.is_max()
                                                        && scan_len < scan_stack.len()
                                                    {
                                                        scan_stack[scan_len] = next_state;
                                                        scan_len += 1;
                                                    }
                                                }
                                                LocalAction::Local { label, lane, .. } => {
                                                    if !arm_seen_recv[arm_idx] {
                                                        if prefix_lens[arm_idx]
                                                            >= MAX_PREFIX_ACTIONS
                                                        {
                                                            panic!("route prefix action overflow");
                                                        }
                                                        let prefix_idx = prefix_lens[arm_idx];
                                                        prefix_actions[arm_idx][prefix_idx] =
                                                            PrefixAction {
                                                                kind: PREFIX_KIND_LOCAL,
                                                                peer: ROLE,
                                                                label,
                                                                lane,
                                                            };
                                                        prefix_lens[arm_idx] += 1;
                                                    }
                                                    let next_state = node.next();
                                                    let next_idx = state_index_to_usize(next_state);
                                                    let mut nested_merged = false;
                                                    if next_idx < node_len && next_idx != scan_idx {
                                                        let next_node = nodes[next_idx];
                                                        let next_scope = next_node.scope();
                                                        let current_scope = node.scope();

                                                        if matches!(
                                                            next_scope.kind(),
                                                            ScopeKind::Route
                                                        ) && !next_scope.is_none()
                                                            && next_scope.local_ordinal()
                                                                != current_scope.local_ordinal()
                                                        {
                                                            let nested_ordinal =
                                                                next_scope.local_ordinal();
                                                            let mut nested_entry_idx = 0usize;
                                                            while nested_entry_idx
                                                                < scope_entries_len
                                                            {
                                                                if scope_entries[nested_entry_idx]
                                                                    .scope_id
                                                                    .local_ordinal()
                                                                    == nested_ordinal
                                                                {
                                                                    let nested = &scope_entries
                                                                        [nested_entry_idx];
                                                                    let mut ni = 0usize;
                                                                    while ni
                                                                        < nested.first_recv_len
                                                                            as usize
                                                                    {
                                                                        let (
                                                                            nlabel,
                                                                            _narm,
                                                                            ntarget,
                                                                        ) = nested
                                                                            .first_recv_dispatch
                                                                            [ni];
                                                                        let mut nconflict = false;
                                                                        let mut nfound = false;
                                                                        let mut ci = 0usize;
                                                                        while ci
                                                                            < dispatch_len as usize
                                                                        {
                                                                            let (
                                                                                existing_label,
                                                                                existing_arm,
                                                                                existing_target,
                                                                            ) = dispatch_table[ci];
                                                                            if existing_label
                                                                                == nlabel
                                                                            {
                                                                                nfound = true;
                                                                                let same_continuation =
                                                                                existing_target.raw()
                                                                                    == ntarget.raw()
                                                                                    || continuations_equivalent(
                                                                                        &nodes,
                                                                                        scope_end,
                                                                                        existing_target,
                                                                                        ntarget,
                                                                                    );
                                                                                if same_continuation
                                                                                {
                                                                                    if existing_arm != arm
                                                                                    && existing_arm != ARM_SHARED
                                                                                {
                                                                                    dispatch_table[ci] =
                                                                                        (nlabel, ARM_SHARED, existing_target);
                                                                                }
                                                                                } else {
                                                                                    nconflict =
                                                                                        true;
                                                                                }
                                                                                break;
                                                                            }
                                                                            ci += 1;
                                                                        }
                                                                        if nconflict {
                                                                            dispatch_functional =
                                                                                false;
                                                                        } else if !nfound {
                                                                            if dispatch_len
                                                                                >= MAX_FIRST_RECV_DISPATCH as u8
                                                                            {
                                                                                panic!(
                                                                                    "FIRST-recv dispatch table overflow from nested"
                                                                                );
                                                                            }
                                                                            dispatch_table
                                                                                [dispatch_len
                                                                                    as usize] = (
                                                                                nlabel, arm,
                                                                                ntarget,
                                                                            );
                                                                            dispatch_len += 1;
                                                                        }
                                                                        ni += 1;
                                                                    }
                                                                    nested_merged = true;
                                                                    break;
                                                                }
                                                                nested_entry_idx += 1;
                                                            }
                                                        }
                                                    }
                                                    if !nested_merged
                                                        && !next_state.is_max()
                                                        && scan_len < scan_stack.len()
                                                    {
                                                        scan_stack[scan_len] = next_state;
                                                        scan_len += 1;
                                                    }
                                                }
                                                LocalAction::Jump {
                                                    reason: JumpReason::PassiveObserverBranch,
                                                } => {
                                                    // This is a passive observer branch - follow to target
                                                    let target = node.next();
                                                    if !target.is_max()
                                                        && scan_len < scan_stack.len()
                                                    {
                                                        scan_stack[scan_len] = target;
                                                        scan_len += 1;
                                                    }
                                                }
                                                LocalAction::Jump {
                                                    reason:
                                                        JumpReason::RouteArmEnd
                                                        | JumpReason::LoopContinue
                                                        | JumpReason::LoopBreak,
                                                } => {
                                                    // Arm boundary or loop boundary - no recv labels to add.
                                                }
                                                _ => {
                                                    // Check if next node enters a nested Route scope.
                                                    // If next node has a different (inner) Route scope, merge its FIRST
                                                    // and stop scanning this path (decision frontier).
                                                    let next_state = node.next();
                                                    let next_idx = state_index_to_usize(next_state);
                                                    let mut nested_merged = false;
                                                    if next_idx < node_len && next_idx != scan_idx {
                                                        let next_node = nodes[next_idx];
                                                        let next_scope = next_node.scope();
                                                        let current_scope = node.scope();

                                                        if matches!(
                                                            next_scope.kind(),
                                                            ScopeKind::Route
                                                        ) && !next_scope.is_none()
                                                            && next_scope.local_ordinal()
                                                                != current_scope.local_ordinal()
                                                        {
                                                            let nested_ordinal =
                                                                next_scope.local_ordinal();
                                                            let mut nested_entry_idx = 0usize;
                                                            while nested_entry_idx
                                                                < scope_entries_len
                                                            {
                                                                if scope_entries[nested_entry_idx]
                                                                    .scope_id
                                                                    .local_ordinal()
                                                                    == nested_ordinal
                                                                {
                                                                    let nested = &scope_entries
                                                                        [nested_entry_idx];
                                                                    let mut ni = 0usize;
                                                                    while ni
                                                                        < nested.first_recv_len
                                                                            as usize
                                                                    {
                                                                        let (
                                                                            nlabel,
                                                                            _narm,
                                                                            ntarget,
                                                                        ) = nested
                                                                            .first_recv_dispatch
                                                                            [ni];
                                                                        let mut nconflict = false;
                                                                        let mut nfound = false;
                                                                        let mut ci = 0usize;
                                                                        while ci
                                                                            < dispatch_len as usize
                                                                        {
                                                                            let (
                                                                                existing_label,
                                                                                existing_arm,
                                                                                existing_target,
                                                                            ) = dispatch_table[ci];
                                                                            if existing_label
                                                                                == nlabel
                                                                            {
                                                                                nfound = true;
                                                                                let same_continuation =
                                                                                existing_target.raw()
                                                                                    == ntarget.raw()
                                                                                    || continuations_equivalent(
                                                                                        &nodes,
                                                                                        scope_end,
                                                                                        existing_target,
                                                                                        ntarget,
                                                                                    );
                                                                                if same_continuation
                                                                                {
                                                                                    if existing_arm != arm && existing_arm != ARM_SHARED {
                                                                                    dispatch_table[ci] =
                                                                                        (nlabel, ARM_SHARED, existing_target);
                                                                                }
                                                                                } else {
                                                                                    nconflict =
                                                                                        true;
                                                                                }
                                                                                break;
                                                                            }
                                                                            ci += 1;
                                                                        }
                                                                        if nconflict {
                                                                            dispatch_functional =
                                                                                false;
                                                                        } else if !nfound {
                                                                            if dispatch_len
                                                                                >= MAX_FIRST_RECV_DISPATCH as u8
                                                                            {
                                                                                panic!(
                                                                                    "FIRST-recv dispatch table overflow from nested"
                                                                                );
                                                                            }
                                                                            dispatch_table
                                                                                [dispatch_len
                                                                                    as usize] = (
                                                                                nlabel, arm,
                                                                                ntarget,
                                                                            );
                                                                            dispatch_len += 1;
                                                                        }
                                                                        ni += 1;
                                                                    }
                                                                    nested_merged = true;
                                                                    break;
                                                                }
                                                                nested_entry_idx += 1;
                                                            }
                                                        }
                                                    }

                                                    // If we didn't hit a nested route, continue scanning forward
                                                    // to find the first recv label (decision frontier).
                                                    if !nested_merged
                                                        && !next_state.is_max()
                                                        && scan_len < scan_stack.len()
                                                    {
                                                        scan_stack[scan_len] = next_state;
                                                        scan_len += 1;
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    arm += 1;
                                }

                                let mut prefix_mismatch = false;
                                if dispatch_len > 0 {
                                    if prefix_lens[0] != prefix_lens[1] {
                                        prefix_mismatch = true;
                                    } else {
                                        let mut pi = 0usize;
                                        while pi < prefix_lens[0] {
                                            if !prefix_action_eq(
                                                prefix_actions[0][pi],
                                                prefix_actions[1][pi],
                                            ) {
                                                prefix_mismatch = true;
                                                break;
                                            }
                                            pi += 1;
                                        }
                                    }
                                    if prefix_mismatch {
                                        dispatch_functional = false;
                                    }
                                }

                                let scope_end = as_state_index(node_len);
                                let arm0_entry = scope_entries[entry_idx].passive_arm_entry[0];
                                let arm1_entry = scope_entries[entry_idx].passive_arm_entry[1];
                                let mergeable =
                                    arm_sequences_equal(&nodes, scope_end, arm0_entry, arm1_entry);
                                scope_entries[entry_idx].mergeable = mergeable;

                                if mergeable {
                                    scope_entries[entry_idx].passive_arm_entry[1] =
                                        scope_entries[entry_idx].passive_arm_entry[0];
                                    scope_entries[entry_idx].first_recv_dispatch =
                                        [(0, 0, StateIndex::MAX); MAX_FIRST_RECV_DISPATCH];
                                    scope_entries[entry_idx].first_recv_len = 0;
                                } else if dispatch_functional && dispatch_len > 0 {
                                    scope_entries[entry_idx].first_recv_dispatch = dispatch_table;
                                    scope_entries[entry_idx].first_recv_len = dispatch_len;
                                    let mut offer_lanes = scope_entries[entry_idx].offer_lanes;
                                    let mut di = 0u8;
                                    while di < dispatch_len {
                                        let target_idx =
                                            state_index_to_usize(dispatch_table[di as usize].2);
                                        if target_idx < node_len
                                            && let LocalAction::Recv { lane, .. } =
                                                nodes[target_idx].action()
                                        {
                                            offer_lanes |= offer_lane_bit(lane);
                                        }
                                        di += 1;
                                    }
                                    scope_entries[entry_idx].offer_lanes = offer_lanes;
                                } else if scope_entries[entry_idx].has_route_policy {
                                    scope_entries[entry_idx].first_recv_dispatch =
                                        [(0, 0, StateIndex::MAX); MAX_FIRST_RECV_DISPATCH];
                                    scope_entries[entry_idx].first_recv_len = 0;
                                } else {
                                    panic!(
                                        "Route unprojectable for this role: arms not mergeable, wire dispatch non-deterministic, and no dynamic policy annotation provided"
                                    );
                                }
                            }
                        }

                        if matches!(scope_entries[entry_idx].kind, ScopeKind::Route)
                            && !offer_entry_locked
                        {
                            scope_entries[entry_idx].offer_entry =
                                if scope_entries[entry_idx].linger {
                                    StateIndex::MAX
                                } else {
                                    scope_entries[entry_idx].start
                                };
                        }

                        scope_entries[entry_idx].end = as_state_index(node_len);
                    }
                }
                scope_marker_idx += 1;
            }

            if eff_idx == slice.len() {
                break;
            }

            let current_scope = if scope_stack_len == 0 {
                ScopeId::none()
            } else {
                scope_stack[scope_stack_len - 1]
            };
            // Find the innermost loop scope (either ScopeKind::Loop or linger Route).
            // Linger scopes are 2-arm Routes with linger=true (like LoopContinue/LoopBreak).
            let mut loop_scope = None;
            let mut search = scope_stack_len;
            while search > 0 {
                let idx = search - 1;
                if matches!(scope_stack_kinds[idx], ScopeKind::Loop) {
                    loop_scope = Some(scope_stack[idx]);
                    break;
                }
                // Also check for linger Route scopes
                if matches!(scope_stack_kinds[idx], ScopeKind::Route) {
                    let entry_idx = scope_stack_entries[idx];
                    if scope_entries[entry_idx].linger {
                        loop_scope = Some(scope_stack[idx]);
                        break;
                    }
                }
                search -= 1;
            }

            let eff = slice[eff_idx];
            if matches!(eff.kind, eff::EffKind::Atom) {
                let atom = eff.atom_data();
                let policy = match program.policy_at(eff_idx) {
                    Some(policy) => policy.with_scope(current_scope),
                    None => PolicyMode::Static,
                };
                let control_spec = if atom.is_control {
                    program.control_spec_at(eff_idx)
                } else {
                    None
                };
                let loop_control = LoopControlMeaning::from_control_spec(control_spec);
                let shot = if atom.is_control {
                    match control_spec {
                        Some(spec) => Some(spec.shot),
                        None => None,
                    }
                } else {
                    None
                };
                if scope_stack_len > 0
                    && matches!(scope_stack_kinds[scope_stack_len - 1], ScopeKind::Route)
                {
                    let entry_idx = scope_stack_entries[scope_stack_len - 1];
                    let entry = &mut scope_entries[entry_idx];
                    if policy.is_dynamic() {
                        if !entry.has_route_policy {
                            entry.route_policy = policy;
                            entry.route_policy_eff = as_eff_index(eff_idx);
                            entry.route_policy_tag = match atom.resource {
                                Some(tag) => tag,
                                None => 0,
                            };
                            entry.has_route_policy = true;
                        } else if route_policy_differs(entry.route_policy, policy) {
                            panic!(
                                "route scope recorded conflicting controller policy annotations"
                            );
                        }
                    }
                    if policy.is_dynamic() || loop_control.is_some() {
                        entry.offer_lanes |= offer_lane_bit(atom.lane);
                    }
                }

                // Passive observer arm tracking is now handled by ScopeMarker Enter events.
                // The arm index is determined solely by route_enter_count (set in ScopeEvent::Enter).
                // Passive observer arm start positions are recorded when the first node of each
                // arm is generated (in Local/Send/Recv processing below).
                //
                // Note: We no longer need to track "other role's self-send" here because:
                // 1. All roles see the same ScopeMarker Enter/Exit events
                // 2. Arm index is route_current_arm = route_enter_count - 1 (set at Enter)
                // 3. Passive arm starts are recorded at first node generation per arm

                if atom.from == ROLE && atom.to == ROLE {
                    // Compute route_arm for local actions (self-send).
                    // Arm index is determined solely by ScopeMarker Enter count (binary route).
                    // route_current_arm is set at ScopeEvent::Enter: arm = enter_count - 1.
                    //
                    // Note: Local nodes (self-send) are never choice determinants
                    // (passive observers only see recv nodes on the wire).
                    let route_arm = if scope_stack_len > 0
                        && matches!(scope_stack_kinds[scope_stack_len - 1], ScopeKind::Route)
                    {
                        let stack_idx = scope_stack_len - 1;
                        let arm = route_current_arm[stack_idx] as usize;
                        let entry_idx = scope_stack_entries[stack_idx];

                        let entry = &mut scope_entries[entry_idx];
                        debug_assert!(
                            !matches!(entry.kind, ScopeKind::Route)
                                || entry.controller_role.is_some(),
                            "route scope missing controller_role"
                        );
                        let is_controller = match entry.controller_role {
                            Some(role) => role == ROLE,
                            None => false,
                        };

                        // Record arm entry for local actions.
                        // Controller roles use controller_arm_entry; passive observers track
                        // the first local action via passive_arm_entry when no wire recv exists.
                        if arm < 2 {
                            if is_controller {
                                if entry.controller_arm_entry[arm].is_max() {
                                    entry.controller_arm_entry[arm] = as_state_index(node_len);
                                    entry.controller_arm_label[arm] = atom.label;
                                }
                            } else if entry.passive_arm_entry[arm].is_max() {
                                entry.passive_arm_entry[arm] = as_state_index(node_len);
                            }
                        }

                        Some(route_current_arm[stack_idx])
                    } else {
                        None
                    };

                    // Update the current_state after potential Jump node insertion
                    let current_state = as_state_index(node_len);
                    let mut next = as_state_index(node_len + 1);
                    // Loop continue decisions jump back to the loop start.
                    if matches!(loop_control, Some(LoopControlMeaning::Continue))
                        && let Some(scope_id) = loop_scope
                        && let Some(entry) = find_loop_entry_state(
                            &loop_entry_ids,
                            &loop_entry_states,
                            loop_entry_len,
                            scope_id,
                        )
                    {
                        next = entry;
                    }

                    nodes[node_len] = LocalNode::local(
                        as_eff_index(eff_idx),
                        atom.label,
                        atom.resource,
                        atom.is_control,
                        shot,
                        policy,
                        atom.lane,
                        next,
                        current_scope,
                        loop_scope,
                        route_arm,
                        false, // Local nodes are never choice determinants
                    );
                    let lane_idx = atom.lane as usize;
                    let mut stack_idx = 0usize;
                    while stack_idx < scope_stack_len {
                        let entry_idx = scope_stack_entries[stack_idx];
                        if scope_entries[entry_idx].lane_first_eff[lane_idx].raw()
                            == EffIndex::MAX.raw()
                        {
                            scope_entries[entry_idx].lane_first_eff[lane_idx] =
                                as_eff_index(eff_idx);
                        }
                        scope_entries[entry_idx].lane_last_eff[lane_idx] = as_eff_index(eff_idx);
                        if matches!(scope_stack_kinds[stack_idx], ScopeKind::Route) {
                            let arm = route_current_arm[stack_idx] as usize;
                            if arm < 2 {
                                scope_entries[entry_idx].arm_lane_last_eff[arm][lane_idx] =
                                    as_eff_index(eff_idx);
                            }
                        }
                        stack_idx += 1;
                    }
                    if let Some(scope_id) = loop_scope
                        && loop_control.is_none()
                    {
                        store_loop_entry_if_absent(
                            &mut loop_entry_ids,
                            &mut loop_entry_states,
                            &mut loop_entry_len,
                            scope_id,
                            current_state,
                        );
                    }
                    // Update linger arm tracking for self-send LoopBreak.
                    if let Some(scope_id) = loop_scope {
                        let mut li = 0;
                        while li < linger_arm_len {
                            if linger_arm_scope_ids[li].local_ordinal() == scope_id.local_ordinal()
                            {
                                if matches!(loop_control, Some(LoopControlMeaning::Break)) {
                                    linger_arm_current[li] = 1;
                                }
                                break;
                            }
                            li += 1;
                        }
                    }
                    // Update linger arm tracking for all active linger scopes (outer + inner).
                    if linger_arm_len > 0 {
                        let mut stack_idx = 0usize;
                        while stack_idx < scope_stack_len {
                            let entry_idx = scope_stack_entries[stack_idx];
                            if scope_entries[entry_idx].linger {
                                let scope_id = scope_stack[stack_idx];
                                let mut li = 0usize;
                                while li < linger_arm_len {
                                    if linger_arm_scope_ids[li].local_ordinal()
                                        == scope_id.local_ordinal()
                                    {
                                        let arm = linger_arm_current[li] as usize;
                                        if arm < 2 {
                                            linger_arm_last_node[li][arm] = node_len;
                                        }
                                        break;
                                    }
                                    li += 1;
                                }
                            }
                            stack_idx += 1;
                        }
                    }
                    // Scope-as-Block: Update non-linger Route arm tracking and reset flag.
                    if scope_stack_len > 0
                        && matches!(scope_stack_kinds[scope_stack_len - 1], ScopeKind::Route)
                    {
                        let stack_idx = scope_stack_len - 1;
                        let entry_idx = scope_stack_entries[stack_idx];
                        if !scope_entries[entry_idx].linger {
                            // Reset "last step was scope" flag
                            last_step_was_scope[stack_idx] = false;
                            // Track last node for current arm
                            if let Some(arm) = route_arm {
                                if (arm as usize) < 2 {
                                    route_arm_last_node[stack_idx][arm as usize] =
                                        as_state_index(node_len);
                                }
                            }
                        }
                    }
                    node_len += 1;
                } else if atom.from == ROLE {
                    // Compute route_arm for send nodes inside a route scope.
                    // This is needed for linger rewind logic to distinguish arms.
                    //
                    // Arm index is determined solely by ScopeMarker Enter count (binary route).
                    // route_current_arm is set at ScopeEvent::Enter: arm = enter_count - 1.
                    //
                    // Note: Send nodes are never choice determinants (passive observers
                    // only see recv nodes on the wire).
                    let route_arm = if scope_stack_len > 0
                        && matches!(scope_stack_kinds[scope_stack_len - 1], ScopeKind::Route)
                    {
                        let stack_idx = scope_stack_len - 1;
                        let arm = route_current_arm[stack_idx];
                        let entry_idx = scope_stack_entries[stack_idx];

                        // Record passive_arm_entry for the first cross-role Send of each arm.
                        // This is used for passive observer arm navigation in linger routes
                        // where an arm may have Send nodes but no Recv nodes.
                        if (arm as usize) < 2
                            && scope_entries[entry_idx].passive_arm_entry[arm as usize].is_max()
                        {
                            scope_entries[entry_idx].passive_arm_entry[arm as usize] =
                                as_state_index(node_len);
                        }

                        Some(arm)
                    } else {
                        None
                    };

                    // Update the current_state after potential Jump node insertion
                    let current_state = as_state_index(node_len);
                    let mut next = as_state_index(node_len + 1);
                    // Loop continue decisions jump back to the loop start.
                    if matches!(loop_control, Some(LoopControlMeaning::Continue))
                        && let Some(scope_id) = loop_scope
                        && let Some(entry) = find_loop_entry_state(
                            &loop_entry_ids,
                            &loop_entry_states,
                            loop_entry_len,
                            scope_id,
                        )
                    {
                        next = entry;
                    }

                    nodes[node_len] = LocalNode::send(
                        as_eff_index(eff_idx),
                        atom.to,
                        atom.label,
                        atom.resource,
                        atom.is_control,
                        shot,
                        policy,
                        atom.lane,
                        next,
                        current_scope,
                        loop_scope,
                        route_arm,
                        false, // Send nodes are never choice determinants
                    );
                    let lane_idx = atom.lane as usize;
                    let mut stack_idx = 0usize;
                    while stack_idx < scope_stack_len {
                        let entry_idx = scope_stack_entries[stack_idx];
                        if scope_entries[entry_idx].lane_first_eff[lane_idx].raw()
                            == EffIndex::MAX.raw()
                        {
                            scope_entries[entry_idx].lane_first_eff[lane_idx] =
                                as_eff_index(eff_idx);
                        }
                        scope_entries[entry_idx].lane_last_eff[lane_idx] = as_eff_index(eff_idx);
                        if matches!(scope_stack_kinds[stack_idx], ScopeKind::Route) {
                            let arm = route_current_arm[stack_idx] as usize;
                            if arm < 2 {
                                scope_entries[entry_idx].arm_lane_last_eff[arm][lane_idx] =
                                    as_eff_index(eff_idx);
                            }
                        }
                        stack_idx += 1;
                    }
                    if let Some(scope_id) = loop_scope
                        && loop_control.is_none()
                    {
                        store_loop_entry_if_absent(
                            &mut loop_entry_ids,
                            &mut loop_entry_states,
                            &mut loop_entry_len,
                            scope_id,
                            current_state,
                        );
                    }
                    // Update linger arm tracking for all active linger scopes (outer + inner).
                    if linger_arm_len > 0 {
                        let mut stack_idx = 0usize;
                        while stack_idx < scope_stack_len {
                            let entry_idx = scope_stack_entries[stack_idx];
                            if scope_entries[entry_idx].linger {
                                let scope_id = scope_stack[stack_idx];
                                let mut li = 0usize;
                                while li < linger_arm_len {
                                    if linger_arm_scope_ids[li].local_ordinal()
                                        == scope_id.local_ordinal()
                                    {
                                        let arm = linger_arm_current[li] as usize;
                                        if arm < 2 {
                                            linger_arm_last_node[li][arm] = node_len;
                                        }
                                        break;
                                    }
                                    li += 1;
                                }
                            }
                            stack_idx += 1;
                        }
                    }
                    // Scope-as-Block: Update non-linger Route arm tracking and reset flag.
                    if scope_stack_len > 0
                        && matches!(scope_stack_kinds[scope_stack_len - 1], ScopeKind::Route)
                    {
                        let stack_idx = scope_stack_len - 1;
                        let entry_idx = scope_stack_entries[stack_idx];
                        if !scope_entries[entry_idx].linger {
                            // Reset "last step was scope" flag
                            last_step_was_scope[stack_idx] = false;
                            // Track last node for current arm
                            if let Some(arm) = route_arm {
                                if (arm as usize) < 2 {
                                    route_arm_last_node[stack_idx][arm as usize] =
                                        as_state_index(node_len);
                                }
                            }
                        }
                    }
                    node_len += 1;
                } else if atom.to == ROLE {
                    // Determine route_arm and is_choice_determinant for this recv node.
                    // Arm index is determined solely by ScopeMarker Enter count (binary route).
                    // route_current_arm is set at ScopeEvent::Enter: arm = enter_count - 1.
                    //
                    // is_choice_determinant: The first recv of each arm is a choice determinant
                    // for passive observer mode (allows label-based arm resolution).
                    let (route_arm, is_choice_determinant) = if scope_stack_len > 0
                        && matches!(scope_stack_kinds[scope_stack_len - 1], ScopeKind::Route)
                    {
                        let stack_idx = scope_stack_len - 1;
                        let arm = route_current_arm[stack_idx];
                        let entry_idx = scope_stack_entries[stack_idx];
                        let entry = &mut scope_entries[entry_idx];

                        // Record passive_arm_entry for the first cross-role Recv of each arm.
                        // This is used for passive observer arm navigation.
                        // Note: Send processing also sets this, so we check if not already set.
                        if (arm as usize) < 2 {
                            let existing = entry.passive_arm_entry[arm as usize];
                            let should_set = if existing.is_max() {
                                true
                            } else {
                                let existing_node = nodes[state_index_to_usize(existing)];
                                !matches!(existing_node.action(), LocalAction::Recv { .. })
                            };
                            if should_set {
                                entry.passive_arm_entry[arm as usize] = as_state_index(node_len);
                            }
                        }

                        // Check if this is the first recv for this arm in this scope.
                        // route_recv_len tracks how many recv nodes we've registered.
                        // For binary routes: arm 0 = recv_len 0, arm 1 = recv_len 1.
                        let is_first_recv_of_arm = (arm as u16) == entry.route_recv_len;

                        if is_first_recv_of_arm && (arm as usize) < 2 {
                            // Register this recv in route_recv_indices (for arm lookup)
                            if entry.route_recv_len >= (u8::MAX as u16) {
                                panic!("route recv arm overflow");
                            }
                            if route_recv_nodes_len >= MAX_STATES {
                                panic!("route recv node capacity exceeded");
                            }
                            let current_state = as_state_index(node_len);
                            route_recv_nodes[route_recv_nodes_len] = RouteRecvNode {
                                state: current_state,
                                next: RouteRecvIndex::MAX,
                            };
                            if entry.route_recv_head.is_max() {
                                entry.route_recv_head =
                                    RouteRecvIndex::from_usize(route_recv_nodes_len);
                            } else {
                                let tail_idx = entry.route_recv_tail.as_usize();
                                route_recv_nodes[tail_idx].next =
                                    RouteRecvIndex::from_usize(route_recv_nodes_len);
                            }
                            entry.route_recv_tail =
                                RouteRecvIndex::from_usize(route_recv_nodes_len);
                            entry.route_recv_len += 1;
                            route_recv_nodes_len += 1;
                            entry.offer_lanes |= offer_lane_bit(atom.lane);
                            (Some(arm), true) // First recv of arm = choice determinant
                        } else {
                            // Subsequent recv within the same arm - not a choice determinant
                            (Some(arm), false)
                        }
                    } else {
                        (None, false)
                    };

                    // Update the current_state after potential Jump node insertion
                    let current_state = as_state_index(node_len);
                    let mut next = as_state_index(node_len + 1);
                    // Loop continue decisions jump back to the loop start.
                    if matches!(loop_control, Some(LoopControlMeaning::Continue))
                        && let Some(scope_id) = loop_scope
                        && let Some(entry) = find_loop_entry_state(
                            &loop_entry_ids,
                            &loop_entry_states,
                            loop_entry_len,
                            scope_id,
                        )
                    {
                        next = entry;
                    }

                    nodes[node_len] = LocalNode::recv(
                        as_eff_index(eff_idx),
                        atom.from,
                        atom.label,
                        atom.resource,
                        atom.is_control,
                        shot,
                        policy,
                        atom.lane,
                        next,
                        current_scope,
                        loop_scope,
                        route_arm,
                        is_choice_determinant,
                    );
                    let lane_idx = atom.lane as usize;
                    let mut stack_idx = 0usize;
                    while stack_idx < scope_stack_len {
                        let entry_idx = scope_stack_entries[stack_idx];
                        if scope_entries[entry_idx].lane_first_eff[lane_idx].raw()
                            == EffIndex::MAX.raw()
                        {
                            scope_entries[entry_idx].lane_first_eff[lane_idx] =
                                as_eff_index(eff_idx);
                        }
                        scope_entries[entry_idx].lane_last_eff[lane_idx] = as_eff_index(eff_idx);
                        if matches!(scope_stack_kinds[stack_idx], ScopeKind::Route) {
                            let arm = route_current_arm[stack_idx] as usize;
                            if arm < 2 {
                                scope_entries[entry_idx].arm_lane_last_eff[arm][lane_idx] =
                                    as_eff_index(eff_idx);
                            }
                        }
                        stack_idx += 1;
                    }
                    if let Some(scope_id) = loop_scope
                        && loop_control.is_none()
                    {
                        store_loop_entry_if_absent(
                            &mut loop_entry_ids,
                            &mut loop_entry_states,
                            &mut loop_entry_len,
                            scope_id,
                            current_state,
                        );
                    }
                    // Update linger arm tracking for all active linger scopes (outer + inner).
                    if linger_arm_len > 0 {
                        let mut stack_idx = 0usize;
                        while stack_idx < scope_stack_len {
                            let entry_idx = scope_stack_entries[stack_idx];
                            if scope_entries[entry_idx].linger {
                                let scope_id = scope_stack[stack_idx];
                                let mut li = 0usize;
                                while li < linger_arm_len {
                                    if linger_arm_scope_ids[li].local_ordinal()
                                        == scope_id.local_ordinal()
                                    {
                                        let arm = linger_arm_current[li] as usize;
                                        if arm < 2 {
                                            linger_arm_last_node[li][arm] = node_len;
                                        }
                                        break;
                                    }
                                    li += 1;
                                }
                            }
                            stack_idx += 1;
                        }
                    }
                    // Scope-as-Block: Update non-linger Route arm tracking and reset flag.
                    if scope_stack_len > 0
                        && matches!(scope_stack_kinds[scope_stack_len - 1], ScopeKind::Route)
                    {
                        let stack_idx = scope_stack_len - 1;
                        let entry_idx = scope_stack_entries[stack_idx];
                        if !scope_entries[entry_idx].linger {
                            // Reset "last step was scope" flag
                            last_step_was_scope[stack_idx] = false;
                            // Track last node for current arm
                            if let Some(arm) = route_arm {
                                if (arm as usize) < 2 {
                                    route_arm_last_node[stack_idx][arm as usize] =
                                        as_state_index(node_len);
                                }
                            }
                        }
                    }
                    node_len += 1;
                }
            }
            eff_idx += 1;
        }

        if scope_stack_len != 0 {
            panic!("unbalanced structured scope markers");
        }

        if node_len >= MAX_STATES {
            panic!("typestate capacity exceeded for role");
        }

        let mut route_recv_flat = [StateIndex::MAX; MAX_STATES];
        let mut route_recv_flat_len = 0usize;
        let mut entry_idx = 0usize;
        while entry_idx < scope_entries_len {
            if scope_entries[entry_idx].route_recv_len > 0 {
                scope_entries[entry_idx].route_recv_offset =
                    RouteRecvIndex::from_usize(route_recv_flat_len);
                let mut remaining = scope_entries[entry_idx].route_recv_len;
                let mut cursor = scope_entries[entry_idx].route_recv_head;
                while remaining > 0 {
                    if cursor.is_max() {
                        panic!("route recv list truncated");
                    }
                    if route_recv_flat_len >= MAX_STATES {
                        panic!("route recv table overflow");
                    }
                    let node = route_recv_nodes[cursor.as_usize()];
                    route_recv_flat[route_recv_flat_len] = node.state;
                    route_recv_flat_len += 1;
                    cursor = node.next;
                    remaining -= 1;
                }
            } else {
                scope_entries[entry_idx].route_recv_offset =
                    RouteRecvIndex::from_usize(route_recv_flat_len);
            }
            entry_idx += 1;
        }

        // Apply backpatches for Jump nodes.
        // Jump targets that were unknown at node creation time now have their
        // destinations resolved.
        {
            let mut bi = 0;
            while bi < jump_backpatch_len {
                let node_idx = jump_backpatch_indices[bi];
                let scope = jump_backpatch_scopes[bi];
                let kind = jump_backpatch_kinds[bi];

                // Find the scope entry for this scope
                let ordinal = scope.local_ordinal();
                let entry_idx = if ordinal < scope_entry_index_by_ordinal.len() as u16 {
                    scope_entry_index_by_ordinal[ordinal as usize]
                } else {
                    u16::MAX
                };

                if entry_idx == u16::MAX {
                    panic!(
                        "jump backpatch failed: scope ordinal not found in scope_entry_index_by_ordinal"
                    );
                }
                let entry = &scope_entries[entry_idx as usize];
                let target = if kind == 1 || kind == 2 {
                    // scope_end target for LoopBreak Jump (kind=1) or RouteArmEnd (kind=2)
                    entry.end
                } else {
                    // loop_start target for LoopContinue Jump (kind=0)
                    entry.start
                };
                nodes[node_idx] = nodes[node_idx].with_next(target);

                bi += 1;
            }
        }

        let terminal_index = as_state_index(node_len);
        nodes[node_len] = LocalNode::terminal(terminal_index);
        let scope_registry = ScopeRegistry::from_scope_entries(
            scope_entries,
            scope_entries_len,
            route_recv_flat,
            route_recv_flat_len,
        );
        Self::new(nodes, node_len + 1, scope_registry)
    }
}
