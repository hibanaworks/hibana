//! Immutable typestate facts and metadata.

use crate::{
    eff::{self, EffIndex, EventOrigin},
    global::{
        compiled::images::EventSemanticKind,
        const_dsl::{
            CompactScopeId, INTRINSIC_ROUTE_RESOLVER_ID, ReentryMark, RouteResolver, ScopeId,
            ScopeKind,
        },
    },
};

mod meta;
pub(crate) use meta::{EventCommitMeta, LocalMeta, RecvMeta, SendMeta, state_index_to_usize};
mod passive_child;
pub(crate) use passive_child::PassiveArmChildFact;

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
    start: u16,
    end: u16,
}

impl LocalDependency {
    #[inline(always)]
    pub(crate) const fn with_conflict_range(
        scope: ScopeId,
        conflict: LocalConflict,
        start: usize,
        end: usize,
    ) -> Self {
        if start > PackedLocalDependency::STEP_MASK as usize
            || end > PackedLocalDependency::STEP_MASK as usize
        {
            crate::invariant();
        }
        if start > end {
            crate::invariant();
        }
        Self {
            scope,
            conflict,
            start: start as u16,
            end: end as u16,
        }
    }

    #[inline(always)]
    pub(crate) const fn scope(self) -> ScopeId {
        self.scope
    }

    #[inline(always)]
    pub(crate) const fn conflict(self) -> LocalConflict {
        self.conflict
    }

    #[inline(always)]
    pub(crate) const fn start(self) -> usize {
        self.start as usize
    }

    #[inline(always)]
    pub(crate) const fn end(self) -> usize {
        self.end as usize
    }
}

/// Compact role-local dependency row stored beside local step lanes.
///
/// Dependency scopes are always parallel scopes. Route conflicts only need the
/// enclosing route ordinal plus the selected arm. Keeping this as one word
/// prevents the event-image row from growing into a full `ScopeId` pair.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PackedLocalDependency(u64);

impl PackedLocalDependency {
    const ABSENT_RAW: u64 = u64::MAX;
    const STEP_BITS: u64 = 12;
    pub(crate) const STEP_MASK: u64 = (1 << Self::STEP_BITS) - 1;
    const DEP_ORDINAL_BITS: u64 = 12;
    const DEP_ORDINAL_MASK: u64 = (1 << Self::DEP_ORDINAL_BITS) - 1;
    const ROUTE_ORDINAL_BITS: u64 = 13;
    const ROUTE_ORDINAL_MASK: u64 = (1 << Self::ROUTE_ORDINAL_BITS) - 1;
    const START_SHIFT: u64 = 0;
    const END_SHIFT: u64 = Self::START_SHIFT + Self::STEP_BITS;
    const DEP_SHIFT: u64 = Self::END_SHIFT + Self::STEP_BITS;
    const CONFLICT_SHIFT: u64 = Self::DEP_SHIFT + Self::DEP_ORDINAL_BITS;
    const ROUTE_SHIFT: u64 = Self::CONFLICT_SHIFT + 2;
    const CONFLICT_UNCONDITIONAL: u64 = 0;
    const CONFLICT_SHARED_ROUTE: u64 = 1;
    const CONFLICT_ROUTE_ARM_0: u64 = 2;
    const CONFLICT_ROUTE_ARM_1: u64 = 3;

    #[inline(always)]
    pub(crate) const fn none() -> Self {
        Self(Self::ABSENT_RAW)
    }

    #[inline(always)]
    pub(crate) const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    #[inline(always)]
    pub(crate) const fn raw(self) -> u64 {
        self.0
    }

    #[inline(always)]
    pub(crate) const fn is_none(self) -> bool {
        self.0 == Self::ABSENT_RAW
    }

