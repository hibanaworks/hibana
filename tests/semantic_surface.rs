#![cfg(feature = "std")]

use std::{fs, path::PathBuf};

fn read(path: &str) -> String {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let full = root.join(path);
    fs::read_to_string(&full).unwrap_or_else(|err| panic!("read {} failed: {err}", full.display()))
}

fn read_offer_tests() -> String {
    let mut source = read("src/endpoint/kernel/test_support/core_offer_tests.rs");
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src/endpoint/kernel/test_support/core_offer_tests");
    let mut parts = fs::read_dir(&root)
        .unwrap_or_else(|err| panic!("read {} failed: {err}", root.display()))
        .map(|entry| {
            entry
                .unwrap_or_else(|err| panic!("read dir entry in {} failed: {err}", root.display()))
                .path()
        })
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.ends_with(".rs") && name != "mod.rs")
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();
    parts.sort();
    for part in parts {
        source.push_str(
            &fs::read_to_string(&part)
                .unwrap_or_else(|err| panic!("read {} failed: {err}", part.display())),
        );
    }
    source
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

fn control_op_variants() -> Vec<String> {
    let mint = read("src/control/cap/mint.rs");
    let body = mint
        .split_once("pub enum ControlOp {")
        .and_then(|(_, tail)| tail.split_once("\n}").map(|(body, _)| body))
        .expect("ControlOp enum must stay in mint.rs");

    body.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with("///") || line.starts_with("#[") {
                return None;
            }
            let variant = line
                .split_once('=')
                .map_or(line, |(name, _)| name)
                .trim()
                .trim_end_matches(',');
            variant
                .chars()
                .next()
                .filter(|ch| ch.is_ascii_uppercase())
                .map(|_| variant.to_string())
        })
        .collect()
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
        "pub struct SessionKitStorage<'cfg, T, U, C, const MAX_RV: usize = 4> where T: crate::transport::Transport + 'cfg, U: crate::runtime::consts::LabelUniverse + 'cfg, C: crate::runtime::config::Clock + 'cfg, {",
        "pub struct ResidentSessionKit<'kit, 'cfg, T, U, C, const MAX_RV: usize = 4> where T: crate::transport::Transport + 'cfg, U: crate::runtime::consts::LabelUniverse + 'cfg, C: crate::runtime::config::Clock + 'cfg, {",
        "pub const fn uninit() -> Self {",
        "pub fn init(&mut self) -> ResidentSessionKit<'_, 'cfg, T, U, C, MAX_RV> {",
        "pub unsafe fn init_in_place( storage: &'cfg mut core::mem::MaybeUninit<Self>, ) -> &'cfg Self {",
        "pub mod program {",
        "pub use crate::global::role_program::{RoleProgram, project};",
        "pub use crate::global::MessageSpec;",
        "pub use crate::global::program::{ Projectable, ProjectionAtomSpec, ProjectionMetadataVisitor, ProjectionPolicySpec, ProjectionProgramFacts, ProjectionScopeSpec, };",
        "pub use crate::global::program::{ ProjectionMessageSpec, ProjectionTypeFingerprint };",
        "pub mod ids {",
        "pub use crate::control::types::{Lane, RendezvousId, SessionId};",
        "pub mod binding {",
        "pub use crate::binding::{BindingSlot, NoBinding};",
        "pub mod policy {",
        "pub use super::cluster::core::{ LoopResolution, ResolverContext, ResolverError, ResolverRef, RouteResolution, };",
        "pub mod wire {",
        "pub use crate::transport::wire::{CodecError, Payload, WireEncode, WirePayload};",
        "pub mod transport {",
        "pub use crate::transport::{ FrameLabel, Outgoing, PortOpen, Transport, TransportEvent, TransportEventKind, TransportEventMeta, TransportMetrics, TransportError, };",
    ] {
        assert!(
            integration.contains(required),
            "integration allowlist missing final-form item: {required}"
        );
    }
}

#[test]
fn readme_documents_the_public_control_op_catalogue() {
    let readme = read("README.md");

    for variant in control_op_variants() {
        let needle = format!("`ControlOp::{variant}`");
        assert!(
            readme.contains(&needle),
            "README control-message section must document public control op: {needle}"
        );
    }

    for public_kind in ["RouteDecisionKind", "LoopContinueKind", "LoopBreakKind"] {
        assert!(
            readme.contains(public_kind),
            "README must identify the built-in public control kind: {public_kind}"
        );
    }

    for required in [
        "`GenericCapToken<K>` plus `ControlResourceKind`",
        "`integration::cap::control::ControlOp`",
        "`ControlPath::Local`",
        "`ControlPath::Wire`",
        "projected descriptor",
    ] {
        assert!(
            readme.contains(required),
            "README control-message section missing mechanism text: {required}"
        );
    }
}

