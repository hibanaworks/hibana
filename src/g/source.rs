use crate::{
    eff::EffAtom,
    global::const_dsl::{
        EffList, ReentryMark, ScopeId, ScopeKind, color_roll_frame_labels, merge_parallel_lanes,
        merge_route_frame_labels, separate_roll_frame_domains,
    },
};

pub(super) mod ownership;
pub(crate) use ownership::{
    Arm as SourceArm, Context as SourceContext, Scope as SourceScope, Selection as SourceSelection,
};
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
    actors: Option<&'static SourceActorImage<CAPACITY>>,
}

pub(crate) type SourceActorImage<
    const E: usize,
    const D: usize = { crate::global::compiled::images::ACTOR_DECISION_CAPACITY },
> = ownership::ActorImage<E, D>;

struct SourceActors<Steps, const CAPACITY: usize>(core::marker::PhantomData<Steps>);

impl<Steps: ProgramShape, const CAPACITY: usize> SourceActors<Steps, CAPACITY> {
    const IMAGE: Result<Option<SourceActorImage<CAPACITY>>, ownership::Error> =
        if !Steps::SOURCE_NODE.has_delegation() {
            // References to associated constants can be evaluated eagerly even in
            // an untaken caller branch. Keep the guard inside the referenced value
            // so ordinary sources never enter ownership analysis or build its DAG.
            Ok(None)
        } else {
            let mut scratch = ownership::Scratch::<256, CAPACITY>::new();
            let mut image = ownership::ActorImage::new();
            match ownership::compile_actor_image_into(&Steps::SOURCE_NODE, &mut scratch, &mut image)
            {
                Ok(()) => Ok(Some(image)),
                Err(error) => Err(error),
            }
        };
}

struct SourceLowering<const CAPACITY: usize> {
    eff: EffList<CAPACITY>,
    actors: Option<&'static SourceActorImage<CAPACITY>>,
    next_scope_ordinal: u16,
}

impl<const CAPACITY: usize> SourceLowering<CAPACITY> {
    const fn new(
        event_count: usize,
        scope_count: usize,
        resolver_count: usize,
        actors: Option<&'static SourceActorImage<CAPACITY>>,
    ) -> Self {
        Self {
            eff: EffList::new_partitioned(event_count, scope_count, resolver_count),
            actors,
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
                let payload = match message.payload {
                    SourcePayload::Wire { schema } => crate::eff::EventPayload::Wire(schema),
                    SourcePayload::Endpoint(key) => {
                        if key.component != 0 {
                            panic!("owned endpoint is outside the composed choreography");
                        }
                        crate::eff::EventPayload::Endpoint(key.role)
                    }
                };
                self.eff.push_event_mut(EffAtom {
                    from: message.from.role,
                    to: message.to.role,
                    label: message.label,
                    payload,
                    origin: crate::eff::EventOrigin::User,
                    lane: 0,
                });
                1
            }
            ProgramSourceNode::Seq(left, right, _) => {
                let left_span = self.emit(left, route_reentry);
                let right_span = self.emit(right, route_reentry);
                max_lane_span(left_span, right_span)
            }
            ProgramSourceNode::Route {
                left,
                right,
                resolver,
                ..
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
            ProgramSourceNode::Par(left, right, _) => {
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
                    self.actors,
                    left_start,
                    right_start,
                    right_end,
                    left_span,
                    right_span,
                )
            }
            ProgramSourceNode::Roll(inner, _) => {
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
    pub(crate) const fn ownership_diagnostic<Steps: ProgramShape>()
    -> Option<super::ProjectionDiagnostic> {
        match &SourceActors::<Steps, CAPACITY>::IMAGE {
            Err(error) => Some(super::ProjectionDiagnostic::from_ownership_error(*error)),
            Ok(_) => None,
        }
    }

    pub(crate) const fn lower<Steps>() -> Self
    where
        Steps: ProgramShape,
    {
        if Steps::SOURCE_ROW_COUNT == 0 || Steps::SOURCE_ROW_COUNT > CAPACITY {
            panic!("source bucket selection");
        }
        let actors = match &SourceActors::<Steps, CAPACITY>::IMAGE {
            Ok(Some(image)) => Some(image),
            Ok(None) => None,
            Err(error) => super::diagnostic_message::panic_diagnostic(
                super::ProjectionDiagnostic::from_ownership_error(*error),
            ),
        };
        let mut lowering = SourceLowering::new(
            Steps::EVENT_COUNT,
            Steps::SCOPE_MARKER_COUNT,
            Steps::RESOLVER_MARKER_COUNT,
            actors,
        );
        let _ = lowering.emit(&Steps::SOURCE_NODE, ReentryMark::SinglePass);
        if lowering.eff.len() != Steps::EVENT_COUNT
            || lowering.eff.scope_marker_count() != Steps::SCOPE_MARKER_COUNT
            || lowering.eff.resolver_marker_count() != Steps::RESOLVER_MARKER_COUNT
        {
            panic!("type tree and lowered source disagree");
        }
        separate_roll_frame_domains(&mut lowering.eff);
        Self {
            eff: lowering.eff,
            actors,
        }
    }

    #[inline(always)]
    pub(crate) const fn eff_list(&self) -> &EffList<CAPACITY> {
        &self.eff
    }

    pub(crate) const fn actors(&self) -> Option<&'static SourceActorImage<CAPACITY>> {
        self.actors
    }

    pub(crate) const fn actor_branch_knowledge(&self, source: &ProgramSourceNode) -> bool {
        match self.actors {
            Some(actors) => actors.has_branch_knowledge(source),
            None => false,
        }
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
