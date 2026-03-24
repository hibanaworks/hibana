//! Role-local typestate synthesis derived from `EffList`.
//!
//! This module materialises a compact state machine for a given role directly
//! from an `EffList`. Each state captures the local action (send/recv/control)
//! together with the successor index, allowing higher layers to drive endpoint
//! transitions.

use crate::control::cap::mint::CapShot;
use crate::eff::{self, EffIndex, EffStruct};
use crate::global::const_dsl::{EffList, PolicyMode, ScopeEvent, ScopeId, ScopeKind};
use crate::runtime::consts::{LABEL_LOOP_BREAK, LABEL_LOOP_CONTINUE};

/// Index identifying a local state within the synthesized typestate graph.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StateIndex(u16);

impl StateIndex {
    pub const ZERO: Self = Self(0);
    pub const MAX: Self = Self(u16::MAX);

    #[inline(always)]
    pub const fn new(raw: u16) -> Self {
        Self(raw)
    }

    #[inline(always)]
    pub const fn from_usize(idx: usize) -> Self {
        if idx > (u16::MAX as usize) {
            panic!("state index overflow");
        }
        Self(idx as u16)
    }

    #[inline(always)]
    pub const fn raw(self) -> u16 {
        self.0
    }

    #[inline(always)]
    pub const fn as_usize(self) -> usize {
        self.0 as usize
    }

    #[inline(always)]
    pub const fn is_max(self) -> bool {
        self.0 == u16::MAX
    }
}

impl PartialEq<u16> for StateIndex {
    fn eq(&self, other: &u16) -> bool {
        self.0 == *other
    }
}

impl PartialEq<StateIndex> for u16 {
    fn eq(&self, other: &StateIndex) -> bool {
        *self == other.0
    }
}

/// Index into the per-scope route-recv linked list and flattened recv table.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct RouteRecvIndex(u16);

impl RouteRecvIndex {
    pub(crate) const ZERO: Self = Self(0);
    pub(crate) const MAX: Self = Self(u16::MAX);

    #[inline(always)]
    pub(crate) const fn from_usize(idx: usize) -> Self {
        if idx > (u16::MAX as usize) {
            panic!("route recv index overflow");
        }
        Self(idx as u16)
    }

    #[inline(always)]
    pub(crate) const fn as_usize(self) -> usize {
        self.0 as usize
    }

    #[inline(always)]
    pub(crate) const fn is_max(self) -> bool {
        self.0 == u16::MAX
    }
}

/// Maximum number of local states tracked per role (one extra slot for the
/// terminal state).
pub(crate) const MAX_STATES: usize = eff::meta::MAX_EFF_NODES + 1;

/// Reason for an explicit control flow jump.
///
/// Used for debugging and observability to track why a jump occurred.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum JumpReason {
    /// Route arm end → jump to scope_end.
    RouteArmEnd,
    /// Loop continue → jump to loop_start.
    LoopContinue,
    /// Loop break → jump to loop_end.
    LoopBreak,
    /// Passive observer branch → jump to arm start.
    PassiveObserverBranch,
}

/// Error during jump traversal.
///
/// Indicates a bug in the typestate compiler (CFG cycle).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct JumpError {
    /// Number of iterations before cycle detected.
    pub iterations: u32,
    /// Node index where cycle was detected.
    pub idx: usize,
}

/// Result of following a passive observer arm in a route scope.
///
/// With CFG-pure design, all arms (including τ-eliminated ones) have
/// ArmEmpty placeholder nodes, so navigation always returns a valid entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PassiveArmNavigation {
    /// Jumped to a node within the arm.
    /// For τ-eliminated arms, this points to the ArmEmpty (RouteArmEnd) placeholder.
    WithinArm { entry: StateIndex },
}

/// Local action associated with a typestate node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LocalAction {
    /// Placeholder used to prefill the backing array.
    None,
    /// Role sends a message to a peer.
    Send {
        eff_index: EffIndex,
        peer: u8,
        label: u8,
        resource: Option<u8>,
        is_control: bool,
        shot: Option<CapShot>,
        policy: PolicyMode,
        /// Type-level lane for parallel composition (default 0).
        lane: u8,
    },
    /// Role receives a message from a peer.
    Recv {
        eff_index: EffIndex,
        peer: u8,
        label: u8,
        resource: Option<u8>,
        is_control: bool,
        shot: Option<CapShot>,
        policy: PolicyMode,
        /// Type-level lane for parallel composition (default 0).
        lane: u8,
    },
    /// Role executes a local action (self-send).
    Local {
        eff_index: EffIndex,
        label: u8,
        resource: Option<u8>,
        is_control: bool,
        shot: Option<CapShot>,
        policy: PolicyMode,
        /// Type-level lane for parallel composition (default 0).
        lane: u8,
    },
    /// Terminal node (no further actions).
    Terminate,
    /// Explicit control flow jump (target is stored in `LocalNode.next` field).
    ///
    /// Option C design: the `reason` field provides debugging/observability,
    /// while the actual target is the `next` field of `LocalNode`.
    Jump {
        /// Why this jump was generated.
        reason: JumpReason,
    },
}

impl LocalAction {
    /// True when the node corresponds to a send action.
    #[inline(always)]
    pub(crate) const fn is_send(&self) -> bool {
        matches!(self, Self::Send { .. })
    }

    /// True when the node corresponds to a receive action.
    #[inline(always)]
    pub(crate) const fn is_recv(&self) -> bool {
        matches!(self, Self::Recv { .. })
    }

    /// True when the node marks a terminal state.
    #[cfg(test)]
    #[inline(always)]
    pub(crate) const fn is_terminal(&self) -> bool {
        matches!(self, Self::Terminate)
    }

    /// True when the node corresponds to a local action.
    #[inline(always)]
    pub(crate) const fn is_local_action(&self) -> bool {
        matches!(self, Self::Local { .. })
    }

    /// True when the node corresponds to an explicit control flow jump.
    #[inline(always)]
    pub(crate) const fn is_jump(&self) -> bool {
        matches!(self, Self::Jump { .. })
    }

    /// Returns the jump reason if this is a Jump action.
    #[inline(always)]
    pub(crate) const fn jump_reason(&self) -> Option<JumpReason> {
        match self {
            Self::Jump { reason } => Some(*reason),
            _ => None,
        }
    }
}

/// Single node in the synthesized typestate graph.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LocalNode {
    action: LocalAction,
    next: StateIndex,
    scope: ScopeId,
    loop_scope: Option<ScopeId>,
    route_arm: Option<u8>,
    /// Whether this node is a choice determinant (first recv of a route arm).
    /// Used by passive observers to identify which recv determines route selection.
    is_choice_determinant: bool,
}

impl LocalNode {
    /// Placeholder used to prefill the backing array.
    pub(crate) const EMPTY: Self = Self {
        action: LocalAction::None,
        next: StateIndex::ZERO,
        scope: ScopeId::none(),
        loop_scope: None,
        route_arm: None,
        is_choice_determinant: false,
    };

