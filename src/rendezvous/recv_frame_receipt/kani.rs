use super::RecvFrameReceiptState;
use core::ptr::NonNull;

#[kani::proof]
fn receive_frame_receipt_resolution_is_affine() {
    let state = RecvFrameReceiptState::new();
    let key = NonNull::from(&state).cast();
    let mut receipt = state.issue(key);

    assert!(state.has_outstanding());
    assert!(receipt.is_current());
    receipt.assert_matches(key, NonNull::from(&state));

    if kani::any() {
        receipt.resolve();
        assert!(!state.has_outstanding());
        assert!(!receipt.is_current());
    } else {
        assert!(state.has_outstanding());
        assert!(receipt.is_current());
    }
}

#[kani::proof]
#[kani::should_panic]
fn receive_frame_receipt_rejects_duplicate_issue() {
    let state = RecvFrameReceiptState::new();
    let key = NonNull::from(&state).cast();
    let first = state.issue(key);
    let _ = first.is_current();
    state.issue(key);
}

#[kani::proof]
#[kani::should_panic]
fn receive_frame_receipt_rejects_duplicate_resolution() {
    let state = RecvFrameReceiptState::new();
    let key = NonNull::from(&state).cast();
    let mut receipt = state.issue(key);
    receipt.resolve();
    receipt.resolve();
}

#[kani::proof]
#[kani::should_panic]
fn receive_frame_receipt_rejects_foreign_port() {
    let state = RecvFrameReceiptState::new();
    let foreign = RecvFrameReceiptState::new();
    let mut receipt = state.issue(NonNull::from(&state).cast());
    receipt.assert_matches(NonNull::from(&foreign).cast(), NonNull::from(&state));
    receipt.resolve();
}

#[kani::proof]
#[kani::should_panic]
fn receive_frame_receipt_rejects_foreign_state() {
    let state = RecvFrameReceiptState::new();
    let foreign = RecvFrameReceiptState::new();
    let key = NonNull::from(&state).cast();
    let mut receipt = state.issue(key);
    receipt.assert_matches(key, NonNull::from(&foreign));
    receipt.resolve();
}
