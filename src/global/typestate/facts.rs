//! Immutable typestate facts and metadata.

use crate::{
    control::cap::mint::CapShot,
    eff::{self, EffIndex},
    global::{
        compiled::images::ControlSemanticKind,
        const_dsl::{CompactScopeId, PolicyMode, ScopeId, ScopeKind},
    },
};

mod meta;
pub(crate) use meta::{LocalMeta, RecvMeta, SendMeta, as_state_index, state_index_to_usize};

/// Route-arm marker used when a first-recv dispatch entry is shared by both
/// arms. It is a compiled descriptor fact, not runtime route authority.
pub(crate) const ARM_SHARED: u8 = 0xFF;

/// Role-local dependency row guarding an event.
///
/// This is a descriptor fact: the row says which local dependency scope must be
/// complete before the guarded event is enabled, plus the route conflict that
/// decides whether the dependency applies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LocalDependency {
    scope: ScopeId,
    conflict: LocalConflict,
}

impl LocalDependency {
    #[inline(always)]
    pub(crate) const fn with_conflict(scope: ScopeId, conflict: LocalConflict) -> Self {
        Self { scope, conflict }
    }

    #[inline(always)]
    pub(crate) const fn scope(self) -> ScopeId {
        self.scope
    }

    #[inline(always)]
    pub(crate) const fn conflict(self) -> LocalConflict {
        self.conflict
    }
}

/// Compact role-local dependency row stored beside local step lanes.
///
/// Dependency scopes are always parallel scopes. Route conflicts only need the
/// enclosing route ordinal plus the selected arm. Keeping this as one word
/// prevents the event-image row from growing into a full `ScopeId` pair.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PackedLocalDependency(u32);

impl PackedLocalDependency {
    const NONE: u32 = u32::MAX;
    const ORDINAL_BITS: u32 = 9;
    const ORDINAL_MASK: u32 = (1 << Self::ORDINAL_BITS) - 1;
    const DEP_SHIFT: u32 = 0;
    const CONFLICT_SHIFT: u32 = Self::DEP_SHIFT + Self::ORDINAL_BITS;
    const ROUTE_SHIFT: u32 = Self::CONFLICT_SHIFT + 2;
    const CONFLICT_UNCONDITIONAL: u32 = 0;
    const CONFLICT_SHARED_ROUTE: u32 = 1;
    const CONFLICT_ROUTE_ARM_0: u32 = 2;
    const CONFLICT_ROUTE_ARM_1: u32 = 3;

    #[inline(always)]
    pub(crate) const fn none() -> Self {
        Self(Self::NONE)
    }

    #[inline(always)]
    pub(crate) const fn from_dependency(dependency: LocalDependency) -> Self {
        let scope = dependency.scope();
        if scope.is_none() {
            return Self::none();
        }
        if !matches!(scope.kind(), ScopeKind::Parallel) {
            panic!("dependency row scope must be a parallel scope");
        }
        let dep_ordinal = scope.local_ordinal() as u32;
        if dep_ordinal > Self::ORDINAL_MASK {
            panic!("dependency scope ordinal overflow");
        }

        let (conflict_tag, route_ordinal) = match dependency.conflict() {
            LocalConflict::Unconditional => (Self::CONFLICT_UNCONDITIONAL, 0),
            LocalConflict::SharedRoute => (Self::CONFLICT_SHARED_ROUTE, 0),
            LocalConflict::RouteArm { scope, arm } => {
                if scope.is_none() || !matches!(scope.kind(), ScopeKind::Route) {
                    panic!("dependency route conflict scope must be a route scope");
                }
                let route_ordinal = scope.local_ordinal() as u32;
                if route_ordinal > Self::ORDINAL_MASK {
                    panic!("dependency route conflict ordinal overflow");
                }
                match arm {
                    0 => (Self::CONFLICT_ROUTE_ARM_0, route_ordinal),
                    1 => (Self::CONFLICT_ROUTE_ARM_1, route_ordinal),
                    _ => panic!("dependency route conflict arm overflow"),
                }
            }
        };

        Self(
            (dep_ordinal << Self::DEP_SHIFT)
                | (conflict_tag << Self::CONFLICT_SHIFT)
                | (route_ordinal << Self::ROUTE_SHIFT),
        )
    }