    /// Construct a send node that advances to `next`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) const fn send(
        eff_index: EffIndex,
        peer: u8,
        label: u8,
        resource: Option<u8>,
        is_control: bool,
        shot: Option<CapShot>,
        policy: PolicyMode,
        lane: u8,
        next: StateIndex,
        scope: ScopeId,
        loop_scope: Option<ScopeId>,
        route_arm: Option<u8>,
        is_choice_determinant: bool,
    ) -> Self {
        Self {
            action: LocalAction::Send {
                eff_index,
                peer,
                label,
                resource,
                is_control,
                shot,
                policy,
                lane,
            },
            next,
            scope,
            loop_scope,
            route_arm,
            is_choice_determinant,
        }
    }

    /// Construct a receive node that advances to `next`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) const fn recv(
        eff_index: EffIndex,
        peer: u8,
        label: u8,
        resource: Option<u8>,
        is_control: bool,
        shot: Option<CapShot>,
        policy: PolicyMode,
        lane: u8,
        next: StateIndex,
        scope: ScopeId,
        loop_scope: Option<ScopeId>,
        route_arm: Option<u8>,
        is_choice_determinant: bool,
    ) -> Self {
        Self {
            action: LocalAction::Recv {
                eff_index,
                peer,
                label,
                resource,
                is_control,
                shot,
                policy,
                lane,
            },
            next,
            scope,
            loop_scope,
            route_arm,
            is_choice_determinant,
        }
    }

    /// Construct a local action node that advances to `next`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) const fn local(
        eff_index: EffIndex,
        label: u8,
        resource: Option<u8>,
        is_control: bool,
        shot: Option<CapShot>,
        policy: PolicyMode,
        lane: u8,
        next: StateIndex,
        scope: ScopeId,
        loop_scope: Option<ScopeId>,
        route_arm: Option<u8>,
        is_choice_determinant: bool,
    ) -> Self {
        Self {
            action: LocalAction::Local {
                eff_index,
                label,
                resource,
                is_control,
                shot,
                policy,
                lane,
            },
            next,
            scope,
            loop_scope,
            route_arm,
            is_choice_determinant,
        }
    }

    /// Construct a terminal node that loops to itself.
    pub(crate) const fn terminal(index: StateIndex) -> Self {
        Self {
            action: LocalAction::Terminate,
            next: index,
            scope: ScopeId::none(),
            loop_scope: None,
            route_arm: None,
            is_choice_determinant: false,
        }
    }

    /// Construct a jump node for explicit control flow.
    ///
    /// Option C design: the `target` is stored in `next`, and `reason` provides
    /// debugging/observability information.
    pub(crate) const fn jump(
        target: StateIndex,
        reason: JumpReason,
        scope: ScopeId,
        loop_scope: Option<ScopeId>,
        route_arm: Option<u8>,
    ) -> Self {
        Self {
            action: LocalAction::Jump { reason },
            next: target,
            scope,
            loop_scope,
            route_arm,
            is_choice_determinant: false,
        }
    }

    /// Action associated with the node.
    #[inline(always)]
    pub(crate) const fn action(&self) -> LocalAction {
        self.action
    }

    /// Successor state reached after performing the action.
    #[inline(always)]
    pub(crate) const fn next(&self) -> StateIndex {
        self.next
    }

    /// Scope identifier associated with the node, when present.
    #[inline(always)]
    pub(crate) const fn scope(&self) -> ScopeId {
        self.scope
    }

    #[inline(always)]
    pub(crate) const fn loop_scope(&self) -> Option<ScopeId> {
        self.loop_scope
    }

    #[inline(always)]
    pub(crate) const fn route_arm(&self) -> Option<u8> {
        self.route_arm
    }

    /// Whether this node is a choice determinant (first recv of a route arm).
    #[inline(always)]
    pub(crate) const fn is_choice_determinant(&self) -> bool {
        self.is_choice_determinant
    }

    /// Returns a copy of this node with a different `next` value.
    ///
    /// Used for backpatching during typestate construction.
    #[inline(always)]
    pub(crate) const fn with_next(self, next: StateIndex) -> Self {
        Self { next, ..self }
    }

    #[inline(always)]
    pub(crate) const fn with_scope(self, scope: ScopeId) -> Self {
        Self { scope, ..self }
    }

    #[inline(always)]
    pub(crate) const fn with_route_arm(self, route_arm: Option<u8>) -> Self {
        Self { route_arm, ..self }
    }
}

const SCOPE_ORDINAL_INDEX_CAPACITY: usize = ScopeId::ORDINAL_CAPACITY as usize;
const SCOPE_ORDINAL_INDEX_EMPTY: u16 = u16::MAX;

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

    fn lookup_record(&self, scope_id: ScopeId) -> Option<&ScopeRecord> {
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
        if !record.present || record.scope_id != canonical {
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
    fn controller_arm_entry_by_arm(&self, scope_id: ScopeId, arm: u8) -> Option<(StateIndex, u8)> {
        let record = self.lookup_record(scope_id)?;
        if arm < 2 && record.controller_arm_entry[arm as usize] != StateIndex::MAX {
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
    fn first_recv_dispatch_entry(
        &self,
        scope_id: ScopeId,
        idx: usize,
    ) -> Option<(u8, u8, StateIndex)> {
        let record = self.lookup_record(scope_id)?;
        if idx >= record.first_recv_len as usize {
            return None;
        }
        Some(record.first_recv_dispatch[idx])
    }
}

/// Role-specific typestate graph synthesized from a global effect list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoleTypestate<const ROLE: u8> {
    nodes: [LocalNode; MAX_STATES],
    len: usize,
    scope_registry: ScopeRegistry,
}

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

    fn scope_region_for(&self, scope_id: ScopeId) -> Option<ScopeRegion> {
        self.scope_registry.lookup_region(scope_id)
    }

    fn scope_parent(&self, scope_id: ScopeId) -> Option<ScopeId> {
        self.scope_registry.parent_of(scope_id)
    }

    /// Get the PassiveObserverBranch Jump target for the specified arm in a scope.
    ///
    /// Returns the StateIndex of the Jump's target node for the given arm (0 or 1),
    /// or `None` if no PassiveObserverBranch Jump is registered for that arm.
    fn passive_arm_jump(&self, scope_id: ScopeId, arm: u8) -> Option<StateIndex> {
        self.scope_registry.passive_arm_jump(scope_id, arm)
    }

    /// Get the passive arm entry index for the specified arm.
    ///
    /// Returns the StateIndex of the first cross-role node (Send or Recv) in the arm,
    /// or `None` if not set.
    fn passive_arm_entry(&self, scope_id: ScopeId, arm: u8) -> Option<StateIndex> {
        self.scope_registry.passive_arm_entry(scope_id, arm)
    }

    fn passive_arm_scope(&self, scope_id: ScopeId, arm: u8) -> Option<ScopeId> {
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

    /// Synthesize a typestate graph from an arbitrary effect list borrow.
    pub const fn from_program(program: &EffList) -> Self {
        Self::build(program, program.as_slice())
    }

    const fn build(program: &EffList, slice: &[EffStruct]) -> Self {
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
                    if policy.is_dynamic()
                        || atom.label == LABEL_LOOP_CONTINUE
                        || atom.label == LABEL_LOOP_BREAK
                    {
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
                    // For LABEL_LOOP_CONTINUE, next should point to loop_start
                    if atom.label == LABEL_LOOP_CONTINUE
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
                        && atom.label != LABEL_LOOP_CONTINUE
                        && atom.label != LABEL_LOOP_BREAK
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
                                if atom.label == LABEL_LOOP_BREAK {
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
                    // For LABEL_LOOP_CONTINUE, next should point to loop_start
                    if atom.label == LABEL_LOOP_CONTINUE
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
                        && atom.label != LABEL_LOOP_CONTINUE
                        && atom.label != LABEL_LOOP_BREAK
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
                    // For LABEL_LOOP_CONTINUE, next should point to loop_start
                    if atom.label == LABEL_LOOP_CONTINUE
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
                        && atom.label != LABEL_LOOP_CONTINUE
                        && atom.label != LABEL_LOOP_BREAK
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

/// Metadata for a send transition derived from typestate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SendMeta {
    pub eff_index: EffIndex,
    pub peer: u8,
    pub label: u8,
    pub resource: Option<u8>,
    pub is_control: bool,
    pub next: usize,
    pub scope: ScopeId,
    pub route_arm: Option<u8>,
    pub shot: Option<CapShot>,
    policy: PolicyMode,
    /// Type-level lane for parallel composition (default 0).
    pub lane: u8,
}

impl SendMeta {
    pub(crate) const fn new(
        eff_index: EffIndex,
        peer: u8,
        label: u8,
        resource: Option<u8>,
        is_control: bool,
        next: usize,
        scope: ScopeId,
        route_arm: Option<u8>,
        shot: Option<CapShot>,
        policy: PolicyMode,
        lane: u8,
    ) -> Self {
        Self {
            eff_index,
            peer,
            label,
            resource,
            is_control,
            next,
            scope,
            route_arm,
            shot,
            policy,
            lane,
        }
    }

    #[inline(always)]
    pub(crate) const fn policy(&self) -> PolicyMode {
        self.policy
    }
}

/// Metadata for a receive transition derived from typestate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RecvMeta {
    pub eff_index: EffIndex,
    pub peer: u8,
    pub label: u8,
    pub resource: Option<u8>,
    pub is_control: bool,
    pub next: usize,
    pub scope: ScopeId,
    pub route_arm: Option<u8>,
    /// Whether this recv is a choice determinant (first recv of a route arm).
    pub is_choice_determinant: bool,
    pub shot: Option<CapShot>,
    pub policy: PolicyMode,
    /// Type-level lane for parallel composition (default 0).
    pub lane: u8,
}

/// Metadata for a local action derived from typestate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LocalMeta {
    pub eff_index: EffIndex,
    pub label: u8,
    pub resource: Option<u8>,
    pub is_control: bool,
    pub next: usize,
    pub scope: ScopeId,
    pub route_arm: Option<u8>,
    pub shot: Option<CapShot>,
    pub policy: PolicyMode,
    /// Type-level lane for parallel composition (default 0).
    pub lane: u8,
}

/// Try to fetch send metadata for a specific typestate location.
/// Returns `None` if the node is not a Send action.
pub(crate) fn try_send_meta<const ROLE: u8>(
    ts: &RoleTypestate<ROLE>,
    idx: usize,
) -> Option<SendMeta> {
    let node = ts.node(idx);
    match node.action() {
        LocalAction::Send {
            eff_index,
            peer,
            label,
            resource,
            is_control,
            shot,
            policy,
            lane,
        } => Some(SendMeta::new(
            eff_index,
            peer,
            label,
            resource,
            is_control,
            state_index_to_usize(node.next()),
            node.scope(),
            node.route_arm(),
            shot,
            policy,
            lane,
        )),
        _ => None,
    }
}

/// Try to fetch receive metadata for a specific typestate location.
/// Returns `None` if the node is not a Recv action.
pub(crate) fn try_recv_meta<const ROLE: u8>(
    ts: &RoleTypestate<ROLE>,
    idx: usize,
) -> Option<RecvMeta> {
    let node = ts.node(idx);
    match node.action() {
        LocalAction::Recv {
            eff_index,
            peer,
            label,
            resource,
            is_control,
            shot,
            policy,
            lane,
        } => Some(RecvMeta {
            eff_index,
            peer,
            label,
            resource,
            is_control,
            next: state_index_to_usize(node.next()),
            scope: node.scope(),
            route_arm: node.route_arm(),
            is_choice_determinant: node.is_choice_determinant(),
            shot,
            policy,
            lane,
        }),
        _ => None,
    }
}

/// Try to fetch local action metadata for a specific typestate location.
/// Returns `None` if the node is not a Local action.
pub(crate) fn try_local_meta<const ROLE: u8>(
    ts: &RoleTypestate<ROLE>,
    idx: usize,
) -> Option<LocalMeta> {
    let node = ts.node(idx);
    match node.action() {
        LocalAction::Local {
            eff_index,
            label,
            resource,
            is_control,
            shot,
            policy,
            lane,
        } => Some(LocalMeta {
            eff_index,
            label,
            resource,
            is_control,
            next: state_index_to_usize(node.next()),
            scope: node.scope(),
            route_arm: node.route_arm(),
            shot,
            policy,
            lane,
        }),
        _ => None,
    }
}

const fn as_eff_index(idx: usize) -> EffIndex {
    EffIndex::from_usize(idx)
}

const fn as_state_index(idx: usize) -> StateIndex {
    StateIndex::from_usize(idx)
}

pub(crate) const fn state_index_to_usize(index: StateIndex) -> usize {
    index.as_usize()
}

/// Role perspective for a loop decision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LoopRole {
    Controller,
    Target,
}

/// Metadata associated with a loop decision site.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LoopMetadata<const ROLE: u8> {
    pub scope: ScopeId,
    pub controller: u8,
    pub target: u8,
    pub role: LoopRole,
    pub eff_index: EffIndex,
    pub decision_index: usize,
    pub continue_index: usize,
    pub break_index: usize,
}

// =============================================================================
// =============================================================================

use crate::global::role_program::{LocalDirection, LocalStep, MAX_LANES, Phase};

/// Maximum phases and steps that PhaseCursor can hold.
const PHASE_CURSOR_MAX_PHASES: usize = 32;
const PHASE_CURSOR_MAX_STEPS: usize = crate::eff::meta::MAX_EFF_NODES;
const PHASE_CURSOR_NO_STEP: u16 = u16::MAX;
const PHASE_CURSOR_NO_STATE: StateIndex = StateIndex::MAX;

/// Phase-aware cursor for multi-lane parallel execution.
///
/// Provides explicit phase/lane tracking for typestate navigation. Each phase represents
/// a fork-join barrier; lanes within a phase execute independently. All lanes must
/// complete before advancing to the next phase.
///
/// # Design Philosophy
///
/// What is expressed in types must be realized at runtime.
///
/// `PhaseCursor` ensures that the parallel structure expressed by `g::par` in the
/// choreography is faithfully represented at runtime, with independent lane cursors
/// and proper barrier semantics.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PhaseCursor<const ROLE: u8> {
    // === Core Typestate (delegates to RoleTypestate) ===
    typestate: RoleTypestate<ROLE>,
    /// Primary typestate index used for scope queries.
    idx: usize,

    /// Current phase index (0-based)
    phase_index: usize,
    /// Per-lane step progress within current phase.
    /// `lane_cursors[lane_idx]` = number of steps completed on that lane.
    lane_cursors: [usize; MAX_LANES],
    /// Label → lane bitmask for the current step on each lane.
    /// Updated when lane cursors advance.
    label_lane_mask: [u8; 256],

    phases: [Phase; PHASE_CURSOR_MAX_PHASES],
    phase_len: usize,
    local_steps: [LocalStep; PHASE_CURSOR_MAX_STEPS],
    local_steps_len: usize,
    eff_index_to_step: [u16; PHASE_CURSOR_MAX_STEPS],
    step_index_to_state: [StateIndex; PHASE_CURSOR_MAX_STEPS],
}

