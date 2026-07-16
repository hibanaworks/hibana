use super::LocalLaneAccumulator;

#[kani::proof]
fn local_lane_accumulator_preserves_exact_lane_relations_and_last_steps() {
    let lane: u8 = kani::any();
    let other: u8 = kani::any();
    let first_step: u16 = kani::any();
    let last_step: u16 = kani::any();
    kani::assume(first_step <= last_step);

    let mut facts = LocalLaneAccumulator::new();
    facts.record(lane, first_step as usize);
    assert_eq!(facts.relation_count, 1);
    assert_eq!(facts.last_steps[lane as usize], first_step);
    assert_eq!(facts.lane_bit_len, lane as usize / 8 + 1);

    facts.record(lane, last_step as usize);
    assert_eq!(facts.relation_count, 1);
    assert_eq!(facts.last_steps[lane as usize], last_step);

    facts.record(other, last_step as usize);
    assert_eq!(facts.relation_count, if other == lane { 1 } else { 2 });
    assert_eq!(facts.lane_bit_len, lane.max(other) as usize / 8 + 1);
    assert_ne!(
        facts.lane_bits[other as usize / 8] & (1u8 << (other % 8)),
        0
    );
}
