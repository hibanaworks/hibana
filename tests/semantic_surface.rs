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
            "pub mod substrate;",
            "pub use endpoint::{Endpoint, RecvError, RecvResult, RouteBranch, SendError, SendResult};",
        ],
        "crate root public surface must stay on g + substrate + endpoint core"
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
        "pub fn flow<'e, M>( &'e mut self, ) -> SendResult<flow::Flow<'e, 'r, ROLE, M>> where M: crate::global::MessageSpec + crate::global::SendableLabel, {",
        "pub fn recv<'e, M>(&'e mut self) -> impl core::future::Future<Output = RecvResult<<<M as crate::global::MessageSpec>::Payload as crate::transport::wire::WirePayload>::Decoded<'e>>> + 'e where M: crate::global::MessageSpec + 'e, M::Payload: crate::transport::wire::WirePayload, {",
        "pub fn offer<'e>( &'e mut self, ) -> impl core::future::Future<Output = RecvResult<RouteBranch<'e, 'r, ROLE>>> + 'e {",
        "pub fn label(&self) -> u8 {",
        "pub fn decode<M>(self) -> impl core::future::Future<Output = RecvResult<<<M as crate::global::MessageSpec>::Payload as crate::transport::wire::WirePayload>::Decoded<'e>>> where M: crate::global::MessageSpec, M::Payload: crate::transport::wire::WirePayload, {",
        "pub enum SendError {",
        "pub enum RecvError {",
        "pub type SendResult<T> = core::result::Result<T, SendError>;",
        "pub type RecvResult<T> = core::result::Result<T, RecvError>;",
    ] {
        assert!(
            endpoint.iter().any(|line| line == required),
            "endpoint allowlist missing final-form item: {required}"
        );
    }
    assert_eq!(
        endpoint.len(),
        11,
        "endpoint public surface must not grow without an explicit final-form review"
    );

    let substrate = lines(".github/allowlists/substrate-public-api.txt").join("\n");
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
            substrate.contains(required),
            "substrate allowlist missing final-form item: {required}"
        );
    }
}

#[test]
fn public_surface_allowlists_keep_forbidden_names_out() {
    let joined = [
        read(".github/allowlists/lib-public-api.txt"),
        read(".github/allowlists/g-public-api.txt"),
        read(".github/allowlists/endpoint-public-api.txt"),
        read(".github/allowlists/substrate-public-api.txt"),
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
    ] {
        assert!(
            !joined.contains(forbidden),
            "public allowlists must not retain forbidden final-form name: {forbidden}"
        );
    }
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
}

#[test]
fn stable_public_api_gate_has_no_nightly_or_rustdoc_json_owner() {
    let script = read(".github/scripts/check_hibana_public_api.sh");
    let workflow = read(".github/workflows/quality-gates.yml");
    let combined = format!("{script}\n{workflow}");

    for required in [
        "export TOOLCHAIN=\"${TOOLCHAIN:-stable}\"",
        "bash ./.github/scripts/check_hibana_public_api.sh",
        "stable public API check passed",
    ] {
        assert!(
            combined.contains(required),
            "stable public API gate missing required owner: {required}"
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
