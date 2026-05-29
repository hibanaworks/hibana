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
            "pub use MessageSpec;",
            "pub use Msg;",
            "pub use Role;",
            "pub use send, seq, route, par;",
            "pub use Send, Seq, Route, Par, Policy;",
        ],
        "hibana::g must stay DSL-only"
    );

    let endpoint = lines(".github/allowlists/endpoint-public-api.txt");
    for required in [
        "pub struct Endpoint<'r, const ROLE: u8> {",
        "pub struct RouteBranch<'e, 'r, const ROLE: u8> {",
        "pub struct Flow<'e, 'r, const ROLE: u8, M> where M: crate::g::MessageSpec, {",
        "pub fn flow<'e, M>( &'e mut self, ) -> EndpointResult<crate::Flow<'e, 'r, ROLE, M>> where M: crate::g::MessageSpec, {",
        "pub fn recv<'e, M>(&'e mut self) -> impl core::future::Future<Output = EndpointResult<M::Decoded<'e>>> + 'e where M: crate::g::MessageSpec + 'e, {",
        "pub fn offer<'e>( &'e mut self, ) -> impl core::future::Future<Output = EndpointResult<RouteBranch<'e, 'r, ROLE>>> + 'e {",
        "pub fn label(&self) -> u8 {",
        "pub fn decode<M>(self) -> impl core::future::Future<Output = EndpointResult<M::Decoded<'e>>> where M: crate::g::MessageSpec, {",
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
    for required in [
        "pub struct SessionKitStorage<'cfg, T, U = crate::runtime::consts::DefaultLabelUniverse, C = crate::runtime::config::CounterClock, const MAX_RV: usize = 4> where T: crate::transport::Transport + 'cfg, U: crate::runtime::consts::LabelUniverse + 'cfg, C: crate::runtime::config::Clock + 'cfg, {",
        "pub const fn uninit() -> Self {",
        "pub fn init(&mut self) -> &SessionKit<'cfg, T, U, C, MAX_RV> {",
        "pub mod program {",
        "pub use crate::global::role_program::{RoleProgram, project};",
        "pub mod advanced {",
        "pub use crate::global::program::Projectable;",
        "pub mod inspect {",
        "pub use crate::global::program::{ ProjectionAtomSpec, ProjectionMetadataVisitor, ProjectionPolicySpec, ProjectionProgramFacts, ProjectionScopeSpec, };",
        "pub mod ids {",
        "pub use crate::control::types::{Lane, RendezvousId, SessionId};",
        "pub use crate::runtime::consts::{DefaultLabelUniverse, LabelUniverse, RING_EVENTS};",
        "pub mod binding {",
        "pub use crate::binding::{BindingError, EndpointSlot, Channel, IngressEvidence};",
        "pub mod policy {",
        "pub use crate::control::cluster::core::{ DecisionArm, DecisionResolution, ResolverError, ResolverRef, };",
        "pub mod wire {",
        "pub use crate::transport::wire::{CodecError, Payload, WireEncode, WirePayload};",
        "pub mod transport {",
        "pub use crate::transport::{FrameLabel, Outgoing, PortOpen, Transport, TransportError};",
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
    for forbidden in [
        "ProjectionMessageSpec",
        "ProjectionTypeFingerprint",
        "ProjectableProgram",
        "pub use crate::global::MessageSpec;",
    ] {
        assert!(
            !integration.contains(forbidden),
            "integration allowlist must not keep std/test-only projection metadata: {forbidden}"
        );
    }
}

#[test]
fn protocol_guide_documents_the_public_control_op_catalogue() {
    let readme = read("README.md");
    let protocol = read("GUIDE.md");

    for variant in control_op_variants() {
        let needle = format!("`ControlOp::{variant}`");
        assert!(
            protocol.contains(&needle),
            "GUIDE control-message section must document public control op: {needle}"
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
            protocol.contains(required),
            "GUIDE control-message section missing mechanism text: {required}"
        );
    }
    assert!(
        readme.contains(
            "The full control opcode catalogue and custom wire-control shape live in `GUIDE.md`."
        ),
        "README must point protocol implementors to the detailed control guide"
    );
}

#[test]
fn capability_tokens_are_documented_as_registered_token_not_mac_authority() {
    let mint = capability_token_source();
    let rendezvous = rendezvous_core_source();
    let capability = read("src/rendezvous/capability.rs");
    let cap_error = read("src/control/cap/mint/error.rs");
    let rendezvous_error = read("src/rendezvous/error.rs");

    for required in [
        "[16B nonce | 40B descriptor header]",
        "trusted-domain registered-token state",
        "Endpoint-owned token authority comes from a nonce entry",
        "Token bytes stop at the descriptor header;",
        "not a keyed verifier",
        "Endpoint-local control progression is witnessed by rendezvous-scoped brands",
        "This module is the rendezvous registered-token owner",
        "Control resource kinds must not use `0`.",
        "[`ControlResourceKind::SHOT`](super::ControlResourceKind::SHOT)",
        "Descriptor, typed-token, or resource-owned handle-byte mismatch.",
        "Decoding must be deterministic, side-effect-free, and non-authoritative.",
        "fn encode_handle(handle: &Self::Handle) -> [u8; CAP_HANDLE_LEN]",
        "Result<GenericCapToken<PageResource>, CodecError>",
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
        "capability docs must stay no_std-friendly and use the public CAP_HANDLE_LEN contract"
    );
    assert!(
        !mint.contains("#[derive(Debug, PartialEq, Eq)]\npub struct GenericCapToken")
            && !mint.contains(".field(\"bytes\"")
            && mint.contains("impl<K: ResourceKind> fmt::Debug for GenericCapToken<K>"),
        "opaque GenericCapToken debug output must be redacted and must not expose token bytes"
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
        read("tests/ui/g-control-resource-zero-tag.rs").contains("const TAG: u8 = 0;")
            && read("tests/ui/g-control-resource-zero-tag.stderr")
                .contains("control resource tag 0 is reserved"),
        "control resource tag zero must have UI coverage for the const descriptor gate"
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
    ] {
        assert!(
            !joined.contains(forbidden),
            "public allowlists must not retain forbidden final-form name: {forbidden}"
        );
    }
}
