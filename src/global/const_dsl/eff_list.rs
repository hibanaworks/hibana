use super::{
    EffList, EffStruct, MAX_CAPACITY, MAX_SEGMENT_EFFS, MAX_SEGMENTS, Message, ReentryMark,
    ResolverMarker, RouteResolver, ScopeEvent, ScopeId, ScopeKind, ScopeMarker, SegmentSummary,
    eff,
};
impl EffList {
    /// Create an empty accumulator.
    pub(crate) const fn new() -> Self {
        Self {
            segments: [[EffStruct::pure(); MAX_SEGMENT_EFFS]; MAX_SEGMENTS],
            segment_summaries: [SegmentSummary::EMPTY; MAX_SEGMENTS],
            len: 0,
            scope_budget: 0,
            scope_markers: [ScopeMarker::empty(); MAX_CAPACITY],
            scope_marker_len: 0,
            resolver_markers: [ResolverMarker::empty(); MAX_CAPACITY],
            resolver_marker_len: 0,
        }
    }

    /// Return the current length.
    pub(crate) const fn len(&self) -> usize {
        self.len
    }

    /// Number of structured scopes encoded within this list.
    pub(crate) const fn scope_budget(&self) -> u16 {
        self.scope_budget
    }

    /// Whether the accumulator is empty.
    pub(crate) const fn is_empty(&self) -> bool {
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
        } else if offset == current_len && offset.is_multiple_of(MAX_SEGMENT_EFFS) {
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
    pub(crate) const fn rebase_scopes(mut self, offset: u16) -> Self {
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
                reentry: marker.reentry,
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
        let mut resolver_idx = 0usize;
        while resolver_idx < self.resolver_marker_len {
            let marker = self.resolver_markers[resolver_idx];
            let mut resolver = marker.resolver;
            let scope = resolver.scope();
            if !scope.is_none() {
                let rebased = scope.add_ordinal(offset);
                resolver = resolver.with_scope(rebased);
            }
            self.resolver_markers[resolver_idx] = ResolverMarker::new(marker.offset, resolver);
            resolver_idx += 1;
        }
        self.scope_budget = max;
        self
    }

    /// Shift every atom lane by a projection-internal lane offset.
    pub(crate) const fn rebase_lanes(mut self, offset: u16) -> Self {
        if offset == 0 {
            return self;
        }
        let mut idx = 0usize;
        while idx < self.len {
            let (segment, local) = Self::segment_slot(idx);
            let node = self.segments[segment][local];
            if matches!(node.kind, eff::EffKind::Atom) {
                let mut atom = node.atom_data();
                let lane = atom.lane as u16 + offset;
                if lane > u8::MAX as u16 {
                    panic!("projection internal lane overflow");
                }
                atom.lane = lane as u8;
                self.segments[segment][local] = EffStruct::atom(atom);
            }
            idx += 1;
        }
        self
    }

    /// Append a single node to the accumulator.
    pub(crate) const fn push(mut self, node: EffStruct) -> Self {
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
    pub(crate) const fn extend_list(mut self, other: EffList) -> Self {
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
                marker.reentry,
                marker.controller_role,
            );
            scope_idx += 1;
        }
        let mut resolver_idx = 0;
        while resolver_idx < other.resolver_marker_len {
            let marker = other.resolver_markers[resolver_idx];
            self = self.push_resolver(base + marker.offset, marker.resolver);
            resolver_idx += 1;
        }
        self
    }

    const fn push_scope_marker_raw(
        self,
        offset: usize,
        scope_id: ScopeId,
        scope_kind: ScopeKind,
        event: ScopeEvent,
        reentry: ReentryMark,
    ) -> Self {
        self.push_scope_marker_full(offset, scope_id, scope_kind, event, reentry, None)
    }