    #[inline(always)]
    pub(crate) const fn to_dependency(self) -> Option<LocalDependency> {
        if self.0 == Self::NONE {
            return None;
        }
        let dep_ordinal = ((self.0 >> Self::DEP_SHIFT) & Self::ORDINAL_MASK) as u16;
        let conflict_tag = (self.0 >> Self::CONFLICT_SHIFT) & 0b11;
        let route_ordinal = ((self.0 >> Self::ROUTE_SHIFT) & Self::ORDINAL_MASK) as u16;
        let scope = ScopeId::parallel(dep_ordinal);
        let conflict = match conflict_tag {
            Self::CONFLICT_UNCONDITIONAL => LocalConflict::Unconditional,
            Self::CONFLICT_SHARED_ROUTE => LocalConflict::SharedRoute,
            Self::CONFLICT_ROUTE_ARM_0 => LocalConflict::RouteArm {
                scope: ScopeId::route(route_ordinal),
                arm: 0,
            },
            Self::CONFLICT_ROUTE_ARM_1 => LocalConflict::RouteArm {
                scope: ScopeId::route(route_ordinal),
                arm: 1,
            },
            _ => LocalConflict::Unconditional,
        };
        Some(LocalDependency::with_conflict(scope, conflict))
    }
}

/// Packed role-local route membership for one event or route scope.
///
/// This is the production conflict row used by the event cursor. It records the
/// nearest enclosing route arm at projection time; parent route membership is
/// represented by the route scope's own conflict row, so runtime enabled checks
/// can walk conflict rows without interpreting scope topology.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PackedEventConflict(u16);

impl PackedEventConflict {
    const NONE: u16 = u16::MAX;
    const ARM_BITS: u16 = 1;
    const ROUTE_MASK: u16 = (1 << 13) - 1;
    /// Maximum row-chain length for conflict traversal.
    ///
    /// An event conflict row can only point through route-scope conflict rows
    /// derived from the same fixed-size local event image. The cursor uses this
    /// row capacity as its cycle guard instead of consulting runtime route
    /// topology counts.
    pub(crate) const MAX_CHAIN_DEPTH: usize = eff::meta::MAX_EFF_NODES + 1;

    #[inline(always)]
    pub(crate) const fn none() -> Self {
        Self(Self::NONE)
    }

    #[inline(always)]
    pub(crate) const fn route_arm(scope: ScopeId, arm: u8) -> Self {
        if scope.is_none() || !matches!(scope.kind(), ScopeKind::Route) {
            panic!("event conflict scope must be a route scope");
        }
        if arm > 1 {
            panic!("event conflict arm overflow");
        }
        let ordinal = scope.local_ordinal();
        if ordinal > Self::ROUTE_MASK {
            panic!("event conflict route ordinal overflow");
        }
        Self((ordinal << Self::ARM_BITS) | arm as u16)
    }

    #[inline(always)]
    pub(crate) const fn from_conflict(conflict: LocalConflict) -> Self {
        match conflict {
            LocalConflict::RouteArm { scope, arm } => Self::route_arm(scope, arm),
            LocalConflict::Unconditional | LocalConflict::SharedRoute => Self::none(),
        }
    }

    #[inline(always)]
    pub(crate) const fn to_conflict(self) -> Option<LocalConflict> {
        if self.0 == Self::NONE {
            return None;
        }
        let arm = (self.0 & 1) as u8;
        let ordinal = self.0 >> Self::ARM_BITS;
        Some(LocalConflict::RouteArm {
            scope: ScopeId::route(ordinal),
            arm,
        })
    }
}

/// Descriptor-derived route decision for a recvless passive parent route.
///
/// This fact identifies the parent route arm implied by a child route path. It
/// does not carry endpoint lane or transport authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RecvlessParentRouteDecision {
    scope: ScopeId,
    arm: u8,
}

impl RecvlessParentRouteDecision {
    #[inline(always)]
    pub(crate) const fn new(scope: ScopeId, arm: u8) -> Option<Self> {
        if !scope.is_none() && arm <= 1 {
            Some(Self { scope, arm })
        } else {
            None
        }
    }

    #[inline(always)]
    pub(crate) const fn scope(self) -> ScopeId {
        self.scope
    }

    #[inline(always)]
    pub(crate) const fn arm(self) -> u8 {
        self.arm
    }
}

/// Role-local route conflict attached to a dependency row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LocalConflict {
    Unconditional,
    SharedRoute,
    RouteArm { scope: ScopeId, arm: u8 },
}

