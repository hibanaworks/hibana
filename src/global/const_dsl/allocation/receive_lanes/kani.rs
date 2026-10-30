use super::{FlowIndex, LogicalDirections, Participants, align_range, direction_members};
use crate::{
    eff::{EffAtom, EventOrigin, EventPayload},
    global::const_dsl::{EffList, ScopeId},
};

#[kani::proof]
// Two events share an eight-row source arena. Seventeen unwinds also cover
// the direction sort's sixteen histogram buckets; assertions stay enabled.
#[kani::unwind(17)]
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
    let candidates = direction_members(
        &events,
        &FlowIndex::new::<0>(&events, None),
        &LogicalDirections::new(&events),
        (0, first.lane, color),
        &mut Participants::empty(events.len()),
    );
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

fn certify_every_receiver<const WORD: u8>() {
    let receiver = WORD * 64 | (kani::any::<u8>() & 63);
    let row = |from, to, lane| EffAtom {
        from,
        to,
        lane,
        label: 0,
        payload: EventPayload::Wire(0),
        origin: EventOrigin::User,
    };
    let mut events = EffList::<4>::new()
        .push(row(receiver.wrapping_add(1), receiver, 7))
        .push(row(receiver.wrapping_add(2), receiver, 0))
        .push(row(receiver.wrapping_add(3), receiver.wrapping_add(4), 7))
        .push(row(receiver.wrapping_add(5), receiver, 7));
    let scopes = FlowIndex::new::<0>(&events, None);
    assert_eq!(
        align_range(&mut events, &scopes, None, (2, 4), (0, 3), (8, 8), None,),
        Some(2)
    );
    assert_eq!(events.atom_at(0).lane, 7);
    assert_eq!(events.atom_at(1).lane, 0);
    assert_eq!(events.atom_at(2).lane, 1);
    assert_eq!(events.atom_at(3).lane, 1);
}

// The four partitions cover every u8 receiver. Relative peers remain distinct
// under wrapping, including assignments crossing the end of the wire domain.
macro_rules! receiver_word {
    ($name:ident, $word:literal) => {
        #[kani::proof]
        #[kani::unwind(9)]
        fn $name() {
            certify_every_receiver::<$word>();
        }
    };
}
receiver_word!(lane_class_checks_every_receiver_word0, 0);
receiver_word!(lane_class_checks_every_receiver_word1, 1);
receiver_word!(lane_class_checks_every_receiver_word2, 2);
receiver_word!(lane_class_checks_every_receiver_word3, 3);
