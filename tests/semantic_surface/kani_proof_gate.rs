use super::common::{read, read_all_rs_tree};

#[test]
fn kani_gate_verifies_production_rust_without_entering_the_package_surface() {
    let root_manifest = read("Cargo.toml");
    let verification_manifest = read("proofs/kani/Cargo.toml");
    let verification_readme = read("proofs/kani/README.md");
    let inventory = read("proofs/kani/harness-inventory.json");
    let script = read(".github/scripts/check_kani.sh");
    let workflow = read(".github/workflows/quality-gates.yml");
    let production_rust = read_all_rs_tree("src");
    let frontier_kind = read("src/endpoint/kernel/frontier/kind.rs");
    let session_fault = read("src/rendezvous/association/fault.rs");
    let endpoint_lease_state = read("src/rendezvous/core.rs");
    let rendezvous_access_state = read("src/rendezvous/core/access_state.rs");
    let harnesses = read("src/rendezvous/core/storage_layout/capacity/kani.rs");
    let authority_harnesses = read("src/endpoint/kernel/authority/kani.rs");
    let decision_state_harnesses = read("src/endpoint/kernel/decision_state/kani.rs");
    let active_offer_harnesses = read("src/endpoint/kernel/frontier/active_offer_entry/kani.rs");
    let frontier_entry_harnesses = read("src/endpoint/kernel/frontier/entry_sets/kani.rs");
    let frontier_observation = read("src/endpoint/kernel/frontier/observation.rs");
    let frontier_scratch = read("src/endpoint/kernel/frontier/scratch.rs");
    let frontier_scratch_harnesses = read("src/endpoint/kernel/frontier/scratch/kani.rs");
    let frontier_entry_buffer = read("src/endpoint/kernel/frontier/entry_sets/buffer.rs");
    let endpoint_core = read("src/endpoint/kernel/core.rs");
    let rendezvous_port = read("src/rendezvous/port.rs");
    let frontier_snapshot = read("src/endpoint/kernel/frontier/snapshot.rs");
    let frontier_snapshot_harnesses = read("src/endpoint/kernel/frontier/snapshot/kani.rs");
    let frontier_observation_harnesses = read("src/endpoint/kernel/core/frontier_observation.rs");
    let scope_evidence_harnesses = read("src/endpoint/kernel/evidence_store.rs");
    let alignment_harnesses =
        read("src/endpoint/kernel/offer/select_alignment/model/pool/tests.rs");
    let offer_ingress_harnesses = read("src/endpoint/kernel/offer/ingress/kani.rs");
    let lane_set = read("src/global/role_program/lane_set.rs");
    let lane_set_harnesses = read("src/global/role_program/lane_set/kani.rs");
    let frontier_state_harnesses = read("src/endpoint/kernel/frontier_state/kani.rs");
    let public_operation_harnesses = read("src/endpoint/kernel/core/public_operation/kani.rs");
    let descriptor_harnesses = read("src/global/typestate/facts/kani.rs");
    let cursor_harnesses = read("src/global/typestate/cursor/kani.rs");
    let image_harnesses = read("src/global/role_program/image_impl/kani.rs");
    let role_event_rows = read("src/global/role_program/image_impl/event_rows.rs");
    let lane_projection_harnesses =
        read("src/global/role_program/image_impl/projection/lanes/kani.rs");
    let scope_harnesses = read("src/global/const_dsl/scope/kani.rs");
    let route_scope_range_harnesses = read("src/global/const_dsl/scope_ranges/kani.rs");
    let scope_range_harnesses = read("src/global/const_dsl/scope_ranges/nesting/kani.rs");
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
    let wire_harnesses = read("src/transport/wire/kani.rs");
    let layout_arithmetic_harnesses = read("src/runtime_core/layout/kani.rs");
    let unique_match_harnesses = read("src/runtime_core/unique_match/kani.rs");
    let receive_receipt_harnesses = read("src/rendezvous/recv_frame_receipt/kani.rs");
    let fault_harnesses = read("src/rendezvous/association/fault/kani.rs");
    let association_harnesses = read("src/rendezvous/association/kani.rs");
    let program_blob_storage = read("src/global/compiled/images/image/blob_storage.rs");
    let route_facts = read("src/global/const_dsl/route.rs");
    let program_columns = read("src/global/compiled/images/image/columns.rs");
    let role_image_types = read("src/global/role_program/image_types.rs");
    let resolver_registration_harnesses =
        read("src/session/cluster/core/dynamic_resolvers/kani.rs");
    let version = read(".github/kani-version");

    assert_eq!(version.trim(), "0.67.0");
    assert!(verification_manifest.contains("path = \"../../src/lib.rs\""));
    assert!(!verification_manifest.contains("[dependencies]"));
    assert!(!verification_manifest.contains("rust-version"));
    assert!(verification_readme.contains("complete `u16` resolver-id domain"));
    assert!(verification_readme.contains("Kani itself discovers every production"));
    assert!(verification_readme.contains("structured\nJSON inventory"));
    assert!(verification_readme.contains("without passing a filter"));
    assert!(verification_readme.contains("without a hand-written second"));
    assert!(verification_readme.contains("derive\n`kani::Arbitrary` from the enum declaration"));
    assert!(
        verification_readme
            .contains("does not treat an unchanged harness name as proof that its body was not")
    );
    assert!(!verification_readme.contains("intrinsic resolver sentinel"));
    assert!(!verification_readme.contains("inventory contains 81"));
    assert!(root_manifest.contains("'cfg(kani)'"));
    assert!(root_manifest.contains("\"!/src/**/kani.rs\""));
    for harness in [
        "equal_projected_scope_ranges_follow_source_preorder",
        "strict_scope_containment_is_authoritative",
        "crossing_scope_ranges_are_rejected",
    ] {
        assert!(scope_range_harnesses.contains(&format!("fn {harness}()")));
    }
    for harness in [
        "normalized_route_primary_preserves_all_valid_compact_bounds",
        "normalized_route_primary_rejects_an_empty_arm",
        "nested_route_topology_authorities_are_exact_and_coherent",
    ] {
        assert!(route_scope_range_harnesses.contains(&format!("fn {harness}()")));
    }
    for harness in [
        "preferred_lane_scan_first_step_is_exact_over_the_full_lane_domain",
        "remaining_lane_scan_step_is_exact_over_the_full_lane_domain",
    ] {
        assert!(offer_ingress_harnesses.contains(&format!("fn {harness}()")));
    }
    assert!(lane_set.contains("pub(crate) use core::primitive::u32 as LaneWord;"));
    assert!(!lane_set.contains("primitive::usize as LaneWord"));
    assert!(lane_set_harnesses.contains("#[kani::unwind(10)]"));
    assert!(lane_set_harnesses.contains(
        "fn descriptor_lane_byte_iteration_returns_the_first_set_lane_in_the_exact_domain()"
    ));
    assert!(
        lane_set_harnesses
            .contains("fn descriptor_lane_byte_view_rejects_lengths_beyond_the_lane_domain()")
    );
    assert!(lane_set.contains("byte_len > lane_byte_count(LANE_DOMAIN_SIZE)"));
    for harness in [
        "frontier_scratch_capacity_is_derived_once_from_its_layout",
        "lane_domain_frontier_workspace_fits_compact_resident_budget",
        "frontier_scratch_rejects_capacity_beyond_lane_domain",
        "zero_capacity_frontier_scratch_rejects_misaligned_storage_before_slice_publication",
        "zero_capacity_frontier_scratch_yields_an_empty_candidate_slice",
        "arbitrary_scratch_bytes_are_canonicalized_before_typed_publication",
    ] {
        assert!(frontier_scratch_harnesses.contains(&format!("fn {harness}()")));
    }
    assert!(frontier_scratch.contains("scratch: &'lease mut [u8]"));
    assert!(frontier_scratch.contains("ptr.add(index).write(initial)"));
    assert!(frontier_scratch.contains("if max_frontier_entries > LANE_DOMAIN_SIZE"));
    assert!(
        frontier_scratch.contains("pub(crate) struct FrontierScratchWorkspace<'lease>")
            && frontier_scratch
                .contains("pub(crate) struct FrontierScratchSectionLease<'lease, T>")
            && frontier_scratch.contains("PhantomData<&'lease mut [T]>")
    );
    assert!(
        frontier_scratch.contains("active.end() > observed.offset()")
            && frontier_scratch.contains("observed.end() > candidates.offset()")
            && frontier_scratch.contains("candidates.end() > layout.total_bytes()")
            && frontier_scratch_harnesses.contains(
                "layout.global_active_entry_slots().end() <= layout.observed_entry_slots().offset()"
            )
    );
    assert!(
        frontier_entry_buffer.contains("pub(super) struct EntryView<'a, T>")
            && frontier_entry_buffer.contains("slots: &'a [T]")
            && frontier_entry_buffer.contains("pub(super) struct EntryBuffer<'a, T>")
            && frontier_entry_buffer.contains("slots: &'a mut [T]")
            && frontier_entry_buffer.contains("fn from_slice(slots: &'a mut [T])")
            && !frontier_entry_buffer.contains("*mut T")
    );
    assert!(
        endpoint_core.contains("lease: &'lease mut ScratchLease<'r>")
            && endpoint_core.contains("-> FrontierScratchWorkspace<'lease>")
            && rendezvous_port
                .contains("fn authorizes(&self, state: &Cell<RendezvousAccessState>)")
            && rendezvous_port.contains("self.require_scratch_lease(lease);")
    );
    for forbidden in [
        "FrontierScratchView",
        "frontier_scratch_view_from_storage",
        "frontier_global_active_entries_view_from_storage",
        "frontier_observed_entries_view_from_storage",
    ] {
        assert!(
            !frontier_scratch.contains(forbidden),
            "frontier scratch must not retain lifetime-free raw view authority: {forbidden}"
        );
    }
    assert!(frontier_snapshot.contains("pub(crate) struct FrontierSnapshot<'a>"));
    assert!(frontier_snapshot.contains("candidates: &'a mut [FrontierCandidate]"));
    assert!(!frontier_snapshot.contains("candidates: *mut FrontierCandidate"));
    assert!(
        frontier_snapshot_harnesses
            .contains("fn two_cell_frontier_snapshot_never_publishes_a_third_candidate()")
    );
    assert_eq!(
        offer_ingress_harnesses
            .matches("#[kani::unwind(10)]")
            .count(),
        2
    );
    assert!(script.contains("cargo kani --version"));
    assert!(script.contains("rg -n 'kani::assume' \"${ROOT_DIR}/src\" --glob '*.rs'"));
    assert!(
        script.contains(
            "Kani harnesses must construct complete symbolic domains without assumptions"
        )
    );
    assert!(!production_rust.contains("kani::assume"));
    assert!(script.contains("list --format json"));
    assert!(script.contains("proofs/kani/harness-inventory.json"));
    assert!(script.contains("cmp -s \"${EXPECTED_INVENTORY}\" \"${ACTUAL_INVENTORY}\""));
    assert!(script.contains("expected_harness_total="));
    assert!(script.contains("!= \"${expected_harness_total}\""));
    assert!(script.contains("cargo kani \\"));
    assert!(script.contains("--run-sanity-checks"));
    assert!(!script.contains("--harness"));
    assert!(script.contains("hibana-kani-verification.XXXXXX"));
    assert!(script.contains("exactly one successful complete-harness summary"));
    assert!(script.contains("successfully verified harnesses, 0 failures"));
    assert!(script.contains("reported_harness_total="));
    assert!(script.contains("requires the complete ${expected_harness_total}-harness inventory"));
    assert!(script.contains(
        "Kani gate passed version=${EXPECTED_VERSION} harnesses=${kani_harness_total} backend=CBMC"
    ));
    assert!(!script.contains("command -v cargo-kani"));
    assert!(!script.contains("exit 0"));
    assert!(inventory.contains("\"kani-version\": \"0.67.0\""));
    assert!(inventory.contains("\"standard-harnesses\": 192"));
    assert!(inventory.contains("\"contract-harnesses\": 0"));
    for (owner, enum_name) in [
        (&frontier_kind, "FrontierKind"),
        (&session_fault, "SessionFaultKind"),
        (&endpoint_lease_state, "EndpointLeaseState"),
        (&rendezvous_access_state, "RendezvousAccessState"),
        (&route_facts, "ReentryMark"),
    ] {
        assert!(owner.contains(&format!(
            "#[cfg_attr(kani, derive(kani::Arbitrary))]\npub(crate) enum {enum_name}"
        )));
    }
    assert!(program_blob_storage.contains(
        "#[cfg_attr(kani, derive(kani::Arbitrary))]\npub(super) enum DescriptorScopeEvent"
    ));
    assert!(session_fault.contains("#[path = \"fault/kani.rs\"]\nmod kani_proofs;"));
    assert!(!session_fault.contains("mod kani;"));
    assert!(active_offer_harnesses.contains("let first_frontier: FrontierKind = kani::any();"));
    assert!(!active_offer_harnesses.contains("frontier_from_raw"));
    assert!(
        frontier_entry_harnesses
            .matches("let frontier: FrontierKind = kani::any();")
            .count()
            == 2
    );
    assert!(
        frontier_entry_harnesses
            .contains("assert_eq!(frontier.bit() & !FrontierKind::ALL_BITS, 0)")
    );
    assert!(fault_harnesses.contains("let fault: SessionFaultKind = kani::any();"));
    assert!(fault_harnesses.contains("let left: SessionFaultKind = kani::any();"));
    assert!(fault_harnesses.contains("let right: SessionFaultKind = kani::any();"));
    assert!(!fault_harnesses.contains("SYMBOLIC_FAULTS"));
    assert!(program_ref_harnesses.contains("let left_event: DescriptorScopeEvent = kani::any();"));
    assert!(program_ref_harnesses.contains("let left_reentry: ReentryMark = kani::any();"));
    assert!(!program_ref_harnesses.contains("left_event_raw"));
    assert!(!program_ref_harnesses.contains("left_reentry_raw"));
    assert!(image_harnesses.contains("let left_mark: ReentryMark = kani::any();"));
    assert!(image_harnesses.contains("let right_mark: ReentryMark = kani::any();"));
    assert!(!image_harnesses.contains("left_mark_raw"));
    assert!(!image_harnesses.contains("right_mark_raw"));
    assert!(harnesses.contains("let state: EndpointLeaseState = kani::any();"));
    assert!(harnesses.contains("let state: RendezvousAccessState = kani::any();"));
    assert!(!harnesses.contains("MembershipSealed as u8"));
    assert!(!harnesses.contains("EndpointScratchLease as u8"));
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
    }
    assert!(!script.contains("rg -q"));
    assert!(!script.contains("hibana-kani-proofs.XXXXXX"));
    assert!(!script.contains("kani_harness_args"));
    for harness in [
        "u32_word_count_is_exact_over_the_complete_usize_domain",
        "checked_alignment_is_exact_over_the_complete_usize_domain",
        "checked_absolute_offset_alignment_is_exact_and_never_wraps",
    ] {
        assert!(layout_arithmetic_harnesses.contains(&format!("fn {harness}()")));
    }
    assert!(
        cursor_harnesses
            .contains("fn local_event_bit_exists_exactly_inside_the_local_step_domain()")
    );
    assert!(
        cursor_harnesses.contains("assert_eq!(located.is_some(), step_idx < local_step_count)")
    );
    assert!(script.contains("Kani should-panic harness may panic before its production call"));
    assert!(script.contains(
        "Kani should-panic harness may not draw direct symbolic input; use a checked constructor"
    ));
    assert!(script.contains(r#"re.search(r"\bkani\s*::\s*any\b", body)"#));
    assert!(
        decision_state_harnesses
            .contains("fn selected_route_commit_rows_finish_is_lane_exact_and_fail_closed()")
    );
    assert!(decision_state_harnesses.contains("routes: SelectedRouteCommitRowsRef::EMPTY"));
    assert!(decision_state_harnesses.contains("assert!(empty.is_empty())"));
    assert!(decision_state_harnesses.contains("finish_for_lane(mismatched_lane)"));
    assert!(decision_state_harnesses.contains("assert!(rejected.is_err())"));
    assert!(decision_state_harnesses.contains("candidate_end <= u32::from(u16::MAX)"));
    assert!(decision_state_harnesses.contains("kani::cover!(start == u16::MAX - 1 && len == 1)"));
    for harness in [
        "active_offer_entry_accepts_only_exact_scope_entry_metadata",
        "active_offer_entry_foreign_scope_is_exact_rejection",
    ] {
        assert!(active_offer_harnesses.contains(&format!("fn {harness}()")));
    }
    assert!(!active_offer_harnesses.contains("kani::assume"));
    for harness in [
        "frontier_entry_identity_distinguishes_scope_at_same_entry",
        "active_frontier_entry_rejects_absent_exact_key",
        "offer_entry_key_rejects_non_route_scopes",
        "frontier_observation_rows_preserve_exact_witnesses_for_one_cursor_target",
        "exact_observation_buffer_retains_same_entry_witness_rows",
        "exact_observation_buffer_groups_all_cursor_target_order_classes",
        "selectable_ready_query_never_admits_an_excluded_exact_witness",
        "exact_observation_capacity_exhaustion_is_fail_closed",
        "frontier_observation_rejects_absent_exact_key",
        "frontier_observation_rejects_first_bit_outside_exact_kind_domain",
        "frontier_observation_rejects_first_flag_outside_exact_domain",
    ] {
        assert!(frontier_entry_harnesses.contains(&format!("fn {harness}()")));
    }
    assert!(frontier_observation.contains("pub(crate) const fn accepts_exact_observation("));
    assert!(frontier_observation.contains("if !Self::accepts_exact_observation(observed)"));
    assert!(
        frontier_entry_harnesses
            .contains("FrontierObservationSlot::accepts_exact_observation(observed)")
    );
    assert!(frontier_entry_harnesses.contains("!valid && key.is_absent()"));
    assert!(frontier_entry_harnesses.contains("raw_frontier & !FrontierKind::ALL_BITS != 0"));
    assert!(
        frontier_entry_harnesses.contains("raw_flags & !OfferEntryObservedState::ALL_FLAGS != 0")
    );
    for harness in [
        "exact_scope_rows_never_synthesize_controller_progress_authority",
        "same_entry_erased_flags_cannot_change_exact_current_observation",
        "excluded_exact_current_never_permits_retention",
    ] {
        assert!(alignment_harnesses.contains(&format!("fn {harness}()")));
    }
    assert!(
        frontier_observation_harnesses
            .contains("fn excluded_scope_never_supplies_progress_sibling_authority()")
    );
    for harness in [
        "distinct_ready_arms_are_sticky_conflict_in_either_order",
        "matching_ready_arm_evidence_remains_exact",
        "ready_arm_conflicting_with_live_selection_is_sticky",
        "ready_evidence_transitions_preserve_canonical_masks",
        "poll_and_materialization_consumption_are_exact",
    ] {
        assert!(scope_evidence_harnesses.contains(&format!("fn {harness}()")));
    }
    for harness in [
        "root_frontier_owner_slots_preserve_symbolic_lane_order",
        "root_frontier_owner_slots_distinguish_scope_at_same_entry",
        "root_frontier_owner_slot_survives_first_entry_removal",
        "root_frontier_owner_slot_survives_last_entry_removal",
        "root_frontier_owner_slot_survives_row_compaction",
    ] {
        assert!(frontier_state_harnesses.contains(&format!("fn {harness}()")));
    }
    assert!(frontier_state_harnesses.contains("table.remove_root_row(0)"));
    assert!(!frontier_state_harnesses.contains("kani::assume"));
    assert!(harnesses.contains("fn symbolic_nonempty_range() -> (usize, usize)"));
    assert!(harnesses.contains("if candidate.0 < candidate.1"));
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
    assert!(
        lane_projection_harnesses
            .contains("fn local_lane_accumulator_preserves_exact_lane_relations_and_last_steps()")
    );
    assert!(lane_projection_harnesses.contains("assert_eq!(facts.relation_count, 1)"));
    assert!(
        lane_projection_harnesses
            .contains("assert_eq!(facts.last_steps[lane as usize], last_step)")
    );

    for harness in [
        "endpoint_generation_advances_or_exhausts",
        "endpoint_lease_slot_count_matches_last_index_domain",
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
    }
    for harness in [
        "route_arm_decoding_accepts_exact_binary_domain",
        "single_ready_mask_decoding_is_exact",
    ] {
        assert!(authority_harnesses.contains(&format!("fn {harness}()")));
    }
    assert!(authority_harnesses.contains("Arm::decode_single_ready_mask(mask) == expected"));
    assert!(!authority_harnesses.contains("kani::assume"));
    let harness = "public_operation_transition_classifier_is_exact";
    assert!(public_operation_harnesses.contains(&format!("fn {harness}()")));
    assert!(public_operation_harnesses.contains("current == PublicActiveOp::Poisoned"));
    assert!(public_operation_harnesses.contains("fn symbolic_edge(raw: u8) -> PublicOpEdge"));
    assert!(
        public_operation_harnesses
            .contains("PublicActiveOp::ALL[usize::from(raw) % PublicActiveOp::ALL.len()]",)
    );
    assert!(
        public_operation_harnesses
            .contains("PublicOpEdge::ALL[usize::from(raw) % PublicOpEdge::ALL.len()]")
    );
    assert!(!public_operation_harnesses.contains("match raw %"));
    assert!(public_operation_harnesses.contains("current == edge.expected()"));
    assert!(public_operation_harnesses.contains("let edge = symbolic_edge(kani::any())"));
    assert!(public_operation_harnesses.contains("transition.phase(), edge.next()"));
    assert!(public_operation_harnesses.contains("current.clear_if_current(expected)"));
    assert!(public_operation_harnesses.contains("current.clear_terminal()"));
    assert!(public_operation_harnesses.contains("current.fault()"));
    assert!(
        public_operation_harnesses
            .matches("transition.phase(), PublicActiveOp::Poisoned")
            .count()
            == 2
    );
    assert!(!public_operation_harnesses.contains("kani::assume"));
    assert!(
        cursor_harnesses.contains("fn compact_cursor_position_covers_the_full_u16_value_domain()")
    );
    assert!(cursor_harnesses.contains("position <= u16::MAX as usize"));
    assert!(
        descriptor_harnesses
            .contains("fn checked_state_index_acceptance_is_the_exact_present_identity_domain()")
    );
    assert!(descriptor_harnesses.contains("StateIndex::checked_from_usize(index)"));
    assert!(
        image_harnesses.contains("fn optional_event_fact_row_reserves_only_the_absent_u16_value()")
    );
    assert!(image_harnesses.contains("row < u16::MAX as usize"));
    assert!(
        role_event_rows
            .matches("encode_optional_event_fact_row(row)")
            .count()
            == 2
            && !role_event_rows.contains("if row > u16::MAX as usize")
    );
    for harness in [
        "frame_header_roundtrip_preserves_every_field",
        "frame_header_identity_is_exact_and_injective",
    ] {
        assert!(transport_harnesses.contains(&format!("fn {harness}()")));
    }
    assert!(transport_harnesses.contains("left_header == right_header"));
    assert!(!transport_harnesses.contains("kani::assume"));
    for harness in [
        "fixed_array_schema_identity_is_injective_over_the_complete_admitted_domain",
        "fixed_array_schema_identity_rejects_the_first_colliding_width",
    ] {
        assert!(wire_harnesses.contains(&format!("fn {harness}()")));
    }
    assert!(wire_harnesses.contains("left_schema != right_schema || left == right"));
    assert!(fault_harnesses.contains("fn session_fault_encoding_roundtrip_is_exact()"));
    assert!(!fault_harnesses.contains("fn symbolic_fault("));
    assert!(fault_harnesses.contains("SessionFaultKind::decode(encoded) == Some(fault)"));
    assert!(fault_harnesses.contains("fn session_fault_encoding_is_injective()"));
    assert!(fault_harnesses.contains("fn session_fault_checked_decoding_domain_is_exact()"));
    assert!(fault_harnesses.contains("assert_eq!(checked.is_some(), raw <= 5)"));
    assert!(fault_harnesses.contains("fn session_fault_decoder_rejects_first_invalid_code()"));
    assert!(session_fault.contains("pub(super) const fn try_decode(raw: u8)"));
    assert!(session_fault.contains("match Self::try_decode(raw)"));
    assert!(fault_harnesses.contains("#[kani::should_panic]"));
    assert!(fault_harnesses.contains("left.encode() == right.encode()"));
    assert!(!fault_harnesses.contains("kani::assume"));
    for harness in [
        "packed_state_preserves_full_count_and_fault_code",
        "attachment_count_accepts_exact_full_role_domain",
        "attachment_increment_preserves_packed_fault_code",
        "attachment_count_allows_256_and_rejects_257",
    ] {
        assert!(association_harnesses.contains(&format!("fn {harness}()")));
    }
    assert!(association_harnesses.contains("count_256 == ENTRY_COUNT_MAX"));
    assert!(association_harnesses.contains("next_attachment_count(count_256).is_none()"));
    assert!(association_harnesses.contains("AssocTable::entry_fault_code(updated) == fault"));
    for harness in [
        "inbound_frame_key_matching_is_exact_on_every_wire_axis",
        "inbound_frame_key_rejects_each_single_axis_mismatch",
        "deterministic_inbound_key_matching_is_exact_on_every_available_axis",
        "deterministic_inbound_key_rejects_each_single_axis_mismatch",
        "packed_event_conflict_decoding_accepts_exact_domain",
        "optional_route_arm_decoding_accepts_exact_domain",
        "packed_local_dependency_decoding_accepts_exact_domain",
        "packed_local_dependency_event_bounds_are_exact",
    ] {
        assert!(descriptor_harnesses.contains(&format!("fn {harness}()")));
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
        "unique_match_zero_one_and_distinct_many_are_exact",
        "unique_match_ambiguity_is_absorbing",
    ] {
        assert!(unique_match_harnesses.contains(&format!("fn {harness}()")));
    }
    assert!(!unique_match_harnesses.contains("kani::assume"));
    for harness in [
        "packed_lane_range_checked_constructor_domain_is_exact",
        "packed_lane_optional_range_canonicality_is_exact",
        "active_lane_metadata_is_exact_for_every_first_byte_bitmap",
        "inactive_lane_metadata_has_one_empty_representation",
        "active_lane_metadata_preserves_the_high_lane_boundary",
        "lane_column_coherence_is_exact_for_one_binary_route",
        "route_commit_partition_requires_the_exact_builder_maximum",
        "packed_lane_range_infallible_constructor_rejects_first_end_overflow",
        "resident_route_arm_index_decoding_accepts_exact_binary_domain",
        "resident_route_scope_decoding_accepts_exact_domain",
        "resident_roll_scope_decoding_accepts_exact_domain",
        "resident_event_header_decoding_accepts_exact_domain",
        "resident_local_step_lane_decoding_accepts_exact_domain",
        "resident_role_local_direction_is_exact_over_the_full_role_domain",
        "resident_route_commit_decision_match_is_exact",
        "resident_passive_child_parent_binding_is_exact",
        "resident_route_arm_lane_step_decoding_accepts_exact_domain",
        "role_image_fit_probe_rejects_undersized_storage",
        "role_image_fit_probe_rejects_plan_mismatch",
    ] {
        assert!(image_harnesses.contains(&format!("fn {harness}()")));
    }
    assert!(!image_harnesses.contains("kani::assume"));
    assert!(image_harnesses.contains("#[kani::should_panic]"));
    assert!(role_image_types.contains("pub(crate) const fn try_new(start: usize, len: usize)"));
    assert!(role_image_types.contains("match Self::try_new(start, len)"));
    assert!(image_harnesses.contains("let start: usize = kani::any()"));
    assert!(image_harnesses.contains("let len: usize = kani::any()"));
    assert!(image_harnesses.contains("assert_eq!(checked.is_some(), encodable)"));
    assert!(
        image_harnesses
            .contains("kani::cover!(valid && left_scope == right_scope && left_arm != right_arm);")
    );
    assert!(image_harnesses.contains("&& left_mark != right_mark"));
    assert!(image_harnesses.contains("plan.build_if_fits::<{ ROLE_IMAGE_EVENT_STRIDE - 1 }, 1>"));
    assert!(image_harnesses.contains("plan.build_if_fits::<ROLE_IMAGE_EVENT_STRIDE, 1>"));

    let harness = "scope_id_decoding_accepts_exact_compact_domain";
    assert!(scope_harnesses.contains(&format!("fn {harness}()")));
    assert!(scope_harnesses.contains("scope.is_none() == (raw == u16::MAX)"));

    for harness in [
        "two_by_two_parallel_lane_matching_has_minimum_span",
        "lane_endpoint_index_aggregates_exact_symbolic_membership",
        "two_arm_route_frame_coloring_is_exact",
    ] {
        assert!(allocation_harnesses.contains(&format!("fn {harness}()")));
    }
    for harness in [
        "nested_roll_frame_coloring_uses_the_complete_inbound_key",
        "route_scope_publication_is_closed_and_atomic",
    ] {
        assert!(roll_coloring_harnesses.contains(&format!("fn {harness}()")));
    }
    for harness in [
        "parallel_lane_coloring_reuses_disjoint_class",
        "parallel_lane_coloring_separates_conflicting_class",
        "lane_reuse_conflict_matches_endpoint_equality",
    ] {
        assert!(production_coloring_harnesses.contains(&format!("fn {harness}()")));
    }
    let harness = "three_by_three_parallel_lane_matching_certificate_is_maximum";
    assert!(maximum_matching_harness.contains(&format!("fn {harness}()")));
    assert!(maximum_matching_harness.contains("assert!(actual == expected)"));
    assert!(production_coloring_harnesses.contains("assert!(source.atom_at(1).lane == 0)"));
    assert!(production_coloring_harnesses.contains("assert!(source.atom_at(1).lane == 1)"));
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

    for harness in [
        "outbound_selector_identity_is_exact_public_send_contract",
        "inbound_selector_identity_is_exact_compact_event_identity",
        "observer_path_decision_has_exact_merge_domain",
    ] {
        assert!(endpoint_selector_harnesses.contains(&format!("fn {harness}()")));
    }
    assert!(endpoint_selector_harnesses.contains("left.payload_schema == right.payload_schema"));
    assert!(endpoint_selector_harnesses.contains("left != u16::MAX"));
    assert!(endpoint_selector_harnesses.contains("(Some(_), None) | (None, Some(_)) =>"));
    assert!(!endpoint_selector_harnesses.contains("kani::assume"));
    let harness = "route_controller_or_in_band_evidence_is_exact_acceptance_domain";
    assert!(route_knowledge_harnesses.contains(&format!("fn {harness}()")));
    assert!(route_knowledge_harnesses.contains("role == controller || observer_paths_mergeable"));
    assert!(route_knowledge_harnesses.contains("role != controller && !observer_paths_mergeable"));
    assert!(!route_knowledge_harnesses.contains("kani::assume"));

    for harness in [
        "causal_witness_table_is_first_write_wins_and_role_exact",
        "three_event_linear_scan_matches_pairwise_checker",
        "three_event_causal_handoff_accepts_every_valid_role_assignment",
        "sender_change_without_causal_handoff_is_rejected",
        "roll_reentry_causal_closure_rejects_open_cycle",
        "roll_reentry_causal_closure_accepts_closed_cycle",
    ] {
        assert!(receive_lane_harnesses.contains(&format!("fn {harness}()")));
    }
    assert!(receive_lane_harnesses.contains("earlier_source != later_source"));
    assert!(receive_lane_harnesses.contains("query_role == first_role"));
    assert!(receive_lane_harnesses.contains("query_role == second_role"));
    assert!(receive_lane_harnesses.contains("event(receiver, later_source, lane)"));
    assert!(receive_lane_harnesses.contains("assert!(!validate_receive_lane_causality(&events))"));
    assert!(receive_lane_harnesses.contains("receive_precedes_after_roll_reentry"));

    for harness in [
        "route_resolver_row_decoding_accepts_exact_range_domain",
        "packed_route_authority_roundtrip_is_exact",
        "canonical_route_participant_identity_accepts_full_u8_role_domain",
        "dynamic_route_resolver_identity_is_scope_and_id",
    ] {
        assert!(resolver_row_harnesses.contains(&format!("fn {harness}()")));
    }
    assert!(resolver_row_harnesses.contains("assert!(decoded.is_some() == expected)"));
    assert!(resolver_row_harnesses.contains("PackedRouteAuthority::encode(scope, resolver)"));
    assert!(resolver_row_harnesses.contains("Some((scope, resolver))"));
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
        "program_image_columns_reject_first_total_byte_overflow",
        "program_image_fit_probe_rejects_undersized_storage",
        "program_image_constructor_rejects_undersized_storage",
        "descriptor_scope_marker_tag_is_exact_and_injective",
        "proof_only_scope_entry_metadata_is_erased_from_descriptor_tags",
        "packed_column_range_construction_is_exact_for_resident_stride_domain",
        "compiled_program_column_range_rejects_stride_multiplication_overflow",
        "role_image_column_range_rejects_stride_multiplication_overflow",
        "compiled_program_blob_comparison_matches_array_equality",
        "compiled_program_image_identity_is_exact_over_facts_columns_and_blob",
        "program_atom_row_decoding_accepts_exact_domain",
        "compiled_program_atom_binary_search_is_exact_for_sorted_rows",
        "compiled_program_atom_order_rejects_noncanonical_rows",
        "compiled_program_atom_blob_decoding_preserves_every_schema_bit",
    ] {
        assert!(program_ref_harnesses.contains(&format!("fn {harness}()")));
    }
    assert!(program_columns.contains("pub(crate) const fn try_new("));
    assert!(program_columns.contains("match Self::try_new("));
    assert!(program_ref_harnesses.contains("let atom_len: usize = kani::any()"));
    assert!(program_ref_harnesses.contains("let scope_marker_len: usize = kani::any()"));
    assert!(program_ref_harnesses.contains("let counts_fit = atom_len"));
    assert!(program_ref_harnesses.contains("assert_eq!(checked.is_some(), valid)"));
    assert!(program_ref_harnesses.contains("kani::cover!(!valid && counts_fit);"));
    assert!(
        program_ref_harnesses.contains("route_resolver_len > COMPACT_DESCRIPTOR_BYTE_CAPACITY")
    );
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
    assert!(program_columns.contains("pub(crate) const fn covers_source_counts("));
    assert!(program_ref_harnesses.contains("let source_resolver_len: usize = kani::any();"));
    assert!(
        program_ref_harnesses.contains("kani::cover!(source_resolver_len <= route_resolver_len);")
    );
    assert!(
        program_ref_harnesses.contains("kani::cover!(source_resolver_len > route_resolver_len);")
    );
    assert!(program_ref_harnesses.contains("source_resolver_len <= route_resolver_len,"));
    assert!(
        program_ref_harnesses
            .matches("columns.covers_source_counts(")
            .count()
            == 4
    );
    assert!(program_blob_storage.contains("fn write_scope_marker("));
    assert!(
        program_blob_storage
            .contains("scope_marker_identity_tag(erase_scope_event(marker.event), marker.reentry)")
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
    assert!(
        program_ref_harnesses
            .contains("usize::from(u16::MAX) / PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE + 1")
    );
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

    let harness = "resolver_registration_key_accepts_full_u16_id_domain";
    assert!(resolver_registration_harnesses.contains(&format!("fn {harness}()")));
    assert!(resolver_registration_harnesses.contains("program::<1>(), u16::MAX"));
    assert!(resolver_registration_harnesses.contains("key.resolver_id() == u16::MAX"));

    let harness = "resolver_initial_storage_is_initialized_and_dispatchable";
    assert!(resolver_registration_harnesses.contains(&format!("fn {harness}()")));
    assert!(resolver_registration_harnesses.contains("MaybeUninit::<Option<ResolverBucketEntry"));
    assert!(resolver_registration_harnesses.contains("bucket.entry_count() == 0"));
    assert!(resolver_registration_harnesses.contains("bucket.get(key).is_none()"));
    assert!(resolver_registration_harnesses.contains("resolver.resolve_decision()"));
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
}
