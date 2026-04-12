//! Const helpers for building `EffStruct` slices at compile time.
//!
//! These helpers progressively migrate the global combinators (`send/seq/par/route`)
//! toward a const-only surface. They provide an `EffList` accumulator that can
//! be populated entirely within const contexts and exposed as an `EffStruct`
//! slice via standard slice traits.

use crate::control::cap::mint::CapShot;
use crate::eff::{self, EffStruct};
use crate::global::{
    ControlHandling, ControlLabelSpec, MessageControlSpec, MessageSpec, RoleMarker, SendableLabel,
};

const MAX_CAPACITY: usize = eff::meta::MAX_EFF_NODES;

/// Structured scope classification used by the global DSL to tag composite
/// fragments such as routes, loops, or parallel lanes.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScopeKind {
    /// Default scope kind when no specialised semantics are required.
    Generic = 0,
    /// Scope representing a routing decision (`g::route`).
    Route = 1,
    /// Scope representing a loop fixpoint.
    Loop = 2,
    /// Scope representing a parallel lane (`g::par`).
    Parallel = 3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScopeEvent {
    Enter,
    Exit,
}

/// Encoded scope identifier embedding the scope kind and its structural ordinals.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScopeId {
    raw: u64,
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CompactScopeId {
    raw: u32,
}

impl Default for ScopeId {
    fn default() -> Self {
        ScopeId::none()
    }
}

impl ScopeId {
    const NONE_RAW: u64 = u64::MAX;
    const KIND_BITS: u64 = 3;
    const LOCAL_BITS: u64 = 13;
    const RANGE_BITS: u64 = 16;
    const NEST_BITS: u64 = 16;

    const NEST_SHIFT: u64 = 0;
    const RANGE_SHIFT: u64 = Self::NEST_SHIFT + Self::NEST_BITS;
    const LOCAL_SHIFT: u64 = Self::RANGE_SHIFT + Self::RANGE_BITS;
    const KIND_SHIFT: u64 = Self::LOCAL_SHIFT + Self::LOCAL_BITS;

    const KIND_MASK: u64 = (1 << Self::KIND_BITS) - 1;
    const LOCAL_MASK: u64 = (1 << Self::LOCAL_BITS) - 1;
    const RANGE_MASK: u64 = (1 << Self::RANGE_BITS) - 1;
    const NEST_MASK: u64 = (1 << Self::NEST_BITS) - 1;

    pub const WIRE_NONE_HI: u32 = u32::MAX;
    pub const WIRE_NONE_LO: u16 = u16::MAX;

    pub const ORDINAL_CAPACITY: u16 = Self::LOCAL_MASK as u16;

    pub(crate) const fn compose(kind: ScopeKind, local: u16, range: u16, nest: u16) -> Self {
        if local as u64 > Self::LOCAL_MASK
            || range as u64 > Self::RANGE_MASK
            || nest as u64 > Self::NEST_MASK
        {
            panic!("scope ordinal overflow");
        }
        let raw = ((kind as u64) << Self::KIND_SHIFT)
            | ((local as u64) << Self::LOCAL_SHIFT)
            | ((range as u64) << Self::RANGE_SHIFT)
            | ((nest as u64) << Self::NEST_SHIFT);
        Self { raw }
    }

    pub(crate) const fn new(kind: ScopeKind, local: u16) -> Self {
        Self::compose(kind, local, 0, 0)
    }

    pub const fn none() -> Self {
        Self {
            raw: Self::NONE_RAW,
        }
    }

    pub const fn is_none(self) -> bool {
        self.raw == Self::NONE_RAW
    }

    pub const fn as_option(self) -> Option<Self> {
        if self.is_none() { None } else { Some(self) }
    }

    pub const fn from_raw(raw: u64) -> Self {
        if raw == Self::NONE_RAW {
            Self::none()
        } else {
            Self { raw }
        }
    }

    pub const fn raw(self) -> u64 {
        self.raw
    }

    pub const fn kind(self) -> ScopeKind {
        if self.is_none() {
            return ScopeKind::Generic;
        }
        match ((self.raw >> Self::KIND_SHIFT) & Self::KIND_MASK) as u8 {
            0 => ScopeKind::Generic,
            1 => ScopeKind::Route,
            2 => ScopeKind::Loop,
            3 => ScopeKind::Parallel,
            _ => ScopeKind::Generic,
        }
    }

    pub const fn ordinal(self) -> u16 {
        self.local_ordinal()
    }

