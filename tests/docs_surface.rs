use std::fs;
use std::path::PathBuf;

fn read(path: &str) -> String {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let full = root.join(path);
    fs::read_to_string(&full)
        .unwrap_or_else(|err| panic!("read {} failed: {}", full.display(), err))
}

#[test]
fn readme_tracks_phase6_core_and_sibling_boundaries() {
    let readme = read("README.md");

    for required in [
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
    ] {
        assert!(
            readme.contains(required),
            "README must spell the phase6 sibling boundary: {required}"
        );
    }

    for forbidden in [
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
    ] {
        assert!(
            !readme.contains(forbidden),
            "README must not teach the deleted in-crate mgmt/epf paths: {forbidden}"
        );
    }
}

#[test]
fn crate_root_docs_do_not_regrow_internal_buckets() {
    let lib_rs = read("src/lib.rs");

    for forbidden in ["mod epf;", "pub mod runtime;", "pub mod transport;", "pub mod observe;"] {
        assert!(
            !lib_rs.contains(forbidden),
            "crate root must stay on the minimal app/substrate surface: {forbidden}"
        );
    }
}