impl LocalConflict {
    #[inline(always)]
    pub(crate) const fn route_arm(scope: ScopeId, arm: Option<u8>) -> Self {
        match arm {
            Some(arm) => Self::RouteArm { scope, arm },
            None => Self::SharedRoute,
        }
    }
}

/// Result of evaluating a dependency row against route conflict and progress.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LocalDependencyState {
    InactiveByConflict,
    Satisfied,
    Blocked,
}

impl LocalDependencyState {
    #[inline(always)]
    pub(crate) const fn allows_event(self) -> bool {
        !matches!(self, Self::Blocked)
    }
}

/// Maximum first-receive dispatch entries stored for a route scope.
pub(crate) const MAX_FIRST_RECV_DISPATCH: usize = 16;

/// Dense region occupied by a compiled scope in the role-local state stream.
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

/// Compiled route scope region containing a local state.
///
/// This is a descriptor fact used by endpoint preview paths so they do not
/// inspect generic scope topology or route kinds while walking local events.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RouteScopeRegion {
    region: ScopeRegion,
}

impl RouteScopeRegion {
    #[inline(always)]
    pub(crate) const fn from_region(region: ScopeRegion) -> Option<Self> {
        if matches!(region.kind, ScopeKind::Route) {
            Some(Self { region })
        } else {
            None
        }
    }

    #[inline(always)]
    pub(crate) const fn scope(self) -> ScopeId {
        self.region.scope_id
    }

    #[inline(always)]
    pub(crate) const fn start(self) -> usize {
        self.region.start
    }

    #[inline(always)]
    pub(crate) const fn end(self) -> usize {
        self.region.end
    }

    #[inline(always)]
    pub(crate) const fn linger(self) -> bool {
        self.region.linger
    }
}

/// Index identifying a local state within the synthesized typestate graph.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct StateIndex(u16);

impl StateIndex {
    pub(crate) const MAX: Self = Self(u16::MAX);

    #[inline(always)]
    pub(crate) const fn new(raw: u16) -> Self {
        Self(raw)
    }

    #[inline(always)]
    pub(crate) const fn from_usize(idx: usize) -> Self {
        if idx > (u16::MAX as usize) {
            panic!("state index overflow");
        }
        Self(idx as u16)
    }

    #[inline(always)]
    pub(crate) const fn raw(self) -> u16 {
        self.0
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

/// Compiled first-recv dispatch fact for a route arm.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FirstRecvDispatchSpec {
    frame_label: u8,
    lane: u8,
    arm: u8,
    target: StateIndex,
}

impl FirstRecvDispatchSpec {
    pub(crate) const EMPTY: Self = Self {
        frame_label: 0,
        lane: 0,
        arm: 0,
        target: StateIndex::MAX,
    };

    #[inline(always)]
    pub(crate) const fn new(frame_label: u8, lane: u8, arm: u8, target: StateIndex) -> Self {
        Self {
            frame_label,
            lane,
            arm,
            target,
        }
    }

    #[inline(always)]
    pub(crate) const fn frame_label(self) -> u8 {
        self.frame_label
    }

    #[inline(always)]
    pub(crate) const fn lane(self) -> u8 {
        self.lane
    }

    #[inline(always)]
    pub(crate) const fn arm(self) -> u8 {
        self.arm
    }

    #[inline(always)]
    pub(crate) const fn target(self) -> StateIndex {
        self.target
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
        frame_label: u8,
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
        frame_label: u8,
        resource: Option<u8>,
        is_control: bool,
        shot: Option<CapShot>,
        policy: PolicyMode,
        /// Type-level lane for parallel composition (default 0).
        lane: u8,
    },
    /// Role executes an endpoint-local action.
    Local {
        eff_index: EffIndex,
        label: u8,
        frame_label: u8,
        resource: Option<u8>,
        is_control: bool,
        shot: Option<CapShot>,
        policy: PolicyMode,
        /// Type-level lane for parallel composition (default 0).
        lane: u8,
    },
    /// Terminal node (no further actions).
    Terminate,
}

const LOCAL_ACTION_STATIC_POLICY_ID: u16 = u16::MAX;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PackedLocalAction {
    Send {
        eff_index: EffIndex,
        peer: u8,
        label: u8,
        frame_label: u8,
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
        frame_label: u8,
        resource: Option<u8>,
        is_control: bool,
        shot: Option<CapShot>,
        policy_id: u16,
        lane: u8,
    },
    Local {
        eff_index: EffIndex,
        label: u8,
        frame_label: u8,
        resource: Option<u8>,
        is_control: bool,
        shot: Option<CapShot>,
        policy_id: u16,
        lane: u8,
    },
    Terminate,
}

/// Message-local facts compiled for a typestate node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LocalAtomFacts {
    pub eff_index: EffIndex,
    pub label: u8,
    pub frame_label: u8,
    pub resource: Option<u8>,
    pub is_control: bool,
    pub shot: Option<CapShot>,
    pub policy: PolicyMode,
    pub lane: u8,
}

