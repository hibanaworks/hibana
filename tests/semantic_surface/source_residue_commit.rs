use super::common::*;

fn repo_file_exists(path: &str) -> bool {
    std::path::PathBuf::from(option_env!("HIBANA_REPO_ROOT").unwrap_or(env!("CARGO_MANIFEST_DIR")))
        .join(path)
        .exists()
}

fn runtime_types_source() -> String {
    let mut source = read("src/endpoint/kernel/core/runtime_types.rs");
    source.push_str(&read("src/endpoint/kernel/core/runtime_types/commit.rs"));
    source
}

fn cursor_scope_route_source() -> String {
    let mut source = read("src/global/typestate/cursor/scope_route.rs");
    source.push_str(&read(
        "src/global/typestate/cursor/scope_route/event_progress.rs",
    ));
    source.push_str(&read(
        "src/global/typestate/cursor/scope_route/navigation.rs",
    ));
    source.push_str(&read(
        "src/global/typestate/cursor/scope_route/row_completion.rs",
    ));
    source
}

#[test]
fn route_commit_apply_and_progress_files_stay_forbidden() {
    for path in [
        "src/endpoint/kernel/core/route_commit_apply.rs",
        "src/endpoint/kernel/core/route_commit_progress.rs",
        "src/endpoint/kernel/core/scope_settlement.rs",
        "src/endpoint/kernel/core/scope_path_progress.rs",
    ] {
        assert!(
            !repo_file_exists(path),
            "forbidden route/settlement file must stay forbidden: {path}"
        );
    }
}

#[test]
fn recvless_parent_route_arm_selection_stays_forbidden() {
    let facts = read("src/global/typestate/facts.rs");
    let cursor_scope_route = cursor_scope_route_source();
    let core = read("src/endpoint/kernel/core.rs");
    let offer_profile = read("src/endpoint/kernel/offer/profile.rs");

    assert!(
        !facts.contains("pub(crate) struct RecvlessParentRouteArm")
            && !cursor_scope_route.contains("pub(crate) fn recvless_parent_route_arm_selection")
            && !core.contains("fn build_recvless_parent_route_arm_selection_plan")
            && !core.contains(".recvless_parent_route_arm_selection(")
            && !offer_profile.contains("publishes_recvless_parent_route_arm_selection"),
        "recvless parent route decisions must not reappear as a route selection special case"
    );
    for forbidden in
        "route_commit_apply apply_parent_route_commit_effects parent_route_commit RouteCommitScope"
            .split_whitespace()
    {
        assert!(
            !core.contains(forbidden) && !facts.contains(forbidden),
            "recvless parent route decision must not depend on route apply facts: {forbidden}"
        );
    }
}

#[test]
fn production_sources_do_not_contain_route_apply_or_settlement_vocabularies() {
    let source = read_production_rs_tree("src");
    for forbidden in [
        "RouteCommitFacts",
        "RouteCommitScope",
        "ParentRouteCommit",
        "RouteCommitResidentProgress",
        "RouteCommitFutureProgress",
        "CommitApplyOutcome",
        "CursorSettlement",
        "apply_route_commit_effects",
        "route_commit_has_pending",
        "scope_lane_first_eff_for_route_commit",
        "scope_lane_last_eff_for_route_commit",
        "settle_after_event_commit",
        "settle_cursor_after_commit",
        "clear_conflicting_route_state_for_other_lanes",
        "clear_descendant_route_state_for_lane",
        "prune_route_state_to_cursor_path_for_lane",
        "preflight_route_arm_commit_after_clearing_other_lanes",
        "node_matches_route_commit_arm",
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
        "passive_arm_jump",
        "passive_dispatch_arm_from_exact_frame_label",
        "static_passive_dispatch_arm_from_exact_frame_label",
        "static_passive_descendant_dispatch_arm_from_exact_frame_label",
        "scope_frame_label_to_arm",
        "scope_evidence_frame_label_to_arm",
        "_semantics: &ControlSemanticsTable",
        "current_recv_is_scope_local",
        "ControlSemanticsTable",
        "CONTROL_SEMANTICS_TABLE",
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
    ] {
        assert!(
            !source.contains(forbidden),
            "production source must not re-grow route apply/settlement repair path: {forbidden}"
        );
    }
}