    pub const fn local_ordinal(self) -> u16 {
        if self.is_none() {
            return 0;
        }
        ((self.raw >> Self::LOCAL_SHIFT) & Self::LOCAL_MASK) as u16
    }

    pub const fn range_ordinal(self) -> u16 {
        if self.is_none() {
            return 0;
        }
        ((self.raw >> Self::RANGE_SHIFT) & Self::RANGE_MASK) as u16
    }

    pub const fn nest_ordinal(self) -> u16 {
        if self.is_none() {
            return 0;
        }
        ((self.raw >> Self::NEST_SHIFT) & Self::NEST_MASK) as u16
    }

    pub const fn with_range_ordinal(self, range: u16) -> Self {
        if self.is_none() {
            return Self::none();
        }
        Self::compose(
            self.kind(),
            self.local_ordinal(),
            range,
            self.nest_ordinal(),
        )
    }

    pub const fn with_nest_ordinal(self, nest: u16) -> Self {
        if self.is_none() {
            return Self::none();
        }
        Self::compose(
            self.kind(),
            self.local_ordinal(),
            self.range_ordinal(),
            nest,
        )
    }

    pub const fn canonical(self) -> Self {
        if self.is_none() {
            return Self::none();
        }
        Self::compose(self.kind(), self.local_ordinal(), 0, 0)
    }

    pub const fn pack_range_nest(self) -> u32 {
        if self.is_none() {
            0
        } else {
            0x8000_0000 | ((self.range_ordinal() as u32) << 16) | (self.nest_ordinal() as u32)
        }
    }

    pub const fn add_ordinal(self, delta: u16) -> Self {
        if self.is_none() {
            return Self::none();
        }
        let ordinal = self.local_ordinal();
        let sum = ordinal as u32 + delta as u32;
        if sum > Self::LOCAL_MASK as u32 {
            panic!("scope ordinal overflow");
        }
        Self::compose(
            self.kind(),
            sum as u16,
            self.range_ordinal(),
            self.nest_ordinal(),
        )
    }

    pub const fn generic(ordinal: u16) -> Self {
        Self::new(ScopeKind::Generic, ordinal)
    }

    pub const fn route(ordinal: u16) -> Self {
        Self::new(ScopeKind::Route, ordinal)
    }

    pub const fn loop_scope(ordinal: u16) -> Self {
        Self::new(ScopeKind::Loop, ordinal)
    }

    pub const fn parallel(ordinal: u16) -> Self {
        Self::new(ScopeKind::Parallel, ordinal)
    }

    pub const fn to_wire_parts(self) -> (u32, u16) {
        let raw = self.raw();
        let hi = (raw >> 16) as u32;
        let lo = (raw & 0xFFFF) as u16;
        (hi, lo)
    }

    pub const fn from_wire_parts(scope_hi: u32, scope_lo: u16) -> Option<Self> {
        if scope_hi == Self::WIRE_NONE_HI && scope_lo == Self::WIRE_NONE_LO {
            None
        } else {
            let raw = ((scope_hi as u64) << 16) | scope_lo as u64;
            Some(Self::from_raw(raw))
        }
    }

    pub const fn encode_wire(scope: Option<Self>) -> (u32, u16) {
        match scope {
            Some(id) if !id.is_none() => id.to_wire_parts(),
            _ => (Self::WIRE_NONE_HI, Self::WIRE_NONE_LO),
        }
    }
}

impl CompactScopeId {
    const NONE_RAW: u32 = u32::MAX;
    const KIND_BITS: u32 = 3;
    const ORDINAL_BITS: u32 = 9;

    const NEST_SHIFT: u32 = 0;
    const RANGE_SHIFT: u32 = Self::NEST_SHIFT + Self::ORDINAL_BITS;
    const LOCAL_SHIFT: u32 = Self::RANGE_SHIFT + Self::ORDINAL_BITS;
    const KIND_SHIFT: u32 = Self::LOCAL_SHIFT + Self::ORDINAL_BITS;

    const KIND_MASK: u32 = (1 << Self::KIND_BITS) - 1;
    const ORDINAL_MASK: u32 = (1 << Self::ORDINAL_BITS) - 1;

    pub(crate) const fn none() -> Self {
        Self {
            raw: Self::NONE_RAW,
        }
    }

    pub(crate) const fn is_none(self) -> bool {
        self.raw == Self::NONE_RAW
    }

