use super::common::read;

#[test]
fn end_to_end_refinement_stays_proof_indexed_and_runtime_erased() {
    let carrier = read("proofs/lean/Hibana/CarrierProfile.lean");
    let codec = read("proofs/lean/Hibana/CodecEvidence.lean");
    let elastic = read("proofs/lean/Hibana/ElasticErasure.lean");
    let prepared_kernel = read("proofs/lean/Hibana/PreparedKernelRefinement.lean");
    let production_kernel = read("proofs/lean/Hibana/ProductionKernelArtifact.lean");
    let protocol_capability = read("proofs/lean/Hibana/ProtocolCapability.lean");
    let deployment = read("proofs/lean/Hibana/Deployment.lean");
    let end_to_end = read("proofs/lean/Hibana/EndToEndRefinement.lean");
    let production_end_to_end = read("proofs/lean/Hibana/ProductionEndToEnd.lean");
    let public_operation = read("proofs/lean/Hibana/PublicOperationKernel.lean");
    let main_theorems = read("proofs/lean/Hibana/MainTheorems.lean");
    let proof_erasure_gate = read(".github/scripts/check_proof_erasure_surface.sh");
    let unix_carrier_gate = read(".github/scripts/check_unix_carrier_proof.sh");
    let unix_carrier = read("proofs/unix-carrier/src/lib.rs");
    let unix_carrier_conformance = read("proofs/unix-carrier/tests/conformance.rs");
    let final_gate = read(".github/scripts/run_final_form_gates.sh");
    let miri_gate = read(".github/scripts/check_miri.sh");
    let production_exporter =
        read("src/test_support/lean_proof_export/production_kernel_artifact.rs");
    let wire_kani = read("src/transport/wire/kani.rs");
    let public_types = format!(
        "{}\n{}",
        read("src/endpoint/kernel/core/public_operation.rs"),
        read("src/endpoint/kernel/core/public_operation/definition.rs")
    );
    let public_type_tests = read("src/endpoint/kernel/core/public_operation/tests.rs");
    let public_ops = read("src/endpoint/kernel/public_ops.rs");
    let frame = read("src/transport.rs");

    for profile in [
        "| mediated",
        "| authentic",
        "| ordered",
        "| closing",
        "| fair",
    ] {
        assert!(carrier.contains(profile));
    }
    assert!(carrier.contains("def CarrierProfile.Holds"));
    assert!(carrier.contains("CarrierAuthenticity carrier abstraction"));
    assert!(carrier.contains("CarrierOrdering carrier abstraction"));
    assert!(carrier.contains("CarrierClosing carrier abstraction"));
    assert!(carrier.contains("carrier_profile_supports_every_weaker_profile"));
    assert!(carrier.contains("carrier_profile_hierarchy_is_strict"));
    assert!(carrier.contains("CarrierProfile.StrictStep .mediated .authentic"));
    assert!(carrier.contains("CarrierProfile.StrictStep .authentic .ordered"));
    assert!(carrier.contains("CarrierProfile.StrictStep .ordered .closing"));
    assert!(carrier.contains("CarrierProfile.StrictStep .closing .fair"));
    assert!(carrier.contains("mediated_profile_does_not_imply_closing_profile"));

    assert!(codec.contains("structure CodecRefinement"));
    assert!(codec.contains("structure CanonicalWireSchema"));
    assert!(codec.contains("canonicalAcceptanceExact"));
    assert!(codec.contains("canonicalRoundTrip"));
    assert!(codec.contains("canonicalBytes"));
    assert!(codec.contains("CanonicalWireSchemaAgreement"));
    assert!(codec.contains("VerifiedCodecCoverage"));
    assert!(codec.contains("verified_codec_coverage_has_unique_wire_schema"));
    assert!(codec.contains("def fixedWidthVerifiedCodec"));
    assert!(codec.contains("def checkFixedWidthSchemaRegistry"));
    assert!(codec.contains("fixed_width_schema_registry_checker_sound"));
    assert!(codec.contains("fixed_width_codec_registry_agrees"));
    assert!(codec.contains("no theorem equates nominal Rust types across binaries"));

    assert!(elastic.contains("def ElasticOccurrenceKey.erase"));
    assert!(elastic.contains("def ElasticAdmissionHistory.ErasesToTransport"));
    assert!(elastic.contains("history.WellFormed"));
    assert!(elastic.contains("transport.WellFormed"));
    assert!(elastic.contains("history.erasedTrace.map frameLabelOf"));
    assert!(elastic.contains("transport.frames.map TransportFrame.frameLabel"));
    assert!(elastic.contains("elastic_transport_trace_is_exact_erasure"));
    assert!(elastic.contains("elastic_admission_append_commutes_with_trace_erasure"));
    assert!(elastic.contains("elastic_transport_erasure_preserved_by_receive"));
    assert!(elastic.contains("elastic_route_publish_commutes_with_trace_erasure"));
    assert!(elastic.contains("structure ElasticErasureRefinement"));
    assert!(!elastic.contains("wireEpoch"));
    assert!(!elastic.contains("residentEpoch"));

    assert!(prepared_kernel.contains("structure PreparedKernelSemantics"));
    assert!(prepared_kernel.contains("prepare : State -> Input -> Option Prepared"));
    assert!(prepared_kernel.contains("structure PreparedKernelRefinement"));
    assert!(prepared_kernel.contains("preparedCommitExact"));
    assert!(prepared_kernel.contains("rejected_kernel_preparation_is_zero_transition"));
    assert!(prepared_kernel.contains("does not identify its\nkernel with Rust source"));

    for effect in [
        "| protocol",
        "| preview",
        "| dropPending",
        "| requeue",
        "| fault",
        "| ambiguousReceive",
        "| observeCancellation",
    ] {
        assert!(production_kernel.contains(effect));
    }
    for owner in [
        "| endpoint",
        "| cursor",
        "| slab",
        "| waiter",
        "| resolver",
        "| receiveReceipt",
        "| transport",
        "| callbackReentry",
    ] {
        assert!(production_kernel.contains(owner));
    }
    for owner_gate in [
        "public-runtime-owner",
        "direct-recv-owner",
        "resident-sidecar-owner",
        "endpoint-waiter-owner",
        "resolver-identity-owner",
        "receive-frame-receipt-owner",
        "transport-contract-owner",
        "transport-requeue-owner",
    ] {
        assert!(miri_gate.contains(owner_gate));
    }
    for operation in [
        "| send",
        "| recv",
        "| localAction",
        "| resolve",
        "| rejectResolver",
        "| roll",
    ] {
        assert!(production_kernel.contains(operation));
    }
    assert!(production_kernel.contains("structure ProductionKernelArtifact"));
    assert!(production_kernel.contains("protocolCases : List ProductionProtocolCase"));
    assert!(production_kernel.contains("def ProductionKernelArtifact.check"));
    assert!(production_kernel.contains("theorem production_kernel_artifact_sound"));
    assert!(production_kernel.contains("artifact.owners = requiredProductionKernelOwners"));
    assert!(production_kernel.contains("artifact.transitions.map"));
    assert!(production_kernel.contains("effectClassesExact : artifact.transitions.map"));
    assert!(production_kernel.contains("requiredProductionEffectClasses"));
    assert!(production_kernel.contains("requiredProductionProtocolClasses.map some"));
    assert!(production_kernel.contains("production_kernel_artifact_covers_every_protocol_class"));
    assert!(
        production_kernel.contains("accepted_production_protocol_case_refines_prepared_kernel")
    );
    assert!(
        production_kernel
            .contains("accepted_production_kernel_artifact_refines_protocol_case_kernels")
    );
    assert!(production_kernel.contains("structure CrossToolProductionRefinement"));
    assert!(production_kernel.contains("structure ProductionOwnerEvidence"));
    assert!(
        production_kernel
            .contains("ownerEvidence : ProductionOwnerEvidence OwnerVerified artifact")
    );
    assert!(
        production_kernel.contains("theorem cross_tool_refinement_verifies_every_required_owner")
    );
    assert!(
        production_kernel.contains(
            "accepted_production_kernel_artifact_combines_with_external_kernel_refinement"
        )
    );
    assert!(!production_kernel.contains("accepted_production_kernel_artifact_refines_all_kernels"));
    assert!(
        production_kernel
            .contains("theorem accepted_production_kernel_artifact_refines_artifact_kernel")
    );
    assert!(production_kernel.contains("PreparedKernelRefinement artifact.kernel"));
    assert!(
        production_kernel.contains(
            "productionKernel : PreparedKernelRefinement kernel session roleCount choreo"
        )
    );
    assert!(!production_kernel.contains("refines_rust_kernel"));

    assert!(deployment.contains("RoleImagesExact"));
    assert!(deployment.contains("roleImages : List RustDescriptorImage"));
    assert!(deployment.contains("checkRoleImages certificate deployment.roleImages"));
    assert!(deployment.contains("codecs : List VerifiedCodec"));
    assert!(deployment.contains("structure AssumptionIndexedDeploymentContract"));
    assert!(deployment.contains("structure StaticDeploymentCertificate"));
    assert!(deployment.contains("def StaticDeploymentCertificate.check"));
    assert!(deployment.contains("checkStaticDeploymentEntries family certificate.entries"));
    assert!(deployment.contains("inductive StaticDeploymentEntriesExact"));
    assert!(deployment.contains("static_deployment_certificate_sound"));
    assert!(deployment.contains("static_deployment_certificate_covers_family_member"));
    assert!(!deployment.contains("member : VerifiedProtocolMember\n  roleImages"));
    assert!(!deployment.contains("MediatedAsyncDeploymentContract"));
    assert!(!deployment.contains("AffineAsyncDeploymentContract"));
    assert!(deployment.contains("core transport\nheaders remain unchanged"));

    assert!(end_to_end.contains("structure AssumptionIndexedEndToEndRefinement"));
    assert!(end_to_end.contains("translationValidated"));
    assert!(end_to_end.contains("canonicalCodecCoverage"));
    assert!(end_to_end.contains("kernelRefinement"));
    assert!(end_to_end.contains("carrierGuarantees"));
    assert!(end_to_end.contains("elasticTraceRefinement"));
    assert!(
        end_to_end.contains("assumption_indexed_epoch_erased_byte_exact_end_to_end_refinement")
    );
    assert!(end_to_end.contains("end_to_end_fair_run_schedules_recurrently_enabled_operation"));
    assert!(end_to_end.contains("GlobalFairnessAssumptions run"));
    assert!(end_to_end.contains("fairness_schedules_recurrently_enabled_operation"));
    assert!(end_to_end.contains("end_to_end_descriptor_send_erases_elastic_iteration"));
    assert!(end_to_end.contains("end_to_end_transport_trace_is_exact_elastic_erasure"));

    assert!(production_end_to_end.contains("structure AssumptionIndexedProductionRefinement"));
    assert!(production_end_to_end.contains("CrossToolProductionRefinement"));
    assert!(
        production_end_to_end.contains(
            "ownerEvidence : ProductionOwnerEvidence OwnerVerified kernelArtifact kernel"
        )
    );
    assert!(production_end_to_end.contains("staticCertificate.Refines family"));
    assert!(
        production_end_to_end
            .contains("assumption_indexed_static_cross_tool_production_refinement")
    );
    assert!(production_end_to_end.contains("production_refinement_prepared_commit_is_exact"));

    assert!(public_operation.contains("def transitionPublicOperation"));
    assert!(public_operation.contains("structure PublicOperationTransition"));
    assert!(public_operation.contains("inductive PublicOperationEdge"));
    assert!(public_operation.contains("def PublicOperationEdge.expected"));
    assert!(public_operation.contains("def PublicOperationEdge.next"));
    assert!(public_operation.contains("def publicOperationEdges"));
    assert!(public_operation.contains("public_operation_phase_inventory_exact"));
    assert!(public_operation.contains("public_operation_edge_inventory_exact"));
    assert!(public_operation.contains("public_operation_edge_expected_is_live"));
    assert!(public_operation.contains("public_operation_edge_success_is_live"));
    assert!(public_operation.contains("def exactPublicOperationTable"));
    assert!(public_operation.contains("exact_public_operation_table_covers_every_transition"));
    assert!(public_operation.contains("public_operation_table_certificate_sound"));
    assert!(public_operation.contains("poisoned_public_operation_remains_faulted"));
    assert!(public_operation.contains("matching_live_public_operation_transitions_exactly"));
    assert!(public_operation.contains("mismatched_live_public_operation_fails_closed"));
    assert!(public_operation.contains("def clearPublicOperationIfCurrent"));
    assert!(public_operation.contains("def clearPublicOperationTerminal"));
    assert!(public_operation.contains("def faultPublicOperation"));
    assert!(public_operation.contains("conditional_public_operation_clear_is_exact"));
    assert!(public_operation.contains("terminal_public_operation_clear_is_exact"));
    assert!(public_operation.contains("public_operation_fault_is_exact"));
    assert!(main_theorems.contains("import Hibana.PublicOperationKernel"));
    assert!(main_theorems.contains("import Hibana.ProductionKernelArtifact"));
    assert!(main_theorems.contains("import Hibana.ProtocolCapability"));
    assert!(main_theorems.contains("import Hibana.ProductionEndToEnd"));

    for capability in [
        "| communication",
        "| sequencing",
        "| parallelComposition",
        "| intrinsicChoice",
        "| resolvedChoice",
        "| recursion",
    ] {
        assert!(protocol_capability.contains(capability));
    }
    assert!(protocol_capability.contains("structure VerifiedProtocolMember"));
    assert!(protocol_capability.contains("accepted : certificate.check"));
    assert!(protocol_capability.contains("codecCoverage : VerifiedCodecCoverage"));
    assert!(protocol_capability.contains("checkVerifiedProtocolFamilyCapabilities"));
    assert!(protocol_capability.contains("verified_protocol_family_capability_checker_sound"));
    assert!(
        protocol_capability.contains("verified_protocol_family_member_has_execution_guarantees")
    );
    assert!(
        protocol_capability
            .contains("verified_protocol_family_member_has_canonical_codec_coverage")
    );

    assert!(production_exporter.contains("generatedProductionKernelArtifact"));
    assert!(production_exporter.contains("generatedProductionRefinement"));
    assert!(production_exporter.contains(
        "OwnerVerified : Hibana.ProductionKernelOwner ->\n      Hibana.PreparedKernelSemantics -> Prop"
    ));
    assert!(
        production_exporter
            .contains("ownerEvidence : Hibana.ProductionOwnerEvidence OwnerVerified")
    );
    assert!(!production_exporter.contains("OwnerVerified := fun _ => True"));
    assert!(production_exporter.contains("generatedStaticDeploymentCertificate"));
    assert!(production_exporter.contains("generatedMissingStaticDeploymentCertificate"));
    assert!(production_exporter.contains("generatedExtraStaticDeploymentCertificate"));
    assert!(production_exporter.contains("generatedCorruptStaticDeploymentCertificate"));
    assert!(
        production_exporter
            .contains("generated_static_deployment_certificate_refines_exact_family")
    );
    assert!(
        production_exporter.contains("generated_production_protocol_cases_refine_prepared_kernels")
    );
    assert!(production_exporter.contains("roleImages := generatedVerifiedProtocol.descriptors"));
    assert!(production_exporter.contains("generated_production_codec_coverage"));
    assert!(production_exporter.contains("generatedVerifiedProtocolFamily"));
    assert!(
        production_exporter.contains("generated_verified_protocol_family_covers_core_capabilities")
    );
    assert!(production_exporter.contains(
        "transitions=7 operations=6 owners=8 kernel-refinement=external-premise owner-evidence=external-premise codecs=3 family=8 deployments=8 deployment-rejections=3 capabilities=6"
    ));

    for harness in [
        "builtin_u8_i8_codecs_are_exact",
        "builtin_u16_i16_codecs_are_exact",
        "builtin_u32_i32_codecs_are_exact",
        "builtin_u64_i64_codecs_are_exact",
        "builtin_u128_i128_codecs_are_exact",
        "builtin_bool_codec_accepts_exact_canonical_bytes",
        "builtin_unit_codec_is_exact",
        "builtin_borrowed_bytes_roundtrip_is_exact",
        "builtin_borrowed_bytes_truncation_is_exact",
        "builtin_fixed_array_schema_and_bytes_are_exact",
    ] {
        assert!(wire_kani.contains(harness));
    }

    assert!(public_types.contains("struct PublicOpTransition"));
    assert!(public_types.contains("enum PublicOpEdge"));
    assert!(public_types.contains("macro_rules! define_public_operation_kernel"));
    assert_eq!(public_types.matches("macro_rules!").count(), 1);
    assert!(public_types.contains("$($phase),+"));
    assert!(public_types.contains("&[$(Self::$phase),+]"));
    assert!(public_types.contains("$($edge),+"));
    assert!(public_types.contains("&[$(Self::$edge),+]"));
    assert!(public_types.contains("$(Self::$edge => PublicActiveOp::$expected),+"));
    assert!(public_types.contains("$(Self::$edge => PublicActiveOp::$next),+"));
    assert_eq!(
        public_types.matches("const ALL: &'static [Self]").count(),
        2,
        "public phases and edges must each own one generated complete inventory"
    );
    assert!(public_types.contains("edge: PublicOpEdge"));
    assert!(public_types.contains("edge.expected()"));
    assert!(public_types.contains("edge.next()"));
    assert!(
        public_type_tests
            .contains("public_operation_transition_classifier_covers_exact_state_product")
    );
    assert!(public_type_tests.contains("export_public_operation_kernel_for_lean"));
    assert!(public_type_tests.contains("PublicOperationGenerated.lean"));
    assert!(public_type_tests.contains("for &current in PublicActiveOp::ALL"));
    assert!(public_type_tests.contains("for &edge in PublicOpEdge::ALL"));
    assert!(!public_type_tests.contains("const STATES:"));
    assert!(!public_type_tests.contains("const EDGES:"));
    assert!(public_type_tests.contains("set_option maxRecDepth 4096 in"));
    assert!(public_ops.contains("self.public_active_op.transition(edge)"));
    assert!(public_ops.contains("self.public_active_op = transition.phase();"));
    assert!(public_ops.contains("self.public_active_op.clear_if_current(op)"));
    assert!(public_ops.contains("self.public_active_op.clear_terminal()"));
    assert!(public_ops.contains("self.public_active_op.fault()"));
    assert!(!public_ops.contains("fn start_public_op"));
    assert!(!frame.contains("iteration_epoch"));
    assert!(!frame.contains("protocol_image_digest"));
    assert!(!frame.contains("carrier_profile"));
    assert!(proof_erasure_gate.contains("PROOF_METADATA_PATTERN"));
    assert!(proof_erasure_gate.contains("runtime-proof-metadata=0"));
    assert!(proof_erasure_gate.contains("wire-proof-fields=0"));
    assert!(proof_erasure_gate.contains("endpoint-proof-types=0"));
    assert!(proof_erasure_gate.contains("core-header-bytes=8"));
    assert!(proof_erasure_gate.contains("pub struct FrameHeader([u8; 8]);"));
    assert!(proof_erasure_gate.contains("#[cfg(all(test, hibana_repo_tests))]"));
    assert!(final_gate.contains("bash ./.github/scripts/check_proof_erasure_surface.sh"));
    assert!(final_gate.contains("bash ./.github/scripts/check_unix_carrier_proof.sh"));
    assert!(unix_carrier_gate.contains("--manifest-path \"${MANIFEST}\""));
    assert!(unix_carrier_gate.contains("profile=closing"));
    assert!(unix_carrier.contains("UnixDatagram::pair()"));
    assert!(unix_carrier.contains("FrameHeader::from_bytes(frame.header)"));
    assert!(unix_carrier.contains("TransportError::Offline"));
    assert!(unix_carrier.contains(".remove(position)"));
    assert!(
        unix_carrier_conformance
            .contains("exact_frames_cross_two_independent_runtimes_in_fifo_order")
    );
    assert!(
        unix_carrier_conformance
            .contains("logical_close_wakes_a_remote_receive_after_accepted_frames_drain")
    );
    assert!(
        unix_carrier_conformance
            .contains("a_fresh_socket_generation_cannot_observe_an_old_session_frame")
    );
}

