use super::{
    FirstCausalWitnesses, RollBodyRange, receive_precedes_after_roll_reentry,
    validate_linear_receive_lane_causality, validate_receive_lane_causality,
    validate_structured_receive_lane_causality,
};
use crate::{
    eff::{EffAtom, EventOrigin},
    global::const_dsl::EffList,
};

fn event(from: u8, to: u8, lane: u8) -> EffAtom {
    EffAtom {
        from,
        to,
        label: 0,
        payload_schema: 0,
        origin: EventOrigin::User,
        lane,
    }
}

fn distinct_role_permutation() -> (u8, u8, u8) {
    match kani::any::<u8>() % 6 {
        0 => (0, 1, 2),
        1 => (0, 2, 1),
        2 => (1, 0, 2),
        3 => (1, 2, 0),
        4 => (2, 0, 1),
        _ => (2, 1, 0),
    }
}

fn distinct_roles() -> (u8, u8, u8) {
    let earlier_source = kani::any::<u8>();
    let receiver = kani::any::<u8>();
    let later_source = kani::any::<u8>();
    if earlier_source != receiver && earlier_source != later_source && receiver != later_source {
        (earlier_source, receiver, later_source)
    } else {
        (0, 1, 2)
    }
}

#[kani::proof]
fn causal_witness_table_is_first_write_wins_and_role_exact() {
    let first_role = kani::any::<u8>();
    let second_role = kani::any::<u8>();
    let query_role = kani::any::<u8>();
    let first_candidate = kani::any::<u32>();
    let second_candidate = kani::any::<u32>();
    let first_idx = if first_candidate == u32::MAX {
        0
    } else {
        first_candidate
    };
    let second_idx = if second_candidate == u32::MAX {
        0
    } else {
        second_candidate
    };

    let mut witnesses = FirstCausalWitnesses::new(first_role, first_idx as usize);
    witnesses.record_first(first_role, second_idx as usize);
    witnesses.record_first(second_role, second_idx as usize);

    let expected = if query_role == first_role {
        Some(first_idx as usize)
    } else if query_role == second_role {
        Some(second_idx as usize)
    } else {
        None
    };
    assert_eq!(witnesses.first(query_role), expected);
}

#[kani::proof]
fn three_event_linear_scan_matches_pairwise_checker() {
    let events = EffList::<4>::new()
        .push(event(kani::any(), kani::any(), kani::any()))
        .push(event(kani::any(), kani::any(), kani::any()))
        .push(event(kani::any(), kani::any(), kani::any()));

    assert_eq!(
        validate_linear_receive_lane_causality(&events),
        validate_structured_receive_lane_causality(&events)
    );
}

#[kani::proof]
fn three_event_causal_handoff_accepts_every_valid_role_assignment() {
    let (earlier_source, receiver, later_source) = distinct_roles();
    let lane = kani::any();

    let events = EffList::<4>::new()
        .push(event(earlier_source, receiver, lane))
        .push(event(receiver, later_source, lane))
        .push(event(later_source, receiver, lane));

    assert!(validate_receive_lane_causality(&events));
}

#[kani::proof]
fn sender_change_without_causal_handoff_is_rejected() {
    let (earlier_source, receiver, later_source) = distinct_roles();
    let lane = kani::any();

    let events = EffList::<4>::new()
        .push(event(earlier_source, receiver, lane))
        .push(event(later_source, receiver, lane));

    assert!(!validate_receive_lane_causality(&events));
}

#[kani::proof]
fn roll_reentry_causal_closure_rejects_open_cycle() {
    let (first_source, receiver, last_source) = distinct_role_permutation();

    let events = EffList::<4>::new()
        .push(event(first_source, receiver, 0))
        .push(event(receiver, last_source, 0))
        .push(event(last_source, receiver, 0));

    assert!(!receive_precedes_after_roll_reentry(
        &events,
        RollBodyRange::new(0, 3),
        2,
        0,
    ));
}

#[kani::proof]
fn roll_reentry_causal_closure_accepts_closed_cycle() {
    let (first_source, receiver, last_source) = distinct_role_permutation();

    let events = EffList::<4>::new()
        .push(event(first_source, receiver, 0))
        .push(event(receiver, last_source, 0))
        .push(event(last_source, receiver, 0))
        .push(event(receiver, first_source, 0));

    assert!(receive_precedes_after_roll_reentry(
        &events,
        RollBodyRange::new(0, 4),
        2,
        0,
    ));
}
