use super::LocalLaneFacts;
use crate::g::{Msg, ProgramSourceData, Send};

type OneSend = Send<0, 1, Msg<1, ()>>;

fn one_send_source() -> ProgramSourceData<1> {
    ProgramSourceData::<1>::lower::<OneSend>()
}

#[test]
#[should_panic]
fn local_lane_facts_reject_out_of_range_effect_end() {
    let source = one_send_source();
    let _ = LocalLaneFacts::for_eff_range(source.eff_list(), 0, 0, 2);
}

#[test]
#[should_panic]
fn local_lane_facts_reject_reversed_effect_range() {
    let source = one_send_source();
    let _ = LocalLaneFacts::for_eff_range(source.eff_list(), 0, 1, 0);
}

#[test]
#[should_panic]
fn local_lane_facts_reject_out_of_range_bitmap_read() {
    let source = one_send_source();
    let facts = LocalLaneFacts::for_eff_range(source.eff_list(), 0, 0, 1);
    let _ = facts.lane_bit(facts.lane_bit_len());
}

#[test]
#[should_panic]
fn local_lane_facts_reject_absent_lane_last_step() {
    let source = one_send_source();
    let facts = LocalLaneFacts::for_eff_range(source.eff_list(), 0, 0, 1);
    let _ = facts.last_step(1);
}
