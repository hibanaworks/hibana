use super::common::*;

#[test]
fn lane_searches_do_not_encode_absence_as_a_preferred_lane_sentinel() {
    let production = read_production_rs_tree("src");
    let ingress = read("src/endpoint/kernel/offer/ingress.rs");

    for forbidden in [
        "first_pending_step_index(usize::MAX)",
        "next_preferred_transport_lane",
        "while advanced < *scan_idx",
    ] {
        assert!(
            !production.contains(forbidden),
            "lane search must not restore a sentinel or restart-from-zero iterator: {forbidden}"
        );
    }
    assert!(
        ingress.contains("enum OfferLaneScanCursor")
            && ingress.contains("Preferred(usize)")
            && ingress.contains("Remaining(usize)")
            && ingress.contains("Exhausted")
            && ingress.contains("self.offer_lanes.next_set_from(start, self.lane_limit)"),
        "offer ingress must enumerate its preferred lane and monotonic remainder through explicit states"
    );
}

#[test]
fn sparse_role_route_rows_never_define_the_route_slot_boundary() {
    let production = read_production_rs_tree("src");
    let event_program = read("src/global/event_program.rs");

    for forbidden in [
        "while let Some(region) = self.machine().route_scope_rows_by_slot(slot)",
        "while let Some(region) = self.event_program.route_scope_rows_by_slot(slot)",
    ] {
        assert!(
            !production.contains(forbidden),
            "an empty role-local route row is not the end of the exact route-slot domain: {forbidden}"
        );
    }
    assert!(
        event_program.contains("pub(crate) fn route_scope_slot_count(&self) -> usize")
            && event_program.contains("self.footprint().route_scope_count"),
        "all route traversal must retain the exact descriptor slot count as its boundary"
    );
}

#[test]
fn role_projection_does_not_hide_exact_count_dispatch() {
    let production = read_production_rs_tree("src");
    let role_projection = read("src/g/role_projection.rs");

    assert!(
        role_projection.len() <= 20 * 1024,
        "role_projection.rs must stay a small value-level projection boundary, not a generated dispatch table"
    );
    assert!(
        !repo_file_exists("src/g/role_projection/role_image_dispatch.rs"),
        "role_image_dispatch.rs must not return as generated exact-count dispatch"
    );
    for forbidden in [
        "role_image_dispatch",
        "dispatch_role_",
        "RoleProjectionColumns<",
        "local_step_events_exact::<",
        "local_step_lanes_exact::<",
        "route_arm_rows_exact::<",
    ] {
        assert!(
            !production.contains(forbidden),
            "production source must not encode role image row counts as type dispatch: {forbidden}"
        );
    }

    for line in production.lines() {
        assert!(
            !(line.contains("macro_rules!")
                && line.contains("role")
                && line.contains("projection")),
            "role projection must not be hidden behind a macro-generated dispatch table: {line}"
        );
        assert!(
            !(line.contains("include!") && line.contains("role") && line.contains("projection")),
            "role projection must not include a generated dispatch table: {line}"
        );
    }

    if repo_file_exists("build.rs") {
        let build_rs = read("build.rs");
        assert!(
            !(build_rs.contains("role_projection")
                || build_rs.contains("role projection")
                || build_rs.contains("dispatch_role_")),
            "build.rs must not generate role projection dispatch"
        );
    }
}

#[test]
fn route_arm_lane_projection_has_one_linear_fact_authority() {
    let plan = read("src/global/role_program/image_impl/plan.rs");
    let emitter = read("src/global/role_program/image_impl/blob_image/lanes.rs");
    let facts = read("src/global/role_program/image_impl/projection/lanes.rs");
    let combined = format!("{plan}\n{emitter}");

    for removed in [
        "lane_byte_len_for_eff_range",
        "route_arm_lane_step_count",
        "eff_lane_bit_byte",
        "eff_lane_bit_len",
        "while scan < eff_idx",
        "while scan_eff < eff_idx",
    ] {
        assert!(
            !combined.contains(removed),
            "route-arm lane projection must not restore repeated event scans: {removed}"
        );
    }
    assert!(
        plan.contains("projection::LocalLaneFacts::for_eff_range")
            && emitter.contains("facts.last_step(atom.lane)")
            && emitter.contains("let (start_eff, end_eff) = facts.eff_range()")
            && !emitter.contains("eff_range: (usize, usize)")
            && emitter.contains("let mut emitted = [0u8; LANE_BITMAP_BYTES]")
            && emitter.contains("if source_row >= column.len as usize")
            && emitter.contains("self.zero_extended_lane_bit_from_row(column, left, idx)")
            && emitter.contains("self.zero_extended_lane_bit_from_row(column, right, idx)")
            && emitter.contains("if row_start < left.end() || row_start < right.end()")
            && facts.contains("struct LocalLaneAccumulator")
            && facts.contains("last_steps: [u16; LANE_DOMAIN_SIZE]")
            && facts.contains("eff_range: (usize, usize)")
            && facts.contains("lanes.record(atom.lane, local_step)")
            && facts.contains("if start_eff > end_eff || end_eff > eff_list.len()")
            && !facts.contains("let limit = if end_eff < eff_list.len()")
            && facts.contains("if byte_idx >= self.lanes.lane_bit_len")
            && facts.contains("if self.lanes.lane_bits[byte_idx] & bit == 0")
            && facts.contains("relation_count: usize"),
        "planner and emitter must share one lane-domain-derived linear fact projection"
    );
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
        "pub(crate) mod delegation",
        "DELEGATION_LEASE",
        "delegation_children",
        "DelegationLeaseSpec",
        "struct EffectEnvelope {",
        "enum EffectEnvelopeSource",
        "control_op_is_idempotent",
        "control_op_requires_gen_bump",
        "control_op_is_terminal",
        "control_op_modifies_history",
        "emit_resolver_event_with_arg2",
        "run_effect_step",
        "after_local_effect",
        "PendingCapRelease::inert",
        "pub(crate) fn inert() -> Self",
        "pub(crate) fn disarm(&mut self)",
        "ResolverEventSpec",
        "ResolverEventKind",
        "TapEvents",
        "#[cfg(all(test, hibana_repo_tests))]\npub const",
        "pub const ROUTE_PICK",
        "pub const RESOLVER_ABORT",
        "pub const RESOLVER_ANNOT",
        "pub const RESOLVER_TRAP",
        "pub const RESOLVER_EFFECT",
        "pub const RESOLVER_STATE_RESTORE",
        "TEST_GLOBAL_TAP_RING",
        "TS_CHECKER",
        "install_ts_checker",
        "global_tap_ring_ptr",
        "check_event_timestamp",
        "_ => ScopeKind::Generic",
        "inferred item nodes",
        "inferred item generated",
        "JumpReason",
        "JumpError",
        "try_follow_jumps_from_index",
        "try_next_index_past_jumps_from",
        "flow_follow_jumps_from",
        "jump_reason_at",
        "jump_target_at",
        "is_jump_at",
    ] {
        assert!(
            !production.contains(forbidden),
            "production sources must not retain repo-test effect runners or test-only owner paths: {forbidden}"
        );
    }
}

