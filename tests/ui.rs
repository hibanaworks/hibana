#[test]
fn g_compile_fails() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/g-*.rs");
    t.pass("tests/ui-pass/dynamic_route_defer_compiles.rs");
    t.pass("tests/ui-pass/g-par-many.rs");
    t.pass("tests/ui-pass/g-route-merged.rs");
    t.pass("tests/ui-pass/g-route-static-control-basic.rs");
    t.pass("tests/ui-pass/g-route-static-control-prefix-local.rs");
    t.pass("tests/ui-pass/g-route-static-control-prefix-send.rs");

    t.compile_fail("tests/ui/control-cancel-payload.rs");
    t.compile_fail("tests/ui/control-checkpoint-payload.rs");
    t.compile_fail("tests/ui/decode-borrow-endpoint-alias.rs");
    t.compile_fail("tests/ui/recv-borrow-endpoint-alias.rs");
    t.compile_fail("tests/ui/route-branch-double-decode.rs");
    t.compile_fail("tests/ui/send-future-endpoint-alias.rs");
}
