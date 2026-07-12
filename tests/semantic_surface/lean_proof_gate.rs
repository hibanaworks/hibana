use super::common::read;

#[test]
fn lean_proof_gate_is_pinned_fail_closed_and_runtime_free() {
    let manifest = read("Cargo.toml");
    let lakefile = read("proofs/lean/lakefile.toml");
    let lean_toolchain = read("proofs/lean/lean-toolchain");
    let syntax = read("proofs/lean/Hibana/Syntax.lean");
    let global_syntax = read("proofs/lean/Hibana/GlobalSyntax.lean");
    let event_graph = read("proofs/lean/Hibana/EventGraph.lean");
    let commit = read("proofs/lean/Hibana/Commit.lean");
    let runtime_monitor = read("proofs/lean/Hibana/RuntimeMonitor.lean");
    let refinement = read("proofs/lean/Hibana/Refinement.lean");
    let layout = read("proofs/lean/Hibana/Layout.lean");
    let progress = read("proofs/lean/Hibana/Progress.lean");
    let generation = read("proofs/lean/Hibana/Generation.lean");
    let global_semantics = read("proofs/lean/Hibana/GlobalSemantics.lean");
    let global_message_transitions = read("proofs/lean/Hibana/GlobalMessageTransitions.lean");
    let global_fidelity = read("proofs/lean/Hibana/GlobalFidelity.lean");
    let descriptor_image = read("proofs/lean/Hibana/DescriptorImage.lean");
    let descriptor_topology = read("proofs/lean/Hibana/DescriptorTopology.lean");
    let descriptor_refinement_leaf = read("proofs/lean/Hibana/DescriptorRefinement.lean");
    let descriptor_refinement = [
        descriptor_image.as_str(),
        descriptor_topology.as_str(),
        descriptor_refinement_leaf.as_str(),
    ]
    .join("\n");
    let cancellation = read("proofs/lean/Hibana/Cancellation.lean");
    let transport_contract = read("proofs/lean/Hibana/TransportContract.lean");
    let async_cancellation = read("proofs/lean/Hibana/AsyncCancellation.lean");
    let async_cancellation_termination =
        read("proofs/lean/Hibana/AsyncCancellationTermination.lean");
    let global_progress = read("proofs/lean/Hibana/GlobalProgress.lean");
    let static_projectability = read("proofs/lean/Hibana/StaticProjectability.lean");
    let global_coherence = read("proofs/lean/Hibana/GlobalCoherence.lean");
    let distributed_semantics = read("proofs/lean/Hibana/DistributedSemantics.lean");
    let roll_freshness = read("proofs/lean/Hibana/RollFreshness.lean");
    let session_composition = read("proofs/lean/Hibana/SessionComposition.lean");
    let session_lifecycle = read("proofs/lean/Hibana/SessionLifecycle.lean");
    let runtime_refinement = read("proofs/lean/Hibana/RuntimeRefinement.lean");
    let protocol_artifact = read("proofs/lean/Hibana/ProtocolArtifact.lean");
    let allocation = read("proofs/lean/Hibana/Allocation.lean");
    let authority = read("proofs/lean/Hibana/Authority.lean");
    let main_theorems = read("proofs/lean/Hibana/MainTheorems.lean");
    let axiom_audit = read("proofs/lean/Hibana/AxiomAudit.lean");
    let exporter_root = read("src/test_support/lean_proof_export.rs");
    let choreo_exporter = read("src/test_support/lean_proof_export/choreo_source.rs");
    let cyclic_roll_exporter =
        read("src/test_support/lean_proof_export/cyclic_roll_certificate.rs");
    let exporter = [
        exporter_root.as_str(),
        choreo_exporter.as_str(),
        cyclic_roll_exporter.as_str(),
    ]
    .join("\n");
    let projection_exporter = read("src/test_support/lean_proof_export/projection_certificate.rs");
    let runtime_exporter =
        read("src/rendezvous/core/storage_layout/capacity/tests/formal_certificate_export.rs");
    let lease_generation = read("src/rendezvous/core/storage_layout/capacity/endpoint_lease.rs");
    let lease_allocator = read("src/rendezvous/core/endpoint_leases.rs");
    let proof_gate = read(".github/scripts/check_lean_proofs.sh");
    let final_gate = read(".github/scripts/run_final_form_gates.sh");
    let workflow = read(".github/workflows/quality-gates.yml");
    let lean_sources = [
        syntax.as_str(),
        global_syntax.as_str(),
        event_graph.as_str(),
        commit.as_str(),
        runtime_monitor.as_str(),
        refinement.as_str(),
        layout.as_str(),
        progress.as_str(),
        generation.as_str(),
        global_semantics.as_str(),
        global_message_transitions.as_str(),
        global_fidelity.as_str(),
        descriptor_refinement.as_str(),
        cancellation.as_str(),
        transport_contract.as_str(),
        async_cancellation.as_str(),
        async_cancellation_termination.as_str(),
        global_progress.as_str(),
        static_projectability.as_str(),
        global_coherence.as_str(),
        distributed_semantics.as_str(),
        roll_freshness.as_str(),
        session_composition.as_str(),
        session_lifecycle.as_str(),
        runtime_refinement.as_str(),
        protocol_artifact.as_str(),
        allocation.as_str(),
        authority.as_str(),
        main_theorems.as_str(),
        axiom_audit.as_str(),
    ]
    .join("\n");

    assert_eq!(lean_toolchain.trim(), "leanprover/lean4:v4.30.0");
    assert!(
        descriptor_topology.contains("import Hibana.DescriptorImage")
            && descriptor_refinement_leaf.contains("import Hibana.DescriptorTopology"),
        "descriptor refinement must remain split into byte-image, topology, and certificate layers"
    );
    assert!(
        !lean_sources.lines().any(|line| {
            let trimmed = line.trim_start();
            trimmed.starts_with("@[") && trimmed.contains("] theorem ")
        }),
        "exported Lean theorem attributes must use a separate line so the axiom inventory cannot skip them"
    );
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
            && syntax.contains("structure MessageKey")
            && syntax.contains("schema : Nat")
            && syntax.contains("if schema = 0 then some (.local label schema) else none")
            && syntax.contains("theorem local_action_payload_self_send_rejected")
            && syntax.contains("| send (sender receiver label schema : Nat)")
            && exporter.contains("LeanChoreo for g::Roll")
            && exporter.contains("for g::Resolve<g::Route<Left, Right>, RESOLVER_ID>")
            && exporter.contains("Hibana.Choreo.route (.dynamic {RESOLVER_ID})")
            && exporter.contains("crate::global::payload_schema::<M>()")
            && exporter.contains("g::Msg<11, u32>")
            && exporter.contains("g::Msg<12, i32>")
            && exporter.contains("type ResolvedLeft = g::Send<0, 1, g::Msg<51, u32>>;")
            && exporter.contains("type ResolvedRight = g::Send<0, 1, g::Msg<51, i32>>;")
            && exporter.contains("schema := {}")
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
            && exporter.contains("generatedCyclicRollTraceRole2")
            && exporter.contains("generatedCyclicRollVerifiedProtocol")
            && exporter.contains("resolver rejection must terminate the production proof trace")
            && exporter.contains("let trace_count = trace_sources.len();")
            && exporter.contains(
                "passed traces={} frames={} projections={} exact-descriptors={} progress={} projectability={} verified-protocols={}"
            )
            && exporter.contains("projectability_certificate_source")
            && exporter.contains("generatedProjectability")
            && exporter.contains("verified_protocol_certificate_source")
            && exporter.contains("generatedVerifiedProtocol")
            && !exporter.contains("passed traces=14 frames={}")
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
            && commit.contains("def enabledKeys")
            && commit.contains("def matchingEvent?")
            && commit.contains("def commitKey")
            && !commit.contains("enabledLabels")
            && !commit.contains("commitLabel")
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
            && projection_exporter.contains("key.schema")
            && projection_exporter.contains("payload_schema")
            && projection_exporter.contains("progress_certificate_source")
            && projection_exporter.contains("state.decode?")
            && !projection_exporter.contains("toGlobalConfig")
            && projection_exporter.contains("ExactDescriptorCertificate")
            && projection_exporter.contains("program.proof_blob_len()")
            && projection_exporter.contains("descriptor.proof_blob_len()")
            && projection_exporter.contains("exact_descriptor_certificate_sound")
            && projection_exporter.contains("format!(\"{exact_name}LaneMismatch\")")
            && projection_exporter.contains("format!(\"{exact_name}GlobalActionMismatch\")")
            && projection_exporter.contains("format!(\"{exact_name}RouteMismatch\")")
            && projection_exporter.contains("format!(\"{exact_name}ResolverMismatch\")")
            && projection_exporter.contains("format!(\"{exact_name}RollMismatch\")")
            && projection_exporter.contains("format!(\"{exact_name}LaneBitMismatch\")")
            && projection_exporter.contains("format!(\"{exact_name}RouteCommitMismatch\")")
            && projection_exporter.contains("format!(\"{exact_name}LogicalLaneCountMismatch\")")
            && projection_exporter.contains(".image.roleBytes.set")
            && projection_exporter.contains(".check = false")
            && !exporter.contains("4096")
            && exporter.contains("generatedProjectionRole0")
            && exporter.contains("generatedRolledResolvedProjectionRole0")
            && exporter.contains("generatedNestedResolvedProgressRole0"),
        "Rust-to-Lean refinement must compare canonical program bytes, role bytes, and resident metadata"
    );
    assert!(
        descriptor_refinement.contains("structure RustDescriptorImage")
            && descriptor_refinement.contains("productionProgramAtomStride : Nat := 11")
            && descriptor_refinement.contains("productionRoleEventStride : Nat := 10")
            && descriptor_refinement.contains("def RustDescriptorImage.decodeActions?")
            && descriptor_refinement.contains("def RustDescriptorImage.decodeGlobalEvents?")
            && descriptor_refinement.contains("theorem canonical_program_source_lane_span")
            && descriptor_refinement.contains("theorem canonical_program_atoms_global_events")
            && descriptor_refinement
                .contains("theorem accepted_descriptor_global_events_bind_canonical_lanes")
            && descriptor_refinement.contains("def RustDescriptorImage.decodeRouteAuthorities?")
            && descriptor_refinement.contains("def Choreo.canonicalProgramAtoms")
            && descriptor_refinement.contains("def Choreo.canonicalRoleDependencies")
            && descriptor_refinement.contains("def Choreo.canonicalRoleLaneBits")
            && descriptor_refinement.contains("def Choreo.canonicalRoleRouteCommitRows")
            && descriptor_refinement.contains("def Choreo.canonicalRoleRollRows")
            && descriptor_refinement.contains("ResidentMetadataMatches")
            && descriptor_refinement.contains("RoleEventImageMatches")
            && descriptor_refinement.contains("RoleRouteImageMatches")
            && descriptor_refinement.contains("if lane = atom.lane then")
            && descriptor_refinement.contains("theorem decoded_event_action_binds_exact_lane")
            && descriptor_refinement.contains("def RustDescriptorImage.decodeProjectionShape?")
            && descriptor_refinement.contains("image.programBytes.length = image.programBlobLen")
            && descriptor_refinement.contains("image.roleBytes.length = image.roleBlobLen")
            && descriptor_refinement
                .contains("theorem accepted_descriptor_actions_match_projection")
            && descriptor_refinement.contains("theorem exact_descriptor_certificate_sound")
            && descriptor_refinement.contains("theorem descriptor_cursor_step_simulates_monitor")
            && descriptor_refinement
                .contains("theorem accepted_descriptor_cursor_step_refines_projection")
            && descriptor_refinement.contains("def applyDescriptorCursorStep")
            && !descriptor_refinement.contains("| none => True")
            && global_semantics.contains("routeSelection : Nat -> Option RouteArm")
            && global_semantics.contains("structure AdmittedMessage")
            && !lean_sources.contains("WireMessage")
            && global_syntax.contains("lane : Nat")
            && global_syntax.contains("def Choreo.laneSpan")
            && global_syntax.contains("def Choreo.globalEventsFrom")
            && global_syntax.contains("right.globalEventsFrom (laneBase + left.laneSpan)")
            && global_syntax.contains("theorem global_events_from_lane_bounds")
            && global_syntax.contains("theorem parallel_global_event_lanes_are_disjoint")
            && global_syntax.contains("def Choreo.globalEventConflictsFrom")
            && global_syntax.contains("theorem global_event_conflicts_from_length")
            && global_semantics.contains("queue : Nat -> Option AdmittedMessage")
            && global_semantics.contains("lane := event.lane")
            && global_semantics.contains("def GlobalConfig.queueForChannel")
            && global_semantics.contains("structure ParticipantResourceOwners")
            && global_semantics.contains("transports : List Nat")
            && global_semantics.contains("waiters : List Nat")
            && global_semantics.contains("leases : List Nat")
            && global_semantics.contains("def mergeRouteSelection?")
            && global_semantics.contains("theorem route_selection_rejects_opposite_arm")
            && global_semantics.contains("def GlobalConfig.resolveStep?")
            && global_semantics.contains("def GlobalConfig.rejectResolverStep?")
            && global_semantics.contains("| rejectResolver (role conflict resolver : Nat)")
            && global_semantics.contains("resolve_step_publishes_one_arm_and_rejects_reselection")
            && global_fidelity.contains("session_fidelity_queue_slot_is_exact")
            && global_fidelity.contains("def RouteSelectionFidelity")
            && global_fidelity.contains("global_step_preserves_route_selection_fidelity")
            && global_semantics.contains("send_step_rejects_occupied_queue")
            && global_semantics.contains("recv_step_rejects_wrong_message")
            && global_message_transitions.contains("theorem send_step_enqueues_exact_message")
            && global_message_transitions.contains("theorem recv_step_consumes_unique_message")
            && global_fidelity.contains("global_step_preserves_global_invariant")
            && global_fidelity.contains("global_reachable_preserves_global_invariant")
            && global_fidelity.contains("initial_global_reachable_preserves_global_invariant")
            && global_semantics.contains("| roll (rollId : Nat)")
            && global_semantics.contains("def GlobalConfig.rollStep?")
            && global_semantics.contains("roll_step_clears_owned_queue")
            && !global_fidelity.contains("every_global_config_has_session_fidelity"),
        "exact descriptor actions and all-role route authority must refine one monitor transition"
    );
    assert!(
        cancellation.contains("inductive CancellationReachable")
            && cancellation.contains("begin_cancellation_quarantines_all_messages")
            && cancellation.contains("begin_cancellation_clears_queue")
            && cancellation.contains("begun_cancellation_has_finite_retirement_trace")
            && cancellation.contains("begun_cancellation_reaches_resource_retirement")
            && cancellation.contains("config.resources = .empty")
            && cancellation.contains("role ∉ next.resources.transports")
            && cancellation.contains("retired.resourcesRetired")
            && cancellation.contains("begin_cancellation_is_well_formed")
            && cancellation.contains("cancellation_observation_preserves_well_formed")
            && cancellation.contains("cancellation_observation_is_affine")
            && cancellation.contains("reject_resolver_step_begins_cancellation")
            && transport_contract.contains("def TransportState.pollReceive")
            && transport_contract.contains("well_formed_transport_has_no_replay")
            && transport_contract.contains("consecutive_transport_receives_are_exactly_once")
            && transport_contract.contains("def TransportState.abortPeer")
            && transport_contract.contains("abort_peer_is_immediately_observable")
            && transport_contract.contains("abort_peer_is_well_formed")
            && transport_contract.contains("generation : Nat")
            && !transport_contract.contains("connection : Nat")
            && transport_contract.contains("frameLabel : Fin 256")
            && !transport_contract.contains("globalId : Nat")
            && !transport_contract.contains("epoch : Nat")
            && !transport_contract.contains("schema : Nat")
            && transport_contract.contains("transport_receive_preserves_well_formed")
            && transport_contract.contains("peer_close_is_observable_after_fifo_drain")
            && global_progress.contains("def Choreo.GuardedRolls")
            && global_progress.contains("def GlobalConfig.topLevelRollStep?")
            && global_progress.contains("top_level_roll_step_uses_global_roll_transition")
            && global_progress.contains("top_level_roll_step_resets_iteration_atomically")
            && global_progress.contains("top_level_roll_step_has_no_owned_stale_queue")
            && global_progress.contains("completed_roll_has_global_successor")
            && global_progress.contains("event_excluded_of_selected_opposite")
            && global_progress.contains("roll_ready_of_consumed_or_excluded")
            && global_progress.contains("def GlobalConfig.unfinishedEvent?")
            && global_progress.contains("find_unfinished_event_sound")
            && !global_progress.contains("(classified : config.HasUnfinishedEvent")
            && global_progress.contains("structure GlobalRun")
            && global_progress.contains("def GlobalConfig.candidateOperations")
            && global_progress.contains("def GlobalConfig.executableOperation?")
            && global_progress.contains("theorem executable_operation_sound")
            && global_progress.contains(".rejectResolver role route.conflict resolver")
            && global_progress.contains("structure GlobalFairnessAssumptions")
            && global_progress.contains("recurrentlyEnabledOperationRuns")
            && global_progress.contains("fairness_schedules_recurrently_enabled_operation")
            && !global_progress.contains("queuedEventuallyReceived")
            && !global_progress.contains("fairness_schedules_each_enabled_roll"),
        "global cancellation, transport, fairness, and roll obligations must remain explicit"
    );
    assert!(
        async_cancellation.contains("inductive AsyncCancellationStep")
            && async_cancellation.contains("structure AcceptedChannelQueue")
            && async_cancellation.contains("channel : TransportChannel")
            && !async_cancellation.contains("structure AcceptedRoleQueue")
            && async_cancellation.contains("AcceptedChannelAuthority")
            && async_cancellation.contains("transport_snapshot_has_accepted_channel_authority")
            && async_cancellation.contains("| drain")
            && async_cancellation.contains("| close")
            && async_cancellation.contains("roleTransportRetired")
            && async_cancellation.contains("async_close_and_drain_commute")
            && async_cancellation_termination.contains("retirement_ready_reaches_full_retirement")
            && async_cancellation_termination.contains("firstTransportRetirementAction")
            && global_coherence.contains("structure CompactGlobalState")
            && global_coherence.contains("def CompactGlobalState.decode?")
            && global_coherence.contains("if accepted : state.canonical roleCount choreo = true")
            && global_coherence.contains(
                "if programAccepted : checkGlobalProgramWellFormed roleCount choreo = true"
            )
            && global_coherence.contains("compact_global_decode_acceptance_implies_canonical")
            && global_coherence
                .contains("compact_global_decode_acceptance_implies_program_checker")
            && global_coherence.contains("compact_global_decode_rejects_noncanonical")
            && global_coherence.contains("compact_global_decode_rejects_invalid_program")
            && !global_coherence.contains("toGlobalConfig")
            && !global_coherence.contains("| none => .ready")
            && !global_coherence.contains("| none => .initial")
            && global_coherence.contains("structure ProjectabilityCertificate")
            && global_coherence.contains("buildProjectabilityCertificate")
            && global_coherence.contains("globalProgressExplorationFuel")
            && !exporter.contains("65536")
            && global_coherence.contains("checkStaticProjectability roleCount choreo")
            && global_coherence
                .contains("projectability_certificate_removes_global_progress_premise")
            && global_coherence
                .contains("projectability_certificate_all_roles_have_well_formed_projection")
            && global_coherence
                .contains("projectability_certificate_establishes_semantic_unstuckness")
            && global_coherence.contains("def SemanticallyUnstuck")
            && static_projectability.contains("inductive StaticEndpointSelector")
            && static_projectability.contains("| outbound (role label schema : Nat)")
            && static_projectability.contains("structure StaticInboundEvidence")
            && static_projectability.contains("checkInboundOccurrenceIdentity")
            && static_projectability.contains("inbound_occurrence_identity_checker_sound")
            && static_projectability.contains("def Choreo.ReceiveLaneCausalSafety")
            && static_projectability.contains("def Choreo.checkReceiveLaneCausality")
            && static_projectability.contains("receive_lane_causality_checker_sound")
            && static_projectability
                .contains("receive_lane_sender_change_requires_exclusion_or_causal_handoff")
            && static_projectability.contains("def Choreo.rollUnfoldedOccurrences")
            && static_projectability.contains("def Choreo.RollBodyReceiveLaneCausalSafety")
            && static_projectability.contains("def Choreo.RollReceiveLaneCausalSafety")
            && static_projectability.contains("def Choreo.checkRollReceiveLaneCausality")
            && static_projectability.contains("roll_receive_lane_causality_checker_sound")
            && static_projectability.contains("roll_body_occurrences_cross_iteration_safe")
            && static_projectability.contains("roll_reentry_sender_change_requires_causal_handoff")
            && static_projectability.contains("ConflictListsMutuallyExclusive")
            && static_projectability.contains("ParallelListsIndependent")
            && static_projectability.contains("receivePrecedesLaterSend")
            && static_projectability.contains("structure TransportAdmission")
            && static_projectability.contains("def Choreo.transportAdmission?")
            && static_projectability.contains("def GlobalConfig.admitTransportFrame?")
            && static_projectability.contains("transport_admission_is_unique")
            && static_projectability.contains("transport_admission_checker_sound")
            && static_projectability.contains("global_transport_admission_checker_sound")
            && static_projectability.contains("transport_admission_depends_only_on_observation")
            && static_projectability
                .contains("observed_transport_admission_ignores_carrier_history")
            && static_projectability
                .contains("transport_admission_binds_exact_descriptor_occurrence")
            && static_projectability
                .contains("transport_admission_produces_exact_admitted_message")
            && static_projectability.contains("checkParallelEndpointLinearityFrom")
            && static_projectability.contains("checkRouteKnowledgeFrom")
            && static_projectability.contains("checkRollReentryLinearityFrom")
            && static_projectability.contains("static_projectability_checker_sound")
            && static_projectability.contains("| [], _ :: _ | _ :: _, [] => false")
            && static_projectability
                .contains("Shared local output is not intrinsic branch evidence")
            && static_projectability.contains("observer_absence_is_not_branch_evidence")
            && static_projectability.contains("observer_outbound_heads_are_not_mergeable")
            && static_projectability.contains("Roll reentry is not branch authority")
            && !static_projectability.contains("reentry == .rolled ||")
            && distributed_semantics.contains("structure DistributedConfig")
            && distributed_semantics.contains("structure LocalConfig")
            && distributed_semantics.contains("structure LocalQueuedFrame")
            && distributed_semantics.contains("localQueue?")
            && distributed_semantics.contains("message.globalId = globalId")
            && distributed_semantics.contains("message.lane = globalEvent.lane")
            && distributed_semantics.contains("wrongLaneLocalQueueWitness")
            && distributed_semantics.contains("local_queued_frame_has_canonical_lane")
            && distributed_semantics.contains("operationOwner?")
            && distributed_semantics.contains("fromGlobalSuccessor")
            && distributed_semantics.contains("inductive DistributedCommitAuthority")
            && distributed_semantics.contains("observeQueuedRouteEvidence?")
            && !distributed_semantics.contains("observePublishedSelection?")
            && distributed_semantics.contains("routeEvidenceCompatible")
            && distributed_semantics.contains("localCommitAuthority?")
            && distributed_semantics.contains("distributed_protocol_step_is_role_owned")
            && distributed_semantics
                .contains("route_selection_observation_has_exact_queued_evidence")
            && distributed_semantics.contains("role_selection_observation_is_global_stutter")
            && distributed_semantics.contains("distributed_step_weakly_simulates_global")
            && roll_freshness.contains("roll_step_with_drained_transport_has_fresh_next_frame")
            && roll_freshness
                .contains("roll_step_with_admitted_transport_frame_is_fresh_occurrence")
            && session_composition.contains("structure SessionPool")
            && session_composition.contains("runtime_fault_classification_is_total")
            && session_composition.contains("session_pool_step_preserves_other_session_state")
            && session_composition.contains("session_pool_step_preserves_well_formed")
            && session_lifecycle.contains("def SessionPool.attachInitial?")
            && session_lifecycle.contains("def SessionPool.retire?")
            && session_lifecycle.contains("attach_initial_preserves_invariant")
            && session_lifecycle.contains("retire_session_preserves_invariant")
            && session_lifecycle.contains("retired_session_allows_fresh_attach")
            && session_lifecycle.contains("session_pool_step_is_target_local")
            && session_lifecycle.contains("distinct_session_steps_commute")
            && session_lifecycle.contains("| fault sessionId reporter fault =>")
            && session_lifecycle.contains("| observeCancellation sessionId role =>")
            && runtime_refinement.contains("inductive RuntimeEffect")
            && runtime_refinement.contains("match before.decode? session roleCount choreo with")
            && runtime_refinement.contains("runtime_effect_acceptance_requires_canonical")
            && runtime_refinement
                .contains("runtime_effect_acceptance_requires_well_formed_program")
            && runtime_refinement
                .contains("runtime_protocol_transition_preserves_global_invariant")
            && runtime_refinement.contains("runtime_fault_transition_preserves_global_invariant")
            && runtime_refinement
                .contains("runtime_ambiguous_receive_transition_preserves_global_invariant")
            && runtime_refinement
                .contains("runtime_cancellation_observation_preserves_global_invariant")
            && runtime_refinement.contains("runtime_stutter_effect_is_zero_transition")
            && runtime_refinement.contains("runtime_transition_certificate_sound")
            && runtime_refinement.contains("ambiguous_receive_fails_closed")
            && protocol_artifact.contains("structure VerifiedProtocolCertificate")
            && protocol_artifact.contains("checkDescriptorCertificates")
            && protocol_artifact
                .contains("verified_protocol_certificate_establishes_all_role_refinement")
            && protocol_artifact.contains("verified_protocol_has_injective_inbound_evidence")
            && protocol_artifact.contains("verified_protocol_transport_admission_is_unique"),
        "asynchronous cancellation, projectability, distributed simulation, runtime refinement, and multi-session isolation must remain gate-owned"
    );
    assert!(
        runtime_monitor.contains("structure MonitorRequest")
            && runtime_monitor.contains("eventId : Nat")
            && runtime_monitor.contains("action : LocalAction")
            && runtime_monitor.contains("def matchingMonitorEvent?")
            && runtime_monitor.contains("event.id = request.eventId")
            && runtime_monitor.contains("event.action = request.action")
            && runtime_monitor.contains("def monitorStep")
            && runtime_monitor.contains("def applyMonitor")
            && runtime_monitor.contains("theorem monitor_rejects_send_schema_mismatch")
            && runtime_monitor.contains("theorem monitor_rejects_send_peer_mismatch")
            && runtime_monitor.contains("theorem monitor_rejects_wrong_direction"),
        "runtime-erased endpoint operations must be proved against exact descriptor event identity"
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
        "theorem initial_state_has_no_completed_event",
        "theorem initial_state_has_no_selected_arm",
        "theorem selected_arm_unique",
        "theorem reset_roll_preserves_outside_done",
        "theorem reset_roll_clears_done",
        "theorem reset_roll_preserves_outside_selection",
        "theorem reset_roll_clears_selection",
        "theorem reset_route_arm_preserves_selection",
        "theorem reset_route_arm_preserves_outside_done",
        "theorem reset_route_arm_clears_done",
        "theorem local_action_sender_projects_send",
        "theorem local_action_receiver_projects_recv",
        "theorem local_action_send_recv_duality",
        "theorem local_action_uninvolved_projects_none",
        "theorem local_action_unit_self_send_projects_local",
        "theorem local_action_payload_self_send_rejected",
        "theorem local_action_preserves_label",
        "theorem local_action_preserves_schema",
        "theorem local_action_preserves_key",
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
        "theorem progress_certificate_covers_reachable",
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
        "theorem first_session_attach_binds_exactly",
        "theorem exact_session_reattach_preserves_binding",
        "theorem mixed_program_session_attach_rejected",
        "theorem cross_rendezvous_session_attach_rejected",
        "theorem accepted_session_reattach_is_exact",
        "theorem matching_monitor_event_is_exact",
        "theorem monitor_rejects_descriptor_id_mismatch",
        "theorem monitor_rejects_action_mismatch",
        "theorem monitor_acceptance_is_exact_commit",
        "theorem monitor_rejects_send_schema_mismatch",
        "theorem monitor_rejects_send_peer_mismatch",
        "theorem monitor_rejects_wrong_direction",
        "theorem monitor_rejects_missing_event",
        "theorem monitor_rejection_preserves_state",
        "theorem global_event_conflicts_from_length",
        "theorem global_event_parallel_arms_from_length",
        "theorem global_event_parallel_arms_length",
        "theorem global_events_from_length_eq",
        "theorem global_events_from_projected_actions_eq",
        "theorem global_events_from_lane_bounds",
        "theorem parallel_global_event_lanes_are_disjoint",
        "theorem canonical_program_atoms_global_events",
        "theorem accepted_descriptor_global_events_bind_canonical_lanes",
        "theorem transport_admission_is_unique",
        "theorem transport_admission_checker_sound",
        "theorem transport_admission_depends_only_on_observation",
        "theorem global_transport_admission_checker_sound",
        "theorem observed_transport_admission_ignores_carrier_history",
        "theorem transport_admission_binds_exact_descriptor_occurrence",
        "theorem transport_admission_produces_exact_admitted_message",
        "theorem receive_lane_causality_checker_sound",
        "theorem receive_lane_sender_change_requires_exclusion_or_causal_handoff",
        "theorem roll_receive_lane_causality_checker_sound",
        "theorem roll_body_occurrences_cross_iteration_safe",
        "theorem roll_reentry_sender_change_requires_causal_handoff",
        "theorem local_queued_frame_has_canonical_lane",
        "theorem initial_route_selection_fidelity",
        "theorem global_step_preserves_route_selection_fidelity",
        "theorem reject_resolver_step_preserves_protocol_identity",
        "theorem reject_resolver_step_begins_cancellation",
        "theorem event_excluded_of_selected_opposite",
        "theorem roll_ready_of_consumed_or_excluded",
        "theorem executable_operation_sound",
        "theorem fairness_schedules_recurrently_enabled_operation",
        "theorem async_close_and_drain_commute",
        "theorem accepted_channel_authority_checker_sound",
        "theorem accepted_channel_authority_frames_match",
        "theorem accepted_channel_queue_checker_complete",
        "theorem accepted_channel_queue_checker_sound",
        "theorem transport_snapshot_has_accepted_channel_authority",
        "theorem retirement_ready_reaches_full_retirement",
        "theorem begin_cancellation_preserves_global_invariant",
        "theorem cancellation_observation_preserves_global_invariant",
        "theorem compact_global_decode_acceptance_implies_canonical",
        "theorem compact_global_decode_acceptance_implies_program_checker",
        "theorem compact_global_decode_rejects_noncanonical",
        "theorem compact_global_decode_rejects_invalid_program",
        "theorem projectability_certificate_sound",
        "theorem projectability_certificate_removes_global_progress_premise",
        "theorem projectability_certificate_establishes_static_projectability",
        "theorem projectability_certificate_all_roles_have_well_formed_projection",
        "theorem projectability_certificate_establishes_semantic_unstuckness",
        "theorem static_projectability_checker_sound",
        "theorem distributed_step_weakly_simulates_global",
        "theorem distributed_protocol_step_is_role_owned",
        "theorem distributed_protocol_step_has_local_authority",
        "theorem route_selection_observation_has_exact_queued_evidence",
        "theorem distributed_successor_abstraction",
        "theorem distributed_roll_step_simulates_global",
        "theorem roll_step_with_drained_transport_has_fresh_next_frame",
        "theorem roll_step_with_admitted_transport_frame_is_fresh_occurrence",
        "theorem abort_peer_is_immediately_observable",
        "theorem abort_peer_is_well_formed",
        "theorem distinct_carrier_generations_cannot_alias",
        "theorem reused_session_on_new_carrier_generation_excludes_old_frame",
        "theorem runtime_fault_classification_is_total",
        "theorem runtime_effect_acceptance_requires_canonical",
        "theorem runtime_effect_acceptance_requires_well_formed_program",
        "theorem runtime_protocol_transition_preserves_global_invariant",
        "theorem runtime_fault_transition_preserves_global_invariant",
        "theorem runtime_ambiguous_receive_transition_preserves_global_invariant",
        "theorem runtime_cancellation_observation_preserves_global_invariant",
        "theorem runtime_stutter_effect_is_zero_transition",
        "theorem session_pool_step_preserves_other_session_state",
        "theorem session_pool_step_preserves_well_formed",
        "theorem attach_initial_exact",
        "theorem with_initial_preserves_other_session",
        "theorem with_initial_preserves_well_formed",
        "theorem attach_initial_preserves_invariant",
        "theorem retire_session_exact",
        "theorem without_session_clears_identity",
        "theorem without_session_preserves_other_session",
        "theorem without_session_preserves_well_formed",
        "theorem retire_session_preserves_invariant",
        "theorem retired_session_allows_fresh_attach",
        "theorem with_session_updates_commute",
        "theorem session_pool_step_is_target_local",
        "theorem distinct_session_steps_commute",
        "theorem runtime_transition_certificate_sound",
        "theorem ambiguous_receive_fails_closed",
        "theorem verified_protocol_certificate_establishes_all_role_refinement",
        "theorem verified_protocol_descriptor_actions_match_projection",
        "theorem verified_protocol_has_injective_inbound_evidence",
        "theorem verified_protocol_transport_admission_is_unique",
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
                || runtime_monitor.contains(theorem)
                || global_syntax.contains(theorem)
                || global_semantics.contains(theorem)
                || global_message_transitions.contains(theorem)
                || global_fidelity.contains(theorem)
                || descriptor_refinement.contains(theorem)
                || cancellation.contains(theorem)
                || transport_contract.contains(theorem)
                || async_cancellation.contains(theorem)
                || async_cancellation_termination.contains(theorem)
                || global_progress.contains(theorem)
                || static_projectability.contains(theorem)
                || global_coherence.contains(theorem)
                || distributed_semantics.contains(theorem)
                || roll_freshness.contains(theorem)
                || session_composition.contains(theorem)
                || session_lifecycle.contains(theorem)
                || runtime_refinement.contains(theorem)
                || protocol_artifact.contains(theorem)
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
            && proof_gate.contains("hibana-lean-theorems.XXXXXX")
            && proof_gate.contains("hibana-lean-audit.XXXXXX")
            && proof_gate
                .contains("Lean proof gate requires an axiom audit for every exported theorem")
            && proof_gate.contains("depends on axioms: [propext, Quot.sound]")
            && proof_gate.contains("Classical.choice")
            && proof_gate.contains("!= \"165\"")
            && proof_gate.contains("!= \"131\"")
            && proof_gate.contains("!= \"26\"")
            && proof_gate.contains("!= \"322\"")
            && axiom_audit.contains("#print axioms Hibana.initial_state_has_no_completed_event")
            && axiom_audit.contains("#print axioms Hibana.initial_state_has_no_selected_arm")
            && axiom_audit.contains("#print axioms Hibana.local_action_sender_projects_send")
            && axiom_audit.contains("#print axioms Hibana.local_action_receiver_projects_recv")
            && axiom_audit.contains("#print axioms Hibana.local_action_preserves_schema")
            && axiom_audit.contains("#print axioms Hibana.local_action_preserves_key")
            && axiom_audit.contains("#print axioms Hibana.monitor_rejects_send_schema_mismatch")
            && axiom_audit.contains("#print axioms Hibana.monitor_rejects_send_peer_mismatch")
            && axiom_audit.contains("#print axioms Hibana.monitor_rejects_wrong_direction")
            && axiom_audit.contains("#print axioms Hibana.global_events_from_length_eq")
            && axiom_audit.contains("#print axioms Hibana.global_events_from_lane_bounds")
            && axiom_audit
                .contains("#print axioms Hibana.parallel_global_event_lanes_are_disjoint")
            && axiom_audit
                .contains("#print axioms Hibana.global_event_parallel_arms_from_length")
            && axiom_audit
                .contains("#print axioms Hibana.global_event_parallel_arms_length")
            && axiom_audit
                .contains("#print axioms Hibana.canonical_program_atoms_global_events")
            && axiom_audit.contains(
                "#print axioms Hibana.accepted_descriptor_global_events_bind_canonical_lanes"
            )
            && axiom_audit.contains("#print axioms Hibana.mixed_program_session_attach_rejected")
            && axiom_audit
                .contains("#print axioms Hibana.cross_rendezvous_session_attach_rejected")
            && axiom_audit.contains("#print axioms Hibana.progress_certificate_covers_reachable")
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
            && axiom_audit.contains("#print axioms Hibana.exact_descriptor_certificate_sound")
            && axiom_audit
                .contains("#print axioms Hibana.begun_cancellation_has_finite_retirement_trace")
            && axiom_audit
                .contains("#print axioms Hibana.begun_cancellation_reaches_resource_retirement")
            && axiom_audit
                .contains("#print axioms Hibana.cancellation_observation_preserves_well_formed")
            && axiom_audit.contains("#print axioms Hibana.well_formed_transport_has_no_replay")
            && axiom_audit.contains("#print axioms Hibana.transport_receive_preserves_well_formed")
            && axiom_audit.contains("#print axioms Hibana.transport_admission_is_unique")
            && axiom_audit
                .contains("#print axioms Hibana.transport_admission_checker_sound")
            && axiom_audit
                .contains("#print axioms Hibana.global_transport_admission_checker_sound")
            && axiom_audit.contains(
                "#print axioms Hibana.transport_admission_binds_exact_descriptor_occurrence"
            )
            && axiom_audit.contains(
                "#print axioms Hibana.transport_admission_produces_exact_admitted_message"
            )
            && axiom_audit
                .contains("#print axioms Hibana.top_level_roll_step_uses_global_roll_transition")
            && axiom_audit
                .contains("#print axioms Hibana.fairness_schedules_recurrently_enabled_operation")
            && axiom_audit.contains("#print axioms Hibana.executable_operation_sound")
            && axiom_audit
                .contains("#print axioms Hibana.reject_resolver_step_begins_cancellation")
            && axiom_audit.contains("#print axioms Hibana.roll_ready_of_consumed_or_excluded")
            && axiom_audit
                .contains("#print axioms Hibana.global_reachable_preserves_global_invariant")
            && axiom_audit.contains("#print axioms Hibana.global_step_preserves_global_invariant")
            && axiom_audit
                .contains("#print axioms Hibana.retirement_ready_reaches_full_retirement")
            && axiom_audit.contains("#print axioms Hibana.projectability_certificate_sound")
            && axiom_audit.contains(
                "#print axioms Hibana.projectability_certificate_establishes_semantic_unstuckness"
            )
            && axiom_audit.contains("#print axioms Hibana.static_projectability_checker_sound")
            && axiom_audit
                .contains("#print axioms Hibana.inbound_occurrence_identity_checker_sound")
            && axiom_audit
                .contains("#print axioms Hibana.receive_lane_causality_checker_sound")
            && axiom_audit.contains(
                "#print axioms Hibana.receive_lane_sender_change_requires_exclusion_or_causal_handoff"
            )
            && axiom_audit
                .contains("#print axioms Hibana.roll_receive_lane_causality_checker_sound")
            && axiom_audit
                .contains("#print axioms Hibana.roll_body_occurrences_cross_iteration_safe")
            && axiom_audit.contains(
                "#print axioms Hibana.roll_reentry_sender_change_requires_causal_handoff"
            )
            && axiom_audit
                .contains("#print axioms Hibana.distributed_step_weakly_simulates_global")
            && axiom_audit
                .contains("#print axioms Hibana.distributed_protocol_step_is_role_owned")
            && axiom_audit
                .contains("#print axioms Hibana.distributed_protocol_step_has_local_authority")
            && axiom_audit
                .contains("#print axioms Hibana.local_queued_frame_has_canonical_lane")
            && axiom_audit.contains(
                "#print axioms Hibana.route_selection_observation_has_exact_queued_evidence"
            )
            && axiom_audit.contains(
                "#print axioms Hibana.roll_step_with_drained_transport_has_fresh_next_frame"
            )
            && axiom_audit.contains(
                "#print axioms Hibana.roll_step_with_admitted_transport_frame_is_fresh_occurrence"
            )
            && axiom_audit
                .contains("#print axioms Hibana.session_pool_step_preserves_other_session_state")
            && axiom_audit.contains(
                "#print axioms Hibana.compact_global_decode_acceptance_implies_canonical"
            )
            && axiom_audit.contains(
                "#print axioms Hibana.compact_global_decode_acceptance_implies_program_checker"
            )
            && axiom_audit
                .contains("#print axioms Hibana.compact_global_decode_rejects_noncanonical")
            && axiom_audit
                .contains("#print axioms Hibana.compact_global_decode_rejects_invalid_program")
            && axiom_audit
                .contains("#print axioms Hibana.runtime_effect_acceptance_requires_canonical")
            && axiom_audit.contains(
                "#print axioms Hibana.runtime_effect_acceptance_requires_well_formed_program"
            )
            && axiom_audit.contains(
                "#print axioms Hibana.runtime_protocol_transition_preserves_global_invariant"
            )
            && axiom_audit.contains(
                "#print axioms Hibana.runtime_fault_transition_preserves_global_invariant"
            )
            && axiom_audit.contains(
                "#print axioms Hibana.runtime_ambiguous_receive_transition_preserves_global_invariant"
            )
            && axiom_audit.contains(
                "#print axioms Hibana.runtime_cancellation_observation_preserves_global_invariant"
            )
            && axiom_audit
                .contains("#print axioms Hibana.runtime_stutter_effect_is_zero_transition")
            && axiom_audit.contains("#print axioms Hibana.runtime_transition_certificate_sound")
            && axiom_audit.contains(
                "#print axioms Hibana.verified_protocol_certificate_establishes_all_role_refinement"
            )
            && axiom_audit.contains(
                "#print axioms Hibana.verified_protocol_has_injective_inbound_evidence"
            )
            && axiom_audit.contains(
                "#print axioms Hibana.verified_protocol_transport_admission_is_unique"
            )
            && commit.contains("rollReentryState?")
            && commit.contains("routeReentryState?")
            && proof_gate.contains(
                "traces=14 frames=64 projections=19 exact-descriptors=19 progress=4 projectability=8 verified-protocols=8"
            )
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
