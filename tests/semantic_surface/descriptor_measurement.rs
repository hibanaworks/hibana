use super::common::*;

#[test]
fn effect_nodes_do_not_read_inactive_union_fields() {
    let eff = read("src/eff.rs");

    assert!(
        !eff.contains("pub union EffData") && !eff.contains("unsafe { self.atom }"),
        "effect nodes must not expose safe reads from inactive union fields"
    );
    assert!(
        eff.contains("pure effect node has no atom data"),
        "pure effect atom access must fail fast instead of returning untagged storage"
    );
}

#[test]
fn failure_cancellation_surface_has_only_domain_evidence() {
    let lib = read("src/lib.rs");
    let endpoint = endpoint_facade_source();
    let resolver = cluster_core_source();
    let attach = read("src/control/cluster/error.rs");
    let integration = integration_source();
    let runtime_config = read("src/runtime/config.rs");
    let transport = transport_source();
    let rendezvous_assoc = read("src/rendezvous/association.rs");
    let endpoint_core = endpoint_kernel_core_source();
    let offer_frontier = offer_frontier_source();
    let frontier_runtime = {
        let mut source = read("src/endpoint/kernel/frontier.rs");
        source.push_str(&read_production_rs_tree("src/endpoint/kernel/frontier"));
        source
    };
    let public_allowlists = [
        read(".github/allowlists/lib-public-api.txt"),
        read(".github/allowlists/g-public-api.txt"),
        read(".github/allowlists/endpoint-public-api.txt"),
        read(".github/allowlists/integration-public-api.txt"),
    ]
    .join("\n");

    for required in [
        "pub type EndpointResult<T> = core::result::Result<T, EndpointError>;",
        "pub use endpoint::{Endpoint, EndpointError, EndpointResult, Flow, RouteBranch};",
        "pub use crate::control::cluster::core::{ DecisionArm, DecisionResolution, ResolverContext, ResolverError, ResolverRef, };",
        "pub use crate::control::cluster::error::AttachError;",
        "pub fn add_rendezvous_from_config( &self, config: crate::integration::runtime::Config<'cfg, U, C>, transport: T, ) -> Result<crate::integration::ids::RendezvousId, AttachError> {",
    ] {
        assert!(
            public_allowlists.contains(required),
            "failure evidence surface missing required domain item: {required}"
        );
    }
    assert!(
        endpoint.contains("pub struct EndpointError {")
            && resolver.contains("pub struct ResolverError {")
            && attach.contains("pub struct AttachError {"),
        "domain evidence structs must exist without exposing public error-kind enums"
    );

    for (path, source) in [
        ("src/lib.rs", lib.as_str()),
        ("src/endpoint.rs", endpoint.as_str()),
        ("src/control/cluster/core.rs", resolver.as_str()),
        ("src/control/cluster/error.rs", attach.as_str()),
        ("src/integration.rs", integration.as_str()),
    ] {
        for forbidden in [
            "pub enum EndpointErrorKind",
            "pub struct EndpointErrorKind",
            "pub type EndpointErrorKind",
            "pub enum ResolverErrorKind",
            "pub struct ResolverErrorKind",
            "pub type ResolverErrorKind",
            "pub enum AttachErrorKind",
            "pub struct AttachErrorKind",
            "pub type AttachErrorKind",
            "pub enum HibanaError",
            "pub struct HibanaError",
            "pub type HibanaError",
            "pub use crate::control::cluster::error::{AttachError, CpError, ResourceScope};",
            "pub use crate::control::cluster::error::{AttachError, CpError};",
            "recv_timeout",
            "send_timeout",
            "offer_timeout",
            "decode_timeout",
            "try_recover",
            "ignore_fault",
            "reconnect",
        ] {
            assert!(
                !source.contains(forbidden),
                "{path} must not expose failure/cancellation escape hatch: {forbidden}"
            );
        }
    }

    assert!(
        endpoint.contains("#[track_caller]\n    pub fn flow")
            && endpoint.contains("#[track_caller]\n    pub fn recv")
            && endpoint.contains("#[track_caller]\n    pub fn offer")
            && endpoint.contains("#[track_caller]\n    pub fn decode"),
        "endpoint operations must capture caller location at the public boundary"
    );
    assert!(
        read("src/endpoint/flow.rs").contains("#[track_caller]\n    pub fn send"),
        "flow send must capture caller location at the public boundary"
    );
    assert!(
        resolver.contains("#[track_caller]\n    pub fn reject")
            && integration.contains("#[track_caller]\n    pub fn add_rendezvous_from_config")
            && integration.contains("#[track_caller]\n    pub fn enter")
            && integration.contains("#[track_caller]\n    pub fn set_resolver"),
        "resolver and attach boundaries must capture caller location"
    );
    assert!(
        !runtime_config.contains("OperationalDeadline")
            && !rendezvous_assoc.contains("DeadlineExceeded")
            && !transport.contains("fn operational_deadline_ticks(&self)")
            && !runtime_config.contains("operational_deadline_ticks")
            && !runtime_config.contains("with_operational_deadline_ticks")
            && endpoint.contains("SessionFault(crate::rendezvous::SessionFaultKind)")
            && rendezvous_assoc.contains("pub(super) fn poison_session"),
        "failure evidence must not keep hidden deadline fuses or public timeout APIs"
    );
    assert!(
        read("tests/cursor_send_recv/session_lifecycle.rs")
            .contains("dropping_live_endpoint_poison_wakes_waiting_peer")
            && read("tests/offer_decode_binding_regression/decode_lifecycle.rs")
                .contains("SessionFault"),
        "session fault cleanup must be behavior-covered instead of pinned to private cleanup helper names"
    );
    assert!(
        read_offer_tests().contains("reset_public_offer_state_restores_carried_binding_evidence")
            && read_offer_tests()
                .contains("reset_public_offer_state_restores_carried_transport_payloads")
            && read_offer_tests().contains("terminal_offer_clear_discards_carried_preview_state"),
        "non-terminal offer reset and terminal offer clear must be guarded by behavior coverage, not private rollback container shape"
    );
    assert!(
        runtime_config.contains("struct OfferProgressPolicy")
            && runtime_config.contains("pub fn from_resources(")
            && !runtime_config.contains("pub fn new(")
            && runtime_config.contains("pub(crate) fn initial_lane_range()")
            && !runtime_config.contains("derived_endpoint_slots")
            && !runtime_config.contains("lane_range: Range")
            && !runtime_config.contains("endpoint_slots: usize")
            && !runtime_config.contains("max_defer")
            && !runtime_config.contains("force_poll")
            && !resolver.contains("retry_hint")
            && !offer_frontier.contains("retry_hint")
            && !offer_frontier.contains("force_poll")
            && !offer_frontier.contains("PolicyAbort {\n                    reason:")
            && frontier_runtime.contains("enum OfferEvidenceOutcome")
            && frontier_runtime.contains("enum FrontierDeferOutcome")
            && frontier_runtime.contains("Pending,"),
        "integration config and offer progress must derive runtime shape and expose only Evidence/Pending/Fault, not offer-time heuristics"
    );
    assert!(
        rendezvous_assoc.contains("EndpointDropped")
            && rendezvous_assoc.contains("register_waiter")
            && rendezvous_assoc.contains("wake_session_waiters")
            && endpoint_core.contains("SessionFaultKind::EndpointDropped"),
        "session poison must wake registered waiters and live endpoint drop must become terminal evidence"
    );
}

