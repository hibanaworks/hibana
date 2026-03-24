use std::fs;
use std::path::PathBuf;

fn read(path: &str) -> String {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let full = root.join(path);
    fs::read_to_string(&full)
        .unwrap_or_else(|err| panic!("read {} failed: {}", full.display(), err))
}

fn contains_tokenish(haystack: &str, needle: &str) -> bool {
    haystack.match_indices(needle).any(|(idx, _)| {
        let before = haystack[..idx].chars().next_back();
        let after = haystack[idx + needle.len()..].chars().next();
        let is_boundary = |ch: Option<char>| {
            ch.is_none_or(|value| !(value.is_ascii_alphanumeric() || value == '_'))
        };
        is_boundary(before) && is_boundary(after)
    })
}

#[test]
fn public_docs_use_canonical_surface_paths() {
    let readme = read("README.md");
    let lib_rs = read("src/lib.rs");
    let api_sketch = read("../api-sketch.md");
    let agents = read("../AGENTS.md");

    for source in [&readme, &lib_rs] {
        for forbidden in [
            "hibana::global::",
            "hibana::runtime::",
            "hibana::control::",
            "hibana::transport::",
            "hibana::observe::",
            "observe::TapEvent",
            "observe::for_each_since",
            "hibana::endpoint::CursorEndpoint",
            "hibana::g::policy(",
            "g::policy::",
            "g::project::<",
            "g::RoleProgram<",
            "hibana::g::with_policy(",
            "hibana::g::advanced::with_policy(",
            "hibana::g::route::<",
            "hibana::g::par::<",
            "hibana::g::route_chain(",
            "hibana::g::par_chain(",
            "PolicyMode::dynamic(",
            "epf::ops::",
            "mgmt_epf_observe.rs",
            "runtime::consts::",
            "`attach_cursor`",
            "handshake/interop",
            "interop-style end-to-end session driving",
            "cluster.attach_cursor(",
            "claim.attach_cursor(",
            "BindingSlot::map_lane(",
            "`map_lane()`",
            "NullBinding",
            "`binding::Binding`",
            "Incoming as IncomingClassification",
            "`SendMeta`",
            "type Cluster = SessionCluster",
            "VmSlot",
            "VmHeader",
            "endpoint::delegate",
            "SessionCluster::delegate_claim",
            "claim.enter(",
            "CONTROLLER_PROGRAM",
            "examples/tcp_tokio.rs",
            "examples/custom_binding.rs",
            "examples/transactional_session.rs",
            "examples/distributed_migration.rs",
            "examples/mgmt_epf_control.rs",
            "--example tcp_tokio",
            "--example custom_binding",
            "--example transactional_session",
            "--example distributed_migration",
            "--example mgmt_epf_control",
            "slot: /* hidden slot-scoped callback parameter */",
            "prefer_local_input: bool",
            "Program<_>",
            "Result<_, ()>",
            "NoopMetrics",
            "ContextValue::from_bool(",
            "mint_endpoint_token(",
            "OneShot",
            "ManyShot",
            "Result<DynamicResolution, ()>",
            "TRYBUILD=overwrite cargo test -p hibana --test ui --features std",
            "`test-utils`",
            "runtime::LABEL_",
            "substrate::runtime::LABEL_",
            "cargo test -p hibana\n",
        ] {
            assert!(
                !source.contains(forbidden),
                "public docs must not teach hidden legacy root paths: {forbidden}"
            );
        }
        assert!(
            !contains_tokenish(source, "binding::SendMeta"),
            "public docs must not teach the deleted SendMeta alias"
        );
    }

    for forbidden in [
        "Program<_>",
        "Result<_, ()>",
        "type WrongLocal =",
        "pub struct SessionCluster<'cfg, T, U, C, const MAX_RV: usize> { /* ... */ }",
        "pub fn enter_controller<...>(...)",
        "pub fn enter_stream_controller<...>(...)",
        "pub fn enter_stream_cluster<...>(...)",
        "pub async fn drive_stream_cluster<...>(...)",
        "pub async fn drive_stream_controller<...>(...)",
        "Client<'_, AppSteps>",
        "Server<'_, AppSteps>",
        "Result<hibana::Endpoint<'_,",
        "hibana::substrate::{One, Many}",
        "hibana::substrate::{Many, One}",
    ] {
        assert!(
            !api_sketch.contains(forbidden),
            "api-sketch must not keep owner-hiding shorthand or underscore escapes: {forbidden}"
        );
    }
    for forbidden in
        ["// set_resolver / enter_controller / enter_cluster / enter_stream_* / drive_cluster /"]
    {
        assert!(
            !api_sketch.contains(forbidden),
            "api-sketch must not keep the deleted duplicate mgmt::session resolver entry: {forbidden}"
        );
    }
    for required in [
        "pub use crate::control::cluster::core::{",
        "ResolverError,",
        "CONGESTION_MARKS,",
        "TRANSPORT_ALGORITHM,",
        "canonical public owner: hibana::substrate::policy::epf::{Header, Slot}",
        "pub mod advanced {",
        "AllowsCanonical,",
        "MintConfig,",
        "RouteDecisionKind,",
        "ControlScopeKind, ScopeId",
        "pub mod wire {",
        "CodecError, Payload, WireDecode, WireEncode",
        "pub mod transport {",
        "TransportAlgorithm,",
        "TransportEventKind,",
        "TransportSnapshot,",
        "pub struct SessionCluster<'cfg, T, U, C, const MAX_RV: usize>",
        "T: hibana::substrate::Transport,",
        "U: hibana::substrate::runtime::LabelUniverse + 'cfg,",
        "C: hibana::substrate::runtime::Clock + 'cfg;",
        "pub fn new(clock: &'cfg C) -> Self;",
        "pub fn enter_controller<'lease, 'cfg, T, U, C, B, const MAX_RV: usize>(",
        "pub fn add_rendezvous_from_config(",
        "pub use crate::runtime::mgmt::{LoadRequest, Request, SlotRequest};",
        "pub fn enter_cluster<'lease, 'cfg, T, U, C, B, const MAX_RV: usize>(",
        "pub fn enter_stream_controller<'lease, 'cfg, T, U, C, B, const MAX_RV: usize>(",
        "pub fn enter_stream_cluster<'lease, 'cfg, T, U, C, B, const MAX_RV: usize>(",
        "rv_id: hibana::substrate::RendezvousId,",
        "sid: hibana::substrate::SessionId,",
        "hibana::substrate::AttachError,",
        "Result<hibana::substrate::mgmt::Reply, hibana::substrate::mgmt::MgmtError>",
        "T: hibana::substrate::Transport + 'cfg,",
        "U: hibana::substrate::runtime::LabelUniverse + 'cfg,",
        "C: hibana::substrate::runtime::Clock + 'cfg;",
        "impl<'request> Request<'request> {",
        "pub async fn drive_controller<'lease, T, U, C, Mint, B, const MAX_RV: usize>(",
        "pub async fn drive_cluster<'lease, 'cfg, T, U, C, Mint, B, const MAX_RV: usize>(",
        "pub async fn drive_stream_cluster<'lease, T, U, C, Mint, F, B, const MAX_RV: usize>(",
        "pub async fn drive_stream_controller<",
        "Mint::Policy: hibana::substrate::cap::advanced::AllowsCanonical,",
        "F: FnMut() -> bool,",
        "F: FnMut(hibana::substrate::mgmt::session::tap::TapEvent) -> bool,",
        "B: hibana::substrate::binding::BindingSlot;",
        "subscribe: hibana::substrate::mgmt::SubscribeReq,",
        "hibana::Endpoint",
        "hibana::g::advanced::RoleProgram",
        "hibana::substrate::policy::{PolicyId, ResolverContext, DynamicResolution, ResolverError}",
        "hibana::substrate::runtime::{Config, Clock, LabelUniverse}",
        "canonical public owner: hibana::substrate::cap::{One, Many}",
        "pub async fn client<'entry, AppSteps>(",
        "entry: Client<'entry, AppSteps>,",
        "pub async fn server<'entry, AppSteps>(",
        "entry: Server<'entry, AppSteps>,",
        "UdpTransport,",
        "hibana::substrate::runtime::DefaultLabelUniverse,",
        "hibana::substrate::runtime::CounterClock,",
        "hibana::substrate::cap::advanced::EpochTbl,",
        "hibana::substrate::cap::advanced::MintConfig,",
        "hibana::Endpoint<\n            'cfg,\n            ROLE,",
        "pub async fn shutdown(&self);",
        "hibana-quic::app::h3::{Client, Server}",
        "hibana-quic::app::raw::{Client, Server}",
        "pub server_addr: core::net::SocketAddr,",
        "pub server_name: rustls::pki_types::ServerName<'static>,",
        "pub tls: rustls::ClientConfig,",
        "pub tls: rustls::ServerConfig,",
        "pub app: &'a hibana::g::Program<AppSteps>,",
    ] {
        assert!(
            api_sketch.contains(required),
            "api-sketch must document the live substrate owner instead of omitting it: {required}"
        );
    }

    for required in [
        "## App Surface",
        "App authors should stay on `g` and `Endpoint`.",
        "## Protocol-Implementor Walkthrough",
        "## Substrate Surface (protocol implementors only)",
        "Protocol implementors use the protocol-neutral SPI:",
        "### App Result and Error Types",
        "## Control Message Surface",
        "## Transport Seam",
        "## BindingSlot Contract",
        "## Policy Plane",
        "## Control Messages and Capability Kinds",
        "## Wire and Transport Observation",
        "## Management Session",
        "`hibana::SendResult<T>`",
        "`hibana::RecvResult<T>`",
        "`hibana::SendError`",
        "`hibana::RecvError`",
        "`hibana::RouteBranch`",
        "`hibana::substrate::CpError`",
        "`hibana::substrate::AttachError`",
        "`hibana::substrate::Transport`",
        "`hibana::substrate::binding::NoBinding`",
        "`hibana::substrate::policy::PolicySignalsProvider`",
        "`hibana::substrate::policy::epf::{Header, Slot}`",
        "`hibana::substrate::wire::{Payload, WireDecode, WireEncode}`",
        "`CanonicalControl<K>`",
        "`ExternalControl<K>`",
        "`MessageSpec`",
        "`ControlMessage`",
        "`ControlMessageKind`",
        "`EffList`",
        "`Config::new(tap_buf, slab)`",
        "`Config::tap_storage()` and `Config::slab()`",
        "`Config::with_lane_range(range)`",
        "`ContextValue::{NONE, FALSE, TRUE}`",
        "`PolicyAttrs::new()`, `insert(id, value)`, and `query(id)`",
        "There is no public `g::splice`, `g::delegate`, or `g::reroute`.",
        "`RouteDecisionKind`, `LoopContinueKind`, `LoopBreakKind`",
        "`SpliceIntentKind`, `SpliceAckKind`, `RerouteKind`",
        "`PolicyLoadKind`, `PolicyActivateKind`, `PolicyRevertKind`, `PolicyAnnotateKind`",
        "`LoadBeginKind`, `LoadCommitKind`",
        "`Reply::Loaded(report)`",
        "`Reply::ActivationScheduled(report)`",
        "`Reply::Reverted(report)`",
        "`Reply::Stats { stats, staged_version }`",
        "`hibana::substrate::mgmt::session::Request::Load(LoadRequest)`",
        "`hibana::substrate::mgmt::session::Request::LoadAndActivate(LoadRequest)`",
        "`hibana::substrate::mgmt::session::Request::Activate(SlotRequest)`",
        "`hibana::substrate::mgmt::session::Request::Revert(SlotRequest)`",
        "`hibana::substrate::mgmt::session::Request::Stats(SlotRequest)`",
        "`LoadRequest`",
        "`SlotRequest`",
        "`enter_cluster`",
        "`drive_cluster`",
        ".drive_controller(controller)",
        "Dynamic policy remains explicit:",
        "there is no public VM-run API separate from the resolver/policy surface",
        "`BindingSlot` is demux and transport observation only. It does not decide route arms.",
        "heap-backed lower-layer storage",
        "`no_alloc` oriented in",
        "bash ./.github/scripts/check_hibana_public_api.sh",
        "bash ./.github/scripts/check_policy_surface_hygiene.sh",
        "bash ./.github/scripts/check_surface_hygiene.sh",
        "bash ./.github/scripts/check_boundary_contracts.sh",
        "bash ./.github/scripts/check_direct_projection_binary.sh",
        "bash ./.github/scripts/check_no_std_build.sh",
        "cargo check --all-targets -p hibana",
        "cargo test -p hibana --features std",
        "cargo test -p hibana --test ui --features std",
        "cargo test -p hibana --test policy_replay --features std",
    ] {
        assert!(
            readme.contains(required),
            "README must document the canonical app/substrate split and validation flow: {required}"
        );
    }

    for line in readme.lines() {
        let trimmed = line.trim_start();
        let is_role_or_message_synonym = trimmed.contains("= Role<") || trimmed.contains("= Msg<");
        let is_step_or_projection_shorthand = trimmed.contains("= StepCons<")
            || trimmed.contains("= SeqSteps<")
            || trimmed.contains("= LoopContinueSteps<")
            || trimmed.contains("= LoopBreakSteps<")
            || trimmed.contains("= LoopDecisionSteps<")
            || (trimmed.contains("= <") && trimmed.contains("as ProjectRole<"))
            || (trimmed.contains("= <") && trimmed.contains("as StepConcat<"));
        assert!(
            !(trimmed.starts_with("type ")
                && (is_role_or_message_synonym || is_step_or_projection_shorthand)),
            "README must not teach owner-hiding type aliases: {trimmed}"
        );
    }

    for source in [&api_sketch, &agents] {
        for forbidden in [
            "resolver fallback",
            "ALPN fallback",
            "`attach_cursor`",
            "attach_cursor()",
            "pub use crate::runtime::AttachError;",
            "RoleProgram<..., StepNil>",
            "RV_ID, SESSION_ID, LANE, TAG, LATENCY_US, QUEUE_DEPTH, ...",
        ] {
            assert!(
                !source.contains(forbidden),
                "design docs must not keep stale rescue/lower-layer vocabulary: {forbidden}"
            );
        }
    }

    for forbidden in [
        "TRYBUILD=overwrite cargo test -p hibana --test ui --features std",
        "Program<_>",
        "hibana-quic/.github/scripts/check_no_testcase_surface.sh",
        "hibana-quic/.github/scripts/check_stack_surface_hygiene.sh",
        "hibana-quic/.github/scripts/check_localside_transport_boundary.sh",
    ] {
        assert!(
            !agents.contains(forbidden),
            "AGENTS must not keep stale fail-open validation, underscore escapes, or redundant quic sub-gate paths: {forbidden}"
        );
    }

    for required in [
        "cargo test -p hibana --test ui --features std",
        "bash ./.github/scripts/check_hibana_public_api.sh",
        "bash ./.github/scripts/check_policy_surface_hygiene.sh",
        "bash ./.github/scripts/check_boundary_contracts.sh",
        "bash ./hibana/.github/scripts/check_direct_projection_binary.sh",
        "bash ./hibana/.github/scripts/check_no_std_build.sh",
        "add_rendezvous_from_config(config, transport)",
        "Result<DynamicResolution, ResolverError>",
        "Request::drive_controller",
        "drive_cluster(cluster, rv_id, sid, endpoint)",
        "Result<hibana::substrate::mgmt::Reply, hibana::substrate::mgmt::MgmtError>",
        "FnMut() -> bool",
        "FnMut(hibana::substrate::mgmt::session::tap::TapEvent) -> bool",
        "hibana::substrate::mgmt::SubscribeReq",
        "hibana::substrate::binding::BindingSlot",
        "enter_stream_controller",
        "enter_stream_cluster",
        "hibana::substrate::cap::advanced",
        "hibana::substrate::wire",
        "hibana::substrate::transport",
        "SessionCluster::new(clock)",
        "hibana::substrate::policy::epf::{Header, Slot}",
        "hibana-quic::app::h3::{Client, Server}",
        "hibana-quic::app::raw::{Client, Server}",
        "socket`, `server_addr`, `server_name`, `tls`, `app`",
        "`Server` は `socket`, `tls`, `app`",
        "std::net::UdpSocket",
        "core::net::SocketAddr",
        "rustls::pki_types::ServerName<'static>",
        "rustls::ClientConfig",
        "rustls::ServerConfig",
        "ConnectionHandle` の public operation は `shutdown()` だけ",
        "Client<'entry, AppSteps>",
        "Server<'entry, AppSteps>",
        "hibana::substrate::runtime::DefaultLabelUniverse",
        "hibana::substrate::runtime::CounterClock",
        "hibana::substrate::cap::advanced::EpochTbl",
        "hibana::substrate::cap::advanced::MintConfig",
        "UdpTransport",
        "bash ./hibana-quic/.github/scripts/check_public_api.sh",
        "bash ./hibana-quic/.github/scripts/check_boundary_contracts.sh",
        "cargo test -p hibana-quic --test surface_invariants --manifest-path hibana-quic/Cargo.toml",
        "bash ./hibana-quic/.github/scripts/run_neqo_non_v2_regression.sh",
        "hibana-quic/.github/scripts/check_public_api.sh",
        "hibana-quic/.github/scripts/check_boundary_contracts.sh",
        "hibana-quic/.github/scripts/run_neqo_non_v2_regression.sh",
    ] {
        assert!(
            agents.contains(required),
            "AGENTS must document the canonical validation/gate owners: {required}"
        );
    }

    assert_eq!(
        agents
            .matches("add_rendezvous_from_config(config, transport)")
            .count(),
        1,
        "AGENTS must keep a single canonical rendezvous bootstrap path"
    );

    assert!(
        api_sketch.contains("pub use crate::control::cluster::error::{AttachError, CpError};"),
        "api-sketch must document AttachError and CpError under the cluster error owner"
    );

    let cargo_toml = read("Cargo.toml");
    for forbidden in [
        "[[example]]",
        "name = \"tcp_tokio\"",
        "name = \"custom_binding\"",
        "name = \"transactional_session\"",
        "name = \"distributed_migration\"",
        "name = \"mgmt_epf_control\"",
        "test-utils = []",
        "hmac = ",
        "sha2 = ",
        "rand = ",
        "rand_chacha = ",
    ] {
        assert!(
            !cargo_toml.contains(forbidden),
            "crate manifest must not keep stale generic examples or their dead dependencies: {forbidden}"
        );
    }
}
