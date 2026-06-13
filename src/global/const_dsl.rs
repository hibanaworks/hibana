//! Const helpers for building segmented `EffStruct` images at compile time.
//!
//! These helpers progressively migrate the global combinators (`send/seq/par/route`)
//! toward a const-only surface. They provide an `EffList` accumulator that stays
//! segment-addressed and is read through crate-private segment-aware accessors.
//!
//! # Unsafe Owner Contract
//!
//! `EffList` owns fixed arrays of compile-time metadata markers. The only raw
//! slice construction in this module exposes initialized prefixes whose lengths
//! are advanced by the same const builder methods that write the backing rows.
//! No returned slice outlives `self`, and no method exposes mutable aliases to
//! those rows while a shared prefix view exists.
mod eff_list;
mod eff_list_resolver;

use crate::eff::{self, EffStruct};
use crate::global::Message;

const MAX_SEGMENT_EFFS: usize = eff::meta::MAX_SEGMENT_EFFS;
const MAX_SEGMENTS: usize = eff::meta::MAX_SEGMENTS;
const MAX_CAPACITY: usize = eff::meta::MAX_EFF_NODES;

mod scope;

pub(crate) use self::eff_list::const_send_typed;
pub(crate) use self::scope::{CompactScopeId, ScopeEvent, ScopeId, ScopeKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ResolverMode {
    Static,
    Dynamic {
        resolver_id: u16,
        scope: CompactScopeId,
    },
}

impl ResolverMode {
    pub(crate) const fn static_mode() -> Self {
        Self::Static
    }

    /// Create a dynamic resolver marker with the given resolver id.
    ///
    /// Route decisions are evaluated from the projected route site and the
    /// registered resolver.
    ///
    /// Public choreography authors do not name this lowering hook directly.
    /// ```rust,ignore
    /// fn resolve_decision(
    ///     state: &DecisionState,
    /// ) -> Result<hibana::runtime::resolver::DecisionResolution, hibana::runtime::resolver::ResolverError> {
    ///     Ok(hibana::runtime::resolver::DecisionResolution::Arm(state.preferred_arm))
    /// }
    ///
    /// let decision_state = DecisionState {
    ///     preferred_arm: hibana::runtime::resolver::DecisionArm::Left,
    /// };
    ///
    /// rv.role(&controller).set_resolver::<MY_RESOLVER_ID>(
    ///     hibana::runtime::resolver::ResolverRef::decision_state(&decision_state, resolve_decision),
    /// )?;
    /// ```
    ///
    /// [`SessionKit::rendezvous`]: crate::runtime::SessionKit::rendezvous
    pub(crate) const fn dynamic(resolver_id: u16) -> Self {
        Self::Dynamic {
            resolver_id,
            scope: CompactScopeId::none(),
        }
    }

    pub(crate) const fn is_dynamic(self) -> bool {
        matches!(self, Self::Dynamic { .. })
    }

    pub(crate) const fn dynamic_resolver_id(self) -> Option<u16> {
        match self {
            Self::Dynamic { resolver_id, .. } => Some(resolver_id),
            Self::Static => None,
        }
    }

    pub(crate) const fn scope(self) -> ScopeId {
        match self {
            Self::Dynamic { scope, .. } => scope.to_scope_id(),
            Self::Static => ScopeId::none(),
        }
    }

    pub(crate) const fn with_scope(self, scope: ScopeId) -> Self {
        match self {
            Self::Dynamic { resolver_id, .. } => Self::Dynamic {
                resolver_id,
                scope: CompactScopeId::from_scope_id(scope),
            },
            Self::Static => Self::Static,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct ResolverMarker {
    pub(crate) offset: usize,
    pub(crate) resolver: ResolverMode,
}

impl ResolverMarker {
    const fn empty() -> Self {
        Self {
            offset: 0,
            resolver: ResolverMode::Static,
        }
    }

    const fn new(offset: usize, resolver: ResolverMode) -> Self {
        Self { offset, resolver }
    }
}

#[derive(Clone, Copy)]
pub struct ScopeMarker {
    pub offset: usize,
    pub scope_id: ScopeId,
    pub scope_kind: ScopeKind,
    pub event: ScopeEvent,
    pub linger: bool,
    /// Controller role for route scopes, derived from the first visible arm action.
    /// `None` for non-Route scopes or when controller info is unavailable.
    pub controller_role: Option<u8>,
}

impl ScopeMarker {
    pub const fn empty() -> Self {
        Self {
            offset: 0,
            scope_id: ScopeId::none(),
            scope_kind: ScopeKind::Generic,
            event: ScopeEvent::Enter,
            linger: false,
            controller_role: None,
        }
    }
}

/// Segment-local summary for effect rows and metadata markers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SegmentSummary {
    eff_len: u16,
    scope_marker_len: u16,
    route_scope_enter_len: u16,
    resolver_marker_len: u16,
}

impl SegmentSummary {
    pub(crate) const EMPTY: Self = Self {
        eff_len: 0,
        scope_marker_len: 0,
        route_scope_enter_len: 0,
        resolver_marker_len: 0,
    };

    #[inline(always)]
    const fn bump(value: u16) -> u16 {
        if value == u16::MAX {
            panic!("segment summary overflow");
        }
        value + 1
    }

    #[inline(always)]
    const fn with_effect(mut self) -> Self {
        self.eff_len = Self::bump(self.eff_len);
        self
    }

    #[inline(always)]
    const fn with_scope_marker(mut self, scope_kind: ScopeKind, event: ScopeEvent) -> Self {
        self.scope_marker_len = Self::bump(self.scope_marker_len);
        if matches!(scope_kind, ScopeKind::Route) && matches!(event, ScopeEvent::Enter) {
            self.route_scope_enter_len = Self::bump(self.route_scope_enter_len);
        }
        self
    }

    #[inline(always)]
    const fn with_resolver_marker(mut self) -> Self {
        self.resolver_marker_len = Self::bump(self.resolver_marker_len);
        self
    }
}

/// Accumulator used to build `EffStruct` sequences in const contexts.
#[derive(Clone, Copy)]
pub struct EffList {
    segments: [[EffStruct; MAX_SEGMENT_EFFS]; MAX_SEGMENTS],
    segment_summaries: [SegmentSummary; MAX_SEGMENTS],
    len: usize,
    scope_budget: u16,
    scope_markers: [ScopeMarker; MAX_CAPACITY],
    scope_marker_len: usize,
    resolver_markers: [ResolverMarker; MAX_CAPACITY],
    resolver_marker_len: usize,
}