    pub(super) const fn push_scope_marker_full(
        mut self,
        offset: usize,
        scope_id: ScopeId,
        scope_kind: ScopeKind,
        event: ScopeEvent,
        reentry: ReentryMark,
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
            reentry,
            controller_role,
        };
        self.scope_marker_len += 1;
        self
    }

    const fn push_scope_marker(self, offset: usize, scope: ScopeId, event: ScopeEvent) -> Self {
        self.push_scope_marker_raw(offset, scope, scope.kind(), event, ReentryMark::SinglePass)
    }

    const fn push_scope_marker_outer_enter(self, offset: usize, scope: ScopeId) -> Self {
        self.push_scope_marker_outer_enter_reentry(offset, scope, ReentryMark::SinglePass)
    }

    const fn push_scope_marker_outer_enter_reentry(
        mut self,
        offset: usize,
        scope: ScopeId,
        reentry: ReentryMark,
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
            reentry,
            controller_role: None,
        };
        self.scope_marker_len += 1;
        self
    }

    pub(crate) const fn with_scope(self, scope: ScopeId) -> Self {
        let len = self.len;
        let scoped = self.push_scope_marker_outer_enter(0, scope);
        scoped.push_scope_marker(len, scope, ScopeEvent::Exit)
    }

    pub(crate) const fn mark_route_scopes_reentry(mut self) -> Self {
        let mut marker_idx = 0usize;
        while marker_idx < self.scope_marker_len {
            let marker = self.scope_markers[marker_idx];
            if matches!(marker.scope_kind, ScopeKind::Route)
                && matches!(marker.event, ScopeEvent::Enter)
            {
                let mut updated = marker;
                updated.reentry = ReentryMark::Reentrant;
                self.scope_markers[marker_idx] = updated;
            }
            marker_idx += 1;
        }
        self
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

    pub(crate) const fn push_resolver(mut self, offset: usize, resolver: RouteResolver) -> Self {
        if offset > self.len || offset > MAX_CAPACITY {
            panic!("EffList resolver marker offset out of bounds");
        }
        let mut idx = 0usize;
        while idx < self.resolver_marker_len {
            if self.resolver_markers[idx].offset == offset {
                self.resolver_markers[idx] = ResolverMarker::new(offset, resolver);
                return self;
            }
            idx += 1;
        }
        if self.resolver_marker_len >= MAX_CAPACITY {
            panic!("EffList resolver marker capacity exceeded");
        }
        let segment = Self::summary_segment_for_effect_indexed_offset(offset);
        self.segment_summaries[segment] = self.segment_summaries[segment].with_resolver_marker();
        self.resolver_markers[self.resolver_marker_len] = ResolverMarker::new(offset, resolver);
        self.resolver_marker_len += 1;
        self
    }

    pub(crate) const fn resolver_at(&self, offset: usize) -> Option<RouteResolver> {
        if offset >= MAX_CAPACITY {
            crate::invariant();
        }
        let mut idx = 0usize;
        while idx < self.resolver_marker_len {
            let marker = self.resolver_markers[idx];
            if marker.offset == offset {
                return Some(marker.resolver);
            }
            idx += 1;
        }
        None
    }

    pub(crate) const fn scope_id_for_offset(&self, offset: usize) -> Option<ScopeId> {
        if offset >= MAX_CAPACITY {
            crate::invariant();
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
                    if stack_len == 0 {
                        crate::invariant();
                    }
                    stack_len -= 1;
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
        reentry: Option<ReentryMark>,
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
                if let Some(value) = reentry {
                    updated.reentry = value;
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

    pub(crate) const fn resolver_with_scope(
        &self,
        offset: usize,
    ) -> Option<(RouteResolver, ScopeId)> {
        match self.resolver_at(offset) {
            Some(resolver) => {
                let baked_scope = resolver.scope();
                let scope = if baked_scope.is_none() {
                    match self.scope_id_for_offset(offset) {
                        Some(scope) => scope,
                        None if resolver.is_dynamic() => crate::invariant(),
                        None => baked_scope,
                    }
                } else {
                    baked_scope
                };
                Some((resolver.with_scope(scope), scope))
            }
            None => None,
        }
    }

    pub(crate) const fn scope_markers(&self) -> &[ScopeMarker] {
        /* SAFETY: the pointer and length are carved from one backing slice after bounds and alignment checks. */
        unsafe { core::slice::from_raw_parts(self.scope_markers.as_ptr(), self.scope_marker_len) }
    }
}

/// Construct a single send atom from const role identities with a lane parameter.
pub(crate) const fn const_send_typed<const FROM: u8, const TO: u8, M, const LANE: u8>() -> EffList
where
    M: Message,
{
    if let Some(message) = crate::g::role_pair_contract_error::<FROM, TO>() {
        panic!("{}", message);
    }
    let atom = eff::EffAtom {
        from: FROM,
        to: TO,
        label: <M as Message>::LOGICAL_LABEL,
        origin: eff::EventOrigin::User,
        resource: None,
        lane: LANE,
    };
    EffList::new().push(EffStruct::atom(atom))
}
