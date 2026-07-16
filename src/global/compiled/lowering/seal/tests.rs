use super::*;
use crate::{
    eff::{EffAtom, EffStruct, EventOrigin},
    global::const_dsl::{ReentryMark, ScopeId, merge_route_frame_labels},
};

const ROUTE_DEPTH: usize = 256;
const EVENT_COUNT: usize = ROUTE_DEPTH + 1;
const SCOPE_MARKER_COUNT: usize = ROUTE_DEPTH * 4;
const SOURCE_CAPACITY: usize = EVENT_COUNT + SCOPE_MARKER_COUNT + ROUTE_DEPTH;

fn route_commit_count_257_source() -> EffList<SOURCE_CAPACITY> {
    let mut source = EffList::new_partitioned(EVENT_COUNT, SCOPE_MARKER_COUNT, ROUTE_DEPTH);
    let mut event = 0usize;
    while event < EVENT_COUNT {
        source.push_event_mut(EffStruct::atom(EffAtom {
            from: 0,
            to: 0,
            label: event as u8,
            payload_schema: 0,
            origin: EventOrigin::User,
            lane: 0,
        }));
        event += 1;
    }

    let mut route = 0usize;
    while route < ROUTE_DEPTH {
        let scope = ScopeId::route(route as u16);
        source.push_scope_enter_reentry_mut(route, scope, ReentryMark::SinglePass);
        source.close_scope_segment_mut(scope, route, route + 1);
        source.push_scope_exit_mut(route + 1, scope);
        source.push_scope_enter_reentry_mut(route + 1, scope, ReentryMark::SinglePass);
        source.close_scope_segment_mut(scope, route + 1, EVENT_COUNT);
        source.push_scope_exit_mut(EVENT_COUNT, scope);
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
fn projection_accepts_a_route_chain_beyond_the_former_256_bound() {
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
