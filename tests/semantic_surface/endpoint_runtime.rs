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
    let no_policy = read("tests/no_policy_route_transport_hint.rs");

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
            cursor.contains(required) || no_policy.contains(required),
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
    let compiled_dispatch =
        read("src/global/compiled/images/image/role_descriptor_ref/route_scope/dispatch.rs");
    let scope_route = read("src/global/typestate/cursor/scope_route.rs");
    let offer_cache = read("src/endpoint/kernel/offer/first_recv_dispatch.rs");
    let frontier_types = read("src/endpoint/kernel/offer/frontier_types.rs");
    assert!(
        facts.contains("pub(crate) struct FirstRecvDispatchSpec")
            && compiled_dispatch.contains("FirstRecvDispatchSpec::new(")
            && scope_route.contains("[FirstRecvDispatchSpec; MAX_FIRST_RECV_DISPATCH]")
            && offer_cache.contains("from_spec(entry: FirstRecvDispatchSpec)")
            && frontier_types.contains("[FirstRecvDispatchSpec; MAX_FIRST_RECV_DISPATCH]"),
        "first-recv dispatch must cross owner layers as typed specs"
    );
    for source in [
        compiled_dispatch.as_str(),
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
    for function in [
        "mint_local_loop_continue_control",
        "mint_local_loop_break_control",
        "mint_local_route_decision_control",
    ] {
        let start = send_control
            .find(&format!("fn {function}"))
            .unwrap_or_else(|| panic!("{function} must exist"));
        let rest = &send_control[start..];
        let end = rest.find("\n    #[inline").unwrap_or(rest.len());
        let body = &rest[..end];

        for forbidden in [
            "record_loop_decision(",
            "record_route_decision_for_scope_lanes(",
            "emit_route_decision(",
        ] {
            assert!(
                !body.contains(forbidden),
                "{function} must build a send-control decision plan, not publish route/loop authority during mint: {forbidden}"
            );
        }
    }

    assert!(
        send_control_commit.contains("fn build_send_control_decision_plan")
            && send_control_commit.contains("SendControlDecisionPlan::Loop")
            && send_control_commit.contains("SendControlDecisionPlan::Route")
            && send_control_commit.contains("fn publish_send_control_decision_plan")
            && send_ops.contains("let decision = self.build_send_control_decision_plan")
            && send_ops.contains("self.publish_send_control_decision_plan(decision);")
            && send_ops
                .find("self.finish_send_control_outcome(control);")
                .expect("send-control emission finish must exist")
                < send_ops
                    .find("self.publish_send_control_decision_plan(decision);")
                    .expect("decision publish must follow dispatch resolution"),
        "local route/loop control authority must publish from the post-dispatch send commit path"
    );
}