impl<const ROLE: u8> PhaseCursor<ROLE> {
    // =========================================================================
    // Construction
    // =========================================================================

    /// Create a PhaseCursor from a RoleProgram.
    pub(crate) fn new<Steps, Mint>(
        program: &crate::global::role_program::RoleProgram<'_, ROLE, Steps, Mint>,
    ) -> Self
    where
        Mint: crate::control::cap::mint::MintConfigMarker,
    {
        let projection = program.projection();

        let mut phases = [Phase::EMPTY; PHASE_CURSOR_MAX_PHASES];
        let phase_len = projection.phase_count().min(PHASE_CURSOR_MAX_PHASES);
        for i in 0..phase_len {
            phases[i] = projection.phases()[i];
        }

        let mut local_steps = [LocalStep::EMPTY; PHASE_CURSOR_MAX_STEPS];
        let local_steps_len = projection.len().min(PHASE_CURSOR_MAX_STEPS);
        for i in 0..local_steps_len {
            local_steps[i] = projection.steps()[i];
        }

        let mut eff_index_to_step = [PHASE_CURSOR_NO_STEP; PHASE_CURSOR_MAX_STEPS];
        let mut step_idx = 0usize;
        while step_idx < local_steps_len {
            let eff_index = local_steps[step_idx].eff_index().as_usize();
            if eff_index < PHASE_CURSOR_MAX_STEPS {
                debug_assert!(
                    eff_index_to_step[eff_index] == PHASE_CURSOR_NO_STEP,
                    "duplicate eff_index in local steps"
                );
                eff_index_to_step[eff_index] = step_idx as u16;
            }
            step_idx += 1;
        }

        let typestate = *projection.typestate();
        let mut step_index_to_state = [PHASE_CURSOR_NO_STATE; PHASE_CURSOR_MAX_STEPS];
        let mut node_idx = 0usize;
        while node_idx < typestate.len() {
            let node = typestate.node(node_idx);
            match node.action() {
                LocalAction::Send {
                    eff_index,
                    peer,
                    label,
                    lane,
                    ..
                } => {
                    Self::map_step_index_to_state(
                        &local_steps,
                        local_steps_len,
                        &eff_index_to_step,
                        &mut step_index_to_state,
                        node_idx,
                        eff_index,
                        LocalDirection::Send,
                        label,
                        peer,
                        lane,
                    );
                }
                LocalAction::Recv {
                    eff_index,
                    peer,
                    label,
                    lane,
                    ..
                } => {
                    Self::map_step_index_to_state(
                        &local_steps,
                        local_steps_len,
                        &eff_index_to_step,
                        &mut step_index_to_state,
                        node_idx,
                        eff_index,
                        LocalDirection::Recv,
                        label,
                        peer,
                        lane,
                    );
                }
                LocalAction::Local {
                    eff_index,
                    label,
                    lane,
                    ..
                } => {
                    Self::map_step_index_to_state(
                        &local_steps,
                        local_steps_len,
                        &eff_index_to_step,
                        &mut step_index_to_state,
                        node_idx,
                        eff_index,
                        LocalDirection::Local,
                        label,
                        0,
                        lane,
                    );
                }
                LocalAction::None | LocalAction::Terminate | LocalAction::Jump { .. } => {}
            }
            node_idx += 1;
        }

        let mut cursor = Self {
            typestate,
            idx: 0,
            phase_index: 0,
            lane_cursors: [0; MAX_LANES],
            label_lane_mask: [0; 256],
            phases,
            phase_len,
            local_steps,
            local_steps_len,
            eff_index_to_step,
            step_index_to_state,
        };
        cursor.rebuild_label_lane_mask();
        cursor
    }

    // =========================================================================
    // =========================================================================

    /// Get the current phase, if any.
    #[inline]
    pub(crate) fn current_phase(&self) -> Option<&Phase> {
        if self.phase_index < self.phase_len {
            Some(&self.phases[self.phase_index])
        } else {
            None
        }
    }

    // =========================================================================
    // Lane Access
    // =========================================================================

    fn current_label_for_lane(&self, lane_idx: usize) -> Option<u8> {
        self.step_at_lane(lane_idx).map(|step| step.label())
    }

    fn rebuild_label_lane_mask(&mut self) {
        self.label_lane_mask = [0; 256];
        let mut lane_idx = 0usize;
        while lane_idx < MAX_LANES {
            if let Some(label) = self.current_label_for_lane(lane_idx) {
                self.label_lane_mask[label as usize] |= 1u8 << (lane_idx as u32);
            }
            lane_idx += 1;
        }
    }

