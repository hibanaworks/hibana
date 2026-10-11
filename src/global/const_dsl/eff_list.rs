use crate::eff::EffAtom;

use super::{
    DynamicRouteResolver, EffList, ReentryMark, RouteResolverMarker, ScopeEvent, ScopeId,
    ScopeKind, ScopeMarker, ScopeMarkerView, SourceRow,
};

#[cfg(all(test, hibana_repo_tests))]
mod tests;

impl<const E: usize> EffList<E> {
    /// Create an empty accumulator.
    #[cfg(any(kani, all(test, hibana_repo_tests)))]
    pub(crate) const fn new() -> Self {
        Self::new_partitioned(E, 0, 0)
    }

    pub(crate) const fn new_partitioned(
        event_count: usize,
        scope_marker_count: usize,
        resolver_count: usize,
    ) -> Self {
        let Some(scope_marker_start) = event_count.checked_add(scope_marker_count) else {
            panic!("source arena capacity overflow");
        };
        let Some(required) = scope_marker_start.checked_add(resolver_count) else {
            panic!("source arena capacity overflow");
        };
        if required > E {
            panic!("source arena partition exceeds bucket");
        }
        Self {
            rows: [SourceRow::Empty; E],
            scope_marker_start: event_count,
            resolver_start: scope_marker_start,
            source_end: required,
            len: 0,
            scope_marker_len: 0,
            resolver_marker_len: 0,
        }
    }

    /// Return the current length.
    pub(crate) const fn len(&self) -> usize {
        self.len
    }

    #[inline(always)]
    pub(crate) const fn atom_at(&self, offset: usize) -> EffAtom {
        if offset >= self.len {
            panic!("EffList atom offset out of bounds");
        }
        match self.rows[offset] {
            SourceRow::Event { atom, .. } => atom,
            SourceRow::Empty | SourceRow::Scope(_) | SourceRow::Resolver(_) => crate::invariant(),
        }
    }

    pub(super) const fn replace_atom(&mut self, offset: usize, atom: EffAtom) {
        if offset >= self.len {
            panic!("EffList atom offset out of bounds");
        }
        let frame_label = match self.rows[offset] {
            SourceRow::Event { frame_label, .. } => frame_label,
            SourceRow::Empty | SourceRow::Scope(_) | SourceRow::Resolver(_) => crate::invariant(),
        };
        self.rows[offset] = SourceRow::Event { atom, frame_label };
    }

    pub(crate) const fn frame_label_at(&self, offset: usize) -> u8 {
        if offset >= self.len {
            panic!("frame label event offset out of bounds");
        }
        match self.rows[offset] {
            SourceRow::Event { frame_label, .. } => frame_label,
            SourceRow::Empty | SourceRow::Scope(_) | SourceRow::Resolver(_) => crate::invariant(),
        }
    }

    pub(super) const fn set_frame_label(&mut self, offset: usize, frame_label: u8) {
        if offset >= self.len {
            panic!("frame label event offset out of bounds");
        }
        let atom = self.atom_at(offset);
        self.rows[offset] = SourceRow::Event { atom, frame_label };
    }

    /// Append a single node to the accumulator.
    #[cfg(any(kani, all(test, hibana_repo_tests)))]
    pub(crate) const fn push(mut self, atom: EffAtom) -> Self {
        self.push_mut(atom);
        self
    }

    const fn push_mut(&mut self, atom: EffAtom) {
        if self.len >= self.scope_marker_start {
            panic!("EffList capacity exceeded");
        }
        self.rows[self.len] = SourceRow::Event {
            atom,
            frame_label: 0,
        };
        self.len += 1;
    }

    pub(crate) const fn push_event_mut(&mut self, atom: EffAtom) {
        self.push_mut(atom);
    }

    const fn insert_scope_marker_mut(&mut self, marker: ScopeMarker) {
        if self.scope_marker_len >= self.resolver_start - self.scope_marker_start {
            panic!("EffList scope marker capacity exceeded");
        }
        let _ = marker.scope_id.local_ordinal();
        let mut idx = self.scope_marker_len;
        while idx > 0 {
            let prev = self.scope_marker_at(idx - 1);
            let enter_precedes_equal_boundary = marker.event.is_enter()
                && prev.offset() == marker.offset()
                && (matches!(prev.event, ScopeEvent::Split)
                    || (prev.event.is_enter()
                        && prev.scope_id.local_ordinal() > marker.scope_id.local_ordinal()));
            if prev.offset() > marker.offset() || enter_precedes_equal_boundary {
                self.write_scope_marker(idx, prev);
                idx -= 1;
            } else {
                break;
            }
        }
        self.write_scope_marker(idx, marker);
        self.scope_marker_len += 1;
    }

    pub(crate) const fn push_route_scope_mut(
        &mut self,
        scope: ScopeId,
        left_start: usize,
        right_start: usize,
        right_end: usize,
        reentry: ReentryMark,
    ) {
        if !matches!(scope.kind(), Some(ScopeKind::Route))
            || left_start >= right_start
            || right_start >= right_end
            || right_end > self.len
        {
            panic!("route scope requires two contiguous non-empty arms");
        }
        self.insert_scope_marker_mut(ScopeMarker::new(
            left_start,
            right_start,
            scope,
            ScopeEvent::route_enter(right_end),
            reentry,
        ));
        self.insert_scope_marker_mut(ScopeMarker::new(
            right_start,
            right_start,
            scope,
            ScopeEvent::Exit,
            ReentryMark::SinglePass,
        ));
        self.insert_scope_marker_mut(ScopeMarker::new(
            right_start,
            right_end,
            scope,
            ScopeEvent::route_arm_continuation(),
            reentry,
        ));
        self.insert_scope_marker_mut(ScopeMarker::new(
            right_end,
            right_end,
            scope,
            ScopeEvent::Exit,
            ReentryMark::SinglePass,
        ));
    }

