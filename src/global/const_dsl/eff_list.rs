use crate::eff::EffStruct;

use super::{
    DynamicRouteResolver, EffList, ReentryMark, RouteResolverMarker, ScopeEvent, ScopeId,
    ScopeKind, ScopeMarker, ScopeMarkerView, SourceRow, eff,
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
    pub(crate) const fn node_at(&self, offset: usize) -> EffStruct {
        if offset >= self.len {
            panic!("EffList node offset out of bounds");
        }
        match self.rows[offset] {
            SourceRow::Event { node, .. } => node,
            _ => crate::invariant(),
        }
    }

    pub(super) const fn replace_node(&mut self, offset: usize, node: EffStruct) {
        if offset >= self.len {
            panic!("EffList node offset out of bounds");
        }
        let frame_label = match self.rows[offset] {
            SourceRow::Event { frame_label, .. } => frame_label,
            _ => crate::invariant(),
        };
        self.rows[offset] = SourceRow::Event { node, frame_label };
    }

    pub(crate) const fn frame_label_at(&self, offset: usize) -> u8 {
        if offset >= self.len || !matches!(self.node_at(offset).kind, eff::EffKind::Atom) {
            panic!("frame label event offset out of bounds");
        }
        match self.rows[offset] {
            SourceRow::Event { frame_label, .. } => frame_label,
            _ => crate::invariant(),
        }
    }

    pub(super) const fn set_frame_label(&mut self, offset: usize, frame_label: u8) {
        if offset >= self.len || !matches!(self.node_at(offset).kind, eff::EffKind::Atom) {
            panic!("frame label event offset out of bounds");
        }
        let node = self.node_at(offset);
        self.rows[offset] = SourceRow::Event { node, frame_label };
    }

    /// Append a single node to the accumulator.
    #[cfg(any(kani, all(test, hibana_repo_tests)))]
    pub(crate) const fn push(mut self, node: EffStruct) -> Self {
        self.push_mut(node);
        self
    }

    const fn push_mut(&mut self, node: EffStruct) {
        if self.len >= self.scope_marker_start {
            panic!("EffList capacity exceeded");
        }
        self.rows[self.len] = SourceRow::Event {
            node,
            frame_label: 0,
        };
        self.len += 1;
    }

    pub(crate) const fn push_event_mut(&mut self, node: EffStruct) {
        if !matches!(node.kind, eff::EffKind::Atom) {
            panic!("source lowering accepts protocol events only");
        }
        self.push_mut(node);
    }

    const fn push_scope_marker_full_mut(
        &mut self,
        offset: usize,
        scope_id: ScopeId,
        event: ScopeEvent,
        reentry: ReentryMark,
    ) {
        if self.scope_marker_len >= self.resolver_start - self.scope_marker_start {
            panic!("EffList scope marker capacity exceeded");
        }
        let _ = scope_id.local_ordinal();
        let mut idx = self.scope_marker_len;
        while idx > 0 {
            let prev = self.scope_marker_at(idx - 1);
            if prev.offset() > offset {
                self.write_scope_marker(idx, prev);
                idx -= 1;
            } else {
                break;
            }
        }
        self.write_scope_marker(
            idx,
            ScopeMarker::new(offset, offset, scope_id, event, reentry),
        );
        self.scope_marker_len += 1;
    }

    const fn push_scope_enter_marker_mut(
        &mut self,
        offset: usize,
        scope: ScopeId,
        reentry: ReentryMark,
    ) {
        if self.scope_marker_len >= self.resolver_start - self.scope_marker_start {
            panic!("EffList scope marker capacity exceeded");
        }
        let _ = scope.local_ordinal();
        let mut idx = self.scope_marker_len;
        while idx > 0 {
            let prev = self.scope_marker_at(idx - 1);
            let precedes_equal_boundary = prev.offset() == offset
                && (matches!(prev.event, ScopeEvent::Split)
                    || (matches!(prev.event, ScopeEvent::Enter)
                        && prev.scope_id.local_ordinal() > scope.local_ordinal()));
            if prev.offset() > offset || precedes_equal_boundary {
                self.write_scope_marker(idx, prev);
                idx -= 1;
            } else {
                break;
            }
        }
        self.write_scope_marker(
            idx,
            ScopeMarker::new(offset, offset, scope, ScopeEvent::Enter, reentry),
        );
        self.scope_marker_len += 1;
    }

    pub(crate) const fn close_scope_segment_mut(
        &mut self,
        scope: ScopeId,
        start: usize,
        end: usize,
    ) {
        if start >= end || end > self.len {
            panic!("scope segment must contain protocol events");
        }
        let mut idx = 0usize;
        while idx < self.scope_marker_len {
            let marker = self.scope_marker_at(idx);
            if matches!(marker.event, ScopeEvent::Enter)
                && marker.scope_id.same(scope)
                && marker.offset() == start
                && marker.segment_end() == start
            {
                self.write_scope_marker(
                    idx,
                    ScopeMarker::new(start, end, scope, ScopeEvent::Enter, marker.reentry),
                );
                return;
            }
            idx += 1;
        }
        panic!("scope segment enter marker missing");
    }

    pub(crate) const fn push_scope_enter_reentry_mut(
        &mut self,
        offset: usize,
        scope: ScopeId,
        reentry: ReentryMark,
    ) {
        self.push_scope_enter_marker_mut(offset, scope, reentry);
    }

    pub(crate) const fn push_scope_split_mut(&mut self, offset: usize, scope: ScopeId) {
        self.push_scope_marker_full_mut(offset, scope, ScopeEvent::Split, ReentryMark::SinglePass);
    }

    pub(crate) const fn push_scope_exit_mut(&mut self, offset: usize, scope: ScopeId) {
        self.push_scope_marker_full_mut(offset, scope, ScopeEvent::Exit, ReentryMark::SinglePass);
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
            _ => crate::invariant(),
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
    let atom = eff::EffAtom {
        from: FROM,
        to: TO,
        label: <M as crate::global::Message>::LOGICAL_LABEL,
        payload_schema: crate::global::payload_schema::<M>(),
        origin: eff::EventOrigin::User,
        lane: LANE,
    };
    EffList::new().push(EffStruct::atom(atom))
}