    fn update_label_lane_mask(
        &mut self,
        lane_idx: usize,
        old_label: Option<u8>,
        new_label: Option<u8>,
    ) {
        let bit = 1u8 << (lane_idx as u32);
        if let Some(label) = old_label {
            self.label_lane_mask[label as usize] &= !bit;
        }
        if let Some(label) = new_label {
            self.label_lane_mask[label as usize] |= bit;
        }
    }

    // =========================================================================
    // =========================================================================

    /// Find the lane that has a pending step with the given label.
    ///
    /// This is the core of Phase-driven execution: we use the label→lane mask
    /// for the current phase to resolve the lane without scanning.
    ///
    /// Returns `Some((lane_idx, step))` if found, `None` otherwise.
    pub(crate) fn find_step_for_label(&self, target_label: u8) -> Option<(usize, &LocalStep)> {
        let lane_mask = self.label_lane_mask[target_label as usize];
        if lane_mask == 0 {
            return None;
        }
        let lane_idx = lane_mask.trailing_zeros() as usize;
        let step = self.step_at_lane(lane_idx)?;
        if step.label() != target_label {
            debug_assert!(false, "label lane mask out of sync");
            return None;
        }
        Some((lane_idx, step))
    }

    /// Get the step at the current cursor position for a specific lane.
    pub(crate) fn step_at_lane(&self, lane_idx: usize) -> Option<&LocalStep> {
        let step_idx = self.step_index_at_lane(lane_idx)?;
        Some(&self.local_steps[step_idx])
    }

    /// Get the step index at the current cursor position for a specific lane.
    pub(crate) fn step_index_at_lane(&self, lane_idx: usize) -> Option<usize> {
        let phase = self.current_phase()?;

        if lane_idx >= MAX_LANES {
            return None;
        }

        let lane_steps = &phase.lanes[lane_idx];
        if !lane_steps.is_active() {
            return None;
        }

        let cursor_pos = self.lane_cursors[lane_idx];
        let step_idx = lane_steps.start + cursor_pos;
        if cursor_pos >= lane_steps.len || step_idx >= self.local_steps_len {
            return None;
        }

        Some(step_idx)
    }

    pub(crate) fn index_for_lane_step(&self, lane_idx: usize) -> Option<usize> {
        let step_idx = self.step_index_at_lane(lane_idx)?;
        let state_idx = self.step_index_to_state[step_idx];
        if state_idx == PHASE_CURSOR_NO_STATE {
            debug_assert!(
                false,
                "missing typestate index for lane step idx={}",
                step_idx
            );
            return None;
        }
        Some(state_idx.as_usize())
    }

    // =========================================================================
    // =========================================================================

    /// Set cursor for a specific lane to the step matching `eff_index`.
    ///
    /// Unlike `advance_lane_to_eff_index`, this positions the lane cursor at the
    /// step itself (not past it). Used for loop rewinds.
    pub(crate) fn set_lane_cursor_to_eff_index(&mut self, lane_idx: usize, eff_index: EffIndex) {
        let Some(phase) = self.current_phase() else {
            return;
        };
        if lane_idx >= MAX_LANES {
            return;
        }
        let lane_steps = &phase.lanes[lane_idx];
        if !lane_steps.is_active() {
            return;
        }
        let eff_idx = eff_index.as_usize();
        if eff_idx >= PHASE_CURSOR_MAX_STEPS {
            debug_assert!(false, "eff_index out of bounds for phase cursor");
            return;
        }
        let step_idx = self.eff_index_to_step[eff_idx];
        if step_idx == PHASE_CURSOR_NO_STEP {
            debug_assert!(false, "eff_index not found in local steps");
            return;
        }
        let step_idx = step_idx as usize;
        if step_idx >= self.local_steps_len {
            debug_assert!(false, "step index out of bounds for local steps");
            return;
        }
        let start = lane_steps.start;
        let end = start.saturating_add(lane_steps.len);
        if step_idx < start || step_idx >= end {
            debug_assert!(
                false,
                "eff_index not in current lane scope: eff_index={} lane={}",
                eff_index, lane_idx
            );
            return;
        }
        let old_label = self.current_label_for_lane(lane_idx);
        let target = step_idx.saturating_sub(start);
        self.lane_cursors[lane_idx] = target;
        let new_label = self.current_label_for_lane(lane_idx);
        self.update_label_lane_mask(lane_idx, old_label, new_label);
    }

    /// Advance cursor for a specific lane to the step matching `eff_index`.
    pub(crate) fn advance_lane_to_eff_index(&mut self, lane_idx: usize, eff_index: EffIndex) {
        let Some(phase) = self.current_phase() else {
            return;
        };
        if lane_idx >= MAX_LANES {
            return;
        }
        let lane_steps = &phase.lanes[lane_idx];
        if !lane_steps.is_active() {
            return;
        }
        let eff_idx = eff_index.as_usize();
        if eff_idx >= PHASE_CURSOR_MAX_STEPS {
            debug_assert!(false, "eff_index out of bounds for phase cursor");
            return;
        }
        let step_idx = self.eff_index_to_step[eff_idx];
        if step_idx == PHASE_CURSOR_NO_STEP {
            debug_assert!(false, "eff_index not found in local steps");
            return;
        }
        let step_idx = step_idx as usize;
        if step_idx >= self.local_steps_len {
            debug_assert!(false, "step index out of bounds for local steps");
            return;
        }
        let start = lane_steps.start;
        let end = start.saturating_add(lane_steps.len);
        if step_idx < start || step_idx >= end {
            debug_assert!(
                false,
                "eff_index not in current lane scope: eff_index={} lane={}",
                eff_index, lane_idx
            );
            return;
        }
        let target = step_idx.saturating_sub(start) + 1;
        if target > self.lane_cursors[lane_idx] {
            let old_label = self.current_label_for_lane(lane_idx);
            self.lane_cursors[lane_idx] = target;
            let new_label = self.current_label_for_lane(lane_idx);
            self.update_label_lane_mask(lane_idx, old_label, new_label);
        }
    }

    fn map_step_index_to_state(
        local_steps: &[LocalStep; PHASE_CURSOR_MAX_STEPS],
        local_steps_len: usize,
        eff_index_to_step: &[u16; PHASE_CURSOR_MAX_STEPS],
        step_index_to_state: &mut [StateIndex; PHASE_CURSOR_MAX_STEPS],
        node_idx: usize,
        eff_index: EffIndex,
        direction: LocalDirection,
        label: u8,
        peer: u8,
        lane: u8,
    ) {
        let eff_idx = eff_index.as_usize();
        if eff_idx >= PHASE_CURSOR_MAX_STEPS {
            debug_assert!(false, "eff_index out of bounds for typestate mapping");
            return;
        }
        let step_idx = eff_index_to_step[eff_idx];
        if step_idx == PHASE_CURSOR_NO_STEP {
            return;
        }
        let step_idx = step_idx as usize;
        if step_idx >= local_steps_len {
            debug_assert!(false, "step index out of bounds for typestate mapping");
            return;
        }
        let step = local_steps[step_idx];
        let matches = match direction {
            LocalDirection::Send => {
                step.is_send()
                    && step.label() == label
                    && step.peer() == peer
                    && step.lane() == lane
            }
            LocalDirection::Recv => {
                step.is_recv()
                    && step.label() == label
                    && step.peer() == peer
                    && step.lane() == lane
            }
            LocalDirection::Local => {
                step.is_local_action() && step.label() == label && step.lane() == lane
            }
            LocalDirection::None => false,
        };
        if !matches {
            debug_assert!(false, "typestate mapping mismatch for eff_index");
            return;
        }
        if step_index_to_state[step_idx] == PHASE_CURSOR_NO_STATE {
            debug_assert!(node_idx <= u16::MAX as usize, "typestate index overflow");
            step_index_to_state[step_idx] = as_state_index(node_idx);
        } else {
            debug_assert!(
                step_index_to_state[step_idx] == as_state_index(node_idx),
                "duplicate typestate mapping for step index"
            );
        }
    }

    /// Advance to next phase without syncing the primary typestate index.
    #[inline]
    pub(crate) fn advance_phase_without_sync(&mut self) {
        self.phase_index += 1;
        self.lane_cursors = [0; MAX_LANES];
        self.rebuild_label_lane_mask();
    }

    pub(crate) fn sync_idx_to_phase_start(&mut self) {
        let Some(phase) = self.current_phase() else {
            return;
        };
        if phase.lane_mask == 0 {
            return;
        };
        let step_idx = phase.min_start;
        if step_idx >= self.local_steps_len {
            debug_assert!(false, "phase start out of local steps range");
            return;
        }
        let state_idx = self.step_index_to_state[step_idx];
        if state_idx == PHASE_CURSOR_NO_STATE {
            debug_assert!(false, "missing typestate index for phase start step");
            return;
        }
        self.idx = state_idx.as_usize();
    }

