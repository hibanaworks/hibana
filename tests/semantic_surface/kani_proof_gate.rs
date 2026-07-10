use super::common::read;

#[test]
fn kani_gate_verifies_production_rust_without_entering_the_package_surface() {
    let root_manifest = read("Cargo.toml");
    let verification_manifest = read("proofs/kani/Cargo.toml");
    let script = read(".github/scripts/check_kani.sh");
    let workflow = read(".github/workflows/quality-gates.yml");
    let harnesses = read("src/rendezvous/core/storage_layout/capacity/kani.rs");
    let version = read(".github/kani-version");

    assert_eq!(version.trim(), "0.67.0");
    assert!(verification_manifest.contains("path = \"../../src/lib.rs\""));
    assert!(!verification_manifest.contains("[dependencies]"));
    assert!(!verification_manifest.contains("rust-version"));
    assert!(root_manifest.contains("'cfg(kani)'"));
    assert!(root_manifest.contains("\"!/src/**/kani.rs\""));
    assert!(script.contains("cargo kani --version"));
    assert!(script.contains("cargo kani \\"));
    assert!(script.contains("--run-sanity-checks"));
    assert!(script.contains("harnesses=9 backend=CBMC"));
    assert!(!script.contains("command -v cargo-kani"));
    assert!(!script.contains("exit 0"));
    assert!(workflow.contains("cargo install --locked kani-verifier"));
    assert!(workflow.contains("cargo kani setup"));
    assert!(workflow.contains("bash ./.github/scripts/check_kani.sh"));
    assert!(!script.contains("rg -q"));
    assert!(harnesses.contains("kani::assume(left_start < left_end)"));
    assert!(
        harnesses
            .contains("assert!(overlap == !(left_end <= right_start || right_end <= left_start));")
    );

    for harness in [
        "endpoint_generation_advances_or_exhausts",
        "endpoint_gap_placement_is_aligned_and_bounded",
        "endpoint_lease_storage_layout_is_bounded_and_exact",
        "association_storage_layout_is_bounded_and_exact",
        "route_storage_layout_is_bounded_and_exact",
        "packed_sidecar_range_is_aligned_and_monotonic",
        "packed_sidecar_pair_is_aligned_and_disjoint",
        "sidecar_overlap_is_symmetric_and_exact",
        "resolver_storage_layout_is_bounded_and_exact",
    ] {
        assert!(harnesses.contains(&format!("fn {harness}()")));
        assert!(script.contains(&format!("--harness {harness}")));
    }
}