#[test]
fn send_recv_branch_recv_publish_paths_apply_prepared_deltas_only() {
    let send_ops = read("src/endpoint/kernel/core/send_ops.rs");
    let recv_commit_plan_source = read("src/endpoint/kernel/recv_commit_plan.rs");
    let branch_recv_builder = read("src/endpoint/kernel/branch_recv/finish.rs");
    let select = read("src/endpoint/kernel/core/decision_resolver/impls/select.rs");
    let offer_select = read("src/endpoint/kernel/offer/select.rs");
    let select_alignment = read("src/endpoint/kernel/offer/select_alignment.rs");
    let commit_delta = read("src/endpoint/kernel/core/commit_delta.rs");
    let commit_delta_apply = read("src/endpoint/kernel/core/commit_delta_apply.rs");
    let decision_state = read("src/endpoint/kernel/decision_state.rs");
    let route_preview = read("src/endpoint/kernel/core/route_preview.rs");
    let offer_refresh = read("src/endpoint/kernel/core/offer_refresh.rs");
    let endpoint_layout = read("src/endpoint/kernel/layout.rs");
    let endpoint_init = read("src/endpoint/kernel/endpoint_init.rs");
    let runtime_types = runtime_types_source();

    let send_publish = send_ops
        .split("fn publish_send_progress_commit_plan")
        .nth(1)
        .and_then(|tail| tail.split("fn preflight_send_cursor_after_preview").next())
        .expect("send progress publish must stay factored");
    let recv_publish = recv_commit_plan_source
        .split("fn publish_recv_commit_plan<F>")
        .nth(1)
        .and_then(|tail| tail.split("\n    }\n}").next())
        .expect("recv publish must stay factored");
    let lane_relocation_preflight = commit_delta
        .split("fn preflight_lane_relocation")
        .nth(1)
        .and_then(|tail| tail.split("    #[inline(always)]").next())
        .expect("lane relocation preflight must stay factored");
    let selected_route_row = decision_state
        .split("pub(crate) struct SelectedRouteCommitRow")
        .nth(1)
        .and_then(|tail| tail.split("impl SelectedRouteCommitRow").next())
        .expect("SelectedRouteCommitRow must stay visible");
    let commit_delta_row = runtime_types
        .split("pub(crate) struct CommitDelta")
        .nth(1)
        .and_then(|tail| tail.split("impl CommitDelta").next())
        .expect("CommitDelta must stay visible");
    let prepared_commit_delta_row = commit_delta
        .split("pub(crate) struct PreparedCommitDelta")
        .nth(1)
        .and_then(|tail| tail.split("impl PreparedCommitDelta").next())
        .expect("PreparedCommitDelta must stay visible");
    let event_chain_preflight = commit_delta
        .split("fn preflight_event_selected_route_chain(")
        .nth(1)
        .and_then(|tail| tail.split("    #[inline]").next())
        .expect("event route-chain preflight must stay factored");
    let forbidden_from_chain_for_lane = "from_conflict_chain_for_lane";

    assert!(
        commit_delta.contains("pub(crate) struct PreparedCommitDelta")
            && decision_state.contains("pub(crate) struct PreparedRouteCommitRows")
            && decision_state.contains("pub(in crate::endpoint::kernel) fn seal(")
            && !decision_state.contains("fn release_sealed(")
            && !commit_delta.contains("MAX_ROUTE_COMMIT_ROWS")
            && commit_delta.contains("fn from_preflighted(")
            && commit_delta.contains("fresh_route_start: Option<usize>")
            && !commit_delta.contains("pub(in crate::endpoint::kernel) const fn from_preflighted")
            && prepared_commit_delta_row.contains("event: Option<CommitEventRow>")
            && prepared_commit_delta_row.contains("selected_routes: PreparedRouteCommitRows")
            && prepared_commit_delta_row.contains("fresh_route_start: u16")
            && !prepared_commit_delta_row.contains("roll_row: RollCommitRow")
            && !prepared_commit_delta_row.contains("delta: CommitDelta")
            && !commit_delta.contains("pub(crate) const fn delta(")
            && !runtime_types.contains("struct SendRouteEvidencePlan")
            && !runtime_types.contains("pub(crate) struct ParentRouteEvidenceRow")
            && !runtime_types.contains("pub(crate) struct PreparedCommitDelta")
            && !runtime_types.contains("struct SelectedRouteCommitRowsRef")
            && !runtime_types.contains("struct RouteOnlyCommitRowsRef")
            && decision_state.contains("struct SelectedRouteCommitRowsRef")
            && decision_state.contains("struct RouteOnlyCommitRowsRef")
            && !decision_state.contains("ptr: *const SelectedRouteCommitRow")
            && !endpoint_layout.contains("route_state_commit_rows")
            && decision_state.contains("conflict: PackedEventConflict")
            && decision_state.contains("range: PackedLaneRange")
            && decision_state.contains("lane: u8")
            && decision_state.contains("from_resident_range_for_lane")
            && !decision_state.contains(forbidden_from_chain_for_lane)
            && !decision_state.contains("route_commit_chain_row_at")
            && !runtime_types.contains("from_inline_with_parent_route_evidence")
            && !runtime_types.contains("fn from_inline(")
            && !runtime_types.contains("SendRouteCommitPlan")
            && !runtime_types.contains("fn from_enabled(")
            && !runtime_types.contains("fn with_roll_row(")
            && runtime_types.contains("fn with_lane_relocation(")
            && runtime_types.contains("fn selected_routes(")
            && runtime_types.contains("fn cursor_only(")
            && runtime_types.contains("fn from_meta(")
            && runtime_types.contains("fn from_recv_meta(")
            && runtime_types.contains("selected_routes: SelectedRouteCommitRowsRef,")
            && !runtime_types.contains("fn with_selected_route_rows(")
            && decision_state.contains("range: PackedLaneRange")
            && !decision_state.contains("fn from_slice_for_lane(")
            && endpoint_init.contains("role_descriptor.max_route_commit_count(),")
            && !endpoint_init.contains("max_route_commit_count().max(1)")
            && !endpoint_init.contains("route_scope_count().saturating_add(1)",)
            && runtime_types.contains("fn route_rows(rows: RouteOnlyCommitRowsRef")
            && !runtime_types.contains("fn route_rows(rows: SelectedRouteCommitRowsRef")
            && !commit_delta_row.contains("route_lane")
            && decision_state.contains("fn finish_route_only_for_lane(")
            && decision_state.contains("fn finish_for_lane(")
            && decision_state.contains("return Err(RecvError::PhaseInvariant);")
            && !decision_state.contains("fn as_commit_rows(")
            && !decision_state.contains("fn as_route_only_commit_rows(")
            && commit_delta.contains(".route_commit_rows")
            && commit_delta.contains(".seal(delta.selected_route_rows_ref())")
            && commit_delta.contains("pub(in crate::endpoint::kernel) fn prepare_commit_delta")
            && commit_delta_apply
                .contains("pub(in crate::endpoint::kernel) fn commit_prepared_delta")
            && commit_delta.contains("fn preflight_event_selected_route_chain(")
            && commit_delta.contains("event_conflict_for_index(event_idx)")
            && commit_delta.contains(".route_commit_range_for_conflict(")
            && commit_delta.contains(".route_commit_row_at(range, idx)")
            && !commit_delta.contains("route_scope_conflict_for_scope(scope)")
            && commit_delta_apply.contains(".get(&self.cursor, idx)")
            && commit_delta.contains("fn commit_cursor_realign_index(")
            && commit_delta.contains("struct CommitDeltaApplyPermit")
            && commit_delta_apply.contains("CommitDeltaApplyPermit::new()")
            && commit_delta.contains("let routes = delta.selected_routes();")
            && commit_delta_apply.contains("crate::invariant()")
            && !commit_delta_apply.contains("panic!(\"prepared route row missing\")")
            && !commit_delta_apply.contains("panic!(\"prepared route lane missing\")")
            && !commit_delta_apply.contains("panic!(\"prepared route scope missing\")")
            && !commit_delta_apply
                .contains("let _ = self.decision_state.apply_prepared_route_selection(")
            && commit_delta_apply.contains("fn apply_prepared_cursor_index(")
            && commit_delta_apply.contains("fn apply_prepared_lane_advance(")
            && commit_delta_apply.contains("fn apply_prepared_lane_relocation(")
            && !commit_delta_apply.contains("self.apply_loop_commit_row(")
            && !commit_delta_apply.contains("self.apply_roll_commit_row(")
            && !route_preview.contains("fn set_cursor_index(")
            && !offer_refresh.contains("fn set_lane_cursor_to_relocatable_step(")
            && !offer_refresh.contains("fn advance_lane_cursor_to_relocatable_step(")
            && !commit_delta.contains("apply_route_commit_effects")
            && !commit_delta_apply.contains("apply_route_commit_effects")
            && !commit_delta.contains("settle_cursor_after_commit")
            && !commit_delta_apply.contains("settle_cursor_after_commit")
            && !commit_delta_apply.contains("publish_commit_apply_outcome")
            && !commit_delta_apply.contains("record_prepared_route_selection"),
        "CommitDelta apply must be the only cursor mutation boundary"
    );
    assert!(
        !runtime_types.contains("struct SelectedRouteCommitRow")
            && decision_state.contains("struct SelectedRouteCommitRow")
            && decision_state.contains("conflict: PackedEventConflict")
            && !selected_route_row.contains("scope: CompactScopeId")
            && !selected_route_row.contains("selected_arm: u8")
            && !selected_route_row.contains("lane:")
            && !selected_route_row.contains("scope_slot:")
            && !selected_route_row.contains("flags:")
            && decision_state.contains("const fn new(scope: ScopeId, selected_arm: u8)")
            && !decision_state
                .contains("pub(crate) const fn new(scope: ScopeId, selected_arm: u8)")
            && !decision_state.contains(
                "pub(in crate::endpoint::kernel) const fn new(scope: ScopeId, selected_arm: u8)"
            ),
        "SelectedRouteCommitRow must be a route-state-owner-only canonical (route_scope, arm) row, not a runtime lane/slot/reentry record"
    );
    assert!(
        event_chain_preflight.contains("routes.len() != range.len()")
            && event_chain_preflight.contains(".route_commit_row_at(range, idx)")
            && !event_chain_preflight.contains("selected_route_chain_row_index")
            && !commit_delta.contains("fn selected_routes_contain("),
        "event route-chain preflight must validate exact selected route rows, not contains-only partial chains"
    );
    assert!(
        lane_relocation_preflight.contains(".node_index_for_relocatable_step(step)")
            && !lane_relocation_preflight.contains("resident_lane_step_locator(")
            && !lane_relocation_preflight.contains("phase_lane_step_ordinal(")
            && !lane_relocation_preflight.contains("resident_row_lane_step_ordinal("),
        "CommitDelta preflight must validate lane relocation through event/lane identity, not phase-row ancestry"
    );
    assert!(
        send_ops.contains("CommitDelta::from_meta(")
            && send_ops.contains("route_rows,")
            && !send_ops.contains("with_selected_route_rows")
            && !send_ops.contains("SendRouteCommitPlan")
            && !send_ops.contains("SendRouteEvidencePlan")
            && !send_ops.contains("build_send_route_commit_plan")
            && !send_ops.contains("publish_send_route_commit_plan")
            && !send_ops.contains("self.apply_selected_route_commit_row(route_row)")
            && !send_ops.contains("self.record_prepared_route_selection(route_row)"),
        "send route selection must be folded into the prepared CommitDelta, not applied by a side route commit path"
    );
    assert!(
        branch_recv_builder.contains("CommitDelta::from_recv_meta(")
            && branch_recv_builder.contains("route_rows.finish_for_lane(")
            && !branch_recv_builder.contains("with_selected_route_rows")
            && !branch_recv_builder.contains("apply_selected_route_commit_row")
            && !branch_recv_builder.contains("record_prepared_route_selection"),
        "branch-recv route rows must be carried by PreparedCommitDelta, not applied inside a side transaction"
    );
    assert!(
        select.contains("CommitDelta::route_rows(")
            && select.contains("finish_route_only_for_lane(")
            && select.contains("self.commit_prepared_delta(delta);")
            && !select.contains("apply_selected_route_commit_row")
            && !select.contains("record_prepared_route_selection"),
        "route materialization must commit selected rows through CommitDelta, not mutate route state directly"
    );
    let passive_materialization = select
        .split("fn descend_selected_passive_route")
        .nth(1)
        .and_then(|tail| {
            tail.split("    pub(in crate::endpoint::kernel) fn emit_route_arm_selection")
                .next()
        })
        .expect("passive route materialization must stay visible");
    let commit_pos = passive_materialization
        .find("self.commit_prepared_delta(delta);")
        .expect("passive route materialization must commit a prepared delta");
    let emit_pos = passive_materialization
        .find("self.emit_route_arm_selection(")
        .expect("passive route materialization may emit only after commit");
    assert!(
        commit_pos < emit_pos,
        "passive route materialization must not emit route selection before prepared CommitDelta commit"
    );
    let branch_recv_arm = recv_publish
        .split("RecvCommitPlanKind::Branch { branch } =>")
        .nth(1)
        .and_then(|tail| tail.split("        }").next())
        .expect("branch recv arm must stay inside shared recv commit publish");
    let branch_recv_commit_pos = branch_recv_arm
        .find("let committed = self.commit_prepared_delta(delta);")
        .expect("branch-recv publish must commit a prepared delta");
    let branch_recv_branch_publish_pos = branch_recv_arm
        .find("self.publish_branch_preview_commit_plan(branch, &committed);")
        .expect("branch-recv publish must publish branch preview");
    let branch_recv_event_publish_pos = branch_recv_arm
        .find("self.emit_endpoint_event(event.event_id, endpoint_meta, event.lane);")
        .expect("branch-recv publish must publish endpoint event");
    assert!(
        branch_recv_commit_pos < branch_recv_branch_publish_pos
            && branch_recv_commit_pos < branch_recv_event_publish_pos,
        "branch-recv route/evidence publish must happen after prepared CommitDelta commit"
    );
    assert!(
        offer_select.contains("commit_cursor_realign_index(")
            && select_alignment.contains("commit_cursor_realign_index(")
            && !offer_select.contains("self.set_cursor_index(")
            && !select_alignment.contains("self.set_cursor_index("),
        "offer cursor reentry must use CommitDelta cursor realignment, not direct endpoint cursor mutation"
    );

    for (name, body, required) in [
        (
            "send",
            send_publish,
            "let committed = self.commit_prepared_delta(plan.delta);",
        ),
        ("recv", recv_publish, "self.commit_prepared_delta(delta);"),
        (
            "branch-recv",
            branch_recv_arm,
            "let committed = self.commit_prepared_delta(delta);",
        ),
    ] {
        assert!(
            body.contains(required),
            "{name} publish path must apply the prepared delta directly"
        );
        for forbidden in [
            "publish_commit_apply_outcome",
            "apply_route_commit_effects",
            "settle_after_event_commit",
            "maybe_advance_phase",
            "clear_conflicting_route_state",
            "ScopeSettlement",
            "CommitApplyOutcome",
            "apply_synthetic_branch_commit_delta",
            "apply_empty_branch_commit_delta",
            "prepare_synthetic_branch_commit_delta",
            "prepare_empty_branch_commit_delta",
        ] {
            assert!(
                !body.contains(forbidden),
                "{name} publish path must not re-grow scattered cursor mutation: {forbidden}"
            );
        }
    }
}

