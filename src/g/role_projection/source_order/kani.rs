use super::{RowBits, pair_candidates};

#[kani::proof]
fn omitted_source_order_pairs_have_equal_unconditional_answers() {
    let expected: u64 = kani::any();
    let actual: u64 = kani::any();
    // These generators cover every lower/upper bound of every actual word.
    let possible = actual | kani::any::<u64>();
    let certain = actual & kani::any::<u64>();
    let excluded: u64 = kani::any();
    let pending = pair_candidates(expected, possible, certain, excluded);
    assert_eq!((expected ^ actual) & !pending & !excluded, 0);
    kani::cover!(pending != 0);
    kani::cover!(pending == 0 && expected != 0);
}

#[kani::proof]
#[kani::unwind(4)]
fn source_order_bit_ranges_preserve_every_boundary_and_membership() {
    let first = usize::from(kani::any::<u8>()) % 129;
    let second = usize::from(kani::any::<u8>()) % 129;
    let (start, end) = if first <= second {
        (first, second)
    } else {
        (second, first)
    };
    let row = usize::from(kani::any::<u8>()) % 128;
    let mut bits = RowBits::<2>::empty();
    bits.insert_range(start, end);
    assert_eq!(bits.get(row), start <= row && row < end);
    kani::cover!(start < 64 && end > 64);
    kani::cover!(start == end);
    bits.clear(2);
    assert!(!bits.get(row));
}
