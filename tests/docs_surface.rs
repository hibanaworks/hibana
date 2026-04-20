use std::fs;
use std::path::PathBuf;

fn read(path: &str) -> String {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let full = root.join(path);
    fs::read_to_string(&full)
        .unwrap_or_else(|err| panic!("read {} failed: {}", full.display(), err))
}

#[test]
fn readme_stays_self_contained_and_hibana_scoped() {
    let readme = read("README.md");

    for required in [
        "## Overview",
        "## Quick Start",
        "## App Surface",
        "## How It Works",
        "## Public Surfaces",
        "## Protocol Integration",
        "### Substrate Surface",
        "### Transport",
        "### SessionKit and Endpoint Attachment",
        "### BindingSlot",
        "### Policy",
        "### Management Boundary",
        "## Validation",
        "`hibana::substrate::wire::{Payload, WireEncode, WirePayload}`",
        "`hibana::substrate::policy::PolicySlot`",
        "`hibana::substrate::tap::TapEvent`",
        "App code builds a local choreography term with `hibana::g`",
        "the canonical path is local `let` inference rather than a named item",
        "let program = g::seq(mgmt_prefix, app);",
        "let client: RoleProgram<0> = project(&program);",
        "bash ./.github/scripts/check_stable_1_95.sh",
        "cargo check --all-targets -p hibana",
        "cargo test -p hibana --test ui --features std",
    ] {
        assert!(
            readme.contains(required),
            "README must stay self-contained and hibana-scoped: {required}"
        );
    }

    for forbidden in [
        "## Constitution",
        "Phase 7",
        "Phase 0a",
        "`WireDecode`",
        "owned default path",
        "hibana-quic",
        "hibana_mgmt",
        "hibana-mgmt",
        "hibana_epf",
        "hibana-epf",
        "hibana-cross-repo",
        "run_workspace_smoke.sh",
        "`hibana::substrate::mgmt`",
        "`hibana::substrate::policy::epf`",
        "`hibana::substrate::mgmt::request_reply::PREFIX`",
        "`hibana::substrate::mgmt::observe_stream::PREFIX`",
        "`hibana::substrate::mgmt::ROLE_CONTROLLER`",
        "`hibana::substrate::mgmt::ROLE_CLUSTER`",
        "`hibana::substrate::mgmt::Request::Load(LoadRequest)`",
        "`hibana::substrate::mgmt::Request::LoadAndActivate(LoadRequest)`",
        "`hibana::substrate::mgmt::Request::Activate(SlotRequest)`",
        "`hibana::substrate::mgmt::Request::Revert(SlotRequest)`",
        "`hibana::substrate::mgmt::Request::Stats(SlotRequest)`",
        "`integration/cross-repo/`",
        "staging location for cross-repo smoke",
        "App code writes `APP: g::Program<_>`",
        "project(&PROGRAM)",
        "project::<",
        "const APP: g::Program<_>",
        "static APP: g::Program<_>",
        "const PROGRAM: g::Program<_>",
        "static PROGRAM: g::Program<_>",
        "cargo +1.95.0 check --all-targets -p hibana",
        "cargo +1.95.0 test -p hibana --test ui --features std",
        "`hibana::g::advanced::steps`",
    ] {
        assert!(
            !readme.contains(forbidden),
            "README must not leak other-crate or internal-only wording: {forbidden}"
        );
    }
}

#[test]
fn spec_docs_exist() {
    for required in [
        "docs/spec/public_surface.md",
        "docs/spec/projection_witness.md",
        "docs/spec/descriptor_kernel.md",
        "docs/spec/payload_plane.md",
        "docs/spec/completion_policy.md",
        "docs/spec/completion_report.md",
        "docs/spec/compiled_image_layout.md",
        "docs/spec/policy_boundary.md",
        "docs/spec/downstream_readiness.md",
    ] {
        let full = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(required);
        assert!(full.exists(), "spec doc must exist: {}", full.display());
    }
}

#[test]
fn core_repo_keeps_cross_repo_harness_outside_tree() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("integration/cross-repo");
    assert!(
        !path.exists(),
        "cross-repo smoke must stay outside the hibana repo: {}",
        path.display()
    );
}

#[test]
fn completion_policy_spells_banned_regressions() {
    let policy = read("docs/spec/completion_policy.md");

    for required in [
        "no compatibility layer",
        "no dual public receive/decode trait story",
        "no raw-pointer frozen image owners",
        "no wrapper-future regressions in localside hot paths",
        "owned-by-value payloads stay on the same contract",
    ] {
        assert!(
            policy.contains(required),
            "completion policy must freeze the branch rules: {required}"
        );
    }
}

#[test]
fn crate_root_docs_do_not_regrow_internal_buckets() {
    let lib_rs = read("src/lib.rs");

    for forbidden in [
        "mod epf;",
        "pub mod runtime;",
        "pub mod transport;",
        "pub mod observe;",
    ] {
        assert!(
            !lib_rs.contains(forbidden),
            "crate root must stay on the minimal app/substrate surface: {forbidden}"
        );
    }
}
