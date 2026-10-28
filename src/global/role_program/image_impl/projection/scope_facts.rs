use crate::global::{
    const_dsl::{EffList, ScopeId, ScopeKind, ScopeMarker},
    typestate::LocalConflict,
};

const SCOPE_KIND_COUNT: usize = 3;
const SCOPE_ORDINAL_COUNT: usize = ScopeId::LOCAL_CAPACITY as usize;

/// Immutable compiler facts shared by every role projection. The index is
/// exactly the encoded scope kind/ordinal domain, including sparse ordinals;
/// no event-count ceiling or runtime storage is introduced.
pub(crate) struct ScopeFacts {
    parents: [[Option<LocalConflict>; SCOPE_ORDINAL_COUNT]; SCOPE_KIND_COUNT],
    entries: [[u16; SCOPE_ORDINAL_COUNT]; SCOPE_KIND_COUNT],
    route_count: usize,
}

impl ScopeFacts {
    pub(crate) const fn new<const E: usize>(source: &EffList<E>) -> Self {
        let markers = source.scope_markers();
        let mut facts = Self {
            parents: [[None; SCOPE_ORDINAL_COUNT]; SCOPE_KIND_COUNT],
            entries: [[u16::MAX; SCOPE_ORDINAL_COUNT]; SCOPE_KIND_COUNT],
            route_count: 0,
        };
        let mut index = 0usize;
        let mut routes = [0u16; E];
        let mut depth = 0usize;
        while index < markers.len() {
            let start = markers.at(index).offset();
            while depth != 0 && Self::end(markers.at(routes[depth - 1] as usize)) <= start {
                depth -= 1;
            }
            let mut limit = index + 1;
            while limit < markers.len() && markers.at(limit).offset() == start {
                limit += 1;
            }
            // Enters at a shared boundary are in source preorder. Collect the
            // whole group: a roll and its inner choice can have equal ranges.
            let mut current = index;
            while current < limit {
                let marker = markers.at(current);
                if marker.event.is_primary_enter() {
                    let kind = Self::kind_index(marker.scope_id);
                    let ordinal = marker.scope_id.local_ordinal() as usize;
                    if facts.entries[kind][ordinal] != u16::MAX {
                        panic!("duplicate primary scope identity");
                    }
                    if current >= u16::MAX as usize || Self::end(marker) > source.len() {
                        crate::invariant();
                    }
                    facts.entries[kind][ordinal] = current as u16;
                    if matches!(marker.scope_id.kind(), Some(ScopeKind::Route)) {
                        if depth != 0
                            && Self::end(markers.at(routes[depth - 1] as usize)) < Self::end(marker)
                        {
                            panic!("structured scope ranges cross without nesting");
                        }
                        routes[depth] = current as u16;
                        depth += 1;
                        facts.route_count += 1;
                    }
                }
                current += 1;
            }
            current = index;
            while current < limit {
                let marker = markers.at(current);
                if marker.event.is_primary_enter() {
                    // Active route ends are nonincreasing. The last covering
                    // route is the innermost one, including equal-range ties.
                    let end = Self::end(marker);
                    let mut low = 0usize;
                    let mut high = depth;
                    while low < high {
                        let middle = low + (high - low) / 2;
                        if Self::end(markers.at(routes[middle] as usize)) >= end {
                            low = middle + 1;
                        } else {
                            high = middle;
                        }
                    }
                    if low != 0 && routes[low - 1] as usize == current {
                        low -= 1;
                    }
                    let conflict = if low == 0 {
                        LocalConflict::Unconditional
                    } else {
                        let route = markers.at(routes[low - 1] as usize);
                        let arm = if start >= route.segment_end() { 1 } else { 0 };
                        LocalConflict::route_arm(route.scope_id, Some(arm))
                    };
                    facts.parents[Self::kind_index(marker.scope_id)]
                        [marker.scope_id.local_ordinal() as usize] = Some(conflict);
                }
                current += 1;
            }
            index = limit;
        }
        facts
    }

    const fn end(marker: ScopeMarker) -> usize {
        match marker.event.route_end() {
            Some(end) => end,
            None => marker.segment_end(),
        }
    }

    #[inline(always)]
    const fn kind_index(scope: ScopeId) -> usize {
        match scope.kind() {
            Some(ScopeKind::Route) => 0,
            Some(ScopeKind::Roll) => 1,
            Some(ScopeKind::Parallel) => 2,
            None => crate::invariant(),
        }
    }

