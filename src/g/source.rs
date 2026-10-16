use crate::{
    eff::EffAtom,
    global::const_dsl::{
        EffList, ReentryMark, ScopeId, ScopeKind, color_roll_frame_labels, merge_parallel_lanes,
        merge_route_frame_labels, separate_roll_frame_domains,
    },
};

mod ownership;
mod tree;
pub(crate) use tree::{EndpointKey, Message as SourceMessage, Payload as SourcePayload};
pub type ProgramSourceNode = tree::Source<'static>;

#[cfg(all(test, hibana_repo_tests))]
mod tests;

// Public within this private module so the sealed composition contract can name
// its associated step-list without exposing the source builders as an API.
pub trait ProgramShape {
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
            ProgramSourceNode::Send(message) => {
                if message.from.component != 0 || message.to.component != 0 {
                    panic!("source component is outside the composed choreography");
                }
                let SourcePayload::Wire { schema } = message.payload else {
                    panic!("owned endpoint requires ownership-aware lowering");
                };
                self.eff.push_event_mut(EffAtom {
                    from: message.from.role,
                    to: message.to.role,
                    label: message.label,
                    payload_schema: schema,
                    origin: crate::eff::EventOrigin::User,
                    lane: 0,
                });
                1
            }
            ProgramSourceNode::Seq(left, right) => {
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
                let left_span = self.emit(left, route_reentry);
                let right_start = self.eff.len();
                let right_span = self.emit(right, route_reentry);
                let right_end = self.eff.len();
                self.eff.push_route_scope_mut(
                    scope,
                    left_start,
                    right_start,
                    right_end,
                    route_reentry,
                );
                merge_route_frame_labels(&mut self.eff, left_start, right_start, right_end);
                if let Some(resolver_id) = *resolver {
                    self.eff.push_route_resolver_mut(scope, resolver_id);
                }
                max_lane_span(left_span, right_span)
            }
            ProgramSourceNode::Par(left, right) => {
                let scope = self.allocate_scope(ScopeKind::Parallel);
                let left_start = self.eff.len();
                let left_span = self.emit(left, route_reentry);
                let right_start = self.eff.len();
                let right_span = self.emit(right, route_reentry);
                let right_end = self.eff.len();
                self.eff
                    .push_parallel_scope_mut(scope, left_start, right_start, right_end);
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
                let lane_span = self.emit(inner, ReentryMark::Reentrant);
                let end = self.eff.len();
                self.eff.push_roll_scope_mut(scope, start, end);
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
        if Steps::SOURCE_NODE.has_delegation() {
            let mut scratch = ownership::Scratch::<256, CAPACITY>::new();
            match ownership::validate(&Steps::SOURCE_NODE, &mut scratch) {
                Ok(()) => (),
                Err(error) => match error.kind {
                    ownership::ErrorKind::Capacity => panic!("owned endpoint source capacity"),
                    ownership::ErrorKind::NonUnitSelfMessage => {
                        panic!("self-role actions require the unit payload")
                    }
                    ownership::ErrorKind::EndpointIsItsOwnCarrier => {
                        panic!("an owned endpoint cannot carry its own delegation")
                    }
                    ownership::ErrorKind::ConflictingInitialOwners => {
                        panic!("owned endpoint has conflicting initial actors")
                    }
                    ownership::ErrorKind::UnrootedInitialOwner => {
                        panic!("owned endpoint has no unique initial actor")
                    }
                    ownership::ErrorKind::ParallelOwnershipConflict => {
                        panic!("parallel endpoint delegation conflicts with another use")
                    }
                    ownership::ErrorKind::RollChangesOwner => {
                        panic!("rolled endpoint delegation must restore its incoming ownership")
                    }
                },
            }
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
        separate_roll_frame_domains(&mut lowering.eff);
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
