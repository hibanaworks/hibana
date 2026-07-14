use super::{
    DynamicRouteResolver, EffList, EffStruct, MAX_CAPACITY, MAX_ROUTE_RESOLVER_MARKERS,
    MAX_SEGMENT_EFFS, MAX_SEGMENTS, Message, ReentryMark, RouteResolverMarker, ScopeEvent, ScopeId,
    ScopeKind, ScopeMarker, eff,
};

#[derive(Clone, Copy)]
pub(crate) enum ScopeRebase {
    Preserve,
    MarkRouteEnters,
}

impl ScopeRebase {
    const fn apply(
        self,
        current: ReentryMark,
        scope_id: ScopeId,
        event: ScopeEvent,
    ) -> ReentryMark {
        match self {
            Self::Preserve => current,
            Self::MarkRouteEnters => {
                if matches!(scope_id.kind(), Some(ScopeKind::Route))
                    && matches!(event, ScopeEvent::Enter)
                {
                    ReentryMark::Reentrant
                } else {
                    current
                }
            }
        }
    }
}

impl EffList {
    /// Create an empty accumulator.
    pub(crate) const fn new() -> Self {
        Self {
            segments: [[EffStruct::pure(); MAX_SEGMENT_EFFS]; MAX_SEGMENTS],
            len: 0,
            scope_budget: 0,
            scope_markers: [ScopeMarker::empty(); MAX_CAPACITY],
            scope_marker_len: 0,
            resolver_markers: [RouteResolverMarker::empty(); MAX_ROUTE_RESOLVER_MARKERS],
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
    pub(crate) const fn node_at(&self, offset: usize) -> EffStruct {
        if offset >= self.len {
            panic!("EffList node offset out of bounds");
        }
        let (segment, local) = Self::segment_slot(offset);
        self.segments[segment][local]
    }

    /// Append a single node to the accumulator.
    pub(crate) const fn push(mut self, node: EffStruct) -> Self {
        self.push_mut(node);
        self
    }

    const fn push_mut(&mut self, node: EffStruct) {
        if self.len >= MAX_CAPACITY {
            panic!("EffList capacity exceeded");
        }
        let (segment, local) = Self::segment_slot(self.len);
        self.segments[segment][local] = node;
        self.len += 1;
    }

    pub(crate) const fn append_rebased_from(
        &mut self,
        other: &EffList,
        lane_offset: u16,
        scope_offset: u16,
        scope_rebase: ScopeRebase,
    ) {
        let mut idx = 0;
        let base = self.len;
        while idx < other.len {
            let mut node = other.node_at(idx);
            if lane_offset != 0 && matches!(node.kind, eff::EffKind::Atom) {
                let mut atom = node.atom_data();
                let lane = atom.lane as u16 + lane_offset;
                if lane > u8::MAX as u16 {
                    panic!("projection internal lane overflow");
                }
                atom.lane = lane as u8;
                node = EffStruct::atom(atom);
            }
            self.push_mut(node);
            idx += 1;
        }
        let mut scope_idx = 0;
        while scope_idx < other.scope_marker_len {
            let marker = other.scope_markers[scope_idx];
            let scope_id = marker.scope_id.add_ordinal(scope_offset);
            let reentry = scope_rebase.apply(marker.reentry, scope_id, marker.event);
            self.push_scope_marker_full_mut(
                base + marker.offset(),
                scope_id,
                marker.event,
                reentry,
            );
            scope_idx += 1;
        }
        let mut resolver_idx = 0;
        while resolver_idx < other.resolver_marker_len {
            let marker = other.resolver_markers[resolver_idx];
            self.push_route_resolver_mut(
                marker.scope.add_ordinal(scope_offset),
                marker.resolver_id,
            );
            resolver_idx += 1;
        }
    }

    pub(crate) const fn rebase_scopes_mut(&mut self, scope_offset: u16, scope_rebase: ScopeRebase) {
        if scope_offset == 0 && matches!(scope_rebase, ScopeRebase::Preserve) {
            return;
        }
        let mut scope_idx = 0usize;
        while scope_idx < self.scope_marker_len {
            let marker = self.scope_markers[scope_idx];
            let scope_id = marker.scope_id.add_ordinal(scope_offset);
            let reentry = scope_rebase.apply(marker.reentry, scope_id, marker.event);
            self.scope_markers[scope_idx] =
                ScopeMarker::new(marker.offset(), scope_id, marker.event, reentry);
            scope_idx += 1;
        }
        let mut resolver_idx = 0usize;
        while resolver_idx < self.resolver_marker_len {
            let marker = self.resolver_markers[resolver_idx];
            self.resolver_markers[resolver_idx] = RouteResolverMarker::new(
                marker.scope.add_ordinal(scope_offset),
                marker.resolver_id,
            );
            resolver_idx += 1;
        }
        if scope_offset != 0 {
            let scope_budget = self.scope_budget as u32 + scope_offset as u32;
            if scope_budget > ScopeId::LOCAL_CAPACITY as u32 {
                panic!("scope ordinal overflow");
            }
            self.scope_budget = scope_budget as u16;
        }
    }

    const fn push_scope_marker_full_mut(
        &mut self,
        offset: usize,
        scope_id: ScopeId,
        event: ScopeEvent,
        reentry: ReentryMark,
    ) {
        if self.scope_marker_len >= MAX_CAPACITY {
            panic!("EffList scope marker capacity exceeded");
        }
        let ordinal = scope_id.local_ordinal();
        let next = ordinal + 1;
        if next > ScopeId::LOCAL_CAPACITY {
            panic!("scope ordinal overflow");
        }
        if next > self.scope_budget {
            self.scope_budget = next;
        }
        let mut idx = self.scope_marker_len;
        while idx > 0 {
            let prev = self.scope_markers[idx - 1];
            if prev.offset() > offset {
                self.scope_markers[idx] = prev;
                idx -= 1;
            } else {
                break;
            }
        }
        self.scope_markers[idx] = ScopeMarker::new(offset, scope_id, event, reentry);
        self.scope_marker_len += 1;
    }

    const fn push_scope_marker_outer_enter_reentry_mut(
        &mut self,
        offset: usize,
        scope: ScopeId,
        reentry: ReentryMark,
    ) {
        if self.scope_marker_len >= MAX_CAPACITY {
            panic!("EffList scope marker capacity exceeded");
        }
        let ordinal = scope.local_ordinal();
        let next = ordinal + 1;
        if next > ScopeId::LOCAL_CAPACITY {
            panic!("scope ordinal overflow");
        }
        if next > self.scope_budget {
            self.scope_budget = next;
        }
        let mut idx = self.scope_marker_len;
        while idx > 0 {
            let prev = self.scope_markers[idx - 1];
            if prev.offset() >= offset {
                self.scope_markers[idx] = prev;
                idx -= 1;
            } else {
                break;
            }
        }
        self.scope_markers[idx] = ScopeMarker::new(offset, scope, ScopeEvent::Enter, reentry);
        self.scope_marker_len += 1;
    }

    pub(crate) const fn push_scope_around(&mut self, start: usize, end: usize, scope: ScopeId) {
        self.push_scope_marker_outer_enter_reentry_mut(start, scope, ReentryMark::SinglePass);
        self.push_scope_marker_full_mut(end, scope, ScopeEvent::Exit, ReentryMark::SinglePass);
    }

    pub(crate) const fn push_scope_enter_at_boundary(&mut self, offset: usize, scope: ScopeId) {
        self.push_scope_marker_full_mut(offset, scope, ScopeEvent::Enter, ReentryMark::SinglePass);
    }

    pub(crate) const fn push_scope_exit_at_boundary(&mut self, offset: usize, scope: ScopeId) {
        self.push_scope_marker_full_mut(offset, scope, ScopeEvent::Exit, ReentryMark::SinglePass);
    }

    pub(crate) const fn push_parallel_scope_split(&mut self, scope: ScopeId, split: usize) {
        if !matches!(scope.kind(), Some(ScopeKind::Parallel)) {
            panic!("parallel split scope");
        }
        if split == 0 || split >= self.len {
            panic!("parallel split must separate non-empty arms");
        }
        let len = self.len;
        self.push_scope_marker_outer_enter_reentry_mut(0, scope, ReentryMark::SinglePass);
        self.push_scope_marker_full_mut(split, scope, ScopeEvent::Split, ReentryMark::SinglePass);
        self.push_scope_marker_full_mut(len, scope, ScopeEvent::Exit, ReentryMark::SinglePass);
    }

    pub(crate) const fn push_route_resolver_mut(&mut self, scope: ScopeId, resolver_id: u16) {
        if !matches!(scope.kind(), Some(ScopeKind::Route)) {
            panic!("EffList route resolver scope");
        }
        let mut idx = 0usize;
        while idx < self.resolver_marker_len {
            if self.resolver_markers[idx].scope.same(scope) {
                panic!("duplicate route resolver scope");
            }
            idx += 1;
        }
        if self.resolver_marker_len >= MAX_ROUTE_RESOLVER_MARKERS {
            panic!("EffList resolver marker capacity exceeded");
        }
        self.resolver_markers[self.resolver_marker_len] =
            RouteResolverMarker::new(scope, resolver_id);
        self.resolver_marker_len += 1;
    }

    pub(crate) const fn resolver_for_scope(&self, scope: ScopeId) -> Option<DynamicRouteResolver> {
        if !matches!(scope.kind(), Some(ScopeKind::Route))
            || scope.local_ordinal() as usize >= crate::eff::meta::MAX_EFF_NODES
        {
            crate::invariant();
        }
        let mut idx = 0usize;
        while idx < self.resolver_marker_len {
            let marker = self.resolver_markers[idx];
            if marker.scope.same(scope) {
                return Some(marker.resolver());
            }
            idx += 1;
        }
        None
    }

    pub(crate) const fn scope_marker_count(&self) -> usize {
        self.scope_marker_len
    }

    pub(crate) const fn scope_marker_at(&self, idx: usize) -> ScopeMarker {
        if idx >= self.scope_marker_len {
            panic!("EffList scope marker index out of bounds");
        }
        self.scope_markers[idx]
    }

    pub(crate) const fn scope_markers(&self) -> &[ScopeMarker] {
        /* SAFETY: `EffList` owns initialized scope-marker rows from the compiled const
        descriptor image, and `scope_marker_len` is the row count carried with
        that pointer for a shared read-only slice. */
        unsafe { core::slice::from_raw_parts(self.scope_markers.as_ptr(), self.scope_marker_len) }
    }
}

/// Construct a single send atom from const role identities with a lane parameter.
pub(crate) const fn const_send_typed<const FROM: u8, const TO: u8, M, const LANE: u8>() -> EffList
where
    M: Message,
    M::Payload: crate::transport::wire::WireEncode + crate::transport::wire::WirePayload,
{
    let atom = eff::EffAtom {
        from: FROM,
        to: TO,
        label: <M as Message>::LOGICAL_LABEL,
        payload_schema: crate::global::payload_schema::<M>(),
        origin: eff::EventOrigin::User,
        lane: LANE,
    };
    EffList::new().push(EffStruct::atom(atom))
}