#[test]
fn resolver_audit_emit_stays_infallible() {
    let source = read("src/endpoint/kernel/core/decision_resolver/impls/audit.rs");
    let audit_fn = source
        .split("fn emit_dynamic_resolver_audit")
        .nth(1)
        .and_then(|tail| {
            tail.split("pub(in crate::endpoint::kernel) fn emit_dynamic_resolver_success_audit")
                .next()
        })
        .expect("decision resolver audit emit helper must stay in audit owner");
    assert!(
        !audit_fn.contains("SendResult") && !audit_fn.contains("Ok(())"),
        "resolver audit emit must not return Result when it has no error source"
    );
    assert!(
        source.contains("emit_dynamic_resolver_success_audit")
            && source.contains("emit_dynamic_resolver_reject_audit")
            && source.contains("events::resolver_audit(")
            && source.contains("scope_id")
            && !source.contains("ids::RESOLVER_AUDIT")
            && !source.contains("((resolver_id as u32) << 16) | result"),
        "dynamic resolver audit must be the only local resolver authority evidence path"
    );
}
#[test]
fn endpoint_hot_paths_do_not_emit_resolver_audit_replay() {
    let endpoint_hot_path = [
        read("src/endpoint/kernel/core/send_ops.rs"),
        read("src/endpoint/kernel/recv.rs"),
        read("src/endpoint/kernel/branch_recv.rs"),
        read("src/endpoint/kernel/branch_recv/finish.rs"),
        read("src/endpoint/kernel/core/route_preview.rs"),
    ]
    .join("\n");
    for forbidden in [
        "emit_endpoint_resolver_audit",
        "emit_resolver_audit_replay",
        "ResolverSlot::EndpointRx",
        "ResolverSlot::EndpointTx",
        "ids::RESOLVER_AUDIT",
        "endpoint_resolver_args",
        "EndpointRxAuditPlan",
    ] {
        assert!(
            !endpoint_hot_path.contains(forbidden),
            "endpoint hot paths must not know resolver audit replay: {forbidden}"
        );
    }
}

#[test]
fn production_sources_do_not_keep_fallible_success_wrappers() {
    let production = read_production_rs_tree("src");
    for forbidden in [
        "fn route_scope_offer_entry_by_slot(&self, slot: usize) -> Option<StateIndex>",
        "pub(crate) const fn transport_frame_label(&self) -> Option<u8>",
        "enum DecodeReentryCursorPlan",
        "DecodeReentryCursorPlan::",
        "ScopeLoopMeta",
        "scope_loop_meta",
        "loop_controller_without_evidence",
        "FLAG_CONTINUE_HAS_RECV",
        "FLAG_BREAK_HAS_RECV",
        "loop_label_scope",
        "ScopeKind::Loop",
        "PackedLoopScopeRow",
        "ROLE_IMAGE_LOOP_SCOPE_STRIDE",
        "loop_scope_row",
        "LoopBodyMissing",
        ") -> RecvResult<DecodeReentryCursorPlan>",
        "pub(super) fn begin(&mut self) -> RecvResult<SelectedRouteCommitRows>",
        ") -> RecvResult<OfferFrontierFacts>",
        "fn publish_recv_commit_plan(&mut self, plan: RecvCommitPlan<'r>) -> RecvResult<Payload<'r>>",
        ") -> Result<(Port<'a, T>, LaneGuard<'a, T, C>), RendezvousError>",
        ") -> Result<LanePortAccess<'lease, 'cfg, T, C>, RendezvousError>",
        "pub(crate) const fn pack_flags(\n        is_controller: bool,",
        "struct CurrentOfferObservation {\n    present: bool,",
        "const fn new(\n        present: bool,\n        ready: bool,",
        ") -> Result<lane_port::ReceivedFrame<'r>, ()>",
        "Err(()) => Ok(MaterializedTransport::DiscardedAndPending)",
        "CurrentOfferEntry::from_meta",
        "CurrentOfferAuthority::from_meta",
        "RawOfferLease::new(",
        "RawRecvFlags::new(",
        "struct RawOfferLease",
        "struct RawRecvFlags",
        "RawOfferLease::from_held_lease",
        "RawRecvFlags::from_held_lease",
        "fn mark_completed(&mut self)",
        "fn must_restore_on_drop(self) -> bool",
        "relocatable_resident_lane_step_at_index(step, lane as usize)\n            .ok()",
        "u16::try_from(step_idx.checked_sub(start)?).ok()",
        "self.ensure_endpoint_lease_capacity(required_slots).ok()?",
        "EndpointLeaseId::try_from(insert_idx).ok()?",
        "u32::try_from(end).ok()?",
        "u32::try_from(start).ok()?",
        "offset + required_bytes > slab_len",
        "offset + len > slab_len",
        "leased: bool",
        "completed: bool",
        "core::ptr::addr_of_mut!((*dst).active).write(false)",
        "self.active = true",
        "self.active = false",
        "pub(crate) active: bool,\n    pub(crate) lane_idx",
        "active: true,\n                lane_idx",
        "public_slot_owned: bool",
        "public_slot_owned: true",
        "self.public_slot_owned = false",
        "init_public_offer_state(&mut self) -> bool",
        "init_public_send_state(&mut self, init: &SendInit) -> bool",
        "init_public_recv_state(&mut self) -> bool",
        "begin_public_branch_recv_state(&mut self) -> bool",
        "restore_on_drop: bool",
        "restore_on_drop = false",
        "restore_on_drop = true",
        "mark_public_endpoint_lease(",
        "mark_public_endpoint_lease(\n        &mut self,\n        lease_slot: EndpointLeaseId,\n        generation: u32,\n    ) -> bool",
        "public_endpoint: bool",
        "slot.public_endpoint",
        "public_endpoint: false",
        "occupied: bool",
        "occupied_len(",
        "global_frontier_scratch_initialized",
        "global_frontier_scratch_initialized: bool",
        "observed_key_present",
        "observed_key_present: bool",
        "observed_key_present: false",
        "observed_key_present = true",
        "initialized: bool",
        "initialized: false",
        "initialized = true",
        "self.initialized",
        "at_route_offer_entry",
        "from_route_entry(",
        "sparse: bool",
        ".sparse",
        "intrinsic_ready: bool",
        "ready: bool,\n    pub(in crate::endpoint::kernel) has_progress_evidence: bool",
        "state.ready |=",
        "state.has_progress_evidence |=",
        "pub(in crate::endpoint::kernel) ready: bool",
        "frontier_facts.ready {",
        "ready: frontier_facts.ready",
        "pub(in crate::endpoint::kernel) ingress_ready: bool",
        "pub(in crate::endpoint::kernel) pending: bool",
        "pub(super) ingress_ready: bool",
        "audit.ingress_ready",
        "audit.pending",
        "Route { has_offer_lanes: bool }",
        "has_offer_lanes: current_scope_meta.has_offer_lanes()",
        "progress_sibling_exists: bool",
        "input.progress_sibling_exists",
        "candidate_has_progress_evidence(\n    has_ready_arm_evidence: bool,",
        "ack_is_progress: bool",
        "ingress_ready: bool",
        "has_ack: bool",
        "EvidenceFingerprint::new(",
        "ack_is_progress_evidence(",
        "linger",
        "Linger",
        "LINGER",
        "Option<bool>",
        "Result<Option<bool>",
        "RecvResult<Option<bool>",
        "Some(false)",
        "then_some(false)",
        "reentry: bool",
        "is_reentry: bool",
        "route_offer_entry_matches_current",
        "is_reentry_route_from_cursor",
        "RouteReentry",
        "ReentryMark::Plain",
        "ReentryMark::Reentry",
        "is_internal",
        "event_internal",
        "ENDPOINT_INTERNAL",
        "origin: bool",
        "origin: false",
        "origin: true",
        "is_choice_determinant: bool",
        "is_choice_determinant: false",
        "is_choice_determinant: true",
        "release_lane_with_tap(&mut self, lane: Lane) -> bool",
        "release_lane(&self, lane: Lane) -> Option<SessionId>",
        "if let Some(sid) = rv.release_lane(lane)",
    ] {
        assert!(
            !production.contains(forbidden),
            "production source must not keep fallible-looking wrappers for infallible owner transitions: {forbidden}"
        );
    }

    let tests = read("tests/runtime_surface.rs");
    assert!(
        !tests.contains("fn baseline_left_resolver() -> Result<DecisionArm, ResolverError>"),
        "resolver tests must model the fallible resolver contract, not a constant-Ok helper"
    );
}

