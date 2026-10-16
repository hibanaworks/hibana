#[kani::proof]
fn view_generation_never_wraps_reuses_or_loses_its_retirement_successor() {
    let current: u32 = kani::any();
    let next = super::next_view_generation(current);
    let valid = current % 2 == 1 && current < u32::MAX - 2;
    assert_eq!(next.is_some(), valid);
    if let Some(next) = next {
        assert!(next > current);
        assert!(next % 2 == 1);
        assert!(next < u32::MAX);
        assert!(next + 1 > next);
        assert!(next + 1 != current);
    }
    kani::cover!(current == u32::MAX - 4 && next.is_some());
    kani::cover!(current == u32::MAX - 2 && next.is_none());
}
