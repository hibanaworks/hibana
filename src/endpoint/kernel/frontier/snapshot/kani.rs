use super::{FrontierVisitSet, StateIndex, checked_state_index};
use crate::global::typestate::MAX_STATES;

#[kani::proof]
fn visited_entry_identity_is_exact_and_never_silent() {
    let first = kani::any::<u8>() as usize;
    let second = kani::any::<u8>() as usize;
    kani::assume(first != second);
    let mut storage = [StateIndex::ABSENT; 2];
    /* SAFETY: `storage` is initialized, live, and exclusively borrowed for the
    complete symbolic visit-set execution. */
    let mut visited = unsafe { FrontierVisitSet::from_parts(storage.as_mut_ptr(), storage.len()) };

    visited.record(first);
    visited.record(second);

    assert_eq!(visited.len(), 2);
    assert!(visited.contains(first));
    assert!(visited.contains(second));
}

#[kani::proof]
fn absent_state_identity_is_rejected() {
    assert!(checked_state_index(MAX_STATES - 1).is_some());
    assert!(checked_state_index(MAX_STATES).is_none());
}
