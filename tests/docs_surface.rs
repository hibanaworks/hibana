use std::fs;
use std::path::{Path, PathBuf};

fn read(path: &str) -> String {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let full = root.join(path);
    fs::read_to_string(&full)
        .unwrap_or_else(|err| panic!("read {} failed: {}", full.display(), err))
}

fn assert_absent(readme: &str, forbidden: &str, why: &str) {
    assert!(!readme.contains(forbidden), "{why}: {forbidden}");
}

fn collect_source_files(root: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(root)
        .unwrap_or_else(|err| panic!("read_dir {} failed: {}", root.display(), err))
    {
        let entry =
            entry.unwrap_or_else(|err| panic!("read_dir entry {} failed: {}", root.display(), err));
        let path = entry.path();
        if path.is_dir() {
            collect_source_files(&path, out);
        } else if path.extension().and_then(|ext| ext.to_str()) != Some("stderr") {
            out.push(path);
        }
    }
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
        "`hibana::substrate::ids::{EffIndex, Lane, RendezvousId, SessionId}`",
        "`hibana::substrate::policy::PolicySlot`",
        "`hibana::substrate::tap::TapEvent`",
        "App code builds a local choreography term with `hibana::g`",
        "the canonical path is a local `let` choreography term rather than a named item",
        "let program = g::seq(mgmt_prefix, app);",
        "let client: RoleProgram<0> = project(&program);",
        "`AUTO_MINT_WIRE` only enables endpoint-side auto-mint",
        "send an explicit `GenericCapToken<K>`",
        "delegation stays on the lower-layer endpoint-token path; it is not a public",
        "bash ./.github/scripts/check_lowering_hygiene.sh",
        "bash ./.github/scripts/check_frozen_image_hygiene.sh",
        "bash ./.github/scripts/check_exact_layout_hygiene.sh",
        "bash ./.github/scripts/check_raw_future_hygiene.sh",
        "bash ./.github/scripts/check_pico_size_matrix.sh",
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
        "`hibana::substrate::mgmt`",
        "`hibana::substrate::policy::epf`",
        "`hibana::substrate::mgmt::request_reply::PREFIX`",
        "`hibana::substrate::mgmt::observe_stream::PREFIX`",
        "`hibana::substrate::mgmt::ROLE_CONTROLLER`",
        "`hibana::substrate::mgmt::ROLE_CLUSTER`",
        "`hibana::substrate::mgmt::Request::Load(LoadRequest)`",
        "`hibana::substrate::mgmt::Request::LoadAndActivate(LoadRequest)`",
        "`hibana::substrate::mgmt::Request::Activate(SlotRequest)`",
        "`hibana::substrate::mgmt::Request::Restore(SlotRequest)`",
        "`hibana::substrate::mgmt::Request::Stats(SlotRequest)`",
        "`integration/cross-repo/`",
        "staging location for cross-repo smoke",
        "App code writes `APP: g::Program<_>`",
        "project(&PROGRAM)",
        "const APP: g::Program<_>",
        "static APP: g::Program<_>",
        "const PROGRAM: g::Program<_>",
        "static PROGRAM: g::Program<_>",
        "`hibana::substrate::program::steps`",
        "public wire control kinds must set `AUTO_MINT_WIRE = true`",
        "`CapDelegate`: `input[0] = (dst_rv << 16) | dst_lane`",
    ] {
        assert_absent(
            &readme,
            forbidden,
            "README must not leak other-crate or internal-only wording",
        );
    }

    assert_absent(
        &readme,
        &["project::", "<"].concat(),
        "README must not leak other-crate or internal-only wording",
    );

    for forbidden in ["cargo +", "workspace_smoke"] {
        assert_absent(
            &readme,
            forbidden,
            "README must not pin removed toolchain or smoke-helper lanes",
        );
    }
}

