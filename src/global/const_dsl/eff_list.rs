use super::{
    ControlMarker, ControlScopeKind, ControlSpecMarker, EffList, EffStruct, MAX_CAPACITY,
    MAX_SEGMENT_EFFS, MAX_SEGMENTS, MessageRuntime, MessageSpec, PolicyMarker, PolicyMode,
    RoleMarker, ScopeEvent, ScopeId, ScopeKind, ScopeMarker, SegmentSummary, StaticControlDesc,
    eff,
};
impl Default for EffList {
    fn default() -> Self {
        Self::new()
    }
}

impl EffList {
    /// Create an empty accumulator.
    pub const fn new() -> Self {
        Self {
            segments: [[EffStruct::pure(); MAX_SEGMENT_EFFS]; MAX_SEGMENTS],
            segment_summaries: [SegmentSummary::EMPTY; MAX_SEGMENTS],
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

    #[inline(always)]
    const fn segment_slot(offset: usize) -> (usize, usize) {
        (offset / MAX_SEGMENT_EFFS, offset % MAX_SEGMENT_EFFS)
    }

    #[inline(always)]
    pub(super) const fn summary_segment_for_scope_marker_offset(
        offset: usize,
        current_len: usize,
        event: ScopeEvent,
    ) -> usize {
        if offset > current_len || current_len > MAX_CAPACITY {
            panic!("EffList marker offset out of bounds");
        }
        if matches!(event, ScopeEvent::Enter) {
            if offset >= MAX_CAPACITY {
                panic!("EffList marker offset out of bounds");
            }
            return offset / MAX_SEGMENT_EFFS;
        }
        if current_len == 0 {
            0
        } else if offset == current_len && offset % MAX_SEGMENT_EFFS == 0 {
            (offset / MAX_SEGMENT_EFFS) - 1
        } else {
            offset / MAX_SEGMENT_EFFS
        }
    }

    #[inline(always)]
    pub(super) const fn summary_segment_for_effect_indexed_offset(offset: usize) -> usize {
        if offset >= MAX_CAPACITY {
            panic!("EffList effect marker offset out of bounds");
        }
        offset / MAX_SEGMENT_EFFS
    }

    #[inline(always)]
    pub(crate) const fn node_at(&self, offset: usize) -> EffStruct {
        if offset >= self.len {
            panic!("EffList node offset out of bounds");
        }
        let (segment, local) = Self::segment_slot(offset);
        self.segments[segment][local]
    }

    #[inline(always)]
    pub(crate) const fn segment_count(&self) -> usize {
        if self.len == 0 {
            0
        } else {
            ((self.len - 1) / MAX_SEGMENT_EFFS) + 1
        }
    }

    #[inline(always)]
    pub(crate) const fn segment_start(segment: usize) -> usize {
        if segment >= MAX_SEGMENTS {
            panic!("EffList segment out of bounds");
        }
        segment * MAX_SEGMENT_EFFS
    }

    #[inline(always)]
    pub(crate) const fn segment_len(&self, segment: usize) -> usize {
        let count = self.segment_count();
        if segment >= count {
            panic!("EffList segment out of range");
        }
        let start = Self::segment_start(segment);
        let remaining = self.len - start;
        if remaining > MAX_SEGMENT_EFFS {
            MAX_SEGMENT_EFFS
        } else {
            remaining
        }
    }

    #[inline(always)]
    pub(crate) const fn segment_summary(&self, segment: usize) -> SegmentSummary {
        if segment >= MAX_SEGMENTS {
            panic!("EffList segment summary out of bounds");
        }
        self.segment_summaries[segment]
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

    /// Append a single node to the accumulator.
    pub const fn push(mut self, node: EffStruct) -> Self {
        if self.len >= MAX_CAPACITY {
            panic!("EffList capacity exceeded");
        }
        let (segment, local) = Self::segment_slot(self.len);
        self.segments[segment][local] = node;
        self.segment_summaries[segment] = self.segment_summaries[segment].with_effect();
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
            self = self.push(other.node_at(idx));
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
            if let Some(control_spec) = spec.spec {
                self = self.push_control_spec(base + spec.offset, control_spec);
            }
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

    pub(super) const fn push_scope_marker_full(
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
        let segment = Self::summary_segment_for_scope_marker_offset(offset, self.len, event);
        self.segment_summaries[segment] =
            self.segment_summaries[segment].with_scope_marker(scope_kind, event);
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
        let segment =
            Self::summary_segment_for_scope_marker_offset(offset, self.len, ScopeEvent::Enter);
        self.segment_summaries[segment] =
            self.segment_summaries[segment].with_scope_marker(scope.kind(), ScopeEvent::Enter);
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
        let segment = Self::summary_segment_for_effect_indexed_offset(offset);
        self.segment_summaries[segment] = self.segment_summaries[segment].with_control_marker();
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

    pub(crate) const fn with_control_spec(self, spec: StaticControlDesc) -> Self {
        if self.len == 0 {
            panic!("EffList is empty");
        }
        self.push_control_spec(self.len - 1, spec)
    }

    pub(crate) const fn push_policy(mut self, offset: usize, policy: PolicyMode) -> Self {
        if offset > self.len || offset > MAX_CAPACITY {
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
        let segment = Self::summary_segment_for_effect_indexed_offset(offset);
        self.segment_summaries[segment] = self.segment_summaries[segment].with_policy_marker();
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

    pub(crate) const fn route_arm_dynamic_policy_head_status(&self) -> u8 {
        let mut marker_idx = 0usize;
        let mut active_scope_depth = 0usize;
        let mut offset = 0usize;
        while offset < self.len {
            let mut scan_marker_idx = marker_idx;
            let mut depth_after_exits = active_scope_depth;
            while scan_marker_idx < self.scope_marker_len {
                let marker = self.scope_markers[scan_marker_idx];
                if marker.offset != offset {
                    break;
                }
                if matches!(marker.event, ScopeEvent::Exit) {
                    depth_after_exits = depth_after_exits.saturating_sub(1);
                }
                scan_marker_idx += 1;
            }

            let mut enter_count = 0usize;
            let mut nested_non_policy_enter = false;
            let mut next_marker_idx = marker_idx;
            while next_marker_idx < self.scope_marker_len {
                let marker = self.scope_markers[next_marker_idx];
                if marker.offset != offset {
                    break;
                }
                if matches!(marker.event, ScopeEvent::Enter) {
                    if depth_after_exits == 0 && !matches!(marker.scope_kind, ScopeKind::Generic) {
                        nested_non_policy_enter = true;
                    }
                    enter_count += 1;
                }
                next_marker_idx += 1;
            }

            if let Some(policy) = self.policy_at(offset)
                && policy.is_dynamic()
            {
                if depth_after_exits == 0 && !nested_non_policy_enter {
                    if offset != 0 {
                        return 1;
                    }
                    let Some(control) = self.control_spec_at(offset) else {
                        return 2;
                    };
                    if !matches!(
                        control.op(),
                        crate::control::cap::mint::ControlOp::RouteDecision
                            | crate::control::cap::mint::ControlOp::LoopContinue
                            | crate::control::cap::mint::ControlOp::LoopBreak
                    ) {
                        return 3;
                    }
                } else if let Some(scope) = self.scope_id_for_offset(offset)
                    && !matches!(scope.kind(), ScopeKind::Route)
                {
                    return 1;
                }
            }

            active_scope_depth = depth_after_exits.saturating_add(enter_count);
            marker_idx = next_marker_idx;
            offset += 1;
        }
        0
    }

    pub(crate) const fn push_control_spec(
        mut self,
        offset: usize,
        spec: StaticControlDesc,
    ) -> Self {
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
        let segment = Self::summary_segment_for_effect_indexed_offset(offset);
        self.segment_summaries[segment] = self.segment_summaries[segment].with_control_spec();
        self.control_specs[self.control_spec_len] = ControlSpecMarker::new(offset, spec);
        self.control_spec_len += 1;
        self
    }

    pub(crate) const fn control_spec_at(&self, offset: usize) -> Option<StaticControlDesc> {
        if offset >= MAX_CAPACITY {
            return None;
        }
        let mut idx = 0usize;
        while idx < self.control_spec_len {
            let marker = self.control_specs[idx];
            if marker.offset == offset {
                return marker.spec;
            }
            idx += 1;
        }
        None
    }

    pub const fn scope_markers(&self) -> &[ScopeMarker] {
        /* SAFETY: the pointer and length are carved from one backing slice after bounds and alignment checks. */
        unsafe { core::slice::from_raw_parts(self.scope_markers.as_ptr(), self.scope_marker_len) }
    }

    pub const fn control_markers(&self) -> &[ControlMarker] {
        /* SAFETY: the pointer and length are carved from one backing slice after bounds and alignment checks. */
        unsafe {
            core::slice::from_raw_parts(self.control_markers.as_ptr(), self.control_marker_len)
        }
    }
}

/// Construct a single send atom using type-level roles with lane parameter.
pub(crate) const fn const_send_typed<From, To, M, const LANE: u8>() -> EffList
where
    From: RoleMarker,
    To: RoleMarker,
    M: MessageSpec,
{
    crate::global::validate_role_index(From::INDEX);
    crate::global::validate_role_index(To::INDEX);
    let spec = <M as MessageRuntime>::CONTROL;
    let atom = eff::EffAtom {
        from: From::INDEX,
        to: To::INDEX,
        label: <M as MessageSpec>::LOGICAL_LABEL,
        is_control: spec.is_some(),
        resource: match spec {
            Some(rule) => Some(rule.resource_tag()),
            None => None,
        },
        lane: LANE,
    };
    let mut list = EffList::new().push(EffStruct::atom(atom));
    if let Some(rule) = spec {
        list = list.with_control(rule.scope_kind(), rule.tap_id());
        list = list.with_control_spec(rule);
        list = list.with_policy(PolicyMode::static_mode());
    }
    list
}
