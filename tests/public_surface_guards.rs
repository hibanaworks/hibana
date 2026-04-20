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
            "core tree must remove the deleted owner path: {}",
            deleted.display()
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
fn core_resource_kind_catalogue_keeps_mgmt_and_policy_lifecycle_internal_only() {
    let resource_kinds_src = read("src/control/cap/resource_kinds.rs");

    for forbidden in [
        "pub struct PolicyLoadKind;",
        "pub struct PolicyActivateKind;",
        "pub struct PolicyRevertKind;",
        "pub struct PolicyAnnotateKind;",
        "pub struct LoadBeginKind;",
        "pub struct LoadCommitKind;",
    ] {
        assert!(
            !resource_kinds_src.contains(forbidden),
            "core must not remain the public owner of mgmt/policy lifecycle kinds: {forbidden}"
        );
    }
}

#[test]
fn substrate_tap_surface_stays_on_tapevent_only() {
    let substrate_src = read("src/substrate.rs");

    assert!(
        substrate_src.contains("pub mod tap {")
            && substrate_src.contains("pub use crate::observe::core::TapEvent;"),
        "substrate tap surface must expose TapEvent from core"
    );

    for forbidden in [
        "TapBatch",
        "RawEvent",
        "for_each_since",
        "install_ring",
        "push(",
    ] {
        assert!(
            !substrate_src.contains(forbidden),
            "substrate tap surface must stay minimal: {forbidden}"
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

#[test]
fn transport_snapshot_surface_stays_getter_only() {
    let transport_src = read("src/transport.rs");

    assert!(
        transport_src.contains("pub struct TransportSnapshotParts"),
        "transport snapshot packing must stay on the single-argument parts owner"
    );
    assert!(
        transport_src.contains("pub const fn from_parts(parts: TransportSnapshotParts) -> Self"),
        "transport snapshot must expose single-step packed construction"
    );
    for forbidden in [
        "pub const fn new(latency_us: Option<u64>, queue_depth: Option<u32>) -> Self",
        "pub const fn with_latency_us",
        "pub const fn with_queue_depth",
        "pub const fn with_congestion_marks",
        "pub const fn with_retransmissions",
        "pub const fn with_congestion_window",
        "pub const fn with_in_flight",
        "pub const fn with_algorithm",
    ] {
        assert!(
            !transport_src.contains(forbidden),
            "transport snapshot builder surface must stay removed: {forbidden}"
        );
    }
}

#[test]
fn core_repo_checks_do_not_assume_sibling_checkout_layout() {
    for path in [
        ".github/scripts/check_mgmt_boundary.sh",
        ".github/scripts/check_plane_boundaries.sh",
        ".github/scripts/check_surface_hygiene.sh",
        "tests/docs_surface.rs",
    ] {
        let src = read(path);
        for forbidden in [
            "../hibana-mgmt",
            "../hibana-epf",
            "hibana crate must live under the repository root",
        ] {
            assert!(
                !src.contains(forbidden),
                "core repo checks must not assume sibling checkout layout: {path}: {forbidden}"
            );
        }
    }
}