    pub(crate) const fn from_scope_id(scope: ScopeId) -> Self {
        if scope.is_none() {
            return Self::none();
        }
        let local = scope.local_ordinal() as u32;
        let range = scope.range_ordinal() as u32;
        let nest = scope.nest_ordinal() as u32;
        if local > Self::ORDINAL_MASK || range > Self::ORDINAL_MASK || nest > Self::ORDINAL_MASK {
            panic!("compact scope ordinal overflow");
        }
        Self {
            raw: ((scope.kind() as u32) << Self::KIND_SHIFT)
                | (local << Self::LOCAL_SHIFT)
                | (range << Self::RANGE_SHIFT)
                | (nest << Self::NEST_SHIFT),
        }
    }

    pub(crate) const fn to_scope_id(self) -> ScopeId {
        if self.is_none() {
            ScopeId::none()
        } else {
            ScopeId::compose(
                self.kind(),
                self.local_ordinal(),
                self.range_ordinal(),
                self.nest_ordinal(),
            )
        }
    }

    pub(crate) const fn raw(self) -> u64 {
        self.to_scope_id().raw()
    }

    pub(crate) const fn canonical(self) -> ScopeId {
        if self.is_none() {
            ScopeId::none()
        } else {
            ScopeId::compose(self.kind(), self.local_ordinal(), 0, 0)
        }
    }

    pub(crate) const fn kind(self) -> ScopeKind {
        if self.is_none() {
            return ScopeKind::Generic;
        }
        match ((self.raw >> Self::KIND_SHIFT) & Self::KIND_MASK) as u8 {
            0 => ScopeKind::Generic,
            1 => ScopeKind::Route,
            2 => ScopeKind::Loop,
            3 => ScopeKind::Parallel,
            _ => ScopeKind::Generic,
        }
    }

    pub(crate) const fn local_ordinal(self) -> u16 {
        if self.is_none() {
            return 0;
        }
        ((self.raw >> Self::LOCAL_SHIFT) & Self::ORDINAL_MASK) as u16
    }

    pub(crate) const fn range_ordinal(self) -> u16 {
        if self.is_none() {
            return 0;
        }
        ((self.raw >> Self::RANGE_SHIFT) & Self::ORDINAL_MASK) as u16
    }

    pub(crate) const fn nest_ordinal(self) -> u16 {
        if self.is_none() {
            return 0;
        }
        ((self.raw >> Self::NEST_SHIFT) & Self::ORDINAL_MASK) as u16
    }
}

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
    /// `EPF(Route) -> resolver -> PolicyAbort`.
    ///
    /// The actual control operation (route, splice, reroute) is determined by
    /// the resource tag of the control message, not by the plan itself.
    ///
    /// # Example
    ///
    /// ```ignore
    /// // Define a route with dynamic policy annotation
    /// const MY_POLICY_ID: u16 = 0x1234;
    /// const MY_ROUTE: Program<Steps> =
    ///     g::route(arm1.policy::<MY_POLICY_ID>(), arm2.policy::<MY_POLICY_ID>());
    ///
    /// // Register resolver before use
    /// let controller = hibana::g::advanced::project(&MY_ROUTE);
    /// struct RouteState {
    ///     preferred_arm: u8,
    /// }
    ///
    /// fn resolve_route(
    ///     state: &RouteState,
    ///     _ctx: hibana::substrate::policy::ResolverContext,
    /// ) -> Result<hibana::substrate::policy::DynamicResolution, hibana::substrate::policy::ResolverError> {
    ///     Ok(hibana::substrate::policy::DynamicResolution::RouteArm {
    ///         arm: state.preferred_arm,
    ///     })
    /// }
    ///
    /// let route_state = RouteState { preferred_arm: 0 };
    ///
    /// cluster.set_resolver::<MY_POLICY_ID, 0, _>(
    ///     rv_id,
    ///     &controller,
    ///     hibana::substrate::policy::ResolverRef::from_state(&route_state, resolve_route),
    /// )?;
    /// ```
    ///
    /// [`SessionKit::set_resolver`]: crate::substrate::SessionKit::set_resolver
    /// [`CpError::PolicyAbort`]: crate::substrate::CpError::PolicyAbort
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
    pub(crate) spec: ControlLabelSpec,
}

impl ControlSpecMarker {
    const fn empty() -> Self {
        Self {
            offset: 0,
            spec: ControlLabelSpec::new(
                0,
                0,
                ControlScopeKind::None,
                0,
                CapShot::One,
                ControlHandling::None,
            ),
        }
    }

    const fn new(offset: usize, spec: ControlLabelSpec) -> Self {
        Self { offset, spec }
    }
}