#[test]
fn production_sources_do_not_reintroduce_implicit_initializers() {
    let production = read_production_rs_tree("src");
    let trait_name = "Default";
    for line in production.lines() {
        let trimmed = line.trim_start();
        assert!(
            !(trimmed.starts_with("#[derive(") && trimmed.contains(trait_name)),
            "production source must use explicit empty/new/zero constructors, not derive({trait_name}): {line}"
        );
        assert!(
            !(trimmed.starts_with("impl") && trimmed.contains("Default for")),
            "production source must not add {trait_name} impls as implicit initializer surface: {line}"
        );
    }

    for forbidden in [
        "TapEvent::default",
        "Evidence::default",
        "FrameFlags::default",
        "FrameLabelMask::default",
        "TapFrameMeta::default",
        "LaneStorageLeaseSet::default",
        "EffList::default",
        "AssocTable::default",
        "RendezvousTable::default",
        "ArrayMap::default",
        "EndpointLeaseId::default",
        "LocalOnly::default",
        "LaneSteps::default",
        "ProgramLoweringFacts::default",
        "RoleCompiledFacts::default",
    ] {
        assert!(
            !production.contains(forbidden),
            "production source must not re-grow implicit initializer: {forbidden}"
        );
    }
}

#[test]
fn route_arm_selection_uses_only_endpoint_or_in_band_authority() {
    let production = read_production_rs_tree("src");
    let cursor_navigation = read("src/global/typestate/cursor/navigation.rs");
    for forbidden in [
        "ack_route_arm_selection",
        "acknowledge_with_role_count",
        "record_scope_ack",
        "peek_scope_ack",
        "peek_live_scope_ack",
        "clear_scope_ack",
        "preview_live_route_arm_selection_non_consuming",
        "ScopeEvidence {\n    pub(super) ack:",
        "OfferPassiveAckEvidence",
        "DynamicAckMaterializable",
        "FLAG_ACK",
    ] {
        assert!(
            !production.contains(forbidden),
            "route arm selection must not regain a generationless ack-consume side path: {forbidden}"
        );
    }
    for forbidden in [
        ".or_else(",
        ".or(",
        ".unwrap_or(",
        ".unwrap_or_else(",
        ".unwrap_or_default(",
    ] {
        assert!(
            !production.contains(forbidden),
            "production authority must not hide precedence or absence behind option fallback: {forbidden}"
        );
    }
    assert!(
        cursor_navigation.contains("if route_arm != self.route_arm_for_index(scope, idx)")
            && cursor_navigation.contains("crate::invariant();")
            && cursor_navigation.contains("(scope, route_arm)"),
        "event conflict and route-arm range membership must agree before metadata publication"
    );

    let resolve = read("src/endpoint/kernel/offer/resolve.rs");
    assert!(
        resolve.contains("fn poll_route_authority(")
            && resolve.contains("let is_dynamic_route_scope")
            && resolve.contains("if is_dynamic_route_scope {\n            return None;")
            && resolve.contains("RouteArmToken::from_poll(arm)")
            && resolve.contains("RouteArmCommitEvidence::PollFrame")
            && !resolve.contains("RouteArmToken::from_ack(arm)"),
        "offer resolve must derive passive route authority from in-band frame evidence"
    );
}