    /// Check if all lanes in current phase are complete.
    pub(crate) fn is_phase_complete(&self) -> bool {
        let Some(phase) = self.current_phase() else {
            return true; // No more phases
        };

        let mut lane_mask = phase.lane_mask;
        while lane_mask != 0 {
            let lane_idx = lane_mask.trailing_zeros() as usize;
            lane_mask &= lane_mask - 1;
            let lane_steps = &phase.lanes[lane_idx];
            if self.lane_cursors[lane_idx] < lane_steps.len {
                return false;
            }
        }
        true
    }

    // =========================================================================
    // Core Typestate Navigation
    // =========================================================================

    /// Current typestate index.
    #[inline(always)]
    pub(crate) const fn index(&self) -> usize {
        self.idx
    }

    /// Access a typestate node by index.
    #[inline(always)]
    pub(crate) fn typestate_node(&self, index: usize) -> LocalNode {
        self.typestate.node(index)
    }

    #[inline(always)]
    fn action(&self) -> LocalAction {
        self.typestate.node(self.idx).action()
    }

    /// Returns `true` when the cursor points at a send action.
    #[inline(always)]
    pub(crate) fn is_send(&self) -> bool {
        self.action().is_send()
    }

    /// Returns `true` when the cursor points at a receive action.
    #[inline(always)]
    pub(crate) fn is_recv(&self) -> bool {
        self.action().is_recv()
    }

    /// Returns `true` when the cursor points at a local action.
    #[inline(always)]
    pub(crate) fn is_local_action(&self) -> bool {
        self.action().is_local_action()
    }

    /// Returns `true` when the cursor points at a Jump action.
    #[inline(always)]
    pub(crate) fn is_jump(&self) -> bool {
        self.action().is_jump()
    }

    /// Returns the jump reason if the current node is a Jump action.
    #[inline(always)]
    pub(crate) fn jump_reason(&self) -> Option<JumpReason> {
        self.action().jump_reason()
    }

    /// Returns the jump target index if the current node is a Jump action.
    #[inline(always)]
    pub(crate) fn jump_target(&self) -> Option<usize> {
        if self.is_jump() {
            Some(state_index_to_usize(self.typestate.node(self.idx).next()))
        } else {
            None
        }
    }

    /// Returns the label associated with the current typestate node.
    #[inline(always)]
    pub(crate) fn label(&self) -> Option<u8> {
        match self.action() {
            LocalAction::Send { label, .. }
            | LocalAction::Recv { label, .. }
            | LocalAction::Local { label, .. } => Some(label),
            LocalAction::None | LocalAction::Terminate | LocalAction::Jump { .. } => None,
        }
    }

    /// Advance typestate index to the successor.
    #[inline(always)]
    pub(crate) fn advance(self) -> Self {
        let next = state_index_to_usize(self.typestate.node(self.idx).next());
        Self { idx: next, ..self }
    }

    /// Follow Jump nodes until reaching a non-Jump or PassiveObserverBranch.
    ///
    /// Jump nodes are control flow instructions that redirect the cursor to
    /// their target (stored in the `next` field). This method follows the
    /// chain of Jump nodes until reaching a non-Jump node.
    ///
    /// **Decision point**: Only `PassiveObserverBranch` Jumps are NOT followed
    /// automatically. The passive observer must use `offer()` to determine
    /// which arm was selected before the Jump can be followed.
    ///
    /// **Auto-followed Jumps**:
    /// - `LoopContinue`: Returns cursor to loop_start for next iteration
    /// - `LoopBreak`: Exits the loop scope to terminal
    /// - `RouteArmEnd`: Exits the route arm to scope_end
    ///
    /// Returns `Err(JumpError)` if the Jump chain exceeds MAX_EFF_NODES iterations,
    /// indicating a CFG cycle bug in the typestate compiler.
    #[inline(always)]
    pub(crate) fn try_follow_jumps(self) -> Result<Self, JumpError> {
        let mut cursor = self;
        let mut iter = 0u32;
        while cursor.is_jump() {
            match cursor.action().jump_reason() {
                Some(JumpReason::PassiveObserverBranch) => {
                    // Decision point: stop for offer() to handle arm selection.
                    // Even when an arm is τ-eliminated, the decision is still required.
                    return Ok(cursor);
                }
                _ => {
                    // Follow all other Jump nodes (LoopContinue, LoopBreak, RouteArmEnd)
                    cursor = cursor.advance();
                    iter += 1;
                    if iter > crate::eff::meta::MAX_EFF_NODES as u32 {
                        return Err(JumpError {
                            iterations: iter,
                            idx: cursor.idx,
                        });
                    }
                }
            }
        }
        Ok(cursor)
    }

    /// Advance to the next node, then follow Jump nodes.
    ///
    /// Returns `Err(JumpError)` if the Jump chain exceeds MAX_EFF_NODES iterations.
    #[inline(always)]
    pub(crate) fn try_advance_past_jumps(self) -> Result<Self, JumpError> {
        self.advance().try_follow_jumps()
    }

    /// Follow a PassiveObserverBranch Jump to the specified arm's target.
    ///
    /// Uses O(1) registry lookup to find the PassiveObserverBranch Jump for the
    /// specified arm, then follows it to the target node.
    ///
    /// Returns `None` if:
    /// - Not in a scope
    /// - No PassiveObserverBranch Jump found for the specified arm
    pub(crate) fn follow_passive_observer_arm(
        &self,
        target_arm: u8,
    ) -> Option<PassiveArmNavigation> {
        let scope_region = self.scope_region()?;
        self.follow_passive_observer_arm_for_scope(scope_region.scope_id, target_arm)
    }

    /// Follow a PassiveObserverBranch Jump to the specified arm's target for a given scope.
    ///
    /// Unlike `follow_passive_observer_arm()`, this takes an explicit `scope_id` parameter
    /// instead of deriving it from the cursor's current node. Use this when you already
    /// know the scope (e.g., in `offer()` after scope decision).
    ///
    /// Returns `PassiveArmNavigation::WithinArm` containing the arm entry index.
    /// For τ-eliminated arms (no cross-role content), returns the ArmEmpty placeholder.
    ///
    /// Navigation priority:
    /// 1. PassiveObserverBranch Jump (if available) - follows the jump to arm entry
    /// 2. passive_arm_entry (direct entry index)
    ///
    /// The direct entry path is needed for nested routes where the inner route may have
    /// controller_arm_entry set (causing PassiveObserverBranch generation to skip),
    /// but passive_arm_entry is still valid for navigation.
    ///
    /// Note: τ-eliminated arms (no cross-role content) are handled at compile time
    /// by generating ArmEmpty (RouteArmEnd) placeholder nodes, ensuring
    /// passive_arm_entry is always set.
    pub(crate) fn follow_passive_observer_arm_for_scope(
        &self,
        scope_id: ScopeId,
        target_arm: u8,
    ) -> Option<PassiveArmNavigation> {
        // O(1) registry lookup for the PassiveObserverBranch Jump node index
        let jump_node_idx = self.typestate.passive_arm_jump(scope_id, target_arm);

        if let Some(jump_idx) = jump_node_idx {
            // Primary path: follow PassiveObserverBranch Jump to target
            let jump_node = self.typestate.node(state_index_to_usize(jump_idx));
            let target = jump_node.next();
            Some(PassiveArmNavigation::WithinArm { entry: target })
        } else if let Some(entry_idx) = self.typestate.passive_arm_entry(scope_id, target_arm) {
            // Secondary path: use passive_arm_entry directly
            // This is needed for nested routes where the inner route may be incorrectly
            // classified as "controller" (due to some nodes having controller_arm_entry set),
            // preventing PassiveObserverBranch generation. However, passive_arm_entry is
            // still correctly tracking the first cross-role node of each arm.
            //
            // For τ-eliminated arms, passive_arm_entry points to the ArmEmpty
            // (RouteArmEnd) placeholder generated at compile time.
            Some(PassiveArmNavigation::WithinArm { entry: entry_idx })
        } else {
            // No valid arm entry found - this should not happen with CFG-pure design.
            // All arms (including τ-eliminated) should have passive_arm_entry set.
            None
        }
    }

    /// Find the route arm containing a Send/Local node with the specified label.
    ///
    /// Uses O(1) registry lookup via `passive_arm_jump()` or `passive_arm_entry()`
    /// to check each arm's entry point label, avoiding full scope scan.
    ///
    /// For 2-arm routes, this performs at most 2 registry lookups + 2 node reads.
    pub(crate) fn find_arm_for_send_label(&self, target_label: u8) -> Option<u8> {
        let scope_region = self.scope_region()?;
        let scope_id = scope_region.scope_id;

        // O(1) per arm: check arm entry node labels
        // 2-arm route constraint means at most 2 iterations
        for arm in 0..2u8 {
            // First try PassiveObserverBranch Jump (for linger routes)
            let entry_idx =
                if let Some(jump_node_idx) = self.typestate.passive_arm_jump(scope_id, arm) {
                    let jump_node = self.typestate.node(state_index_to_usize(jump_node_idx));
                    Some(state_index_to_usize(jump_node.next()))
                } else {
                    // Direct entry path for non-linger routes.
                    self.typestate
                        .passive_arm_entry(scope_id, arm)
                        .map(state_index_to_usize)
                };

            if let Some(target_idx) = entry_idx {
                let entry_node = self.typestate.node(target_idx);

                // Check arm entry node's label
                match entry_node.action() {
                    LocalAction::Send { label, .. } | LocalAction::Local { label, .. }
                        if label == target_label =>
                    {
                        return Some(arm);
                    }
                    _ => {}
                }
            }
        }
        None
    }

