use super::{UniqueMatch, UniqueMatchFailure};

#[test]
fn repeated_identity_is_one_and_distinct_identity_is_ambiguous() {
    let one = UniqueMatch::NONE.add(7u8).add(7);
    assert_eq!(one.finish(), Ok(7));

    let ambiguous = one.add(8);
    assert_eq!(ambiguous.finish(), Err(UniqueMatchFailure::Ambiguous));
    assert_eq!(
        ambiguous.add(7).finish_optional(),
        Err(UniqueMatchFailure::Ambiguous)
    );
    assert_eq!(UniqueMatch::<u8>::NONE.finish_optional(), Ok(None));
    assert_eq!(one.finish_optional(), Ok(Some(7)));
}