#[test]
fn every_public_endpoint_operation_is_owned_by_the_checked_phase_inventory() {
    let allowlist = read(".github/allowlists/endpoint-public-api.txt");
    let public_types = format!(
        "{}\n{}",
        read("src/endpoint/kernel/core/public_operation.rs"),
        read("src/endpoint/kernel/core/public_operation/definition.rs")
    );
    let public_type_tests = read("src/endpoint/kernel/core/public_operation/tests.rs");
    let public_operation = read("proofs/lean/Hibana/PublicOperationKernel.lean");
    let final_gate = read(".github/scripts/run_final_form_gates.sh");
    let public_surface_gate = read(".github/scripts/check_public_surface_budget.sh");

    let methods = allowlist
        .lines()
        .filter_map(|line| {
            let name = line.split_whitespace().next()?;
            name.contains("::").then_some(name)
        })
        .collect::<Vec<_>>();
    assert_eq!(
        methods,
        [
            "Endpoint::send",
            "Endpoint::recv",
            "Endpoint::offer",
            "RouteBranch::label",
            "RouteBranch::recv",
            "RouteBranch::send",
        ],
        "a new public endpoint method must enter the checked operation inventory before release"
    );

    for (method, rust_phase, lean_phase) in [
        ("Endpoint::send", "Send", ".send"),
        ("Endpoint::recv", "Recv", ".recv"),
        ("Endpoint::offer", "Offer", ".offer"),
        ("RouteBranch::recv", "BranchRecv", ".branchRecv"),
        ("RouteBranch::send", "BranchSend", ".branchSend"),
    ] {
        assert!(methods.contains(&method), "missing public method {method}");
        assert!(
            public_types.contains(&format!("    {rust_phase},")),
            "{method} must own a Rust PublicActiveOp phase"
        );
        assert!(
            public_type_tests
                .contains(&format!("PublicActiveOp::{rust_phase} => \"{lean_phase}\"")),
            "{method} must export its exact phase to Lean"
        );
        assert!(
            public_operation.contains(&format!("  | {}", &lean_phase[1..])),
            "{method} must have a checked Lean PublicOperationPhase"
        );
    }

    assert!(
        !public_types.contains("\n    Label,\n"),
        "RouteBranch::label is a read-only observation and must not acquire a progress phase"
    );
    assert!(
        final_gate.contains("bash ./.github/scripts/check_hibana_public_api.sh --surface-only")
            && public_surface_gate.contains("check_public_api_allowlists.py"),
        "the final gate must compare the complete public surface before checking operation phases"
    );
}
