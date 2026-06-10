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
        "pub use crate::control::cluster::core::{ DecisionArm, DecisionResolution, ResolverError, ResolverRef, };",
        "pub use crate::control::cluster::error::AttachError;",
        "pub fn rendezvous( &self, config: crate::integration::runtime::Config<'cfg, U, C>, transport: T, ) -> Result<RendezvousKit<'_, 'cfg, T, U, C, false, MAX_RV>, AttachError> {",
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
            && integration.contains("#[track_caller]\n    pub fn rendezvous")
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
            && read("tests/offer_decode_receive_evidence.rs")
                .contains("forgotten_decode_future_leaves_endpoint_fail_closed"),
        "session fault cleanup must be behavior-covered instead of pinned to private cleanup helper names"
    );
    assert!(
        !endpoint_kernel_source().contains("core_offer_tests")
            && !endpoint_kernel_source().contains("IngressInbox")
            && !endpoint_kernel_source().contains("PackedIngressEvidence")
            && !endpoint_kernel_source().contains("IngressSlot"),
        "offer regression coverage must not preserve deleted binding/inbox rollback structures"
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
        cluster_runtime.contains("let compiled = program.role_image_ref();")
            && cluster_runtime.contains("RoleImageSlice::from_resident(compiled)")
            && cluster_runtime.contains("program.role_image_ref().program")
            && !cluster_runtime.contains("RoleImageSlice::from_raw(")
            && !cluster_runtime.contains("CompiledProgramRef::from_raw(")
            && !cluster_runtime.contains("CompiledProgramRef::from_")
            && role_image.contains("Self { resident: image }")
            && role_image.contains("self.resident.program")
            && role_image
                .contains("pub(crate) const fn from_resident(image: &'static RoleImageRef)")
            && !role_image.contains("RoleDescriptorSource"),
        "runtime attach must consume a pre-existing resident RoleImageRef that reads its compact program descriptor directly"
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
fn projectable_bound_and_lane_domain_stay_embedded_exact() {
    let program = read("src/global/program.rs");
    let projection = read("src/global/program/projection.rs");
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
                && !integration_source().contains(forbidden),
            "projection public/runtime path must not retain stale metadata or type-name inspection: {forbidden}"
        );
    }
    let projectable = program
        .split("impl<Steps> projection::seal::Sealed for Program<Steps>")
        .nth(1)
        .and_then(|tail| tail.split("pub const fn seq").next())
        .expect("Projectable sealed impl");
    assert!(
        projectable.contains("Steps: crate::g::ProgramTerm")
            && !projectable.contains("ProgramTerm<Source")
            && program
                .contains("#[diagnostic::do_not_recommend]\nimpl<Steps> projection::seal::Sealed")
            && !program.contains("impl<Steps> Projectable for Program<Steps>")
            && projection.contains("pub trait Projectable: seal::Sealed")
            && projection.contains("impl<P> Projectable for P where P: seal::Sealed + ?Sized")
            && projection.contains("The trait is not an extension point.")
            && !program.contains("BuildProgramSource")
            && !projection.contains("BuildProgramSource")
            && !program.contains("#[cfg(any(feature = \"std\", test))]\nimpl<Steps> Projectable")
            && !program
                .contains("#[cfg(not(any(feature = \"std\", test)))]\nimpl<Steps> Projectable"),
        "projection must use one Pico-compatible Projectable impl, not std/test split metadata"
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
    let role_lane_image = role_program
        .split("pub(crate) struct RoleLaneImage")
        .nth(1)
        .and_then(|tail| tail.split("pub(crate) mod private").next())
        .expect("RoleLaneImage section must stay present");
    let role_lane_scratch = role_program
        .split("pub(crate) struct RoleLaneScratch")
        .nth(1)
        .and_then(|tail| tail.split("pub(crate) struct RoleImageRef").next())
        .expect("RoleLaneScratch section must stay present");
    assert!(
        role_program.contains("pub(crate) struct LaneSetView<'a> {")
            && role_program.contains("_marker: PhantomData<&'a [LaneWord]>")
            && role_program.contains("byte_len: u16")
            && role_program.contains("pub(crate) const fn from_bytes")
            && !role_program.contains("struct LaneSetSnapshot")
            && !role_program.contains("LaneSetSnapshot::from_view")
            && role_program.contains("struct RoleLaneImage")
            && role_program.contains("struct RoleLaneScratch")
            && role_program.contains("struct PackedLocalEventRow")
            && role_program.contains("scope_slot: u16")
            && role_program.contains("ROLE_IMAGE_EVENT_STRIDE: usize = 10")
            && !role_program
                .split("struct PackedLocalEventRow")
                .nth(1)
                .and_then(|tail| tail.split("struct PackedColumn").next())
                .unwrap_or("")
                .contains("CompactScopeId")
            && !role_image.contains("event.scope.raw()")
            && role_program.contains("struct PackedColumn")
            && role_program.contains("struct RoleImageColumns")
            && role_program.contains("struct RouteArmLaneStepRow")
            && role_program.contains("route_arm_lane_step_rows: PackedColumn")
            && role_program.contains("lane_step_row(self) -> PackedLaneRange")
            && role_program.contains("child_slot_delta(self) -> Option<u8>")
            && role_program.contains("passive_arm_child_ordinal_by_slot")
            && !role_program.contains("passive_children")
            && !role_program.contains("MAX_ROUTE_ARM_LANE_STEP_ROWS")
            && !role_lane_scratch.contains("route_arm_lane_step_rows: [")
            && !role_program.contains("route_arm_lane_first_steps")
            && !role_program.contains("route_arm_lane_last_steps")
            && !role_program.contains("route_arm_lane_step_len")
            && !role_program.contains("route_arm_lane_step_bounds")
            && role_lane_image.contains("columns: RoleImageColumns")
            && role_lane_image.contains("blob: &'static [u8]")
            && !role_lane_image.contains("local_step_events: &'static [PackedLocalEventRow]")
            && !role_lane_image.contains("local_step_lanes: &'static [u8]")
            && !role_lane_image.contains("resident_row_boundaries: &'static [u16]")
            && !role_program.contains("phase_lane_bit_boundaries")
            && !role_lane_image.contains("lane_bit_rows: &'static [u8]")
            && !role_lane_image.contains("route_arm_rows: &'static [PackedRouteArmRow]")
            && !role_lane_image.contains("route_offer_lane_rows: &'static [PackedLaneRange]")
            && !role_lane_image.contains("[LocalNode; MAX_LOCAL_STEP_LANES]")
            && !role_lane_image.contains("[u8; MAX_LOCAL_STEP_LANES]")
            && !role_lane_image.contains("[PackedLocalDependency; MAX_LOCAL_STEP_LANES]")
            && !role_lane_image.contains("[PackedEventConflict; MAX_LOCAL_STEP_LANES]")
            && !role_lane_image.contains("[PackedRouteArmRow; MAX_ROUTE_ARM_LANE_ROWS]")
            && !role_lane_image.contains("[PackedLaneRange; MAX_ROUTE_ARM_LANE_ROWS]")
            && !role_lane_image.contains("[PackedLaneRange; MAX_ROUTE_SCOPE_LANE_ROWS]")
            && role_lane_scratch.contains("local_step_events: [PackedLocalEventRow; MAX_LOCAL_STEP_LANES]")
            && role_lane_scratch.contains("local_step_lanes: [u8; MAX_LOCAL_STEP_LANES]")
            && role_lane_scratch.contains(
                "resident_row_boundaries: [u16; MAX_RESIDENT_ROW_BOUNDARY_ROWS]"
            )
            && role_lane_scratch.contains("lane_bit_rows: [u8; MAX_RESIDENT_LANE_BIT_BYTES]")
            && !role_program.contains("phase_rows: [PackedLaneRange; MAX_RESIDENT_ROW_LANE_ROWS]")
            && !role_program.contains("active_words: [LaneWord; LANE_SET_VIEW_WORDS]")
            && !role_program.contains("phase_words: [LaneWord; LANE_SET_VIEW_WORDS]")
            && role_lane_scratch
                .contains("route_arm_lane_rows: [PackedLaneRange; MAX_ROUTE_ARM_LANE_ROWS]")
            && role_lane_scratch
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
            && role_program.contains("resident_row_len: u16")
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
            && role_image.contains("self.image().active_lane_set()")
            && !role_image.contains(".phase_lane_set(idx)")
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
            && !role_image.contains("pub(crate) fn phase_lane_set")
            && role_program.contains("route_arm_lane_first_step_by_slot")
            && role_program.contains("route_arm_lane_last_step_by_slot")
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
            && !segment.contains("policies: [ResolverMode; MAX_SEGMENT_EFFS]")
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
            && lowering.contains(
                "const MAX_COMPILED_POLICY_ROWS: usize = crate::eff::meta::MAX_EFF_NODES"
            )
            && lowering.contains(
                "const MAX_COMPILED_CONTROL_DESC_ROWS: usize = crate::eff::meta::MAX_EFF_NODES"
            )
            && lowering.contains("const MAX_COMPILED_CONTROL_MARKERS: usize = MAX_SEGMENTS * 2")
            && !lowering.contains("policy_rows_complete: bool")
            && !lowering.contains("control_desc_rows_complete: bool")
            && lowering.contains("control_markers_complete: bool")
            && !lowering.contains("ProgramSourceLookup")
            && !lowering.contains("self.source_lookup.policy_at(offset)")
            && !lowering.contains("self.source_lookup.control_desc_at(offset)")
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
fn compact_bucket_overflow_paths_stay_fail_closed() {
    let program_blob = read("src/global/compiled/images/image/blob_storage.rs");
    let role_blob = read("src/global/role_program/image_impl/blob_image.rs");
    let projection = read("src/g/role_projection.rs");

    let program_from_image = program_blob
        .split("pub(crate) const fn from_image(image: &CompiledProgramImage) -> Self {")
        .nth(1)
        .and_then(|tail| tail.split("let view = image.view();").next())
        .expect("program image fail-closed constructor");
    assert!(
        program_from_image
            .contains("if projected_len > N {\n            panic!(\"program image\");\n        }")
            && !program_from_image.contains("return Self::empty(image);"),
        "ProgramImageBytes::from_image overflow must fail closed, not fall back to an empty or max-capacity image"
    );

    let role_from_scratch = role_blob
        .split("pub(crate) const fn from_scratch(scratch: RoleLaneScratch, facts: RuntimeRoleFacts) -> Self {")
        .nth(1)
        .and_then(|tail| tail.split("let mut out = Self::empty();").next())
        .expect("role image fail-closed constructor");
    assert!(
        role_from_scratch
            .contains("if projected_len > N {\n            panic!(\"role image\");\n        }")
            && !role_from_scratch.contains("return Self::empty();"),
        "RoleImageBytes::from_scratch overflow must fail closed, not fall back to an empty or max-capacity image"
    );

    assert!(
        program_blob.contains("pub(crate) const fn from_unselected_bucket_or_empty(")
            && role_blob.contains("pub(crate) const fn from_unselected_bucket_or_empty(")
            && !program_blob.contains("pub(crate) const fn from_projection_bucket(")
            && !role_blob.contains("pub(crate) const fn from_projection_bucket(")
            && projection.contains("from_unselected_bucket_or_empty")
            && !projection.contains("from_projection_bucket")
            && !projection.contains("ROLE_IMAGE_BLOB_CAPACITY")
            && !projection.contains("PROGRAM_IMAGE_BLOB_CAPACITY")
            && !projection.contains("CompiledProgramRef { image: &'static CompiledProgramImage }"),
        "projection may use private dead-bucket sentinels, but selected buckets must stay on fail-closed compact constructors without max-capacity fallback or resident CompiledProgramImage handles"
    );
}

#[test]
fn compiled_image_sources_stay_split_below_one_thousand_lines() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let image = read("src/global/compiled/images/image.rs");
    for required in [
        "mod blob_storage;",
        "mod columns;",
        "mod program_ref;",
        "mod role_descriptor_ref;",
        "mod route_controls;",
    ] {
        assert!(
            image.contains(required),
            "compiled image source must stay split by ownership boundary: {required}"
        );
    }

    let mut stack = vec![root.join("src")];
    let mut oversized = Vec::new();
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)
            .unwrap_or_else(|err| panic!("read {} failed: {err}", dir.display()))
        {
            let path = entry
                .unwrap_or_else(|err| panic!("read dir entry in {} failed: {err}", dir.display()))
                .path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
                let body = std::fs::read_to_string(&path)
                    .unwrap_or_else(|err| panic!("read {} failed: {err}", path.display()));
                let lines = body.lines().count();
                if lines > 1000 {
                    oversized.push(format!(
                        "{}:{lines}",
                        path.strip_prefix(&root).unwrap_or(path.as_path()).display()
                    ));
                }
            }
        }
    }
    assert!(
        oversized.is_empty(),
        "production source files must stay below 1000 lines after image split: {}",
        oversized.join(", ")
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
        "== final-form projected protocol matrix ==",
        "projected_protocol_matrix_reports_compact_resident_images",
        "PROTOCOL_MATRIX_OUTPUT",
        "protocol-matrix ",
        "minimal_send_recv",
        "nested_par_join",
        "route_with_unselected_nested_par",
        "triple_nested_route",
        "passive_nested_route_observer",
        "alternating_par_route",
        "huge_legal_choreography",
        "program_blob_len",
        "role_blob_len",
        "endpoint_scratch_bytes",
        "largest_section_bytes",
        "== final-form protocol artifact flash matrix ==",
        "FINAL_FORM_PROTOCOL_FIXTURE=\"${ROOT_DIR}/src/global/role_program/tests/final_form_protocol_matrix.rs\"",
        "FINAL_FORM_PROTOCOL_BLACK_BOX_FIXTURE=\"${ROOT_DIR}/src/global/role_program/tests/final_form_protocol_black_box_roles.rs\"",
        "cp \"${FINAL_FORM_PROTOCOL_FIXTURE}\"",
        "cp \"${FINAL_FORM_PROTOCOL_BLACK_BOX_FIXTURE}\"",
        "final_form_protocol!(${protocol_name})",
        "final_form_protocol_black_box_roles!(${protocol_name}, &program)",
        "protocol-artifact ",
        "flash_total",
        "rodata_map_bytes",
        "rodata_map_fragments",
        "bucket_symbol_count",
        "map_bucket_symbol_count",
        "selected_program_bucket_count",
        "selected_role_bucket_count",
        "full_bucket_floor_bytes",
        "llvm-nm",
        "-Map=${map}",
        "snapshot-check protocol-artifact",
        "protocol artifact rodata={rodata} exceeds",
        "exceeds selected bucket count",
        "is compatible with retaining every bucket ladder entry",
        "final-form measurement violation: missing protocol artifact rows",
        "protocol artifact flash_total={actual} exceeds",
        "final-form measurement violation: minimal_send_recv",
    ] {
        assert!(
            final_gate.contains(required),
            "final-form protocol matrix measurement missing required guard: {required}"
        );
    }

    for required in [
        "PUBLISHED_CRATES_IO_0_8_0_REF=\"${HIBANA_SIZE_PUBLISHED_BASE_REF:-d95e83eb503f35f8beeb60a29d41b4cf6a8d5290}\"",
        "BASE_REF=\"${PUBLISHED_CRATES_IO_0_8_0_REF}\"",
        "Default base is the crates.io 0.8.0 publish commit.",
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
        "runtime shape {shape} peak_stack_bytes exceeds published baseline",
        "runtime shape {shape} localside_peak_stack_bytes exceeds published baseline",
        "SNAPSHOT_FILE=\"${ROOT_DIR}/.github/measurement_snapshots/hibana-size-snapshot.json\"",
        "budget_snapshot = json.load(f)",
        "worktree-snapshot budget-section {key} actual={actual} budget={maximum}",
        "section {key} exceeds snapshot budget",
        "worktree-snapshot budget-runtime shape={shape} {key} actual={actual} budget={maximum}",
        "runtime shape {shape} {key} exceeds snapshot budget",
        "worktree-snapshot budget-aggregate {name} actual={new} budget={maximum}",
        "aggregate {name} exceeds published baseline",
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
        "BASE_REF=\"HEAD^\"",
    ] {
        assert!(
            !worktree_gate.contains(forbidden) && !final_gate.contains(forbidden),
            "size gate must not reintroduce current-tree self-patching or CI fixed-snapshot coupling: {forbidden}"
        );
    }

    assert!(
        workflow.contains("fetch-depth: 0")
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
        "offer_requires_framed_receive_evidence_for_branch_demux",
        "offer_decode_transport_consumes_frame_once",
        "forgotten_route_branch_leaves_endpoint_fail_closed",
        "forgotten_decode_future_leaves_endpoint_fail_closed",
        "route_inside_parallel_lane_cannot_release_join_before_sibling_lane",
        "alternating_route_parallel_join_uses_only_selected_arms",
        "unselected_route_arm_parallel_events_are_dead_and_not_join_obligations",
        "unselected_route_arm_parallel_events_do_not_block_parallel_join",
        "outer_left_selection_kills_nested_right_route_and_parallel_body",
        "lane_set_view_iterates_set_bits_without_empty_lane_scan",
    ] {
        assert!(
            performance_gate.contains(required),
            "runtime performance hygiene gate missing required operation-count/source guard: {required}"
        );
    }
}
