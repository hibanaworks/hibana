use super::actor_operation_mask;

#[kani::proof]
fn checked_actor_rows_require_exact_logical_directions() {
    let from: u8 = kani::any();
    let to: u8 = kani::any();
    let sender: bool = kani::any();
    let receiver: bool = kani::any();
    let mask = actor_operation_mask(from, to, sender, receiver);
    assert_eq!(mask & 1 != 0, from == to && sender);
    assert_eq!(mask & 2 != 0, from != to && sender);
    assert_eq!(mask & 4 != 0, from != to && receiver);
    assert!(matches!(mask, 0 | 1 | 2 | 4 | 6));
    if from != to && sender && receiver {
        assert_eq!(mask.trailing_zeros(), 1);
        let remaining = mask & (mask - 1);
        assert_eq!(remaining.trailing_zeros(), 2);
        assert_eq!(remaining & (remaining - 1), 0);
    }
}
