use super::common::*;

fn named_struct_body<'a>(source: &'a str, name: &str) -> &'a str {
    let marker = format!("struct {name} {{");
    let tail = source
        .split(&marker)
        .nth(1)
        .unwrap_or_else(|| panic!("{name} struct must stay visible"));
    tail.split("\n}")
        .next()
        .unwrap_or_else(|| panic!("{name} struct body must stay visible"))
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
fn production_sources_do_not_retain_test_only_effect_or_offer_helpers() {
    let production = read_production_rs_tree("src");
    for forbidden in [
        concat!("for_", "test"),
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
        concat!("control", "_op_is_idempotent"),
        concat!("control", "_op_requires_gen_bump"),
        concat!("control", "_op_is_terminal"),
        concat!("control", "_op_modifies_history"),
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
    let source = read("src/endpoint/kernel/core/decision_resolver/impls.rs");
    let audit_fn = source
        .split("fn emit_decision_resolver_audit")
        .nth(1)
        .and_then(|tail| tail.split("fn evaluate_arm_decision_resolver").next())
        .expect("decision resolver audit emit helper must stay local");

    assert!(
        !audit_fn.contains("SendResult") && !audit_fn.contains("Ok(())"),
        "resolver audit emit must not return Result when it has no error source"
    );
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
        concat!("Loop", "Body", "Mis", "sing"),
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
        concat!("Raw", "Offer", "Lease::new("),
        concat!("Raw", "Recv", "Flags::new("),
        concat!("struct Raw", "Offer", "Lease"),
        concat!("struct Raw", "Recv", "Flags"),
        concat!("Raw", "Offer", "Lease::from_held_lease"),
        concat!("Raw", "Recv", "Flags::from_held_lease"),
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
        "active: true,\n            ..OfferEntryState::EMPTY",
        "active: true,\n                lane_idx",
        "public_slot_owned: bool",
        "public_slot_owned: true",
        "self.public_slot_owned = false",
        "init_public_offer_state(&mut self) -> bool",
        "init_public_send_state(&mut self, init: &SendInit) -> bool",
        "init_public_recv_state(&mut self) -> bool",
        "begin_public_decode_state(&mut self) -> bool",
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
        concat!("lin", "ger"),
        concat!("Lin", "ger"),
        concat!("LIN", "GER"),
        "Option<bool>",
        "Result<Option<bool>",
        "RecvResult<Option<bool>",
        "Some(false)",
        "then_some(false)",
        "reentry: bool",
        "is_reentry: bool",
        "route_offer_entry_matches_current",
        "is_reentry_route_from_cursor",
        concat!("Route", "Reentry"),
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
        !tests.contains(concat!(
            "fn ",
            "base",
            "line",
            "_left_resolver() -> Result<DecisionResolution, ResolverError>"
        )),
        "resolver tests must model the fallible resolver contract, not a constant-Ok helper"
    );
}

