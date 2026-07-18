use crate::eff::EffAtom;

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
    Event { atom: EffAtom, frame_label: u8 },
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
            SourceRow::Empty | SourceRow::Event { .. } | SourceRow::Resolver(_) => {
                crate::invariant()
            }
        }
    }

    pub(crate) const fn is_first_enter(self, index: usize) -> bool {
        self.at(index).event.is_primary_enter()
    }

    pub(crate) const fn first_enter_index(self, scope: ScopeId) -> Option<usize> {
        if scope.is_none() {
            return None;
        }
        let mut index = 0usize;
        while index < self.len {
            let marker = self.at(index);
            if marker.event.is_primary_enter() && marker.scope_id.same(scope) {
                return Some(index);
            }
            index += 1;
        }
        None
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
        if scope_id.is_none() {
            panic!("scope marker requires a present scope");
        }
        if event.is_enter() {
            if segment_end <= offset {
                panic!("scope segment must be non-empty");
            }
            let entry_matches_scope = match scope_id.kind() {
                Some(super::ScopeKind::Route) => event.route_arm().is_some(),
                Some(super::ScopeKind::Parallel) => event.parallel_split().is_some(),
                Some(super::ScopeKind::Roll) => event.is_roll_enter(),
                None => false,
            };
            if !entry_matches_scope {
                panic!("scope entry kind mismatch");
            }
            if let Some(right_end) = event.route_end()
                && right_end <= segment_end
            {
                panic!("route right arm must be non-empty");
            }
            if let Some(split) = event.parallel_split()
                && (split <= offset || split >= segment_end)
            {
                panic!("parallel arms must be non-empty");
            }
        } else if segment_end != offset {
            panic!("scope boundary marker cannot carry a segment");
        } else if matches!(event, ScopeEvent::Split)
            && !matches!(scope_id.kind(), Some(super::ScopeKind::Parallel))
        {
            panic!("split marker requires a parallel scope");
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
