use super::{UniqueMatch, UniqueMatchFailure};

#[kani::proof]
fn unique_match_zero_one_and_distinct_many_are_exact() {
    let first: u8 = kani::any();
    let distinct = first.wrapping_add(1);
    let empty = UniqueMatch::NONE;

    assert_eq!(empty.finish(), Err(UniqueMatchFailure::None));
    let one = empty.add(first);
    assert_eq!(one.finish(), Ok(first));
    assert_eq!(one.add(first).finish(), Ok(first));
    assert_eq!(
        one.add(distinct).finish(),
        Err(UniqueMatchFailure::Ambiguous)
    );
}

#[kani::proof]
fn unique_match_ambiguity_is_absorbing() {
    let first: u8 = kani::any();
    let later: u8 = kani::any();
    let ambiguous = UniqueMatch::NONE.add(first).add(first.wrapping_add(1));

    assert!(ambiguous.is_ambiguous());
    assert_eq!(ambiguous.add(later), UniqueMatch::Ambiguous);
    assert_eq!(ambiguous.into_option(), None);
}