#[test]
fn topology_ack_mint_peeks_cached_operands_until_dispatch_success() {
    let send_control = read("src/endpoint/kernel/core/send_control_ops.rs");
    let prepared_send = read("src/control/cluster/core/descriptor_controls/prepared_send.rs");

    let start = send_control
        .find("fn mint_local_topology_ack_control")
        .expect("topology ack mint path must exist");
    let rest = &send_control[start..];
    let end = rest
        .find("\n    #[inline(never)]\n    fn mint_control_token_bytes_with_handle")
        .expect("topology ack mint body must be bounded by the next helper");
    let body = &rest[..end];

    assert!(
        body.contains("cached_topology_operands(cp_sid)")
            && !body.contains("take_cached_topology_operands"),
        "topology ack mint must only peek cached operands while send remains a preview"
    );

    let ack_start = prepared_send
        .find("ControlOp::TopologyAck =>")
        .expect("TopologyAck prepared publish owner must exist");
    let ack_body = &prepared_send[ack_start..];
    let acknowledge = ack_body
        .find("publish_prepared_ack(")
        .expect("TopologyAck must consume its reserved topology state");
    let local_ack = ack_body
        .find("publish_prepared_destination_topology_ack(destination)")
        .expect("TopologyAck must publish the destination-local prepared proof");
    let consume = ack_body
        .find("cached_operands_remove(sid)")
        .expect("TopologyAck success must consume cached operands");
    assert!(
        acknowledge < local_ack && local_ack < consume,
        "cached topology operands must be consumed only after TopologyAck distributed and local proofs are published"
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
    let local_prepare = prepare_body
        .find("prepare_destination_topology_ack(")
        .expect("TopologyAck must build the destination-local prepared proof");
    let distributed_reserve = prepare_body
        .find("reserve_preflighted_ack(")
        .expect("TopologyAck must reserve distributed ack state after local proof");
    assert!(
        distributed_preflight < local_prepare && local_prepare < distributed_reserve,
        "TopologyAck prepare must preflight distributed state, mint the local proof, then reserve distributed state infallibly"
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
    let bundle = read("src/control/lease/bundle.rs");
    let planner = read("src/control/lease/planner.rs");
    let endpoint_attach = read("src/control/cluster/core/endpoint_attach.rs");
    let lease_core = read("src/control/lease/core.rs");

    for forbidden in [
        "CapsRollbackAuthority",
        "CapsBundleHandle",
        "track_mint",
        "release_by_nonce",
        "NonNull<CapTable>",
    ] {
        assert!(
            !bundle.contains(forbidden),
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
            !lease_core.contains(forbidden) && !bundle.contains(forbidden),
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
            "LeaseGraphBudget must not retain inert facet aggregation: {forbidden}"
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
            && staged_body.contains("WireOnly")
            && !staged_body.contains("RawEmittedCapToken")
            && !runtime_types.contains("StagedDispatchToken")
            && send_ops.contains("StagedControlEmission::WireOnly")
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
        "send-control emitted/registered return ability must be carried by variants, not a policy enum or post-transport panic"
    );
}

#[test]
fn topology_revocation_drains_send_terminal_without_cluster_reentry() {
    let endpoint_core = read("src/endpoint/kernel/core.rs");
    let public_ops = read("src/endpoint/kernel/public_ops.rs");
    let lifecycle = read("src/endpoint/carrier/lifecycle.rs");
    let local_topology = read("src/rendezvous/core/local_topology.rs");
    let prepared_send = read("src/control/cluster/core/descriptor_controls/prepared_send.rs");

    assert!(
        endpoint_core.contains("descriptor_terminal: &mut Option<SendDescriptorTerminal<'r>>")
            && endpoint_core.contains("waiter_lane: &mut Option<Lane>")
            && endpoint_core.contains("*waiter_lane = Some(self.primary_physical_lane());")
            && endpoint_core.contains("self.revoke_drain_public_send_state(descriptor_terminal);")
            && public_ops.contains("fn revoke_drain_public_send_state(")
            && public_ops.contains("fn revoke_clear_public_recv_state(")
            && public_ops.contains("fn revoke_clear_public_offer_state(")
            && public_ops.contains("fn revoke_clear_public_decode_state(")
            && public_ops.contains("cancel_detached_send_state_into(")
            && public_ops.contains("plan.into_descriptor_terminal()")
            && lifecycle
                .contains("endpoint.revoke_public_owner(descriptor_terminal, waiter_lane);")
            && local_topology.contains("let mut descriptor_terminal = None;")
            && local_topology.contains("let mut waiter_lane = None;")
            && local_topology.contains("(*this).clear_session_waiter(sid, lane);")
            && local_topology.contains("rollback_terminal(terminal);")
            && prepared_send.contains("revoke_public_endpoints_for_session_raw(")
            && prepared_send.contains("rollback_descriptor_terminal_in_core(core, ticket)")
            && prepared_send.contains("retire_session_lanes_raw(src_ptr, sid)"),
        "topology revocation must drain pending send terminals and roll them back through the active ControlCore, not by re-entering SessionCluster"
    );
    assert!(
        !lifecycle.contains("endpoint.revoke_public_owner();")
            && !local_topology.contains("terminal.rollback()")
            && !local_topology.contains("cluster.rollback_descriptor_terminal")
            && !prepared_send.contains("self.rollback_descriptor_effect_terminal(ticket);"),
        "revocation cleanup must not call descriptor publishers or cluster rollback through a nested control mutation"
    );
    let revoke_start = endpoint_core
        .find("pub(crate) fn revoke_public_owner(")
        .expect("revoke_public_owner must exist");
    let revoke_body = &endpoint_core[revoke_start
        ..revoke_start
            + endpoint_core[revoke_start..]
                .find("\n    }\n}\n\nimpl<'r, const ROLE")
                .expect("revoke_public_owner body must be bounded")];
    for forbidden in ["clear_session_waiter(", "terminal_clear_public_send_state("] {
        assert!(
            !revoke_body.contains(forbidden),
            "topology revocation must not call public terminal helpers that re-enter cluster mutation: {forbidden}"
        );
    }
}

#[test]
fn production_sources_do_not_retain_test_only_effect_or_offer_helpers() {
    let production = read_production_rs_tree("src");
    for forbidden in [
        "for_test",
        "CpCommand",
        "PendingEffect",
        "EffectRunner",
        "DelegateOperands",
        "struct EffectEnvelope {",
        "enum EffectEnvelopeSource",
        "control_op_is_idempotent",
        "control_op_requires_gen_bump",
        "control_op_is_terminal",
        "control_op_modifies_history",
        "emit_policy_event_with_arg2",
        "run_effect_step",
        "after_local_effect",
        "PendingCapRelease::inert",
        "pub(crate) fn inert() -> Self",
        "pub(crate) fn disarm(&mut self)",
        "PolicyEventSpec",
        "PolicyEventKind",
        "TapEvents",
        "TEST_GLOBAL_TAP_RING",
        "TS_CHECKER",
        "install_ts_checker",
    ] {
        assert!(
            !production.contains(forbidden),
            "production sources must not retain repo-test effect runners or for-test escape hatches: {forbidden}"
        );
    }
}

#[test]
fn source_tree_does_not_retain_impossible_test_only_fixtures() {
    let source = read_all_rs_tree("src");
    for forbidden in [
        "CpCommand",
        "PendingEffect",
        "EffectRunner",
        "DelegateOperands",
        "run_effect_step",
        "after_local_effect",
        "dispatch_topology_ack_with_handle",
        "synthetic_for_test",
        "transport_for_test",
        "NonNull::dangling",
        "receipt: None",
    ] {
        assert!(
            !source.contains(forbidden),
            "source tests must not retain test-only effect runners or impossible transport fixtures: {forbidden}"
        );
    }
}

#[test]
fn package_artifact_does_not_ship_repo_integration_tests() {
    let cargo = read("Cargo.toml");
    let package_gate = read(".github/scripts/check_package_artifact.sh");

    assert!(
        !cargo.contains("autotests")
            && !cargo.contains("[[test]]")
            && !cargo.contains("\"/tests/**\"")
            && package_gate.contains("repo integration tests must not ship")
            && package_gate.contains("'^tests/'"),
        "repo integration tests must stay auto-discovered locally and absent from the production crate package"
    );
    assert!(
        package_gate
            .contains("run_package_allowing_omitted_repo_tests \"cargo package --no-verify\"")
            && package_gate.contains("package test build --features std")
            && package_gate.contains("cargo +\"${TOOLCHAIN}\" test --manifest-path"),
        "package artifact gate must whitelist only Cargo's omitted repo-test warnings and compile the packaged test target"
    );
}

#[test]
fn decode_failure_completion_is_terminal_without_branch_restore() {
    let endpoint = endpoint_facade_source();
    let decode = read("src/endpoint/kernel/decode.rs");

    assert!(
        !endpoint.contains("core::hint::black_box") && !decode.contains("core::hint::black_box"),
        "decode terminal cleanup must not rely on black_box to hide branch ownership"
    );
    assert!(
        !endpoint.contains("unsafe fn begin_public_decode_state(&mut self) -> RecvResult<()>"),
        "begin_public_decode_state must not expose a dead Result"
    );

    assert!(
        read("tests/no_policy_route_transport_hint.rs")
            .contains("completed decode future must fail fast on post-Ready poll"),
        "decode terminal paths must be guarded by behavior coverage, not private cleanup helper names"
    );
}

#[test]
fn offer_transport_payload_presence_is_not_length_sentinel() {
    let offer = offer_frontier_source();
    let offer_ingress = read("src/endpoint/kernel/offer/ingress.rs");
    let offer_materialization = read("src/endpoint/kernel/offer/materialization.rs");
    let offer_state = read("src/endpoint/kernel/offer/state.rs");
    let core = read("src/endpoint/kernel/core.rs");

    for forbidden in [
        "transport_payload_len",
        "transport_payload_lane",
        "binding_evidence: [Option<LaneIngressEvidence>; 2]",
        "transport_payload: [Option<",
    ] {
        assert!(
            !offer.contains(forbidden)
                && !offer_ingress.contains(forbidden)
                && !offer_materialization.contains(forbidden)
                && !offer_state.contains(forbidden),
            "offer preview staging must not resurrect stale sentinel or anonymous rollback storage: {forbidden}"
        );
    }
    assert!(
        !offer.contains("!payload.as_bytes().is_empty()")
            && !offer_ingress.contains("!payload.as_bytes().is_empty()")
            && !offer_materialization.contains("!payload.as_bytes().is_empty()"),
        "offer preview staging must keep zero-length transport payloads as real consumed frames"
    );
    assert!(
        !core.contains("for (len, lane, _payload) in rollback.transport_payload"),
        "offer rollback must not hide ingress ownership in tuple mini-vec iteration"
    );
}