/// Accumulator used to build `EffStruct` sequences in const contexts.
#[derive(Clone, Copy)]
pub struct EffList {
    data: [EffStruct; MAX_CAPACITY],
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

impl Default for EffList {
    fn default() -> Self {
        Self::new()
    }
}

impl EffList {
    /// Create an empty accumulator.
    pub const fn new() -> Self {
        Self {
            data: [EffStruct::pure(); MAX_CAPACITY],
            len: 0,
            scope_budget: 0,
            scope_markers: [ScopeMarker::empty(); MAX_CAPACITY],
            scope_marker_len: 0,
            control_markers: [ControlMarker::empty(); MAX_CAPACITY],
            control_marker_len: 0,
            policy_markers: [PolicyMarker::empty(); MAX_CAPACITY],
            policy_marker_len: 0,
            control_specs: [ControlSpecMarker::empty(); MAX_CAPACITY],
            control_spec_len: 0,
        }
    }

    /// Return the current length.
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Number of structured scopes encoded within this list.
    pub const fn scope_budget(&self) -> u16 {
        self.scope_budget
    }

    /// Whether the accumulator is empty.
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Shift every scope identifier by `offset` ordinals.
    ///
    /// This is the only required linear scan: rebasing changes every scope id.
    pub const fn rebase_scopes(mut self, offset: u16) -> Self {
        if offset == 0 {
            return self;
        }
        let mut idx = 0usize;
        let mut max = 0u16;
        while idx < self.scope_marker_len {
            let marker = self.scope_markers[idx];
            let rebased = marker.scope_id.add_ordinal(offset);
            self.scope_markers[idx] = ScopeMarker {
                offset: marker.offset,
                scope_id: rebased,
                scope_kind: rebased.kind(),
                event: marker.event,
                linger: marker.linger,
                controller_role: marker.controller_role,
            };
            let ordinal = rebased.ordinal();
            if ordinal == ScopeId::ORDINAL_CAPACITY {
                panic!("scope ordinal overflow");
            }
            let next = ordinal + 1;
            if next > max {
                max = next;
            }
            idx += 1;
        }
        let mut policy_idx = 0usize;
        while policy_idx < self.policy_marker_len {
            let marker = self.policy_markers[policy_idx];
            let mut policy = marker.policy;
            let scope = policy.scope();
            if !scope.is_none() {
                let rebased = scope.add_ordinal(offset);
                policy = policy.with_scope(rebased);
            }
            self.policy_markers[policy_idx] = PolicyMarker::new(marker.offset, policy);
            policy_idx += 1;
        }
        self.scope_budget = max;
        self
    }

    /// Borrow the accumulated effects as a slice.
    #[inline(always)]
    pub const fn as_slice(&self) -> &[EffStruct] {
        unsafe { core::slice::from_raw_parts(self.data.as_ptr(), self.len) }
    }

    /// Append a single node to the accumulator.
    pub const fn push(mut self, node: EffStruct) -> Self {
        if self.len >= MAX_CAPACITY {
            panic!("EffList capacity exceeded");
        }
        self.data[self.len] = node;
        self.len += 1;
        self
    }

    /// Extend the accumulator with another `EffList`.
    ///
    /// Linear by construction: offsets and scope metadata must be rebased.
    pub const fn extend_list(mut self, other: EffList) -> Self {
        let mut idx = 0;
        let base = self.len;
        while idx < other.len {
            self = self.push(other.data[idx]);
            idx += 1;
        }
        let mut scope_idx = 0;
        while scope_idx < other.scope_marker_len {
            let marker = other.scope_markers[scope_idx];
            self = self.push_scope_marker_full(
                base + marker.offset,
                marker.scope_id,
                marker.scope_kind,
                marker.event,
                marker.linger,
                marker.controller_role,
            );
            scope_idx += 1;
        }
        let mut ctrl_idx = 0;
        while ctrl_idx < other.control_marker_len {
            let marker = other.control_markers[ctrl_idx];
            self = self.push_control_marker(
                base + marker.offset as usize,
                marker.scope_kind,
                marker.tap_id,
            );
            ctrl_idx += 1;
        }
        let mut policy_idx = 0;
        while policy_idx < other.policy_marker_len {
            let marker = other.policy_markers[policy_idx];
            self = self.push_policy(base + marker.offset, marker.policy);
            policy_idx += 1;
        }
        let mut spec_idx = 0;
        while spec_idx < other.control_spec_len {
            let spec = other.control_specs[spec_idx];
            self = self.push_control_spec(base + spec.offset, spec.spec);
            spec_idx += 1;
        }
        self
    }