    pub(crate) const fn push_parallel_scope_mut(
        &mut self,
        scope: ScopeId,
        start: usize,
        split: usize,
        end: usize,
    ) {
        if !matches!(scope.kind(), Some(ScopeKind::Parallel))
            || start >= split
            || split >= end
            || end > self.len
        {
            panic!("parallel scope requires two contiguous non-empty arms");
        }
        self.insert_scope_marker_mut(ScopeMarker::new(
            start,
            end,
            scope,
            ScopeEvent::parallel_enter(split),
            ReentryMark::SinglePass,
        ));
        self.insert_scope_marker_mut(ScopeMarker::new(
            split,
            split,
            scope,
            ScopeEvent::Split,
            ReentryMark::SinglePass,
        ));
        self.insert_scope_marker_mut(ScopeMarker::new(
            end,
            end,
            scope,
            ScopeEvent::Exit,
            ReentryMark::SinglePass,
        ));
    }

    pub(crate) const fn push_roll_scope_mut(&mut self, scope: ScopeId, start: usize, end: usize) {
        if !matches!(scope.kind(), Some(ScopeKind::Roll)) || start >= end || end > self.len {
            panic!("roll scope requires a non-empty body");
        }
        self.insert_scope_marker_mut(ScopeMarker::new(
            start,
            end,
            scope,
            ScopeEvent::roll_enter(),
            ReentryMark::SinglePass,
        ));
        self.insert_scope_marker_mut(ScopeMarker::new(
            end,
            end,
            scope,
            ScopeEvent::Exit,
            ReentryMark::SinglePass,
        ));
    }

    pub(crate) const fn push_route_resolver_mut(&mut self, scope: ScopeId, resolver_id: u16) {
        if !matches!(scope.kind(), Some(ScopeKind::Route)) {
            panic!("EffList route resolver scope");
        }
        let mut idx = 0usize;
        while idx < self.resolver_marker_len {
            if self.resolver_marker_at(idx).scope.same(scope) {
                panic!("duplicate route resolver scope");
            }
            idx += 1;
        }
        if self.resolver_marker_len >= self.source_end - self.resolver_start {
            panic!("EffList resolver marker capacity exceeded");
        }
        self.rows[self.resolver_start + self.resolver_marker_len] =
            SourceRow::Resolver(RouteResolverMarker::new(scope, resolver_id));
        self.resolver_marker_len += 1;
    }

    pub(crate) const fn resolver_for_scope(&self, scope: ScopeId) -> Option<DynamicRouteResolver> {
        if !matches!(scope.kind(), Some(ScopeKind::Route)) {
            crate::invariant();
        }
        let mut idx = 0usize;
        while idx < self.resolver_marker_len {
            let marker = self.resolver_marker_at(idx);
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

    pub(crate) const fn resolver_marker_count(&self) -> usize {
        self.resolver_marker_len
    }

    #[inline(always)]
    const fn scope_marker_at(&self, index: usize) -> ScopeMarker {
        self.scope_markers().at(index)
    }

    #[inline(always)]
    const fn write_scope_marker(&mut self, index: usize, marker: ScopeMarker) {
        if index >= self.resolver_start - self.scope_marker_start {
            panic!("EffList scope marker capacity exceeded");
        }
        self.rows[self.scope_marker_start + index] = SourceRow::Scope(marker);
    }

    #[inline(always)]
    const fn resolver_marker_at(&self, index: usize) -> RouteResolverMarker {
        if index >= self.resolver_marker_len {
            panic!("resolver marker offset out of bounds");
        }
        match self.rows[self.resolver_start + index] {
            SourceRow::Resolver(marker) => marker,
            SourceRow::Empty | SourceRow::Event { .. } | SourceRow::Scope(_) => crate::invariant(),
        }
    }

    pub(crate) const fn scope_markers(&self) -> ScopeMarkerView<'_> {
        ScopeMarkerView {
            rows: &self.rows,
            start: self.scope_marker_start,
            len: self.scope_marker_len,
        }
    }
}

/// Construct a single send atom for direct validator and proof tests.
#[cfg(all(test, hibana_repo_tests))]
pub(crate) const fn const_send_typed<
    const FROM: u8,
    const TO: u8,
    M,
    const LANE: u8,
    const CAPACITY: usize,
>() -> EffList<CAPACITY>
where
    M: crate::global::Message,
    M::Payload: crate::transport::wire::WireEncode + crate::transport::wire::WirePayload,
{
    let atom = crate::eff::EffAtom {
        from: FROM,
        to: TO,
        label: <M as crate::global::Message>::LOGICAL_LABEL,
        payload_schema: crate::global::payload_schema::<M>(),
        origin: crate::eff::EventOrigin::User,
        lane: LANE,
    };
    EffList::new().push(atom)
}
