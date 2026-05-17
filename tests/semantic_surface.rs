#![cfg(feature = "std")]

use std::{fs, path::PathBuf};

fn read(path: &str) -> String {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let full = root.join(path);
    fs::read_to_string(&full).unwrap_or_else(|err| panic!("read {} failed: {err}", full.display()))
}

fn lines(path: &str) -> Vec<String> {
    read(path)
        .lines()
        .map(normalize_ws)
        .filter(|line| !line.is_empty())
        .collect()
}

fn normalize_ws(input: impl AsRef<str>) -> String {
    let mut normalized = String::new();
    let mut first = true;
    for part in input.as_ref().split_whitespace() {
        if !first {
            normalized.push(' ');
        }
        first = false;
        normalized.push_str(part);
    }
    normalized
}

#[test]
fn stable_public_surface_allowlists_are_final_form() {
    assert_eq!(
        lines(".github/allowlists/lib-public-api.txt"),
        [
            "pub mod g;",
            "pub mod integration;",
            "pub use endpoint::{Endpoint, EndpointError, EndpointResult, RouteBranch};",
        ],
        "crate root public surface must stay on g + integration + endpoint core"
    );

    assert_eq!(
        lines(".github/allowlists/g-public-api.txt"),
        [
            "pub use Program;",
            "pub use Msg;",
            "pub use Role;",
            "pub use par;",
            "pub use route;",
            "pub use send;",
            "pub use seq;",
        ],
        "hibana::g must stay DSL-only"
    );

    let endpoint = lines(".github/allowlists/endpoint-public-api.txt");
    for required in [
        "pub struct Endpoint<'r, const ROLE: u8> {",
        "pub struct RouteBranch<'e, 'r, const ROLE: u8> {",
        "pub fn flow<'e, M>( &'e mut self, ) -> EndpointResult<flow::Flow<'e, 'r, ROLE, M>> where M: crate::global::MessageSpec + crate::global::SendableLabel, {",
        "pub fn recv<'e, M>(&'e mut self) -> impl core::future::Future<Output = EndpointResult<<<M as crate::global::MessageSpec>::Payload as crate::transport::wire::WirePayload>::Decoded<'e>>> + 'e where M: crate::global::MessageSpec + 'e, M::Payload: crate::transport::wire::WirePayload, {",
        "pub fn offer<'e>( &'e mut self, ) -> impl core::future::Future<Output = EndpointResult<RouteBranch<'e, 'r, ROLE>>> + 'e {",
        "pub fn label(&self) -> u8 {",
        "pub fn decode<M>(self) -> impl core::future::Future<Output = EndpointResult<<<M as crate::global::MessageSpec>::Payload as crate::transport::wire::WirePayload>::Decoded<'e>>> where M: crate::global::MessageSpec, M::Payload: crate::transport::wire::WirePayload, {",
        "pub struct EndpointError {",
        "pub type EndpointResult<T> = core::result::Result<T, EndpointError>;",
    ] {
        assert!(
            endpoint.iter().any(|line| line == required),
            "endpoint allowlist missing final-form item: {required}"
        );
    }
    assert_eq!(
        endpoint.len(),
        9,
        "endpoint public surface must not grow without an explicit final-form review"
    );

    let integration = lines(".github/allowlists/integration-public-api.txt").join("\n");
    for required in [
        "pub mod program {",
        "pub use crate::global::role_program::{RoleProgram, project};",
        "pub use crate::global::{MessageSpec, StaticControlDesc};",
        "pub mod ids {",
        "pub use crate::control::types::{Lane, RendezvousId, SessionId};",
        "pub mod binding {",
        "pub use crate::binding::{BindingSlot, NoBinding};",
        "pub mod policy {",
        "pub use super::cluster::core::{ LoopResolution, ResolverContext, ResolverError, ResolverRef, RouteResolution, };",
        "pub mod wire {",
        "pub use crate::transport::wire::{CodecError, Payload, WireEncode, WirePayload};",
        "pub mod transport {",
        "pub use crate::transport::{ Outgoing, TransportError, };",
    ] {
        assert!(
            integration.contains(required),
            "integration allowlist missing final-form item: {required}"
        );
    }
}

