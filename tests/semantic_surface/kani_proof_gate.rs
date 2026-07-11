use super::common::read;

#[test]
fn kani_gate_verifies_production_rust_without_entering_the_package_surface() {
    let root_manifest = read("Cargo.toml");
    let verification_manifest = read("proofs/kani/Cargo.toml");
    let script = read(".github/scripts/check_kani.sh");
    let workflow = read(".github/workflows/quality-gates.yml");
    let harnesses = read("src/rendezvous/core/storage_layout/capacity/kani.rs");
    let authority_harnesses = read("src/endpoint/kernel/authority/kani.rs");
    let descriptor_harnesses = read("src/global/typestate/facts/kani.rs");
    let image_harnesses = read("src/global/role_program/image_impl/kani.rs");
    let scope_harnesses = read("src/global/const_dsl/scope/kani.rs");
    let resolver_row_harnesses = read("src/global/compiled/images/image/route_resolvers/kani.rs");
    let atom_row_harnesses = read("src/global/compiled/images/image/program_ref/kani.rs");
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
    assert!(script.contains("harnesses=22 backend=CBMC"));
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
    let compaction_harness = harnesses
        .split("fn packed_sidecar_pair_compacts_before_source_ranges()")
        .nth(1)
        .expect("sidecar compaction harness")
        .split("#[kani::proof]")
        .next()
        .expect("sidecar compaction harness body");
    assert!(!compaction_harness.contains("kani::assume"));
    assert!(compaction_harness.contains("first_destination_end <= second_source_start"));
    assert!(compaction_harness.contains("second_destination_end <= second_source_end"));

    for harness in [
        "endpoint_generation_advances_or_exhausts",
        "endpoint_gap_placement_is_aligned_and_bounded",
        "endpoint_lease_storage_layout_is_bounded_and_exact",
        "association_storage_layout_is_bounded_and_exact",
        "route_storage_layout_is_bounded_and_exact",
        "packed_sidecar_range_is_aligned_and_monotonic",
        "packed_sidecar_pair_is_aligned_and_disjoint",
        "packed_sidecar_pair_compacts_before_source_ranges",
        "sidecar_overlap_is_symmetric_and_exact",
        "resolver_storage_layout_is_bounded_and_exact",
    ] {
        assert!(harnesses.contains(&format!("fn {harness}()")));
        assert!(script.contains(&format!("--harness {harness}")));
    }
    for harness in [
        "route_arm_decoding_accepts_exact_binary_domain",
        "single_ready_mask_decoding_is_exact",
    ] {
        assert!(authority_harnesses.contains(&format!("fn {harness}()")));
        assert!(script.contains(&format!("--harness {harness}")));
    }
    assert!(authority_harnesses.contains("Arm::decode_single_ready_mask(mask) == expected"));
    assert!(!authority_harnesses.contains("kani::assume"));
    for harness in [
        "packed_event_conflict_decoding_accepts_exact_domain",
        "optional_route_arm_decoding_accepts_exact_domain",
        "packed_local_dependency_decoding_accepts_exact_domain",
    ] {
        assert!(descriptor_harnesses.contains(&format!("fn {harness}()")));
        assert!(script.contains(&format!("--harness {harness}")));
    }
    assert!(
        descriptor_harnesses.contains("(route_ordinal as usize) < crate::eff::meta::MAX_EFF_NODES")
    );
    assert!(
        descriptor_harnesses.contains("(dep_ordinal as usize) < crate::eff::meta::MAX_EFF_NODES")
    );
    assert!(descriptor_harnesses.contains("matches!(decoded, Some(None)) == absent"));
    for harness in [
        "resident_route_arm_index_decoding_accepts_exact_binary_domain",
        "resident_route_scope_decoding_accepts_exact_domain",
        "resident_roll_scope_decoding_accepts_exact_domain",
        "resident_event_header_decoding_accepts_exact_domain",
    ] {
        assert!(image_harnesses.contains(&format!("fn {harness}()")));
        assert!(script.contains(&format!("--harness {harness}")));
    }
    assert!(!image_harnesses.contains("kani::assume"));

    let harness = "scope_id_decoding_accepts_exact_compact_domain";
    assert!(scope_harnesses.contains(&format!("fn {harness}()")));
    assert!(scope_harnesses.contains("scope.is_none() == (raw == u16::MAX)"));
    assert!(script.contains(&format!("--harness {harness}")));

    let harness = "route_resolver_row_decoding_accepts_exact_domain";
    assert!(resolver_row_harnesses.contains(&format!("fn {harness}()")));
    assert!(resolver_row_harnesses.contains("assert!(decoded.is_some() == expected)"));
    assert!(!resolver_row_harnesses.contains("kani::assume"));
    assert!(script.contains(&format!("--harness {harness}")));

    let harness = "program_atom_row_decoding_accepts_exact_domain";
    assert!(atom_row_harnesses.contains(&format!("fn {harness}()")));
    assert!(atom_row_harnesses.contains("assert!(decoded.is_some() == expected)"));
    assert!(!atom_row_harnesses.contains("kani::assume"));
    assert!(script.contains(&format!("--harness {harness}")));
}
