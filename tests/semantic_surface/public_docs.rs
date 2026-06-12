use super::common::*;

#[test]
fn stable_public_surface_allowlists_are_final_form() {
    assert_eq!(
        lines(".github/allowlists/lib-public-api.txt"),
        [
            "pub mod g;",
            "pub mod integration;",
            "pub use endpoint::{Endpoint, EndpointError, EndpointResult, Flow, RouteBranch};",
        ],
        "crate root public surface must stay on g + integration + endpoint core"
    );

    assert_eq!(
        lines(".github/allowlists/g-public-api.txt"),
        [
            "pub use Program;",
            "pub use Message;",
            "pub use Msg;",
            "pub use ControlMsg;",
            "pub use control;",
            "pub use send, seq, route, par;",
            "pub use Send, Seq, Route, Par;",
        ],
        "hibana::g must stay on canonical choreography terms and sealed message families"
    );

    let endpoint = lines(".github/allowlists/endpoint-public-api.txt");
    for required in [
        "pub struct Endpoint<'r, const ROLE: u8> {",
        "pub struct RouteBranch<'e, 'r, const ROLE: u8> {",
        "pub struct Flow<'e, 'r, const ROLE: u8, M> where M: crate::g::Message, {",
        "pub fn flow<'e, M>( &'e mut self, ) -> EndpointResult<crate::Flow<'e, 'r, ROLE, M>> where M: crate::g::Message, {",
        "pub fn recv<'e, M>(&'e mut self) -> impl core::future::Future<Output = EndpointResult<M::Decoded<'e>>> + 'e where M: crate::g::Message + 'e, {",
        "pub fn offer<'e>( &'e mut self, ) -> impl core::future::Future<Output = EndpointResult<RouteBranch<'e, 'r, ROLE>>> + 'e {",
        "pub fn label(&self) -> u8 {",
        "pub fn decode<M>(self) -> impl core::future::Future<Output = EndpointResult<M::Decoded<'e>>> where M: crate::g::Message, {",
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
        10,
        "endpoint public surface must not grow without an explicit final-form review"
    );

    let integration = lines(".github/allowlists/integration-public-api.txt").join("\n");
    let integration_source = read("src/integration/buckets.rs");
    for required in [
        "pub struct SessionKitStorage<'cfg, T, U = crate::runtime::consts::DefaultLabelUniverse, C = crate::runtime::config::CounterClock, const MAX_RV: usize = 4> where T: crate::transport::Transport + 'cfg, U: crate::runtime::consts::LabelUniverse + 'cfg, C: crate::runtime::config::Clock + 'cfg, {",
        "pub const fn uninit() -> Self {",
        "pub fn init(&mut self) -> &SessionKit<'cfg, T, U, C, MAX_RV> {",
        "pub mod program {",
        "pub use crate::global::program::Projectable;",
        "pub use crate::global::role_program::{RoleProgram, project};",
        "pub mod ids {",
        "pub use crate::control::types::SessionId;",
        "pub use crate::runtime::consts::{DefaultLabelUniverse, LabelUniverse, RING_EVENTS};",
        "pub mod resolver {",
        "pub use crate::control::cluster::core::{ DecisionArm, DecisionResolution, ResolverError, ResolverRef, };",
        "pub mod wire {",
        "pub use crate::transport::wire::{CodecError, Payload, WireEncode, WirePayload};",
        "pub mod transport {",
        "pub use crate::transport::{",
        "IngressEvidence",
        "ReceivedFrame",
    ] {
        assert!(
            integration.contains(required),
            "integration allowlist missing final-form item: {required}"
        );
    }
    assert!(
        !integration.contains("ResidentSessionKit"),
        "integration public surface must not retain a thin resident wrapper"
    );
    for removed in [
        "pub fn ingress(",
        "pub mod binding {",
        "IngressSlot",
        "IngressError",
        "IngressChannel,",
        "pub use crate::control::types::{Lane, SessionId};",
        "pub use crate::control::types::Lane",
        "integration::ids::{EffIndex, Lane, SessionId}",
    ] {
        assert!(
            !integration.contains(removed),
            "integration allowlist must not keep ingress binding as a final-form public API: {removed}"
        );
    }
    assert!(
        !integration_source.contains("pub mod binding {")
            && !integration_source.contains("pub fn ingress("),
        "integration source must not expose ingress binding as a core public API"
    );
    for forbidden in [
        "pub mod inspect {",
        "ProjectionMetadataVisitor",
        "ProjectionProgramFacts",
        "ProjectionAtomSpec",
        "ProjectionPolicySpec",
        "ProjectionScopeSpec",
    ] {
        assert!(
            !integration.contains(forbidden) && !integration_source.contains(forbidden),
            "projection inspection metadata is internal substrate, not public API: {forbidden}"
        );
    }
    let control_error = read("src/control/cluster/error.rs");
    assert!(
        !control_error.contains("EndpointResidentBudget")
            && !control_error.contains("EndpointStorageBudget")
            && control_error.contains("EndpointHeader")
            && control_error.contains("Self::EndpointHeader => \"ep-header\""),
        "public attach diagnostics must describe concrete endpoint allocation scopes, not internal resident vocabulary"
    );
    assert_eq!(
        integration_source
            .matches("pub use crate::global::program::Projectable;")
            .count(),
        1,
        "Projectable must remain a single sealed projection bound, not a duplicate wrapper surface"
    );
    let projection_source = read("src/global/program/projection.rs");
    assert!(
        projection_source.contains("pub trait Projectable: seal::Sealed")
            && !projection_source.contains("pub trait Projectable<")
            && !projection_source.contains("DefaultLabelUniverse")
            && !projection_source.contains("LabelUniverse")
            && !projection_source.contains("Projectable<DefaultLabelUniverse>")
            && !integration_source.contains("Projectable<DefaultLabelUniverse>"),
        "Projectable must stay a parameter-free unnamed-choreography bound; runtime label universes belong to storage/configuration, not projection"
    );
    let projectable_trait = projection_source
        .split("pub trait Projectable: seal::Sealed {")
        .nth(1)
        .and_then(|tail| tail.split("impl<P> Projectable").next())
        .expect("Projectable trait block must be present");
    assert!(
        !projectable_trait.contains("fn ")
            && !projectable_trait.contains("visit_projection_metadata")
            && !projectable_trait.contains("fn project<const ROLE"),
        "Projectable must stay a pure sealed bound; projection and metadata authority stay on Hibana owners"
    );
    assert!(
        !integration_source.contains("pub mod advanced {")
            && integration_source.contains("pub use crate::global::program::Projectable;"),
        "unnamed choreography projection must use the canonical project entry, not a second advanced bucket"
    );
    let role_program_project = read("src/global/role_program/program.rs");
    assert!(
        role_program_project.contains("pub fn project<const ROLE: u8, P>")
            && role_program_project.contains("program: &P")
            && role_program_project.contains("P: crate::global::program::Projectable + ?Sized")
            && role_program_project.contains("crate::global::program::project_unnamed(program)")
            && !role_program_project.contains("ProjectableProgram"),
        "canonical project must accept both concrete g::Program terms and unnamed Projectable wrappers through one entry"
    );
    let ids = read("src/control/types.rs");
    let rendezvous_impl = ids
        .split("impl RendezvousId {")
        .nth(1)
        .and_then(|tail| tail.split("#[cfg(test)]").next())
        .expect("RendezvousId impl must be present");
    assert!(
        !rendezvous_impl.contains("pub const fn new")
            && rendezvous_impl.contains("pub(crate) const fn new"),
        "RendezvousId must be internal registry identity, not reconstructed from raw public input"
    );
    assert!(
        integration.contains("pub fn rendezvous( &self")
            && integration.contains(
                ") -> Result<RendezvousKit<'_, 'cfg, T, U, C, false, MAX_RV>, AttachError> {"
            )
            && !integration.contains("pub fn add_rendezvous( &self")
            && !integration.contains("Result<crate::integration::ids::RendezvousId"),
        "public rendezvous registration must return the registered RendezvousKit witness and must not expose raw id attach authority"
    );
    for forbidden in [
        "ProjectionMessageSpec",
        "ProjectionTypeFingerprint",
        "ProjectableProgram",
        "pub use crate::global::Message;",
        "CAP_HANDLE_LEN",
        "CapError",
        "RouteArmHandle",
        "LoopDecisionHandle",
    ] {
        assert!(
            !integration.contains(forbidden),
            "integration allowlist must not keep internal projection or handle-codec surface: {forbidden}"
        );
    }
    assert!(
        !std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("src/ingress.rs")
            .exists()
            && !std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("src/binding.rs")
                .exists(),
        "core ingress binding bridge must stay deleted; receive demux belongs to Transport"
    );
    let readme = read("README.md");
    for stale in [
        "enter(None)",
        "Passing `None`",
        "`None` at attach time",
        "role(...).binding",
        "protocol-agnostic binding API",
    ] {
        assert!(
            !readme.contains(stale) && !integration_source.contains(stale),
            "public docs must not teach stale ingress attach or binding wording: {stale}"
        );
    }
    let removed_attach_verb = concat!("enter_with_", "binding");
    assert!(
        !readme.contains(removed_attach_verb) && !integration_source.contains(removed_attach_verb),
        "public docs must not preserve a second public attach verb"
    );
}