#[test]
fn compact_route_arm_authority_fails_closed_before_commit() {
    let authority = read("src/endpoint/kernel/authority.rs");
    let evidence = read("src/endpoint/kernel/evidence.rs");
    let evidence_store = read("src/endpoint/kernel/evidence_store.rs");
    let frontier_types = read("src/endpoint/kernel/offer/frontier_types.rs");
    let first_recv_dispatch = read("src/endpoint/kernel/offer/first_recv_dispatch.rs");
    let scope_evidence = read("src/endpoint/kernel/core/scope_evidence_logic.rs");
    let route_commit = read("src/endpoint/kernel/core/route_commit_helpers.rs");
    let route_preview = read("src/endpoint/kernel/core/route_preview.rs");
    let offer_select = read("src/endpoint/kernel/offer/select.rs");
    let offer_resolve = read("src/endpoint/kernel/offer/resolve.rs");
    let route_authority_paths =
        format!("{scope_evidence}\n{route_commit}\n{offer_select}\n{offer_resolve}");

    for forbidden in [
        "RouteTable",
        "peek_route_arm_selection",
        "poll_route_arm_selection",
    ] {
        assert!(
            !route_authority_paths.contains(forbidden),
            "route authority must not regain shared runtime state: {forbidden}"
        );
    }

    assert!(
        authority.contains("pub(super) const fn decode_raw(value: u8) -> Option<Self>")
            && authority.contains("pub(super) const fn from_raw(value: u8) -> Self")
            && authority
                .contains("const fn decode_single_ready_mask(mask: u8) -> Option<Option<Self>>")
            && authority.contains("pub(super) const fn from_single_ready_mask(mask: u8)")
            && authority.contains("0 => Some(None)")
            && authority.contains("1 => Some(Some(Self::LEFT))")
            && authority.contains("2 => Some(Some(Self::RIGHT))")
            && authority.contains("3..=u8::MAX => None")
            && authority.contains("match Self::decode_single_ready_mask(mask)")
            && authority.contains("None => crate::invariant()"),
        "compact route authority must have one explicit fail-closed decoding boundary"
    );
    for forbidden in [
        "Arm::new(",
        ".and_then(Arm::decode_raw)",
        ".map(RouteArmToken::from_poll)",
        "if let Some(arm) = Arm::decode_raw",
    ] {
        assert!(
            !route_authority_paths.contains(forbidden),
            "route authority must not collapse an invalid compact arm into absence: {forbidden}"
        );
    }
    assert!(
        scope_evidence.contains("Arm::from_single_ready_mask(mask)")
            && route_commit.contains("Arm::from_single_ready_mask(mask).map(Arm::as_u8)")
            && !route_commit.contains("scope_evidence.peek_ack")
            && route_commit.contains("let arm_value = Arm::decode_raw(arm)?;")
            && route_commit.contains(".selection_is_coherent(scope_slot, arm_value)")
            && evidence.contains("pub(super) enum ScopeEvidenceStatus")
            && evidence.contains("Conflicted = 1")
            && !evidence.contains("RouteArmToken")
            && evidence_store.contains("selected: Option<Arm>")
            && route_preview.contains("(Some(selected), Some(ready)) if selected != ready")
            && !route_preview.contains(".or_else(|| self.poll_arm_from_ready_mask")
            && first_recv_dispatch.contains("if arm_mask & !0b11 != 0")
            && !first_recv_dispatch.contains("arm_mask & 0b11")
            && !evidence.contains("_ => false"),
        "decoded route arms must remain typed through ready-mask and first-recv evidence"
    );
    assert!(
        !frontier_types.contains("CachedRouteArm")
            && !frontier_types.contains("route_arm: CachedRouteArm")
            && !frontier_types.contains("semantic: EventSemanticKind")
            && !frontier_types.contains("choice: RouteChoiceMark")
            && frontier_types.contains("pub(in crate::endpoint::kernel) const FLAG_NEXT_PRESENT")
            && frontier_types.contains(
                "self.cursor_index.is_absent() || (self.flags & Self::FLAG_NEXT_PRESENT) == 0"
            ),
        "frontier recv metadata must not duplicate route authority that no consumer reads"
    );
    let validation = offer_resolve
        .find("let (arm, marks_descendant) = if let Some(target_idx)")
        .expect("staged passive route arm validation must remain present");
    let intrinsic_publication = offer_resolve
        .find("self.mark_intrinsic_passive_descendant_path_ready")
        .expect("staged passive intrinsic publication must remain present");
    let ready_publication = offer_resolve
        .find("self.mark_scope_ready_arm_from_exact_passive_arm")
        .expect("staged passive ready-arm publication must remain present");
    assert!(
        validation < intrinsic_publication && intrinsic_publication < ready_publication,
        "staged passive route authority must validate before either evidence publication"
    );
}