#[test]
fn route_history_and_traversal_are_descriptor_derived() {
    let lowering_driver = lowering_driver_source();
    let lowering_seal = read("src/global/compiled/lowering/seal.rs");
    let passive_child_seal = read("src/global/compiled/lowering/seal/passive_child.rs");
    let first_recv_dispatch = read("src/global/typestate/cursor/first_recv_dispatch.rs");
    let decision_state = read("src/endpoint/kernel/decision_state.rs");
    let route_history = read("src/endpoint/kernel/decision_state/route_arm_history.rs");
    let endpoint_layout = read("src/endpoint/kernel/layout.rs");
    let cursor = read("src/global/typestate/cursor.rs");
    let conflict = read("src/global/typestate/facts.rs");
    assert!(
        !lowering_driver.contains("max_route_commit_count_for_projection")
            && !lowering_seal.contains("validate_route_stack_depth")
            && route_history.contains("struct RouteArmHistoryView")
            && route_history.contains("lane_lengths: *mut u16")
            && route_history.contains("capacity: u16")
            && route_history.contains("len: u16")
            && decision_state.contains("range: PackedLaneRange")
            && !route_history.contains("lane_reentry_counts")
            && !route_history.contains("range.len() > u8::MAX as usize")
            && endpoint_layout.contains("footprint.route_arm_state_capacity")
            && !endpoint_layout
                .contains("footprint.active_lane_count, footprint.max_route_commit_count")
            && cursor.contains("fn route_chain_bound(&self) -> usize")
            && cursor.contains(".route_scope_count")
            && !conflict.contains("MAX_CHAIN_DEPTH"),
        "route history must be sparse over emitted descriptor relations and traversal bounds must come from the route-scope count"
    );
    assert!(
        !lowering_seal.contains("validate_first_recv_dispatch_capacity")
            && !first_recv_dispatch.contains("MAX_FIRST_RECV_DISPATCH")
            && !first_recv_dispatch.contains("FirstRecvDispatchSpec")
            && !first_recv_dispatch.contains("[FirstRecvDispatch")
            && first_recv_dispatch.contains("visit_first_recv_dispatch(")
            && first_recv_dispatch.contains("footprint().route_scope_count")
            && first_recv_dispatch.contains("passive_arm_child_fact_by_slot")
            && passive_child_seal.contains("passive_child_route_scope("),
        "passive first-recv dispatch must stream from descriptor route-scope child-slot authority without an arbitrary fixed table"
    );
}

