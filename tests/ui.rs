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

fn rustc_minor_version() -> Option<u32> {
    let rustup_toolchain = std::env::var("RUSTUP_TOOLCHAIN").ok();
    if let Some(minor) = rustup_toolchain
        .as_deref()
        .and_then(parse_rustc_minor_version)
    {
        return Some(minor);
    }

    let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let output = std::process::Command::new(rustc)
        .arg("--version")
        .output()
        .ok()?;
    let stdout = String::from_utf8(output.stdout).ok()?;
    parse_rustc_minor_version(&stdout)
}

fn parse_rustc_minor_version(version: &str) -> Option<u32> {
    let version = version.trim_start_matches("rustc ").trim();
    let mut parts = version.split('.');
    match (parts.next(), parts.next()) {
        (Some("1"), Some(minor)) => minor.parse().ok(),
        _ => None,
    }
}

const RUSTC_1_95_STDERR_CASES: &[&str] = &[
    "tests/ui/g-project-role-out-of-range.rs",
    "tests/ui/g-role-out-of-range.rs",
    "tests/ui/g-roleprogram-witness-mismatch.rs",
    "tests/ui/g-par-ambiguous-endpoint-op.rs",
    "tests/ui/g-roll-ambiguous-reentry-exit-outbound.rs",
    "tests/ui/g-route-passive-first-visible-ambiguous.rs",
    "tests/ui/g-route-controller-mismatch.rs",
    "tests/ui/g-route-roll-before-resolve.rs",
    "tests/ui/g-route-unprojectable.rs",
    "tests/ui/g-typed-route-duplicate-label-project.rs",
    "tests/ui/resolver-decision-fn-removed.rs",
];

fn compile_fail(t: &trybuild::TestCases, path: &'static str) {
    t.compile_fail(path);
}

struct StderrSwap {
    originals: Vec<(std::path::PathBuf, String)>,
}

impl StderrSwap {
    fn for_rustc_1_95(rustc_minor: Option<u32>) -> Option<Self> {
        if rustc_minor != Some(95) {
            return None;
        }

        let mut originals = Vec::new();
        for source in RUSTC_1_95_STDERR_CASES {
            let stem = source
                .strip_prefix("tests/ui/")
                .and_then(|path| path.strip_suffix(".rs"))
                .expect("versioned trybuild fixture path must be tests/ui/*.rs");
            let path = std::path::PathBuf::from(format!("tests/ui/{stem}.stderr"));
            let original =
                std::fs::read_to_string(&path).expect("failed to read base trybuild stderr");
            let replacement = std::fs::read_to_string(format!("tests/ui-rustc-1-95/{stem}.stderr"))
                .expect("failed to read rustc 1.95 trybuild stderr");
            let replacement = replacement.replace("tests/ui-rustc-1-95/", "tests/ui/");
            std::fs::write(&path, replacement).expect("failed to install rustc 1.95 stderr");
            originals.push((path, original));
        }

        Some(Self { originals })
    }
}

impl Drop for StderrSwap {
    fn drop(&mut self) {
        for (path, original) in self.originals.drain(..).rev() {
            std::fs::write(path, original).expect("failed to restore base trybuild stderr");
        }
    }
}

