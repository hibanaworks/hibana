use super::common::read;

#[test]
fn lean_proof_gate_is_pinned_fail_closed_and_runtime_free() {
    let manifest = read("Cargo.toml");
    let lakefile = read("proofs/lean/lakefile.toml");
    let lean_toolchain = read("proofs/lean/lean-toolchain");
    let syntax = read("proofs/lean/Hibana/Syntax.lean");
    let event_graph = read("proofs/lean/Hibana/EventGraph.lean");
    let commit = read("proofs/lean/Hibana/Commit.lean");
    let refinement = read("proofs/lean/Hibana/Refinement.lean");
    let layout = read("proofs/lean/Hibana/Layout.lean");
    let progress = read("proofs/lean/Hibana/Progress.lean");
    let generation = read("proofs/lean/Hibana/Generation.lean");
    let allocation = read("proofs/lean/Hibana/Allocation.lean");
    let authority = read("proofs/lean/Hibana/Authority.lean");
    let main_theorems = read("proofs/lean/Hibana/MainTheorems.lean");
    let axiom_audit = read("proofs/lean/Hibana/AxiomAudit.lean");
    let exporter = read("src/test_support/lean_proof_export.rs");
    let projection_exporter = read("src/test_support/lean_proof_export/projection_certificate.rs");
    let runtime_exporter =
        read("src/rendezvous/core/storage_layout/capacity/tests/formal_certificate_export.rs");
    let lease_generation = read("src/rendezvous/core/storage_layout/capacity/endpoint_lease.rs");
    let lease_allocator = read("src/rendezvous/core/endpoint_leases.rs");
    let proof_gate = read(".github/scripts/check_lean_proofs.sh");
    let final_gate = read(".github/scripts/run_final_form_gates.sh");
    let workflow = read(".github/workflows/quality-gates.yml");

    assert_eq!(lean_toolchain.trim(), "leanprover/lean4:v4.30.0");
    assert!(
        lakefile.contains("name = \"Hibana\"")
            && !lakefile.contains("[[require]]")
            && !lakefile.contains("git =")
            && !manifest.contains("proofs/lean"),
        "Lean proofs must remain a Core/Std-only host package outside Cargo and Pico artifacts"
    );
    assert!(
        syntax.contains("inductive RouteArm")
            && syntax.contains("inductive RouteAuthority")
            && syntax.contains("inductive ReentryMode")
            && syntax.contains("| dynamic (resolver : Nat)")
            && syntax.contains("| route (authority : RouteAuthority)")
            && syntax.contains("| roll")
            && exporter.contains("LeanChoreo for g::Roll")
            && exporter.contains("for g::Resolve<g::Route<Left, Right>, RESOLVER_ID>")
            && exporter.contains("Hibana.Choreo.route (.dynamic {RESOLVER_ID})")
            && exporter.contains("generatedRolledChoreo")
            && exporter.contains("&[41, 43, 41, 43]")
            && exporter.contains("&[42, 43, 42, 43]")
            && exporter.contains("generatedRolledLeftTraceRole1")
            && exporter.contains("generatedNestedRolledChoreo")
            && exporter.contains("&[71, 72, 73, 71, 72, 73]")
            && exporter.contains("generatedResolvedLeftTraceRole0")
            && exporter.contains("generatedResolvedRightTraceRole0")
            && exporter.contains("generatedNestedResolvedTraceRole0")
            && exporter.contains("generatedRolledResolvedTraceRole0")
            && exporter.contains("generatedRejectedTraceRole0")
            && exporter.contains("resolver rejection must terminate the production proof trace")
            && exporter.contains("let trace_count = trace_sources.len();")
            && exporter.contains("passed traces={} frames={} projections={} progress={}")
            && !exporter.contains("passed traces=13 frames={}")
            && exporter.contains("env!(\"CARGO_MANIFEST_DIR\")")
            && !exporter.contains("std::env")
            && event_graph.contains("reentry : ReentryMode")
            && !event_graph.contains("insideRoll : Bool")
            && !event_graph.contains("reentrant : Bool")
            && !syntax.contains("| resolve"),
        "route authority must stay canonical while roll and dynamic resolve retain production witnesses"
    );
    assert!(
        commit.contains("def ValidTrace")
            && !commit.contains("TraceCommitsEnabled")
            && commit.contains("eventCandidateBaseState?")
            && commit.contains("eventBaseState?")
            && commit.contains("eventCommitReady")
            && commit.contains("def applyResolver")
            && commit.contains("| resolve (conflict resolver : Nat) (arm : RouteArm)")
            && commit.contains("| reject (conflict resolver : Nat)")
            && !commit.contains(".getD")
            && exporter.contains("Hibana.ValidTrace"),
        "trace soundness must separate candidate frontiers from explicit commit and resolver authority"
    );
    assert!(
        refinement.contains("structure ProjectionCertificate")
            && refinement.contains("def EventGraph.projectionShape")
            && refinement.contains("route.leftEvents.isEmpty && route.rightEvents.isEmpty")
            && !refinement.contains("laneCount")
            && projection_exporter.contains("production.event_program.program_ref()")
            && projection_exporter.contains("route_resolver_scope_at_row")
            && projection_exporter.contains("let Some((conflict, resolver_id)) = authority_row")
            && projection_exporter.contains("progress_certificate_source")
            && !exporter.contains("4096")
            && exporter.contains("generatedProjectionRole0")
            && exporter.contains("generatedRolledResolvedProjectionRole0")
            && exporter.contains("generatedNestedResolvedProgressRole0"),
        "Rust-to-Lean refinement must compare production topology while leaving physical lane packing to executable closure checks"
    );
    assert!(
        layout.contains("structure SlabLayoutCertificate")
            && layout.contains("region.aligned certificate.base")
            && layout.contains("def PowerOfTwoAlignment")
            && layout.contains("value &&& (value - 1)")
            && layout.contains("RegionsPairwiseDisjoint")
            && progress.contains("structure ProgressCertificate")
            && progress.contains("inductive CompactReachable")
            && progress.contains("def compactSuccessors")
            && progress.contains("resolverRejectAvailable")
            && progress.contains("def progressStateBound")
            && progress.contains("2 ^ graph.events.length * 3 ^ graph.conflictCount")
            && runtime_exporter.contains("rv.live_sidecars()")
            && runtime_exporter.contains("layout_certificate_source")
            && runtime_exporter
                .contains("regions={region_count} poison=1 generation=1 atomic-failures=4"),
        "layout and logical-progress certificates must be generated from live production owners and checked as closed finite models"
    );
    assert!(
        generation.contains("def nextLeaseGeneration?")
            && generation.contains("def applySessionGenerationAction")
            && generation.contains("def runSessionGenerationTrace")
            && generation.contains("def SessionGenerationState.publicationPermitted")
            && generation.contains("theorem poisoned_generation_publish_rejected")
            && generation.contains("theorem publication_permitted_implies_live")
            && generation.contains("poisoned_generation_trace_never_revives")
            && lease_generation.contains("next_endpoint_lease_generation(&self) -> Option<u32>")
            && lease_generation.contains("checked_add(1)")
            && !lease_generation.contains("wrapping_add(1)")
            && lease_allocator.contains("self.endpoint_lease_generation.get() == u32::MAX")
            && lease_allocator.contains(".ok_or(ResourceScope::EndpointLease)?")
            && runtime_exporter
                .contains("production lease generation must fail closed at exhaustion")
            && runtime_exporter.contains("a poisoned generation must reject new lane attachment")
            && allocation.contains("def prepareLeaseAllocation")
            && allocation.contains("def commitLeaseAllocation")
            && allocation.contains("| initializationRejected")
            && allocation.contains("if publishReady then")
            && allocation.contains("theorem failed_lease_allocation_preserves_state")
            && allocation.contains("theorem poisoned_generation_aborts_lease_publication")
            && allocation.contains("structure LeaseAllocationFailureCertificate")
            && allocation.contains("structure LeaseAllocationAbortCertificate")
            && allocation.contains("PreservesAuthorityAndCapacity")
            && allocation.contains("activeLaneAttachments : Nat")
            && allocation.contains("associationWitnesses : List LeaseAssociationWitness")
            && allocation.contains("assocBytes : Nat")
            && allocation.contains("routeBytes : Nat")
            && allocation.contains("resolverBytes : Nat")
            && runtime_exporter.contains("generatedInitialAllocationFailure")
            && runtime_exporter.contains("generatedGrowthAllocationFailure")
            && runtime_exporter.contains("generatedAbortedAllocation")
            && runtime_exporter.contains("generatedCompactingAbort")
            && runtime_exporter.contains("rv.active_lane_attachment_count()")
            && runtime_exporter.contains("rv.has_lane_attachment(*sid, *lane)")
            && runtime_exporter.contains("rv.assoc_storage.get()")
            && runtime_exporter.contains("rv.route_storage.get()")
            && runtime_exporter.contains("rv.resolver_storage_sidecar()"),
        "lease generations must not wrap and poisoned session generations must remain fail-closed until retirement"
    );
    for theorem in [
        "theorem artifact_checker_sound",
        "theorem trace_checker_sound",
        "theorem commit_marks_event_done",
        "theorem selected_arm_unique",
        "theorem reset_roll_preserves_outside_done",
        "theorem reset_roll_clears_done",
        "theorem reset_roll_preserves_outside_selection",
        "theorem reset_roll_clears_selection",
        "theorem reset_route_arm_preserves_selection",
        "theorem reset_route_arm_preserves_outside_done",
        "theorem reset_route_arm_clears_done",
        "theorem local_action_send_recv_duality",
        "theorem local_action_uninvolved_projects_none",
        "theorem local_action_self_send_projects_local",
        "theorem local_action_preserves_label",
        "theorem commit_exact_successor",
        "theorem resolver_reject_never_selects",
        "theorem resolver_select_never_rejects",
        "theorem resolver_selection_exact_authority",
        "theorem selected_route_is_not_resolvable",
        "theorem unresolved_dynamic_route_cannot_commit",
        "theorem unresolved_dynamic_commit_rejected",
        "theorem wrong_resolver_id_rejected",
        "theorem resolver_reuse_without_reset_rejected",
        "theorem resolver_reject_is_terminal",
        "theorem program_trace_checker_sound",
        "theorem projection_certificate_sound",
        "theorem slab_layout_certificate_sound",
        "theorem logical_progress_checker_sound",
        "theorem progress_certificate_sound",
        "theorem reachable_state_has_logical_progress",
        "theorem next_lease_generation_strictly_increases",
        "theorem next_lease_generation_stays_nonzero",
        "theorem next_lease_generation_stays_in_domain",
        "theorem max_lease_generation_is_exhausted",
        "theorem session_generation_run_sound",
        "theorem session_generation_trace_checker_sound",
        "theorem poisoned_generation_step_never_revives",
        "theorem poisoned_generation_attach_rejected",
        "theorem poisoned_generation_publish_rejected",
        "theorem publication_permitted_implies_live",
        "theorem poisoned_generation_trace_never_revives",
        "theorem failed_lease_allocation_preserves_state",
        "theorem poisoned_generation_aborts_lease_publication",
        "theorem successful_lease_allocation_commits_exact_plan",
        "theorem prepared_lease_generation_strictly_increases",
        "theorem prepared_lease_capacity_never_shrinks",
        "theorem lease_allocation_failure_certificate_sound",
        "theorem lease_allocation_abort_certificate_sound",
        "theorem dynamic_resolver_site_key_injective",
        "theorem resolver_registration_key_is_program_and_id",
        "theorem distinct_program_images_have_distinct_registration_keys",
        "theorem scope_topology_difference_has_distinct_registration_keys",
    ] {
        assert!(
            syntax.contains(theorem)
                || event_graph.contains(theorem)
                || commit.contains(theorem)
                || refinement.contains(theorem)
                || layout.contains(theorem)
                || progress.contains(theorem)
                || generation.contains(theorem)
                || allocation.contains(theorem)
                || authority.contains(theorem)
                || main_theorems.contains(theorem),
            "Lean proof kernel missing {theorem}"
        );
    }
    for identity_field in [
        "roleCount : Nat",
        "atomCount : Nat",
        "routeResolverCount : Nat",
        "scopeMarkerCount : Nat",
        "blob : List Nat",
    ] {
        assert!(
            authority.contains(identity_field),
            "Lean program identity must retain exact Rust image field: {identity_field}"
        );
    }
    assert!(
        proof_gate.contains("forbids sorry, admit, and custom axioms")
            && proof_gate.contains("must remain Core/Std-only")
            && proof_gate.contains("Lean proof gate axiom set changed")
            && proof_gate.contains("depends on axioms: [propext, Quot.sound]")
            && proof_gate.contains("Classical.choice")
            && axiom_audit.contains("#print axioms Hibana.program_trace_checker_sound")
            && axiom_audit.contains("#print axioms Hibana.resolver_selection_exact_authority")
            && axiom_audit.contains("#print axioms Hibana.unresolved_dynamic_route_cannot_commit")
            && axiom_audit.contains("#print axioms Hibana.resolver_reject_is_terminal")
            && axiom_audit.contains("#print axioms Hibana.projection_certificate_sound")
            && axiom_audit.contains("#print axioms Hibana.slab_layout_certificate_sound")
            && axiom_audit.contains("#print axioms Hibana.reachable_state_has_logical_progress")
            && axiom_audit.contains("#print axioms Hibana.poisoned_generation_trace_never_revives")
            && axiom_audit.contains("#print axioms Hibana.poisoned_generation_publish_rejected")
            && axiom_audit.contains("#print axioms Hibana.publication_permitted_implies_live")
            && axiom_audit
                .contains("#print axioms Hibana.lease_allocation_failure_certificate_sound")
            && axiom_audit
                .contains("#print axioms Hibana.poisoned_generation_aborts_lease_publication")
            && axiom_audit
                .contains("#print axioms Hibana.lease_allocation_abort_certificate_sound")
            && axiom_audit.contains("#print axioms Hibana.dynamic_resolver_site_key_injective")
            && axiom_audit
                .contains("#print axioms Hibana.resolver_registration_key_is_program_and_id")
            && axiom_audit.contains(
                "#print axioms Hibana.distinct_program_images_have_distinct_registration_keys"
            )
            && axiom_audit.contains(
                "#print axioms Hibana.scope_topology_difference_has_distinct_registration_keys"
            )
            && commit.contains("rollReentryState?")
            && commit.contains("routeReentryState?")
            && proof_gate.contains("traces=13 frames=55 projections=7 progress=4")
            && proof_gate.contains("regions=5 poison=1 generation=1 atomic-failures=4")
            && proof_gate.contains("export_production_trace_for_lean")
            && proof_gate.contains("export_runtime_certificates_for_lean")
            && proof_gate.contains("rm -f \"${GENERATED}\" \"${RUNTIME_GENERATED}\"")
            && proof_gate.contains("[[ ! -s \"${GENERATED}\" ]]")
            && proof_gate.contains("[[ ! -s \"${RUNTIME_GENERATED}\" ]]")
            && final_gate.contains("bash ./.github/scripts/check_lean_proofs.sh")
            && workflow.contains("ELAN_VERSION=\"4.2.3\"")
            && workflow.contains("elan-aarch64-unknown-linux-gnu.tar.gz")
            && workflow
                .contains("cb69af0803b04157bc30201c29c12fca882bb3ad8b43476b8d2d3064810bc3ac")
            && workflow.contains("sha256sum --check -")
            && workflow.contains("leanprover/elan/releases/download/v${ELAN_VERSION}"),
        "final-form CI must execute the pinned nonempty Lean proof and production-trace gate"
    );
}