#[test]
fn resident_descriptor_attach_has_no_lowering_materialization_path() {
    let compiled_mod = read("src/global/compiled/mod.rs");
    let lowering_mod = read("src/global/compiled/lowering/mod.rs");
    let rendezvous = rendezvous_core_source();
    let cluster = cluster_core_source();
    let endpoint_core = endpoint_kernel_core_source();
    let cluster_runtime = cluster
        .split_once("\n#[cfg(test)]\nmod tests")
        .map(|(runtime, _)| runtime)
        .unwrap_or(cluster.as_str());

    assert!(
        !compiled_mod.contains("mod materialize")
            && !compiled_mod.contains("mod layout")
            && !lowering_mod.contains("program_image_builder")
            && !lowering_mod.contains("program_tail_storage")
            && !lowering_mod.contains("role_image_builder")
            && !lowering_mod.contains("role_scope_storage")
            && !lowering_mod.contains("role_image_lowering"),
        "transient lowering/materialization builders must not remain, even behind cfg(test)"
    );

    for forbidden in [
        "with_lowering_lease",
        "LoweringLeaseMode",
        "RoleLoweringScratch",
        "MaterializedRoleImage",
        "CompiledProgramFacts",
        "materialize_program_image_from_",
        "materialize_role_image_from_",
        "pin_endpoint_images",
        "RoleImageSlice::from_raw(",
        "CompiledProgramRef::from_raw(",
        "scratch_reserved_bytes",
        "program_images",
        "role_images",
    ] {
        assert!(
            !cluster_runtime.contains(forbidden)
                && !rendezvous.contains(forbidden)
                && !compiled_mod.contains(forbidden)
                && !lowering_mod.contains(forbidden),
            "runtime attach path must not keep transient materialization primitive: {forbidden}"
        );
    }

    let role_image = compiled_image_source();
    assert!(
        cluster_runtime.contains("let compiled = program.compiled_role_image();")
            && cluster_runtime.contains("RoleImageSlice::from_resident(compiled)")
            && cluster_runtime.contains("program.compiled_role_image().program()")
            && !cluster_runtime.contains("RoleImageSlice::from_raw(")
            && !cluster_runtime.contains("CompiledProgramRef::from_raw(")
            && !cluster_runtime.contains("CompiledProgramRef::from_")
            && role_image.contains("program: compiled.program()")
            && role_image.contains("resident: compiled")
            && role_image.contains(
                "pub(crate) const fn from_resident(compiled: &'static CompiledRoleImage)"
            )
            && !role_image.contains("RoleDescriptorSource"),
        "runtime attach must consume a pre-existing resident CompiledRoleImage that already carries its program descriptor"
    );

    assert!(
        !rendezvous.contains("materialize_")
            && !rendezvous.contains("compiled_ptr")
            && !rendezvous.contains("scratch_reserved_bytes")
            && !role_image.contains("Materialized")
            && !role_image.contains("from_raw("),
        "attach is resident descriptor reference only; no scratch-backed or test-only compatibility path may remain"
    );

    for forbidden in [
        "struct PreparedSendControl",
        "stage_payload:",
        "fn stage_data_send_payload",
        "fn stage_registered_send_payload",
        "fn stage_emitted_send_payload",
        "fn stage_explicit_wire_control_payload",
        "prepare_send_control",
    ] {
        assert!(
            !endpoint_core.contains(forbidden),
            "send control staging must be direct and resident-descriptor derived; no indirect compatibility plan may remain: {forbidden}"
        );
    }
}