#[test]
fn resident_route_arm_descriptors_reject_invalid_compact_values() {
    let image_impl = read("src/global/role_program/image_impl.rs");
    let event_rows = read("src/global/role_program/image_impl/event_rows.rs");
    let lane_image = read("src/global/role_program/image_impl/lane_image.rs");
    let lane_image_decode = read("src/global/role_program/image_impl/lane_image/decode.rs");
    let scope_rows = format!(
        "{}\n{}\n{}\n{}",
        read("src/global/role_program/image_impl/projection.rs"),
        read("src/global/role_program/image_impl/projection/route.rs"),
        read("src/global/const_dsl/scope_ranges/route.rs"),
        read("src/global/const_dsl/scope.rs")
    );
    let blob_image = read("src/global/role_program/image_impl/blob_image.rs");
    let facts = read("src/global/typestate/facts.rs");
    let first_recv_dispatch = read("src/global/typestate/cursor/first_recv_dispatch.rs");
    let resident_descriptor_arm_paths = format!("{event_rows}\n{lane_image}\n{blob_image}");
    let child_scope_accessor = lane_image
        .split("pub(crate) const fn passive_arm_child_ordinal_by_slot")
        .nth(1)
        .expect("passive arm child accessor")
        .split("pub(crate) const fn route_arm_event_row_by_slot")
        .next()
        .expect("passive arm child accessor body");
    let lane_step_row_accessor = lane_image
        .split("const fn route_arm_lane_step_row_at")
        .nth(1)
        .expect("route arm lane-step row accessor")
        .split("const fn route_arm_lane_step_row(")
        .next()
        .expect("route arm lane-step row accessor body");

    assert!(
        image_impl.contains("const fn decode_binary_route_arm_index(arm: u8) -> Option<usize>")
            && image_impl.contains("const fn binary_route_arm_index(arm: u8) -> usize")
            && image_impl.contains("const fn route_arm_row_index(slot: usize, arm: u8) -> usize")
            && image_impl.contains(
                "match decode_binary_route_arm_index(arm) {\n        Some(index) => index,\n        None => crate::invariant(),"
            ),
        "resident descriptor arm indexing must have one fail-closed binary boundary"
    );
    for forbidden in ["if arm >= 2", "if arm > 1"] {
        assert!(
            !resident_descriptor_arm_paths.contains(forbidden),
            "resident descriptor arm access must not collapse an invalid arm into absence: {forbidden}"
        );
    }
    assert!(
        lane_image.matches("route_arm_row_index(slot, arm)").count() >= 5
            && scope_rows.contains("let arm = binary_route_arm_index(arm);")
            && scope_rows.contains("let Some(ranges) = route_arm_ranges(eff_list.scope_markers(), route) else {\n        crate::invariant();")
            && scope_rows.contains("let arm = binary_route_arm_index(arm) as u8;")
            && scope_rows.contains("marker.event.route_arm()")
            && scope_rows.contains("Self::Enter(ScopeEntry::Route { .. }) => Some(0)")
            && scope_rows.contains("Self::Enter(ScopeEntry::RouteArmContinuation) => Some(1)")
            && scope_rows.contains("let Some((_, start, _)) = scope_dependency_bounds(markers, view_len, scope) else {")
            && blob_image.contains("route_arm_row_index(route_slot, arm as u8)")
            && child_scope_accessor.contains("let child_slot = child_slot as usize;")
            && child_scope_accessor.contains("if child_slot <= slot {")
            && child_scope_accessor.contains("let Some(scope) = self.route_scope_by_slot(child_slot) else {\n                    invalid_resident_descriptor();")
            && child_scope_accessor.contains("if !passive_child_parent_matches(")
            && child_scope_accessor
                .contains("self.route_scope_conflict_by_slot(child_slot),")
            && lane_image_decode.contains("child_scope.same(parent_scope)")
            && lane_image_decode.contains("recorded_parent.same(parent_scope)")
            && lane_image_decode.contains("recorded_arm == arm")
            && scope_rows.contains("outermost_scope_range(current, candidate)")
            && !scope_rows.contains("child_span")
            && lane_step_row_accessor.contains("logical_lane_count: usize")
            && lane_step_row_accessor.contains("event_row: PackedLaneRange")
            && lane_step_row_accessor.contains("decode_resident_route_arm_lane_step(")
            && lane_step_row_accessor.contains("None => invalid_resident_descriptor(),")
            && !lane_image.contains("let Some(row) = self.route_arm_lane_step_row_at(pos) else")
            && lane_image.contains("decode_resident_local_step_lane(raw, logical_lane_count)")
            && !lane_image.contains("if found != usize::MAX {")
            && lane_image.contains("if row.lane() == lane {")
            && lane_image.contains("return Some(row);")
            && lane_image.contains("if !route_commit_decisions_match(current, expected) {")
            && lane_image.contains("let parent = self.route_commit_parent(scope);")
            && facts
                .contains("const fn decode_optional_route_arm_raw(raw: u8) -> Option<Option<u8>>")
            && facts.contains("2..=254 => None")
            && facts.contains(
                "match Self::decode_optional_route_arm_raw(raw) {\n            Some(arm) => arm,\n            None => crate::invariant(),"
            )
            && facts.contains("const fn decode_raw(raw: u16) -> Option<Self>")
            && facts.contains("raw & !Self::ROUTE_VALUE_MASK")
            && first_recv_dispatch.contains("let arm = validated_dispatch_arm(arm, target);")
            && !first_recv_dispatch.contains("if arm >= 2 || target.is_absent()"),
        "compact descriptor facts and dispatch entries must reject invalid values before use"
    );
}

#[test]
fn resident_descriptor_columns_reject_in_range_sentinels() {
    let lane_image = read("src/global/role_program/image_impl/lane_image.rs");
    let lane_decode = read("src/global/role_program/image_impl/lane_image/decode.rs");
    let image_types = read("src/global/role_program/image_types.rs");
    let metadata = format!(
        "{}\n{}",
        read("src/global/role_program/image_impl/metadata.rs"),
        read("src/global/role_program/image_impl/metadata/scope_ranges.rs")
    );
    let program_ref = read("src/global/compiled/images/image/program_ref.rs");
    let ref_access = read("src/global/role_program/image_impl/ref_access.rs");
    let event_rows = read("src/global/role_program/image_impl/event_rows.rs");
    let tests = read("src/global/role_program/image_impl/tests.rs");
    let roll_scope = lane_image
        .split("pub(crate) const fn roll_scope_row")
        .nth(1)
        .expect("roll scope row accessor")
        .split("const fn route_scope_row")
        .next()
        .expect("roll scope row accessor body");
    let route_scope = lane_image
        .split("const fn route_scope_row")
        .nth(1)
        .expect("route scope row accessor")
        .split("pub(crate) const fn route_scope_by_slot")
        .next()
        .expect("route scope row accessor body");
    let route_arm = lane_image
        .split("const fn packed_route_arm_row")
        .nth(1)
        .expect("route arm row accessor")
        .split("const fn packed_dependency_row")
        .next()
        .expect("route arm row accessor body");
    let lane_range = lane_image
        .split("const fn lane_range_row")
        .nth(1)
        .expect("lane range row accessor")
        .split("pub(super) const fn route_scope_arm_lane_set_by_slot")
        .next()
        .expect("lane range row accessor body");

    for required in [
        "absent_route_scope_row",
        "non_route_scope_row",
        "route_scope_ordinal_out_of_range",
        "duplicate_route_scope_authority",
        "empty_route_arm_row",
        "noncanonical_empty_route_arm_lane_step_range",
        "noncanonical_empty_route_arm_event_range",
        "empty_lane_range_row",
        "noncanonical_empty_lane_range_row",
        "empty_roll_scope_row",
        "empty_roll_scope_event_range",
        "roll_scope_ordinal_out_of_range",
        "empty_local_event_row",
        "reserved_local_event_flags",
        "out_of_domain_local_event_index",
        "out_of_domain_local_event_scope",
        "local_step_lane_outside_logical_domain",
        "program_lane_mismatch",
        "foreign_role_event",
        "missing_program_event",
        "in_range_missing_event_and_lane_columns",
        "empty_referenced_dependency_row",
        "empty_referenced_conflict_row",
        "out_of_domain_conflict_route_scope",
        "out_of_domain_dependency_route_scope",
        "dependency_range_beyond_events",
        "zero_length_resident_boundary_row",
        "resident_boundary_beyond_lane_rows",
        "route_arm_event_range_beyond_events",
        "route_arm_lane_step_range_beyond_rows",
        "passive_child_without_parent_authority",
        "route_arm_lane_step_outside_own_arm",
        "route_commit_range_beyond_rows",
        "zero_length_route_commit_range",
        "foreign_route_commit_current",
        "foreign_route_commit_parent",
        "truncated_route_commit_parent_chain",
        "route_commit_rows_before_terminal_parent",
        "roll_scope_event_range_beyond_events",
    ] {
        assert!(
            tests.contains(&format!("fn resident_descriptor_rejects_{required}()")),
            "resident descriptor gate missing fail-closed case: {required}"
        );
    }

    assert!(
        lane_image
            .contains("const fn invalid_resident_descriptor() -> ! {\n    crate::invariant()\n}")
            && lane_image.matches("crate::invariant()").count() == 1
            && !ref_access.contains("crate::invariant()")
            && ref_access.contains("derive_active_lane_metadata(")
            && ref_access.contains("lane_columns_are_coherent(")
            && ref_access.contains("route_commit_capacity_is_exact(")
            && metadata.contains("pub(super) const fn derive_active_lane_metadata")
            && metadata.contains("pub(super) const fn lane_columns_are_coherent")
            && metadata.contains("offer_bitmap_is_arm_union")
            && metadata.contains("arm_bitmap_matches_lane_steps")
            && metadata.contains("resident_event_lanes_match_active")
            && metadata.contains(
                "pub(in crate::global::role_program::image_impl) const fn route_commit_capacity_is_exact"
            )
            && metadata.contains("active_lane_row.start() != 0")
            && metadata.contains("observed_max == max_route_commit_count")
            && roll_scope.contains("decode_resident_roll_scope(self.read_u16_at(offset))")
            && roll_scope.contains("let event_row = row.event_row();")
            && roll_scope.contains("event_row.end() > self.columns.events.len as usize")
            && !roll_scope.contains("if row.is_empty() { None }")
            && route_scope.contains("decode_resident_route_scope(self.read_u16_at(offset))")
            && route_scope.contains("None => invalid_resident_descriptor(),")
            && lane_decode.contains("if raw_ordinal >= ScopeId::LOCAL_CAPACITY")
            && image_types.contains("!event_row.is_canonical_optional_range()")
            && image_types.contains("!self.is_zero_len() || self.start() == 0")
            && route_arm.contains("if row.is_empty() {")
            && route_arm.contains("event_row.end() > self.columns.events.len as usize")
            && route_arm.contains("event_row.is_zero_len() != (row.lane_step_len() == 0)")
            && route_arm
                .contains("lane_step_end > self.columns.route_arm_lane_step_rows.len as usize")
            && route_arm.contains("invalid_resident_descriptor();")
            && lane_range.contains("if !row.is_canonical_optional_range() {")
            && lane_range.contains("invalid_resident_descriptor();")
            && event_rows.contains("decode_resident_event_header(eff_index, scope_raw, flags)")
            && event_rows.contains("const fn role_local_direction(")
            && event_rows.contains("program.event_atom_at(eff_idx)")
            && event_rows.contains("None => crate::invariant(),")
            && tests.contains("fn lane_columns_bind_partition_arm_steps_and_offer_union()")
            && program_ref.contains("pub(crate) const fn event_atom_at")
            && program_ref.contains("None => crate::invariant(),")
            && ref_access
                .matches("None => invalid_resident_descriptor(),")
                .count()
                >= 2
            && lane_image.contains("Some(dependency) => Some(dependency)")
            && lane_image.contains("if conflict.is_none() {")
            && lane_image.contains("if start >= end || end > self.columns.lanes.len as usize")
            && !lane_image.contains("while pos < end && pos < self.columns.lanes.len as usize"),
        "every serialized resident row must reject a reserved sentinel before publication"
    );
}

