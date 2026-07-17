//! Immutable typestate facts and metadata.

use crate::{
    eff::{EffIndex, EventOrigin},
    global::{
        compiled::images::EventSemanticKind,
        const_dsl::{ReentryMark, ScopeId, ScopeKind},
    },
};

mod dependency;
pub(crate) use dependency::{LocalDependency, PackedLocalDependency};
mod inbound_key;
pub(crate) use inbound_key::{DeterministicInboundKey, InboundFrameKey};
#[cfg(kani)]
mod kani;
mod meta;
pub(crate) use meta::{EventCommitMeta, LocalMeta, RecvMeta, SendMeta, state_index_to_usize};
mod passive_child;
pub(crate) use passive_child::PassiveArmChildFact;

/// Route-arm marker used when a first-recv dispatch entry is shared by both
/// arms. It is a compiled descriptor fact, not runtime route authority.
pub(crate) const ARM_SHARED: u8 = 0xFF;

/// Packed role-local route membership for one event or route scope.
///
/// This is the production conflict row used by the event cursor. It records the
/// nearest enclosing route arm at projection time; parent route membership is
/// represented by the route scope's own conflict row, so runtime enabled checks
/// can walk conflict rows without interpreting route-scope structure.
/// Bits 0, 1..=13, 14, and 15 are respectively arm, route ordinal,
/// reentry, and the no-conflict tag. Route rows keep bit 15 clear; `0xc000`
/// carries reentry without a route conflict and `0xffff` is absent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PackedEventConflict(u16);

impl PackedEventConflict {
    const ABSENT_RAW: u16 = u16::MAX;
    const ARM_BITS: u16 = 1;
    const ROUTE_MASK: u16 = (1 << 13) - 1;
    const ROUTE_REENTRY_BIT: u16 = 1 << 14;
    const NO_CONFLICT_BIT: u16 = 1 << 15;
    const REENTRY_WITHOUT_CONFLICT_RAW: u16 = Self::NO_CONFLICT_BIT | Self::ROUTE_REENTRY_BIT;
    const ROUTE_VALUE_MASK: u16 =
        (Self::ROUTE_MASK << Self::ARM_BITS) | 1 | Self::ROUTE_REENTRY_BIT;
    #[inline(always)]
    pub(crate) const fn none() -> Self {
        Self(Self::ABSENT_RAW)
    }

    #[inline(always)]
    const fn decode_raw(raw: u16) -> Option<Self> {
        if raw == Self::ABSENT_RAW || raw == Self::REENTRY_WITHOUT_CONFLICT_RAW {
            return Some(Self(raw));
        }
        if (raw & !Self::ROUTE_VALUE_MASK) != 0 {
            return None;
        }
        Some(Self(raw))
    }

    #[inline(never)]
    pub(crate) const fn from_raw(raw: u16) -> Self {
        match Self::decode_raw(raw) {
            Some(conflict) => conflict,
            None => crate::invariant(),
        }
    }

    #[inline(always)]
    pub(crate) const fn raw(self) -> u16 {
        self.0
    }

    #[inline(always)]
    pub(crate) const fn is_none(self) -> bool {
        self.0 == Self::ABSENT_RAW || self.0 == Self::REENTRY_WITHOUT_CONFLICT_RAW
    }

    #[inline(always)]
    pub(crate) const fn route_arm(scope: ScopeId, arm: u8) -> Self {
        if scope.is_none() || !matches!(scope.kind(), Some(ScopeKind::Route)) {
            crate::invariant();
        }
        if arm > 1 {
            crate::invariant();
        }
        let ordinal = scope.local_ordinal();
        Self((ordinal << Self::ARM_BITS) | arm as u16)
    }

    #[inline(always)]
    pub(crate) const fn with_route_reentry(self, mark: ReentryMark) -> Self {
        match mark {
            ReentryMark::SinglePass => {
                if self.0 == Self::ABSENT_RAW || self.0 == Self::REENTRY_WITHOUT_CONFLICT_RAW {
                    Self::none()
                } else {
                    Self(self.0 & !Self::ROUTE_REENTRY_BIT)
                }
            }
            ReentryMark::Reentrant => {
                if self.0 == Self::ABSENT_RAW {
                    Self(Self::REENTRY_WITHOUT_CONFLICT_RAW)
                } else {
                    Self(self.0 | Self::ROUTE_REENTRY_BIT)
                }
            }
        }
    }

    #[inline(always)]
    pub(crate) const fn route_reentry(self) -> bool {
        self.0 != Self::ABSENT_RAW && (self.0 & Self::ROUTE_REENTRY_BIT) != 0
    }

