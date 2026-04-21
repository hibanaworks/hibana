//! Immutable typestate facts and metadata.

use super::builder::RoleTypestateValue;
use crate::{
    control::cap::mint::CapShot,
    eff::{self, EffIndex},
    global::{
        compiled::images::ControlSemanticKind,
        const_dsl::{CompactScopeId, PolicyMode, ScopeId},
    },
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

const LOCAL_ACTION_STATIC_POLICY_ID: u16 = u16::MAX;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PackedLocalAction {
    Send {
        eff_index: EffIndex,
        peer: u8,
        label: u8,
        resource: Option<u8>,
        is_control: bool,
        shot: Option<CapShot>,
        policy_id: u16,
        lane: u8,
    },
    Recv {
        eff_index: EffIndex,
        peer: u8,
        label: u8,
        resource: Option<u8>,
        is_control: bool,
        shot: Option<CapShot>,
        policy_id: u16,
        lane: u8,
    },
    Local {
        eff_index: EffIndex,
        label: u8,
        resource: Option<u8>,
        is_control: bool,
        shot: Option<CapShot>,
        policy_id: u16,
        lane: u8,
    },
    Terminate,
    Jump {
        reason: JumpReason,
    },
}

#[inline(always)]
const fn encode_policy_id(policy: PolicyMode) -> u16 {
    match policy.dynamic_policy_id() {
        Some(policy_id) => policy_id,
        None => LOCAL_ACTION_STATIC_POLICY_ID,
    }
}

#[inline(always)]
const fn decode_policy(policy_id: u16, scope: CompactScopeId) -> PolicyMode {
    if policy_id == LOCAL_ACTION_STATIC_POLICY_ID {
        PolicyMode::Static
    } else {
        PolicyMode::Dynamic { policy_id, scope }
    }
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
    action: PackedLocalAction,
    next: StateIndex,
    scope: CompactScopeId,
    route_arm_raw: u8,
    /// Bit-packed node flags.
    /// FLAG_CHOICE_DETERMINANT marks the first recv of a route arm.
    flags: u8,
}

impl LocalNode {
    const ROUTE_ARM_NONE: u8 = u8::MAX;
    const FLAG_CHOICE_DETERMINANT: u8 = 1 << 0;
    const FLAG_SEMANTIC_SHIFT: u8 = 1;
    const FLAG_SEMANTIC_MASK: u8 = 0b11 << Self::FLAG_SEMANTIC_SHIFT;

    #[cfg(test)]
    #[inline(always)]
    pub(crate) const fn packed_action_size() -> usize {
        core::mem::size_of::<PackedLocalAction>()
    }

    #[inline(always)]
    const fn encode_route_arm(route_arm: Option<u8>) -> u8 {
        match route_arm {
            Some(arm) => arm,
            None => Self::ROUTE_ARM_NONE,
        }
    }

    #[inline(always)]
    const fn decode_route_arm(raw: u8) -> Option<u8> {
        if raw == Self::ROUTE_ARM_NONE {
            None
        } else {
            Some(raw)
        }
    }

    #[inline(always)]
    const fn encode_semantic(semantic: ControlSemanticKind) -> u8 {
        semantic.packed_bits() << Self::FLAG_SEMANTIC_SHIFT
    }

    #[inline(always)]
    const fn decode_semantic(flags: u8) -> ControlSemanticKind {
        ControlSemanticKind::from_packed_bits(
            (flags & Self::FLAG_SEMANTIC_MASK) >> Self::FLAG_SEMANTIC_SHIFT,
        )
    }

    #[inline(always)]
    const fn flags(is_choice_determinant: bool, semantic: ControlSemanticKind) -> u8 {
        let mut flags = Self::encode_semantic(semantic);
        if is_choice_determinant {
            flags |= Self::FLAG_CHOICE_DETERMINANT;
        }
        flags
    }

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
        semantic: ControlSemanticKind,
        next: StateIndex,
        scope: ScopeId,
        _loop_scope: Option<ScopeId>,
        route_arm: Option<u8>,
        is_choice_determinant: bool,
    ) -> Self {
        Self {
            action: PackedLocalAction::Send {
                eff_index,
                peer,
                label,
                resource,
                is_control,
                shot,
                policy_id: encode_policy_id(policy),
                lane,
            },
            next,
            scope: CompactScopeId::from_scope_id(scope),
            route_arm_raw: Self::encode_route_arm(route_arm),
            flags: Self::flags(is_choice_determinant, semantic),
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
        semantic: ControlSemanticKind,
        next: StateIndex,
        scope: ScopeId,
        _loop_scope: Option<ScopeId>,
        route_arm: Option<u8>,
        is_choice_determinant: bool,
    ) -> Self {
        Self {
            action: PackedLocalAction::Recv {
                eff_index,
                peer,
                label,
                resource,
                is_control,
                shot,
                policy_id: encode_policy_id(policy),
                lane,
            },
            next,
            scope: CompactScopeId::from_scope_id(scope),
            route_arm_raw: Self::encode_route_arm(route_arm),
            flags: Self::flags(is_choice_determinant, semantic),
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
        semantic: ControlSemanticKind,
        next: StateIndex,
        scope: ScopeId,
        _loop_scope: Option<ScopeId>,
        route_arm: Option<u8>,
        is_choice_determinant: bool,
    ) -> Self {
        Self {
            action: PackedLocalAction::Local {
                eff_index,
                label,
                resource,
                is_control,
                shot,
                policy_id: encode_policy_id(policy),
                lane,
            },
            next,
            scope: CompactScopeId::from_scope_id(scope),
            route_arm_raw: Self::encode_route_arm(route_arm),
            flags: Self::flags(is_choice_determinant, semantic),
        }
    }

    /// Construct a terminal node that loops to itself.
    pub(crate) const fn terminal(index: StateIndex) -> Self {
        Self {
            action: PackedLocalAction::Terminate,
            next: index,
            scope: CompactScopeId::none(),
            route_arm_raw: Self::ROUTE_ARM_NONE,
            flags: 0,
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
        _loop_scope: Option<ScopeId>,
        route_arm: Option<u8>,
    ) -> Self {
        Self {
            action: PackedLocalAction::Jump { reason },
            next: target,
            scope: CompactScopeId::from_scope_id(scope),
            route_arm_raw: Self::encode_route_arm(route_arm),
            flags: 0,
        }
    }

    /// Action associated with the node.
    #[inline(always)]
    pub(crate) const fn action(&self) -> LocalAction {
        match self.action {
            PackedLocalAction::Send {
                eff_index,
                peer,
                label,
                resource,
                is_control,
                shot,
                policy_id,
                lane,
            } => LocalAction::Send {
                eff_index,
                peer,
                label,
                resource,
                is_control,
                shot,
                policy: decode_policy(policy_id, self.scope),
                lane,
            },
            PackedLocalAction::Recv {
                eff_index,
                peer,
                label,
                resource,
                is_control,
                shot,
                policy_id,
                lane,
            } => LocalAction::Recv {
                eff_index,
                peer,
                label,
                resource,
                is_control,
                shot,
                policy: decode_policy(policy_id, self.scope),
                lane,
            },
            PackedLocalAction::Local {
                eff_index,
                label,
                resource,
                is_control,
                shot,
                policy_id,
                lane,
            } => LocalAction::Local {
                eff_index,
                label,
                resource,
                is_control,
                shot,
                policy: decode_policy(policy_id, self.scope),
                lane,
            },
            PackedLocalAction::Terminate => LocalAction::Terminate,
            PackedLocalAction::Jump { reason } => LocalAction::Jump { reason },
        }
    }

    /// Successor state reached after performing the action.
    #[inline(always)]
    pub(crate) const fn next(&self) -> StateIndex {
        self.next
    }

    #[inline(always)]
    pub(crate) const fn scope(&self) -> ScopeId {
        self.scope.to_scope_id()
    }

    #[inline(always)]
    pub(crate) const fn route_arm(&self) -> Option<u8> {
        Self::decode_route_arm(self.route_arm_raw)
    }

    /// Whether this node is a choice determinant (first recv of a route arm).
    #[inline(always)]
    pub(crate) const fn is_choice_determinant(&self) -> bool {
        (self.flags & Self::FLAG_CHOICE_DETERMINANT) != 0
    }

    #[inline(always)]
    pub(crate) const fn control_semantic(&self) -> ControlSemanticKind {
        Self::decode_semantic(self.flags)
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
        Self {
            scope: CompactScopeId::from_scope_id(scope),
            ..self
        }
    }

    #[inline(always)]
    pub(crate) const fn with_route_arm(self, route_arm: Option<u8>) -> Self {
        Self {
            route_arm_raw: Self::encode_route_arm(route_arm),
            ..self
        }
    }
}

/// Metadata for a send transition derived from typestate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SendMeta {
    pub eff_index: EffIndex,
    pub peer: u8,
    pub label: u8,
    pub resource: Option<u8>,
    pub semantic: ControlSemanticKind,
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
        semantic: ControlSemanticKind,
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
            semantic,
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
    pub semantic: ControlSemanticKind,
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
    pub semantic: ControlSemanticKind,
    pub is_control: bool,
    pub next: usize,
    pub scope: ScopeId,
    pub route_arm: Option<u8>,
    pub shot: Option<CapShot>,
    pub policy: PolicyMode,
    /// Type-level lane for parallel composition (default 0).
    pub lane: u8,
}

pub(crate) fn try_send_meta_value(ts: &RoleTypestateValue, idx: usize) -> Option<SendMeta> {
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
            node.control_semantic(),
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

pub(crate) fn try_recv_meta_value(ts: &RoleTypestateValue, idx: usize) -> Option<RecvMeta> {
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
            semantic: node.control_semantic(),
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

pub(crate) fn try_local_meta_value(ts: &RoleTypestateValue, idx: usize) -> Option<LocalMeta> {
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
            semantic: node.control_semantic(),
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
