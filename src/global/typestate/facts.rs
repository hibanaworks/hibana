//! Immutable typestate facts and metadata.

use super::builder::RoleTypestate;
use crate::{
    control::cap::mint::CapShot,
    eff::{self, EffIndex},
    global::const_dsl::{PolicyMode, ScopeId},
};

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

pub(in crate::global::typestate) const SCOPE_ORDINAL_INDEX_CAPACITY: usize =
    ScopeId::ORDINAL_CAPACITY as usize;
pub(in crate::global::typestate) const SCOPE_ORDINAL_INDEX_EMPTY: u16 = u16::MAX;

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

pub(crate) const fn as_eff_index(idx: usize) -> EffIndex {
    EffIndex::from_usize(idx)
}

pub(crate) const fn as_state_index(idx: usize) -> StateIndex {
    StateIndex::from_usize(idx)
}

pub(crate) const fn state_index_to_usize(index: StateIndex) -> usize {
    index.as_usize()
}