#[test]
fn public_surface_allowlists_keep_forbidden_names_out() {
    let joined = [
        read(".github/allowlists/lib-public-api.txt"),
        read(".github/allowlists/g-public-api.txt"),
        read(".github/allowlists/endpoint-public-api.txt"),
        read(".github/allowlists/integration-public-api.txt"),
    ]
    .join("\n");

    for forbidden in [
        "g::advanced",
        "FlowSendArg",
        "SendOutcomeKind",
        "CapFlow",
        "FlowInner",
        "DynamicResolution",
        "from_fn",
        "from_state",
        "fallback",
        "legacy",
        "compat",
        "heuristic",
        "rescue",
        "state machine",
        "TransportSnapshotParts",
        "ConfigParts",
        "RegisteredTokenParts",
        "HibanaError",
        "EndpointErrorKind",
        "ResolverErrorKind",
        "AttachErrorKind",
        "recv_timeout",
        "send_timeout",
        "offer_timeout",
        "decode_timeout",
        "try_recover",
        "ignore_fault",
        "reconnect",
    ] {
        assert!(
            !joined.contains(forbidden),
            "public allowlists must not retain forbidden final-form name: {forbidden}"
        );
    }
}

#[test]
fn failure_deadline_cancellation_surface_has_only_domain_evidence() {
    let lib = read("src/lib.rs");
    let endpoint = read("src/endpoint.rs");
    let resolver = read("src/control/cluster/core.rs");
    let attach = read("src/control/cluster/error.rs");
    let integration = read("src/integration.rs");
    let runtime_config = read("src/runtime/config.rs");
    let transport = read("src/transport.rs");
    let rendezvous_assoc = read("src/rendezvous/association.rs");
    let offer_frontier = read("src/endpoint/kernel/route_frontier/offer.rs");
    let frontier_runtime = read("src/endpoint/kernel/runtime/frontier.rs");
    let public_allowlists = [
        read(".github/allowlists/lib-public-api.txt"),
        read(".github/allowlists/g-public-api.txt"),
        read(".github/allowlists/endpoint-public-api.txt"),
        read(".github/allowlists/integration-public-api.txt"),
    ]
    .join("\n");

    for required in [
        "pub type EndpointResult<T> = core::result::Result<T, EndpointError>;",
        "pub use endpoint::{Endpoint, EndpointError, EndpointResult, RouteBranch};",
        "pub use super::cluster::core::{ LoopResolution, ResolverContext, ResolverError, ResolverRef, RouteResolution, };",
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
                "{path} must not expose failure/deadline/cancellation escape hatch: {forbidden}"
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
        runtime_config.contains("struct OperationalDeadline")
            && transport.contains("fn operational_deadline_ticks(&self) -> Option<u32>")
            && !runtime_config.contains("operational_deadline_ticks")
            && !runtime_config.contains("with_operational_deadline_ticks")
            && endpoint.contains("SessionFault(crate::rendezvous::SessionFaultKind)")
            && rendezvous_assoc.contains("pub(super) fn poison_session"),
        "operational wait fuses must be substrate-owned and poison a session generation without adding public timeout APIs"
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
            && read("src/endpoint/kernel/core.rs").contains("SessionFaultKind::EndpointDropped"),
        "session poison must wake registered waiters and live endpoint drop must become terminal evidence"
    );
}