#[test]
fn projection_constructor_stays_on_canonical_call_shape() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let forbidden = ["project::", "<"].concat();
    let mut files = vec![root.join("README.md")];

    for dir in ["src", "tests"] {
        collect_source_files(&root.join(dir), &mut files);
    }

    let mut offenders = Vec::new();
    for file in files {
        let src = fs::read_to_string(&file)
            .unwrap_or_else(|err| panic!("read {} failed: {}", file.display(), err));
        for (line_idx, line) in src.lines().enumerate() {
            if line.contains(&forbidden) {
                let rel = file.strip_prefix(&root).unwrap_or(file.as_path()).display();
                offenders.push(format!("{}:{}:{}", rel, line_idx + 1, line.trim()));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "projection must use the canonical `project(&program)` call shape:\n{}",
        offenders.join("\n")
    );
}

#[test]
fn quality_gates_do_not_directly_execute_non_executable_scripts() {
    let workflow = read(".github/workflows/quality-gates.yml");

    for required in [
        "bash ./.github/scripts/check_plane_boundaries.sh",
        "bash ./.github/scripts/check_pico_smoke.sh",
    ] {
        assert!(
            workflow.contains(required),
            "quality gates must invoke non-executable scripts through bash: {required}"
        );
    }

    for forbidden in [
        "run: ./.github/scripts/check_plane_boundaries.sh",
        "run: ./.github/scripts/check_pico_smoke.sh",
    ] {
        assert!(
            !workflow.contains(forbidden),
            "quality gates must not rely on executable bits for 100644 scripts: {forbidden}"
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
        "docs/spec/policy-semantics.md",
        "docs/spec/downstream_readiness.md",
    ] {
        let full = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(required);
        assert!(full.exists(), "spec doc must exist: {}", full.display());
    }
}

#[test]
fn readme_wire_control_example_uses_reserved_control_label_band() {
    let readme = read("README.md");

    for required in [
        "const LABEL: u8 = 124;",
        "const TAP_ID: u16 = 0x0300 + 124;",
        "CapShot::Many",
    ] {
        assert!(
            readme.contains(required),
            "README explicit wire-control example must stay in the descriptor label contract: {required}"
        );
    }

    for forbidden in ["const LABEL: u8 = 90;", "0x0300 + 90"] {
        assert!(
            !readme.contains(forbidden),
            "README explicit wire-control example must not use rejected labels: {forbidden}"
        );
    }
}

#[test]
fn policy_semantics_doc_stays_on_current_core_boundary() {
    let policy = read("docs/spec/policy-semantics.md");
    let boundary = read("docs/spec/policy_boundary.md");

    for required in [
        "`hibana` core owns the policy input boundary and fail-closed reduction only",
        "Bytecode verification, VM execution, and management load semantics are outside",
        "`src/transport/context.rs`",
        "`src/policy_runtime.rs`",
        "`src/endpoint/kernel/core.rs`",
    ] {
        assert!(
            policy.contains(required),
            "policy semantics doc must describe the current core boundary: {required}"
        );
    }

    for forbidden in [
        "src/epf.rs",
        "src/epf/",
        "src/runtime/mgmt.rs",
        "load_commit",
        "transport/forward.rs",
    ] {
        assert!(
            !policy.contains(forbidden),
            "policy semantics doc must not point at removed core owners: {forbidden}"
        );
    }

    for required in [
        "daily policy boundary to resolver inputs only",
        "`hibana::substrate::policy::PolicySlot`",
        "`hibana::substrate::policy::PolicySlot::Route`",
    ] {
        assert!(
            boundary.contains(required),
            "policy boundary doc must keep slot identity in the single policy bucket: {required}"
        );
    }
}

#[test]
fn downstream_readiness_tracks_rev_lane_contract() {
    let readiness = read("docs/spec/downstream_readiness.md");

    for required in [
        "immutable `git` + `rev` dependencies",
        "local worktree overlays stay smoke-only",
    ] {
        assert!(
            readiness.contains(required),
            "downstream readiness must freeze the immutable revision lane: {required}"
        );
    }

    for forbidden in [
        "explicit local path dependencies",
        "floating branches, git overlays",
    ] {
        assert!(
            !readiness.contains(forbidden),
            "downstream readiness must not revive the old local-path contract: {forbidden}"
        );
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
        "no second public surface",
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
fn readme_keeps_advanced_buckets_out_of_everyday_substrate_list() {
    let readme = read("README.md");
    let everyday = readme
        .split("The everyday protocol-side owners are:")
        .nth(1)
        .and_then(|tail| tail.split("Lower-level substrate buckets:").next())
        .expect("README must keep everyday substrate owners and lower-level buckets separated");

    assert!(
        !everyday.contains("::advanced"),
        "README everyday substrate list must not include advanced buckets"
    );
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

#[test]
fn crate_root_docs_keep_descriptor_first_control_story() {
    let lib_rs = read("src/lib.rs");

    for required in [
        "descriptor-first control facts",
        "shot, path, and atomic op are baked into descriptor metadata",
        "descriptor-baked `ControlOp` values",
    ] {
        assert!(
            lib_rs.contains(required),
            "crate root docs must describe the descriptor-first control model: {required}"
        );
    }

    for forbidden in [
        "cancel pair, checkpoint/rollback, splice",
        "shot and permissions are embedded in the const metadata",
        "manages local state (lane/gen/cap/splice)",
    ] {
        assert!(
            !lib_rs.contains(forbidden),
            "crate root docs must not describe the removed control execution model: {forbidden}"
        );
    }
}