#[test]
fn projection_metadata_and_lane_domain_stay_embedded_exact() {
    let program = read("src/global/program.rs");
    let projection = read("src/global/program/projection.rs");
    let source = read("src/global/program/source.rs");
    let role_image = compiled_image_source();

    for forbidden in [
        "ProjectionTypeFingerprint",
        "ProjectionMessageSpec",
        "VisitProjectionMessages",
        "visit_projection_messages",
        "visit_message",
        "core::any::type_name",
    ] {
        assert!(
            !program.contains(forbidden)
                && !projection.contains(forbidden)
                && !source.contains(forbidden)
                && !integration_source().contains(forbidden),
            "projection metadata public/runtime path must be Pico-compatible numeric facts only: {forbidden}"
        );
    }
    let projectable = program
        .split("impl<Universe, Steps> Projectable<Universe> for Program<Steps>")
        .nth(1)
        .and_then(|tail| tail.split("pub const fn seq").next())
        .expect("Projectable impl");
    assert!(
        projectable.contains("Steps: BuildProgramSource")
            && !program.contains(
                "#[cfg(any(feature = \"std\", test))]\nimpl<Universe, Steps> Projectable"
            )
            && !program.contains(
                "#[cfg(not(any(feature = \"std\", test)))]\nimpl<Universe, Steps> Projectable"
            ),
        "projection metadata must use one Pico-compatible Projectable impl, not std/test split metadata"
    );
    assert!(
        !program.contains("pub const fn embedded") && !projection.contains("pub const fn embedded"),
        "embedded projection fingerprint fallback is an internal representation detail, not public API"
    );

    let role_program = {
        let mut source = read("src/global/role_program.rs");
        source.push_str(&read_production_rs_tree("src/global/role_program"));
        source
    };
    assert!(
        role_program.contains("pub(crate) struct LaneSetView<'a> {")
            && role_program.contains("_marker: PhantomData<&'a [LaneWord]>")
            && role_program.contains("byte_len: u16")
            && role_program.contains("pub(crate) const fn from_bytes")
            && !role_program.contains("struct LaneSetSnapshot")
            && !role_program.contains("LaneSetSnapshot::from_view")
            && role_program.contains("struct RoleLaneImage")
            && role_program.contains("local_step_lanes: [u8; MAX_LOCAL_STEP_LANES]")
            && role_program.contains("phase_boundaries: [u16; MAX_PHASE_BOUNDARY_ROWS]")
            && role_program.contains("phase_lane_bit_boundaries: [u16; MAX_PHASE_BOUNDARY_ROWS]")
            && role_program.contains("lane_bit_rows: [u8; MAX_RESIDENT_LANE_BIT_BYTES]")
            && !role_program.contains("phase_rows: [PackedLaneRange; MAX_PHASE_LANE_ROWS]")
            && !role_program.contains("active_words: [LaneWord; LANE_SET_VIEW_WORDS]")
            && !role_program.contains("phase_words: [LaneWord; LANE_SET_VIEW_WORDS]")
            && role_program
                .contains("route_arm_lane_rows: [PackedLaneRange; MAX_ROUTE_ARM_LANE_ROWS]")
            && role_program
                .contains("route_offer_lane_rows: [PackedLaneRange; MAX_ROUTE_SCOPE_LANE_ROWS]")
            && !role_program.contains("from_lanes")
            && !role_program.contains("local_lane_view")
            && !role_program
                .contains("phase_step_rows: [PackedPhaseLaneStep; MAX_PHASE_LANE_STEP_ROWS]")
            && role_program.contains("MAX_LOCAL_STEP_LANES: usize = crate::eff::meta::MAX_EFF_NODES")
            && role_program.contains(
                "MAX_ROUTE_SCOPE_LANE_ROWS: usize = crate::eff::meta::MAX_EFF_NODES / 2"
            )
            && role_program.contains("MAX_ROUTE_ARM_LANE_ROWS: usize = MAX_ROUTE_SCOPE_LANE_ROWS * 2")
            && !role_program.contains(
                "MAX_LOCAL_STEP_LANES: usize =\n    crate::global::compiled::images::MAX_COMPILED_PROGRAM_TAP_EVENTS"
            )
            && !role_program.contains("route_arm_lane_entries: [u8; MAX_ROUTE_ARM_LANE_ENTRIES]")
            && role_program.contains("phase_row_len: u16")
            && !role_program.contains("phase_steps: [LaneSteps; LANE_DOMAIN_SIZE]")
            && !role_program.contains("PhaseLaneEntry")
            && !lowering_driver_source().contains("fill_role_atom_lanes_in_range")
            && !offer_frontier_source()
                .split("struct OfferFrontierFacts {")
                .nth(1)
                .and_then(|tail| tail.split("}").next())
                .unwrap_or("")
                .contains("LaneSetView")
            && !role_image
                .contains("[DENSE_LANE_NONE; crate::global::role_program::LANE_DOMAIN_SIZE]")
            && !role_image.contains("[DENSE_LANE_NONE; LANE_DOMAIN_SIZE]")
            && role_image.contains(".role_image().active_lane_set()")
            && role_image.contains(".role_image().phase_lane_set(idx)")
            && !read("src/endpoint/kernel/decision_state.rs")
                .contains("route_scope_lane_words")
            && !read("src/endpoint/kernel/endpoint_init.rs")
                .contains("set_route_scope_arm_lane_set")
            && endpoint_kernel_core_source()
                .contains(".route_scope_offer_lane_set(scope_id)")
            && endpoint_kernel_core_source()
                .contains("self.cursor.route_scope_arm_lane_set(scope_id, arm)")
            && !role_image
                .split("pub(crate) fn route_scope_arm_lane_set_by_slot")
                .nth(1)
                .and_then(|tail| tail
                    .split("pub(crate) fn route_scope_offer_lane_set_by_slot")
                    .next())
                .unwrap_or("")
                .contains("view.len()")
            && !role_image
                .split("pub(crate) fn route_scope_arm_lane_set_by_slot")
                .nth(1)
                .and_then(|tail| tail
                    .split("pub(crate) fn route_scope_offer_lane_set_by_slot")
                    .next())
                .unwrap_or("")
                .contains("fill_role_atom_lanes_in_range")
            && !role_image
                .split("pub(crate) fn phase_lane_set(&self, idx: usize)")
                .nth(1)
                .and_then(|tail| tail.split("pub(crate) fn phase_min_start").next())
                .unwrap_or("")
                .contains("while")
            && !role_image
                .split("pub(crate) fn fill_active_lane_dense_by_lane")
                .nth(1)
                .and_then(|tail| tail
                    .split("pub(crate) fn fill_logical_lane_dense_by_lane")
                    .next())
                .unwrap_or("")
                .contains("view.len()"),
        "resident lane queries must read exact lane bitmap rows and avoid effect-list scans on attach/frontier hot paths"
    );
}