#[test]
fn resident_descriptor_attach_has_no_lowering_materialization_path() {
    let compiled_mod = read("src/global/compiled/mod.rs");
    let lowering_mod = read("src/global/compiled/lowering/mod.rs");
    let rendezvous = read("src/rendezvous/core.rs");
    let cluster = read("src/control/cluster/core.rs");
    let endpoint_core = read("src/endpoint/kernel/core.rs");
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

    let role_image = read("src/global/compiled/images/image.rs");
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
    assert!(
        endpoint_core.contains("enum SendPayloadPlan")
            && endpoint_core.contains("fn prepare_send_payload_plan")
            && endpoint_core.contains("fn stage_send_payload"),
        "send control staging must stay explicit and compact after resident descriptor attach"
    );
}

#[test]
fn transport_contract_documents_lane_and_hint_drain() {
    let readme = read("README.md");
    let transport = read("src/transport.rs");
    let offer_frontier = read("src/endpoint/kernel/route_frontier/offer.rs");
    let scope_evidence = read("src/endpoint/kernel/route_frontier/scope_evidence_logic.rs");
    let test_transport = read("tests/common/mod.rs");
    let route_tests = read("tests/route_dynamic_control.rs");

    for (path, source) in [
        ("README.md", readme.as_str()),
        ("src/transport.rs", transport.as_str()),
    ] {
        assert!(
            source.contains("open(local_role, session_id, lane)") || source.contains("lane: u8"),
            "{path} must document Transport::open lane preservation"
        );
        assert!(
            source.contains("hint-drain"),
            "{path} must document recv_frame_hint as a route-observation drain"
        );
        assert!(
            source.contains("must not consume payload bytes")
                || source.contains("must not yield the same observation again"),
            "{path} must separate route-observation draining from payload receive"
        );
    }

    assert!(
        !readme.contains("open(local_role, session_id)`"),
        "README must not keep the old two-argument Transport::open contract"
    );
    assert!(
        transport.contains("fn open<'a>(")
            && transport.contains("local_role: u8")
            && transport.contains("session_id: u32")
            && transport.contains("lane: u8"),
        "Transport trait must require lane at attach/open time"
    );
    for required in [
        "lane: u8",
        "hint_drained: bool",
        "const TEST_LANE_CAPACITY: usize = 256",
        "self.waiters[lane as usize] = Some(waker)",
        "current_hint_drained",
        "outgoing.lane()",
        "state.dequeue(rx.role, rx.lane)",
        "state.add_waiter(rx.role, rx.lane",
        "front_matching_mut(|frame| frame.lane == rx.lane)",
        "frame.hint_drained = true",
        "frame.hint_drained = false",
    ] {
        assert!(
            test_transport.contains(required),
            "shared test transport must enforce production lane/hint-drain contract: missing {required}"
        );
    }
    assert!(
        !test_transport.contains("_lane: u8"),
        "shared test transport must not ignore the opened logical lane"
    );
    let pending_hint_section = scope_evidence
        .split_once("pub(in crate::endpoint::kernel) fn pending_scope_frame_hint_on_lane")
        .and_then(|(_, tail)| tail.split_once("#[inline]").map(|(section, _)| section))
        .expect("scope evidence logic must define pending_scope_frame_hint_on_lane");
    assert!(
        offer_frontier.contains("pending_scope_frame_hint_on_lane(\n                lane_idx")
            && pending_hint_section.contains("Lane::new(lane_idx as u32)")
            && !pending_hint_section.contains("offer_lane_idx"),
        "route hint observation must be drained from the same logical lane being inspected, not from a summary lane"
    );
    assert!(
        scope_evidence.contains("pub(in crate::endpoint::kernel) fn passive_dispatch_arm_from_exact_frame_label")
            && offer_frontier.contains(".passive_dispatch_arm_from_exact_frame_label(\n                                scope_id,\n                                route_evidence_lane")
            && offer_frontier.contains(".passive_dispatch_arm_from_exact_frame_label(scope_id, lane, frame_label)"),
        "passive route evidence must resolve dynamic branches from lane+frame evidence before falling back to label-only metadata"
    );
    assert!(
        offer_frontier.contains("let has_ack =")
            && offer_frontier.contains("let has_frame_hint =")
            && offer_frontier.contains("if has_ack || has_frame_hint")
            && offer_frontier.contains("passive_evidence_can_skip_recv")
            && scope_evidence.contains("peek_scope_frame_hint_with_lane")
            && scope_evidence.contains("record_scope_frame_hint(\n        &mut self,\n        scope_id: ScopeId,\n        lane: u8"),
        "route evidence collection must observe ack and frame hints independently, preserve the hint lane, and avoid parking on the wrong representative lane"
    );
    assert!(
        route_tests.contains("test_transport_demuxes_lane_and_drains_route_hint"),
        "route tests must include a functional lane demux + hint-drain regression"
    );
}