#[test]
fn protocol_guide_documents_route_choice_without_raw_control_vocabulary() {
    let readme = read("README.md");

    for required in [
        "Route choice is a protocol fact",
        "Prefer in-band choice",
        "`integration::resolver`",
        "Built-in local",
        "`g::ControlMsg` self-sends",
        "stay local to the endpoint",
        "Protocol crates that compose runtime/control protocol events",
        "`g::ControlMsg` with Hibana-defined",
        "`g::control::*` markers",
        "State and transaction controls use the same shape",
        "snapshot starts the local",
        "fail closed if the snapshot",
        "generation does not exist",
        "g::control::StateSnapshot",
        "g::control::TxnCommit",
        "`ReceivedFrame`",
        "`IngressEvidence`",
        "descriptor-checked evidence",
    ] {
        assert!(
            readme.contains(required),
            "README route/evidence section missing mechanism text: {required}"
        );
    }
    assert!(
        !readme.contains("GUIDE.md")
            && !readme.contains("GenericCapToken")
            && !readme.contains("WireControlKind")
            && !readme.contains("WireControlEffect")
            && !readme.contains("integration::cap"),
        "README must own the route/evidence guide without exposing raw control substrate"
    );
    for stale in [
        "route self-send",
        "wire/local effects",
        "Protocol-owned wire or local",
        "Public protocol-owned local",
    ] {
        assert!(
            !readme.contains(stale),
            "public docs must not imply custom protocol-owned local minting: {stale}"
        );
    }
}

