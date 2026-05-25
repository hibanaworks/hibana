use super::common::*;

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
    let mint = cap_mint_source();
    let rendezvous = rendezvous_core_source();
    let capability = read("src/rendezvous/capability.rs");
    let rendezvous_error = read("src/rendezvous/error.rs");

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
    ] {
        assert!(
            mint.contains(required) || capability.contains(required),
            "capability token docs must teach the trusted-domain nonce-ledger authority: {required}"
        );
    }

    assert!(
        !mint.contains("mint_token"),
        "capability docs must not mention stale mint_token convenience APIs"
    );
    assert!(
        !mint.to_ascii_lowercase().contains("affine proof object")
            && !rendezvous
                .to_ascii_lowercase()
                .contains("affine proof object"),
        "capability docs must not describe an unused internal proof object"
    );
    assert!(
        mint.contains("resource-owned handle")
            && !mint.contains("field validation failures (kind/shot/sid/lane)")
            && !mint.contains("Token field mismatch (kind/shot/sid/lane)")
            && !rendezvous_error.contains("field mismatch (kind/shot/sid/lane)"),
        "CapError::Mismatch docs must include handle-byte validation and avoid stale field-only wording"
    );
    assert!(
        rendezvous_error.contains("pub(crate) use crate::control::cap::mint::CapError;")
            && !rendezvous_error.contains("pub(crate) enum CapError"),
        "rendezvous claim path must use the canonical capability error owner instead of mirroring it"
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
    ] {
        assert!(
            !mint_lower.contains(forbidden)
                && !rendezvous_lower.contains(forbidden)
                && !capability_lower.contains(forbidden),
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
