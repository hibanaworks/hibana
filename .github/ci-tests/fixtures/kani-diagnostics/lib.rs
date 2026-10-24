//! Intentional failures: the diagnostic stream must never turn these green.
#[kani::proof]
fn real_assertion_failure() {
    assert!(false, "HIBANA_REAL_ASSERTION_CONTROL");
}

#[kani::proof]
fn real_memory_failure() {
    let bytes = [0u8];
    let offset = usize::from(kani::any::<bool>());
    // This negative control intentionally violates read's bounds precondition.
    let byte = unsafe { bytes.as_ptr().add(offset).read() };
    assert_eq!(byte, 0);
}

#[kani::proof]
#[kani::unwind(2)]
fn real_unwind_failure() {
    let limit = kani::any::<u8>() % 4;
    let mut value = 0;
    while value < limit {
        value += 1;
    }
    assert_eq!(value, limit);
}