#[test]
fn g_compile_fails() {
    normalise_trybuild_rustflags();
    let rustc_minor = rustc_minor_version();
    let stderr_swap = StderrSwap::for_rustc_1_95(rustc_minor);
    let t = trybuild::TestCases::new();
    compile_fail(&t, "tests/ui/g-efflist-deref.rs");
    compile_fail(&t, "tests/ui/g-project-role-out-of-range.rs");
    compile_fail(&t, "tests/ui/g-resolver-data-send.rs");
    compile_fail(&t, "tests/ui/g-role-out-of-range.rs");
    compile_fail(&t, "tests/ui/g-roleprogram-witness-mismatch.rs");
    compile_fail(&t, "tests/ui/g-par-ambiguous-endpoint-op.rs");
    compile_fail(&t, "tests/ui/g-roll-ambiguous-reentry-exit-outbound.rs");
    compile_fail(&t, "tests/ui/g-route-passive-first-visible-ambiguous.rs");
    compile_fail(&t, "tests/ui/g-route-controller-mismatch.rs");
    compile_fail(&t, "tests/ui/g-route-roll-before-resolve.rs");
    compile_fail(&t, "tests/ui/g-route-unprojectable.rs");
    compile_fail(&t, "tests/ui/g-typed-route-duplicate-label-project.rs");
    t.pass("tests/ui-pass/g-par-many.rs");
    t.pass("tests/ui-pass/g-par-same-role-auto-lanes.rs");
    t.pass("tests/ui-pass/g-par-same-label-distinct-local-endpoints.rs");
    t.pass("tests/ui-pass/g-par-same-label-distinct-inbound-evidence.rs");
    t.pass("tests/ui-pass/g-par-same-label-distinct-inbound-same-endpoint.rs");
    t.pass("tests/ui-pass/g-roll-same-label-distinct-inbound-evidence.rs");
    t.pass("tests/ui-pass/g-route-first-visible-passive-dispatch.rs");
    t.pass("tests/ui-pass/g-route-intrinsic-passive-same-label-frame-evidence.rs");
    t.pass("tests/ui-pass/g-route-merged.rs");
    t.pass("tests/ui-pass/g-route-resolver-scope-nested-par.rs");
    t.pass("tests/ui-pass/g-route-resolved-cross-arm-same-label.rs");
    t.pass("tests/ui-pass/g-route-resolved-cross-arm-overlap-after-left-par.rs");
    t.pass("tests/ui-pass/g-route-resolved-intra-arm-distinct-inbound-evidence.rs");
    t.pass("tests/ui-pass/g-route-static-basic.rs");
    t.pass("tests/ui-pass/g-route-static-prefix-local.rs");
    t.pass("tests/ui-pass/g-route-static-prefix-send.rs");
    t.pass("tests/ui-pass/local_let_program_inference.rs");
    t.pass("tests/ui-pass/local_let_prefix_facade_composition.rs");
    t.pass("tests/ui-pass/local_project_without_public_steps.rs");
    t.pass("tests/ui-pass/role_program_lifetime_free.rs");
    t.pass("tests/ui-pass/readme-route-example.rs");
    t.pass("tests/ui-pass/runtime-transport-recv-frame.rs");
    t.pass("tests/ui-pass/endpoint_transport_erased.rs");
    t.pass("tests/ui-pass/g-generic-role-ids.rs");
    t.pass("tests/ui-pass/g-codec-free-message-project.rs");
    t.pass("tests/ui-pass/endpoint-send-only-payload.rs");
    t.pass("tests/ui-pass/endpoint-recv-only-payload.rs");
    t.pass("tests/ui-pass/resolver-state-unit.rs");
    t.pass("tests/ui-pass/resolver-wrapper-decide.rs");

    compile_fail(&t, "tests/ui/runtime-storage-removed.rs");
    compile_fail(&t, "tests/ui/runtime-config-removed.rs");
    compile_fail(&t, "tests/ui/runtime-eff-index-removed.rs");
    compile_fail(&t, "tests/ui/runtime-session-id-field-private.rs");
    compile_fail(&t, "tests/ui/runtime-ingress-evidence-private.rs");
    compile_fail(&t, "tests/ui/runtime-received-frame-evidence-private.rs");
    compile_fail(&t, "tests/ui/runtime-decision-arm-index-private.rs");
    compile_fail(&t, "tests/ui/runtime-tap-event-fields-private.rs");
    compile_fail(&t, "tests/ui/runtime-tap-derived-helpers-private.rs");
    compile_fail(&t, "tests/ui/runtime-frame-header-peer-role-removed.rs");
    compile_fail(&t, "tests/ui/runtime-frame-label-new-private.rs");
    compile_fail(
        &t,
        "tests/ui/runtime-transport-error-associated-type-removed.rs",
    );
    compile_fail(
        &t,
        "tests/ui/runtime-transport-custom-error-return-removed.rs",
    );
    compile_fail(&t, "tests/ui/runtime-outgoing-peer-removed.rs");
    compile_fail(&t, "tests/ui/runtime-session-kit-storage-max-rv-removed.rs");
    compile_fail(&t, "tests/ui/runtime-fluent-session-removed.rs");
    compile_fail(&t, "tests/ui/runtime-fluent-role-removed.rs");
    compile_fail(&t, "tests/ui/runtime-fluent-witness-types-removed.rs");
    compile_fail(&t, "tests/ui/public_step_name_import.rs");
    compile_fail(&t, "tests/ui/public_compile_link_boundary.rs");
    compile_fail(&t, "tests/ui/public_fragment_boundary.rs");
    compile_fail(&t, "tests/ui/port-open-constructor.rs");
    compile_fail(&t, "tests/ui/route-branch-decode-removed.rs");
    compile_fail(&t, "tests/ui/route-recv-borrow-endpoint-alias.rs");
    compile_fail(&t, "tests/ui/recv-borrow-endpoint-alias.rs");
    compile_fail(&t, "tests/ui/route-branch-double-recv.rs");
    compile_fail(&t, "tests/ui/send-future-endpoint-alias.rs");
    compile_fail(&t, "tests/ui/endpoint-result-removed.rs");
    compile_fail(&t, "tests/ui/endpoint-send-only-payload-cannot-recv.rs");
    compile_fail(&t, "tests/ui/endpoint-recv-only-payload-cannot-send.rs");
    compile_fail(&t, "tests/ui/endpoint-error-operation-removed.rs");
    compile_fail(&t, "tests/ui/attach-error-operation-removed.rs");
    compile_fail(&t, "tests/ui/resolver-error-operation-removed.rs");
    compile_fail(&t, "tests/ui/resolver-decision-fn-removed.rs");
    compile_fail(&t, "tests/ui/resolver-decision-resolution-removed.rs");
    compile_fail(&t, "tests/ui/resolver-evaluate-removed.rs");
    compile_fail(&t, "tests/ui/resolver-resolve-decision-private.rs");
    drop(t);
    drop(stderr_swap);
}