    const fn push_scope_marker_raw(
        self,
        offset: usize,
        scope_id: ScopeId,
        scope_kind: ScopeKind,
        event: ScopeEvent,
        linger: bool,
    ) -> Self {
        self.push_scope_marker_full(offset, scope_id, scope_kind, event, linger, None)
    }

    const fn push_scope_marker_full(
        mut self,
        offset: usize,
        scope_id: ScopeId,
        scope_kind: ScopeKind,
        event: ScopeEvent,
        linger: bool,
        controller_role: Option<u8>,
    ) -> Self {
        if self.scope_marker_len >= MAX_CAPACITY {
            panic!("EffList scope marker capacity exceeded");
        }
        let ordinal = scope_id.ordinal();
        if ordinal == ScopeId::ORDINAL_CAPACITY {
            panic!("scope ordinal overflow");
        }
        let next = ordinal + 1;
        if next > self.scope_budget {
            self.scope_budget = next;
        }
        let mut idx = self.scope_marker_len;
        while idx > 0 {
            let prev = self.scope_markers[idx - 1];
            if prev.offset > offset {
                self.scope_markers[idx] = prev;
                idx -= 1;
            } else {
                break;
            }
        }
        self.scope_markers[idx] = ScopeMarker {
            offset,
            scope_id,
            scope_kind,
            event,
            linger,
            controller_role,
        };
        self.scope_marker_len += 1;
        self
    }

    const fn push_scope_marker(self, offset: usize, scope: ScopeId, event: ScopeEvent) -> Self {
        self.push_scope_marker_raw(offset, scope, scope.kind(), event, false)
    }

    const fn push_scope_marker_outer_enter(self, offset: usize, scope: ScopeId) -> Self {
        self.push_scope_marker_outer_enter_linger(offset, scope, false)
    }

    const fn push_scope_marker_outer_enter_linger(
        mut self,
        offset: usize,
        scope: ScopeId,
        linger: bool,
    ) -> Self {
        if self.scope_marker_len >= MAX_CAPACITY {
            panic!("EffList scope marker capacity exceeded");
        }
        let ordinal = scope.ordinal();
        if ordinal == ScopeId::ORDINAL_CAPACITY {
            panic!("scope ordinal overflow");
        }
        let next = ordinal + 1;
        if next > self.scope_budget {
            self.scope_budget = next;
        }
        let mut idx = self.scope_marker_len;
        while idx > 0 {
            let prev = self.scope_markers[idx - 1];
            if prev.offset >= offset {
                self.scope_markers[idx] = prev;
                idx -= 1;
            } else {
                break;
            }
        }
        self.scope_markers[idx] = ScopeMarker {
            offset,
            scope_id: scope,
            scope_kind: scope.kind(),
            event: ScopeEvent::Enter,
            linger,
            controller_role: None,
        };
        self.scope_marker_len += 1;
        self
    }

    pub const fn push_control_marker(
        mut self,
        offset: usize,
        scope_kind: ControlScopeKind,
        tap_id: u16,
    ) -> Self {
        if self.control_marker_len >= MAX_CAPACITY {
            panic!("EffList control marker capacity exceeded");
        }
        self.control_markers[self.control_marker_len] = ControlMarker {
            offset: ControlMarker::encode_offset(offset),
            scope_kind,
            tap_id,
        };
        self.control_marker_len += 1;
        self
    }

    pub const fn with_scope(self, scope: ScopeId) -> Self {
        let len = self.len;
        let scoped = self.push_scope_marker_outer_enter(0, scope);
        scoped.push_scope_marker(len, scope, ScopeEvent::Exit)
    }

    /// Wrap the effect list with a Route scope that has controller role information.
    /// Used by binary `route(left, right)` after deriving the controller from the arm entry.
    pub(crate) const fn with_scope_controller(self, scope: ScopeId, controller_role: u8) -> Self {
        // Use with_scope for correct marker ordering, then update controller_role
        self.with_scope(scope)
            .with_scope_controller_role(scope, controller_role)
    }

    /// Update controller_role for all markers with the given scope_id.
    pub(crate) const fn with_scope_controller_role(
        self,
        scope: ScopeId,
        controller_role: u8,
    ) -> Self {
        self.update_scope_markers(scope, None, Some(controller_role))
    }

