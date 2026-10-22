#[test]
fn parallel_borrows_preserve_rust_ownership() {
    let tests = trybuild::TestCases::new();
    tests.compile_fail("tests/ui/parallel-parent-alias.rs");
    tests.compile_fail("tests/ui/parallel-parent-move.rs");
    tests.compile_fail("tests/ui/parallel-borrow-not-owned.rs");
    tests.compile_fail("tests/ui/parallel-empty.rs");
    tests.pass("tests/ui-pass/parallel-borrow.rs");
}
