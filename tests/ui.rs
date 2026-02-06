#[test]
fn g_compile_fails() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/g-*.rs");
    t.pass("tests/ui-pass/g-*.rs");

    // Control-plane mini kernel typestate tests
    t.compile_fail("tests/ui/cp-double-commit.rs");
    t.compile_fail("tests/ui/cp-wrong-shot.rs");
    t.compile_fail("tests/ui/cp-skip-ack.rs");
    t.compile_fail("tests/ui/cp-reopen-closed.rs");
    t.compile_fail("tests/ui/cp-use-after-consume.rs");
    t.compile_fail("tests/ui/control-crash-next-non-send.rs");
    t.compile_fail("tests/ui/control-cancel-payload.rs");
    t.compile_fail("tests/ui/control-checkpoint-payload.rs");

    t.compile_fail("tests/ui/lease-budget-missing-facet.rs");
    t.pass("tests/ui-pass/lease-budget-covered.rs");
    t.compile_fail("tests/ui/lease-program-missing-facet.rs");
    t.pass("tests/ui-pass/lease-program-covered.rs");
}
