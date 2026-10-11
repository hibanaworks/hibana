use super::{OfferLaneScan, OfferLaneScanCursor};
use crate::global::role_program::{LANE_SET_VIEW_WORDS, LaneSetView, LaneWord};

#[kani::proof]
#[kani::unwind(10)]
fn preferred_lane_scan_first_step_is_exact_over_the_full_lane_domain() {
    let words: [LaneWord; LANE_SET_VIEW_WORDS] = kani::any();
    let preferred = kani::any::<u8>() as usize;
    /* SAFETY: `words` remains live and immutable for the complete proof. */
    let lanes = unsafe { LaneSetView::from_parts(words.as_ptr(), words.len()) };
    let expected = if lanes.contains(preferred) {
        Some(preferred)
    } else {
        lanes.first_set(256)
    };
    let mut scan = OfferLaneScan::new(preferred, lanes, 256);
    assert_eq!(scan.next(), expected);
}

#[kani::proof]
#[kani::unwind(10)]
fn remaining_lane_scan_step_is_exact_over_the_full_lane_domain() {
    let words: [LaneWord; LANE_SET_VIEW_WORDS] = kani::any();
    let preferred = kani::any::<u8>() as usize;
    let candidate = kani::any::<u16>();
    let start = if candidate <= 256 {
        candidate as usize
    } else {
        256
    };
    /* SAFETY: `words` remains live and immutable for the complete proof. */
    let lanes = unsafe { LaneSetView::from_parts(words.as_ptr(), words.len()) };
    let first = lanes.next_set_from(start, 256);
    let expected = if first == Some(preferred) {
        lanes.next_set_from(preferred + 1, 256)
    } else {
        first
    };
    let mut scan = OfferLaneScan {
        offer_lanes: lanes,
        lane_limit: 256,
        preferred_lane: lanes.contains(preferred).then_some(preferred),
        cursor: OfferLaneScanCursor::Remaining(start),
    };
    let actual = scan.next();
    assert_eq!(actual, expected);
    match actual {
        Some(lane) => {
            assert!(lane < 256);
            assert!(lanes.contains(lane));
            assert_ne!(lane, preferred);
            assert!(matches!(
                scan.cursor,
                OfferLaneScanCursor::Remaining(next) if next == lane + 1
            ));
        }
        None => assert!(matches!(scan.cursor, OfferLaneScanCursor::Exhausted)),
    }
}
