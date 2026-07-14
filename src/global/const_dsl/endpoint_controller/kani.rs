use super::FirstVisibleController;

#[kani::proof]
fn controller_merge_accepts_exact_single_role_domain() {
    let left = kani::any::<u8>();
    let right = kani::any::<u8>();
    let merged = FirstVisibleController::Unique(left).merge(FirstVisibleController::Unique(right));

    assert_eq!(merged.unique().is_some(), left == right);
    if left == right {
        assert_eq!(merged.unique(), Some(left));
    }
}