#[test]
fn route_history_publishes_shared_refs_after_sparse_commit() {
    let decision_state = read("src/endpoint/kernel/decision_state.rs");
    let reentry_clear = read("src/endpoint/kernel/decision_state/reentry_clear.rs");
    let apply = decision_state
        .split("pub(super) fn apply_prepared_route_selection")
        .nth(1)
        .and_then(|tail| {
            tail.split("pub(super) fn selected_arm_for_scope_slot")
                .next()
        })
        .expect("prepared route selection commit must stay factored");
    let history_set = apply
        .find("self.lane_route_arms.set(")
        .expect("existing route history must commit through the sparse owner");
    let history_push = apply
        .find("self.lane_route_arms.push(")
        .expect("new route history must commit through the sparse owner");
    let publications: Vec<_> = apply.match_indices(".write(next_slot);").collect();
    let direct_slot_mutation = apply
        .lines()
        .any(|line| line.trim_start().starts_with("slot."));

    assert!(
        apply.contains("let mut next_slot =")
            && publications.len() == 2
            && history_set < publications[0].0
            && history_push < publications[1].0
            && !apply.contains("let slot =")
            && !direct_slot_mutation,
        "route selection must prepare shared ref state locally and publish it only after sparse history commits"
    );

    let clear = reentry_clear
        .split("pub(in crate::endpoint::kernel) fn clear_lane_route_selections_in_scope")
        .nth(1)
        .and_then(|tail| tail.split("fn prepare_selected_arm_ref_release").next())
        .expect("route history clear must stay factored");
    let prepare_release = clear
        .find("prepare_selected_arm_ref_release(scope_slot)")
        .expect("shared ref release must be prepared before mutation");
    let remove = clear
        .find("lane_route_arms.remove(lane_idx, idx)")
        .expect("route history row must be removed through the sparse owner");
    let publish = clear
        .find("publish_selected_arm_slot(scope_slot, next_slot)")
        .expect("prepared shared ref state must be published");
    assert!(
        prepare_release < remove
            && remove < publish
            && !reentry_clear.contains("release_selected_arm_ref"),
        "route history removal must validate shared refs first and publish them only after sparse compaction"
    );
}