    #[inline(always)]
    pub(in crate::global::role_program::image_impl) const fn conflict(
        &self,
        scope: ScopeId,
    ) -> LocalConflict {
        match self.parents[Self::kind_index(scope)][scope.local_ordinal() as usize] {
            Some(conflict) => conflict,
            None => panic!("scope projection fact missing"),
        }
    }

    #[inline(always)]
    pub(in crate::global::role_program::image_impl) const fn first_enter(
        &self,
        scope: ScopeId,
    ) -> usize {
        let index = self.entries[Self::kind_index(scope)][scope.local_ordinal() as usize];
        if index == u16::MAX {
            panic!("scope projection fact missing");
        }
        index as usize
    }

    #[inline(always)]
    pub(super) const fn route_count(&self) -> usize {
        self.route_count
    }
}

#[cfg(all(test, hibana_repo_tests))]
mod tests {
    use super::super::dependency_conflict_for_scope;
    use super::*;
    use crate::{
        eff::{EffAtom, EventOrigin},
        g::{Msg, Par, ProgramSourceData, Roll, Route, Send},
    };

    fn reference_passive_error(
        markers: crate::global::const_dsl::ScopeMarkerView<'_>,
    ) -> Option<u8> {
        use crate::global::const_dsl::{
            StructuredScopeRange, closed_route_arm_ranges_from_first_enter, outermost_scope_range,
            route_arm_event_ranges_for_scope, route_parent_arm_for_scope,
            route_scope_slot_for_scope,
        };
        let error = crate::g::ProgramSourceError::ProjectionRouteUnprojectable as u8;
        let mut slot = 0;
        for index in 0..markers.len() {
            let route = markers.at(index);
            if !route.event.is_primary_enter()
                || !matches!(route.scope_id.kind(), Some(ScopeKind::Route))
            {
                continue;
            }
            let Some(ranges) = route_arm_event_ranges_for_scope(markers, route.scope_id) else {
                return Some(error);
            };
            for (arm, (start, end)) in ranges.into_iter().enumerate() {
                let mut child: Option<StructuredScopeRange> = None;
                for candidate_index in 0..markers.len() {
                    let candidate = markers.at(candidate_index);
                    if candidate.event.is_primary_enter()
                        && matches!(candidate.scope_id.kind(), Some(ScopeKind::Route))
                        && !candidate.scope_id.same(route.scope_id)
                        && candidate.offset() == start
                    {
                        let [_, (_, stop)] =
                            closed_route_arm_ranges_from_first_enter(markers, candidate_index)
                                .unwrap();
                        if stop <= end {
                            let range = StructuredScopeRange::new(candidate.scope_id, start, stop);
                            child = Some(match child {
                                Some(current) => outermost_scope_range(current, range),
                                None => range,
                            });
                        }
                    }
                }
                if let Some(child) = child {
                    let child = child.scope();
                    if child.same(route.scope_id)
                        || route_parent_arm_for_scope(markers, child)
                            != Some((route.scope_id, arm as u8))
                        || !route_scope_slot_for_scope(markers, child).is_some_and(|child_slot| {
                            child_slot > slot && child_slot < u16::MAX as usize
                        })
                    {
                        return Some(error);
                    }
                }
            }
            slot += 1;
        }
        None
    }

    #[test]
    fn cached_parent_facts_match_the_source_definition_for_every_scope() {
        type Body = Roll<
            Par<
                Route<Send<0, 1, Msg<1, ()>>, Send<0, 1, Msg<2, ()>>>,
                Route<
                    Send<0, 2, Msg<3, ()>>,
                    Roll<Route<Send<0, 2, Msg<4, ()>>, Send<0, 2, Msg<5, ()>>>>,
                >,
            >,
        >;
        let source = ProgramSourceData::<32>::lower::<Body>();
        let source = source.eff_list();
        let facts = ScopeFacts::new(source);
        let markers = source.scope_markers();
        let mut routes = 0;
        for index in 0..markers.len() {
            let marker = markers.at(index);
            if marker.event.is_primary_enter() {
                assert_eq!(facts.first_enter(marker.scope_id), index);
                assert_eq!(
                    facts.conflict(marker.scope_id),
                    dependency_conflict_for_scope(markers, source.len(), marker.scope_id)
                );
                if matches!(marker.scope_id.kind(), Some(ScopeKind::Route)) {
                    routes += 1;
                }
            }
        }
        assert_eq!(facts.route_count(), routes);
    }