#[test]
fn production_sources_do_not_reintroduce_implicit_initializers() {
    let production = read_production_rs_tree("src");
    let trait_name = concat!("De", "fault");
    for line in production.lines() {
        let trimmed = line.trim_start();
        assert!(
            !(trimmed.starts_with("#[derive(") && trimmed.contains(trait_name)),
            "production source must use explicit empty/new/zero constructors, not derive({trait_name}): {line}"
        );
        assert!(
            !(trimmed.starts_with("impl") && trimmed.contains(concat!("De", "fault for"))),
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
        "ScopeTrace::default",
        "EffList::default",
        "RouteTable::default",
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
fn production_sources_do_not_reintroduce_static_hygiene_residue() {
    let production = read_production_rs_tree("src");
    for forbidden in [
        "#[allow(dead_code)]",
        "legacy",
        "heuristic",
        "fallback",
        "infer",
        "absorbed",
        "keeps_waiting",
        "DiscardedAndPending",
        "mismatch must stay pending",
        "mismatch discard",
    ] {
        assert!(
            !production.contains(forbidden),
            "production source must not re-grow static hygiene residue: {forbidden}"
        );
    }

    let mut inline_in_attr_group = false;
    for line in production.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("#[") {
            if trimmed == "#[inline]" {
                assert!(
                    !inline_in_attr_group,
                    "production source must not duplicate #[inline] in one attribute group"
                );
                inline_in_attr_group = true;
            }
        } else if !trimmed.is_empty() {
            inline_in_attr_group = false;
        }
    }

    assert!(
        !production.contains("wake_by_ref();\n        return Poll::Pending;"),
        "transport mismatch paths must not wake and return Pending"
    );
    assert!(
        !production.contains("wake_by_ref();\n            Poll::Pending"),
        "transport mismatch paths must not wake and stay Pending"
    );
}

fn assert_frame_label_mask_byte_limb_storage(labels: &str) {
    let body = named_struct_body(labels, "FrameLabelMask");
    assert!(
        labels.contains("#[repr(transparent)]"),
        "FrameLabelMask must stay a transparent byte-limb wrapper"
    );
    assert_eq!(
        body.trim(),
        "limbs: [u8; 32],",
        "FrameLabelMask must stay a fixed [u8; 32] mask"
    );
    assert!(
        labels.contains("limbs[(frame_label >> 3) as usize]"),
        "FrameLabelMask indexing must stay byte-limb based"
    );
    assert!(
        labels.contains("1u8 << (frame_label & 7)"),
        "FrameLabelMask bit construction must stay u8 based"
    );
}

fn assert_frame_label_mask_ops_use_no_u64(labels: &str) {
    for forbidden in [
        "u64", "1u64", "[u64;", "word0", "word1", "word2", "word3", ">> 6", "<< 6", "* 64", "/ 64",
    ] {
        assert!(
            !labels.contains(forbidden),
            "FrameLabelMask must not re-grow wide integer storage or helpers: {forbidden}"
        );
    }
}

#[test]
fn frame_label_mask_has_no_u64_storage() {
    let labels = read("src/transport/labels.rs");
    assert_frame_label_mask_byte_limb_storage(&labels);
}

#[test]
fn frame_label_mask_ops_do_not_use_u64() {
    let labels = read("src/transport/labels.rs");
    assert_frame_label_mask_ops_use_no_u64(&labels);
}

#[test]
fn scope_frame_label_meta_does_not_own_frame_label_masks() {
    let evidence = read("src/endpoint/kernel/evidence.rs");
    let meta = named_struct_body(&evidence, "ScopeFrameLabelMeta");
    let masks = named_struct_body(&evidence, "ScopeFrameLabelMasks");
    let scratch = named_struct_body(&evidence, "ScopeFrameLabelScratch");
    let view = named_struct_body(&evidence, "ScopeFrameLabelView<'a>");

    for required in [
        "recv_frame_label: u8",
        "recv_arm: u8",
        "controller_frame_labels: [u8; 2]",
        "flags: u8",
    ] {
        assert!(
            meta.contains(required),
            "ScopeFrameLabelMeta must keep only scalar route facts: {required}"
        );
    }
    for forbidden in [
        "FrameLabelMask",
        "ScopeFrameLabelMasks",
        "limbs",
        "[u8; 32]",
    ] {
        assert!(
            !meta.contains(forbidden),
            "ScopeFrameLabelMeta must not own frame-label masks: {forbidden}"
        );
    }
    assert!(
        scratch.contains("masks: ScopeFrameLabelMasks")
            && view.contains("masks: &'a ScopeFrameLabelMasks"),
        "mask ownership must stay confined to scratch and borrowed through ScopeFrameLabelView"
    );
    assert_eq!(
        masks.trim(),
        "pub(super) arm_frame_label_masks: [FrameLabelMask; 2],",
        "ScopeFrameLabelMasks must not re-grow duplicate per-arm mask sets"
    );
}

#[test]
fn no_by_value_scope_frame_label_meta_in_hot_paths() {
    let hot_path = [
        read("src/endpoint/kernel/core/frontier_helpers.rs"),
        read("src/endpoint/kernel/core/scope_evidence_logic.rs"),
        read("src/endpoint/kernel/offer.rs"),
        read("src/endpoint/kernel/offer/facts.rs"),
        read("src/endpoint/kernel/offer/passive.rs"),
        read("src/endpoint/kernel/offer/select.rs"),
    ]
    .join("\n");
    for forbidden in [
        "ScopeFrameLabelMasks::EMPTY",
        "frame_label_masks",
        "frame_label_meta: &ScopeFrameLabelMeta",
        ") -> ScopeFrameLabelMeta",
        ".frame_hint_mask(&",
        "fn selection_frame_label_meta(",
        "fn offer_scope_frame_label_meta(",
        "fn scope_frame_label_meta(",
        "fn scope_frame_label_meta_at(",
    ] {
        assert!(
            !hot_path.contains(forbidden),
            "scope frame-label hot path must use ScopeFrameLabelScratch/View, not by-value mask plumbing: {forbidden}"
        );
    }
    assert!(
        hot_path.contains("ScopeFrameLabelScratch::EMPTY")
            && hot_path.contains("ScopeFrameLabelView<'_>"),
        "offer hot paths must build scratch locally and pass ScopeFrameLabelView"
    );
}

#[test]
fn production_sources_keep_absence_codes_named_by_meaning() {
    let production = read_production_rs_tree("src");
    for forbidden in [
        concat!("PROGRAM_IMAGE_", "NO", "_ROUTE_CONTROLLER"),
        concat!("EventSemanticKind::", "O", "ther"),
        concat!("EVENT_CURSOR_", "NO", "_STATE"),
        concat!("NO", "_SELECTED_ARM"),
        concat!("NO", "_ACTIVE_LANE"),
        concat!("RouteTable::", "NO", "_FRAME"),
        concat!("Self::", "NO", "_FRAME"),
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
        concat!("Invalid(&'", "static str)"),
        concat!("CodecError::", "Invalid"),
        concat!("CodecError::", "Invalid("),
        "\n    Invalid,\n",
        concat!("ERR_", "PAYLOAD_LEN"),
        concat!("ERR_", "ZERO_PAYLOAD"),
        concat!("ERR_", "BOOLEAN_PAYLOAD"),
        concat!(
            "require_exact_len(input.as_bytes().len(), 20, ",
            "\"payload length\")"
        ),
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
fn public_failure_evidence_is_compact_operation_only() {
    for (name, source) in [
        ("EndpointError", endpoint_facade_source()),
        ("AttachError", read("src/session/cluster/error.rs")),
        ("ResolverError", cluster_core_source()),
    ] {
        let required = "pub const fn operation(&self) -> &'static str";
        assert!(
            source.contains(required),
            "{name} must keep compact public operation diagnostics: {required}"
        );
        for forbidden in [
            "pub const fn file(&self) -> &'static str",
            "pub const fn line(&self) -> u32",
            "pub const fn column(&self) -> u32",
        ] {
            assert!(
                !source.contains(forbidden),
                "{name} must not expose source-location diagnostics: {forbidden}"
            );
        }
    }
}

#[test]
fn production_callsite_location_storage_is_std_cfg_only() {
    let production = read_production_rs_tree("src");
    let diag = read("src/diag.rs");
    let without_diag = production.replace(&diag, "");

    assert!(
        diag.contains("pub(crate) struct Callsite")
            && diag.contains("#[cfg(feature = \"std\")]")
            && diag.contains("#[cfg(not(feature = \"std\"))]")
            && diag.contains("use core::panic::Location;")
            && diag.contains("location: &'static Location<'static>")
            && diag.contains("Location::caller()"),
        "diag::Callsite must keep Location behind the std cfg and provide a no_std compact shape"
    );
    for forbidden in [
        "core::panic::Location",
        "panic::Location",
        "Location::caller()",
        "&'static Location<'static>",
        "ErrorLocation",
        "ResolverErrorLocation",
    ] {
        assert!(
            !without_diag.contains(forbidden),
            "production source must not duplicate callsite Location storage outside diag::Callsite: {forbidden}"
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
fn tap_ring_storage_shape_is_a_type_sized_dual_ring() {
    let observe = read("src/observe/core.rs");
    assert!(
        observe.contains("const _: [(); RING_EVENTS] = [(); RING_BUFFER_SIZE * 2];"),
        "tap ring storage layout must be fixed by a compile-time equality"
    );
    for forbidden in [
        "RingBuffer::new",
        "assert!(storage.len()",
        "storage.len() >= RING_BUFFER_SIZE",
    ] {
        assert!(
            !observe.contains(forbidden),
            "tap ring construction must not keep slice-length runtime fallback: {forbidden}"
        );
    }
    assert!(
        observe.contains("RingBuffer::from_ptr(storage)")
            && observe.contains("storage.add(RING_BUFFER_SIZE)"),
        "tap ring halves must be split from the fixed storage array"
    );
}

#[test]
fn source_tree_does_not_retain_impossible_test_only_helpers() {
    let source = read_all_rs_tree("src");
    let forbidden_route_ack_dispatch = concat!("dispatch_", "topo", "logy", "_ack_with_handle");
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
        concat!("syn", "thetic", "_for_", "test"),
        concat!("transport_", "for_", "test"),
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
fn package_artifact_ships_repo_tests_without_publish_warning_filter() {
    let cargo = read("Cargo.toml");
    let package_gate = read(".github/scripts/check_package_artifact.sh");

    assert!(
        !cargo.contains("autotests")
            && !cargo.contains("[[test]]")
            && cargo.contains("\"/tests/**\"")
            && !package_gate.contains("repo repository tests must not ship")
            && !package_gate.contains("run_package_clean_with_omitted_repo_tests")
            && !package_gate.contains("ignoring test `"),
        "repo repository tests must stay Cargo-auto-discovered and ship with the crate so publish is warning-free"
    );
    assert!(
        package_gate.contains("run_package_clean \"cargo package --no-verify\"")
            && package_gate.contains("shipped repository tests must include their module tree")
            && package_gate.contains("package representative test build --features std")
            && package_gate.contains("--test semantic_surface --no-run")
            && package_gate.contains("cargo +\"${TOOLCHAIN}\" test --manifest-path"),
        "package artifact gate must reject package warnings and compile a representative packaged repository target"
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
