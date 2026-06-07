use super::common::*;

fn repo_file_exists(path: &str) -> bool {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(path)
        .exists()
}

fn runtime_types_source() -> String {
    let mut source = read("src/endpoint/kernel/core/runtime_types.rs");
    source.push_str(&read("src/endpoint/kernel/core/runtime_types/commit.rs"));
    source
}

#[test]
fn route_commit_apply_and_progress_files_stay_deleted() {
    for path in [
        "src/endpoint/kernel/core/route_commit_apply.rs",
        "src/endpoint/kernel/core/route_commit_progress.rs",
        "src/endpoint/kernel/core/scope_settlement.rs",
        "src/endpoint/kernel/core/scope_path_progress.rs",
    ] {
        assert!(
            !repo_file_exists(path),
            "old route/settlement file must stay deleted: {path}"
        );
    }
}

#[test]
fn production_sources_do_not_reintroduce_route_apply_or_settlement_vocabularies() {
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
    ] {
        assert!(
            !source.contains(forbidden),
            "production source must not re-grow route apply/settlement fallback: {forbidden}"
        );
    }
}

#[test]
fn send_recv_decode_publish_paths_apply_prepared_deltas_only() {
    let send_ops = read("src/endpoint/kernel/core/send_ops.rs");
    let recv = read("src/endpoint/kernel/recv.rs");
    let finish = read("src/endpoint/kernel/decode/finish.rs");
    let decode_txn = read("src/endpoint/kernel/decode/finish/commit_txn.rs");
    let select = read("src/endpoint/kernel/core/decision_policy/impls/select.rs");
    let offer_select = read("src/endpoint/kernel/offer/select.rs");
    let select_alignment = read("src/endpoint/kernel/offer/select_alignment.rs");
    let commit_delta = read("src/endpoint/kernel/core/commit_delta.rs");
    let route_preview = read("src/endpoint/kernel/core/route_preview.rs");
    let offer_refresh = read("src/endpoint/kernel/core/offer_refresh.rs");
    let runtime_types = runtime_types_source();

    let send_publish = send_ops
        .split("fn publish_send_progress_commit_plan")
        .nth(1)
        .and_then(|tail| tail.split("fn preflight_send_cursor_after_preview").next())
        .expect("send progress publish must stay factored");
    let recv_publish = recv
        .split("fn publish_recv_commit_plan")
        .nth(1)
        .and_then(|tail| tail.split("fn finish_recv_payload").next())
        .expect("recv publish must stay factored");
    let decode_publish = finish
        .split("fn publish_decode_commit_plan(&mut self, plan: PreparedDecodePublishPlan")
        .nth(1)
        .and_then(|tail| tail.split("plan.committed_payload").next())
        .expect("decode publish function must stay factored");
    let lane_relocation_preflight = commit_delta
        .split("fn preflight_lane_relocation")
        .nth(1)
        .and_then(|tail| tail.split("    #[inline(always)]").next())
        .expect("lane relocation preflight must stay factored");

    assert!(
        runtime_types.contains("pub(crate) struct PreparedCommitDelta")
            && !runtime_types.contains("struct SendRouteEvidencePlan")
            && runtime_types.contains("pub(crate) struct ParentRouteEvidenceRow")
            && runtime_types.contains("struct SelectedRouteCommitRowsRef")
            && !runtime_types.contains("SendRouteCommitPlan")
            && !runtime_types.contains("fn from_enabled(")
            && runtime_types.contains("fn with_loop_row(")
            && runtime_types.contains("fn with_lane_relocation(")
            && runtime_types.contains("fn selected_routes(")
            && runtime_types.contains("fn cursor_only(")
            && runtime_types.contains("fn with_selected_route_rows(")
            && commit_delta.contains("pub(in crate::endpoint::kernel) fn prepare_commit_delta")
            && commit_delta.contains("pub(in crate::endpoint::kernel) fn commit_prepared_delta")
            && commit_delta.contains("fn commit_cursor_realign_index(")
            && commit_delta.contains("struct CommitDeltaApplyPermit")
            && commit_delta.contains("CommitDeltaApplyPermit::new()")
            && commit_delta.contains("let routes = delta.selected_routes();")
            && commit_delta.contains("fn apply_prepared_cursor_index(")
            && commit_delta.contains("fn apply_prepared_lane_advance(")
            && commit_delta.contains("fn apply_prepared_lane_relocation(")
            && commit_delta.contains("self.apply_loop_commit_row(")
            && !route_preview.contains("fn set_cursor_index(")
            && !offer_refresh.contains("fn set_lane_cursor_to_relocatable_step(")
            && !offer_refresh.contains("fn advance_lane_cursor_to_relocatable_step(")
            && !commit_delta.contains("apply_route_commit_effects")
            && !commit_delta.contains("settle_cursor_after_commit")
            && !commit_delta.contains("publish_commit_apply_outcome")
            && !commit_delta.contains("record_prepared_route_selection"),
        "CommitDelta apply must be the only cursor mutation boundary"
    );
    assert!(
        lane_relocation_preflight.contains(".node_index_for_relocatable_step(step)")
            && !lane_relocation_preflight.contains("resident_lane_step_locator(")
            && !lane_relocation_preflight.contains("phase_lane_step_ordinal(")
            && !lane_relocation_preflight.contains("resident_row_lane_step_ordinal("),
        "CommitDelta preflight must validate lane relocation through event/lane identity, not phase-row topology"
    );
    assert!(
        send_ops.contains("delta = delta.with_selected_route_rows(route_rows);")
            && !send_ops.contains("SendRouteCommitPlan")
            && !send_ops.contains("SendRouteEvidencePlan")
            && !send_ops.contains("build_send_route_commit_plan")
            && !send_ops.contains("publish_send_route_commit_plan")
            && !send_ops.contains("self.apply_selected_route_commit_row(route_row)")
            && !send_ops.contains("self.record_prepared_route_selection(route_row)"),
        "send route selection must be folded into the prepared CommitDelta, not applied by a side route commit path"
    );
    assert!(
        decode_txn.contains(".with_selected_route_rows(route_rows.as_commit_rows())")
            && !decode_txn.contains("apply_selected_route_commit_row")
            && !decode_txn.contains("record_prepared_route_selection"),
        "decode route rows must be carried by PreparedCommitDelta, not applied inside a decode-side transaction"
    );
    assert!(
        select.contains("CommitDelta::route_rows(")
            && select.contains("self.commit_prepared_delta(delta);")
            && !select.contains("apply_selected_route_commit_row")
            && !select.contains("record_prepared_route_selection"),
        "route materialization must commit selected rows through CommitDelta, not mutate route state directly"
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
            "self.commit_prepared_delta(plan.delta);",
        ),
        ("recv", recv_publish, "self.commit_prepared_delta(delta);"),
        (
            "decode",
            decode_publish,
            "self.commit_prepared_delta(delta);",
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
fn decode_progress_plan_no_longer_carries_route_cleanup_inputs() {
    let decode = read("src/endpoint/kernel/decode.rs");
    let decode_finish = read("src/endpoint/kernel/decode/finish.rs");
    let wire_progress = decode
        .split("enum DecodeProgressPlan")
        .nth(1)
        .and_then(|tail| tail.split("Branch {").next())
        .expect("DecodeProgressPlan::Wire must stay visible");

    assert!(
        wire_progress.contains("delta: CommitDelta")
            && !wire_progress.contains("branch_scope")
            && !wire_progress.contains("branch_lane"),
        "wire decode progress must carry only the semantic commit delta"
    );
    assert!(
        decode.contains("Branch { delta: CommitDelta }")
            && decode.contains("Empty { delta: CommitDelta }")
            && decode.contains("enum PreparedDecodeProgressPlan")
            && decode.contains("Wire { delta: PreparedCommitDelta }")
            && decode.contains("Branch { delta: PreparedCommitDelta }")
            && decode.contains("Empty { delta: PreparedCommitDelta }")
            && !decode.contains("PreparedSyntheticBranchCommitDelta")
            && !decode.contains("PreparedEmptyBranchCommitDelta"),
        "decode planning must carry CommitDelta until endpoint preflight and publish only PreparedCommitDelta"
    );
    for forbidden in ["route_ancestor_arm", "scope_parent("] {
        assert!(
            !decode_finish.contains(forbidden),
            "decode commit planning must not re-grow endpoint-side route ancestry walk: {forbidden}"
        );
    }
}
