use super::common::*;

#[test]
fn endpoint_kernel_stays_monomorphic_behind_raw_ops() {
    let endpoint = endpoint_facade_source();
    let flow = read("src/endpoint/flow.rs");
    let kernel = read("src/endpoint/kernel/core.rs");

    assert!(
        !endpoint.contains("dyn Any")
            && !flow.contains("dyn Any")
            && !kernel.contains("TypeId")
            && !kernel.contains("Box<dyn"),
        "typed Endpoint APIs must not recover behavior through runtime type-erasure escape hatches"
    );
}

#[test]
fn completed_raw_futures_fail_fast_on_repoll() {
    let endpoint = endpoint_facade_source();
    let flow = read("src/endpoint/flow.rs");
    let cursor = cursor_send_recv_tests_source();
    let offer_decode = read("tests/offer_decode_receive_evidence.rs");

    assert!(
        !endpoint.contains("post-ready poll advances")
            && !flow.contains("post-ready poll advances")
            && !endpoint.contains("silently repoll")
            && !flow.contains("silently repoll"),
        "raw futures must not document or implement silent post-Ready progress"
    );
    for required in [
        "completed_recv_future_repoll_is_fail_fast_and_does_not_advance_again",
        "completed_send_future_repoll_is_fail_fast_and_does_not_advance_again",
        "completed offer future must fail fast on post-Ready poll",
        "completed decode future must fail fast on post-Ready poll",
    ] {
        assert!(
            cursor.contains(required) || offer_decode.contains(required),
            "post-Ready fail-fast must have runtime coverage: {required}"
        );
    }
}

#[test]
fn payload_decode_after_commit_is_infallible() {
    let endpoint = endpoint_facade_source();
    let wire = read("src/transport/wire.rs");

    assert!(
        wire.contains("fn decode_validated_payload<'a>(input: Payload<'a>) -> Self::Decoded<'a>;")
            && wire.contains(
                "fn decode_payload<'a>(input: Payload<'a>) -> Result<Self::Decoded<'a>, CodecError>"
            ),
        "WirePayload must split pre-commit validation from infallible post-commit decode"
    );
    assert!(
        endpoint.contains("decode_validated_payload(payload)")
            && !endpoint.contains("::decode_payload(payload);"),
        "Endpoint recv/decode must not run a fallible payload decoder after committing progress"
    );
}

#[test]
fn empty_route_branch_progress_does_not_borrow_scope_tail_eff() {
    let materialization = read("src/endpoint/kernel/offer/materialization.rs");
    let decode = read("src/endpoint/kernel/decode.rs");
    let finish = read("src/endpoint/kernel/decode/finish.rs");
    let finish_commit_txn = read("src/endpoint/kernel/decode/finish/commit_txn.rs");

    assert!(
        !materialization.contains(
            "scope_lane_last_eff_for_route_commit(scope_id, selected_arm, lane_wire, None)"
        ) && !materialization.contains("self.cursor.scope_lane_last_eff(scope_id, lane_wire)")
            && materialization.contains("eff_index: meta.eff_index"),
        "offer materialization must not recover branch progress from a scope-wide tail fallback"
    );

    let empty_variant_start = decode
        .find("Empty {")
        .expect("DecodeProgressPlan::Empty variant must exist");
    let empty_variant = &decode[empty_variant_start
        ..empty_variant_start
            + decode[empty_variant_start..]
                .find("}")
                .expect("DecodeProgressPlan::Empty variant must close")];
    assert!(
        !empty_variant.contains("progress_eff"),
        "empty route arms must be terminal progress, not fake resident eff progress"
    );

    let empty_build_start = finish_commit_txn
        .find("BranchKind::EmptyArmTerminal => {\n                let next_index")
        .expect("empty branch decode builder must exist");
    let empty_build = &finish_commit_txn[empty_build_start
        ..empty_build_start
            + finish_commit_txn[empty_build_start..]
                .find("BranchKind::WireRecv | BranchKind::ArmSendHint")
                .expect("empty branch decode builder must be followed by wire/hint guard")];
    assert!(
        !empty_build.contains("resident_lane_step"),
        "empty route branch planning must not require a fabricated resident lane step"
    );

    let empty_publish_start = finish
        .find("DecodeProgressPlan::Empty {")
        .expect("empty branch publisher must exist");
    let empty_publish = &finish[empty_publish_start
        ..empty_publish_start
            + finish[empty_publish_start..]
                .find("}\n        }")
                .expect("empty branch publisher must close")];
    assert!(
        !empty_publish.contains("advance_lane_cursor"),
        "empty route branch publishing must not advance a lane with borrowed eff progress"
    );
}