#[test]
fn offer_frontier_capacity_is_derived_from_active_lanes() {
    let role_image = read("src/global/role_program/image_types.rs");
    let lane_set = read("src/global/role_program/lane_set.rs");
    let entry_sets = read("src/endpoint/kernel/frontier/entry_sets.rs");
    let entry_buffer = read("src/endpoint/kernel/frontier/entry_sets/buffer.rs");
    let snapshot = read("src/endpoint/kernel/frontier/snapshot.rs");
    let layout = read("src/endpoint/kernel/layout.rs");
    let cache_refresh = read("src/endpoint/kernel/offer/cache_refresh.rs");
    let selection_pool = read("src/endpoint/kernel/offer/select_alignment/model/pool.rs");
    let observation = read("src/endpoint/kernel/frontier/observation.rs");
    let decision_state = read("src/endpoint/kernel/decision_state.rs");
    let route_arm_history = read("src/endpoint/kernel/decision_state/route_arm_history.rs");
    let evidence_store = read("src/endpoint/kernel/evidence_store.rs");
    let assoc_storage = read("src/rendezvous/association/storage.rs");

    assert!(
        role_image.contains("pub(crate) const fn frontier_entry_count(self) -> usize")
            && role_image.contains("self.active_lane_count")
            && role_image.contains("pub(crate) const fn frontier_visit_count(self) -> usize")
            && role_image.contains("pub(crate) const fn frontier_visit_capacity(")
            && role_image.contains("count.checked_add(1)")
            && role_image.contains("frontier_visit_capacity(self.frontier_entry_count())")
            && !role_image.contains("if self.active_lane_count == 0")
            && lane_set.contains("endpoint_lane_slot_count")
            && lane_set.contains("active_lane_count > endpoint_lane_slot_count")
            && !lane_set.contains("MIN_ENDPOINT_LANE_SLOTS")
            && !lane_set.contains("RESERVED_BINDING_LANES")
            && !role_image.contains("frontier_entry_count_for_route_depth")
            && !entry_sets.contains("FRONTIER_SLOT_MASK_BITS")
            && !entry_sets.contains("occupancy_mask")
            && !entry_sets.contains("controller_mask")
            && !entry_sets.contains("progress_mask")
            && !entry_sets.contains("ready_arm_mask")
            && entry_sets.contains("struct ActiveEntrySetBuilder")
            && entry_sets.contains("struct ObservedEntrySetBuilder")
            && entry_sets.contains("const fn seal(self) -> ActiveEntrySet")
            && entry_sets.contains("const fn seal(self) -> ObservedEntrySet")
            && entry_buffer.contains("const unsafe fn from_parts")
            && !entry_buffer.contains("#[derive(Clone, Copy)]\npub(super) struct EntryBuffer")
            && entry_buffer.contains("const fn into_view(self) -> EntryView<T>")
            && !entry_buffer.contains("const fn view(&self) -> EntryView<T>")
            && snapshot.contains("slots: *mut StateIndex")
            && snapshot.contains("visited.contains(candidate.entry.as_usize())")
            && snapshot.contains("if self.len >= self.capacity")
            && snapshot.contains("crate::invariant();")
            && !snapshot.contains(
                "#[derive(Clone, Copy, Debug, PartialEq, Eq)]\npub(crate) struct FrontierVisitSet"
            )
            && !lane_set.contains("#[derive(Clone, Copy, Debug)]\npub(crate) struct LaneSet {")
            && !decision_state.contains("#[derive(Clone, Copy)]\nstruct LaneOfferStateView")
            && !route_arm_history
                .contains("#[derive(Clone, Copy)]\npub(super) struct RouteArmHistoryView")
            && !evidence_store
                .contains("#[derive(Clone, Copy)]\npub(super) struct ScopeEvidenceTable")
            && !assoc_storage.contains("#[derive(Clone, Copy)]\nstruct AssocStorageParts")
            && !snapshot.contains("visited.contains(candidate.scope_id)")
            && layout.contains("frontier_visited_entries: EndpointArenaSection")
            && layout.contains("footprint.frontier_visit_count()")
            && !repo_file_exists("src/endpoint/kernel/offer/select_alignment/model/set.rs")
            && selection_pool.contains("while slot_idx < self.observed_entries.len()")
            && observation.contains("frontier_mask & !FrontierKind::ALL_BITS")
            && observation.contains("self.frontier_mask = frontier_mask;")
            && !observation.contains("frontier_mask & 0x0f")
            && cache_refresh.contains("crate::invariant_some(composed.insert_entry(entry_idx))")
            && cache_refresh.contains("composed.seal()"),
        "offer arbitration and exact entry-identity visits must stream the active-lane-derived frontier without a fixed mask or silent truncation"
    );
}