    pub(crate) const fn with_scope_linger(self, scope: ScopeId, linger: bool) -> Self {
        self.update_scope_markers(scope, Some(linger), None)
    }

    pub const fn scope_has_linger(&self, scope: ScopeId) -> bool {
        if scope.is_none() {
            return false;
        }
        let mut marker_idx = 0usize;
        while marker_idx < self.scope_marker_len {
            let marker = self.scope_markers[marker_idx];
            if marker.scope_id.raw() == scope.raw() && marker.linger {
                return true;
            }
            marker_idx += 1;
        }
        false
    }

    pub(crate) const fn with_control(self, scope_kind: ControlScopeKind, tap_id: u16) -> Self {
        self.push_control_marker(0, scope_kind, tap_id)
    }

    pub(crate) const fn with_policy(self, policy: PolicyMode) -> Self {
        if self.len == 0 {
            panic!("EffList is empty");
        }
        self.push_policy(self.len - 1, policy)
    }

    pub const fn with_control_spec(self, spec: ControlLabelSpec) -> Self {
        if self.len == 0 {
            panic!("EffList is empty");
        }
        self.push_control_spec(self.len - 1, spec)
    }

    pub(crate) const fn push_policy(mut self, offset: usize, policy: PolicyMode) -> Self {
        if offset >= MAX_CAPACITY {
            panic!("EffList policy marker offset out of bounds");
        }
        let mut idx = 0usize;
        while idx < self.policy_marker_len {
            if self.policy_markers[idx].offset == offset {
                self.policy_markers[idx] = PolicyMarker::new(offset, policy);
                return self;
            }
            idx += 1;
        }
        if self.policy_marker_len >= MAX_CAPACITY {
            panic!("EffList policy marker capacity exceeded");
        }
        self.policy_markers[self.policy_marker_len] = PolicyMarker::new(offset, policy);
        self.policy_marker_len += 1;
        self
    }

    pub(crate) const fn policy_at(&self, offset: usize) -> Option<PolicyMode> {
        if offset >= MAX_CAPACITY {
            return None;
        }
        let mut idx = 0usize;
        while idx < self.policy_marker_len {
            let marker = self.policy_markers[idx];
            if marker.offset == offset {
                return Some(marker.policy);
            }
            idx += 1;
        }
        None
    }

    pub(crate) const fn scope_id_for_offset(&self, offset: usize) -> Option<ScopeId> {
        if offset >= MAX_CAPACITY {
            return None;
        }
        let mut stack = [ScopeId::none(); MAX_CAPACITY];
        let mut stack_len = 0usize;
        let mut marker_idx = 0usize;
        while marker_idx < self.scope_marker_len {
            let marker = self.scope_markers[marker_idx];
            if marker.offset > offset {
                break;
            }
            match marker.event {
                ScopeEvent::Enter => {
                    if stack_len >= MAX_CAPACITY {
                        panic!("EffList scope stack overflow");
                    }
                    stack[stack_len] = marker.scope_id;
                    stack_len += 1;
                }
                ScopeEvent::Exit => {
                    if stack_len > 0 {
                        stack_len -= 1;
                    }
                }
            }
            marker_idx += 1;
        }
        if stack_len == 0 {
            None
        } else {
            Some(stack[stack_len - 1])
        }
    }

    /// Update scope markers by ordinal-indexed lists (no full scan).
    const fn update_scope_markers(
        mut self,
        scope: ScopeId,
        linger: Option<bool>,
        controller_role: Option<u8>,
    ) -> Self {
        if scope.is_none() {
            return self;
        }
        let mut marker_idx = 0usize;
        while marker_idx < self.scope_marker_len {
            let marker = self.scope_markers[marker_idx];
            if marker.scope_id.raw() == scope.raw() {
                let mut updated = marker;
                if let Some(value) = linger {
                    updated.linger = value;
                }
                if let Some(role) = controller_role {
                    updated.controller_role = Some(role);
                }
                self.scope_markers[marker_idx] = updated;
            }
            marker_idx += 1;
        }
        self
    }

    pub(crate) const fn policy_with_scope(&self, offset: usize) -> Option<(PolicyMode, ScopeId)> {
        match self.policy_at(offset) {
            Some(policy) => {
                let scope = match self.scope_id_for_offset(offset) {
                    Some(scope) => scope,
                    None => ScopeId::none(),
                };
                Some((policy.with_scope(scope), scope))
            }
            None => None,
        }
    }