/// Non-message facts compiled for a typestate node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LocalNodeMeta {
    pub semantic: ControlSemanticKind,
    pub next: StateIndex,
    pub scope: ScopeId,
    pub route_arm: Option<u8>,
    pub is_choice_determinant: bool,
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
        false
    }

    /// Returns the jump reason if this is a Jump action.
    #[inline(always)]
    pub(crate) const fn jump_reason(&self) -> Option<JumpReason> {
        None
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

    /// Construct a send node that advances to `meta.next`.
    pub(crate) const fn send(peer: u8, facts: LocalAtomFacts, meta: LocalNodeMeta) -> Self {
        Self {
            action: PackedLocalAction::Send {
                eff_index: facts.eff_index,
                peer,
                label: facts.label,
                frame_label: facts.frame_label,
                resource: facts.resource,
                is_control: facts.is_control,
                shot: facts.shot,
                policy_id: encode_policy_id(facts.policy),
                lane: facts.lane,
            },
            next: meta.next,
            scope: CompactScopeId::from_scope_id(meta.scope),
            route_arm_raw: Self::encode_route_arm(meta.route_arm),
            flags: Self::flags(meta.is_choice_determinant, meta.semantic),
        }
    }

    /// Construct a receive node that advances to `meta.next`.
    pub(crate) const fn recv(peer: u8, facts: LocalAtomFacts, meta: LocalNodeMeta) -> Self {
        Self {
            action: PackedLocalAction::Recv {
                eff_index: facts.eff_index,
                peer,
                label: facts.label,
                frame_label: facts.frame_label,
                resource: facts.resource,
                is_control: facts.is_control,
                shot: facts.shot,
                policy_id: encode_policy_id(facts.policy),
                lane: facts.lane,
            },
            next: meta.next,
            scope: CompactScopeId::from_scope_id(meta.scope),
            route_arm_raw: Self::encode_route_arm(meta.route_arm),
            flags: Self::flags(meta.is_choice_determinant, meta.semantic),
        }
    }

    /// Construct a local action node that advances to `meta.next`.
    pub(crate) const fn local(facts: LocalAtomFacts, meta: LocalNodeMeta) -> Self {
        Self {
            action: PackedLocalAction::Local {
                eff_index: facts.eff_index,
                label: facts.label,
                frame_label: facts.frame_label,
                resource: facts.resource,
                is_control: facts.is_control,
                shot: facts.shot,
                policy_id: encode_policy_id(facts.policy),
                lane: facts.lane,
            },
            next: meta.next,
            scope: CompactScopeId::from_scope_id(meta.scope),
            route_arm_raw: Self::encode_route_arm(meta.route_arm),
            flags: Self::flags(meta.is_choice_determinant, meta.semantic),
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

    /// Action associated with the node.
    #[inline(always)]
    pub(crate) const fn action(&self) -> LocalAction {
        match self.action {
            PackedLocalAction::Send {
                eff_index,
                peer,
                label,
                frame_label,
                resource,
                is_control,
                shot,
                policy_id,
                lane,
            } => LocalAction::Send {
                eff_index,
                peer,
                label,
                frame_label,
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
                frame_label,
                resource,
                is_control,
                shot,
                policy_id,
                lane,
            } => LocalAction::Recv {
                eff_index,
                peer,
                label,
                frame_label,
                resource,
                is_control,
                shot,
                policy: decode_policy(policy_id, self.scope),
                lane,
            },
            PackedLocalAction::Local {
                eff_index,
                label,
                frame_label,
                resource,
                is_control,
                shot,
                policy_id,
                lane,
            } => LocalAction::Local {
                eff_index,
                label,
                frame_label,
                resource,
                is_control,
                shot,
                policy: decode_policy(policy_id, self.scope),
                lane,
            },
            PackedLocalAction::Terminate => LocalAction::Terminate,
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
}