    /// Find the route arm containing a Recv node with the specified label.
    ///
    /// Uses FIRST-recv dispatch for O(1) lookup. The dispatch table now includes
    /// the arm directly as `(label, arm, target_idx)`, eliminating positional inference.
    ///
    /// Returns `None` if label not found in dispatch table.
    ///
    /// Direct entry via passive_arm_entry is only needed for τ-eliminated arms or
    /// arms with no recv nodes (which have no FIRST entries).
    #[cfg(test)]
    pub(crate) fn find_arm_for_recv_label(&self, target_label: u8) -> Option<u8> {
        let scope_region = self.scope_region()?;
        let scope_id = scope_region.scope_id;

        // FIRST-recv dispatch: O(1) lookup returns (arm, target_idx) directly.
        // The arm is now stored in the dispatch table, eliminating positional inference.
        if let Some((arm, _target_idx)) = self.typestate.first_recv_target(scope_id, target_label) {
            if arm == ARM_SHARED {
                return Some(0);
            }
            return Some(arm);
        }

        // Bounded O(4) scan of arm entry node labels for τ-eliminated or local-only arms.
        for arm in 0..2u8 {
            let entry_idx =
                if let Some(jump_node_idx) = self.typestate.passive_arm_jump(scope_id, arm) {
                    let jump_node = self.typestate.node(state_index_to_usize(jump_node_idx));
                    Some(state_index_to_usize(jump_node.next()))
                } else {
                    self.typestate
                        .passive_arm_entry(scope_id, arm)
                        .map(state_index_to_usize)
                };

            if let Some(target_idx) = entry_idx {
                let entry_node = self.typestate.node(target_idx);
                if let LocalAction::Recv { label, .. } = entry_node.action() {
                    if label == target_label {
                        return Some(arm);
                    }
                }
            }
        }
        None
    }

    /// Follow a PassiveObserverBranch to the arm containing the specified label.
    ///
    /// This combines `find_arm_for_send_label` and `follow_passive_observer_arm`
    /// to directly navigate to the correct position for a passive observer.
    pub(crate) fn follow_passive_observer_for_label(&self, label: u8) -> Option<Self> {
        let target_arm = self.find_arm_for_send_label(label)?;
        let PassiveArmNavigation::WithinArm { entry } =
            self.follow_passive_observer_arm(target_arm)?;
        Some(self.with_index(state_index_to_usize(entry)))
    }

    /// Create a cursor at a specific typestate index.
    pub(crate) fn with_index(&self, idx: usize) -> Self {
        debug_assert!(idx < self.typestate.len());
        Self { idx, ..*self }
    }

    // =========================================================================
    // Scope Queries (delegated to typestate)
    // =========================================================================

    /// Get scope region for current node.
    pub(crate) fn scope_region(&self) -> Option<ScopeRegion> {
        let scope_id = self.typestate.node(self.idx).scope();
        if scope_id.is_none() {
            None
        } else {
            self.typestate.scope_region_for(scope_id)
        }
    }

    /// Get scope region by scope ID.
    #[inline(always)]
    pub(crate) fn scope_region_by_id(&self, scope_id: ScopeId) -> Option<ScopeRegion> {
        self.typestate.scope_region_for(scope_id)
    }

    /// FIRST-recv dispatch lookup for passive observers.
    ///
    /// Given a recv label, returns the route arm and leaf recv StateIndex.
    /// Returns `(arm, target_idx)` for O(1) dispatch without extra inference.
    ///
    /// Returns `None` if label not found.
    #[inline]
    pub(crate) fn first_recv_target(
        &self,
        scope_id: ScopeId,
        label: u8,
    ) -> Option<(u8, StateIndex)> {
        if let Some((policy, _, _)) = self.route_scope_controller_policy(scope_id)
            && policy.is_dynamic()
        {
            return None;
        }
        self.typestate.first_recv_target(scope_id, label)
    }

    #[inline]
    pub(crate) fn first_recv_target_evidence(
        &self,
        scope_id: ScopeId,
        label: u8,
    ) -> Option<(u8, StateIndex)> {
        self.typestate.first_recv_target(scope_id, label)
    }

    /// Check if this role is the controller for the given route scope.
    ///
    /// Uses type-level `controller_role` from `ScopeRegion` (propagated from
    /// binary route construction via `ScopeMarker`). This eliminates runtime
    /// inference based on `controller_arm_entry` presence.
    ///
    /// Returns `true` if `controller_role == ROLE`, `false` otherwise.
    #[inline]
    pub(crate) fn is_route_controller(&self, scope_id: ScopeId) -> bool {
        self.scope_region_by_id(scope_id)
            .and_then(|region| region.controller_role)
            .map_or(false, |ctrl| ctrl == ROLE)
    }

    /// Get scope ID at current position.
    #[cfg(test)]
    pub(crate) fn scope_id(&self) -> Option<ScopeId> {
        self.scope_region().map(|region| region.scope_id)
    }

    /// Scope ID stored on the current node (no parent traversal).
    #[inline(always)]
    pub(crate) fn node_scope_id(&self) -> ScopeId {
        self.typestate.node(self.idx).scope()
    }

    /// Get scope kind at current position.
    #[cfg(test)]
    pub(crate) fn scope_kind(&self) -> Option<ScopeKind> {
        self.scope_region().map(|region| region.kind)
    }

    /// Advance past the current scope if it matches the given kind.
    pub(crate) fn advance_scope_if_kind(&self, kind: ScopeKind) -> Option<Self> {
        let region = self.scope_region()?;
        if region.kind == kind {
            Some(self.with_index(region.end))
        } else {
            None
        }
    }

    /// Advance past a scope by ID.
    ///
    /// If cursor is already at or beyond scope.end, returns None since no
    /// advancement is needed (cursor has already exited the scope).
    pub(crate) fn advance_scope_by_id(&self, scope_id: ScopeId) -> Option<Self> {
        let region = self.scope_region_by_id(scope_id)?;
        // Only advance if cursor is still inside the scope
        if self.idx < region.end {
            Some(self.with_index(region.end))
        } else {
            // Cursor already at or beyond scope.end - no advancement needed
            None
        }
    }

    /// Get parent scope.
    pub(crate) fn scope_parent(&self, scope_id: ScopeId) -> Option<ScopeId> {
        self.typestate.scope_parent(scope_id)
    }

    // =========================================================================
    // Label Seeking
    // =========================================================================

    /// Find cursor at node with given label.
    #[cfg(test)]
    #[inline(always)]
    pub(crate) fn seek_label(&self, label: u8) -> Option<Self> {
        self.try_index_for_label(label)
            .map(|idx| self.with_index(idx))
    }

    /// Position after advancing from node with label.
    pub(crate) fn successor_for_label(&self, label: u8) -> Self {
        self.with_index(self.index_for_label(label)).advance()
    }

    /// Find index for label (panics if not found).
    pub(crate) fn index_for_label(&self, label: u8) -> usize {
        self.try_index_for_label(label)
            .expect("label not found in typestate")
    }

    /// Try to find index for label.
    pub(crate) fn try_index_for_label(&self, label: u8) -> Option<usize> {
        for i in 0..self.typestate.len() {
            let node = self.typestate.node(i);
            let node_label = match node.action() {
                LocalAction::Send { label: l, .. }
                | LocalAction::Recv { label: l, .. }
                | LocalAction::Local { label: l, .. } => Some(l),
                LocalAction::None | LocalAction::Terminate | LocalAction::Jump { .. } => None,
            };
            if node_label == Some(label) {
                return Some(i);
            }
        }
        None
    }

    // =========================================================================
    // Route Scope Methods
    // =========================================================================

    /// Get recv node index for a route arm.
    pub(crate) fn route_scope_arm_recv_index(
        &self,
        scope_id: ScopeId,
        target_arm: u8,
    ) -> Option<usize> {
        let state = self
            .typestate
            .scope_registry
            .route_recv_state(scope_id, target_arm)?;
        Some(state_index_to_usize(state))
    }

    /// Get arm count for a route scope.
    pub(crate) fn route_scope_arm_count(&self, scope_id: ScopeId) -> Option<u8> {
        self.typestate
            .scope_registry
            .route_arm_count(scope_id)
            .map(|count| count as u8)
    }