    #[test]
    fn all_scope_shapes_through_depth_three_match_the_independent_range_scan() {
        use crate::global::const_dsl::ReentryMark;
        use std::vec;

        #[derive(Clone, Copy)]
        enum Shape {
            Send,
            Seq(usize, usize),
            Scope(ScopeKind, usize, Option<usize>),
        }
        fn append(shapes: &[Shape], shape: usize, ordinal: &mut u16, source: &mut EffList<64>) {
            match shapes[shape] {
                Shape::Send => source.push_event_mut(EffAtom {
                    from: 0,
                    to: 1,
                    label: source.len() as u8 + 1,
                    payload: crate::eff::EventPayload::Wire(1),
                    origin: EventOrigin::User,
                    lane: 0,
                }),
                Shape::Seq(left, right) => {
                    append(shapes, left, ordinal, source);
                    append(shapes, right, ordinal, source);
                }
                Shape::Scope(kind, left, right) => {
                    let scope = ScopeId::new(kind, *ordinal * 17 + 5);
                    *ordinal += 1;
                    let start = source.len();
                    append(shapes, left, ordinal, source);
                    let split = source.len();
                    if let Some(right) = right {
                        append(shapes, right, ordinal, source);
                    }
                    let end = source.len();
                    match kind {
                        ScopeKind::Route => source.push_route_scope_mut(
                            scope,
                            start,
                            split,
                            end,
                            ReentryMark::SinglePass,
                        ),
                        ScopeKind::Parallel => {
                            source.push_parallel_scope_mut(scope, start, split, end);
                        }
                        ScopeKind::Roll => source.push_roll_scope_mut(scope, start, end),
                    }
                }
            }
        }
        let mut shapes = vec![Shape::Send];
        let mut level = vec![0];
        for _ in 0..3 {
            let previous = level;
            level = vec![0];
            for &left in &previous {
                level.push(shapes.len());
                shapes.push(Shape::Scope(ScopeKind::Roll, left, None));
                for &right in &previous {
                    level.extend(shapes.len()..shapes.len() + 3);
                    shapes.extend([
                        Shape::Seq(left, right),
                        Shape::Scope(ScopeKind::Route, left, Some(right)),
                        Shape::Scope(ScopeKind::Parallel, left, Some(right)),
                    ]);
                }
            }
        }
        assert_eq!(level.len(), 19_765);
        for shape in level {
            let mut source = EffList::new_partitioned(8, 56, 0);
            append(&shapes, shape, &mut 0, &mut source);
            let facts = ScopeFacts::new(&source);
            let markers = source.scope_markers();
            assert_eq!(
                crate::global::compiled::lowering::validate_passive_child_projection_guarantees(
                    markers
                )
                .map(|error| error as u8),
                reference_passive_error(markers),
                "passive shape={shape}",
            );
            for index in 0..markers.len() {
                let marker = markers.at(index);
                if marker.event.is_primary_enter() {
                    assert_eq!(facts.first_enter(marker.scope_id), index);
                    assert_eq!(
                        facts.conflict(marker.scope_id),
                        dependency_conflict_for_scope(markers, source.len(), marker.scope_id),
                        "shape={shape} scope={:?}",
                        marker.scope_id,
                    );
                }
            }
        }
    }

    #[test]
    fn sparse_last_encoded_scope_ordinal_has_an_exact_cache_slot() {
        let mut source = EffList::<8>::new_partitioned(2, 4, 1);
        let atom = EffAtom {
            from: 0,
            to: 1,
            label: 1,
            payload: crate::eff::EventPayload::Wire(1),
            origin: EventOrigin::User,
            lane: 0,
        };
        source.push_event_mut(atom);
        source.push_event_mut(EffAtom { label: 2, ..atom });
        let scope = ScopeId::route(ScopeId::LOCAL_CAPACITY - 1);
        source.push_route_scope_mut(
            scope,
            0,
            1,
            2,
            crate::global::const_dsl::ReentryMark::SinglePass,
        );
        let facts = ScopeFacts::new(&source);
        assert_eq!(facts.conflict(scope), LocalConflict::Unconditional);
        assert_eq!(
            facts.first_enter(scope),
            source.scope_markers().first_enter_index(scope).unwrap()
        );
        assert_eq!(facts.route_count(), 1);
    }

    #[test]
    #[should_panic(expected = "scope projection fact missing")]
    fn absent_identity_is_rejected_instead_of_becoming_a_root() {
        ScopeFacts::new(&EffList::<1>::new()).conflict(ScopeId::route(0));
    }
}