#[test]
fn cap_release_context_carries_rendezvous_lifetime() {
    let capability = read("src/rendezvous/capability.rs");
    let public_types = read("src/endpoint/kernel/core/public_types.rs");

    for required in [
        "pub(crate) struct CapReleaseCtx<'rv>",
        "cap_table: &'rv CapTable",
        "snapshots: &'rv StateSnapshotTable",
        "revisions: &'rv Cell<u64>",
        "impl<'rv> CapReleaseCtx<'rv>",
        "release_ctx: Option<CapReleaseCtx<'rv>>",
        "pub(crate) struct PendingCapRelease<'rv>",
        "pub(crate) fn release_now(mut self)",
    ] {
        assert!(
            capability.contains(required) || public_types.contains(required),
            "capability release ownership must be tied to the rendezvous lifetime: {required}"
        );
    }

    for forbidden in [
        "NonNull<CapTable>",
        "NonNull<StateSnapshotTable>",
        "NonNull<Cell<u64>>",
        "pub(crate) struct CapReleaseCtx {\n",
        "#[derive(Clone, Copy)]\npub(crate) struct CapReleaseCtx",
        "RawRegisteredCapToken",
        "into_registered_token",
    ] {
        assert!(
            !capability.contains(forbidden) && !public_types.contains(forbidden),
            "capability release must not erase lifetime or keep registered-token roundtrip state: {forbidden}"
        );
    }
}