#[test]
fn resident_descriptor_metadata_stays_columnar() {
    let lowering = lowering_driver_source();
    let segment = lowering
        .split("struct ProgramImageSegmentData {")
        .nth(1)
        .and_then(|tail| tail.split("impl ProgramImageSegmentData").next())
        .expect("ProgramImageSegmentData section");

    assert!(
        segment.contains("atom_mask: u128")
            && !segment.contains("nodes: [EffStruct; MAX_SEGMENT_EFFS]")
            && !segment.contains("steps: [ProgramStepRow; MAX_SEGMENT_EFFS]")
            && !segment.contains("policies: [PolicyMode; MAX_SEGMENT_EFFS]")
            && !segment.contains("control_descs: [Option<ControlDesc>; MAX_SEGMENT_EFFS]")
            && segment.contains("atom_row_start: u16")
            && segment.contains("atom_row_len: u16")
            && segment.contains("policy_row_start: u16")
            && segment.contains("policy_row_len: u16")
            && !segment.contains("route_scope_row_start: u16")
            && !segment.contains("route_scope_row_len: u16")
            && segment.contains("control_desc_row_start: u16")
            && segment.contains("control_desc_row_len: u16")
            && !lowering.contains("struct ProgramRouteScopeRow")
            && lowering.contains("struct ProgramAtomRow")
            && lowering.contains("struct ProgramPolicyRow")
            && lowering.contains("struct ProgramControlDescRow")
            && lowering
                .contains("const MAX_COMPILED_ATOM_ROWS: usize = crate::eff::meta::MAX_EFF_NODES")
            && lowering.contains("const MAX_COMPILED_POLICY_ROWS: usize = MAX_SEGMENTS * 2")
            && lowering.contains("const MAX_COMPILED_CONTROL_DESC_ROWS: usize = MAX_SEGMENTS * 2")
            && lowering.contains("const MAX_COMPILED_CONTROL_MARKERS: usize = MAX_SEGMENTS * 2")
            && lowering.contains("policy_rows_complete: bool")
            && lowering.contains("control_desc_rows_complete: bool")
            && lowering.contains("control_markers_complete: bool")
            && lowering.contains(
                "pub(crate) const fn new(\n        policy_at: fn(usize) -> Option<PolicyMode>,"
            )
            && lowering.contains("return self.source_lookup.policy_at(offset);")
            && lowering.contains("return self.source_lookup.control_desc_at(offset);")
            && lowering
                .contains("const MAX_COMPILED_SCOPE_MARKERS: usize = MAX_COMPILED_PROGRAM_SCOPES")
            && !lowering.contains("const MAX_COMPILED_SCOPE_MARKERS: usize = MAX_SEGMENTS * 4")
            && lowering.contains("atom_rows: [ProgramAtomRow; MAX_COMPILED_ATOM_ROWS]")
            && !lowering.contains("pub(crate) type ProgramNodeAt")
            && !lowering.contains("source_node_at: ProgramNodeAt")
            && !lowering.contains("atom_rows: [EffAtom;")
            && !lowering.contains("atom_rows: [EffAtom; MAX_COMPILED_IMAGE_NODES]")
            && lowering.contains("offset_is_atom")
            && !lowering.contains("message_atoms")
            && !lowering.contains("self.atom_rows[offset]")
            && !lowering.contains("route_scope_rows: [ProgramRouteScopeRow")
            && lowering.contains("policy_rows: [ProgramPolicyRow; MAX_COMPILED_POLICY_ROWS]")
            && lowering.contains(
                "control_desc_rows: [ProgramControlDescRow; MAX_COMPILED_CONTROL_DESC_ROWS]"
            ),
        "resident descriptor metadata must stay columnar: segment rows own atoms and ranges, policy/control metadata live in side tables"
    );
}