    pub(crate) const fn push_control_spec(mut self, offset: usize, spec: ControlLabelSpec) -> Self {
        if offset >= MAX_CAPACITY {
            panic!("EffList control spec offset out of bounds");
        }
        let mut idx = 0usize;
        while idx < self.control_spec_len {
            if self.control_specs[idx].offset == offset {
                self.control_specs[idx] = ControlSpecMarker::new(offset, spec);
                return self;
            }
            idx += 1;
        }
        if self.control_spec_len >= MAX_CAPACITY {
            panic!("EffList control spec capacity exceeded");
        }
        self.control_specs[self.control_spec_len] = ControlSpecMarker::new(offset, spec);
        self.control_spec_len += 1;
        self
    }

    pub const fn control_spec_at(&self, offset: usize) -> Option<ControlLabelSpec> {
        if offset >= MAX_CAPACITY {
            return None;
        }
        let mut idx = 0usize;
        while idx < self.control_spec_len {
            let marker = self.control_specs[idx];
            if marker.offset == offset {
                return Some(marker.spec);
            }
            idx += 1;
        }
        None
    }

    pub const fn scope_markers(&self) -> &[ScopeMarker] {
        unsafe { core::slice::from_raw_parts(self.scope_markers.as_ptr(), self.scope_marker_len) }
    }