#[test]
fn production_sources_keep_absence_codes_named_by_meaning() {
    let production = read_production_rs_tree("src");
    for forbidden in [
        "PROGRAM_IMAGE_NO_ROUTE_CONTROLLER",
        "EventSemanticKind::Other",
        "EVENT_CURSOR_NO_STATE",
        "NO_SELECTED_ARM",
        "NO_ACTIVE_LANE",
        "Self::NO_FRAME",
        "clamped to the",
        "u16::MAX, 0",
        "fn invalidate(&mut self)",
        ".invalidate()",
        "RecvPayloadSource::Empty",
        "EmptyArmTerminal",
        "LoopBodyEmpty",
        "ParallelEmpty",
        "ArmEmpty",
        "pub(crate) const fn header_bytes",
        "pub(crate) const fn port_slots_bytes",
        "pub(crate) const fn guard_slots_bytes",
        "pub(crate) const fn arena_bytes",
        "pub(crate) const fn arena_align",
    ] {
        assert!(
            !production.contains(forbidden),
            "production source must name compact absence codes by invariant meaning: {forbidden}"
        );
    }

    let public_endpoint_layout = read("src/session/cluster/core.rs");
    let public_endpoint_layout = public_endpoint_layout
        .split("struct PublicEndpointStorageLayout")
        .nth(1)
        .and_then(|tail| tail.split("use core::fmt;").next())
        .expect("PublicEndpointStorageLayout must remain in session cluster core");
    for forbidden in [
        "header_bytes",
        "port_slots_bytes",
        "guard_slots_bytes",
        "header_padding_bytes",
        "arena_bytes",
        "arena_align",
    ] {
        assert!(
            !public_endpoint_layout.contains(forbidden),
            "PublicEndpointStorageLayout must carry only fields consumed by endpoint attach: {forbidden}"
        );
    }
}

#[test]
fn offer_lane_lookup_distinguishes_empty_membership_from_missing_scope() {
    let core = read("src/endpoint/kernel/core.rs");
    let accessor = core
        .split("pub(crate) fn offer_lane_set_for_scope")
        .nth(1)
        .expect("offer lane-set accessor")
        .split("pub(crate) fn route_scope_arm_lane_set_for_scope")
        .next()
        .expect("offer lane-set accessor body");

    assert!(accessor.contains("Some(lanes) => lanes"));
    assert!(accessor.contains("None => crate::invariant()"));
    assert!(!accessor.contains("None => LaneSetView::EMPTY"));
}

#[test]
fn package_gate_must_not_hide_dead_code() {
    let package_gate = read(".github/scripts/check_package_artifact.sh");
    assert!(
        !package_gate.contains("-Adead_code"),
        "package artifact gate must not hide dead-code warnings"
    );
}

#[test]
fn wire_codec_errors_do_not_carry_static_text() {
    let production = read_production_rs_tree("src");
    let readme = read("README.md");
    let matrix_gate = read(".github/scripts/check_message_heavy_matrix.sh");

    for forbidden in [
        "Invalid(&'static str)",
        "CodecError::Invalid",
        "CodecError::Invalid(",
        "\n    Invalid,\n",
        "ERR_PAYLOAD_LEN",
        "ERR_ZERO_PAYLOAD",
        "ERR_BOOLEAN_PAYLOAD",
        "require_exact_len(input.as_bytes().len(), 20, \"payload length\")",
    ] {
        assert!(
            !production.contains(forbidden)
                && !readme.contains(forbidden)
                && !matrix_gate.contains(forbidden),
            "wire codec errors must stay string-free and unit-sized: {forbidden}"
        );
    }
}