#[test]
fn capability_tokens_are_documented_as_registered_token_not_mac_authority() {
    let mint = capability_token_source();
    let rendezvous = rendezvous_core_source();
    let capability = read("src/rendezvous/capability.rs");
    let cap_error = read("src/control/cap/mint/error.rs");
    let rendezvous_error = read("src/rendezvous/error.rs");
    let readme = read("README.md");

    for required in [
        "[16B nonce | 40B descriptor header]",
        "trusted-domain registered-token state",
        "Endpoint-owned token authority comes from a nonce entry",
        "Token bytes stop at the descriptor header;",
        "not a keyed verifier",
        "Endpoint-local control progression is witnessed by rendezvous-scoped brands",
        "This module is the rendezvous registered-token owner",
        "Topology wire controls are",
        "Hibana-owned endpoint/session bindings",
        "Public `g::ControlMsg` controls are protocol descriptors",
        "Local controls are endpoint-owned self-sends",
        "distributed topology",
        "unit control events whose token/header is minted",
        "Descriptor, token header, or resource-owned handle-byte mismatch.",
    ] {
        assert!(
            mint.contains(required)
                || capability.contains(required)
                || cap_error.contains(required),
            "capability token docs must teach the trusted-domain registered-token authority: {required}"
        );
    }

    assert!(
        !mint.contains("mint_token"),
        "capability docs must not mention stale mint_token convenience APIs"
    );
    let resource = read("src/control/cap/mint/resource.rs");
    let global_message = read("src/global/message.rs");
    assert!(
        !resource.contains("WireControlKind")
            && !mint.contains("GenericCapToken")
            && !mint.contains("CONTROL_PAYLOAD_WIRE_TOKEN")
            && !global_message.contains("GenericCapToken")
            && !resource.contains("handle_scope")
            && !readme.contains("const TAP_ID")
            && !resource.contains("pub trait EndpointOwnedControlKind")
            && resource.contains("pub(crate) trait LocalControlKind")
            && !readme.contains("GenericCapToken")
            && !readme.contains("WireControlKind")
            && !readme.contains("WireControlEffect"),
        "raw explicit wire-token substrate must not remain beside topology wire control authority"
    );
    let token = read("src/control/cap/mint/token.rs");
    let header = read("src/control/cap/mint/header.rs");
    let effects = read("src/control/cluster/effects.rs");
    assert!(
        !header.contains("tap_id(")
            && !header.contains("observe::ids")
            && effects.contains("pub(crate) const fn control_op_tap_event_id")
            && effects.contains("use crate::observe::ids;"),
        "capability header codec must not own observability metadata; op tap ids belong to the control-effect owner"
    );
    assert!(
        !token.contains("pub fn scope(&self)")
            && !token.contains("pub struct HandleView")
            && !token.contains("pub fn as_view")
            && !token.contains("scope_from_header")
            && !token.contains("scope: Option<ScopeId>"),
        "public capability token views must not expose duplicate raw scope or decoded handle authority"
    );
    for forbidden in [
        "Rendezvous::mint_cap",
        "Rendezvous::claim_cap",
        "claim_cap()",
        "ClaimableResourceKind",
        "CapError::Exhausted",
        "One-shot token already consumed",
        "UnknownToken",
        "WrongSessionOrLane",
        "TableFull",
        "ledger entry on claim",
        "one_shot_exhausts_on_second_claim",
        "claim authority is the nonce ledger",
        "ledger-entry mismatch",
    ] {
        assert!(
            !mint.contains(forbidden)
                && !rendezvous.contains(forbidden)
                && !capability.contains(forbidden)
                && !cap_error.contains(forbidden),
            "capability docs must not retain deleted claim/mint compatibility text: {forbidden}"
        );
    }
    assert!(
        !mint.contains("thread_local!") && !mint.contains("[u8; 6]"),
        "capability docs must stay no_std-friendly and avoid stale short-token layouts"
    );
    assert!(
        !mint.contains("pub(crate) struct GenericCapToken")
            && !mint.contains("WireControlKind")
            && mint.contains("pub(crate) struct ControlToken")
            && !mint.contains(".field(\"bytes\"")
            && mint.contains("impl fmt::Debug for ControlToken"),
        "opaque control token debug output must be redacted and must not expose token bytes"
    );
    assert!(
        !mint.to_ascii_lowercase().contains("affine proof object")
            && !rendezvous
                .to_ascii_lowercase()
                .contains("affine proof object"),
        "capability docs must not describe an unused internal proof object"
    );
    assert!(
        (mint.contains("resource-owned handle") || cap_error.contains("resource-owned handle"))
            && !cap_error.contains("field validation failures (kind/shot/sid/lane)")
            && !cap_error.contains("Token field mismatch (kind/shot/sid/lane)")
            && !cap_error.contains("the token was found in CapTable")
            && !rendezvous_error.contains("field mismatch (kind/shot/sid/lane)"),
        "CapError docs must include handle-byte validation and avoid stale field-only wording"
    );
    assert!(
        !rendezvous_error.contains("CapError"),
        "rendezvous error module must not mirror or re-export capability ledger errors"
    );
    assert!(
        !read("tests/ui.rs").contains("g-wire-control-zero-tag.rs"),
        "wire control tag gates are internal now; UI coverage must not expose public control kinds"
    );

    let mint_lower = mint.to_ascii_lowercase();
    let rendezvous_lower = rendezvous.to_ascii_lowercase();
    let capability_lower = capability.to_ascii_lowercase();
    for forbidden in [
        "authentication tag",
        "keyed_hash",
        "invalidmac",
        "invalidproof",
        "proof bytes",
        "mac validation",
        "mac tag",
        "no authentication tag",
        "secure claim path",
        "cryptographically validated",
        "cap_tag_len",
        "cap_proof_len",
        "cap_strategy_len",
        "derive_tag",
        "derive_proof",
        "derive_strategy",
        "strategy bytes",
        "strategy-owned",
        "ledger-free",
        "original capability token system",
        "`resourcekind::shot`",
        "multisafe",
        "capability table is full (64 entries)",
        "external control/resource kinds must not use `0`",
        "nonce-authenticated",
        "ensure_authenticated_session_lane",
        "authenticated lane",
        "timing attacks",
        "attacker",
        "# security",
    ] {
        assert!(
            !mint_lower.contains(forbidden)
                && !rendezvous_lower.contains(forbidden)
                && !capability_lower.contains(forbidden),
            "capability implementation must not imply a cryptographic MAC ledger path: {forbidden}"
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
        "state machines",
        "state-machine",
        "state-machines",
        "TransportSnapshotParts",
        "ConfigParts",
        "RegisteredTokenParts",
        "TransportOpsError",
        "pub mod binding {",
        "IngressSlot",
        "IngressError",
        "ingress::IngressEvidence",
        "binding::advanced",
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
        "resident",
        "Resident",
    ] {
        assert!(
            !joined.contains(forbidden),
            "public allowlists must not retain forbidden final-form name: {forbidden}"
        );
    }
}
