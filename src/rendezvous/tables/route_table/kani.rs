use super::{RouteEntry, RouteTable};

#[kani::proof]
fn selected_local_participant_mask_is_exact_intersection() {
    let selected_participants: u16 = kani::any();
    let attached_roles: u16 = kani::any();
    let owner_role: u8 = kani::any();
    kani::assume(owner_role < u16::BITS as u8);
    let owner_role_bit = 1u16 << owner_role;
    kani::assume((selected_participants & owner_role_bit) != 0);
    kani::assume((attached_roles & owner_role_bit) != 0);

    let local = RouteTable::selected_local_participant_mask(
        selected_participants,
        attached_roles,
        owner_role_bit,
    );
    assert!(local == (selected_participants & attached_roles));
    assert!((local & !selected_participants) == 0);
    assert!((local & !attached_roles) == 0);
    assert!((local & owner_role_bit) != 0);
}

#[kani::proof]
fn active_route_entry_cannot_be_overwritten() {
    let arm: u8 = kani::any();
    let observed_mask: u16 = kani::any();
    let replacement_arm: u8 = kani::any();
    let replacement_observed: u16 = kani::any();
    kani::assume(arm <= 1);
    kani::assume(observed_mask != 0);

    let active = RouteEntry { arm, observed_mask };
    assert!(
        active
            .try_begin(replacement_observed, replacement_arm)
            .is_none()
    );
}

#[kani::proof]
fn empty_route_entry_begin_is_exact() {
    let arm: u8 = kani::any();
    let observed_mask: u16 = kani::any();
    kani::assume(arm <= 1);
    kani::assume(observed_mask != 0);

    let begun = RouteEntry::EMPTY
        .try_begin(observed_mask, arm)
        .expect("valid begin");
    assert!(begun.arm == arm);
    assert!(begun.observed_mask == observed_mask);
}

#[kani::proof]
fn active_route_entry_rejects_conflicting_arm_observation() {
    let arm: u8 = kani::any();
    let observed_mask: u16 = kani::any();
    let additional_observed: u16 = kani::any();
    kani::assume(arm <= 1);
    kani::assume(observed_mask != 0);
    kani::assume(additional_observed != 0);

    let active = RouteEntry { arm, observed_mask };
    assert!(active.try_observe(additional_observed, arm ^ 1).is_none());
}

#[kani::proof]
fn active_route_entry_observation_is_arm_preserving_and_monotone() {
    let arm: u8 = kani::any();
    let observed_mask: u16 = kani::any();
    let additional_observed: u16 = kani::any();
    kani::assume(arm <= 1);
    kani::assume(observed_mask != 0);
    kani::assume(additional_observed != 0);

    let active = RouteEntry { arm, observed_mask };
    let extended = active
        .try_observe(additional_observed, arm)
        .expect("valid extension");
    assert!(extended.arm == arm);
    assert!((extended.observed_mask & observed_mask) == observed_mask);
    assert!((extended.observed_mask & additional_observed) == additional_observed);
}

#[kani::proof]
fn route_role_consumption_is_affine_and_arm_preserving() {
    let arm: u8 = kani::any();
    let observed_mask: u16 = kani::any();
    let role: u8 = kani::any();
    kani::assume(arm <= 1);
    kani::assume(observed_mask != 0);
    kani::assume(role < u16::BITS as u8);
    let role_bit = 1u16 << role;
    kani::assume((observed_mask & role_bit) == 0);

    let active = RouteEntry { arm, observed_mask };
    let (consumed, selected_arm) = active
        .try_consume_role(role_bit)
        .expect("first observation");
    assert!(selected_arm == arm);
    assert!(consumed.arm == arm);
    assert!(consumed.observed_mask == (observed_mask | role_bit));
    assert!(consumed.try_consume_role(role_bit).is_none());
}

#[kani::proof]
fn route_entry_initial_mask_is_exact_over_selected_participants() {
    let role_count: u8 = kani::any();
    let participant_mask: u16 = kani::any();
    let observed_mask: u16 = kani::any();
    kani::assume(role_count != 0 && role_count <= u16::BITS as u8);
    let complete = RouteTable::complete_observed_mask(role_count as usize);
    kani::assume(participant_mask != 0);
    kani::assume((participant_mask & !complete) == 0);
    kani::assume((observed_mask & !participant_mask) == 0);

    let initial =
        RouteTable::initial_observed_mask(role_count as usize, participant_mask, observed_mask);
    assert!((initial & !complete) == 0);
    assert!((participant_mask & !initial) == (participant_mask & !observed_mask));
    assert!((!participant_mask & complete & !initial) == 0);
}
