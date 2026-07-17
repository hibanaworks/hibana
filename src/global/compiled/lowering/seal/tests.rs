use super::*;
use crate::{
    eff::{EffAtom, EventOrigin},
    global::const_dsl::{ReentryMark, ScopeId, merge_route_frame_labels},
};

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
            payload_schema: 0,
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

    route = ROUTE_DEPTH;
    while route > 0 {
        route -= 1;
        merge_route_frame_labels(&mut source, route, route + 1, EVENT_COUNT);
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
            .role_lowering_counts(&source, 0)
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
            payload_schema: 0,
            origin: EventOrigin::User,
            lane: 0,
        });
    }
    source.push_route_scope_mut(ScopeId::route(0), 0, 1, 2, ReentryMark::SinglePass);

    let summary = CompiledProgramImage::scan_const(&source);
    assert_eq!(
        summary
            .role_lowering_counts(&source, 0)
            .max_route_commit_count,
        1
    );
}
