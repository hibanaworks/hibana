use super::common::*;

fn cursor_scope_route_source() -> String {
    let mut source = read("src/global/typestate/cursor/scope_route.rs");
    source.push_str(&read(
        "src/global/typestate/cursor/scope_route/event_flow.rs",
    ));
    source.push_str(&read(
        "src/global/typestate/cursor/scope_route/navigation.rs",
    ));
    source
}

fn runtime_types_source() -> String {
    let mut source = read("src/endpoint/kernel/core/runtime_types.rs");
    source.push_str(&read("src/endpoint/kernel/core/runtime_types/commit.rs"));
    source
}

fn repo_file_exists(path: &str) -> bool {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(path)
        .exists()
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
fn repo_test_support_modules_are_not_orphaned() {
    fn collect_rs_files(dir: &std::path::Path, files: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir)
            .unwrap_or_else(|err| panic!("read {} failed: {err}", dir.display()))
        {
            let path = entry
                .unwrap_or_else(|err| panic!("read dir entry in {} failed: {err}", dir.display()))
                .path();
            if path.is_dir() {
                collect_rs_files(&path, files);
            } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
                files.push(path);
            }
        }
    }

    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let tests_root = root.join("tests");
    let support_root = tests_root.join("support");
    let tests_source = read_all_rs_tree("tests");
    let mut support_files = Vec::new();
    collect_rs_files(&support_root, &mut support_files);
    support_files.sort();

    for path in support_files {
        let relative = path
            .strip_prefix(&tests_root)
            .expect("support file must be under tests")
            .to_string_lossy()
            .replace('\\', "/");
        let marker = format!("#[path = \"{relative}\"]");
        assert!(
            tests_source.contains(&marker),
            "repo test support module must be referenced by #[path] or deleted: {relative}"
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
        "add_rendezvous_auto",
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
fn package_artifact_ships_repo_integration_tests_without_publish_warning_filter() {
    let cargo = read("Cargo.toml");
    let package_gate = read(".github/scripts/check_package_artifact.sh");

    assert!(
        !cargo.contains("autotests")
            && !cargo.contains("[[test]]")
            && cargo.contains("\"/tests/**\"")
            && !package_gate.contains("repo integration tests must not ship")
            && !package_gate.contains("run_package_clean_with_omitted_repo_tests")
            && !package_gate.contains("ignoring test `"),
        "repo integration tests must stay Cargo-auto-discovered and ship with the crate so publish is warning-free"
    );
    assert!(
        package_gate.contains("run_package_clean \"cargo package --no-verify\"")
            && package_gate.contains("package test build --features std")
            && package_gate.contains("cargo +\"${TOOLCHAIN}\" test --manifest-path"),
        "package artifact gate must reject all package warnings and compile the packaged test target"
    );
}

#[test]
fn public_integration_tests_name_registered_rendezvous_witnesses() {
    let tests = read_all_rs_tree("tests");
    let stale = concat!("rv", "_id");

    assert!(
        !tests.contains(stale),
        "public integration tests must name registered rendezvous values as witnesses, not ids"
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
        read("tests/offer_decode_receive_evidence.rs")
            .contains("completed decode future must fail fast on post-Ready poll"),
        "decode terminal paths must be guarded by behavior coverage, not private cleanup helper names"
    );
}

#[test]
fn endpoint_dependency_guard_uses_local_dependency_facts() {
    let route_preview_flow = read("src/endpoint/kernel/core/route_preview_flow.rs");
    let recv = read("src/endpoint/kernel/recv.rs");
    let facts = read("src/global/typestate/facts.rs");
    let event_program = read("src/global/event_program.rs");
    let cursor = read("src/global/typestate/cursor.rs");
    let cursor_scope_route = cursor_scope_route_source();
    let descriptor_route_scope =
        read("src/global/compiled/images/image/role_descriptor_ref/route_scope.rs");
    let role_program_types = read("src/global/role_program/image_types.rs");
    let role_program_impl = read("src/global/role_program/image_impl.rs");
    let reference_tests = read("src/global/event_program_tests.rs");
    let dependency_guard = cursor_scope_route
        .split("pub(crate) fn event_dependency_allows")
        .nth(1)
        .and_then(|tail| tail.split("pub(crate) fn event_conflict_allows").next())
        .expect("event dependency guard must stay cursor-owned");
    let node_conflict_guard = cursor_scope_route
        .split("pub(crate) fn node_conflict_allows")
        .nth(1)
        .and_then(|tail| tail.split("pub(crate) fn event_conflict_row_allows").next())
        .expect("node conflict guard must stay cursor-owned");
    let event_conflict_guard = cursor_scope_route
        .split("pub(crate) fn event_conflict_allows")
        .nth(1)
        .and_then(|tail| {
            tail.split("pub(crate) fn recv_start_index_for_label")
                .next()
        })
        .expect("event conflict guard must stay cursor-owned");
    let selected_arm_membership_guard = cursor_scope_route
        .split("pub(crate) fn node_in_selected_route_arm")
        .nth(1)
        .and_then(|tail| tail.split("pub(crate) fn node_conflict_allows").next())
        .expect("selected route-arm membership guard must stay cursor-owned");
    let lane_head_guard = cursor_scope_route
        .split("pub(crate) fn event_lane_head_allows")
        .nth(1)
        .and_then(|tail| {
            tail.split("pub(crate) fn selected_route_scope_end_at")
                .next()
        })
        .expect("event lane-head guard must stay cursor-owned");
    let lane_progress = read("src/global/typestate/cursor/lane_progress.rs");
    let event_done_guard = lane_progress
        .split("pub(crate) fn relocatable_step_done")
        .nth(1)
        .and_then(|tail| {
            tail.split("pub(crate) fn node_index_for_relocatable_step")
                .next()
        })
        .expect("event done guard must stay cursor-owned");
    let event_row_match = cursor_scope_route
        .split("pub(crate) fn event_row_matches_commit")
        .nth(1)
        .and_then(|tail| {
            tail.split("pub(crate) fn enabled_event_allows_commit")
                .next()
        })
        .expect("event row match must stay cursor-owned");

    assert!(
        facts.contains("pub(crate) struct LocalDependency")
            && facts.contains("pub(crate) enum LocalConflict")
            && facts.contains("pub(crate) enum LocalDependencyState")
            && facts.contains("pub(crate) struct PackedEventConflict")
            && facts.contains("pub(crate) const fn to_conflict(self) -> Option<LocalConflict>"),
        "event dependency state must be represented as local descriptor/cursor facts"
    );
    assert!(
        event_program.contains("pub(crate) struct LocalEventProgram")
            && event_program.contains("pub(crate) struct LocalEventRow")
            && event_program.contains("pub(crate) fn event_row_at")
            && event_program.contains("pub(crate) fn matches_commit")
            && event_program.contains("dependency: Option<LocalDependency>")
            && event_program.contains("conflict: PackedEventConflict")
            && event_program.contains("pub(crate) const fn dependency")
            && event_program.contains("pub(crate) const fn conflict")
            && event_program.contains("pub(crate) fn event_conflict_for_index")
            && event_program.contains("pub(crate) fn route_scope_conflict_by_slot")
            && event_program.contains("self.event_row_at(idx).and_then(LocalEventRow::dependency)")
            && event_program.contains(".map(LocalEventRow::conflict)")
            && cursor.contains("event_program: LocalEventProgram")
            && cursor.contains("fn event_conflict_for_index")
            && cursor.contains("fn route_scope_conflict_by_slot")
            && !cursor.contains("fn scope_parent(")
            && !cursor.contains("fn route_parent(")
            && !cursor.contains("fn route_parent_arm(")
            && !cursor.contains("fn route_ancestor_arm(")
            && !cursor_scope_route.contains("pub(crate) fn scope_parent(")
            && !cursor_scope_route.contains("route_parent_scope(")
            && !cursor_scope_route.contains("route_parent_arm(")
            && !cursor_scope_route.contains("route_ancestor_arm(")
            && event_row_match.contains(".event_program()")
            && event_row_match.contains(".event_row_at(idx)")
            && event_row_match.contains("row.matches_commit(")
            && !event_program.contains("pub(crate) const fn role_descriptor")
            && !event_program.contains("pub(crate) fn scope_parent(")
            && !event_program.contains("pub(crate) fn route_parent(")
            && !event_program.contains("pub(crate) fn route_parent_arm(")
            && !event_program.contains("pub(crate) fn route_ancestor_arm(")
            && !cursor.contains("fn role_descriptor(")
            && !cursor.contains("fn role_descriptor_ref(")
            && !event_row_match.contains("try_send_meta_at")
            && !event_row_match.contains("try_recv_meta_at")
            && !event_row_match.contains("try_local_meta_at")
            && !event_program.contains("std::vec")
            && !event_program.contains("Vec<")
            && !event_program.contains("alloc::")
            && !event_program.contains("#[cfg(test)]"),
        "runtime event rows must be a no_alloc descriptor-backed LocalEventProgram, not endpoint-local send/recv topology branches"
    );
    assert!(
        facts.contains("conflict: LocalConflict")
            && facts.contains("pub(crate) const fn conflict(self) -> LocalConflict")
            && facts.contains("pub(crate) struct PackedLocalDependency")
            && role_program_types.contains("local_step_dependencies:")
            && role_program_types.contains("local_step_conflicts:")
            && role_program_types.contains("route_scope_conflicts:")
            && role_program_impl.contains("PackedLocalDependency::from_dependency(dependency)")
            && role_program_impl.contains("Self::route_conflict_for_eff(markers, idx)")
            && role_program_impl.contains("self.route_scope_conflicts[route_slot]")
            && descriptor_route_scope
                .contains(".role_image()\n            .dependency_for_index(current_idx)")
            && descriptor_route_scope
                .contains(".role_image()\n            .event_conflict_for_index(current_idx)")
            && descriptor_route_scope
                .contains(".role_image()\n            .route_scope_conflict_by_slot(slot)")
            && cursor_scope_route.contains("fn dependency_events_done")
            && cursor_scope_route.contains("pub(crate) fn event_conflict_row_allows")
            && !cursor_scope_route.contains("fn dependency_conflict")
            && dependency_guard.contains(".dependency_state_for_index(")
            && dependency_guard.contains(".allows_event()")
            && node_conflict_guard.contains("self.event_conflict_row_allows(")
            && node_conflict_guard.contains("self.machine().event_conflict_for_index(idx)")
            && event_conflict_guard.contains("self.event_conflict_row_allows(")
            && event_conflict_guard.contains("self.machine().event_conflict_for_index(idx)")
            && selected_arm_membership_guard
                .contains("self.event_conflict_row_contains_route_arm(")
            && selected_arm_membership_guard
                .contains("self.machine().event_conflict_for_index(idx)")
            && lane_head_guard.contains("self.event_conflict_allows(")
            && cursor_scope_route.contains("pub(crate) fn flow_preview_send_meta_for_label")
            && cursor_scope_route.contains(".event_dependency_allows(")
            && route_preview_flow.contains(".flow_preview_send_meta_for_label::<ROLE>(")
            && recv.contains(".event_dependency_allows(")
            && !route_preview_flow.contains(".event_dependency_allows(")
            && !route_preview_flow.contains("dependencies_complete_for_index")
            && !recv.contains("dependencies_complete_for_index")
            && !dependency_guard.contains("current_phase_eff_done(")
            && !lane_head_guard.contains("current_phase")
            && !lane_head_guard.contains("phase_index_usize(")
            && !lane_head_guard.contains("logical_lane_count()")
            && lane_progress.contains("self.mark_local_event_done(target.step_idx as usize);")
            && event_done_guard.contains("self.local_event_done(target.step_idx as usize)")
            && !event_done_guard.contains("phase_index_usize(")
            && !event_done_guard.contains("current_phase")
            && !event_done_guard.contains("target_phase"),
        "dependency conflict must be carried by resident dependency rows, and dependency progress must not be tied to the current phase"
    );
    for (name, guard) in [
        ("node_conflict_allows", node_conflict_guard),
        ("event_conflict_allows", event_conflict_guard),
        ("node_in_selected_route_arm", selected_arm_membership_guard),
    ] {
        for forbidden in [
            "scope_parent(",
            "route_parent_scope(",
            "route_parent_arm(",
            "route_ancestor_arm(",
            "node.scope()",
            "ScopeKind::Route",
        ] {
            assert!(
                !guard.contains(forbidden),
                "{name} must not re-grow route topology interpretation: {forbidden}"
            );
        }
    }
    for forbidden in [
        "route_parent_scope(",
        "route_parent_arm(",
        "dependency_conflict(",
        "ARM_SHARED",
        "ScopeKind::Route",
    ] {
        assert!(
            !dependency_guard.contains(forbidden),
            "endpoint dependency guard must not re-grow route ancestry interpretation: {forbidden}"
        );
    }
    for required in [
        "route_unselected_nested_parallel_arm_is_dead_not_join_obligation",
        "outer_left_selection_excludes_nested_right_route_and_parallel_events",
        "alternating_route_parallel_nesting_uses_only_selected_arms_for_joins",
    ] {
        assert!(
            reference_tests.contains(required),
            "reference event semantics must cover nested route/par conflict membership: {required}"
        );
    }
}

#[test]
fn old_route_apply_and_settlement_files_stay_deleted() {
    for path in [
        "src/endpoint/kernel/core/route_commit_apply.rs",
        "src/endpoint/kernel/core/route_commit_progress.rs",
        "src/endpoint/kernel/core/scope_settlement.rs",
        "src/endpoint/kernel/core/scope_path_progress.rs",
    ] {
        assert!(
            !repo_file_exists(path),
            "old topology path must stay deleted: {path}"
        );
    }
}

#[test]
fn production_sources_do_not_retain_route_apply_or_resident_settlement_paths() {
    let source = read_production_rs_tree("src");
    for forbidden in [
        "RouteCommitFacts",
        "RouteCommitScope",
        "ParentRouteCommit",
        "RouteCommitResidentProgress",
        "RouteCommitFutureProgress",
        "CommitApplyOutcome",
        "CursorSettlement",
        "scope_settlement",
        "scope_path_progress",
        "apply_route_commit_effects",
        "publish_commit_apply_outcome",
        "settle_after_event_commit",
        "settle_cursor_after_commit",
        "route_commit_has_pending",
        "scope_lane_first_eff_for_route_commit",
        "scope_lane_last_eff_for_route_commit",
        "clear_conflicting_route_state_for_other_lanes",
        "clear_descendant_route_state_for_lane",
        "prune_route_state_to_cursor_path_for_lane",
        "preflight_route_arm_commit_after_clearing_other_lanes",
        "node_matches_route_commit_arm",
        "cursor_at_active_route_offer_entry",
        "advance_scope_by_id_in_place",
        "lane_pending_step_belongs_to_scope",
        "control_parent_scope",
        "SEND_ROUTE_WAS_SELECTED",
        "clear_other_lanes",
        "SyntheticBranchCommitDelta",
        "PreparedSyntheticBranchCommitDelta",
        "EmptyBranchCommitDelta",
        "PreparedEmptyBranchCommitDelta",
        "apply_synthetic_branch_commit_delta",
        "apply_empty_branch_commit_delta",
        "prepare_synthetic_branch_commit_delta",
        "prepare_empty_branch_commit_delta",
        "selected_branch_event_row_matches_commit",
        "CurrentResidentLaneStep",
        "current_resident_lane_step",
        "advance_lane_to_current_step",
        "advance_lane_cursor_to_current_step",
        "relocatable_resident_lane_step(",
        "set_lane_cursor_to_relocatable_eff_index",
        "step_for_eff_index",
        "scope_lane_first_eff",
        "passive_authority_from_frame_hint",
        "PassiveRouteAuthority::StaticPoll",
        "static_passive_dispatch_arm_from_exact_frame_label",
        "scope_frame_label_to_arm",
        "fn route_frame_label",
        "fn route_lane",
        "recover_scope_evidence_conflict",
        "recovers_frame_hint_conflict",
        "clear_scope_frame_hint_conflict",
        "clear_frame_hint_conflict",
        "scope_frame_hint_conflicted",
        "frame_hint_conflicted",
        "scope_ack_conflicted",
        "fn ack_conflicted",
        "SelfSendController",
        "self_send_controller",
        "OfferControllerArmEntry",
        "PhaseCursor",
        "PhaseCursorState",
        "phase_cursor",
        "phase_index_usize",
        "select_phase_for_lane",
        "reference_event",
    ] {
        assert!(
            !source.contains(forbidden),
            "production source must not re-grow old route apply or resident settlement path: {forbidden}"
        );
    }
}

#[test]
fn route_selection_keeps_descriptor_facts_without_endpoint_cleanup_fallback() {
    let cursor_scope_route = cursor_scope_route_source();
    let route_preview = read("src/endpoint/kernel/core/route_preview.rs");
    let send_ops = read("src/endpoint/kernel/core/send_ops.rs");
    let offer_commit = read("src/endpoint/kernel/offer/commit.rs");
    let decode_finish = read("src/endpoint/kernel/decode/finish.rs");
    let runtime_types = runtime_types_source();
    let commit_delta = read("src/endpoint/kernel/core/commit_delta.rs");

    assert!(
        cursor_scope_route.contains("pub(crate) fn route_scope_for_selected_child_arm")
            && cursor_scope_route.contains("pub(crate) fn node_in_selected_route_arm")
            && cursor_scope_route.contains("pub(crate) fn selected_route_label_index")
            && runtime_types.contains("pub(crate) struct SelectedRouteCommitRow")
            && route_preview
                .contains("pub(in crate::endpoint::kernel) fn prepare_selected_route_commit_row")
            && !route_preview.contains("fn record_prepared_route_selection")
            && !route_preview.contains("fn apply_selected_route_commit_row")
            && commit_delta.contains("struct CommitDeltaApplyPermit")
            && commit_delta.contains("CommitDeltaApplyPermit::new()")
            && commit_delta.contains(".apply_prepared_route_selection(row,")
            && commit_delta.contains("fn apply_prepared_selected_route_commit_row")
            && send_ops.contains("prepare_event_selected_route_commit_row_from_parts")
            && send_ops.contains(".with_selected_route_rows(route_rows)")
            && !send_ops.contains(".route_scope_for_selected_child_arm(")
            && offer_commit.contains("prepare_event_selected_route_commit_row_from_parts")
            && !offer_commit.contains("self.record_prepared_route_selection(")
            && !offer_commit.contains("self.apply_selected_route_commit_row("),
        "route selection must preflight a self-contained commit row and leave route-state application inside prepared commit deltas"
    );
    for (name, body) in [
        ("route-preview", route_preview.as_str()),
        ("send", send_ops.as_str()),
        ("offer-commit", offer_commit.as_str()),
        ("runtime-types", runtime_types.as_str()),
    ] {
        for forbidden in [
            "preflight_route_arm_commit",
            "commit_route_arm_after_preflight",
            "RouteArmCommitProof",
            "RouteCommitProofWorkspace",
            "route_commit_proofs",
            "preflight_route_arm_commit_after_clearing_other_lanes",
            "clear_conflicting_route_state",
            "pop_route_arm",
            "lane_route_arm_for",
            "last_lane_scope",
            "active_route_lanes()",
        ] {
            assert!(
                !body.contains(forbidden),
                "{name} must not re-grow route-state cleanup fallback: {forbidden}"
            );
        }
    }
    for forbidden in ["route_ancestor_arm", "scope_parent("] {
        assert!(
            !decode_finish.contains(forbidden),
            "decode publish/preflight must not re-grow endpoint-side route ancestry walk: {forbidden}"
        );
    }
}

#[test]
fn send_recv_decode_publish_paths_are_commit_delta_apply_only() {
    let send_ops = read("src/endpoint/kernel/core/send_ops.rs");
    let recv = read("src/endpoint/kernel/recv.rs");
    let finish = read("src/endpoint/kernel/decode/finish.rs");
    let commit_delta = read("src/endpoint/kernel/core/commit_delta.rs");
    let runtime_types = runtime_types_source();
    let route_preview = read("src/endpoint/kernel/core/route_preview.rs");
    let offer_refresh = read("src/endpoint/kernel/core/offer_refresh.rs");

    assert!(
        runtime_types.contains("pub(crate) struct PreparedCommitDelta")
            && !runtime_types.contains("fn from_enabled(")
            && runtime_types.contains("fn with_loop_row(")
            && runtime_types.contains("fn with_lane_relocation(")
            && commit_delta.contains("pub(in crate::endpoint::kernel) fn commit_prepared_delta")
            && commit_delta.contains("fn apply_prepared_cursor_index(")
            && commit_delta.contains("fn apply_prepared_lane_advance(")
            && commit_delta.contains("fn apply_prepared_lane_relocation(")
            && commit_delta.contains("self.apply_loop_commit_row(")
            && !route_preview.contains("fn set_cursor_index(")
            && !offer_refresh.contains("fn set_lane_cursor_to_relocatable_step(")
            && !offer_refresh.contains("fn advance_lane_cursor_to_relocatable_step(")
            && !commit_delta.contains("apply_route_commit_effects")
            && !commit_delta.contains("settle_cursor_after_commit"),
        "CommitDelta must be the only cursor/route/loop mutation boundary"
    );

    for (name, source, marker) in [
        (
            "send",
            send_ops.as_str(),
            "self.commit_prepared_delta(plan.delta);",
        ),
        ("recv", recv.as_str(), "self.commit_prepared_delta(delta);"),
        (
            "decode",
            finish.as_str(),
            "self.commit_prepared_delta(delta);",
        ),
    ] {
        assert!(
            source.contains(marker),
            "{name} must apply a prepared delta"
        );
        for forbidden in [
            "publish_commit_apply_outcome",
            "apply_route_commit_effects",
            "settle_after_event_commit",
            "maybe_advance_phase",
            "ScopeSettlement",
            "CommitApplyOutcome",
            "apply_synthetic_branch_commit_delta",
            "apply_empty_branch_commit_delta",
            "prepare_synthetic_branch_commit_delta",
            "prepare_empty_branch_commit_delta",
        ] {
            assert!(
                !source.contains(forbidden),
                "{name} must not re-grow scattered cursor mutation: {forbidden}"
            );
        }
    }
}

#[test]
fn offer_and_frontier_do_not_call_resident_settlement_primitives() {
    let offer_refresh = read("src/endpoint/kernel/core/offer_refresh.rs");
    let offer_select = read("src/endpoint/kernel/offer/select.rs");
    let frontier_select = read("src/endpoint/kernel/core/frontier_select.rs");
    let frontier_helpers = read("src/endpoint/kernel/core/frontier_helpers.rs");
    let cursor = read("src/global/typestate/cursor.rs");
    let cursor_lane_progress = read("src/global/typestate/cursor/lane_progress.rs");
    let endpoint_kernel = endpoint_kernel_source();

    assert!(
        offer_refresh.contains(".selected_arm_for_scope(")
            && offer_select.contains(".route_scope_for_offer_node(")
            && offer_select.contains(".route_offer_entry_allows_current(")
            && offer_select.contains(".route_scope_present_for_entry(")
            && !offer_select.contains(".route_scope_region_by_id(")
            && !offer_select.contains(".route_scope_region_at(")
            && !frontier_select.contains("align_cursor_to_lane_progress")
            && !frontier_select.contains("first_pending_step_index("),
        "offer/frontier still use cursor facts for selected arms and event frontier metadata"
    );
    for forbidden in [
        ".route_scope_region_by_id(",
        ".route_scope_region_at(",
        ".passive_arm_scope_by_arm(",
    ] {
        assert!(
            !endpoint_kernel.contains(forbidden),
            "endpoint kernel must not read raw route topology directly: {forbidden}"
        );
    }
    for (name, body) in [
        ("frontier-helpers", frontier_helpers.as_str()),
        ("cursor-lane-progress", cursor_lane_progress.as_str()),
    ] {
        for forbidden in [
            "settle_after_event_commit",
            "settle_after_completed_resident_set",
            "current_phase_live_lanes_complete",
            "cursor_index_is_current_phase_resident_step",
            "cursor_at_active_route_offer_entry",
            "advance_phase_without_sync",
        ] {
            assert!(
                !body.contains(forbidden),
                "{name} must not re-grow resident-set settlement correctness: {forbidden}"
            );
        }
    }
    let resident_lane_step = cursor
        .split("struct ResidentLaneStep")
        .nth(1)
        .and_then(|tail| {
            tail.split("pub(crate) struct RelocatableResidentLaneStep")
                .next()
        })
        .expect("ResidentLaneStep must stay visible");
    let token_factory = cursor_lane_progress
        .split("pub(crate) fn relocatable_resident_lane_step_at_index")
        .nth(1)
        .and_then(|tail| {
            tail.split("    #[inline(always)]\n    fn select_resident_row_for_lane")
                .next()
        })
        .expect("relocatable token factory must stay visible");
    let token_lookup = cursor_lane_progress
        .split("pub(crate) fn node_index_for_relocatable_step")
        .nth(1)
        .and_then(|tail| tail.split("    /// Position a lane").next())
        .expect("relocatable token lookup must stay visible");
    assert!(
        resident_lane_step.contains("step_idx: u16")
            && resident_lane_step.contains("lane: u8")
            && !resident_lane_step.contains("phase")
            && !resident_lane_step.contains("ordinal")
            && cursor_lane_progress.contains("resident_lane_step_locator(")
            && cursor_lane_progress.contains("fn event_lane_step_matches(")
            && token_factory.contains("event_lane_step_matches(step_idx, lane_idx)")
            && !token_factory.contains("resident_lane_step_locator(")
            && token_lookup.contains("event_lane_step_matches(target.step_idx as usize")
            && !token_lookup.contains("resident_lane_step_locator(")
            && cursor_lane_progress.contains("self.local_event_done(target.step_idx as usize)"),
        "resident lane progress tokens must carry event/lane identity, not cached phase/ordinal correctness"
    );
}

#[test]
fn recvless_parent_route_decision_is_cursor_fact_not_route_apply_effect() {
    let facts = read("src/global/typestate/facts.rs");
    let cursor_scope_route = cursor_scope_route_source();
    let core = read("src/endpoint/kernel/core.rs");

    assert!(
        facts.contains("pub(crate) struct RecvlessParentRouteDecision")
            && cursor_scope_route.contains("pub(crate) fn recvless_parent_route_decision")
            && core.contains("fn build_recvless_parent_route_decision_plan")
            && core.contains(".recvless_parent_route_decision("),
        "recvless parent route decision may remain as a cursor descriptor fact"
    );
    for forbidden in [
        "route_commit_apply",
        "apply_parent_route_commit_effects",
        "parent_route_commit",
        "RouteCommitScope",
    ] {
        assert!(
            !core.contains(forbidden) && !facts.contains(forbidden),
            "recvless parent route decision must not depend on route apply facts: {forbidden}"
        );
    }
}