    #[inline(always)]
    const fn decoded_route(self) -> Option<(u16, u8)> {
        if self.is_none() {
            None
        } else {
            Some((
                (self.0 >> Self::ARM_BITS) & Self::ROUTE_MASK,
                (self.0 & 1) as u8,
            ))
        }
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
        let Some((ordinal, arm)) = self.decoded_route() else {
            return None;
        };
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
            || !matches!(scope.kind(), Some(ScopeKind::Route))
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
        if raw == u16::MAX {
            crate::invariant();
        }
        Self(raw)
    }

    #[inline(always)]
    pub(crate) const fn checked_from_usize(idx: usize) -> Option<Self> {
        if idx < MAX_STATES {
            Some(Self(idx as u16))
        } else {
            None
        }
    }

    #[inline(always)]
    pub(crate) const fn from_usize(idx: usize) -> Self {
        match Self::checked_from_usize(idx) {
            Some(index) => index,
            None => crate::invariant(),
        }
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

/// Maximum number of local states tracked per role (one extra slot for the
/// terminal state).
pub(crate) const MAX_STATES: usize = crate::eff::meta::COMPACT_EVENT_IDENTITY_CAPACITY;

/// Local action associated with a typestate node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LocalAction {
    /// Role sends a message to a peer.
    Send {
        eff_index: EffIndex,
        peer: u8,
        label: u8,
        frame_label: u8,
        origin: EventOrigin,
        /// Type-level lane for parallel composition; lane 0 is the primary lane.
        lane: u8,
    },
    /// Role receives a message from a peer.
    Recv {
        eff_index: EffIndex,
        peer: u8,
        label: u8,
        frame_label: u8,
        origin: EventOrigin,
        /// Type-level lane for parallel composition; lane 0 is the primary lane.
        lane: u8,
    },
    /// Role executes an endpoint-local action.
    Local {
        eff_index: EffIndex,
        label: u8,
        frame_label: u8,
        origin: EventOrigin,
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
        origin: EventOrigin,
        lane: u8,
    },
    Recv {
        eff_index: EffIndex,
        peer: u8,
        label: u8,
        frame_label: u8,
        origin: EventOrigin,
        lane: u8,
    },
    Local {
        eff_index: EffIndex,
        label: u8,
        frame_label: u8,
        origin: EventOrigin,
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
    pub(crate) origin: EventOrigin,
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
    scope: ScopeId,
    /// `0`/`1` select an arm and `255` is the sole absence representation.
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
            Some(arm @ 0..=1) => arm,
            Some(_) => crate::invariant(),
            None => Self::ROUTE_ARM_NONE,
        }
    }

    #[inline(always)]
    const fn decode_optional_route_arm_raw(raw: u8) -> Option<Option<u8>> {
        match raw {
            0..=1 => Some(Some(raw)),
            Self::ROUTE_ARM_NONE => Some(None),
            2..=254 => None,
        }
    }

    #[inline(never)]
    const fn decode_route_arm(raw: u8) -> Option<u8> {
        match Self::decode_optional_route_arm_raw(raw) {
            Some(arm) => arm,
            None => crate::invariant(),
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
                origin: facts.origin,
                lane: facts.lane,
            },
            next: meta.next,
            scope: meta.scope,
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
                origin: facts.origin,
                lane: facts.lane,
            },
            next: meta.next,
            scope: meta.scope,
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
                origin: facts.origin,
                lane: facts.lane,
            },
            next: meta.next,
            scope: meta.scope,
            route_arm_raw: Self::encode_route_arm(meta.route_arm),
            flags: Self::flags(meta.choice, meta.semantic),
        }
    }

    /// Construct a terminal node that loops to itself.
    pub(crate) const fn terminal(index: StateIndex) -> Self {
        Self {
            action: PackedLocalAction::Terminate,
            next: index,
            scope: ScopeId::none(),
            route_arm_raw: Self::ROUTE_ARM_NONE,
            flags: 0,
        }
    }

    /// Action associated with the node.
    pub(crate) const fn action(&self) -> LocalAction {
        match self.action {
            PackedLocalAction::Send {
                eff_index,
                peer,
                label,
                frame_label,
                origin,
                lane,
            } => LocalAction::Send {
                eff_index,
                peer,
                label,
                frame_label,
                origin,
                lane,
            },
            PackedLocalAction::Recv {
                eff_index,
                peer,
                label,
                frame_label,
                origin,
                lane,
            } => LocalAction::Recv {
                eff_index,
                peer,
                label,
                frame_label,
                origin,
                lane,
            },
            PackedLocalAction::Local {
                eff_index,
                label,
                frame_label,
                origin,
                lane,
            } => LocalAction::Local {
                eff_index,
                label,
                frame_label,
                origin,
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
        self.scope
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

#[cfg(all(test, hibana_repo_tests))]
mod tests;
