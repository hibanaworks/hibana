use super::OfferLaneScan;
use crate::global::role_program::{LANE_SET_VIEW_WORDS, LaneSetView, LaneWord};

fn collect(words: &[LaneWord; LANE_SET_VIEW_WORDS], preferred: usize) -> [bool; 256] {
    let lanes = unsafe { LaneSetView::from_parts(words.as_ptr(), words.len()) };
    let mut scan = OfferLaneScan::new(preferred, lanes, 256);
    let mut seen = [false; 256];
    while let Some(lane) = scan.next() {
        assert!(!seen[lane], "lane {lane} was yielded twice");
        seen[lane] = true;
    }
    seen
}

#[test]
fn preferred_lane_is_first_and_every_lane_is_yielded_once() {
    let mut words = [0; LANE_SET_VIEW_WORDS];
    for lane in [0usize, 63, 64, 127, 128, 191, 192, 255] {
        let (word, bit) = crate::global::role_program::lane_word_index(lane);
        words[word] |= bit;
    }
    let seen = collect(&words, 128);
    let lanes = unsafe { LaneSetView::from_parts(words.as_ptr(), words.len()) };
    for (lane, present) in seen.into_iter().enumerate() {
        assert_eq!(present, lanes.contains(lane));
    }

    let mut scan = OfferLaneScan::new(128, lanes, 256);
    assert_eq!(scan.next(), Some(128));
}

#[test]
fn absent_or_out_of_range_preference_does_not_change_membership() {
    let mut words = [0; LANE_SET_VIEW_WORDS];
    let (word, bit) = crate::global::role_program::lane_word_index(255);
    words[word] = bit;
    assert!(collect(&words, 17)[255]);
    assert!(collect(&words, 256)[255]);

    let lanes = unsafe { LaneSetView::from_parts(words.as_ptr(), words.len()) };
    let mut bounded = OfferLaneScan::new(255, lanes, 255);
    assert_eq!(bounded.next(), None);
}