#[test]
fn descriptor_counts_are_not_reexpanded_by_runtime_fallbacks() {
    let role_descriptor = read("src/global/compiled/images/image/role_descriptor_ref.rs");
    let cursor = read("src/global/typestate/cursor.rs");
    let attach = read("src/session/cluster/core/endpoint_attach.rs");
    let endpoint_init = read("src/endpoint/kernel/endpoint_init.rs");
    let capacity = read("src/rendezvous/core/storage_layout/capacity.rs");

    assert!(
        !role_descriptor.contains("endpoint_lane_slot_count.max(1)")
            && !role_descriptor.contains("max(self.endpoint_lane_slot_count())")
            && !cursor.contains("endpoint_lane_slot_count.max(1)")
            && !attach.contains("logical_lane_count().max(1)")
            && !endpoint_init.contains("max_route_commit_count().max(1)")
            && capacity.contains("required_lane_slots == 0")
            && capacity.contains("required_assoc_slots == 0")
            && !capacity.contains("required_lane_slots.max(1)")
            && !capacity.contains("required_assoc_slots.max(1)"),
        "accepted descriptor counts must flow unchanged into runtime storage; zero is either exact or rejected, never silently expanded"
    );
}

#[test]
fn inbound_projection_identity_uses_the_compact_event_domain() {
    let selectors = read("src/global/const_dsl/endpoint_selectors.rs");

    assert!(
        selectors.contains("crate::eff::meta::COMPACT_EVENT_IDENTITY_CAPACITY")
            && selectors.contains("frame-label reuse remains independent")
            && !selectors.contains("0x00ff_ffff")
            && !selectors.contains("issued monotonically"),
        "projection evidence identity must share the descriptor event domain and must not retain obsolete monotonic-frame-label assumptions"
    );
}

#[test]
fn compact_state_and_route_reference_identities_fail_closed() {
    let facts = read("src/global/typestate/facts.rs");
    let decision_state = read("src/endpoint/kernel/decision_state.rs");
    let reselection = decision_state
        .split("fn commit_existing_lane_reselection")
        .nth(1)
        .expect("route reselection transition")
        .split("impl LaneOfferStateView")
        .next()
        .expect("route reselection owner body");

    assert!(
        facts.contains("if raw == u16::MAX")
            && facts.contains("if idx >= MAX_STATES")
            && reselection.contains("if self.refs != 1")
            && reselection.contains("self.arm = selected_arm")
            && !reselection.contains("self.refs = 1")
            && decision_state.contains("slot.commit_existing_lane_reselection(current.arm, arm)"),
        "present state identities must exclude the absent sentinel and route reselection must preserve the exact shared reference count"
    );
}

#[test]
fn lean_role_metadata_matches_production_capacity_semantics() {
    let topology = read("proofs/lean/Hibana/DescriptorTopology.lean");
    let descriptor = read("proofs/lean/Hibana/DescriptorImage.lean");
    let refinement = read("proofs/lean/Hibana/DescriptorRefinement.lean");
    let exporter = read("src/test_support/lean_proof_export/projection_certificate.rs");
    let complete_surface = [
        topology.as_str(),
        descriptor.as_str(),
        refinement.as_str(),
        exporter.as_str(),
    ]
    .join("\n");

    assert!(
        topology.contains("let logicalLaneCount := endpointLaneSlotCount")
            && topology.contains("canonical_logical_lane_count_is_exact_endpoint_span")
            && descriptor.contains("maxRouteCommitCount : Nat")
            && descriptor.contains("production_frontier_capacity_is_exact_active_lane_count")
            && !descriptor.contains("if activeLaneCount = 0 then 1")
            && refinement.contains("certificate.image.maxRouteCommitCount")
            && exporter.contains("maxRouteCommitCount := {}")
            && !complete_surface.contains("maxRouteStackDepth")
            && !complete_surface.contains("activeLaneCount + 2"),
        "Lean canonical metadata and the Rust proof exporter must use the exact descriptor lane span and route-commit capacity semantics"
    );
}