#[test]
fn resolver_reject_error_captures_public_callsite() {
    let reject_line = line!() + 1;
    let error = hibana::integration::policy::ResolverError::reject();

    assert_eq!(error.operation(), "reject");
    assert!(error.file().ends_with("tests/semantic_surface.rs"));
    assert_eq!(error.line(), reject_line);
}

#[test]
fn topology_validation_has_no_test_only_semantic_owner() {
    let topology = read("src/control/automaton/topology.rs");
    let distributed = read("src/control/automaton/distributed.rs");
    let rendezvous_topology = read("src/rendezvous/topology.rs");
    let rendezvous_core = read("src/rendezvous/core.rs");

    for forbidden in [
        "TopologyCommitAutomaton",
        "pub(crate) fn process_intent",
        "DistributedTopology::process_intent",
        "pub(super) fn topology_commit",
        ".topology.topology_commit(",
    ] {
        assert!(
            !topology.contains(forbidden)
                && !distributed.contains(forbidden)
                && !rendezvous_topology.contains(forbidden)
                && !rendezvous_core.contains(forbidden),
            "topology validation must use production cluster/rendezvous paths, not test-only owner: {forbidden}"
        );
    }

    let perform_effect = rendezvous_core
        .split_once("fn perform_effect(")
        .and_then(|(_, tail)| {
            tail.split_once("fn eval_effect(")
                .map(|(section, _)| section)
        })
        .expect("rendezvous core must keep perform_effect before eval_effect");

    for forbidden in [
        "ControlOp::TopologyBegin",
        "ControlOp::TopologyAck",
        "ControlOp::TopologyCommit",
    ] {
        assert!(
            !perform_effect.contains(forbidden),
            "topology operations must stay out of direct Rendezvous::perform_effect: {forbidden}"
        );
    }
}

#[test]
fn stable_public_api_gate_has_no_nightly_or_rustdoc_json_owner() {
    let script = read(".github/scripts/check_hibana_public_api.sh");
    let final_gate = read(".github/scripts/run_final_form_gates.sh");
    let workflow = read(".github/workflows/quality-gates.yml");
    let combined = format!("{script}\n{final_gate}\n{workflow}");

    for required in [
        "export TOOLCHAIN=\"${TOOLCHAIN:-1.95.0}\"",
        "bash ./.github/scripts/run_final_form_gates.sh",
        "bash ./.github/scripts/check_hibana_public_api.sh",
        "stable public API check passed",
    ] {
        assert!(
            combined.contains(required),
            "Rust 1.95 public API gate missing required owner: {required}"
        );
    }

    for forbidden in [
        "dtolnay/rust-toolchain@nightly",
        "rustup which cargo --toolchain nightly",
        "rustup which rustc --toolchain nightly",
        "rustup which rustdoc --toolchain nightly",
        "target/doc/hibana.json",
        "HIBANA_RUSTDOC_JSON",
        "-Z unstable-options",
        "--output-format json",
    ] {
        assert!(
            !combined.contains(forbidden),
            "stable public API gate must not depend on nightly rustdoc JSON: {forbidden}"
        );
    }
}
