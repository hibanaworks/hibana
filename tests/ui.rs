#[test]
fn g_compile_fails() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/g-*.rs");
    t.pass("tests/ui-pass/g-*.rs");
    t.pass("tests/ui-pass/dynamic_route_defer_compiles.rs");

    t.compile_fail("tests/ui/control-cancel-payload.rs");
    t.compile_fail("tests/ui/control-checkpoint-payload.rs");
}