    /// Get offer lanes list for a route scope.
    /// Returns the lane list and its length for the first recv nodes in the scope.
    pub(crate) fn route_scope_offer_lane_list(
        &self,
        scope_id: ScopeId,
    ) -> Option<([u8; MAX_LANES], usize)> {
        self.typestate
            .scope_registry
            .route_offer_lane_list(scope_id)
    }

    /// Get offer entry index for a route scope.
    /// u16::MAX indicates the entry check is disabled (e.g., linger routes).
    pub(crate) fn route_scope_offer_entry(&self, scope_id: ScopeId) -> Option<StateIndex> {
        self.typestate.scope_registry.route_offer_entry(scope_id)
    }

    #[inline]
    pub(crate) fn route_scope_first_recv_dispatch_entry(
        &self,
        scope_id: ScopeId,
        idx: usize,
    ) -> Option<(u8, u8, StateIndex)> {
        self.typestate
            .scope_registry
            .first_recv_dispatch_entry(scope_id, idx)
    }

    #[inline]
    pub(crate) fn route_scope_slot(&self, scope_id: ScopeId) -> Option<usize> {
        self.typestate.scope_registry.route_scope_slot(scope_id)
    }

    pub(crate) fn scope_lane_first_eff(&self, scope_id: ScopeId, lane: u8) -> Option<EffIndex> {
        self.typestate
            .scope_registry
            .scope_lane_first_eff(scope_id, lane)
    }

    pub(crate) fn scope_lane_last_eff(&self, scope_id: ScopeId, lane: u8) -> Option<EffIndex> {
        self.typestate
            .scope_registry
            .scope_lane_last_eff(scope_id, lane)
    }

    pub(crate) fn scope_lane_last_eff_for_arm(
        &self,
        scope_id: ScopeId,
        arm: u8,
        lane: u8,
    ) -> Option<EffIndex> {
        self.typestate
            .scope_registry
            .scope_lane_last_eff_for_arm(scope_id, arm, lane)
    }

    /// Get the controller arm entry index for a given label.
    /// Returns the StateIndex of the arm whose label matches, used by flow() for O(1) lookup.
    pub(crate) fn controller_arm_entry_for_label(
        &self,
        scope_id: ScopeId,
        label: u8,
    ) -> Option<StateIndex> {
        self.typestate
            .scope_registry
            .controller_arm_entry_for_label(scope_id, label)
    }

    /// Check if the cursor is at a controller arm entry for the given scope.
    /// Used by flow() to determine if arm repositioning is valid.
    pub(crate) fn is_at_controller_arm_entry(&self, scope_id: ScopeId) -> bool {
        self.typestate
            .scope_registry
            .is_at_controller_arm_entry(scope_id, as_state_index(self.idx))
    }

    /// Get the controller arm entry (index, label) for a given arm number.
    /// Used by offer() to navigate to the selected arm's entry point.
    pub(crate) fn controller_arm_entry_by_arm(
        &self,
        scope_id: ScopeId,
        arm: u8,
    ) -> Option<(StateIndex, u8)> {
        self.typestate
            .scope_registry
            .controller_arm_entry_by_arm(scope_id, arm)
    }

    #[inline]
    pub(crate) fn passive_arm_scope_by_arm(&self, scope_id: ScopeId, arm: u8) -> Option<ScopeId> {
        self.typestate.passive_arm_scope(scope_id, arm)
    }

    /// Get route controller policy metadata.
    ///
    /// The tuple `(PolicyMode, EffIndex, u8)` corresponds to the controller-provided
    /// policy mode, the effect index of the send action that declared it, and the
    /// control resource tag embedded in the DSL. Route policies are tracked for both
    /// generic route decisions and loop-based routing (LoopContinue/LoopBreak).
    pub(crate) fn route_scope_controller_policy(
        &self,
        scope_id: ScopeId,
    ) -> Option<(PolicyMode, EffIndex, u8)> {
        self.typestate.scope_registry.route_controller(scope_id)
    }

    // =========================================================================
    // Metadata Extraction
    // =========================================================================

    /// Try to get send metadata at the current cursor location.
    /// Returns `None` if the current node is not a Send action.
    pub(crate) fn try_send_meta(&self) -> Option<SendMeta> {
        try_send_meta(&self.typestate, self.idx)
    }

    /// Try to get receive metadata at the current cursor location.
    /// Returns `None` if the current node is not a Recv action.
    pub(crate) fn try_recv_meta(&self) -> Option<RecvMeta> {
        try_recv_meta(&self.typestate, self.idx)
    }

    /// Try to get local action metadata at the current cursor location.
    /// Returns `None` if the current node is not a Local action.
    pub(crate) fn try_local_meta(&self) -> Option<LocalMeta> {
        try_local_meta(&self.typestate, self.idx)
    }

    // =========================================================================
    // Loop Metadata
    // =========================================================================

    /// Get loop metadata for current scope.
    pub(crate) fn loop_metadata_inner(&self) -> Option<LoopMetadata<ROLE>> {
        let node = self.typestate.node(self.idx);
        let action = node.action();
        let (label, eff_index, controller, target, role_kind) = match action {
            LocalAction::Send {
                label,
                eff_index,
                peer,
                ..
            } => (label, eff_index, ROLE, peer, LoopRole::Controller),
            LocalAction::Recv {
                label,
                eff_index,
                peer,
                ..
            } => (label, eff_index, peer, ROLE, LoopRole::Target),
            _ => return None,
        };
        if label != LABEL_LOOP_CONTINUE {
            return None;
        }
        let scope = node.loop_scope()?;
        let continue_index = self.successor_for_label(LABEL_LOOP_CONTINUE).index();
        let break_index = self.successor_for_label(LABEL_LOOP_BREAK).index();
        Some(LoopMetadata {
            scope,
            controller,
            target,
            role: role_kind,
            eff_index,
            decision_index: self.idx,
            continue_index,
            break_index,
        })
    }

    // =========================================================================
    // Terminal State
    // =========================================================================