#[test]
fn measurement_gates_prevent_recurrent_size_and_stack_regressions() {
    let final_gate = read(".github/scripts/check_final_form_measurements.sh");
    let worktree_gate = read(".github/scripts/check_size_snapshot_regression.sh");
    let performance_gate = read(".github/scripts/check_runtime_performance_hygiene.sh");
    let run_final_gate = read(".github/scripts/run_final_form_gates.sh");
    let snapshot = read(".github/measurement_snapshots/hibana-size-snapshot.json");
    let workflow = read(".github/workflows/quality-gates.yml");

    for required in [
        "if [[ \"${HIBANA_SKIP_FIXED_SNAPSHOT_CHECK:-0}\" != \"1\" ]]; then",
        "fixed snapshot thumb budget check skipped by explicit override; worktree regression gate still runs",
        "fixed snapshot runtime budget check skipped by explicit override; worktree regression gate still runs",
        "bash \"${ROOT_DIR}/.github/scripts/check_size_snapshot_regression.sh\"",
        "aggregate refactor gate requires ",
        "max_stack/sram/flash all <= snapshot budget and at least one decrease",
    ] {
        assert!(
            final_gate.contains(required),
            "final-form fixed snapshot/worktree gate missing required guard: {required}"
        );
    }

    for required in [
        "git worktree add --detach \"${BASE_WORKTREE}\" \"${BASE_REF}\"",
        "measure_tree \"base-${BASE_LABEL}\" \"${BASE_WORKTREE}\" \"${BASE_JSON}\"",
        "measure_tree \"current-${CURRENT_LABEL}\" \"${CURRENT_TREE}\" \"${CURRENT_JSON}\"",
        "metrics[\"localside_peak_stack_bytes\"] = metrics.get(\"peak_stack_bytes\", 0)",
        "os.environ[\"LABEL\"].startswith(\"base-\")",
        "hibana-projected-measure",
        "pub fn projected_pair() -> (RoleProgram<0>, RoleProgram<1>)",
        "projected_sections",
        "worktree-snapshot runtime-shape-stack shape={shape}",
        "worktree-snapshot runtime-shape-localside-stack shape={shape}",
        "SNAPSHOT_FILE=\"${ROOT_DIR}/.github/measurement_snapshots/hibana-size-snapshot.json\"",
        "budget_snapshot = json.load(f)",
        "worktree-snapshot budget-section {key} actual={actual} budget={maximum}",
        "section {key} exceeds snapshot budget",
        "worktree-snapshot budget-runtime shape={shape} {key} actual={actual} budget={maximum}",
        "runtime shape {shape} {key} exceeds snapshot budget",
        "worktree-snapshot budget-aggregate {name} actual={new} budget={maximum}",
        "aggregate snapshot budget gate failed: max_stack/sram/flash must all be <= budget ",
        "and at least one must decrease below budget",
    ] {
        assert!(
            worktree_gate.contains(required),
            "worktree size/stack regression gate missing required guard: {required}"
        );
    }

    for forbidden in [
        "measure_tree \"current-${CURRENT_LABEL}\" \"${CURRENT_TREE}\" \"${CURRENT_JSON}\" 1",
        "allow_probe_patch",
        "text.replace(",
        "path.write_text",
        "failed to inject localside stack probe",
        "refusing to patch current source",
        "HIBANA_SKIP_FIXED_SNAPSHOT_CHECK=0",
        "\"${CI:-false}\" != \"true\"",
        "CI/override",
    ] {
        assert!(
            !worktree_gate.contains(forbidden) && !final_gate.contains(forbidden),
            "size gate must not reintroduce current-tree self-patching or CI fixed-snapshot coupling: {forbidden}"
        );
    }

    assert!(
        workflow.contains("fetch-depth: 2")
            && workflow.contains("run: bash ./.github/scripts/run_final_form_gates.sh")
            && run_final_gate.contains("bash ./.github/scripts/check_unsafe_contract_hygiene.sh")
            && run_final_gate
                .contains("bash ./.github/scripts/check_surface_test_alias_hygiene.sh")
            && run_final_gate
                .contains("bash ./.github/scripts/check_runtime_performance_hygiene.sh")
            && final_gate.contains("HIBANA_SKIP_FIXED_SNAPSHOT_CHECK=1")
            && final_gate
                .contains("if [[ \"${HIBANA_SKIP_WORKTREE_SIZE_REGRESSION:-0}\" != \"1\" ]]; then"),
        "CI must run fixed Pico snapshots and the worktree regression gate unless an explicit local override is set"
    );
    let size_gate_pos = run_final_gate
        .find("bash ./.github/scripts/check_final_form_measurements.sh")
        .expect("final gate must include stack/SRAM/flash measurements");
    let performance_gate_pos = run_final_gate
        .find("bash ./.github/scripts/check_runtime_performance_hygiene.sh")
        .expect("final gate must include runtime performance hygiene");
    assert!(
        size_gate_pos < performance_gate_pos,
        "size/stack/SRAM/flash measurements must run before performance hygiene"
    );

    for required in [
        "\"hibana_0_6_0_baseline\"",
        "\"localside_peak_stack_bytes\"",
        "\"flash_total_formula\": \".text + .rodata + .data\"",
        "\".text\": 154624",
        "\".rodata\": 15341",
        "\"flash_total\": 169965",
        "\"policy\": \"Measured stack, SRAM, and flash values must satisfy",
    ] {
        assert!(
            snapshot.contains(required),
            "measurement snapshot must record the 0.6.0 physical baseline and localside stack budget: {required}"
        );
    }

    for required in [
        "Size is primary. This gate only blocks structural hot-path regressions",
        "LaneSetView::next_set_from must skip empty lane runs with bit operations",
        "compiled image hot path ",
        "must not rebuild lane sets by effect-list or full-view scans",
        "endpoint arena must not reintroduce route-scope lane-word caches",
        "preview_offer_entry_evidence_skips_binding_probe_when_ack_already_progresses_scope",
        "preview_offer_entry_evidence_defers_binding_poll_until_selected_scope",
        "poll_binding_for_offer_polls_only_selected_lane_for_unbuffered_generic_mask",
        "poll_binding_for_offer_polls_authoritative_demux_lane_when_current_lane_is_excluded",
        "static_passive_offer_with_known_arm_waits_on_transport_without_busy_restart",
        "nested_dispatch_arm_counts_as_recv_for_known_passive_route",
        "lane_set_view_iterates_set_bits_without_empty_lane_scan",
    ] {
        assert!(
            performance_gate.contains(required),
            "runtime performance hygiene gate missing required operation-count/source guard: {required}"
        );
    }
}