    #[inline(always)]
    pub(crate) const fn from_dependency(dependency: LocalDependency) -> Self {
        let scope = dependency.scope();
        if scope.is_none() {
            return Self::none();
        }
        if !matches!(scope.kind(), ScopeKind::Parallel) {
            crate::invariant();
        }
        let dep_ordinal = scope.local_ordinal() as u64;
        if dep_ordinal > Self::DEP_ORDINAL_MASK {
            crate::invariant();
        }
        let start = dependency.start() as u64;
        let end = dependency.end() as u64;
        if start > Self::STEP_MASK || end > Self::STEP_MASK || start > end {
            crate::invariant();
        }

        let (conflict_tag, route_ordinal) = match dependency.conflict() {
            LocalConflict::Unconditional => (Self::CONFLICT_UNCONDITIONAL, 0),
            LocalConflict::SharedRoute => (Self::CONFLICT_SHARED_ROUTE, 0),
            LocalConflict::RouteArm { scope, arm } => {
                if scope.is_none() || !matches!(scope.kind(), ScopeKind::Route) {
                    crate::invariant();
                }
                let route_ordinal = scope.local_ordinal() as u64;
                if route_ordinal > Self::ROUTE_ORDINAL_MASK {
                    crate::invariant();
                }
                match arm {
                    0 => (Self::CONFLICT_ROUTE_ARM_0, route_ordinal),
                    1 => (Self::CONFLICT_ROUTE_ARM_1, route_ordinal),
                    _ => crate::invariant(),
                }
            }
        };

        Self(
            (start << Self::START_SHIFT)
                | (end << Self::END_SHIFT)
                | (dep_ordinal << Self::DEP_SHIFT)
                | (conflict_tag << Self::CONFLICT_SHIFT)
                | (route_ordinal << Self::ROUTE_SHIFT),
        )
    }

    #[inline(always)]
    pub(crate) const fn to_dependency(self) -> Option<LocalDependency> {
        if self.0 == Self::ABSENT_RAW {
            return None;
        }
        let start = ((self.0 >> Self::START_SHIFT) & Self::STEP_MASK) as usize;
        let end = ((self.0 >> Self::END_SHIFT) & Self::STEP_MASK) as usize;
        let dep_ordinal = ((self.0 >> Self::DEP_SHIFT) & Self::DEP_ORDINAL_MASK) as u16;
        let conflict_tag = (self.0 >> Self::CONFLICT_SHIFT) & 0b11;
        let route_ordinal = ((self.0 >> Self::ROUTE_SHIFT) & Self::ROUTE_ORDINAL_MASK) as u16;
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
            4..=u64::MAX => crate::invariant(),
        };
        Some(LocalDependency::with_conflict_range(
            scope, conflict, start, end,
        ))
    }
}

/// Packed role-local route membership for one event or route scope.
///
/// This is the production conflict row used by the event cursor. It records the
/// nearest enclosing route arm at projection time; parent route membership is
/// represented by the route scope's own conflict row, so runtime enabled checks
/// can walk conflict rows without interpreting route-scope structure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PackedEventConflict(u16);

impl PackedEventConflict {
    const ABSENT_RAW: u16 = u16::MAX;
    const ARM_BITS: u16 = 1;
    const ROUTE_MASK: u16 = (1 << 13) - 1;
    /// Maximum row-chain length for conflict traversal.
    ///
    /// An event conflict row can only point through route-scope conflict rows
    /// derived from the same fixed-size local event image. The cursor uses this
    /// row capacity as its cycle guard instead of consulting runtime route
    /// structure counts.
    pub(crate) const MAX_CHAIN_DEPTH: usize = eff::meta::MAX_EFF_NODES + 1;

    #[inline(always)]
    pub(crate) const fn none() -> Self {
        Self(Self::ABSENT_RAW)
    }

    #[inline(always)]
    pub(crate) const fn from_raw(raw: u16) -> Self {
        Self(raw)
    }

    #[inline(always)]
    pub(crate) const fn raw(self) -> u16 {
        self.0
    }

    #[inline(always)]
    pub(crate) const fn is_none(self) -> bool {
        self.0 == Self::ABSENT_RAW
    }

