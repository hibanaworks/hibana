use std::fs;
use std::path::PathBuf;

fn read(path: &str) -> String {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let full = root.join(path);
    fs::read_to_string(&full)
        .unwrap_or_else(|err| panic!("read {} failed: {}", full.display(), err))
}

fn repo_path(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path)
}

#[test]
fn core_source_tree_no_longer_keeps_mgmt_or_epf_owners() {
    for deleted in [
        repo_path("src/runtime/mgmt.rs"),
        repo_path("src/runtime/mgmt"),
        repo_path("src/epf.rs"),
        repo_path("src/epf"),
    ] {
        assert!(
            !deleted.exists(),
            "phase6 must remove the deleted core owner path: {}",
            deleted.display()
        );
    }
}

#[test]
fn sibling_crates_exist_for_optional_mgmt_and_epf_layers() {
    let hibana_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = hibana_root
        .parent()
        .expect("hibana crate must live under the repository root");

    for sibling in [
        repo_root.join("hibana-mgmt/src/lib.rs"),
        repo_root.join("hibana-epf/src/lib.rs"),
    ] {
        assert!(
            sibling.exists(),
            "phase6 must provide the sibling crate entrypoint: {}",
            sibling.display()
        );
    }
}

#[test]
fn transport_context_uses_generic_policy_slot_owner() {
    let context_src = read("src/transport/context.rs");

    assert!(
        context_src.contains("use crate::substrate::policy::PolicySlot;"),
        "transport context must use the surviving generic PolicySlot owner"
    );
    assert!(
        !context_src.contains("policy::epf::Slot"),
        "transport context must not mention the deleted core EPF slot path"
    );
}

#[test]
fn substrate_tap_surface_stays_on_tapevent_only() {
    let substrate_src = read("src/substrate.rs");

    assert!(
        substrate_src.contains("pub mod tap {")
            && substrate_src.contains("pub use crate::observe::core::TapEvent;"),
        "substrate tap surface must expose TapEvent from core"
    );

    for forbidden in ["TapBatch", "RawEvent", "for_each_since", "install_ring", "push("] {
        assert!(
            !substrate_src.contains(forbidden),
            "substrate tap surface must stay minimal after phase6: {forbidden}"
        );
    }
}

#[test]
fn substrate_policy_surface_exports_policyslot_only() {
    let substrate_src = read("src/substrate.rs");

    assert!(
        substrate_src.contains("pub use crate::policy_runtime::PolicySlot;"),
        "substrate::policy must re-export PolicySlot"
    );
    for forbidden in ["pub mod epf {", "crate::epf::", "policy::epf"] {
        assert!(
            !substrate_src.contains(forbidden),
            "substrate::policy must not regrow the deleted epf bucket: {forbidden}"
        );
    }
}
