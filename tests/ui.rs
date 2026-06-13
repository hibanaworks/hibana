fn strip_repo_cfg<'a, I>(tokens: I) -> Vec<&'a str>
where
    I: Iterator<Item = &'a str>,
{
    let mut filtered = Vec::new();
    let mut parts = tokens.peekable();
    while let Some(part) = parts.next() {
        if part == "--cfg" && parts.peek() == Some(&"hibana_repo_tests") {
            let _ = parts.next();
            continue;
        }
        if part == "--cfg=hibana_repo_tests" {
            continue;
        }
        filtered.push(part);
    }
    filtered
}

fn normalise_trybuild_rustflags() {
    let rustflags = std::env::var("RUSTFLAGS").ok();
    let encoded = std::env::var("CARGO_ENCODED_RUSTFLAGS").ok();
    unsafe {
        // SAFETY: this test binary has a single test, and the environment is
        // normalised before trybuild spawns compiler subprocesses.
        if let Some(flags) = rustflags {
            let filtered = strip_repo_cfg(flags.split_whitespace()).join(" ");
            if filtered.is_empty() {
                std::env::remove_var("RUSTFLAGS");
            } else {
                std::env::set_var("RUSTFLAGS", filtered);
            }
        }
        if let Some(flags) = encoded {
            let filtered = strip_repo_cfg(flags.split('\x1f')).join("\x1f");
            if filtered.is_empty() {
                std::env::remove_var("CARGO_ENCODED_RUSTFLAGS");
            } else {
                std::env::set_var("CARGO_ENCODED_RUSTFLAGS", filtered);
            }
        }
    }
}

#[test]
fn g_compile_fails() {
    normalise_trybuild_rustflags();
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/g-*.rs");
    t.pass("tests/ui-pass/g-par-many.rs");
    t.pass("tests/ui-pass/g-par-same-role-auto-lanes.rs");
    t.pass("tests/ui-pass/g-route-first-visible-passive-dispatch.rs");
    t.pass("tests/ui-pass/g-route-merged.rs");
    t.pass("tests/ui-pass/g-route-static-basic.rs");
    t.pass("tests/ui-pass/g-route-static-prefix-local.rs");
    t.pass("tests/ui-pass/g-route-static-prefix-send.rs");
    t.pass("tests/ui-pass/local_let_program_inference.rs");
    t.pass("tests/ui-pass/local_let_prefix_facade_composition.rs");
    t.pass("tests/ui-pass/local_project_without_public_steps.rs");
    t.pass("tests/ui-pass/role_program_lifetime_free.rs");
    t.pass("tests/ui-pass/readme-route-example.rs");
    t.pass("tests/ui-pass/endpoint_transport_erased.rs");
    t.pass("tests/ui-pass/g-generic-role-ids.rs");

    t.compile_fail("tests/ui/public_step_name_import.rs");
    t.compile_fail("tests/ui/public_compile_link_boundary.rs");
    t.compile_fail("tests/ui/public_fragment_boundary.rs");
    t.compile_fail("tests/ui/port-open-constructor.rs");
    t.compile_fail("tests/ui/decode-borrow-endpoint-alias.rs");
    t.compile_fail("tests/ui/recv-borrow-endpoint-alias.rs");
    t.compile_fail("tests/ui/route-branch-double-decode.rs");
    t.compile_fail("tests/ui/send-future-endpoint-alias.rs");
}
