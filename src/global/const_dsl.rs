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

use crate::eff::{self, EffStruct};
use crate::global::{
    MessageControlSpec, MessageSpec, RoleMarker, SendableLabel, StaticControlDesc,
};

const MAX_SEGMENT_EFFS: usize = eff::meta::MAX_SEGMENT_EFFS;
const MAX_SEGMENTS: usize = eff::meta::MAX_SEGMENTS;
const MAX_CAPACITY: usize = eff::meta::MAX_EFF_NODES;

mod scope;

pub(crate) use self::eff_list::const_send_typed;
pub(crate) use self::scope::CompactScopeId;
pub use self::scope::{ControlScopeKind, ScopeEvent, ScopeId, ScopeKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PolicyMode {
    Static,
    Dynamic {
        policy_id: u16,
        scope: CompactScopeId,
    },
}

impl PolicyMode {
    pub(crate) const fn static_mode() -> Self {
        Self::Static
    }

    /// Create a dynamic policy annotation with the given policy id.
    ///
    /// Route decisions are evaluated with fixed priority:
    /// `policy(Route) -> resolver -> PolicyAbort`.
    ///
    /// The actual control operation (route or loop) is determined by the baked
    /// control descriptor metadata, not by the proof term itself.
    ///
    /// # Example
    ///
    /// ```ignore
    /// // Define a route with dynamic policy annotation
    /// const MY_POLICY_ID: u16 = 0x1234;
    /// let left = arm1.policy::<MY_POLICY_ID>();
    /// let right = arm2.policy::<MY_POLICY_ID>();
    /// let program = g::route(left, right);
    ///
    /// // Register resolver before use
    /// let controller = hibana::integration::program::project(&program);
    /// struct RouteState {
    ///     preferred_arm: u8,
    /// }
    ///
    /// fn resolve_route(
    ///     state: &RouteState,
    ///     _ctx: hibana::integration::policy::ResolverContext,
    /// ) -> Result<hibana::integration::policy::RouteResolution, hibana::integration::policy::ResolverError> {
    ///     Ok(hibana::integration::policy::RouteResolution::Arm(state.preferred_arm))
    /// }
    ///
    /// let route_state = RouteState { preferred_arm: 0 };
    ///
    /// cluster.rendezvous(rv_id).role(&controller).set_resolver::<MY_POLICY_ID>(
    ///     hibana::integration::policy::ResolverRef::route_state(&route_state, resolve_route),
    /// )?;
    /// ```
    ///
    /// [`SessionKit::rendezvous`]: crate::integration::SessionKit::rendezvous
    /// [`CpError::PolicyAbort`]: crate::integration::CpError::PolicyAbort
    pub(crate) const fn dynamic(policy_id: u16) -> Self {
        Self::Dynamic {
            policy_id,
            scope: CompactScopeId::none(),
        }
    }

    pub(crate) const fn is_dynamic(self) -> bool {
        matches!(self, Self::Dynamic { .. })
    }

    pub(crate) const fn is_static(self) -> bool {
        matches!(self, Self::Static)
    }

    pub(crate) const fn dynamic_policy_id(self) -> Option<u16> {
        match self {
            Self::Dynamic { policy_id, .. } => Some(policy_id),
            _ => None,
        }
    }

    pub(crate) const fn scope(self) -> ScopeId {
        match self {
            Self::Dynamic { scope, .. } => scope.to_scope_id(),
            _ => ScopeId::none(),
        }
    }

    pub(crate) const fn with_scope(self, scope: ScopeId) -> Self {
        match self {
            Self::Dynamic { policy_id, .. } => Self::Dynamic {
                policy_id,
                scope: CompactScopeId::from_scope_id(scope),
            },
            other => other,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct PolicyMarker {
    pub(crate) offset: usize,
    pub(crate) policy: PolicyMode,
}

impl PolicyMarker {
    const fn empty() -> Self {
        Self {
            offset: 0,
            policy: PolicyMode::Static,
        }
    }

    const fn new(offset: usize, policy: PolicyMode) -> Self {
        Self { offset, policy }
    }
}

#[derive(Clone, Copy)]
pub struct ScopeMarker {
    pub offset: usize,
    pub scope_id: ScopeId,
    pub scope_kind: ScopeKind,
    pub event: ScopeEvent,
    pub linger: bool,
    /// Controller role for Route scopes (derived from the arm-entry self-send).
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

#[derive(Clone, Copy)]
pub struct ControlMarker {
    pub offset: u16,
    pub scope_kind: ControlScopeKind,
    pub tap_id: u16,
}

impl ControlMarker {
    const fn encode_offset(offset: usize) -> u16 {
        if offset > u16::MAX as usize {
            panic!("control marker offset overflow");
        }
        offset as u16
    }

    pub const fn empty() -> Self {
        Self {
            offset: 0,
            scope_kind: ControlScopeKind::None,
            tap_id: 0,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct ControlSpecMarker {
    pub(crate) offset: usize,
    pub(crate) spec: Option<StaticControlDesc>,
}

impl ControlSpecMarker {
    const fn empty() -> Self {
        Self {
            offset: 0,
            spec: None,
        }
    }

    const fn new(offset: usize, spec: StaticControlDesc) -> Self {
        Self {
            offset,
            spec: Some(spec),
        }
    }
}

/// Segment-local summary for effect rows and metadata markers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SegmentSummary {
    eff_len: u16,
    scope_marker_len: u16,
    route_scope_enter_len: u16,
    control_marker_len: u16,
    policy_marker_len: u16,
    control_spec_len: u16,
}

impl SegmentSummary {
    pub(crate) const EMPTY: Self = Self {
        eff_len: 0,
        scope_marker_len: 0,
        route_scope_enter_len: 0,
        control_marker_len: 0,
        policy_marker_len: 0,
        control_spec_len: 0,
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
    const fn with_control_marker(mut self) -> Self {
        self.control_marker_len = Self::bump(self.control_marker_len);
        self
    }

    #[inline(always)]
    const fn with_policy_marker(mut self) -> Self {
        self.policy_marker_len = Self::bump(self.policy_marker_len);
        self
    }

    #[inline(always)]
    const fn with_control_spec(mut self) -> Self {
        self.control_spec_len = Self::bump(self.control_spec_len);
        self
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) const fn eff_len(self) -> usize {
        self.eff_len as usize
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) const fn scope_marker_len(self) -> usize {
        self.scope_marker_len as usize
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) const fn route_scope_enter_len(self) -> usize {
        self.route_scope_enter_len as usize
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) const fn control_marker_len(self) -> usize {
        self.control_marker_len as usize
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) const fn policy_marker_len(self) -> usize {
        self.policy_marker_len as usize
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) const fn control_spec_len(self) -> usize {
        self.control_spec_len as usize
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
    control_markers: [ControlMarker; MAX_CAPACITY],
    control_marker_len: usize,
    policy_markers: [PolicyMarker; MAX_CAPACITY],
    policy_marker_len: usize,
    control_specs: [ControlSpecMarker; MAX_CAPACITY],
    control_spec_len: usize,
}

#[cfg(test)]
mod tests;