    /// Assert that the cursor is at a terminal state.
    #[cfg(test)]
    pub(crate) fn assert_terminal(&self) {
        assert!(
            matches!(self.action(), LocalAction::Terminate),
            "cursor at index {} is not terminal",
            self.idx
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{LocalAction, PhaseCursor, StateIndex};
    use crate::control::cap::mint::GenericCapToken;
    use crate::control::cap::resource_kinds::{LoopBreakKind, LoopContinueKind};
    use crate::eff::EffIndex;
    use crate::g::{self, Msg, Role};
    use crate::global::const_dsl::{PolicyMode, ScopeKind};
    use crate::global::role_program;
    use crate::global::role_program::{RoleProgram, project};
    use crate::global::steps::{LoopSteps, ProjectRole, SendStep, StepConcat, StepCons, StepNil};
    use crate::global::{CanonicalControl, MessageSpec};
    use crate::runtime::consts::{LABEL_LOOP_BREAK, LABEL_LOOP_CONTINUE};

    const BODY: g::Program<StepCons<SendStep<Role<0>, Role<1>, Msg<7, ()>>, StepNil>> =
        g::send::<Role<0>, Role<1>, Msg<7, ()>, 0>();

    const LOOP_POLICY_ID: u16 = 9300;
    const ROUTE_POLICY_ID: u16 = 9301;

    #[allow(clippy::type_complexity)]
    const LOOP_PROGRAM: g::Program<
        LoopSteps<
            StepCons<SendStep<Role<0>, Role<1>, Msg<7, ()>>, StepNil>,
            Role<0>,
            Msg<
                { LABEL_LOOP_CONTINUE },
                GenericCapToken<LoopContinueKind>,
                CanonicalControl<LoopContinueKind>,
            >,
            Msg<
                { LABEL_LOOP_BREAK },
                GenericCapToken<LoopBreakKind>,
                CanonicalControl<LoopBreakKind>,
            >,
            StepNil,
        >,
    > = {
        // Self-send for CanonicalControl: Controller → Controller
        let continue_control = g::send::<
            Role<0>,
            Role<0>, // self-send
            Msg<
                { LABEL_LOOP_CONTINUE },
                GenericCapToken<LoopContinueKind>,
                CanonicalControl<LoopContinueKind>,
            >,
            0,
        >()
        .policy::<LOOP_POLICY_ID>();
        let continue_arm = g::seq(continue_control, BODY);
        let break_arm = g::send::<
            Role<0>,
            Role<0>, // self-send
            Msg<
                { LABEL_LOOP_BREAK },
                GenericCapToken<LoopBreakKind>,
                CanonicalControl<LoopBreakKind>,
            >,
            0,
        >()
        .policy::<LOOP_POLICY_ID>();
        // Route decision is local to Controller (0 → 0)
        g::route(continue_arm, break_arm)
    };

    const CONTROLLER_PROGRAM: RoleProgram<
        'static,
        0,
        <LoopSteps<
            StepCons<SendStep<Role<0>, Role<1>, Msg<7, ()>>, StepNil>,
            Role<0>,
            Msg<
                { LABEL_LOOP_CONTINUE },
                GenericCapToken<LoopContinueKind>,
                CanonicalControl<LoopContinueKind>,
            >,
            Msg<
                { LABEL_LOOP_BREAK },
                GenericCapToken<LoopBreakKind>,
                CanonicalControl<LoopBreakKind>,
            >,
            StepNil,
        > as ProjectRole<Role<0>>>::Output,
    > = project(&LOOP_PROGRAM);

    const TARGET_PROGRAM: RoleProgram<
        'static,
        1,
        <LoopSteps<
            StepCons<SendStep<Role<0>, Role<1>, Msg<7, ()>>, StepNil>,
            Role<0>,
            Msg<
                { LABEL_LOOP_CONTINUE },
                GenericCapToken<LoopContinueKind>,
                CanonicalControl<LoopContinueKind>,
            >,
            Msg<
                { LABEL_LOOP_BREAK },
                GenericCapToken<LoopBreakKind>,
                CanonicalControl<LoopBreakKind>,
            >,
            StepNil,
        > as ProjectRole<Role<1>>>::Output,
    > = project(&LOOP_PROGRAM);

    const LOCAL_PROGRAM: g::Program<StepCons<SendStep<Role<0>, Role<0>, Msg<9, ()>>, StepNil>> =
        g::send::<Role<0>, Role<0>, Msg<9, ()>, 0>();
    const LOCAL_ROLE: role_program::RoleProgram<
        'static,
        0,
        <StepCons<SendStep<Role<0>, Role<0>, Msg<9, ()>>, StepNil> as ProjectRole<Role<0>>>::Output,
    > = role_program::project(&LOCAL_PROGRAM);

    #[test]
    fn state_cursor_rewinds_on_loop_continue() {
        let decision = PhaseCursor::new(&CONTROLLER_PROGRAM);
        let continue_branch = decision
            .seek_label(LABEL_LOOP_CONTINUE)
            .expect("continue branch available");
        assert_eq!(continue_branch.label(), Some(LABEL_LOOP_CONTINUE));

        let after_continue = continue_branch.advance();
        assert!(after_continue.is_send());
        assert_eq!(after_continue.label(), Some(7));

        let rewind = decision
            .seek_label(LABEL_LOOP_CONTINUE)
            .expect("continue branch rewinds");
        assert_eq!(rewind.label(), Some(LABEL_LOOP_CONTINUE));
    }

    #[test]
    fn state_cursor_loop_branch_successors() {
        let decision = PhaseCursor::new(&CONTROLLER_PROGRAM);
        assert_eq!(decision.scope_kind(), Some(ScopeKind::Route));
        let cont_cursor = decision
            .seek_label(LABEL_LOOP_CONTINUE)
            .expect("continue branch")
            .advance();
        let mut cont_cursor = cont_cursor;
        while cont_cursor.label() == Some(LABEL_LOOP_CONTINUE) {
            cont_cursor = cont_cursor.advance();
        }
        assert_eq!(cont_cursor.label(), Some(7));

        let break_branch = decision.seek_label(LABEL_LOOP_BREAK).expect("break branch");
        assert_eq!(break_branch.label(), Some(LABEL_LOOP_BREAK));
        // After advancing from Local(BREAK), we land on Jump(LoopBreak).
        // Follow the Jump to its target (terminal).
        let break_cursor = break_branch
            .advance()
            .try_follow_jumps()
            .expect("follow loop break jump");
        break_cursor.assert_terminal();

        // Target (Role<1>) only sees the LoopBody message (label 7), not the
        // LoopContinue/LoopBreak self-sends. With self-send CanonicalControl,
        // Target's projection contains only the actual cross-role messages,
        // plus PassiveObserverBranch Jump nodes for empty arms (Break arm in this case).
        let target_cursor = PhaseCursor::new(&TARGET_PROGRAM);
        // Target sees label 7 (the loop body recv) directly
        assert_eq!(target_cursor.label(), Some(7));

        let target_projection = TARGET_PROGRAM.projection();
        let ts = target_projection.typestate();
        let after_body = target_cursor.advance();
        // After advancing past the Recv, we encounter PassiveObserverBranch Jump nodes.
        // - Jump for arm 0 (Continue): loops back to loop_start
        // - Jump for arm 1 (Break): goes to scope_end (terminal)
        let cursor = after_body;

        // For a passive observer in a linger scope, the normal flow after Recv
        // is determined by which arm was selected. The arm 0 Jump loops back,
        // so we need to check that arm 1 (Break) properly terminates.
        assert!(
            cursor.is_jump(),
            "after Recv should be arm 0 PassiveObserverBranch Jump"
        );
        let jump_node = ts.nodes[cursor.idx as usize];
        // Arm 0 Jump targets loop start (idx 0)
        assert_eq!(
            jump_node.next(),
            StateIndex::ZERO,
            "arm 0 should jump to loop start"
        );

        // Advance past arm 0 Jump to find arm 1 Jump
        // Note: advance() on a Jump follows the target, so we need to check next node manually
        let arm1_idx = (cursor.idx + 1) as usize;
        if arm1_idx < ts.nodes.len() && !matches!(ts.nodes[arm1_idx].action(), LocalAction::None) {
            let arm1_node = ts.nodes[arm1_idx];
            if arm1_node.action().is_jump() {
                // Arm 1 (Break) Jump should target scope_end (which should be terminal)
                let arm1_target = arm1_node.next();
                let target_idx = arm1_target.as_usize();
                if target_idx < ts.nodes.len() {
                    let terminal_node = ts.nodes[target_idx];
                    assert!(
                        terminal_node.action().is_terminal(),
                        "arm 1 Break Jump should reach terminal"
                    );
                }
            }
        }

        // The test passes if we've verified the structure. The actual runtime
        // behavior uses offer() to select which arm, not linear advance().
        // For linger scopes with passive observers, both arms have Jump nodes.
    }

    #[test]
    fn route_scope_kind_detected() {
        // Route is local to Controller (0 → 0)
        const ROUTE: g::Program<
            <StepCons<
                SendStep<
                    Role<0>,
                    Role<0>,
                    Msg<
                        { LABEL_LOOP_CONTINUE },
                        GenericCapToken<LoopContinueKind>,
                        CanonicalControl<LoopContinueKind>,
                    >,
                >,
                StepNil,
            > as StepConcat<
                StepCons<
                    SendStep<
                        Role<0>,
                        Role<0>,
                        Msg<
                            { LABEL_LOOP_BREAK },
                            GenericCapToken<LoopBreakKind>,
                            CanonicalControl<LoopBreakKind>,
                        >,
                    >,
                    StepNil,
                >,
            >>::Output,
        > = g::route(
            g::send::<
                Role<0>,
                Role<0>,
                Msg<
                    { LABEL_LOOP_CONTINUE },
                    GenericCapToken<LoopContinueKind>,
                    CanonicalControl<LoopContinueKind>,
                >,
                0,
            >()
            .policy::<ROUTE_POLICY_ID>(),
            g::send::<
                Role<0>,
                Role<0>,
                Msg<
                    { LABEL_LOOP_BREAK },
                    GenericCapToken<LoopBreakKind>,
                    CanonicalControl<LoopBreakKind>,
                >,
                0,
            >()
            .policy::<ROUTE_POLICY_ID>(),
        );

        const CONTROLLER: RoleProgram<
            'static,
            0,
            <<StepCons<
                SendStep<
                    Role<0>,
                    Role<0>,
                    Msg<
                        { LABEL_LOOP_CONTINUE },
                        GenericCapToken<LoopContinueKind>,
                        CanonicalControl<LoopContinueKind>,
                    >,
                >,
                StepNil,
            > as StepConcat<
                StepCons<
                    SendStep<
                        Role<0>,
                        Role<0>,
                        Msg<
                            { LABEL_LOOP_BREAK },
                            GenericCapToken<LoopBreakKind>,
                            CanonicalControl<LoopBreakKind>,
                        >,
                    >,
                    StepNil,
                >,
            >>::Output as ProjectRole<Role<0>>>::Output,
        > = project(&ROUTE);

        let cursor = PhaseCursor::new(&CONTROLLER);
        assert_eq!(cursor.scope_kind(), Some(ScopeKind::Route));
        let scope_id = cursor.scope_id().expect("route scope id present");
        let (policy, eff_index, _) = cursor
            .route_scope_controller_policy(scope_id)
            .expect("controller policy recorded");
        let expected_policy = PolicyMode::dynamic(ROUTE_POLICY_ID).with_scope(scope_id);
        assert_eq!(policy, expected_policy);
        assert_ne!(eff_index, EffIndex::MAX);
    }

    #[test]
    fn local_action_produces_metadata() {
        let cursor = PhaseCursor::<0>::new(&LOCAL_ROLE);
        assert!(cursor.is_local_action());
        assert_eq!(cursor.label(), Some(<Msg<9, ()> as MessageSpec>::LABEL));
        let cursor = cursor.advance();
        cursor.assert_terminal();
    }
}
