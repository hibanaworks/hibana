//! Const helpers for building `EffStruct` slices at compile time.
//!
//! These helpers progressively migrate the global combinators (`send/seq/par/route`)
//! toward a const-only surface. They provide an `EffList` accumulator that can
//! be populated entirely within const contexts and exposed as an `EffStruct`
//! slice via standard slice traits.

use crate::control::cap::CapShot;
use crate::eff::{self, EffSlice, EffStruct};
use crate::global::{ControlHandling, ControlLabelSpec, MessageControlSpec};

const MAX_CAPACITY: usize = eff::meta::MAX_EFF_NODES;

/// Static policy metadata propagated alongside dynamic handle plans.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DynamicMeta {
    pub shard_hint: Option<u32>,
    pub static_weight: u8,
}

impl Default for DynamicMeta {
    fn default() -> Self {
        Self::new()
    }
}

impl DynamicMeta {
    pub const fn new() -> Self {
        Self {
            shard_hint: None,
            static_weight: 0,
        }
    }

    pub const fn with_shard_hint(self, shard_hint: Option<u32>) -> Self {
        Self { shard_hint, ..self }
    }

    pub const fn with_static_weight(self, weight: u8) -> Self {
        Self {
            static_weight: weight,
            ..self
        }
    }
}

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

    pub const fn compose(kind: ScopeKind, local: u16, range: u16, nest: u16) -> Self {
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

    pub const fn new(kind: ScopeKind, local: u16) -> Self {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StaticPlanKind {
    SpliceLocal { dst_lane: u16 },
    RerouteLocal { dst_lane: u16, shard: u32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandlePlan {
    None,
    Static {
        kind: StaticPlanKind,
    },
    Dynamic {
        policy_id: u16,
        meta: DynamicMeta,
        scope: ScopeId,
    },
}

impl HandlePlan {
    pub const fn none() -> Self {
        Self::None
    }

    pub const fn splice_local(dst_lane: u16) -> Self {
        Self::Static {
            kind: StaticPlanKind::SpliceLocal { dst_lane },
        }
    }

    pub const fn reroute_local(dst_lane: u16, shard: u32) -> Self {
        Self::Static {
            kind: StaticPlanKind::RerouteLocal { dst_lane, shard },
        }
    }

    /// Create a dynamic control plan with the given policy_id and metadata.
    ///
    /// **IMPORTANT**: Using a dynamic plan requires registering a resolver via
    /// [`SessionCluster::register_control_plan_resolver`] before executing the
    /// choreography. If no resolver is registered for the policy_id, the
    /// operation will fail with [`CpError::PolicyAbort`] at runtime.
    ///
    /// The actual control operation (route, splice, reroute) is determined by
    /// the resource tag of the control message, not by the plan itself.
    ///
    /// # Example
    ///
    /// ```ignore
    /// // Define a route with dynamic plan
    /// const MY_POLICY_ID: u16 = 0x1234;
    /// const MY_ROUTE: Program<Steps> = g::route(
    ///     g::route_chain::<0, 0, Arm1>(
    ///         g::with_control_plan(arm1, HandlePlan::dynamic(MY_POLICY_ID, DynamicMeta::new()))
    ///     ).and(arm2)
    /// );
    ///
    /// // Register resolver before use
    /// cluster.register_control_plan_resolver(rv_id, &info, |ctx| {
    ///     // Return selected arm index
    ///     Ok(DynamicResolution::arm(0))
    /// })?;
    /// ```
    ///
    /// [`SessionCluster::register_control_plan_resolver`]: crate::control::cluster::SessionCluster::register_control_plan_resolver
    /// [`CpError::PolicyAbort`]: crate::control::CpError::PolicyAbort
    pub const fn dynamic(policy_id: u16, meta: DynamicMeta) -> Self {
        Self::Dynamic {
            policy_id,
            meta,
            scope: ScopeId::none(),
        }
    }

    pub const fn static_kind(self) -> Option<StaticPlanKind> {
        match self {
            Self::Static { kind } => Some(kind),
            _ => None,
        }
    }

    pub const fn is_dynamic(self) -> bool {
        matches!(self, Self::Dynamic { .. })
    }

    pub const fn is_none(self) -> bool {
        matches!(self, Self::None)
    }

    pub const fn dynamic_components(self) -> Option<(u16, DynamicMeta)> {
        match self {
            Self::Dynamic {
                policy_id, meta, ..
            } => Some((policy_id, meta)),
            _ => None,
        }
    }

    pub const fn scope(self) -> ScopeId {
        match self {
            Self::Dynamic { scope, .. } => scope,
            _ => ScopeId::none(),
        }
    }

    pub const fn with_scope(self, scope: ScopeId) -> Self {
        match self {
            Self::Dynamic {
                policy_id, meta, ..
            } => Self::Dynamic {
                policy_id,
                meta,
                scope,
            },
            other => other,
        }
    }
}

#[derive(Clone, Copy)]
pub struct ControlPlanMarker {
    pub offset: usize,
    pub plan: HandlePlan,
}

impl ControlPlanMarker {
    const fn empty() -> Self {
        Self {
            offset: 0,
            plan: HandlePlan::None,
        }
    }

    const fn new(offset: usize, plan: HandlePlan) -> Self {
        Self { offset, plan }
    }
}

const SCOPE_MARKER_INDEX_NONE: u16 = u16::MAX;

#[derive(Clone, Copy)]
struct DynamicPlanInfo {
    plan: HandlePlan,
    offset: u16,
    tag: u8,
}

#[derive(Clone, Copy)]
pub struct ScopeMarker {
    pub offset: usize,
    pub scope_id: ScopeId,
    pub scope_kind: ScopeKind,
    pub event: ScopeEvent,
    pub linger: bool,
    /// Controller role for Route scopes (from `route_chain<CONTROLLER>`).
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
    pub offset: usize,
    pub scope_kind: ControlScopeKind,
    pub tap_id: u16,
}

impl ControlMarker {
    pub const fn empty() -> Self {
        Self {
            offset: 0,
            scope_kind: ControlScopeKind::None,
            tap_id: 0,
        }
    }
}

#[derive(Clone, Copy)]
pub struct ControlSpecMarker {
    pub offset: usize,
    pub spec: ControlLabelSpec,
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
    scope_by_offset: [ScopeId; MAX_CAPACITY],
    scope_linger_by_ordinal: [bool; ScopeId::ORDINAL_CAPACITY as usize],
    scope_marker_head_by_ordinal: [u16; ScopeId::ORDINAL_CAPACITY as usize],
    scope_marker_next: [u16; MAX_CAPACITY],
    control_markers: [ControlMarker; MAX_CAPACITY],
    control_marker_len: usize,
    control_plans: [ControlPlanMarker; MAX_CAPACITY],
    control_plan_len: usize,
    control_plan_by_offset: [Option<HandlePlan>; MAX_CAPACITY],
    control_plan_index_by_offset: [u16; MAX_CAPACITY],
    dynamic_plan_from_offset: [Option<DynamicPlanInfo>; MAX_CAPACITY],
    control_specs: [ControlSpecMarker; MAX_CAPACITY],
    control_spec_len: usize,
    control_spec_by_offset: [Option<ControlLabelSpec>; MAX_CAPACITY],
    control_spec_index_by_offset: [u16; MAX_CAPACITY],
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
            scope_by_offset: [ScopeId::none(); MAX_CAPACITY],
            scope_linger_by_ordinal: [false; ScopeId::ORDINAL_CAPACITY as usize],
            scope_marker_head_by_ordinal: [SCOPE_MARKER_INDEX_NONE; ScopeId::ORDINAL_CAPACITY as usize],
            scope_marker_next: [SCOPE_MARKER_INDEX_NONE; MAX_CAPACITY],
            control_markers: [ControlMarker::empty(); MAX_CAPACITY],
            control_marker_len: 0,
            control_plans: [ControlPlanMarker::empty(); MAX_CAPACITY],
            control_plan_len: 0,
            control_plan_by_offset: [None; MAX_CAPACITY],
            control_plan_index_by_offset: [SCOPE_MARKER_INDEX_NONE; MAX_CAPACITY],
            dynamic_plan_from_offset: [None; MAX_CAPACITY],
            control_specs: [ControlSpecMarker::empty(); MAX_CAPACITY],
            control_spec_len: 0,
            control_spec_by_offset: [None; MAX_CAPACITY],
            control_spec_index_by_offset: [SCOPE_MARKER_INDEX_NONE; MAX_CAPACITY],
        }
    }

    /// Populate the accumulator from an existing `EffSlice`.
    pub const fn from_slice(list: EffSlice) -> Self {
        let mut acc = Self::new();
        let mut idx = 0;
        while idx < list.len() {
            acc = acc.push(list[idx]);
            idx += 1;
        }
        acc
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

    /// Access an element by index (const-compatible).
    pub const fn at(&self, idx: usize) -> EffStruct {
        self.data[idx]
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
        let mut plan_idx = 0usize;
        while plan_idx < self.control_plan_len {
            let marker = self.control_plans[plan_idx];
            let mut plan = marker.plan;
            let scope = plan.scope();
            if !scope.is_none() {
                let rebased = scope.add_ordinal(offset);
                plan = plan.with_scope(rebased);
            }
            self.control_plans[plan_idx] = ControlPlanMarker::new(marker.offset, plan);
            if marker.offset < MAX_CAPACITY {
                self.control_plan_by_offset[marker.offset] = Some(plan);
            }
            plan_idx += 1;
        }
        self.scope_budget = max;
        self.rebuild_scope_by_offset().rebuild_dynamic_plan_index()
    }

    /// Return the last atom contained in the list (panic if empty or last node not atom).
    pub const fn last_atom(&self) -> eff::EffAtom {
        if self.len == 0 {
            panic!("EffList is empty");
        }
        let node = self.data[self.len - 1];
        if !matches!(node.kind, eff::EffKind::Atom) {
            panic!("EffList does not end with an atom");
        }
        node.atom_data()
    }

    /// Return the first atom contained in the list (panic if the list is empty or does not start with an atom).
    pub const fn first_atom(&self) -> eff::EffAtom {
        if self.len == 0 {
            panic!("EffList is empty");
        }
        let node = self.data[0];
        if !matches!(node.kind, eff::EffKind::Atom) {
            panic!("EffList does not start with an atom");
        }
        node.atom_data()
    }

    /// Borrow the accumulated effects as a slice.
    #[inline(always)]
    pub fn as_slice(&self) -> &[EffStruct] {
        &self.data[..self.len]
    }

    /// Borrow the accumulated effects as a `'static` slice.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `self` lives for the `'static` lifetime
    /// (e.g., a `pub static` value).
    #[inline(always)]
    pub const fn as_static_slice(&'static self) -> EffSlice {
        unsafe { core::slice::from_raw_parts(self.data.as_ptr(), self.len) }
    }

    #[inline(always)]
    pub const fn node_at(&self, offset: usize) -> crate::eff::EffStruct {
        if offset >= self.len {
            panic!("EffList::node_at offset out of bounds");
        }
        self.data[offset]
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

    /// Runtime-only push that mutates the accumulator in place to avoid large copies.
    pub fn push_mut(&mut self, node: EffStruct) {
        if self.len >= MAX_CAPACITY {
            panic!("EffList capacity exceeded");
        }
        self.data[self.len] = node;
        self.len += 1;
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
            self = self.push_control_marker(base + marker.offset, marker.scope_kind, marker.tap_id);
            ctrl_idx += 1;
        }
        let mut plan_idx = 0;
        while plan_idx < other.control_plan_len {
            let plan = other.control_plans[plan_idx];
            self = self.push_control_plan(base + plan.offset, plan.plan);
            plan_idx += 1;
        }
        let mut spec_idx = 0;
        while spec_idx < other.control_spec_len {
            let spec = other.control_specs[spec_idx];
            self = self.push_control_spec(base + spec.offset, spec.spec);
            spec_idx += 1;
        }
        self
    }

    /// Merge multiple accumulators into one.
    ///
    /// Linear in total size: each list is rebased and appended.
    pub const fn merge_lists<const N: usize>(items: [EffList; N]) -> Self {
        if N == 0 {
            panic!("const_merge requires at least one EffList");
        }
        let mut acc = Self::new();
        let mut i = 0;
        while i < N {
            let list = items[i];
            if list.len == 0 {
                panic!("EffList slice must not be empty");
            }
            acc = acc.extend_list(list);
            i += 1;
        }
        acc
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
        self.rebuild_scope_by_offset()
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
        self.rebuild_scope_by_offset()
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
            offset,
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
    /// Used by `route_chain` to propagate the CONTROLLER const parameter.
    pub const fn with_scope_controller(self, scope: ScopeId, controller_role: u8) -> Self {
        // Use with_scope for correct marker ordering, then update controller_role
        self.with_scope(scope).with_scope_controller_role(scope, controller_role)
    }

    /// Update controller_role for all markers with the given scope_id.
    pub const fn with_scope_controller_role(self, scope: ScopeId, controller_role: u8) -> Self {
        self.update_scope_markers(scope, None, Some(controller_role))
    }

    pub const fn with_scope_linger(self, scope: ScopeId, linger: bool) -> Self {
        self.update_scope_markers(scope, Some(linger), None)
    }

    pub const fn scope_has_linger(&self, scope: ScopeId) -> bool {
        if scope.is_none() {
            return false;
        }
        let canonical = scope.canonical();
        let ordinal = canonical.local_ordinal() as usize;
        if ordinal >= ScopeId::ORDINAL_CAPACITY as usize {
            return false;
        }
        self.scope_linger_by_ordinal[ordinal]
    }

    pub const fn with_control(self, scope_kind: ControlScopeKind, tap_id: u16) -> Self {
        self.push_control_marker(0, scope_kind, tap_id)
    }

    pub(crate) const fn with_control_plan(self, plan: HandlePlan) -> Self {
        if self.len == 0 {
            panic!("EffList is empty");
        }
        self.push_control_plan(self.len - 1, plan)
    }

    pub const fn with_control_spec(self, spec: ControlLabelSpec) -> Self {
        if self.len == 0 {
            panic!("EffList is empty");
        }
        self.push_control_spec(self.len - 1, spec)
    }

    pub const fn push_control_plan(mut self, offset: usize, plan: HandlePlan) -> Self {
        if offset >= MAX_CAPACITY {
            panic!("EffList control plan offset out of bounds");
        }
        let plan_idx = self.control_plan_index_by_offset[offset];
        if plan_idx != SCOPE_MARKER_INDEX_NONE {
            let idx = plan_idx as usize;
            self.control_plans[idx] = ControlPlanMarker::new(offset, plan);
            self.control_plan_by_offset[offset] = Some(plan);
            return self.rebuild_dynamic_plan_index();
        }
        if self.control_plan_len >= MAX_CAPACITY {
            panic!("EffList control plan capacity exceeded");
        }
        self.control_plans[self.control_plan_len] = ControlPlanMarker::new(offset, plan);
        self.control_plan_index_by_offset[offset] = self.control_plan_len as u16;
        self.control_plan_len += 1;
        self.control_plan_by_offset[offset] = Some(plan);
        self.rebuild_dynamic_plan_index()
    }

    pub fn push_control_plan_mut(&mut self, offset: usize, plan: HandlePlan) {
        if offset >= MAX_CAPACITY {
            panic!("EffList control plan offset out of bounds");
        }
        let plan_idx = self.control_plan_index_by_offset[offset];
        if plan_idx != SCOPE_MARKER_INDEX_NONE {
            let idx = plan_idx as usize;
            self.control_plans[idx] = ControlPlanMarker::new(offset, plan);
            self.control_plan_by_offset[offset] = Some(plan);
            self.rebuild_dynamic_plan_index_mut();
            return;
        }
        if self.control_plan_len >= MAX_CAPACITY {
            panic!("EffList control plan capacity exceeded");
        }
        self.control_plans[self.control_plan_len] = ControlPlanMarker::new(offset, plan);
        self.control_plan_index_by_offset[offset] = self.control_plan_len as u16;
        self.control_plan_len += 1;
        self.control_plan_by_offset[offset] = Some(plan);
        self.rebuild_dynamic_plan_index_mut();
    }

    pub const fn control_plan_at(&self, offset: usize) -> Option<HandlePlan> {
        if offset >= MAX_CAPACITY {
            return None;
        }
        self.control_plan_by_offset[offset]
    }

    /// Find the first Dynamic control plan within an offset range [start, end).
    ///
    /// Returns (plan, eff_offset, resource_tag) if found.
    /// Used at scope Enter to set route_plan independent of role projection.
    pub const fn first_dynamic_plan_in_range(
        &self,
        scope_start: usize,
        scope_end: usize,
    ) -> Option<(HandlePlan, usize, u8)> {
        if scope_start >= MAX_CAPACITY {
            return None;
        }
        let entry = match self.dynamic_plan_from_offset[scope_start] {
            Some(entry) => entry,
            None => return None,
        };
        let offset = entry.offset as usize;
        if offset >= scope_end {
            return None;
        }
        Some((entry.plan, offset, entry.tag))
    }

    pub fn scope_id_for_offset(&self, offset: usize) -> Option<ScopeId> {
        if offset >= MAX_CAPACITY {
            return None;
        }
        let scope = self.scope_by_offset[offset];
        if scope.is_none() {
            None
        } else {
            Some(scope)
        }
    }

    const fn rebuild_scope_by_offset(mut self) -> Self {
        let mut table = [ScopeId::none(); MAX_CAPACITY];
        let mut linger_by_ordinal = [false; ScopeId::ORDINAL_CAPACITY as usize];
        let mut head_by_ordinal = [SCOPE_MARKER_INDEX_NONE; ScopeId::ORDINAL_CAPACITY as usize];
        let mut next_by_index = [SCOPE_MARKER_INDEX_NONE; MAX_CAPACITY];
        let mut marker_idx = 0usize;
        while marker_idx < self.scope_marker_len {
            let marker = self.scope_markers[marker_idx];
            if !marker.scope_id.is_none() {
                let ordinal = marker.scope_id.local_ordinal() as usize;
                if ordinal >= ScopeId::ORDINAL_CAPACITY as usize {
                    panic!("scope ordinal overflow");
                }
                if marker.linger {
                    linger_by_ordinal[ordinal] = true;
                }
                next_by_index[marker_idx] = head_by_ordinal[ordinal];
                head_by_ordinal[ordinal] = marker_idx as u16;
            }
            marker_idx += 1;
        }
        let mut stack = [ScopeId::none(); MAX_CAPACITY];
        let mut stack_len = 0usize;
        let mut marker_idx = 0usize;
        let mut offset = 0usize;
        while offset < MAX_CAPACITY {
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
            table[offset] = if stack_len == 0 {
                ScopeId::none()
            } else {
                stack[stack_len - 1]
            };
            offset += 1;
        }
        self.scope_by_offset = table;
        self.scope_linger_by_ordinal = linger_by_ordinal;
        self.scope_marker_head_by_ordinal = head_by_ordinal;
        self.scope_marker_next = next_by_index;
        self
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
        let canonical = scope.canonical();
        let ordinal = canonical.local_ordinal() as usize;
        if ordinal >= ScopeId::ORDINAL_CAPACITY as usize {
            return self;
        }
        if let Some(value) = linger {
            self.scope_linger_by_ordinal[ordinal] = value;
        }
        let mut marker_idx = self.scope_marker_head_by_ordinal[ordinal];
        while marker_idx != SCOPE_MARKER_INDEX_NONE {
            let marker = self.scope_markers[marker_idx as usize];
            if marker.scope_id.raw() == scope.raw() {
                let mut updated = marker;
                if let Some(value) = linger {
                    updated.linger = value;
                }
                if let Some(role) = controller_role {
                    updated.controller_role = Some(role);
                }
                self.scope_markers[marker_idx as usize] = updated;
            }
            marker_idx = self.scope_marker_next[marker_idx as usize];
        }
        self
    }

    pub fn control_plan_with_scope(&self, offset: usize) -> Option<(HandlePlan, ScopeId)> {
        let plan = self.control_plan_at(offset)?;
        let scope = self
            .scope_id_for_offset(offset)
            .unwrap_or_else(ScopeId::none);
        Some((plan.with_scope(scope), scope))
    }


    pub const fn push_control_spec(mut self, offset: usize, spec: ControlLabelSpec) -> Self {
        if offset >= MAX_CAPACITY {
            panic!("EffList control spec offset out of bounds");
        }
        let spec_idx = self.control_spec_index_by_offset[offset];
        if spec_idx != SCOPE_MARKER_INDEX_NONE {
            let idx = spec_idx as usize;
            self.control_specs[idx] = ControlSpecMarker::new(offset, spec);
            self.control_spec_by_offset[offset] = Some(spec);
            return self;
        }
        if self.control_spec_len >= MAX_CAPACITY {
            panic!("EffList control spec capacity exceeded");
        }
        self.control_specs[self.control_spec_len] = ControlSpecMarker::new(offset, spec);
        self.control_spec_index_by_offset[offset] = self.control_spec_len as u16;
        self.control_spec_len += 1;
        self.control_spec_by_offset[offset] = Some(spec);
        self
    }

    pub fn push_control_spec_mut(&mut self, offset: usize, spec: ControlLabelSpec) {
        if offset >= MAX_CAPACITY {
            panic!("EffList control spec offset out of bounds");
        }
        let spec_idx = self.control_spec_index_by_offset[offset];
        if spec_idx != SCOPE_MARKER_INDEX_NONE {
            let idx = spec_idx as usize;
            self.control_specs[idx] = ControlSpecMarker::new(offset, spec);
            self.control_spec_by_offset[offset] = Some(spec);
            return;
        }
        if self.control_spec_len >= MAX_CAPACITY {
            panic!("EffList control spec capacity exceeded");
        }
        self.control_specs[self.control_spec_len] = ControlSpecMarker::new(offset, spec);
        self.control_spec_index_by_offset[offset] = self.control_spec_len as u16;
        self.control_spec_len += 1;
        self.control_spec_by_offset[offset] = Some(spec);
    }

    pub const fn control_spec_at(&self, offset: usize) -> Option<ControlLabelSpec> {
        if offset >= MAX_CAPACITY {
            return None;
        }
        self.control_spec_by_offset[offset]
    }

    const fn rebuild_dynamic_plan_index(mut self) -> Self {
        let mut table: [Option<DynamicPlanInfo>; MAX_CAPACITY] = [None; MAX_CAPACITY];
        let mut next: Option<DynamicPlanInfo> = None;
        let mut offset = MAX_CAPACITY;
        while offset > 0 {
            offset -= 1;
            if let Some(plan) = self.control_plan_by_offset[offset] {
                if plan.is_dynamic() {
                    let tag = if offset < self.len {
                        let eff_struct = self.data[offset];
                        if matches!(eff_struct.kind, eff::EffKind::Atom) {
                            match eff_struct.atom_data().resource {
                                Some(t) => t,
                                None => 0,
                            }
                        } else {
                            0
                        }
                    } else {
                        0
                    };
                    next = Some(DynamicPlanInfo {
                        plan,
                        offset: offset as u16,
                        tag,
                    });
                }
            }
            table[offset] = next;
        }
        self.dynamic_plan_from_offset = table;
        self
    }

    fn rebuild_dynamic_plan_index_mut(&mut self) {
        let mut next: Option<DynamicPlanInfo> = None;
        let mut offset = MAX_CAPACITY;
        while offset > 0 {
            offset -= 1;
            if let Some(plan) = self.control_plan_by_offset[offset] {
                if plan.is_dynamic() {
                    let tag = if offset < self.len {
                        let eff_struct = self.data[offset];
                        if matches!(eff_struct.kind, eff::EffKind::Atom) {
                            match eff_struct.atom_data().resource {
                                Some(t) => t,
                                None => 0,
                            }
                        } else {
                            0
                        }
                    } else {
                        0
                    };
                    next = Some(DynamicPlanInfo {
                        plan,
                        offset: offset as u16,
                        tag,
                    });
                }
            }
            self.dynamic_plan_from_offset[offset] = next;
        }
    }

    pub const fn scope_markers(&self) -> &[ScopeMarker] {
        unsafe { core::slice::from_raw_parts(self.scope_markers.as_ptr(), self.scope_marker_len) }
    }

    pub const fn control_markers(&self) -> &[ControlMarker] {
        unsafe {
            core::slice::from_raw_parts(self.control_markers.as_ptr(), self.control_marker_len)
        }
    }

    pub const fn control_plans(&self) -> &[ControlPlanMarker] {
        unsafe { core::slice::from_raw_parts(self.control_plans.as_ptr(), self.control_plan_len) }
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

/// Construct a single send atom with lane parameter.
pub const fn const_send<const FROM: u8, const TO: u8, M, const LANE: u8>() -> EffList
where
    M: crate::g::MessageSpec + crate::g::SendableLabel + crate::global::MessageControlSpec,
{
    if FROM == TO {
        panic!("sender and receiver roles must be distinct");
    }
    let label = <M as crate::global::MessageSpec>::LABEL;
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
        from: FROM,
        to: TO,
        label,
        is_control: spec.is_some(),
        resource: match spec {
            Some(rule) => Some(rule.resource_tag),
            None => None,
        },
        direction: eff::EffDirection::Send,
        lane: LANE,
    };
    let mut list = EffList::new().push(EffStruct::atom(atom));
    if let Some(rule) = spec {
        list = list.with_control(rule.scope_kind, rule.tap_id);
        list = list.with_control_spec(rule);
        list = list.with_control_plan(HandlePlan::none());
    }
    list
}

/// Construct a single send atom using type-level roles with lane parameter.
pub const fn const_send_typed<From, To, M, const LANE: u8>() -> EffList
where
    From: crate::g::RoleMarker,
    To: crate::g::RoleMarker,
    M: crate::g::MessageSpec + crate::g::SendableLabel + crate::global::MessageControlSpec,
{
    let label = <M as crate::g::MessageSpec>::LABEL;
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
        direction: eff::EffDirection::Send,
        lane: LANE,
    };
    let mut list = EffList::new().push(EffStruct::atom(atom));
    if let Some(rule) = spec {
        list = list.with_control(rule.scope_kind, rule.tap_id);
        list = list.with_control_spec(rule);
        list = list.with_control_plan(HandlePlan::none());
    }
    list
}

pub const fn splice_local_plan<const DST_LANE: u16>() -> HandlePlan {
    HandlePlan::splice_local(DST_LANE)
}

pub const fn reroute_local_plan<const DST_LANE: u16, const SHARD: u32>() -> HandlePlan {
    HandlePlan::reroute_local(DST_LANE, SHARD)
}

pub const fn dynamic_plan(policy_id: u16, meta: DynamicMeta) -> HandlePlan {
    HandlePlan::dynamic(policy_id, meta)
}