    #[inline(always)]
    pub(crate) const fn route_arm(scope: ScopeId, arm: u8) -> Self {
        if scope.is_none() || !matches!(scope.kind(), ScopeKind::Route) {
            crate::invariant();
        }
        if arm > 1 {
            crate::invariant();
        }
        let ordinal = scope.local_ordinal();
        if ordinal > Self::ROUTE_MASK {
            crate::invariant();
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
        if self.0 == Self::ABSENT_RAW {
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

/// Dense event-row interval occupied by a compiled route scope.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RouteScopeRows {
    scope: ScopeId,
    start: usize,
    end: usize,
    reentry: ReentryMark,
}

impl RouteScopeRows {
    #[inline(always)]
    pub(crate) const fn new(
        scope: ScopeId,
        start: usize,
        end: usize,
        reentry: ReentryMark,
    ) -> Option<Self> {
        if scope.is_none()
            || !matches!(scope.kind(), ScopeKind::Route)
            || start >= end
            || end > u16::MAX as usize
        {
            None
        } else {
            Some(Self {
                scope,
                start,
                end,
                reentry,
            })
        }
    }

    #[inline(always)]
    pub(crate) const fn scope(self) -> ScopeId {
        self.scope
    }

    #[inline(always)]
    pub(crate) const fn start(self) -> usize {
        self.start
    }

    #[inline(always)]
    pub(crate) const fn end(self) -> usize {
        self.end
    }

    #[inline(always)]
    pub(crate) const fn reentry(self) -> bool {
        self.reentry.is_reentrant()
    }
}

/// Index identifying a local state within the synthesized typestate graph.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct StateIndex(u16);

impl StateIndex {
    pub(crate) const ABSENT: Self = Self(u16::MAX);

    #[inline(always)]
    pub(crate) const fn new(raw: u16) -> Self {
        Self(raw)
    }

    #[inline(always)]
    pub(crate) const fn from_usize(idx: usize) -> Self {
        if idx > (u16::MAX as usize) {
            crate::invariant();
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
    pub(crate) const fn is_absent(self) -> bool {
        self.0 == u16::MAX
    }
}

/// Compiled first-recv dispatch fact for a route arm.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FirstRecvDispatchSpec {
    lane: u8,
    frame_label: u8,
    arm: u8,
    target: StateIndex,
}

impl FirstRecvDispatchSpec {
    pub(crate) const EMPTY: Self = Self {
        lane: 0,
        frame_label: 0,
        arm: 0,
        target: StateIndex::ABSENT,
    };

    #[inline(always)]
    pub(crate) const fn new(lane: u8, frame_label: u8, arm: u8, target: StateIndex) -> Self {
        Self {
            lane,
            frame_label,
            arm,
            target,
        }
    }

    #[inline(always)]
    pub(crate) const fn lane(self) -> u8 {
        self.lane
    }

    #[inline(always)]
    pub(crate) const fn frame_label(self) -> u8 {
        self.frame_label
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
        origin: EventOrigin,
        resolver: RouteResolver,
        /// Type-level lane for parallel composition; lane 0 is the primary lane.
        lane: u8,
    },
    /// Role receives a message from a peer.
    Recv {
        eff_index: EffIndex,
        peer: u8,
        label: u8,
        frame_label: u8,
        resource: Option<u8>,
        origin: EventOrigin,
        resolver: RouteResolver,
        /// Type-level lane for parallel composition; lane 0 is the primary lane.
        lane: u8,
    },
    /// Role executes an endpoint-local action.
    Local {
        eff_index: EffIndex,
        label: u8,
        frame_label: u8,
        resource: Option<u8>,
        origin: EventOrigin,
        resolver: RouteResolver,
        /// Type-level lane for parallel composition; lane 0 is the primary lane.
        lane: u8,
    },
    /// Terminal node (no further actions).
    Terminate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PackedLocalAction {
    Send {
        eff_index: EffIndex,
        peer: u8,
        label: u8,
        frame_label: u8,
        resource: Option<u8>,
        origin: EventOrigin,
        resolver_id: u16,
        lane: u8,
    },
    Recv {
        eff_index: EffIndex,
        peer: u8,
        label: u8,
        frame_label: u8,
        resource: Option<u8>,
        origin: EventOrigin,
        resolver_id: u16,
        lane: u8,
    },
    Local {
        eff_index: EffIndex,
        label: u8,
        frame_label: u8,
        resource: Option<u8>,
        origin: EventOrigin,
        resolver_id: u16,
        lane: u8,
    },
    Terminate,
}

/// Message-local facts compiled for a typestate node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LocalAtomFacts {
    pub(crate) eff_index: EffIndex,
    pub(crate) label: u8,
    pub(crate) frame_label: u8,
    pub(crate) resource: Option<u8>,
    pub(crate) origin: EventOrigin,
    pub(crate) resolver: RouteResolver,
    pub(crate) lane: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RouteChoiceMark {
    Ordinary,
    Determinant,
}

impl RouteChoiceMark {
    #[inline(always)]
    pub(crate) const fn is_determinant(self) -> bool {
        matches!(self, Self::Determinant)
    }
}

/// Non-message facts compiled for a typestate node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LocalNodeMeta {
    pub(crate) semantic: EventSemanticKind,
    pub(crate) next: StateIndex,
    pub(crate) scope: ScopeId,
    pub(crate) route_arm: Option<u8>,
    pub(crate) choice: RouteChoiceMark,
}

#[inline(always)]
const fn encode_resolver_id(resolver: RouteResolver) -> u16 {
    match resolver {
        RouteResolver::Dynamic { resolver_id, .. } => resolver_id,
        RouteResolver::Intrinsic => INTRINSIC_ROUTE_RESOLVER_ID,
    }
}

#[inline(always)]
const fn decode_resolver(resolver_id: u16, scope: CompactScopeId) -> RouteResolver {
    if resolver_id == INTRINSIC_ROUTE_RESOLVER_ID {
        RouteResolver::Intrinsic
    } else {
        RouteResolver::Dynamic { resolver_id, scope }
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
    const fn encode_semantic(semantic: EventSemanticKind) -> u8 {
        semantic.packed_bits() << Self::FLAG_SEMANTIC_SHIFT
    }

    #[inline(always)]
    const fn decode_semantic(flags: u8) -> EventSemanticKind {
        EventSemanticKind::from_packed_bits(
            (flags & Self::FLAG_SEMANTIC_MASK) >> Self::FLAG_SEMANTIC_SHIFT,
        )
    }

    #[inline(always)]
    const fn flags(choice: RouteChoiceMark, semantic: EventSemanticKind) -> u8 {
        let mut flags = Self::encode_semantic(semantic);
        if choice.is_determinant() {
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
                origin: facts.origin,
                resolver_id: encode_resolver_id(facts.resolver),
                lane: facts.lane,
            },
            next: meta.next,
            scope: CompactScopeId::from_scope_id(meta.scope),
            route_arm_raw: Self::encode_route_arm(meta.route_arm),
            flags: Self::flags(meta.choice, meta.semantic),
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
                origin: facts.origin,
                resolver_id: encode_resolver_id(facts.resolver),
                lane: facts.lane,
            },
            next: meta.next,
            scope: CompactScopeId::from_scope_id(meta.scope),
            route_arm_raw: Self::encode_route_arm(meta.route_arm),
            flags: Self::flags(meta.choice, meta.semantic),
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
                origin: facts.origin,
                resolver_id: encode_resolver_id(facts.resolver),
                lane: facts.lane,
            },
            next: meta.next,
            scope: CompactScopeId::from_scope_id(meta.scope),
            route_arm_raw: Self::encode_route_arm(meta.route_arm),
            flags: Self::flags(meta.choice, meta.semantic),
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
                origin,
                resolver_id,
                lane,
            } => LocalAction::Send {
                eff_index,
                peer,
                label,
                frame_label,
                resource,
                origin,
                resolver: decode_resolver(resolver_id, self.scope),
                lane,
            },
            PackedLocalAction::Recv {
                eff_index,
                peer,
                label,
                frame_label,
                resource,
                origin,
                resolver_id,
                lane,
            } => LocalAction::Recv {
                eff_index,
                peer,
                label,
                frame_label,
                resource,
                origin,
                resolver: decode_resolver(resolver_id, self.scope),
                lane,
            },
            PackedLocalAction::Local {
                eff_index,
                label,
                frame_label,
                resource,
                origin,
                resolver_id,
                lane,
            } => LocalAction::Local {
                eff_index,
                label,
                frame_label,
                resource,
                origin,
                resolver: decode_resolver(resolver_id, self.scope),
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
    pub(crate) const fn choice_mark(&self) -> RouteChoiceMark {
        if self.is_choice_determinant() {
            RouteChoiceMark::Determinant
        } else {
            RouteChoiceMark::Ordinary
        }
    }

    #[inline(always)]
    pub(crate) const fn event_semantic(&self) -> EventSemanticKind {
        Self::decode_semantic(self.flags)
    }
}