    pub const fn control_markers(&self) -> &[ControlMarker] {
        unsafe {
            core::slice::from_raw_parts(self.control_markers.as_ptr(), self.control_marker_len)
        }
    }
}

impl core::ops::Deref for EffList {
    type Target = [EffStruct];

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl AsRef<[EffStruct]> for EffList {
    #[inline(always)]
    fn as_ref(&self) -> &[EffStruct] {
        self.as_slice()
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlScopeKind {
    None = 0,
    Loop = 1,
    Checkpoint = 2,
    Cancel = 3,
    Splice = 4,
    Reroute = 5,
    Policy = 6,
    Route = 7,
}

const fn is_reserved_control_label(label: u8) -> bool {
    match label {
        crate::runtime::consts::LABEL_LOOP_CONTINUE
        | crate::runtime::consts::LABEL_LOOP_BREAK
        | crate::runtime::consts::LABEL_SPLICE_INTENT
        | crate::runtime::consts::LABEL_SPLICE_ACK
        | crate::runtime::consts::LABEL_REROUTE
        | crate::runtime::consts::LABEL_ROUTE_DECISION
        | crate::runtime::consts::LABEL_POLICY_LOAD
        | crate::runtime::consts::LABEL_POLICY_ACTIVATE
        | crate::runtime::consts::LABEL_POLICY_REVERT
        | crate::runtime::consts::LABEL_POLICY_ANNOTATE
        | crate::runtime::consts::LABEL_CANCEL
        | crate::runtime::consts::LABEL_CHECKPOINT
        | crate::runtime::consts::LABEL_COMMIT
        | crate::runtime::consts::LABEL_ROLLBACK
        | crate::runtime::consts::LABEL_MGMT_LOAD_BEGIN
        | crate::runtime::consts::LABEL_MGMT_LOAD_COMMIT => true,
        _ => false,
    }
}

/// Construct a single send atom using type-level roles with lane parameter.
pub(crate) const fn const_send_typed<From, To, M, const LANE: u8>() -> EffList
where
    From: RoleMarker,
    To: RoleMarker,
    M: MessageSpec + SendableLabel + crate::global::MessageControlSpec,
{
    let label = <M as MessageSpec>::LABEL;
    if label > crate::runtime::consts::LABEL_MAX {
        panic!("label exceeds universe");
    }
    if !<M as MessageControlSpec>::IS_CONTROL && is_reserved_control_label(label) {
        panic!("control labels require capability payloads");
    }
    let spec = if <M as MessageControlSpec>::IS_CONTROL {
        Some(<M as MessageControlSpec>::CONTROL_SPEC)
    } else {
        None
    };
    let atom = eff::EffAtom {
        from: From::INDEX,
        to: To::INDEX,
        label,
        is_control: spec.is_some(),
        resource: match spec {
            Some(rule) => Some(rule.resource_tag),
            None => None,
        },
        lane: LANE,
    };
    let mut list = EffList::new().push(EffStruct::atom(atom));
    if let Some(rule) = spec {
        list = list.with_control(rule.scope_kind, rule.tap_id);
        list = list.with_control_spec(rule);
        list = list.with_policy(PolicyMode::static_mode());
    }
    list
}

#[cfg(test)]
mod tests {
    use core::mem::size_of;

    use super::{CompactScopeId, ControlMarker, EffList, ScopeId, ScopeKind};
    use crate::g;
    use crate::g::advanced::CanonicalControl;
    use crate::g::advanced::steps::{
        PolicySteps, RouteSteps, SendStep, SeqSteps, StepCons, StepNil,
    };
    use crate::runtime::consts::{LABEL_LOOP_BREAK, LABEL_LOOP_CONTINUE};
    use crate::substrate::cap::GenericCapToken;
    use crate::substrate::cap::advanced::{LoopBreakKind, LoopContinueKind};

    const LOOP_POLICY_ID: u16 = 120;
    type LoopContinueHead = PolicySteps<
        StepCons<
            SendStep<
                g::Role<0>,
                g::Role<0>,
                g::Msg<
                    { LABEL_LOOP_CONTINUE },
                    GenericCapToken<LoopContinueKind>,
                    CanonicalControl<LoopContinueKind>,
                >,
            >,
            StepNil,
        >,
        LOOP_POLICY_ID,
    >;
    type LoopBreakHead = PolicySteps<
        StepCons<
            SendStep<
                g::Role<0>,
                g::Role<0>,
                g::Msg<
                    { LABEL_LOOP_BREAK },
                    GenericCapToken<LoopBreakKind>,
                    CanonicalControl<LoopBreakKind>,
                >,
            >,
            StepNil,
        >,
        LOOP_POLICY_ID,
    >;
    type LoopContinueProgram = SeqSteps<
        LoopContinueHead,
        StepCons<SendStep<g::Role<0>, g::Role<1>, g::Msg<1, u32>>, StepNil>,
    >;
    type LoopDecisionProgram = RouteSteps<LoopContinueProgram, LoopBreakHead>;

    #[test]
    fn control_marker_stays_compact() {
        assert!(
            size_of::<ControlMarker>() <= 8,
            "ControlMarker regressed to a wide offset layout: {} bytes",
            size_of::<ControlMarker>()
        );
    }

    #[test]
    fn compact_scope_id_roundtrips_scope_id() {
        let scope = ScopeId::compose(ScopeKind::Route, 256, 255, 254);
        let compact = CompactScopeId::from_scope_id(scope);
        assert_eq!(compact.to_scope_id(), scope);
        assert_eq!(CompactScopeId::none().to_scope_id(), ScopeId::none());
        assert!(
            size_of::<CompactScopeId>() <= 4,
            "CompactScopeId regressed beyond its packed u32 storage: {} bytes",
            size_of::<CompactScopeId>()
        );
    }

    const LOOP_BODY: g::Program<
        StepCons<SendStep<g::Role<0>, g::Role<1>, g::Msg<1, u32>>, StepNil>,
    > = g::send::<g::Role<0>, g::Role<1>, g::Msg<1, u32>, 0>();
    const LOOP_BREAK_ARM: g::Program<LoopBreakHead> = g::send::<
        g::Role<0>,
        g::Role<0>,
        g::Msg<
            { LABEL_LOOP_BREAK },
            GenericCapToken<LoopBreakKind>,
            CanonicalControl<LoopBreakKind>,
        >,
        0,
    >()
    .policy::<LOOP_POLICY_ID>();
    const LOOP_CONTINUE_ARM: g::Program<LoopContinueProgram> = g::seq(
        g::send::<
            g::Role<0>,
            g::Role<0>,
            g::Msg<
                { LABEL_LOOP_CONTINUE },
                GenericCapToken<LoopContinueKind>,
                CanonicalControl<LoopContinueKind>,
            >,
            0,
        >()
        .policy::<LOOP_POLICY_ID>(),
        LOOP_BODY,
    );
    const LOOP_DECISION: g::Program<LoopDecisionProgram> =
        g::route(LOOP_CONTINUE_ARM, LOOP_BREAK_ARM);

    #[test]
    fn policy_scope_stays_internal() {
        let list: &EffList = LOOP_DECISION.eff_list();
        let mut policies = 0usize;
        let mut offset = 0usize;
        while offset < list.len() {
            if list.policy_at(offset).is_some() {
                policies += 1;
                let (_, scope) = list
                    .policy_with_scope(offset)
                    .expect("policy scope should be derivable");
                assert!(!scope.is_none(), "loop policy should expose a scope id");
                assert_eq!(scope.kind(), ScopeKind::Route, "loop scope kind matches");
            }
            offset += 1;
        }
        assert!(
            policies >= 2,
            "loop continue/break policies should be present"
        );
    }
}
