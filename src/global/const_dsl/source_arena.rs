use crate::eff::EffStruct;

use super::{ReentryMark, RouteResolverMarker, ScopeEvent, ScopeId};

#[derive(Clone, Copy)]
pub(crate) struct ScopeMarker {
    offset: u16,
    segment_end: u16,
    pub(crate) scope_id: ScopeId,
    pub(crate) event: ScopeEvent,
    pub(crate) reentry: ReentryMark,
}

#[derive(Clone, Copy)]
pub(super) enum SourceRow {
    Empty,
    Event { node: EffStruct, frame_label: u8 },
    Scope(ScopeMarker),
    Resolver(RouteResolverMarker),
}

#[derive(Clone, Copy)]
pub(crate) struct ScopeMarkerView<'a> {
    pub(super) rows: &'a [SourceRow],
    pub(super) start: usize,
    pub(super) len: usize,
}

impl ScopeMarkerView<'_> {
    #[inline(always)]
    pub(crate) const fn len(self) -> usize {
        self.len
    }

    #[inline(always)]
    pub(crate) const fn at(self, index: usize) -> ScopeMarker {
        if index >= self.len {
            panic!("scope marker offset out of bounds");
        }
        match self.rows[self.start + index] {
            SourceRow::Scope(marker) => marker,
            _ => crate::invariant(),
        }
    }

    pub(crate) const fn is_first_enter(self, index: usize) -> bool {
        let marker = self.at(index);
        if !matches!(marker.event, ScopeEvent::Enter) {
            return false;
        }
        let mut previous = 0usize;
        while previous < index {
            let candidate = self.at(previous);
            if matches!(candidate.event, ScopeEvent::Enter)
                && candidate.scope_id.same(marker.scope_id)
            {
                return false;
            }
            previous += 1;
        }
        true
    }
}

impl ScopeMarker {
    pub(crate) const fn new(
        offset: usize,
        segment_end: usize,
        scope_id: ScopeId,
        event: ScopeEvent,
        reentry: ReentryMark,
    ) -> Self {
        if offset > u16::MAX as usize || segment_end > u16::MAX as usize {
            panic!("scope marker offset overflow");
        }
        if matches!(event, ScopeEvent::Enter) && segment_end < offset {
            panic!("scope segment ends before it starts");
        }
        Self {
            offset: offset as u16,
            segment_end: segment_end as u16,
            scope_id,
            event,
            reentry,
        }
    }

    #[inline(always)]
    pub(crate) const fn offset(self) -> usize {
        self.offset as usize
    }

    #[inline(always)]
    pub(crate) const fn segment_end(self) -> usize {
        self.segment_end as usize
    }
}

/// Bucketed tagged arena used only while lowering a choreography.
/// Partition boundaries retain exact source counts; unused bucket tail rows
/// stay `Empty` and never enter a descriptor.
pub(crate) struct EffList<const ARENA_CAPACITY: usize> {
    pub(super) rows: [SourceRow; ARENA_CAPACITY],
    pub(super) scope_marker_start: usize,
    pub(super) resolver_start: usize,
    pub(super) source_end: usize,
    pub(super) len: usize,
    pub(super) scope_marker_len: usize,
    pub(super) resolver_marker_len: usize,
}