#[test]
fn branch_recv_progress_plan_no_longer_carries_route_cleanup_inputs() {
    let branch_recv = read("src/endpoint/kernel/branch_recv.rs");
    let branch_recv_finish = read("src/endpoint/kernel/branch_recv/finish.rs");
    let recv_commit_plan = read("src/endpoint/kernel/recv_commit_plan.rs");
    let runtime_types = read("src/endpoint/kernel/core/runtime_types.rs");
    let branch_recv_surface = [branch_recv.as_str(), branch_recv_finish.as_str()].join("\n");

    assert!(
        !repo_file_exists("src/endpoint/kernel/branch_recv/finish/commit_builder.rs")
            && !runtime_types.contains("BranchRecvRuntimeDesc")
            && !branch_recv_finish.contains("BranchRecvRuntimeDesc")
            && !branch_recv.contains("BranchRecvRuntimeDesc")
            && !runtime_types.contains("validate:")
            && !runtime_types.contains("fn validate_payload(")
            && !recv_commit_plan.contains("pub(super) struct BranchRecvCommitInput<'r>")
            && !recv_commit_plan.contains("pub(super) enum BranchRecvCommitDelta")
            && recv_commit_plan.contains("pub(super) enum RecvCommitPayload<'r>")
            && recv_commit_plan.contains("pub(super) struct RecvCommitPlan<'r>")
            && recv_commit_plan.contains("enum RecvCommitPlanKind")
            && !recv_commit_plan.contains("fn prepare_branch_recv_commit_plan(")
            && recv_commit_plan.contains("fn publish_recv_commit_plan<F>(")
            && recv_commit_plan.contains("frame.validated_payload(validate)")
            && recv_commit_plan.contains("frame.into_payload()")
            && recv_commit_plan.contains("let committed = self.commit_prepared_delta(delta);")
            && recv_commit_plan
                .contains("self.publish_branch_preview_commit_plan(branch, &committed);")
            && !branch_recv_surface.contains("struct DecodePublishPlan")
            && branch_recv_finish.contains("fn build_wire_branch_recv_commit_plan(")
            && branch_recv_finish.contains("fn build_non_wire_branch_recv_commit_plan(")
            && branch_recv_finish.contains("RecvCommitPlan::branch(")
            && branch_recv_finish.contains("RecvCommitPayload::wire(frame)")
            && branch_recv_finish.contains("RecvCommitPayload::non_wire(payload)")
            && branch_recv_finish.contains("let mut frame = Some(frame);")
            && branch_recv_finish.contains("if result.is_err()")
            && !branch_recv_surface.contains("with_branch_recv_commit_builder")
            && !branch_recv_surface.contains("BranchRecvCommitBuilder")
            && !branch_recv_surface.contains("WireBranchRecvCommitInput")
            && !branch_recv_surface.contains("struct RecvCommitPlan")
            && !branch_recv_surface.contains("enum BranchRecvCommitPayload")
            && !branch_recv_surface.contains("BranchRecvProgressPlan")
            && !branch_recv_surface.contains("PreparedBranchRecvProgressPlan")
            && !branch_recv_surface.contains("PreparedBranchRecvPublishPlan")
            && !branch_recv_surface.contains("frame.validated_payload")
            && !branch_recv_surface.contains("frame.into_payload()")
            && !branch_recv_finish.contains("fn publish_branch_recv_commit_plan(")
            && !branch_recv_surface.contains("PreparedSyntheticBranchCommitDelta")
            && !branch_recv_surface.contains("PreparedEmptyBranchCommitDelta"),
        "branch recv must use the shared RecvCommitPlan as its only commit authority"
    );
    let public_types = read("src/endpoint/kernel/core/public_types.rs");
    let offered_frame = public_types
        .split("pub(crate) struct OfferedFrame")
        .nth(1)
        .and_then(|tail| tail.split("#[derive(Clone, Copy)]").next())
        .expect("OfferedFrame must stay visible");
    assert!(
        !offered_frame.contains("validated_payload")
            && !offered_frame.contains("fn commit(")
            && offered_frame.contains("fn into_frame(")
            && offered_frame.contains("fn discard_terminal("),
        "offered branch frames must not expose validation or receipt-consume outside RecvCommitPlan"
    );
    for forbidden in ["route_ancestor_arm", "scope_parent("] {
        assert!(
            !branch_recv_finish.contains(forbidden),
            "branch-recv commit planning must not re-grow endpoint-side route ancestry walk: {forbidden}"
        );
    }
}