#[test]
fn first_recv_dispatch_crosses_layers_as_typed_specs() {
    let facts = read("src/global/typestate/facts.rs");
    let cursor_machine = read("src/global/typestate/cursor.rs");
    let cursor_first_recv_dispatch = read("src/global/typestate/cursor/first_recv_dispatch.rs");
    let scope_route = read("src/global/typestate/cursor/scope_route.rs");
    let offer_cache = read("src/endpoint/kernel/offer/first_recv_dispatch.rs");
    let frontier_types = read("src/endpoint/kernel/offer/frontier_types.rs");
    let first_recv_spec = facts
        .split("pub(crate) struct FirstRecvDispatchSpec")
        .nth(1)
        .and_then(|tail| tail.split("impl FirstRecvDispatchSpec").next())
        .expect("first recv dispatch spec must stay present");
    assert!(
        facts.contains("pub(crate) struct FirstRecvDispatchSpec")
            && first_recv_spec.contains("lane: u8,")
            && cursor_machine.contains("mod first_recv_dispatch;")
            && cursor_first_recv_dispatch.contains("FirstRecvDispatchSpec::new(")
            && scope_route.contains("[FirstRecvDispatchSpec; MAX_FIRST_RECV_DISPATCH]")
            && offer_cache.contains("struct FirstRecvDispatchCache")
            && offer_cache.contains("arm_mask: u8")
            && offer_cache.contains("const EMPTY: Self = Self { arm_mask: 0 }")
            && frontier_types.contains("record_first_recv_dispatch(&mut self, arm_mask: u8)")
            && !offer_cache.contains("FirstRecvDispatchEntry")
            && !offer_cache.contains("entries:")
            && !offer_cache.contains("len:")
            && !frontier_types.contains("[FirstRecvDispatchSpec; MAX_FIRST_RECV_DISPATCH]"),
        "first-recv dispatch must stay typed in cursor rows and collapse to a mask in endpoint metadata"
    );
    for source in [
        cursor_machine.as_str(),
        cursor_first_recv_dispatch.as_str(),
        scope_route.as_str(),
        offer_cache.as_str(),
        frontier_types.as_str(),
    ] {
        for forbidden in [
            "[(u8, u8, u8, StateIndex); MAX_FIRST_RECV_DISPATCH]",
            "Option<(u8, u8, u8, StateIndex)>",
            "from_tuple",
        ] {
            assert!(
                !source.contains(forbidden),
                "first-recv dispatch must not retain positional tuple compatibility: {forbidden}"
            );
        }
    }
}
#[test]
fn offer_alignment_exposes_typed_selection_not_mask_counts() {
    let alignment = read("src/endpoint/kernel/offer/select_alignment.rs");
    let candidates = read("src/endpoint/kernel/offer/select_alignment/candidates.rs");
    let model = format!(
        "{}\n{}\n{}\n{}",
        read("src/endpoint/kernel/offer/select_alignment/model.rs"),
        read("src/endpoint/kernel/offer/select_alignment/model/entry.rs"),
        read("src/endpoint/kernel/offer/select_alignment/model/set.rs"),
        read("src/endpoint/kernel/offer/select_alignment/model/pool.rs")
    );
    let orchestration = format!("{alignment}\n{candidates}");
    assert!(
        model.contains("enum OfferAlignmentOutcome")
            && model.contains("struct OfferEntrySet")
            && model.contains("struct CurrentOfferObservation")
            && model.contains("struct OfferAlignmentCandidatePool")
            && model.contains("struct OfferAlignmentSelection")
            && model.contains("UniqueDynamicController(usize)")
            && model.contains("AmbiguousCandidates")
            && candidates.contains("selection: OfferAlignmentSelection"),
        "offer alignment must expose a typed selection boundary"
    );
    for forbidden in [
        "observed: u8",
        "ready: u8",
        "ready_arm: u8",
        "controller: u8",
        "dynamic_controller: u8",
        "progress: u8",
        "current: u8",
        "candidates: u8",
        "controllers: u8",
        "dynamic_controllers: u8",
    ] {
        assert!(
            !model.contains(forbidden),
            "offer alignment model must keep mask storage behind typed entry sets: {forbidden}"
        );
    }
    for forbidden in [
        "observed: OfferEntryMask",
        "ready: OfferEntryMask",
        "ready_arm: OfferEntryMask",
        "controller: OfferEntryMask",
        "dynamic_controller: OfferEntryMask",
        "progress: OfferEntryMask",
        "current: OfferEntryMask",
        "struct OfferAlignmentCandidateSet",
        "pub(super) current_ready",
        "pub(super) current_progress_evidence",
    ] {
        assert!(
            !model.contains(forbidden) && !orchestration.contains(forbidden),
            "offer alignment must not expose raw mask/current-evidence storage: {forbidden}"
        );
    }
    for forbidden in [
        "candidate_mask",
        "observed_mask",
        "hint_filter_mask",
        "candidate_count",
        "dynamic_controller_count",
        "controller_count",
        "candidate_idx",
        "choose_offer_priority",
    ] {
        assert!(
            !orchestration.contains(forbidden),
            "offer alignment orchestration must not carry mask/count arbitration residue: {forbidden}"
        );
    }
}
#[test]
fn local_control_mint_does_not_publish_route_or_loop_authority() {
    let send_control = read("src/endpoint/kernel/core/send_control_ops.rs");
    let send_ops = read("src/endpoint/kernel/core/send_ops.rs");
    let send_control_commit = read("src/endpoint/kernel/core/send_control_commit.rs");
    assert!(
        !send_control.contains("mint_local_route_arm_selection_control"),
        "route decisions must not have a local self-send control mint path"
    );
    assert!(
        !send_ops.contains("ControlOp::RouteResolve")
            && !send_control_commit.contains("ControlOp::RouteResolve"),
        "route arm resolution must not be represented as a send-control opcode"
    );
    for function in [
        "mint_local_loop_continue_control",
        "mint_local_loop_break_control",
    ] {
        let start = send_control
            .find(&format!("fn {function}"))
            .unwrap_or_else(|| panic!("{function} must exist"));
        let rest = &send_control[start..];
        let end = rest.find("\n    #[inline").unwrap_or(rest.len());
        let body = &rest[..end];

        for forbidden in [
            "record_loop_decision(",
            "record_route_arm_selection_for_scope_lanes(",
            "emit_route_arm_selection(",
        ] {
            assert!(
                !body.contains(forbidden),
                "{function} must build a send-control decision plan, not publish route/loop authority during mint: {forbidden}"
            );
        }
    }

    assert!(
        send_control_commit.contains("fn build_send_loop_commit_row")
            && send_control_commit.contains("LoopCommitRow::decision")
            && !send_control_commit.contains("fn publish_send_control_decision_plan")
            && !send_control_commit.contains("SendControlDecisionPlan")
            && send_ops.contains("let loop_row = self.build_send_loop_commit_row")
            && send_ops.contains(".with_loop_row(loop_row);")
            && !send_ops.contains("self.publish_send_control_decision_plan"),
        "local loop control authority must be a CommitDelta loop row, not a side publish path"
    );
}