#[test]
fn capability_tokens_are_documented_as_nonce_ledger_not_mac_authority() {
    let mint = read("src/control/cap/mint.rs");
    let rendezvous = read("src/rendezvous/core.rs");
    let capability = read("src/rendezvous/capability.rs");
    let rendezvous_error = read("src/rendezvous/error.rs");
    let global = read("src/global.rs");

    for required in [
        "[16B nonce | 40B descriptor header]",
        "trusted-domain nonce ledger",
        "Claim authority comes from a nonce table entry",
        "Token bytes stop at the descriptor header;",
        "not a keyed verifier",
        "Endpoint-local control progression is witnessed by rendezvous-scoped brands",
        "Implements the rendezvous-local capability nonce ledger.",
        "Control resource kinds must not use `0`.",
        "Control resource kinds choose this through [`ControlResourceKind::SHOT`].",
        "Capability table reached its configured capacity.",
        "Decoding must be deterministic, side-effect-free, and non-authoritative.",
        "fn typed_header(&self) -> Result<CapHeader, CapError>",
        "if header.tag() != K::TAG",
    ] {
        assert!(
            mint.contains(required) || capability.contains(required),
            "capability token docs must teach the trusted-domain nonce-ledger authority: {required}"
        );
    }

    let typed_header_pos = mint.find("fn typed_header(&self)").expect("typed_header");
    let decode_handle_pos = mint
        .find("pub fn decode_handle(&self)")
        .expect("decode_handle");
    let as_view_pos = mint.find("pub fn as_view(&self)").expect("as_view");
    assert!(
        typed_header_pos < decode_handle_pos
            && typed_header_pos < as_view_pos
            && mint[decode_handle_pos..as_view_pos].contains("self.typed_header()?")
            && mint[as_view_pos..].contains("let header = self.typed_header()?"),
        "GenericCapToken typed handle decode and typed view must share the same resource-tag preflight"
    );
    assert!(
        mint.contains("pub fn scope(&self) -> Result<Option<ScopeId>, CapError>")
            && mint.contains("self.as_view().map(|view| view.scope())")
            && !mint.contains("self.as_view().ok().and_then(|view| view.scope())"),
        "GenericCapToken::scope must preserve malformed-token errors instead of collapsing them into None"
    );
    assert!(
        !mint.contains("mint_token"),
        "capability docs must not mention stale mint_token convenience APIs"
    );
    assert!(
        rendezvous.contains(
            "Claim authority is the rendezvous-local nonce ledger plus descriptor validation."
        ),
        "claim path must pin nonce-ledger descriptor authority at the implementation site"
    );
    assert!(
        capability.contains("expected_handle: &[u8; CAP_HANDLE_LEN]")
            && capability.contains("if &entry.handle != expected_handle"),
        "claim_by_nonce must validate token handle bytes against the ledger entry"
    );
    assert!(
        capability.contains(") -> Result<bool, CapError>")
            && !capability.contains("Result<(bool, [u8; CAP_HANDLE_LEN]), CapError>"),
        "claim_by_nonce must report claim state only; typed handle decode belongs to claim_cap preflight"
    );
    let claim_cap_pos = rendezvous
        .find("pub(crate) fn claim_cap")
        .expect("claim_cap");
    let decode_preflight_pos = rendezvous[claim_cap_pos..]
        .find("let mut handle = token.decode_handle()")
        .expect("typed handle decode preflight")
        + claim_cap_pos;
    let ledger_claim_pos = rendezvous[claim_cap_pos..]
        .find(".claim_by_nonce(")
        .expect("ledger claim")
        + claim_cap_pos;
    let claim_event_pos = rendezvous[claim_cap_pos..]
        .find("let claim_id = crate::observe::cap_claim::<K>();")
        .expect("claim event setup")
        + claim_cap_pos;
    assert!(
        decode_preflight_pos < ledger_claim_pos && ledger_claim_pos < claim_event_pos,
        "claim_cap must decode typed handles before one-shot ledger mutation and publish events only after claim"
    );
    assert!(
        !mint.contains("VerifiedCap")
            && !mint.contains("struct Witness")
            && !mint.contains("from_claimed_handle_unchecked")
            && !rendezvous.contains("VerifiedCap")
            && rendezvous.contains("K::zeroize(&mut handle);\n        Ok(())"),
        "claim_cap should publish the ledger claim directly instead of carrying an unused proof object"
    );
    assert!(
        rendezvous
            .contains("claim_cap_rejects_malformed_handle_without_consuming_one_shot_authority"),
        "claim_cap must regress malformed typed handle decode before one-shot consumption"
    );
    assert!(
        mint.contains("resource-owned handle")
            && rendezvous_error.contains("resource-owned handle")
            && !mint.contains("field validation failures (kind/shot/sid/lane)")
            && !mint.contains("Token field mismatch (kind/shot/sid/lane)")
            && !rendezvous_error.contains("field mismatch (kind/shot/sid/lane)"),
        "CapError::Mismatch docs must include handle-byte validation and avoid stale field-only wording"
    );
    assert!(
        global.contains("spec.resource_tag() == 0")
            && global.contains("control resource tag 0 is reserved"),
        "control resource tag zero must be rejected by the const descriptor gate"
    );
    assert!(
        read("tests/ui/g-control-resource-zero-tag.rs").contains("const TAG: u8 = 0;")
            && read("tests/ui/g-control-resource-zero-tag.stderr")
                .contains("control resource tag 0 is reserved"),
        "control resource tag zero must have UI coverage for the const descriptor gate"
    );

    for forbidden in [
        "authentication tag",
        "keyed_hash",
        "InvalidMac",
        "InvalidProof",
        "proof bytes",
        "MAC validation",
        "MAC tag",
        "No authentication tag",
        "secure claim path",
        "cryptographically validated",
        "CAP_TAG_LEN",
        "CAP_PROOF_LEN",
        "CAP_STRATEGY_LEN",
        "derive_tag",
        "derive_proof",
        "derive_strategy",
        "strategy bytes",
        "strategy-owned",
        "ledger-free",
        "Original Capability Token System",
        "`ResourceKind::SHOT`",
        "MultiSafe",
        "Capability table is full (64 entries)",
        "External control/resource kinds must not use `0`",
        "nonce-authenticated",
        "ensure_authenticated_session_lane",
        "authenticated lane",
    ] {
        assert!(
            !mint.contains(forbidden)
                && !rendezvous.contains(forbidden)
                && !capability.contains(forbidden),
            "capability implementation must not imply a cryptographic MAC claim path: {forbidden}"
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
    let endpoint_core = read("src/endpoint/kernel/core.rs");
    let offer_frontier = read("src/endpoint/kernel/route_frontier/offer.rs");
    let offer_state = read("src/endpoint/kernel/route_frontier/offer/state.rs");
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
        endpoint_core.contains("fn terminal_clear_public_offer_state")
            && endpoint_core.contains("fn terminal_clear_public_recv_state")
            && endpoint_core.contains("fn terminal_clear_public_decode_state")
            && endpoint_core.contains("if let Some(kind) = self.session_fault() {\n            self.terminal_clear_public_offer_state();")
            && endpoint_core.contains("if let Some(kind) = self.session_fault() {\n            self.terminal_clear_public_recv_state();")
            && endpoint_core.contains("if let Some(kind) = self.session_fault() {\n            self.terminal_clear_public_decode_state();")
            && endpoint_core.contains(
                "if let Some(kind) = self.session_fault() {\n            self.reset_public_send_state();"
            ),
        "session fault fast paths must terminal-clear public operation state instead of relying on future Drop cleanup after Ready(Err(_))"
    );
    assert!(
        endpoint_core.contains("fn restore_detached_offer_state")
            && endpoint_core.contains("state.take_rollback_items()")
            && endpoint_core.contains("lane_port::requeue_recv_frame(port, payload);")
            && endpoint_core.contains("self.put_back_binding_for_lane(lane_idx, evidence);")
            && offer_state.contains("pub(in crate::endpoint::kernel) fn take_rollback_items")
            && offer_state.contains("OfferRunStage::CollectEvidence(stage)")
            && offer_state.contains("OfferRunStage::ResolveToken(stage)")
            && read_offer_tests()
                .contains("reset_public_offer_state_restores_carried_binding_evidence")
            && read_offer_tests()
                .contains("reset_public_offer_state_restores_carried_transport_payloads")
            && read_offer_tests().contains("terminal_offer_clear_discards_carried_preview_state"),
        "non-terminal offer reset must restore carried ingress, while terminal offer clear must discard it"
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
fn projection_metadata_and_lane_domain_stay_embedded_exact() {
    let program = read("src/global/program.rs");
    let role_image = read("src/global/compiled/images/image.rs");

    assert!(
        program.contains("#[cfg(any(feature = \"std\", test))]\n#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]\npub struct ProjectionTypeFingerprint")
            && program
                .contains("#[cfg(any(feature = \"std\", test))]\nimpl ProjectionTypeFingerprint")
            && program.contains("pub fn of<T: ?Sized>()")
            && program.contains("Self::from_type_name(core::any::type_name::<T>())")
            && !program
                .contains("#[cfg(not(any(feature = \"std\", test)))]\n    pub fn of<T: ?Sized>()")
            && !program.contains("Self::embedded()")
            && read("src/integration.rs").contains(
                "#[cfg(any(feature = \"std\", test))]\n    pub use crate::global::program::{ProjectionMessageSpec, ProjectionTypeFingerprint};",
            ),
        "ProjectionTypeFingerprint and typed message metadata must be host/test-only; embedded metadata authority is numeric facts"
    );
    let embedded_projectable = program
        .split("#[cfg(not(any(feature = \"std\", test)))]\nimpl<Universe, Steps> Projectable<Universe> for Program<Steps>")
        .nth(1)
        .and_then(|tail| tail.split("pub const fn seq").next())
        .expect("embedded Projectable impl");
    assert!(
        embedded_projectable.contains("Steps: BuildProgramSource")
            && !embedded_projectable.contains("VisitProjectionMessages")
            && !embedded_projectable.contains("visit_projection_messages"),
        "embedded projection metadata must be descriptor/numeric-only and avoid typed message metadata traversal"
    );
    assert!(
        !program.contains("pub const fn embedded"),
        "embedded projection fingerprint fallback is an internal representation detail, not public API"
    );

    let role_program = read("src/global/role_program.rs");
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
            && !read("src/global/compiled/lowering/driver.rs")
                .contains("fill_role_atom_lanes_in_range")
            && !read("src/endpoint/kernel/route_frontier/offer.rs")
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
            && !read("src/endpoint/kernel/runtime/route_state.rs")
                .contains("route_scope_lane_words")
            && !read("src/endpoint/kernel/endpoint_init.rs")
                .contains("set_route_scope_arm_lane_set")
            && read("src/endpoint/kernel/core.rs")
                .contains(".route_scope_offer_lane_set(scope_id)")
            && read("src/endpoint/kernel/core.rs")
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
    let lowering = read("src/global/compiled/lowering/driver.rs");
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
            && lowering.contains("ProgramSourceLookup::new")
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
        "if [[ \"${HIBANA_SKIP_FIXED_SNAPSHOT_CHECK:-0}\" != \"1\" && \"${CI:-false}\" != \"true\" ]]; then",
        "fixed snapshot thumb budget check skipped in CI/override; worktree regression gate still runs",
        "fixed snapshot runtime budget check skipped in CI/override; worktree regression gate still runs",
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
        "measure_tree \"base-${BASE_LABEL}\" \"${BASE_WORKTREE}\" \"${BASE_JSON}\" 1",
        "measure_tree \"current-${CURRENT_LABEL}\" \"${CURRENT_TREE}\" \"${CURRENT_JSON}\" 0",
        "current tree is missing committed localside_peak_stack_bytes measurement; refusing to patch current source for the regression gate",
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
        "HIBANA_SKIP_FIXED_SNAPSHOT_CHECK=0",
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
                .contains("bash ./.github/scripts/check_runtime_performance_hygiene.sh")
            && final_gate.contains("HIBANA_SKIP_FIXED_SNAPSHOT_CHECK=1")
            && final_gate
                .contains("if [[ \"${HIBANA_SKIP_WORKTREE_SIZE_REGRESSION:-0}\" != \"1\" ]]; then"),
        "CI must run the worktree regression gate while keeping fixed host snapshots local-only"
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

#[test]
fn endpoint_kernel_stays_monomorphic_behind_raw_ops() {
    let endpoint = read("src/endpoint.rs");
    let flow = read("src/endpoint/flow.rs");
    let kernel = read("src/endpoint/kernel/core.rs");

    assert!(
        endpoint.contains("struct RawRecvFuture<'e, 'r, const ROLE: u8>")
            && endpoint.contains("struct RawDecodeFuture<'e, 'r, const ROLE: u8>")
            && endpoint.contains("raw: RawRecvFuture<'e, 'r, ROLE>")
            && endpoint.contains("raw: RawDecodeFuture<'e, 'r, ROLE>")
            && flow.contains("struct RawSendFuture<'e, 'r, const ROLE: u8>")
            && flow.contains("pub(crate) struct SendFuture<'e, 'r, const ROLE: u8>")
            && flow.contains("raw: RawSendFuture<'e, 'r, ROLE>")
            && kernel.contains(
                "pub(crate) fn kernel_recv<'r>(\n    endpoint: &mut dyn RecvKernelEndpoint<'r>"
            )
            && kernel.contains(
                "pub(crate) fn kernel_decode<'r>(\n    endpoint: &mut dyn DecodeKernelEndpoint<'r>"
            )
            && kernel.contains(
                "pub(crate) fn kernel_send<'r>(\n    endpoint: &mut dyn SendKernelEndpoint<'r>"
            ),
        "typed Endpoint API must lower to raw monomorphic send/recv/decode kernels; payload codec adapters may remain generic"
    );
}

#[test]
fn completed_raw_futures_fail_fast_on_repoll() {
    let endpoint = read("src/endpoint.rs");
    let flow = read("src/endpoint/flow.rs");
    let cursor = read("tests/cursor_send_recv.rs");
    let no_policy = read("tests/no_policy_route_transport_hint.rs");

    for required in [
        "panic!(\"completed recv future polled after Ready\");",
        "panic!(\"completed offer future polled after Ready\");",
        "panic!(\"completed decode future polled after Ready\");",
    ] {
        assert!(
            endpoint.contains(required),
            "endpoint raw futures must fail fast instead of advancing after Ready: {required}"
        );
    }
    assert!(
        flow.contains("panic!(\"completed send future polled after Ready\");"),
        "send raw future must fail fast instead of advancing after Ready"
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
    let endpoint = read("src/endpoint.rs");
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
fn decode_failure_completion_is_terminal_without_branch_restore() {
    let endpoint = read("src/endpoint.rs");
    let kernel = read("src/endpoint/kernel/core.rs");
    let decode = read("src/endpoint/kernel/decode.rs");

    let raw_decode = endpoint
        .split("impl<'e, 'r, const ROLE: u8> RawDecodeFuture")
        .nth(1)
        .and_then(|tail| {
            tail.split("impl<'e, 'r, const ROLE: u8> RawRecvFuture")
                .next()
        })
        .expect("RawDecodeFuture impl must precede RawRecvFuture impl");
    assert!(
        raw_decode.contains("Poll::Ready(Err(err)) => {\n                self.completed = true;"),
        "RawDecodeFuture must mark error completion before Drop can run"
    );
    assert!(
        raw_decode.contains("let branch = core::mem::ManuallyDrop::new(branch);")
            && !raw_decode.contains("core::hint::black_box"),
        "RawDecodeFuture must suppress RouteBranch drop directly, not through black_box"
    );
    assert!(
        endpoint.contains("unsafe fn begin_public_decode_state(&mut self) {")
            && !endpoint
                .contains("unsafe fn begin_public_decode_state(&mut self) -> RecvResult<()>"),
        "begin_public_decode_state must not expose a dead Result"
    );
    assert!(
        decode.contains("self.publish_branch_preview_commit_plan(plan.branch);")
            && !decode.contains("core::hint::black_box"),
        "decode commit publishing must not consume semantic values through black_box"
    );

    let branch_missing = kernel
        .split("let Some(branch) = decode_state.branch() else {")
        .nth(1)
        .and_then(|tail| {
            tail.split("let descriptor = DecodeRuntimeDesc::new(")
                .next()
        })
        .expect("public decode must materialize a route branch before descriptor decode");
    assert!(
        branch_missing.contains("self.terminal_clear_public_decode_state();")
            && branch_missing.contains("let _ = self.poison_for_recv_error(&err);"),
        "missing public decode branch must be terminal like every other decode failure"
    );
}

#[test]
fn offer_transport_payload_presence_is_not_length_sentinel() {
    let offer = read("src/endpoint/kernel/route_frontier/offer.rs");
    let offer_ingress = read("src/endpoint/kernel/route_frontier/offer/ingress.rs");
    let offer_materialization = read("src/endpoint/kernel/route_frontier/offer/materialization.rs");
    let offer_state = read("src/endpoint/kernel/route_frontier/offer/state.rs");
    let core = read("src/endpoint/kernel/core.rs");
    let lane_port = read("src/endpoint/kernel/runtime/lane_port.rs");
    let port = read("src/rendezvous/port.rs");
    let offer_tests = read_offer_tests();

    assert!(
        !offer.contains("transport_payload_len")
            && !offer_ingress.contains("transport_payload_len")
            && !offer_materialization.contains("transport_payload_len")
            && !offer.contains("transport_payload_lane")
            && !offer_ingress.contains("transport_payload_lane")
            && !offer_materialization.contains("transport_payload_lane")
            && !offer.contains("binding_evidence: [Option<LaneIngressEvidence>; 2]")
            && !offer_state.contains("binding_evidence: [Option<LaneIngressEvidence>; 2]")
            && !offer.contains("transport_payload: [Option<lane_port::ReceivedFrame<'r>>; 2]")
            && !offer_state
                .contains("transport_payload: [Option<lane_port::ReceivedFrame<'r>>; 2]"),
        "offer preview staging must not represent payload presence with byte-length sentinels or anonymous rollback mini-vecs"
    );
    assert!(
        !port.contains("#[derive(Clone, Copy)]\npub(crate) struct ReceivedFrame"),
        "staged offer transport payload ownership must not be Copy"
    );
    assert!(
        port.contains("pub(crate) struct ReceivedFrame<'r>")
            && lane_port.contains("pub(crate) use crate::rendezvous::port::ReceivedFrame;")
            && port.contains(
                "received transport frames must be committed, explicitly requeued, or explicitly discarded"
            )
            && port.contains("Option<PortRecvFrameReceipt>")
            && port.contains("fn consume_receipt(&mut self)")
            && port.contains("fn discard_uncommitted(mut self)")
            && !port.contains("fn discard_terminal(mut self)")
            && !port.contains("fn discard_nonsemantic(mut self)")
            && port.contains("impl Drop for ReceivedFrame")
            && port
                .contains("received transport frame dropped without explicit commit, requeue, or discard")
            && port.contains(
                "transport receive frame polled while previous frame receipt is unresolved"
            )
            && port.contains("transport receive frame receipt is no longer current")
            && port.contains("received transport frame requeued on a different lane")
            && port.contains("received transport frame requeued on a different endpoint port")
            && port.contains("different Rx handle")
            && port.contains("struct PortRecvFrameReceipt")
            && !port.contains("pub(crate) struct PortRecvFrameReceipt")
            && !port.contains("#[derive(Clone, Copy)]\npub(crate) struct PortRecvFrameReceipt")
            && !lane_port.contains("port_key: u32")
            && !lane_port.contains("port_identity")
            && !lane_port.contains(".addr()")
            && !port.contains("fn drop(&mut self) {\n        self.consume_receipt();")
            && offer_tests
                .contains("produce_non_wire_recv_evidence_requeues_staged_transport_payload"),
        "offer rollback must carry a consumed frame authority, fail fast on unresolved drops, and must not carry lossy exposed Rx identities"
    );
    assert!(
        !lane_port.contains("PortRecvFrameReceipt"),
        "lane_port must not name raw port receipts; it should traffic only in ReceivedFrame"
    );
    assert!(
        !offer.contains("!payload.as_bytes().is_empty()")
            && !offer_ingress.contains("!payload.as_bytes().is_empty()")
            && !offer_materialization.contains("!payload.as_bytes().is_empty()"),
        "offer preview staging must keep zero-length transport payloads as real consumed frames"
    );
    assert!(
        offer_state.contains("carried_binding_evidence: Option<LaneIngressEvidence>")
            && offer_state.contains("stage_binding_evidence: Option<LaneIngressEvidence>")
            && core.contains("rollback.carried_transport_payload")
            && core.contains("rollback.stage_transport_payload")
            && !core.contains("for (len, lane, _payload) in rollback.transport_payload"),
        "offer rollback must name carried/stage ingress ownership instead of hiding it in a mini-vec"
    );

    assert!(
        offer_ingress.contains("fn collect_offer_ingress(")
            && offer_materialization.contains("fn produce_branch(")
            && offer_materialization.contains("fn resolve_materialized_binding(")
            && offer_materialization.contains("fn resolve_materialized_transport(")
            && !offer.contains("ProbeBinding {")
            && !offer_ingress.contains("ProbeBinding {")
            && !offer_materialization.contains("ProbeBinding {")
            && offer_tests
                .contains("deep_right_nested_final_reply_offer_materializes_leaf_label_with_deferred_binding_ingress")
            && offer_tests
                .contains("produce_non_wire_recv_evidence_requeues_staged_transport_payload"),
        "transport payload consumed during offer collection must be either staged or requeued behind the offer ingress collector"
    );

    assert!(
        offer_ingress.contains("fn requeue_offer_transport_payload")
            && offer.contains("if let Some(payload) = transport_payload.take()")
            && offer.contains("self.requeue_offer_transport_payload(payload);")
            && offer_materialization.contains("self.requeue_offer_transport_payload(payload);"),
        "offer resolve requeue must consume staged payload ownership before returning to transport"
    );
    assert!(
        offer_tests.contains("produce_wire_recv_frame_mismatch_is_terminal_without_requeue")
            && offer_tests
                .contains("terminal frame mismatch must not rollback staged transport payload"),
        "terminal wire frame-label mismatch must be guarded by behavior coverage, not source-shape snippets"
    );
    assert!(
        offer_ingress.contains("lane_port::requeue_recv_frame(port, payload);")
            && !offer.contains("lane_port::requeue_recv_frame(port, payload);")
            && !offer_materialization.contains("lane_port::requeue_recv_frame(port, payload);"),
        "offer frontier transport requeue should stay behind the ingress ownership helper without freezing surrounding source shape"
    );
    assert!(
        lane_port.contains("fn requeue_recv_frame")
            && lane_port.contains("mut frame: ReceivedFrame<'r>")
            && lane_port.contains("frame.consume_receipt();"),
        "received frame requeue must explicitly consume frame authority instead of relying on Drop"
    );
    assert!(
        offer_state.contains("fn discard_terminal(&mut self)")
            && offer_state.contains("payload.discard_uncommitted();")
            && core.contains("state.discard_terminal();"),
        "offer terminal paths must explicitly discard unresolved transport frame authority"
    );
}

#[test]
fn direct_recv_requeues_transport_payload_when_binding_source_wins() {
    let recv = read("src/endpoint/kernel/recv.rs");
    let tests = read("tests/cursor_send_recv.rs");
    let build_pos = recv
        .find("fn build_recv_commit_plan(")
        .expect("direct recv must have a fallible commit-plan builder");
    let publish_pos = recv
        .find("fn publish_recv_commit_plan(")
        .expect("direct recv must have an infallible commit-plan publisher");
    let finish_pos = recv
        .find("fn finish_recv_payload(")
        .expect("direct recv must keep a thin finish wrapper");
    let build_block = &recv[build_pos..publish_pos];
    let publish_block = &recv[publish_pos..finish_pos];
    let finish_block = &recv[finish_pos..];
    let publish_requeue_pos = publish_block
        .find("lane_port::requeue_recv_frame(port, frame);")
        .expect("direct recv publish must perform pending transport frame requeue");
    let publish_audit_pos = publish_block
        .find("self.emit_endpoint_policy_audit(")
        .expect("direct recv publish must still emit EndpointRx audit");

    assert!(
        recv.contains(
            "let transport_payload_is_semantic =\n                !transport_payload.as_bytes().is_empty() || accepts_empty_payload;"
        )
            && recv.contains("RecvPayloadSource::Direct")
            && recv.contains("BindingWithUnconsumedTransport")
            && recv.contains("RecvCommitEffect::RequeueTransport")
            && recv.contains("frame.discard_uncommitted();")
            && !recv.contains("rollback_on_commit: Option")
            && !recv.contains("RecvPayloadSource::Borrowed")
            && !recv.contains("RecvPayloadSource::Binding {"),
        "direct recv must express late transport rollback as an explicit variant, not as nullable state or an overloaded binding source"
    );
    assert!(
        build_block.contains("if let Err(err) = validate(payload)")
            && build_block.contains("commit_effect.discard_uncommitted();")
            && build_block.contains("try_next_index_past_jumps()")
            && !build_block.contains("lane_port::requeue_recv_frame(port, frame);")
            && !build_block.contains("self.emit_endpoint_policy_audit("),
        "direct recv commit-plan build must hold all fallible preflight and no irreversible side effects"
    );
    assert!(
        publish_block.contains("RecvCommitEffect::RequeueTransport(frame)")
            && publish_requeue_pos < publish_audit_pos
            && publish_block.contains("self.set_cursor_index(next_index.as_usize());")
            && !publish_block.contains("map_err("),
        "direct recv commit-plan publish must requeue before EndpointRx audit and contain no fallible preflight"
    );
    assert!(
        finish_block.contains("let plan = self.build_recv_commit_plan")
            && finish_block.contains("let payload = self.publish_recv_commit_plan(plan);"),
        "direct recv finish must only connect commit-plan build and publish"
    );
    assert!(
        tests.contains("direct_recv_requeues_transport_payload_when_binding_wins_after_poll_recv")
            && tests.contains(
                "direct_recv_does_not_requeue_transport_payload_when_late_binding_payload_fails_validation"
            )
            && tests.contains("direct_recv_late_binding_requeues_before_endpoint_rx_audit_flush")
            && tests.contains(
                "demux_binding_without_policy_signals_keeps_empty_transport_payload_nonsemantic"
            ),
        "direct recv binding/transport ownership boundaries must have regression coverage"
    );
}

#[test]
fn array_map_unsafe_boundaries_are_explicit_and_panic_safe() {
    let map = read("src/control/lease/map.rs");
    let lease_core = read("src/control/lease/core.rs");

    assert!(
        map.contains("pub(crate) unsafe fn try_push_with")
            && map.contains(
                "`init` must fully initialize the provided slot before returning `Ok(())`"
            ),
        "ArrayMap::try_push_with must expose its MaybeUninit invariant as an unsafe contract"
    );
    assert!(
        lease_core.contains("SAFETY: The key written before delegation is `RendezvousId: Copy`")
            && lease_core.contains(".try_push_with("),
        "ArrayMap::try_push_with callers must document the exact initialized-state invariant"
    );
    assert!(
        map.contains("as_mut_ptr().replace((key, value))")
            && !map.contains(
                "assume_init_drop();\n                    self.entries[i].write((key, value));"
            ),
        "ArrayMap::insert must replace before dropping the old value"
    );
    assert!(
        map.contains("pub(crate) fn retain(&mut self, mut keep: impl FnMut(&K, &mut V) -> bool)\n    where\n        V: Copy,"),
        "ArrayMap::retain must stay constrained to Copy values instead of exposing a generic panic-unsafe compactor"
    );
    assert!(
        map.contains("let old_len = self.len;\n        self.len = 0;")
            && map.contains("self.entries[self.len].write(entry);")
            && map.contains("self.len += 1;"),
        "ArrayMap::retain must keep len as the committed retained prefix during unwinding"
    );
}

#[test]
fn transport_optional_default_is_explicit_and_documented() {
    let transport = read("src/transport.rs");
    let readme = read("README.md");
    let hygiene = read(".github/scripts/check_surface_hygiene.sh");
    let trait_block = transport
        .split("pub trait Transport {")
        .nth(1)
        .and_then(|tail| tail.split("#[cfg(test)]").next())
        .expect("Transport trait block must precede transport tests");

    assert!(
        trait_block
            .contains("fn operational_deadline_ticks(&self) -> Option<u32> {\n        None\n    }")
            && !trait_block.contains("apply_pacing_update"),
        "transport optional default must stay limited to the explicit wait-fuse hook"
    );
    for required in [
        "fn drain_events(&self, emit: &mut dyn FnMut(TransportEvent));",
        "fn recv_frame_hint<'a>(&'a self, rx: &'a Self::Rx<'a>) -> Option<FrameLabel>;",
        "fn metrics(&self) -> Self::Metrics;",
    ] {
        assert!(
            trait_block.contains(required),
            "non-optional transport evidence contracts must remain required: {required}"
        );
    }
    assert!(
        hygiene.contains("drain_events")
            && hygiene.contains("recv_frame_hint")
            && !hygiene.contains("fn[[:space:]]+apply_pacing_update"),
        "surface hygiene gate must continue rejecting semantic fallback hooks"
    );
    assert!(
        readme.contains("`drain_events(...)` and `metrics()` for observation and policy input")
            && readme.contains(
                "optional `operational_deadline_ticks()` for integration-owned wait fuses"
            )
            && !readme.contains("apply_pacing_update"),
        "README must document the wait-fuse default without teaching unused pacing fallback hooks"
    );
    assert!(
        trait_block.contains("Provide transport-level metrics for observation and policy input.")
            && trait_block.contains("Metrics are not route authority")
            && !trait_block.contains("metrics for routing decisions")
            && !trait_block.contains("adaptive route selection")
            && !trait_block.contains("protocol inference"),
        "Transport::metrics docs must keep metrics as observation/policy input, not route authority"
    );
}

#[test]
fn endpoint_resident_payload_unsafe_contracts_are_documented() {
    let lane_port = read("src/endpoint/kernel/runtime/lane_port.rs");

    for function in ["endpoint_resident_payload", "recv_from_binding"] {
        let marker = format!("unsafe fn {function}");
        let start = lane_port
            .find(&marker)
            .unwrap_or_else(|| panic!("missing unsafe helper: {function}"));
        let prefix_start = start.saturating_sub(700);
        let prefix = &lane_port[prefix_start..start];
        assert!(
            prefix.contains("# Safety"),
            "{function} must carry its unsafe preconditions at the function boundary"
        );
    }
}

#[test]
fn type_level_choreography_stays_segmented_without_new_dsl() {
    let g = read("src/g.rs");
    let program = read("src/global/program.rs");
    let const_dsl = read("src/global/const_dsl.rs");

    assert!(
        g.contains("pub use crate::global::program::Program;")
            && g.contains("pub use crate::global::{Msg, Role, par, route, send, seq};")
            && !g.contains("macro_rules!")
            && !g.contains("advanced")
            && !g.contains("loop_"),
        "app-facing choreography DSL must stay fixed to g::{{Role, Msg, Program, send, seq, route, par}}"
    );

    assert!(
        const_dsl.contains("segments: [[EffStruct; MAX_SEGMENT_EFFS]; MAX_SEGMENTS]")
            && const_dsl.contains("segment_summaries: [SegmentSummary; MAX_SEGMENTS]")
            && const_dsl.contains("pub(crate) const fn segment_count(&self) -> usize")
            && const_dsl.contains("pub(crate) const fn segment_len(&self, segment: usize) -> usize")
            && const_dsl.contains("pub(crate) const fn segment_summary(&self, segment: usize)")
            && program.contains("impl<Left, Right> BuildProgramSource for SeqSteps<Left, Right>")
            && program.contains(
                "const SOURCE: ProgramSourceData =\n        <Left as BuildProgramSource>::SOURCE.seq(<Right as BuildProgramSource>::SOURCE);",
            )
            && !program.contains("fn source_node_at(offset: usize) -> crate::eff::EffStruct")
            && program.contains("fn source_policy_at(offset: usize) -> Option<PolicyMode>")
            && program.contains("fn source_control_desc_at(offset: usize) -> Option<ControlDesc>")
            && program.contains("CompiledProgramImage::scan_const_with_lookup")
            && program.contains("#[cfg(not(any(feature = \"std\", test)))]\nimpl<Universe, Steps> Projectable<Universe> for Program<Steps>")
            && program.contains("validated_program_image::<Steps>().visit_projection_metadata(visitor);"),
        "g::seq must keep the public user path while embedded projection uses segmented descriptor facts instead of re-walking typed message metadata"
    );
}

#[test]
fn transport_contract_documents_lane_and_hint_drain() {
    let readme = read("README.md");
    let transport = read("src/transport.rs");
    let offer_frontier = read("src/endpoint/kernel/route_frontier/offer.rs");
    let offer_materialization = read("src/endpoint/kernel/route_frontier/offer/materialization.rs");
    let offer_facts = read("src/endpoint/kernel/route_frontier/offer/facts.rs");
    let scope_evidence = read("src/endpoint/kernel/route_frontier/scope_evidence_logic.rs");
    let offer_tests = read_offer_tests();
    let transport_tests = read("src/transport.rs");
    let test_transport = read("tests/common/mod.rs");
    let route_tests = read("tests/route_dynamic_control.rs");

    for (path, source) in [
        ("README.md", readme.as_str()),
        ("src/transport.rs", transport.as_str()),
    ] {
        assert!(
            source.contains("open(port)") || source.contains("PortOpen"),
            "{path} must document Transport::open as a descriptor-derived port witness"
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
        !readme.contains("open(local_role")
            && !readme.contains("open(local_role, session_id, lane)"),
        "README must not keep the old raw Transport::open contract"
    );
    assert!(
        transport.contains("fn open<'a>(")
            && transport.contains("port: PortOpen")
            && transport.contains("pub struct PortOpen")
            && transport.contains("session_id: crate::control::types::SessionId")
            && transport.contains("lane: crate::control::types::Lane"),
        "Transport trait must receive typed descriptor facts at attach/open time"
    );
    assert!(
        transport.contains("pub(crate) const fn from_descriptor(")
            && !transport.contains("pub const fn from_descriptor("),
        "PortOpen raw descriptor constructor must remain crate-private so external transport users cannot mint role/session/lane triples"
    );
    assert!(
        readme.contains("`requeue(...)` as the required rollback path")
            && transport.contains("A no-op requeue violates the")
            && transport.contains("endpoint rollback contract"),
        "Transport::requeue must be documented as a required rollback contract, not an optional best-effort hook"
    );
    let silent_noop_requeue = "fn requeue<'a>(&'a self, _rx: &'a mut Self::Rx<'a>) {}";
    let fail_fast_allowance =
        "Rollback contract exemption: this transport never exercises endpoint rollback.";
    let fail_fast_body = "unreachable!(\"this fixture never exercises endpoint rollback\")";
    for path in [
        "src/control/automaton/topology.rs",
        "src/control/lease/bundle.rs",
        "src/control/cluster/core.rs",
        "src/endpoint/kernel/test_support/core_offer_tests.rs",
        "src/rendezvous/core.rs",
    ] {
        let source = if path == "src/endpoint/kernel/test_support/core_offer_tests.rs" {
            read_offer_tests()
        } else {
            read(path)
        };
        let silent_noop_count = source.matches(silent_noop_requeue).count();
        let allowance_count = source.matches(fail_fast_allowance).count();
        let fail_fast_count = source.matches(fail_fast_body).count();
        assert_eq!(
            silent_noop_count, 0,
            "{path} must not keep silent no-op requeue exemptions"
        );
        assert_eq!(
            allowance_count, fail_fast_count,
            "{path} must pair every rollback exemption with a fail-fast requeue body"
        );
        if path == "src/endpoint/kernel/test_support/core_offer_tests.rs" {
            assert!(
                source.contains("Rollback contract implementation: `poll_recv` is stateless")
                    && source.contains("Nothing to restore."),
                "{path} must document the stateless replay transport instead of treating it as an exemption"
            );
        }
    }
    assert!(
        transport_tests.contains(
            "Rollback contract exemption: WakerAwareTransport only exercises direct poll_recv"
        ) && transport_tests
            .contains("unreachable!(\"WakerAwareTransport does not exercise endpoint rollback\")")
            && !transport_tests.contains(
                "fn requeue<'a>(&'a self, rx: &'a mut Self::Rx<'a>) {\n            let _ = rx;"
            ),
        "src/transport.rs waker fixture must not keep a silent no-op requeue body"
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
            && offer_frontier.contains("let arm = if is_dynamic_route_scope {\n                        None\n                    } else {")
            && offer_frontier.contains(".passive_dispatch_arm_from_exact_frame_label(scope_id, lane, frame_label)"),
        "passive route evidence may materialize payload readiness, but dynamic branch authority must not come from lane+frame hints"
    );
    assert!(
        offer_materialization.contains("transport_payload_frame_mismatch")
            && offer_tests
                .contains("passive_dynamic_offer_does_not_use_fresh_hint_as_route_authority")
            && offer_tests.contains("fresh frame hint must not bypass the dynamic route resolver"),
        "fresh transport hints must be tested as demux/materialization evidence, not dynamic route authority"
    );
    assert!(
        offer_frontier.contains("let has_ack =")
            && offer_frontier.contains("let has_frame_hint =")
            && offer_frontier.contains("if has_ack || has_frame_hint")
            && offer_facts.contains("passive_evidence_can_skip_recv")
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