#[test]
fn source_text_does_not_regrow_old_private_boundary_vocabulary() {
    let source = format!("{}\n{}", read_production_rs_tree("src"), read("README.md"));

    for forbidden in [
        "INTERNAL IMPLEMENTATION",
        "DO NOT USE DIRECTLY",
        "NOT PUBLIC",
        "internal implementation",
        "should not use this module directly",
        "descriptor evaluator",
        "ra module",
        "Internal endpoint kernel",
        "Internal runtime kernel",
        "Internal TapEvent",
        "Internal generativity",
        "Internal rendezvous",
        "AttachOp::Internal",
        "Resolver VM",
    ] {
        assert!(
            !source.contains(forbidden),
            "source text must not regrow old private-boundary vocabulary: {forbidden}"
        );
    }
}

#[test]
fn public_failure_evidence_has_no_stringly_accessors() {
    for (name, source) in [
        ("EndpointError", endpoint_facade_source()),
        ("AttachError", read("src/session/cluster/error.rs")),
        ("ResolverError", cluster_core_source()),
    ] {
        for forbidden in [
            "pub const fn operation(&self) -> &'static str",
            "pub fn operation(&self) -> &'static str",
            "pub const fn file(&self) -> &'static str",
            "pub const fn line(&self) -> u32",
            "pub const fn column(&self) -> u32",
        ] {
            assert!(
                !source.contains(forbidden),
                "{name} must not expose stringly public diagnostics: {forbidden}"
            );
        }
    }
}

#[test]
fn route_branch_surface_has_no_decode_entrypoint() {
    let public_endpoint = endpoint_facade_source();
    let readme = read("README.md");
    let endpoint_allowlist = read(".github/allowlists/endpoint-public-api.txt");
    for forbidden in ["RouteBranch::decode", ".decode::<", "pub fn decode<M>"] {
        assert!(
            !public_endpoint.contains(forbidden)
                && !readme.contains(forbidden)
                && !endpoint_allowlist.contains(forbidden),
            "RouteBranch public surface must stay on recv/send without decode residue: {forbidden}"
        );
    }
    assert!(
        endpoint_allowlist.contains("RouteBranch::label")
            && endpoint_allowlist.contains("RouteBranch::send")
            && endpoint_allowlist.contains("RouteBranch::recv"),
        "RouteBranch allowlist must be the canonical label/send/recv authority"
    );
}

#[test]
fn resolver_registration_has_only_stateful_entry() {
    let resolver = cluster_core_source();
    let readme = read("README.md");
    for forbidden in [
        "pub fn decision_fn",
        "ResolverRef::<ROUTE_RESOLVER>::decision_fn",
        "dispatch_decision_fn",
        "stateless:",
        "stateless resolver",
    ] {
        assert!(
            !resolver.contains(forbidden) && !readme.contains(forbidden),
            "resolver registration must stay on decision_state only: {forbidden}"
        );
    }
    assert!(
        resolver.contains("pub fn decision_state<S: 'cfg>")
            && readme.contains("`ResolverRef::<ID>::decision_state(...)`"),
        "resolver registration must document and expose only the stateful entry"
    );
}

#[test]
fn production_has_no_source_location_diagnostic_plumbing() {
    let production = read_production_rs_tree("src");

    assert!(
        !repo_file_exists("src/diag.rs") && !read("src/lib.rs").contains("mod diag;"),
        "source-location diagnostic module must not return"
    );
    for forbidden in [
        "Callsite",
        "core::panic::Location",
        "panic::Location",
        "Location::caller()",
        "&'static Location<'static>",
        "ErrorLocation",
        "ResolverErrorLocation",
        "_location",
        "fn file(",
        "fn line(",
        "fn column(",
    ] {
        assert!(
            !production.contains(forbidden),
            "production source must not retain source-location diagnostic storage: {forbidden}"
        );
    }
}

#[test]
fn runtime_production_path_has_no_string_panic_alternate_paths() {
    fn strip_cfg_test_modules(source: &str) -> String {
        let mut out = String::new();
        let mut skip = false;
        let mut pending_cfg_test = false;
        let mut depth = 0usize;

        for line in source.lines() {
            let trimmed = line.trim_start();
            if skip {
                depth = depth
                    .saturating_add(line.matches('{').count())
                    .saturating_sub(line.matches('}').count());
                if depth == 0 {
                    skip = false;
                }
                continue;
            }
            if trimmed.starts_with("#[cfg(test)]") {
                pending_cfg_test = true;
                continue;
            }
            if pending_cfg_test && trimmed.starts_with("mod tests") {
                depth = line
                    .matches('{')
                    .count()
                    .saturating_sub(line.matches('}').count());
                if depth > 0 {
                    skip = true;
                }
                pending_cfg_test = false;
                continue;
            }
            if pending_cfg_test {
                out.push_str("#[cfg(test)]\n");
                pending_cfg_test = false;
            }
            out.push_str(line);
            out.push('\n');
        }
        out
    }

    let mut source = String::new();
    for path in [
        "src/runtime.rs",
        "src/runtime_core.rs",
        "src/observe/core.rs",
        "src/eff.rs",
        "src/endpoint.rs",
        "src/rendezvous.rs",
        "src/session.rs",
        "src/transport.rs",
        "src/global/compiled/images/program.rs",
        "src/global/compiled/images/image/program_ref.rs",
    ] {
        source.push_str(&strip_cfg_test_modules(&read(path)));
    }
    for path in [
        "src/runtime",
        "src/runtime_core",
        "src/global/typestate",
        "src/endpoint",
        "src/rendezvous",
        "src/session",
        "src/transport",
    ] {
        source.push_str(&strip_cfg_test_modules(&read_production_rs_tree(path)));
    }

    for forbidden in [
        "expect(\"invariant\")",
        "panic!(\"invariant\")",
        "offer ingress cannot stage two transport frames",
        "offer transport wait must not poll while a received frame is already staged",
        "transport receive frame polled while current frame receipt is unresolved",
        "transport receive frame receipt is no longer current",
        "offer entry table mutation requires caller-owned storage",
        "dense active lane ordinal fits u16",
        "committed wire decode must retain staged payload",
    ] {
        assert!(
            !source.contains(forbidden),
            "runtime production path must use typed errors or crate::invariant(), not string invariant panics: {forbidden}"
        );
    }

    let mut offenders = Vec::new();
    for line in source.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("panic!(\"")
            || ((trimmed.starts_with("assert!(")
                || trimmed.starts_with("assert_eq!(")
                || trimmed.starts_with("assert_ne!(")
                || trimmed.starts_with("debug_assert!(")
                || trimmed.starts_with("debug_assert_eq!(")
                || trimmed.starts_with("debug_assert_ne!("))
                && trimmed.contains(", \""))
        {
            offenders.push(trimmed.to_owned());
        }
    }
    assert!(
        offenders.is_empty(),
        "runtime production path must not keep format panic or string assert invariant paths: {}",
        offenders.join(" | ")
    );
}