#[test]
fn topology_ack_uses_prepared_distributed_and_local_proofs_without_cached_operands() {
    let send_control = read("src/endpoint/kernel/core/send_control_ops.rs");
    let prepared_send = read("src/control/cluster/core/descriptor_controls/prepared_send.rs");

    assert!(
        !send_control.contains("fn mint_local_topology_ack_control")
            && !send_control.contains("cached_topology_operands(cp_sid)")
            && !send_control.contains("take_cached_topology_operands"),
        "topology ack must not re-enter a local raw-handle mint preview path"
    );
    assert!(
        !prepared_send.contains("cached_operands")
            && !prepared_send.contains("CachedTopologyBucket")
            && !prepared_send.contains("cache_topology_operands"),
        "TopologyAck publish must not retain a cached-operand side authority"
    );

    let ack_start = prepared_send
        .find("ReservedTopologyTerminal::Ack(ticket) =>")
        .expect("TopologyAck prepared publish owner must exist");
    let ack_body = &prepared_send[ack_start..];
    let acknowledge = ack_body
        .find("publish_prepared_ack(")
        .expect("TopologyAck must consume its reserved topology state");
    let local_ack = ack_body
        .find("publish_prepared_destination_topology_ack(destination)")
        .expect("TopologyAck must publish the destination-local prepared proof");
    assert!(
        acknowledge < local_ack,
        "TopologyAck publish must consume the distributed prepared proof before publishing the destination-local proof"
    );

    let prepare_start = prepared_send
        .find("fn prepare_topology_ack_descriptor_commit")
        .expect("TopologyAck prepare owner must exist");
    let prepare_rest = &prepared_send[prepare_start..];
    let prepare_end = prepare_rest
        .find("\n    #[inline(never)]\n    fn prepare_topology_commit_descriptor_commit")
        .expect("TopologyAck prepare body must be bounded by topology commit prepare");
    let prepare_body = &prepare_rest[..prepare_end];
    let distributed_preflight = prepare_body
        .find("preflight_ack(")
        .expect("TopologyAck must preflight distributed state before local mutation");
    let local_preflight = prepare_body
        .find("preflight_destination_topology_ack(")
        .expect("TopologyAck must preflight destination-local state before storage ensure");
    let storage_ensure = prepare_body
        .find("ensure_local_topology_storage_in_core(")
        .expect("TopologyAck must materialize destination storage only after preflight");
    let local_prepare = prepare_body
        .find("prepare_destination_topology_ack(")
        .expect("TopologyAck must build the destination-local prepared proof");
    let distributed_reserve = prepare_body
        .find("reserve_preflighted_ack(")
        .expect("TopologyAck must reserve distributed ack state after local proof");
    assert!(
        distributed_preflight < local_preflight
            && local_preflight < storage_ensure
            && storage_ensure < local_prepare
            && local_prepare < distributed_reserve,
        "TopologyAck prepare must preflight all semantic state before storage materialization, then mint the local proof and reserve distributed state infallibly"
    );
    assert!(
        !prepare_body.contains("rollback_prepared_ack(")
            && !prepare_body.contains("rollback_prepared_destination_topology_ack(")
            && !prepare_body.contains("reserve_ack("),
        "TopologyAck prepare must not rely on local/distributed rollback after one side has been prepared"
    );
}

#[test]
fn lease_bundle_does_not_advertise_inert_cap_rollback_owner() {
    let planner = read("src/control/lease/planner.rs");
    let endpoint_attach = read("src/control/cluster/core/endpoint_attach.rs");
    let lease_core = read("src/control/lease/core.rs");

    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    assert!(
        !root.join("src/control/lease/bundle.rs").exists()
            && !root.join("src/control/lease/bundle/tests.rs").exists(),
        "lease bundle must stay deleted rather than retaining an inert test/measurement owner"
    );

    for forbidden in [
        "CapsRollbackAuthority",
        "CapsBundleHandle",
        "track_mint",
        "release_by_nonce",
        "NonNull<CapTable>",
    ] {
        assert!(
            !planner.contains(forbidden)
                && !endpoint_attach.contains(forbidden)
                && !lease_core.contains(forbidden),
            "lease cap rollback must not keep an inert production owner: {forbidden}"
        );
    }

    for forbidden in [
        "FACET_CAPS",
        "requires_caps",
        "facets_caps",
        "\"caps\"",
        "slot/caps/topology",
    ] {
        assert!(
            !planner.contains(forbidden) && !endpoint_attach.contains(forbidden),
            "lease planner must not retain ghost cap facets after cap rollback moved to token drop guards: {forbidden}"
        );
    }

    for forbidden in [
        "LeaseObserve",
        "from_resident_tap",
        "observe: Option<LeaseObserve",
        "commit_event: Option<TapEvent>",
        "rollback_event: Option<TapEvent>",
        "pub(crate) const fn new(tap: *const TapRing",
    ] {
        assert!(
            !lease_core.contains(forbidden),
            "lease bundle must not retain unused observe/tap authority, even behind test cfg: {forbidden}"
        );
    }

    for forbidden in [
        "pub(crate) facets: LeaseFacetNeeds",
        "facets: LeaseFacetNeeds",
        "self.facets",
        "req.facets",
        "with_facets",
    ] {
        assert!(
            !planner.contains(forbidden),
            "LeaseCapacityBudget must not retain inert facet aggregation: {forbidden}"
        );
    }
}

