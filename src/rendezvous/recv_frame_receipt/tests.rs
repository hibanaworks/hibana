use super::*;

#[test]
fn receive_frame_receipt_issue_and_resolution_are_exact() {
    let state = RecvFrameReceiptState::new();
    let key = NonNull::from(&state).cast();
    let mut receipt = state.issue(key);

    assert!(state.has_outstanding());
    assert!(receipt.is_current());
    receipt.assert_matches(key, NonNull::from(&state));

    receipt.resolve();

    assert!(!state.has_outstanding());
    assert!(!receipt.is_current());
}

#[test]
fn typed_receive_frame_receipt_preserves_compact_layout() {
    assert_eq!(
        core::mem::size_of::<RecvFrameReceiptState>(),
        core::mem::size_of::<u8>()
    );
    assert_eq!(
        core::mem::size_of::<PortRecvFrameReceipt>(),
        core::mem::size_of::<usize>() * 2
    );
}

#[test]
#[should_panic]
fn receive_frame_receipt_rejects_duplicate_issue() {
    let state = RecvFrameReceiptState::new();
    let key = NonNull::from(&state).cast();
    let first = state.issue(key);
    assert!(first.is_current());
    state.issue(key);
}

#[test]
#[should_panic]
fn receive_frame_receipt_rejects_duplicate_resolution() {
    let state = RecvFrameReceiptState::new();
    let key = NonNull::from(&state).cast();
    let mut receipt = state.issue(key);
    receipt.resolve();
    receipt.resolve();
}

#[test]
#[should_panic]
fn receive_frame_receipt_rejects_foreign_port() {
    let state = RecvFrameReceiptState::new();
    let foreign = RecvFrameReceiptState::new();
    let mut receipt = state.issue(NonNull::from(&state).cast());
    receipt.assert_matches(NonNull::from(&foreign).cast(), NonNull::from(&state));
    receipt.resolve();
}

#[test]
#[should_panic]
fn receive_frame_receipt_rejects_foreign_state() {
    let state = RecvFrameReceiptState::new();
    let foreign = RecvFrameReceiptState::new();
    let key = NonNull::from(&state).cast();
    let mut receipt = state.issue(key);
    receipt.assert_matches(key, NonNull::from(&foreign));
    receipt.resolve();
}
