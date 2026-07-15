use crate::{
    eff::{EffAtom, EffStruct},
    global::const_dsl::{
        EffList, INTRINSIC_ROUTE_RESOLVER_ID, ReentryMark, ScopeId, ScopeKind,
        color_roll_frame_labels, merge_parallel_lanes, merge_route_frame_labels,
    },
};

#[derive(Clone, Copy)]
pub(crate) enum SourceRouteResolver {
    Intrinsic,
    Dynamic(u16),
}

#[derive(Clone, Copy)]
pub(crate) enum ProgramSourceNode {
    Send(EffAtom),
    Seq {
        left: &'static Self,
        right: &'static Self,
    },
    Route {
        left: &'static Self,
        right: &'static Self,
        resolver: SourceRouteResolver,
    },
    Parallel {
        left: &'static Self,
        right: &'static Self,
    },
    Roll(&'static Self),
}

pub(crate) trait ProgramShape {
    const SOURCE_NODE: ProgramSourceNode;
    const EVENT_COUNT: usize;
    const SCOPE_MARKER_COUNT: usize;
    const RESOLVER_MARKER_COUNT: usize;

    const SOURCE_ROW_COUNT: usize = checked_source_count(
        Self::EVENT_COUNT,
        Self::SCOPE_MARKER_COUNT,
        Self::RESOLVER_MARKER_COUNT,
    );
}

pub(crate) struct ProgramSourceData<const CAPACITY: usize> {
    eff: EffList<CAPACITY>,
}

struct SourceLowering<const CAPACITY: usize> {
    eff: EffList<CAPACITY>,
    next_scope_ordinal: u16,
}

impl<const CAPACITY: usize> SourceLowering<CAPACITY> {
    const fn new(event_count: usize, scope_count: usize, resolver_count: usize) -> Self {
        Self {
            eff: EffList::new_partitioned(event_count, scope_count, resolver_count),
            next_scope_ordinal: 0,
        }
    }

    const fn allocate_scope(&mut self, kind: ScopeKind) -> ScopeId {
        if self.next_scope_ordinal >= ScopeId::LOCAL_CAPACITY {
            panic!("structured scope domain exceeded");
        }
        let ordinal = self.next_scope_ordinal;
        self.next_scope_ordinal += 1;
        ScopeId::new(kind, ordinal)
    }

    const fn emit(&mut self, node: &ProgramSourceNode, route_reentry: ReentryMark) -> u16 {
        match node {
            ProgramSourceNode::Send(atom) => {
                self.eff.push_event_mut(EffStruct::atom(*atom));
                1
            }
            ProgramSourceNode::Seq { left, right } => {
                let left_span = self.emit(left, route_reentry);
                let right_span = self.emit(right, route_reentry);
                max_lane_span(left_span, right_span)
            }
            ProgramSourceNode::Route {
                left,
                right,
                resolver,
            } => {
                let scope = self.allocate_scope(ScopeKind::Route);
                let left_start = self.eff.len();
                self.eff
                    .push_scope_enter_reentry_mut(left_start, scope, route_reentry);
                let left_span = self.emit(left, route_reentry);
                let right_start = self.eff.len();
                self.eff
                    .close_scope_segment_mut(scope, left_start, right_start);
                self.eff.push_scope_exit_mut(right_start, scope);
                self.eff
                    .push_scope_enter_reentry_mut(right_start, scope, route_reentry);
                let right_span = self.emit(right, route_reentry);
                let right_end = self.eff.len();
                self.eff
                    .close_scope_segment_mut(scope, right_start, right_end);
                self.eff.push_scope_exit_mut(right_end, scope);
                merge_route_frame_labels(&mut self.eff, left_start, right_start, right_end);
                if let SourceRouteResolver::Dynamic(resolver_id) = *resolver {
                    if resolver_id == INTRINSIC_ROUTE_RESOLVER_ID {
                        panic!("route resolver id must be < u16::MAX");
                    }
                    self.eff.push_route_resolver_mut(scope, resolver_id);
                }
                max_lane_span(left_span, right_span)
            }
            ProgramSourceNode::Parallel { left, right } => {
                let scope = self.allocate_scope(ScopeKind::Parallel);
                let left_start = self.eff.len();
                self.eff
                    .push_scope_enter_reentry_mut(left_start, scope, ReentryMark::SinglePass);
                let left_span = self.emit(left, route_reentry);
                let right_start = self.eff.len();
                self.eff.push_scope_split_mut(right_start, scope);
                let right_span = self.emit(right, route_reentry);
                let right_end = self.eff.len();
                self.eff
                    .close_scope_segment_mut(scope, left_start, right_end);
                self.eff.push_scope_exit_mut(right_end, scope);
                merge_parallel_lanes(
                    &mut self.eff,
                    left_start,
                    right_start,
                    right_end,
                    left_span,
                    right_span,
                )
            }
            ProgramSourceNode::Roll(inner) => {
                let scope = self.allocate_scope(ScopeKind::Roll);
                let start = self.eff.len();
                self.eff
                    .push_scope_enter_reentry_mut(start, scope, ReentryMark::SinglePass);
                let lane_span = self.emit(inner, ReentryMark::Reentrant);
                let end = self.eff.len();
                self.eff.close_scope_segment_mut(scope, start, end);
                self.eff.push_scope_exit_mut(end, scope);
                color_roll_frame_labels(&mut self.eff, start, end);
                lane_span
            }
        }
    }
}

impl<const CAPACITY: usize> ProgramSourceData<CAPACITY> {
    pub(crate) const fn lower<Steps>() -> Self
    where
        Steps: ProgramShape,
    {
        if Steps::SOURCE_ROW_COUNT == 0 || Steps::SOURCE_ROW_COUNT > CAPACITY {
            panic!("source bucket selection");
        }
        let mut lowering = SourceLowering::new(
            Steps::EVENT_COUNT,
            Steps::SCOPE_MARKER_COUNT,
            Steps::RESOLVER_MARKER_COUNT,
        );
        let _ = lowering.emit(&Steps::SOURCE_NODE, ReentryMark::SinglePass);
        if lowering.eff.len() != Steps::EVENT_COUNT
            || lowering.eff.scope_marker_count() != Steps::SCOPE_MARKER_COUNT
            || lowering.eff.resolver_marker_count() != Steps::RESOLVER_MARKER_COUNT
        {
            panic!("type tree and lowered source disagree");
        }
        Self { eff: lowering.eff }
    }

    #[inline(always)]
    pub(crate) const fn eff_list(&self) -> &EffList<CAPACITY> {
        &self.eff
    }
}

const fn max_lane_span(lhs: u16, rhs: u16) -> u16 {
    if lhs >= rhs { lhs } else { rhs }
}

pub(crate) const fn checked_source_count(lhs: usize, rhs: usize, added: usize) -> usize {
    let Some(partial) = lhs.checked_add(rhs) else {
        panic!("choreography source count overflow");
    };
    let Some(sum) = partial.checked_add(added) else {
        panic!("choreography source count overflow");
    };
    sum
}
