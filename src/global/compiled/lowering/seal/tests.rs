use super::*;
use crate::{
    eff::{EffAtom, EventOrigin},
    global::const_dsl::{
        ReentryMark, ScopeId, validate_parallel_endpoint_selectors,
        validate_receive_lane_causality, validate_roll_reentry_endpoint_selectors,
    },
};

// Apply the production obligations to raw EffList fixtures for comparison with
// the independent oracle. Typed projection caches each obligation separately.
const fn projection_error_all_roles<const E: usize>(
    summary: &CompiledProgramImage,
    source: &EffList<E>,
) -> Option<ProgramSourceError> {
    if !validate_receive_lane_causality(source) {
        return Some(ProgramSourceError::ReceiveLaneCausalityConflict);
    }
    if !validate_parallel_endpoint_selectors(source) {
        return Some(ProgramSourceError::ParallelAmbiguousEndpointSelector);
    }
    if !validate_roll_reentry_endpoint_selectors(source) {
        return Some(ProgramSourceError::ReentryAmbiguousEndpointSelector);
    }
    if let Some(error) = validate_passive_child_projection_guarantees(source.scope_markers()) {
        return Some(error);
    }
    validate_route_projection_guarantees(summary, source)
}

const ROUTE_DEPTH: usize = 257;
const EVENT_COUNT: usize = ROUTE_DEPTH + 1;
const SCOPE_MARKER_COUNT: usize = ROUTE_DEPTH * 4;
const SOURCE_CAPACITY: usize = EVENT_COUNT + SCOPE_MARKER_COUNT + ROUTE_DEPTH;

fn route_commit_count_257_source() -> EffList<SOURCE_CAPACITY> {
    let mut source = EffList::new_partitioned(EVENT_COUNT, SCOPE_MARKER_COUNT, ROUTE_DEPTH);
    let mut event = 0usize;
    while event < EVENT_COUNT {
        source.push_event_mut(EffAtom {
            from: 0,
            to: 0,
            label: event as u8,
            payload: crate::eff::EventPayload::Wire(0),
            origin: EventOrigin::User,
            lane: 0,
        });
        event += 1;
    }

    let mut route = 0usize;
    while route < ROUTE_DEPTH {
        let scope = ScopeId::route(route as u16);
        source.push_route_scope_mut(
            scope,
            route,
            route + 1,
            EVENT_COUNT,
            ReentryMark::SinglePass,
        );
        source.push_route_resolver_mut(scope, route as u16);
        route += 1;
    }

    source
}

#[test]
fn projection_derives_exact_route_capacity_beyond_256_rows() {
    let source = route_commit_count_257_source();
    let summary = CompiledProgramImage::scan_const(&source);

    assert_eq!(
        projection_error_all_roles(&summary, &source).map(|error| error as u8),
        None
    );
    assert_eq!(
        summary
            .role_lowering_counts(
                &source,
                &crate::global::role_program::ScopeFacts::new(&source),
                crate::global::role_program::LocalRows::Role(0)
            )
            .max_route_commit_count,
        257
    );
}

#[test]
fn projection_derives_one_row_for_one_route() {
    let mut source = EffList::<6>::new_partitioned(2, 4, 0);
    for label in 0..2 {
        source.push_event_mut(EffAtom {
            from: 0,
            to: 0,
            label,
            payload: crate::eff::EventPayload::Wire(0),
            origin: EventOrigin::User,
            lane: 0,
        });
    }
    source.push_route_scope_mut(ScopeId::route(0), 0, 1, 2, ReentryMark::SinglePass);

    let summary = CompiledProgramImage::scan_const(&source);
    assert_eq!(
        summary
            .role_lowering_counts(
                &source,
                &crate::global::role_program::ScopeFacts::new(&source),
                crate::global::role_program::LocalRows::Role(0)
            )
            .max_route_commit_count,
        1
    );
}

#[test]
fn scope_first_route_validation_does_not_skip_later_observers() {
    use crate::g::{Msg, ProgramSourceData, Route, Send, Seq};
    type MissingRightObserver =
        Route<Seq<Send<0, 1, Msg<1, ()>>, Send<1, 2, Msg<2, ()>>>, Send<0, 1, Msg<3, ()>>>;
    let source = ProgramSourceData::<16>::lower::<MissingRightObserver>();
    let summary = CompiledProgramImage::scan_const(source.eff_list());
    assert!(matches!(
        projection_error_all_roles(&summary, source.eff_list()),
        Some(ProgramSourceError::ProjectionRouteUnprojectable)
    ));
}

mod participant_mask;
