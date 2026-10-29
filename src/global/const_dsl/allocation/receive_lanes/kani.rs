use super::direction_members;
use crate::{
    eff::{EffAtom, EventOrigin, EventPayload},
    global::const_dsl::{EffList, ScopeId},
};

#[kani::proof]
#[kani::unwind(40)]
fn direction_candidates_preserve_every_symbolic_parallel_exclusion() {
    fn event() -> EffAtom {
        EffAtom {
            from: kani::any(),
            to: kani::any(),
            label: 0,
            lane: kani::any(),
            payload: EventPayload::Wire(0),
            origin: EventOrigin::User,
        }
    }
    let first = event();
    let second = event();
    // Every color other than the current lane has a preimage in this generator.
    let offset = kani::any::<u8>() % 255 + 1;
    let color = first.lane.wrapping_add(offset);
    let mut events = EffList::<8>::new_partitioned(2, 6, 0)
        .push(first)
        .push(second);
    events.push_parallel_scope_mut(ScopeId::parallel(0), 0, 1, 2);
    let candidates = direction_members(&events, None, (first, color));
    let overlap = first.from == second.from
        || first.from == second.to
        || first.to == second.from
        || first.to == second.to;
    assert_eq!(candidates[0], !(second.lane == color && overlap));
    assert_eq!(
        candidates[1],
        second.from == first.from && second.to == first.to && second.lane == first.lane
    );
    kani::cover!(candidates[0]);
    kani::cover!(!candidates[0]);
    kani::cover!(candidates[1]);
}