#[test]
fn offer_and_frontier_do_not_call_resident_settlement_primitives() {
    let offer_refresh = read("src/endpoint/kernel/core/offer_refresh.rs");
    let offer_select = read("src/endpoint/kernel/offer/select.rs");
    let frontier_select = read("src/endpoint/kernel/core/frontier_select.rs");
    let frontier_helpers = read("src/endpoint/kernel/core/frontier_helpers.rs");
    let cursor = read("src/global/typestate/cursor.rs");
    let cursor_scope_route = read("src/global/typestate/cursor/scope_route.rs");
    let cursor_route_navigation = read("src/global/typestate/cursor/scope_route/navigation.rs");
    let first_recv_dispatch = read("src/global/typestate/cursor/first_recv_dispatch.rs");
    let cursor_lane_progress = read("src/global/typestate/cursor/lane_progress.rs");
    let role_program_types = read("src/global/role_program/image_types.rs");
    let mut role_program_impl = read("src/global/role_program/image_impl.rs");
    role_program_impl.push_str(&read("src/global/role_program/image_impl/blob_image.rs"));
    role_program_impl.push_str(&read("src/global/role_program/image_impl/lane_image.rs"));
    role_program_impl.push_str(&read("src/global/role_program/image_impl/ref_access.rs"));
    let endpoint_kernel = endpoint_kernel_source();
    let current_offer_scope_id = cursor_route_navigation
        .split("pub(crate) fn current_offer_scope_id")
        .nth(1)
        .and_then(|tail| tail.split("    #[inline(always)]").next())
        .expect("current offer scope authority must stay factored");
    let passive_child_scope = cursor_scope_route
        .split("fn passive_child_scope_inner")
        .nth(1)
        .and_then(|tail| tail.split("    #[inline(always)]").next())
        .expect("passive child scope authority must stay factored");
    let passive_dispatch = cursor_route_navigation
        .split("pub(crate) fn passive_descendant_dispatch_arm_for_key")
        .nth(1)
        .and_then(|tail| {
            tail.split("    /// Check if this role is the controller for the given route scope.")
                .next()
        })
        .expect("passive dispatch must stay factored");
    let passive_rebase = cursor_route_navigation
        .split("pub(crate) fn rebase_passive_descendant_scope")
        .nth(1)
        .and_then(|tail| tail.split("\n    }\n}").next())
        .expect("passive descendant rebase must stay factored");

    assert!(
        offer_refresh.contains(".selected_arm_for_scope(")
            && offer_select.contains(".route_scope_for_offer_node(")
            && offer_select.contains(".route_offer_entry_allows_current(")
            && offer_select.contains(".route_scope_present_for_entry(")
            && current_offer_scope_id.contains(".route_scope_slot_inner(node_scope).is_some()")
            && !current_offer_scope_id.contains("node_scope.kind()")
            && !current_offer_scope_id.contains("ScopeKind::Route")
            && cursor_scope_route.contains("PassiveArmChildFact")
            && cursor_scope_route
                .contains("passive_child_scope(&self, route_scope: ScopeId, arm: u8)")
            && passive_child_scope.contains("child_scope != route_scope")
            && !passive_child_scope.contains("scope_id.kind()")
            && !passive_child_scope.contains("ScopeKind::Route")
            && cursor_route_navigation.contains("self.route_chain_bound()")
            && role_program_types.contains("PackedRouteArmRow")
            && role_program_types.contains("RouteArmLaneStepRow")
            && role_program_types.contains("child_slot(self) -> Option<u16>")
            && role_program_impl.contains("passive_arm_child_ordinal_by_slot")
            && !role_program_types.contains("passive_children")
            && !role_program_types.contains("route_arm_rows: &'static")
            && role_program_impl.contains("PackedRouteArmRow::new(")
            && role_program_impl.contains("child_slot,")
            && !role_program_types.contains("passive_arm_child_rows")
            && !role_program_types.contains("PassiveArmChildRow")
            && !cursor_scope_route.contains("PassiveArmChildRow")
            && !role_program_impl.contains("passive_arm_child_rows")
            && endpoint_kernel.contains(
                "prepare_route_site_materialization_rows_from_resident_route_commit_range"
            )
            && !offer_select.contains(".route_scope_rows(")
            && !offer_select.contains(".route_scope_rows_at(")
            && !frontier_select.contains("align_cursor_to_lane_progress")
            && !frontier_select.contains("first_pending_step_index("),
        "offer/frontier still use cursor facts for selected arms and event frontier metadata"
    );
    for forbidden in [
        ".route_scope_rows(",
        ".route_scope_rows_at(",
        ".passive_arm_scope_by_arm(",
        "passive_arm_scope_inner",
        "route_scope_for_passive_arm_entry",
    ] {
        assert!(
            !endpoint_kernel.contains(forbidden),
            "endpoint kernel must not read raw route ancestry directly: {forbidden}"
        );
    }
    for forbidden in [
        "passive_arm_scope_inner",
        "passive_arm_scope_by_arm",
        "route_scope_for_passive_arm_entry",
        "first_recv_target_in_passive_child_chain",
        "passive_child_chain_contains_descendant",
    ] {
        assert!(
            !cursor_scope_route.contains(forbidden) && !cursor_route_navigation.contains(forbidden),
            "cursor passive navigation must not re-grow child-scope inference: {forbidden}"
        );
    }
    assert!(
        passive_dispatch.contains(".first_recv_descendant_target_for_key(")
            && first_recv_dispatch.contains("visit_first_recv_dispatch(")
            && first_recv_dispatch.contains("first_recv_dispatch_root_arm")
            && first_recv_dispatch.contains("passive_arm_child_fact_by_slot")
            && first_recv_dispatch.contains("route_scope_rows_by_slot")
            && first_recv_dispatch.contains("footprint().route_scope_count")
            && !first_recv_dispatch.contains("MAX_FIRST_RECV_DISPATCH")
            && !first_recv_dispatch.contains("FirstRecvDispatchSpec"),
        "passive dispatch must derive first-recv rows from descriptor route-scope child-slot authority without a fixed dispatch table"
    );
    for forbidden in [
        "MAX_PASSIVE_DISPATCH_ROW_WALK",
        "first_recv_target_in_passive_child_row_walk",
        "[ScopeId::none();",
        "[0u8; MAX_PASSIVE_DISPATCH_ROW_WALK]",
    ] {
        assert!(
            !cursor_route_navigation.contains(forbidden),
            "passive dispatch must not allocate a route-depth DFS workspace: {forbidden}"
        );
    }
    assert!(
        !passive_dispatch.contains(".passive_descendant_dispatch_arm_for_key("),
        "passive descendant dispatch must stay non-recursive"
    );
    assert!(
        passive_rebase.contains("self.passive_child_scope(stop_scope, stop_arm)")
            && passive_rebase.contains("self.passive_child_scope(selected_scope, arm)")
            && !passive_rebase.contains("route_conflict_parent_arm")
            && !passive_rebase.contains("materialization_index_for_selected_arm")
            && !passive_rebase.contains("node_scope_id_at"),
        "passive descendant rebase must stay on PackedRouteArmRow child-slot authority"
    );
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