#[test]
fn tap_ring_storage_shape_is_a_single_type_sized_wrap_safe_ring() {
    let core = read("src/observe/core.rs");
    let ring_state = read("src/observe/core/ring_state.rs");
    assert!(
        core.contains("TAP_RESIDENT_BYTE_LIMIT: usize = 256")
            && core.contains("TAP_RESIDENT_BYTES: usize")
            && core.contains("RingBuffer::from_ptr(storage.as_mut_ptr())")
            && core.contains("PhantomData<&'a mut [TapRecord; TAP_EVENTS]>")
            && core.contains("state: Cell<RingState>")
            && core.contains("let idx = state.write_index() as usize")
            && core.contains("cursor: head.wrapping_sub(state.resident_len() as usize)")
            && core.contains("index: state.oldest_index()"),
        "tap ring storage must derive its record count from one 256-byte upper bound and index independently of the wrapping ordinal"
    );
    let observe = format!("{core}\n{ring_state}");
    for forbidden in [
        "RingBuffer::new",
        "assert!(storage.len()",
        "storage.len() >=",
        "RING_BUFFER_SIZE",
        "USER_EVENT_RANGE_END",
        "storage.add(RING_BUFFER_SIZE)",
        "storage.add(TAP_EVENTS)",
        "tap_event_precedes",
        "head % TAP_EVENTS",
        "head.saturating_sub(TAP_EVENTS)",
    ] {
        assert!(
            !observe.contains(forbidden),
            "tap ring construction must not keep split-ring or slice-length fallback residue: {forbidden}"
        );
    }
}

#[test]
fn source_tree_does_not_retain_impossible_test_only_helpers() {
    let source = read_all_rs_tree("src");
    let forbidden_route_ack_dispatch = "dispatch_topology_ack_with_handle";
    for forbidden in [
        "CpCommand",
        "PendingEffect",
        "EffectRunner",
        "DelegateOperands",
        "delegate_resolver",
        "endpoint_delegate",
        "invalid delegate token",
        "run_effect_step",
        "after_local_effect",
        forbidden_route_ack_dispatch,
        "synthetic_for_test",
        "transport_for_test",
        "add_rendezvous_auto",
        "NonNull::dangling",
        "receipt: None",
        "fn discard_terminal(self) {}",
        "fn discard_terminal(self) {\n    }",
        "discard_terminal_ingress",
    ] {
        assert!(
            !source.contains(forbidden),
            "source tests must not retain test-only effect runners or impossible transport test support: {forbidden}"
        );
    }
}

#[test]
fn package_artifact_ships_self_contained_tests_and_excludes_repo_gates() {
    let cargo = read("Cargo.toml");
    let package_gate = read(".github/scripts/check_package_artifact.sh");
    assert!(
        cargo.contains("autotests     = false")
            && cargo.contains("[[test]]")
            && cargo.contains("name = \"ui\"")
            && cargo.contains("name = \"lane_lifecycle_tap\"")
            && cargo.contains("\"/tests/**\"")
            && cargo.contains("\"!/tests/semantic_surface.rs\"")
            && cargo.contains("\"!/tests/semantic_surface/**\"")
            && cargo.contains("\"!/tests/public_surface_guards.rs\"")
            && cargo.contains("\"!/tests/runtime_surface.rs\"")
            && !package_gate.contains("repo repository tests must not ship")
            && !package_gate.contains("run_package_clean_with_omitted_repo_tests")
            && !package_gate.contains("run_package_with_repo_test_exclusions")
            && !package_gate.contains("warning: ignoring test `")
            && package_gate.contains("package test target declaration drift")
            && package_gate.contains("package artifact check detected warnings in:"),
        "package tests must be explicit so repo-only gates stay outside the crate package without Cargo package warnings"
    );
    assert!(
        package_gate.contains("run_package_clean \"cargo package --no-verify\"")
            && package_gate.contains("packaged tests must include their module tree")
            && package_gate.contains("package UI harness")
            && package_gate.contains("--test ui")
            && package_gate.contains("-- --list")
            && package_gate.contains("package behavior test")
            && package_gate.contains("--test lane_lifecycle_tap")
            && package_gate.contains("repo-only gate source shipped in crate package")
            && package_gate.contains(".github/measurement_snapshots/")
            && package_gate.contains("cargo +\"${TOOLCHAIN}\" test --manifest-path"),
        "package artifact gate must reject package warnings, list the packaged UI harness, and execute self-contained packaged behavior tests"
    );
}
#[test]
fn cached_recv_meta_index_overflow_fails_closed() {
    fn impl_fn_body<'a>(source: &'a str, name: &str) -> &'a str {
        let marker = format!("fn {name}(");
        let tail = source
            .split(&marker)
            .nth(1)
            .unwrap_or_else(|| panic!("{name} must stay visible"));
        let next = tail
            .find("\n    #[inline]\n    fn ")
            .or_else(|| tail.find("\n    fn "))
            .unwrap_or(tail.len());
        &tail[..next]
    }
    let source = read("src/endpoint/kernel/core/decision_resolver/impls/select.rs");
    for name in [
        "cached_recv_meta_from_recv",
        "cached_recv_meta_from_send",
        "cached_recv_meta_from_local",
        "route_arm_cached_recv_meta",
    ] {
        let body = impl_fn_body(&source, name);
        assert!(
            body.contains("checked_state_index("),
            "{name} must keep StateIndex bounds explicit"
        );
        assert!(
            body.contains("crate::invariant()"),
            "{name} must fail closed when descriptor/cursor indices cannot fit StateIndex"
        );
        assert!(
            !body.contains("return CachedRecvMeta::EMPTY;"),
            "{name} must not hide index overflow as missing receive evidence"
        );
    }
}
