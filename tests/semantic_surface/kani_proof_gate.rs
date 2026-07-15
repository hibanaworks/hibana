use super::common::read;

#[test]
fn kani_gate_verifies_production_rust_without_entering_the_package_surface() {
    let root_manifest = read("Cargo.toml");
    let verification_manifest = read("proofs/kani/Cargo.toml");
    let script = read(".github/scripts/check_kani.sh");
    let workflow = read(".github/workflows/quality-gates.yml");
    let harnesses = read("src/rendezvous/core/storage_layout/capacity/kani.rs");
    let authority_harnesses = read("src/endpoint/kernel/authority/kani.rs");
    let public_operation_harnesses = read("src/endpoint/kernel/core/public_types/kani.rs");
    let descriptor_harnesses = read("src/global/typestate/facts/kani.rs");
    let image_harnesses = read("src/global/role_program/image_impl/kani.rs");
    let scope_harnesses = read("src/global/const_dsl/scope/kani.rs");
    let allocation_harnesses = read("src/global/const_dsl/allocation/kani.rs");
    let maximum_matching_harness =
        read("src/global/const_dsl/allocation/kani/maximum_certificate.rs");
    let production_coloring_harnesses =
        read("src/global/const_dsl/allocation/kani/production_coloring.rs");
    let roll_coloring_harnesses = read("src/global/const_dsl/allocation/kani/roll_coloring.rs");
    let endpoint_selector_harnesses = read("src/global/const_dsl/endpoint_selectors/kani.rs");
    let route_knowledge_harnesses = read("src/global/compiled/lowering/seal/kani.rs");
    let receive_lane_harnesses = read("src/global/const_dsl/receive_lane_causality/kani.rs");
    let resolver_row_harnesses = read("src/global/compiled/images/image/route_resolvers/kani.rs");
    let program_ref_harnesses = read("src/global/compiled/images/image/program_ref/kani.rs");
    let transport_harnesses = read("src/transport/kani.rs");
    let receive_receipt_harnesses = read("src/rendezvous/recv_frame_receipt/kani.rs");
    let fault_harnesses = read("src/rendezvous/association/fault/kani.rs");
    let association_harnesses = read("src/rendezvous/association/kani.rs");
    let program_blob_storage = read("src/global/compiled/images/image/blob_storage.rs");
    let program_columns = read("src/global/compiled/images/image/columns.rs");
    let role_image_types = read("src/global/role_program/image_types.rs");
    let resolver_registration_harnesses =
        read("src/session/cluster/core/dynamic_resolvers/kani.rs");
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
    assert!(script.contains("kani_harness_total=\"$(wc -l < \"${gate_inventory}\" | tr -d ' ')\""));
    assert!(script.contains(
        "Kani gate passed version=${EXPECTED_VERSION} harnesses=${kani_harness_total} backend=CBMC"
    ));
    assert!(!script.contains("command -v cargo-kani"));
    assert!(!script.contains("exit 0"));
    assert!(workflow.contains("cargo install --locked kani-verifier"));
    assert!(workflow.contains("cargo kani setup"));
    assert!(workflow.contains("bash ./.github/scripts/check_kani.sh"));
    for harness in [
        "receive_frame_receipt_resolution_is_affine",
        "receive_frame_receipt_rejects_duplicate_issue",
        "receive_frame_receipt_rejects_duplicate_resolution",
        "receive_frame_receipt_rejects_foreign_port",
        "receive_frame_receipt_rejects_foreign_state",
    ] {
        assert!(receive_receipt_harnesses.contains(&format!("fn {harness}()")));
        assert!(script.contains(&format!("--harness {harness}")));
    }
    assert!(!script.contains("rg -q"));
    assert!(script.contains("hibana-kani-proofs.XXXXXX"));
    assert!(script.contains("hibana-kani-gate.XXXXXX"));
    assert!(script.contains("Kani proof and gate harness names must each be unique"));
    assert!(script.contains("Kani should-panic harness may panic before its production call"));
    assert!(
        script.contains("Kani gate harness inventory does not match production proof inventory")
    );
    assert!(harnesses.contains("kani::assume(left_start < left_end)"));
    assert!(
        harnesses
            .contains("assert!(overlap == !(left_end <= right_start || right_end <= left_start));")
    );
    let compaction_harness = harnesses
        .split("fn three_resident_sidecars_compact_before_all_source_ranges()")
        .nth(1)
        .expect("sidecar compaction harness")
        .split("#[kani::proof]")
        .next()
        .expect("sidecar compaction harness body");
    assert!(!compaction_harness.contains("kani::assume"));
    assert!(harnesses.contains("bytes: [usize; 3]"));
    assert!(harnesses.contains("gaps: [usize; 2]"));
    assert!(compaction_harness.contains("destinations[index].1 <= sources[index].1"));
    assert!(compaction_harness.contains("destinations[index].1 <= sources[later].0"));

    for harness in [
        "endpoint_generation_advances_or_exhausts",
        "endpoint_membership_seal_is_published_and_idempotent",
        "endpoint_operation_and_nested_scratch_transitions_are_exact",
        "endpoint_gap_placement_is_aligned_and_bounded",
        "endpoint_lease_storage_layout_is_bounded_and_exact",
        "association_storage_layout_is_bounded_and_exact",
        "packed_sidecar_range_is_aligned_and_monotonic",
        "packed_sidecar_pair_is_aligned_and_disjoint",
        "three_resident_sidecars_compact_before_all_source_ranges",
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
    let harness = "public_operation_transition_classifier_is_exact";
    assert!(public_operation_harnesses.contains(&format!("fn {harness}()")));
    assert!(public_operation_harnesses.contains("current == PublicActiveOp::Poisoned"));
    assert!(public_operation_harnesses.contains("current == expected"));
    assert!(!public_operation_harnesses.contains("kani::assume"));
    assert!(script.contains(&format!("--harness {harness}")));
    for harness in [
        "frame_header_roundtrip_preserves_every_field",
        "frame_header_identity_is_exact_and_injective",
    ] {
        assert!(transport_harnesses.contains(&format!("fn {harness}()")));
        assert!(script.contains(&format!("--harness {harness}")));
    }
    assert!(transport_harnesses.contains("left_header == right_header"));
    assert!(!transport_harnesses.contains("kani::assume"));
    assert!(fault_harnesses.contains("fn session_fault_encoding_roundtrip_is_exact()"));
    assert!(fault_harnesses.contains("fn symbolic_fault(raw: u8) -> SessionFaultKind"));
    assert!(fault_harnesses.contains("SessionFaultKind::decode(encoded) == Some(fault)"));
    assert!(fault_harnesses.contains("fn session_fault_encoding_is_injective()"));
    assert!(fault_harnesses.contains("fn invalid_session_fault_encoding_is_fail_fast()"));
    assert!(fault_harnesses.contains("#[kani::should_panic]"));
    assert!(fault_harnesses.contains("left.encode() == right.encode()"));
    assert!(script.contains("--harness session_fault_encoding_roundtrip_is_exact"));
    assert!(script.contains("--harness session_fault_encoding_is_injective"));
    assert!(script.contains("--harness invalid_session_fault_encoding_is_fail_fast"));
    assert!(!fault_harnesses.contains("kani::assume"));
    for harness in [
        "packed_state_preserves_full_count_and_fault_code",
        "attachment_count_accepts_exact_full_role_domain",
        "attachment_increment_preserves_packed_fault_code",
        "attachment_count_allows_256_and_rejects_257",
    ] {
        assert!(association_harnesses.contains(&format!("fn {harness}()")));
        assert!(script.contains(&format!("--harness {harness}")));
    }
    assert!(association_harnesses.contains("count_256 == ENTRY_COUNT_MAX"));
    assert!(association_harnesses.contains("next_attachment_count(count_256).is_none()"));
    assert!(association_harnesses.contains("AssocTable::entry_fault_code(updated) == fault"));
    for harness in [
        "packed_event_conflict_decoding_accepts_exact_domain",
        "optional_route_arm_decoding_accepts_exact_domain",
        "packed_local_dependency_decoding_accepts_exact_domain",
        "packed_local_dependency_event_bounds_are_exact",
    ] {
        assert!(descriptor_harnesses.contains(&format!("fn {harness}()")));
        assert!(script.contains(&format!("--harness {harness}")));
    }
    assert!(
        descriptor_harnesses
            .contains("route_ordinal < crate::global::const_dsl::ScopeId::LOCAL_CAPACITY")
    );
    assert!(
        descriptor_harnesses
            .contains("dep_ordinal < crate::global::const_dsl::ScopeId::LOCAL_CAPACITY")
    );
    assert!(descriptor_harnesses.contains("matches!(decoded, Some(None)) == absent"));
    for harness in [
        "packed_lane_range_encoding_avoids_reserved_sentinel",
        "packed_lane_range_reserved_sentinel_is_rejected",
        "resident_route_arm_index_decoding_accepts_exact_binary_domain",
        "resident_route_scope_decoding_accepts_exact_domain",
        "resident_roll_scope_decoding_accepts_exact_domain",
        "resident_event_header_decoding_accepts_exact_domain",
        "resident_local_step_lane_decoding_accepts_exact_domain",
        "resident_route_commit_decision_match_is_exact",
        "resident_route_arm_lane_step_decoding_accepts_exact_domain",
        "role_image_fit_probe_rejects_undersized_storage",
        "role_image_fit_probe_rejects_plan_mismatch",
    ] {
        assert!(image_harnesses.contains(&format!("fn {harness}()")));
        assert!(script.contains(&format!("--harness {harness}")));
    }
    assert!(!image_harnesses.contains("kani::assume"));
    assert!(image_harnesses.contains("#[kani::should_panic]"));
    assert!(
        image_harnesses
            .contains("kani::cover!(valid && left_scope == right_scope && left_arm != right_arm);")
    );
    assert!(image_harnesses.contains("&& left_mark_raw != right_mark_raw"));
    assert!(image_harnesses.contains("plan.build_if_fits::<{ ROLE_IMAGE_EVENT_STRIDE - 1 }, 1>"));
    assert!(image_harnesses.contains("plan.build_if_fits::<ROLE_IMAGE_EVENT_STRIDE, 1>"));

    let harness = "scope_id_decoding_accepts_exact_compact_domain";
    assert!(scope_harnesses.contains(&format!("fn {harness}()")));
    assert!(scope_harnesses.contains("scope.is_none() == (raw == u16::MAX)"));
    assert!(script.contains(&format!("--harness {harness}")));

    for harness in [
        "two_by_two_parallel_lane_matching_has_minimum_span",
        "lane_endpoint_index_aggregates_exact_symbolic_membership",
        "two_arm_route_frame_coloring_is_exact",
    ] {
        assert!(allocation_harnesses.contains(&format!("fn {harness}()")));
        assert!(script.contains(&format!("--harness {harness}")));
    }
    let harness = "nested_roll_frame_coloring_uses_the_complete_inbound_key";
    assert!(roll_coloring_harnesses.contains(&format!("fn {harness}()")));
    assert!(script.contains(&format!("--harness {harness}")));
    for harness in [
        "parallel_lane_coloring_reuses_disjoint_class",
        "parallel_lane_coloring_separates_conflicting_class",
        "lane_reuse_conflict_matches_endpoint_equality",
    ] {
        assert!(production_coloring_harnesses.contains(&format!("fn {harness}()")));
        assert!(script.contains(&format!("--harness {harness}")));
    }
    let harness = "three_by_three_parallel_lane_matching_certificate_is_maximum";
    assert!(maximum_matching_harness.contains(&format!("fn {harness}()")));
    assert!(maximum_matching_harness.contains("assert!(actual == expected)"));
    assert!(script.contains(&format!("--harness {harness}")));
    assert!(
        production_coloring_harnesses.contains("assert!(source.node_at(1).atom_data().lane == 0)")
    );
    assert!(
        production_coloring_harnesses.contains("assert!(source.node_at(1).atom_data().lane == 1)")
    );
    assert!(
        allocation_harnesses
            .contains("source.frame_label_at(1) == if same_inbound_key { 1 } else { 0 }")
    );
    assert!(!allocation_harnesses.contains("kani::assume"));

    let controller_harnesses = read("src/global/const_dsl/endpoint_controller/kani.rs");
    let harness = "controller_merge_accepts_exact_single_role_domain";
    assert!(controller_harnesses.contains(&format!("fn {harness}()")));
    assert!(controller_harnesses.contains("merged.unique().is_some(), left == right"));
    assert!(!controller_harnesses.contains("kani::assume"));
    assert!(script.contains(&format!("--harness {harness}")));

    for harness in [
        "outbound_selector_identity_is_exact_public_send_contract",
        "observer_path_decision_has_exact_merge_domain",
    ] {
        assert!(endpoint_selector_harnesses.contains(&format!("fn {harness}()")));
        assert!(script.contains(&format!("--harness {harness}")));
    }
    assert!(endpoint_selector_harnesses.contains("left.payload_schema == right.payload_schema"));
    assert!(endpoint_selector_harnesses.contains("(Some(_), None) | (None, Some(_)) =>"));
    assert!(!endpoint_selector_harnesses.contains("kani::assume"));
    let harness = "route_controller_or_in_band_evidence_is_exact_acceptance_domain";
    assert!(route_knowledge_harnesses.contains(&format!("fn {harness}()")));
    assert!(route_knowledge_harnesses.contains("role == controller || observer_paths_mergeable"));
    assert!(route_knowledge_harnesses.contains("role != controller && !observer_paths_mergeable"));
    assert!(script.contains(&format!("--harness {harness}")));
    assert!(!route_knowledge_harnesses.contains("kani::assume"));

    for harness in [
        "three_event_causal_handoff_accepts_every_valid_role_assignment",
        "sender_change_without_causal_handoff_is_rejected",
        "roll_reentry_causal_closure_rejects_open_cycle",
        "roll_reentry_causal_closure_accepts_closed_cycle",
    ] {
        assert!(receive_lane_harnesses.contains(&format!("fn {harness}()")));
        assert!(script.contains(&format!("--harness {harness}")));
    }
    assert!(receive_lane_harnesses.contains("earlier_source != later_source"));
    assert!(receive_lane_harnesses.contains("event(receiver, later_source, lane)"));
    assert!(receive_lane_harnesses.contains("assert!(!validate_receive_lane_causality(&events))"));
    assert!(receive_lane_harnesses.contains("receive_precedes_after_roll_reentry"));

    for harness in [
        "route_resolver_row_decoding_accepts_exact_range_domain",
        "canonical_route_participant_identity_accepts_full_u8_role_domain",
        "dynamic_route_resolver_identity_is_scope_and_id",
    ] {
        assert!(resolver_row_harnesses.contains(&format!("fn {harness}()")));
        assert!(script.contains(&format!("--harness {harness}")));
    }
    assert!(resolver_row_harnesses.contains("assert!(decoded.is_some() == expected)"));
    assert!(resolver_row_harnesses.contains("left_scope == right_scope && left_id == right_id"));
    assert!(resolver_row_harnesses.contains("participant_start.checked_add"));
    assert!(resolver_row_harnesses.contains("participant_mid.is_some_and"));
    assert!(resolver_row_harnesses.contains("mid < participant_end"));
    assert!(resolver_row_harnesses.contains("participant_end - mid <= 256"));
    assert!(resolver_row_harnesses.contains("participant_end <= participant_count"));
    assert!(resolver_row_harnesses.contains("ProgramImageFacts { max_role: u8::MAX }"));
    assert!(resolver_row_harnesses.contains("program.role_count() == 256"));
    assert!(resolver_row_harnesses.contains("route_has_participant(scope, 0, controller)"));
    assert!(!resolver_row_harnesses.contains("kani::assume"));

    for harness in [
        "program_image_columns_are_canonical_for_exact_count_domain",
        "program_image_columns_reject_total_byte_overflow",
        "program_image_fit_probe_rejects_undersized_storage",
        "program_image_constructor_rejects_undersized_storage",
        "scope_marker_identity_tag_is_exact_and_injective",
        "packed_column_range_construction_is_exact_for_resident_stride_domain",
        "compiled_program_column_range_rejects_stride_multiplication_overflow",
        "role_image_column_range_rejects_stride_multiplication_overflow",
        "compiled_program_blob_comparison_matches_array_equality",
        "compiled_program_image_identity_is_exact_over_facts_columns_and_blob",
        "program_atom_row_decoding_accepts_exact_domain",
        "compiled_program_atom_blob_decoding_preserves_every_schema_bit",
    ] {
        assert!(program_ref_harnesses.contains(&format!("fn {harness}()")));
        assert!(script.contains(&format!("--harness {harness}")));
    }
    assert!(program_ref_harnesses.contains("assert!(decoded.is_some() == expected)"));
    assert!(program_ref_harnesses.contains("row.atom.payload_schema == payload_schema"));
    assert!(program_ref_harnesses.contains("row.payload_schema == payload_schema"));
    assert_eq!(
        program_ref_harnesses
            .matches("ProgramColumnRange::new(0, 2, usize::MAX)")
            .count(),
        1
    );
    assert_eq!(
        program_ref_harnesses
            .matches("let _ = ColumnRange::new(0, 2, usize::MAX)")
            .count(),
        1
    );
    assert!(program_ref_harnesses.contains("kani::cover!(valid);"));
    assert!(program_ref_harnesses.contains("kani::cover!(!valid);"));
    for stride in [
        "0 => 1", "1 => 2", "2 => 4", "3 => 5", "4 => 6", "5 => 7", "6 => 8", "7 => 10", "8 => 11",
    ] {
        assert!(program_ref_harnesses.contains(stride));
    }
    assert!(program_columns.contains("let byte_len = match len.checked_mul(stride)"));
    assert!(program_columns.contains("scope_marker_len: u16"));
    assert!(program_columns.contains("route_participant_len: u16"));
    assert!(program_columns.contains("route_participant_len,"));
    assert!(program_blob_storage.contains("fn write_scope_marker("));
    assert!(
        program_blob_storage.contains("scope_marker_identity_tag(marker.event, marker.reentry)")
    );
    assert!(
        program_blob_storage
            .contains("out.write_scope_marker(columns.scope_markers(), idx, markers.at(idx));")
    );
    assert!(role_image_types.contains("let byte_len = match len.checked_mul(stride)"));
    assert!(program_ref_harnesses.contains("let left_bytes: [u8; 16] = kani::any();"));
    assert!(program_ref_harnesses.contains("let right_bytes: [u8; 16] = kani::any();"));
    assert!(program_ref_harnesses.contains("let expected = left_bytes == right_bytes;"));
    assert!(program_ref_harnesses.contains("assert!(left.same_image(&right) == expected);"));
    assert!(program_ref_harnesses.contains("kani::cover!(expected);"));
    assert!(program_ref_harnesses.contains("kani::cover!(!expected);"));
    assert!(program_ref_harnesses.contains("assert!(!canonical.same_image(&different_facts));"));
    assert!(program_ref_harnesses.contains("assert!(!canonical.same_image(&different_columns));"));
    assert!(
        program_ref_harnesses.contains("assert!(!canonical.same_image(&different_final_byte));")
    );
    assert!(
        program_ref_harnesses
            .contains("kani::cover!(canonical.same_image(&same_image_at_another_address));")
    );
    assert!(
        program_ref_harnesses.contains("kani::cover!(!canonical.same_image(&different_facts));")
    );
    assert!(
        program_ref_harnesses.contains("kani::cover!(!canonical.same_image(&different_columns));")
    );
    assert!(
        program_ref_harnesses
            .contains("kani::cover!(!canonical.same_image(&different_final_byte));")
    );
    assert!(program_ref_harnesses.contains(
        "assert!(canonical.columns.blob_len() == different_columns.columns.blob_len());"
    ));
    assert!(program_ref_harnesses.contains("ProgramImageColumns::new(0, 0, 27, 0)"));
    assert!(program_ref_harnesses.contains("ProgramImageColumns::new(2, 0, 5, 0)"));
    assert!(program_ref_harnesses.contains("let overflow = blob_len > usize::from(u16::MAX);"));
    assert!(program_ref_harnesses.contains("(u16::MAX, u16::MAX, u16::MAX, u16::MAX)"));
    assert!(program_ref_harnesses.contains("ProgramImageBytes::<10>::from_image_if_fits"));
    assert!(program_ref_harnesses.contains(".is_none()"));
    assert!(!program_ref_harnesses.contains("kani::assume"));

    let harness = "resolver_registration_key_is_program_and_id";
    assert!(resolver_registration_harnesses.contains(&format!("fn {harness}()")));
    assert!(
        resolver_registration_harnesses
            .contains("assert!((left == same_program) == (left_id == right_id));")
    );
    assert!(resolver_registration_harnesses.contains("assert!(left != other_program);"));
    assert!(!resolver_registration_harnesses.contains("kani::assume"));
    assert!(script.contains(&format!("--harness {harness}")));

    let harness = "resolver_registration_key_rejects_intrinsic_id";
    assert!(resolver_registration_harnesses.contains(&format!("fn {harness}()")));
    assert!(resolver_registration_harnesses.contains("#[kani::should_panic]"));
    assert!(resolver_registration_harnesses.contains("INTRINSIC_ROUTE_RESOLVER_ID"));
    assert!(script.contains(&format!("--harness {harness}")));

    let harness = "resolver_initial_storage_is_initialized_and_dispatchable";
    assert!(resolver_registration_harnesses.contains(&format!("fn {harness}()")));
    assert!(resolver_registration_harnesses.contains("MaybeUninit::<Option<ResolverBucketEntry"));
    assert!(resolver_registration_harnesses.contains("bucket.entry_count() == 0"));
    assert!(resolver_registration_harnesses.contains("bucket.get(key).is_none()"));
    assert!(resolver_registration_harnesses.contains("resolver.resolve_decision()"));
    assert!(script.contains(&format!("--harness {harness}")));
    assert_eq!(
        resolver_registration_harnesses
            .matches("bucket.replace_storage(")
            .count(),
        2,
        "both initial allocation and replacement proofs must enter the production wrapper"
    );

    let harness = "resolver_replacement_compacts_entries_and_preserves_dispatch";
    assert!(resolver_registration_harnesses.contains(&format!("fn {harness}()")));
    assert!(
        resolver_registration_harnesses.contains("let hole = usize::from(kani::any::<u8>() % 3);")
    );
    assert!(resolver_registration_harnesses.contains("kani::cover!(hole == 0);"));
    assert!(resolver_registration_harnesses.contains("kani::cover!(hole == 1);"));
    assert!(resolver_registration_harnesses.contains("kani::cover!(hole == 2);"));
    assert!(
        resolver_registration_harnesses
            .contains("bucket.replace_storage(replacement.cast(), replacement_slots.len());")
    );
    assert!(!resolver_registration_harnesses.contains("bucket.init_replacement_storage"));
    assert!(!resolver_registration_harnesses.contains("bucket.commit_storage"));
    assert!(resolver_registration_harnesses.contains("bucket.entry_count() == 2"));
    assert!(resolver_registration_harnesses.contains("Ok(DecisionArm::Left)"));
    assert!(resolver_registration_harnesses.contains("Ok(DecisionArm::Right)"));
    assert!(script.contains(&format!("--harness {harness}")));
}