#[test]
fn send_control_emitted_return_policy_is_typed() {
    let runtime_types = read("src/endpoint/kernel/core/runtime_types.rs");
    let send_ops = read("src/endpoint/kernel/core/send_ops.rs");
    let send_control_commit = read("src/endpoint/kernel/core/send_control_commit.rs");
    let staged_start = runtime_types
        .find("pub(crate) enum StagedControlEmission<'rv>")
        .expect("StagedControlEmission must exist");
    let staged_body = &runtime_types[staged_start
        ..runtime_types[staged_start..]
            .find("\n}\n\npub(crate) struct StagedSendPayload")
            .expect("StagedControlEmission body must be bounded")
            + staged_start];

    assert!(
        staged_body.contains("Registered(PendingCapRelease<'rv>)")
            && !staged_body.contains("WireOnly")
            && !staged_body.contains("RawEmittedCapToken")
            && !runtime_types.contains("StagedDispatchToken")
            && !send_ops.contains("StagedControlEmission::WireOnly")
            && send_ops.contains("StagedControlEmission::Registered")
            && send_control_commit.contains("release.release_now();")
            && !send_control_commit.contains("send_control_token_bytes")
            && !send_control_commit.contains("into_registered_token(")
            && !send_control_commit.contains("_meta: SendCommitMeta")
            && !send_ops.contains("RawEmittedCapToken")
            && !runtime_types.contains("RawRegisteredCapToken")
            && !runtime_types.contains("RawEmittedCapToken")
            && !runtime_types.contains("enum EmittedControlReturn")
            && !runtime_types.contains("return_policy: EmittedControlReturn")
            && !send_ops.contains("EmittedControlReturn::")
            && !runtime_types.contains("return_emitted: bool")
            && !send_ops.contains("return_emitted")
            && !send_control_commit.contains("registered send control outcome must be preflighted")
            && !send_control_commit.contains("registered-token return policy must be preflighted"),
        "send-control outcome must be none-or-registered, with no raw wire-only return policy"
    );
}

