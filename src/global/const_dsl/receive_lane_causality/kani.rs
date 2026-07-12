use super::validate_receive_lane_causality;
use crate::{
    eff::{EffAtom, EffStruct, EventOrigin},
    global::const_dsl::EffList,
};

fn symbolic_role() -> u8 {
    kani::any::<u8>() % crate::g::ROLE_DOMAIN_SIZE
}

fn event(from: u8, to: u8, lane: u8) -> EffStruct {
    EffStruct::atom(EffAtom {
        from,
        to,
        label: 0,
        payload_schema: 0,
        origin: EventOrigin::User,
        lane,
    })
}

#[kani::proof]
fn three_event_causal_handoff_accepts_every_valid_role_assignment() {
    let earlier_source = symbolic_role();
    let receiver = symbolic_role();
    let later_source = symbolic_role();
    let lane = kani::any();
    kani::assume(earlier_source != receiver);
    kani::assume(later_source != receiver);
    kani::assume(earlier_source != later_source);

    let events = EffList::new()
        .push(event(earlier_source, receiver, lane))
        .push(event(receiver, later_source, lane))
        .push(event(later_source, receiver, lane));

    assert!(validate_receive_lane_causality(&events));
}

#[kani::proof]
fn sender_change_without_causal_handoff_is_rejected() {
    let earlier_source = symbolic_role();
    let receiver = symbolic_role();
    let later_source = symbolic_role();
    let lane = kani::any();
    kani::assume(earlier_source != receiver);
    kani::assume(later_source != receiver);
    kani::assume(earlier_source != later_source);

    let events = EffList::new()
        .push(event(earlier_source, receiver, lane))
        .push(event(later_source, receiver, lane));

    assert!(!validate_receive_lane_causality(&events));
}
