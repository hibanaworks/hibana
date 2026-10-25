//! Shared fixed source geometry and the actual production query entrypoint.
//! Both the symbolic proofs and finite execution use these same inputs.
use super::{EffAtom, EffList, Obligation, Paths, SourceActorImage, SourceContext};
use crate::eff::EventOrigin;
#[cfg(not(kani))]
use crate::global::const_dsl::{ReentryMark, ScopeId};

const fn event(from: u8, to: u8) -> EffAtom {
    EffAtom {
        from,
        to,
        label: 0,
        payload: crate::eff::EventPayload::Wire(0),
        origin: EventOrigin::User,
        lane: 0,
    }
}

#[cfg(kani)]
pub(super) const LINEAR: EffList<3> = EffList::<3>::new()
    .push(event(0, 1))
    .push(event(2, 3))
    .push(event(4, 1));
#[cfg(not(kani))]
pub(super) const ROUTE: EffList<8> = {
    let mut events = EffList::<8>::new_partitioned(4, 4, 0)
        .push(event(0, 1))
        .push(event(1, 2))
        .push(event(1, 3))
        .push(event(4, 1));
    events.push_route_scope_mut(ScopeId::route(0), 1, 2, 3, ReentryMark::SinglePass);
    events
};
#[cfg(not(kani))]
pub(super) const PARALLEL: EffList<8> = {
    let mut events = EffList::<8>::new_partitioned(4, 3, 0)
        .push(event(0, 1))
        .push(event(1, 2))
        .push(event(3, 4))
        .push(event(5, 1));
    events.push_parallel_scope_mut(ScopeId::parallel(0), 1, 2, 3);
    events
};

#[cfg(not(kani))]
pub(super) const fn prefix_source(repeated: bool) -> EffList<12> {
    let mut events = EffList::<12>::new_partitioned(4, 5, 0)
        .push(event(0, 0))
        .push(event(0, 1))
        .push(event(2, 1))
        .push(event(3, 1));
    events.push_parallel_scope_mut(ScopeId::parallel(1), 0, 2, 3);
    if repeated {
        events.push_roll_scope_mut(ScopeId::roll_scope(0), 0, 4);
    }
    events
}

pub(super) fn receive_conflict<const E: usize, const D: usize>(
    events: &EffList<E>,
    actors: &SourceActorImage<E, D>,
) -> bool {
    let empty = SourceContext::new(&[]);
    Obligation {
        events,
        actors: Some(actors),
        earlier: 0,
        end: events.len(),
        repeated: None,
    }
    .check(
        Paths {
            shared: &empty,
            current: &empty,
            next: &empty,
        },
        None,
    )
}