#[test]
fn topology_revocation_drains_send_terminal_without_cluster_reentry() {
    let endpoint_core = read("src/endpoint/kernel/core.rs");
    let runtime_types = read("src/endpoint/kernel/core/runtime_types.rs");
    let send_descriptor_terminal = read("src/endpoint/kernel/core/send_descriptor_terminal.rs");
    let public_ops = read("src/endpoint/kernel/public_ops.rs");
    let lifecycle = read("src/endpoint/carrier/lifecycle.rs");
    let local_topology = read("src/rendezvous/core/local_topology.rs")
        + &read("src/rendezvous/core/local_topology/endpoint_revocation.rs");
    let prepared_send = read("src/control/cluster/core/descriptor_controls/prepared_send.rs");

    assert!(
        send_descriptor_terminal.contains("pub(crate) struct EndpointRevocationTerminal<'rv>")
            && send_descriptor_terminal.contains("send: Option<SendRollbackPlan<'rv>>")
            && send_descriptor_terminal.contains("waiter_lane: Option<Lane>")
            && send_descriptor_terminal.contains("fn rollback_send_with<R>(&mut self, rollback: &mut R)")
            && runtime_types.contains("pub(crate) struct SendRollbackPlan<'rv>")
            && runtime_types.contains("fn into_rollback_plan(self) -> SendRollbackPlan<'rv>")
            && runtime_types.contains("fn into_rollback_parts(")
            && send_descriptor_terminal.contains("self.send = Some(plan.into_rollback_plan());")
            && send_descriptor_terminal.contains("rollback.rollback_endpoint_revocation_descriptor(ticket);")
            && send_descriptor_terminal.contains("drop(control);")
            && endpoint_core.contains("fn prepare_public_owner_revocation(")
            && endpoint_core.contains("fn finish_public_owner_revocation(")
            && endpoint_core.contains("terminal.set_waiter_lane(self.primary_physical_lane());")
            && endpoint_core.contains("self.revoke_drain_public_send_terminal(terminal);")
            && endpoint_core.contains("self.revoke_finish_public_send_state();")
            && public_ops.contains("fn revoke_drain_public_send_terminal(")
            && public_ops.contains("fn revoke_finish_public_send_state(")
            && public_ops.contains("fn revoke_clear_public_recv_state(")
            && public_ops.contains("fn revoke_clear_public_offer_state(")
            && public_ops.contains("fn revoke_clear_public_decode_state(")
            && public_ops.contains("terminal.set_send_plan(plan);")
            && public_ops.contains("self.cancel_detached_send_state(state);")
            && lifecycle.contains("endpoint.prepare_public_owner_revocation(terminal);")
            && lifecycle.contains("endpoint.finish_public_owner_revocation();")
            && local_topology
                .contains("#[must_use = \"revoked public endpoint cleanup must be finished\"]")
            && local_topology.contains("pub(crate) struct RevokedPublicEndpoint<'cfg>")
            && local_topology.contains("pub(crate) struct PreparedEndpointRevocation<'cfg, Phase>")
            && local_topology.contains("pub(crate) enum NeedsDescriptorRollback")
            && local_topology.contains("pub(crate) enum ReadyToRelease")
            && local_topology.contains("struct EndpointRevocationPlan<'cfg>")
            && local_topology.contains("released_lanes: [Lane; u8::MAX as usize + 1]")
            && local_topology.contains("lease_slot: EndpointLeaseId")
            && local_topology.contains("finish_entered: bool")
            && local_topology.contains("impl Drop for RevokedPublicEndpoint")
            && local_topology.contains("fn rollback_descriptor_with<R>(")
            && local_topology
                .contains("R: crate::endpoint::kernel::EndpointRevocationDescriptorRollback + ?Sized")
            && local_topology.contains("self.inner_mut().terminal.rollback_send_with(rollback);")
            && !local_topology.contains("crate::control::cluster::core::ControlCore")
            && local_topology.contains("PreparedEndpointRevocation<'cfg, ReadyToRelease>")
            && local_topology
                .contains("impl<Phase> Drop for PreparedEndpointRevocation<'_, Phase>")
            && local_topology.contains("fn finish(mut self)")
            && local_topology.contains("fn prepare_one_public_endpoint_revocation(")
            && local_topology.contains("fn commit_prepared_public_endpoint_revocation(")
            && local_topology.contains("EndpointRevocationTerminal::none()")
            && local_topology.contains("self.clear_session_waiter(plan.sid, lane);")
            && local_topology.contains("RevokedPublicEndpoint {")
            && prepared_send.contains("fn finish_topology_commit_revocation(")
            && prepared_send.contains("source_owner: RendezvousOwnerProof")
            && prepared_send.contains("fn drain_one_topology_commit_revocation(")
            && prepared_send.contains("core.locals.get_mut_by_proof(source_owner)")
            && prepared_send.contains("src.prepare_one_public_endpoint_revocation(sid)")
            && prepared_send.contains("let revocation = revocation.rollback_descriptor_with(core);")
            && prepared_send.contains("src.commit_prepared_public_endpoint_revocation(revocation)")
            && prepared_send
                .contains("crate::endpoint::kernel::EndpointRevocationDescriptorRollback")
            && prepared_send
                .contains("fn rollback_endpoint_revocation_descriptor(&mut self, ticket: DescriptorTerminal)")
            && prepared_send.contains(
                "SessionCluster::<T, U, C, MAX_RV>::rollback_descriptor_terminal_in_core(self, ticket);",
            )
            && prepared_send.contains("endpoint.finish();")
            && prepared_send.contains("fn retire_topology_commit_session_lanes(")
            && prepared_send.contains("src.retire_session_lanes_for_topology(sid);"),
        "topology revocation must drain endpoint obligations inside the SessionCluster owner phase and defer only endpoint transport cleanup"
    );
    assert!(
        !lifecycle.contains("endpoint.revoke_public_owner();")
            && !endpoint_core.contains("descriptor_terminal: &mut Option<SendDescriptorTerminal")
            && !endpoint_core.contains("waiter_lane: &mut Option<Lane>")
            && !local_topology.contains("terminal.rollback()")
            && !local_topology.contains("cluster.rollback_descriptor_terminal")
            && !local_topology.contains("fn drain_one_public_endpoint_revocation(")
            && !prepared_send.contains("revoke_public_endpoints_for_session_raw(")
            && !prepared_send.contains("drain_one_public_endpoint_revocation_raw(")
            && !prepared_send.contains("*mut LocalRendezvous")
            && !local_topology
                .contains("pub(crate) unsafe fn drain_one_public_endpoint_revocation_raw")
            && !local_topology.contains("retire_session_lanes_raw")
            && !prepared_send.contains("self.rollback_descriptor_effect_terminal(ticket);"),
        "revocation cleanup must not call descriptor publishers, expose raw rendezvous pointers, or bypass the SessionCluster owner phase"
    );

    let prepare_start = endpoint_core
        .find("pub(crate) fn prepare_public_owner_revocation(")
        .expect("prepare_public_owner_revocation must exist");
    let prepare_body = &endpoint_core[prepare_start
        ..prepare_start
            + endpoint_core[prepare_start..]
                .find("\n    pub(crate) fn finish_public_owner_revocation(")
                .expect("prepare revocation body must be bounded")];
    for forbidden in [
        "clear_session_waiter(",
        "terminal_clear_public_send_state(",
        "revoke_finish_public_send_state(",
        "cancel_send_outgoing",
        "drop(port)",
    ] {
        assert!(
            !prepare_body.contains(forbidden),
            "revocation prepare phase must not clear waiters through cluster APIs or run transport cleanup: {forbidden}"
        );
    }

    let finish_start = endpoint_core
        .find("pub(crate) fn finish_public_owner_revocation(")
        .expect("finish_public_owner_revocation must exist");
    let finish_body = &endpoint_core[finish_start
        ..finish_start
            + endpoint_core[finish_start..]
                .find("\n    }\n}\n\nimpl<'r, const ROLE")
                .expect("finish revocation body must be bounded")];
    assert!(
        finish_body
            .find("self.invalidate_public_owner();")
            .expect("finish must invalidate public owner")
            < finish_body
                .find("self.revoke_finish_public_send_state();")
                .expect("finish must cancel pending send after invalidation"),
        "revocation finish must invalidate the public owner before external transport cleanup can run"
    );

    let publish_start = prepared_send
        .find("fn publish_reserved_topology_terminal(")
        .expect("publish_reserved_topology_terminal must exist");
    let publish_body = &prepared_send[publish_start
        ..publish_start
            + prepared_send[publish_start..]
                .find("\n    fn finish_topology_commit_revocation(")
                .expect("publish body must be bounded")];
    for forbidden in [
        "drain_one_topology_commit_revocation(",
        "drain_one_public_endpoint_revocation(",
        "prepare_one_public_endpoint_revocation(",
        "commit_prepared_public_endpoint_revocation(",
        "finish_revoke_for_session",
        "endpoint.finish()",
        "cancel_send_outgoing",
    ] {
        assert!(
            !publish_body.contains(forbidden),
            "ControlCore topology publication must not run endpoint transport cleanup: {forbidden}"
        );
    }

    let finish_topology_start = prepared_send
        .find("fn finish_topology_commit_revocation(")
        .expect("finish_topology_commit_revocation must exist");
    let finish_topology_body = &prepared_send[finish_topology_start
        ..finish_topology_start
            + prepared_send[finish_topology_start..]
                .find("\n    fn drain_one_topology_commit_revocation(")
                .expect("finish topology body must be bounded")];
    for forbidden in ["src_ptr", "*mut LocalRendezvous", "unsafe fn"] {
        assert!(
            !finish_topology_body.contains(forbidden),
            "topology revocation finish must carry owner proof, not escaped raw rendezvous pointers: {forbidden}"
        );
    }

    let prepare_revocation_start = local_topology
        .find("pub(crate) fn prepare_one_public_endpoint_revocation(")
        .expect("prepare_one_public_endpoint_revocation must exist");
    let prepare_revocation_body = &local_topology[prepare_revocation_start
        ..prepare_revocation_start
            + local_topology[prepare_revocation_start..]
                .find("\n    pub(crate) fn commit_prepared_public_endpoint_revocation(")
                .expect("prepare revocation body must be bounded")];
    for forbidden in [
        "clear_session_waiter(",
        "release_endpoint_lease(",
        "release_lane(",
        "emit_lane_release(",
        "finish_revoke_for_session",
        "cancel_send_outgoing",
        "drop(port)",
    ] {
        assert!(
            !prepare_revocation_body.contains(forbidden),
            "prepared endpoint revocation must only drain obligations before descriptor rollback: {forbidden}"
        );
    }

    let commit_revocation_start = local_topology
        .find("pub(crate) fn commit_prepared_public_endpoint_revocation(")
        .expect("commit_prepared_public_endpoint_revocation must exist");
    let rollback_revocation_start = local_topology
        .find("pub(crate) fn rollback_descriptor_with<R>(")
        .expect("rollback_descriptor_with must exist");
    let rollback_revocation_body = &local_topology[rollback_revocation_start
        ..rollback_revocation_start
            + local_topology[rollback_revocation_start..]
                .find("\n}\n\nimpl Drop for RevokedPublicEndpoint")
                .expect("rollback revocation body must be bounded")];
    assert!(
        rollback_revocation_body
            .find("self.inner_mut().terminal.rollback_send_with(rollback);")
            .expect("rollback phase must consume pending send terminal before release")
            < rollback_revocation_body
                .find("let inner = self.take_inner();")
                .expect("rollback phase must not consume typestate owner before rollback"),
        "prepared endpoint revocation must retain its typestate owner until descriptor rollback has completed"
    );
    let terminal_rollback_start = send_descriptor_terminal
        .find("pub(crate) fn rollback_send_with<R>(&mut self, rollback: &mut R)")
        .expect("endpoint revocation send rollback must exist");
    let terminal_rollback_body = &send_descriptor_terminal[terminal_rollback_start..];
    assert!(
        terminal_rollback_body
            .find("let SendRollbackPlan {")
            .expect("revocation rollback must split the pending send rollback plan")
            < terminal_rollback_body
                .find("rollback.rollback_endpoint_revocation_descriptor(ticket);")
                .expect("revocation rollback must rollback descriptor")
            && terminal_rollback_body
                .find("rollback.rollback_endpoint_revocation_descriptor(ticket);")
                .expect("revocation rollback must rollback descriptor")
                < terminal_rollback_body.find("drop(control);").expect(
                    "revocation rollback must release registered control after descriptor rollback"
                ),
        "revocation rollback must rollback descriptor before registered control release"
    );

    let commit_revocation_body = &local_topology[commit_revocation_start
        ..commit_revocation_start
            + local_topology[commit_revocation_start..]
                .find("\n    fn retire_session_lane(")
                .unwrap_or(local_topology[commit_revocation_start..].len())];
    assert!(
        commit_revocation_body
            .contains("mut revocation: PreparedEndpointRevocation<'cfg, ReadyToRelease>")
            && !commit_revocation_body.contains("descriptor_drained")
            && !commit_revocation_body.contains("take_descriptor_ticket")
            && commit_revocation_body.contains("let plan = revocation.inner_ref();")
            && commit_revocation_body
                .contains("self.release_endpoint_lease(plan.lease_slot, plan.lease_generation);")
            && commit_revocation_body.contains("revocation.take_inner()")
            && commit_revocation_body
                .find("self.release_endpoint_lease(plan.lease_slot, plan.lease_generation);")
                .expect("commit must release endpoint lease before consuming the finish handle")
                < commit_revocation_body
                    .find("revocation.take_inner()")
                    .expect("commit must consume typestate owner after release phase"),
        "prepared endpoint revocation commit must be type-gated on descriptor-rollback-completed ReadyToRelease instead of runtime descriptor-drained asserts"
    );

    let drain_topology_start = prepared_send
        .find("fn drain_one_topology_commit_revocation(")
        .expect("drain_one_topology_commit_revocation must exist");
    let drain_topology_body = &prepared_send[drain_topology_start
        ..drain_topology_start
            + prepared_send[drain_topology_start..]
                .find("\n    fn retire_topology_commit_session_lanes(")
                .expect("drain topology body must be bounded")];
    assert!(
        drain_topology_body
            .find("src.prepare_one_public_endpoint_revocation(sid)")
            .expect("topology revocation must first prepare endpoint obligations")
            < drain_topology_body
                .find("revocation.rollback_descriptor_with(core)")
                .expect("topology revocation must rollback descriptors before release")
            && drain_topology_body
                .find("revocation.rollback_descriptor_with(core)")
                .expect("topology revocation must rollback descriptors before release")
                < drain_topology_body
                    .find("src.commit_prepared_public_endpoint_revocation(revocation)")
                    .expect("topology revocation must release lanes only after rollback"),
        "topology revocation must transition to ReadyToRelease only through descriptor rollback before endpoint lease or lane release"
    );
}
