use std::fs;
use std::path::PathBuf;

fn read(path: &str) -> String {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let full = root.join(path);
    fs::read_to_string(&full)
        .unwrap_or_else(|err| panic!("read {} failed: {}", full.display(), err))
}

#[test]
fn readme_tracks_phase7_external_repo_boundaries() {
    let readme = read("README.md");

    for required in [
        "`hibana::substrate::wire::{Payload, WireEncode, WirePayload}`",
        "`hibana::substrate::policy::PolicySlot`",
        "`hibana::substrate::tap::TapEvent`",
        "`hibana_mgmt::request_reply::PREFIX`",
        "`hibana_mgmt::observe_stream::PREFIX`",
        "`hibana_mgmt::ROLE_CONTROLLER`",
        "`hibana_mgmt::ROLE_CLUSTER`",
        "`hibana_mgmt::Request::Load(LoadRequest)`",
        "`hibana_mgmt::Request::LoadAndActivate(LoadRequest)`",
        "`hibana_mgmt::Request::Activate(SlotRequest)`",
        "`hibana_mgmt::Request::Revert(SlotRequest)`",
        "`hibana_mgmt::Request::Stats(SlotRequest)`",
        "`hibana_epf::{Header, Slot}`",
        "`https://github.com/hibanaworks/hibana-epf`",
        "`https://github.com/hibanaworks/hibana-mgmt`",
        "`hibana-cross-repo`",
        "`https://github.com/hibanaworks/hibana-cross-repo`",
        "`run_workspace_smoke.sh`",
    ] {
        assert!(
            readme.contains(required),
            "README must spell the phase7 external repo boundary: {required}"
        );
    }

    for forbidden in [
        "`WireDecode`",
        "owned default path",
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
    ] {
        assert!(
            !readme.contains(forbidden),
            "README must not teach the deleted in-crate mgmt/epf paths: {forbidden}"
        );
    }
}

#[test]
fn phase7_spec_docs_exist() {
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
        assert!(
            full.exists(),
            "phase7 spec doc must exist: {}",
            full.display()
        );
    }
}

#[test]
fn core_repo_no_longer_keeps_in_tree_cross_repo_harness() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("integration/cross-repo");
    assert!(
        !path.exists(),
        "phase4 must move cross-repo smoke out of the hibana repo: {}",
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
