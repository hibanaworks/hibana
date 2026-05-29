use std::fs;
use std::path::PathBuf;

fn read(path: &str) -> String {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let full = root.join(path);
    fs::read_to_string(&full)
        .unwrap_or_else(|err| panic!("read {} failed: {}", full.display(), err))
}

fn repo_path(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path)
}

fn read_dir_rs(path: &str) -> String {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path);
    let mut parts = fs::read_dir(&root)
        .unwrap_or_else(|err| panic!("read {} failed: {}", root.display(), err))
        .map(|entry| {
            entry
                .unwrap_or_else(|err| {
                    panic!("read dir entry in {} failed: {}", root.display(), err)
                })
                .path()
        })
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("rs"))
        .collect::<Vec<_>>();
    parts.sort();
    let mut source = String::new();
    for part in parts {
        source.push_str(
            &fs::read_to_string(&part)
                .unwrap_or_else(|err| panic!("read {} failed: {}", part.display(), err)),
        );
    }
    source
}

fn cluster_core_source() -> String {
    let mut source = read("src/control/cluster/core.rs");
    source.push_str(&read_dir_rs("src/control/cluster/core"));
    source
}

fn capability_token_source() -> String {
    let mut source = read("src/control/cap/mint.rs");
    source.push_str(&read_dir_rs("src/control/cap/mint"));
    source
}

fn integration_source() -> String {
    let mut source = read("src/integration.rs");
    source.push_str(&read_dir_rs("src/integration"));
    source
}

fn transport_source() -> String {
    let mut source = read("src/transport.rs");
    source.push_str(&read_dir_rs("src/transport"));
    source
}

#[test]
fn core_source_tree_no_longer_keeps_mgmt_or_epf_owners() {
    for deleted in [
        repo_path("src/runtime/mgmt.rs"),
        repo_path("src/runtime/mgmt"),
        repo_path("src/epf.rs"),
        repo_path("src/epf"),
    ] {
        assert!(
            !deleted.exists(),
            "core tree must remove the deleted owner path: {}",
            deleted.display()
        );
    }
}

#[test]
fn transport_context_keeps_replay_attrs_named() {
    let context_src = read("src/transport/context.rs");

    assert!(
        !context_src.contains("PolicySlot"),
        "transport context must not expose audit slot vocabulary through resolver input"
    );
    assert!(
        !context_src.contains("policy::epf::Slot"),
        "transport context must not mention the deleted core EPF slot path"
    );
    assert!(
        context_src.contains("pub const fn latency_us(&self) -> Option<u64>")
            && context_src.contains("pub const fn queue_depth(&self) -> Option<u32>"),
        "policy attrs must keep arbitrary lookup internal and expose named replay accessors"
    );
    assert!(
        !context_src.contains("pub fn query(&self, id: ContextId) -> Option<ContextValue>"),
        "packed policy attrs must not keep duplicate lookup aliases"
    );
    assert!(
        !context_src.contains("Route-policy input"),
        "transport context must not describe resolver input as route-only"
    );
}

#[test]
fn binding_surface_is_ingress_only() {
    let binding_src = read("src/binding.rs");
    let readme_src = read("README.md");
    let observe_src = read("src/observe/core.rs");

    for forbidden in [
        "policy_signals",
        "PolicySignals",
        "route_policy_signals",
        "Route-policy input",
        "Route-policy staging",
        "route_input(",
        "route_attrs(",
    ] {
        assert!(
            !binding_src.contains(forbidden)
                && !readme_src.contains(forbidden)
                && !observe_src.contains(forbidden),
            "public binding surface must not expose policy signal vocabulary: {forbidden}"
        );
    }
}

#[test]
fn core_resource_kind_catalogue_keeps_mgmt_and_policy_lifecycle_internal_only() {
    let resource_kinds_src = read("src/control/cap/resource_kinds.rs");
    let mint_src = capability_token_source();

    for forbidden in [
        "pub struct PolicyLoadKind;",
        "pub struct PolicyActivateKind;",
        "pub struct PolicyRevertKind;",
        "pub struct PolicyAnnotateKind;",
        "pub struct LoadBeginKind;",
        "pub struct LoadCommitKind;",
    ] {
        assert!(
            !resource_kinds_src.contains(forbidden),
            "core must not remain the public owner of mgmt/policy lifecycle kinds: {forbidden}"
        );
    }

    assert!(
        !mint_src.contains("pub bytes: [u8; CAP_TOKEN_LEN]")
            && !mint_src.contains("fn from_parts("),
        "GenericCapToken must not expose or retain capability wire-layout part constructors"
    );
    assert!(
        !mint_src.contains("#[derive(Debug, PartialEq, Eq)]\npub struct GenericCapToken")
            && !mint_src.contains(".field(\"bytes\"")
            && mint_src.contains("impl<K: ResourceKind> fmt::Debug for GenericCapToken<K>"),
        "GenericCapToken must keep debug output redacted because the token is an opaque payload"
    );
    assert!(
        !mint_src.contains("pub const fn new(\n        sid: SessionId,\n        lane: Lane,\n        role: u8,\n        tag: u8,"),
        "CapHeader must not expose a raw multi-field public constructor"
    );
    assert!(
        !integration_source().contains("CapHeader"),
        "CapHeader must remain an internal codec carrier, not an integration surface owner"
    );
    for forbidden in [
        "pub fn nonce(&self)",
        "pub fn tag(&self)",
        "pub fn control_header(&self)",
        "pub fn shot(&self)",
        "pub fn handle_bytes(&self)",
        "pub fn handle_bytes_ref(&self)",
        "pub fn decode_handle(&self)",
    ] {
        assert!(
            !mint_src.contains(forbidden),
            "GenericCapToken must keep low-level token/header accessors internal: {forbidden}"
        );
    }
}

#[test]
fn integration_runtime_surface_owns_tapevent_resource() {
    let integration_src = integration_source();

    assert!(
        integration_src.contains("pub mod runtime {")
            && integration_src.contains("pub use crate::observe::core::TapEvent;"),
        "integration runtime surface must expose TapEvent with the storage envelope"
    );

    for forbidden in [
        "TapBatch",
        "RawEvent",
        "for_each_since",
        "install_ring",
        "push(",
    ] {
        assert!(
            !integration_src.contains(forbidden),
            "integration tap surface must stay minimal: {forbidden}"
        );
    }
}

#[test]
fn integration_policy_surface_is_decision_input_owner() {
    let integration_src = integration_source();

    assert!(
        integration_src.contains("ResolverRef")
            && integration_src.contains("pub mod replay {")
            && integration_src.contains("pub use crate::transport::context::PolicyAttrs;"),
        "integration::policy must keep resolver root and replay attrs under policy::replay"
    );
    let policy_root = integration_src
        .split("pub mod policy {")
        .nth(1)
        .and_then(|tail| tail.split("/// Canonical capability-token surface").next())
        .expect("integration policy surface must be followed by cap surface");
    for required in ["ResolverRef", "pub mod replay"] {
        assert!(
            policy_root.contains(required),
            "integration::policy must keep the resolver root and replay owner: {required}"
        );
    }
    let policy_root_before_replay = policy_root
        .split("pub mod replay {")
        .next()
        .expect("policy root must contain replay bucket");
    for forbidden in [
        "ResolverContext",
        "ContextId",
        "ContextValue",
        "PolicyInput",
        "PolicySignals,",
        "PolicySlot",
        "pub mod core",
    ] {
        assert!(
            !policy_root_before_replay.contains(forbidden),
            "integration::policy root must not expose lower-level replay metadata: {forbidden}"
        );
    }
    for forbidden in [
        "pub mod advanced {",
        "pub mod epf {",
        "crate::epf::",
        "policy::epf",
    ] {
        assert!(
            !policy_root.contains(forbidden),
            "integration::policy must not regrow deleted or compatibility buckets: {forbidden}"
        );
    }
}

#[test]
fn dynamic_policy_surface_uses_one_decision_resolver() {
    let cluster_src = cluster_core_source();
    let integration_src = integration_source();
    let readme_src = read("README.md");
    let decision_policy_src = read("src/endpoint/kernel/core/decision_policy/impls.rs");
    let collapsed_resolution = concat!("Dynamic", "Resolution");
    let generic_stateless_ctor = concat!("ResolverRef::", "from_fn");
    let generic_state_ctor = concat!("ResolverRef::", "from_state");

    for src in [&cluster_src, &integration_src, &readme_src] {
        assert!(
            !src.contains(collapsed_resolution),
            "dynamic resolver surface must use the named DecisionResolution API, not a generic DynamicResolution alias"
        );
        assert!(
            !src.contains(generic_stateless_ctor) && !src.contains(generic_state_ctor),
            "resolver constructors must stay decision-named"
        );
    }

    for required in [
        "pub enum DecisionResolution",
        "pub fn decision_fn",
        "pub fn decision_state",
    ] {
        assert!(
            cluster_src.contains(required),
            "dynamic resolver public SPI must keep the route/loop-neutral decision item: {required}"
        );
    }

    for forbidden in [
        "pub enum LoopResolution",
        "pub fn loop_fn",
        "pub fn loop_state",
        concat!(
            "pub fn decision_fn(resolver: fn(ResolverContext) -> DecisionResolution",
            "Outcome)"
        ),
        concat!(
            "resolver: fn(&S, ResolverContext) -> DecisionResolution",
            "Outcome,"
        ),
        "pub struct ResolverContext",
    ] {
        assert!(
            !cluster_src.contains(forbidden),
            "dynamic resolver public SPI must not expose loop resolver or private alias residue: {forbidden}"
        );
    }
    assert!(
        !decision_policy_src.contains("if meta.peer == ROLE"),
        "dynamic decision policy must not bypass resolver validation for local route/loop self-send controls"
    );
}

#[test]
fn core_policy_runtime_has_no_in_crate_appliance_shim() {
    let policy_runtime = read("src/policy_runtime.rs");
    for forbidden in [
        "pub(crate) struct PolicyCtx",
        "pub(crate) struct HostSlots",
        "pub(crate) enum Action",
        "pub(crate) struct AbortInfo",
        "pub(crate) enum Trap",
    ] {
        assert!(
            !policy_runtime.contains(forbidden),
            "hibana core must not keep an old policy appliance shim: {forbidden}"
        );
    }

    for path in [
        "src/rendezvous/port.rs",
        "src/rendezvous/core.rs",
        "src/endpoint/kernel/core.rs",
    ] {
        let src = read(path);
        for forbidden in ["run_policy(", "policy_mode_tag("] {
            assert!(
                !src.contains(forbidden),
                "hibana core must audit policy inputs without a no-op policy executor: {path}: {forbidden}"
            );
        }
    }

    let authority = read("src/endpoint/kernel/authority.rs");
    for forbidden in [
        "RoutePolicyDecision",
        "route_policy_decision_from_action",
        "DeferSource::Epf",
    ] {
        assert!(
            !authority.contains(forbidden),
            "route authority must stay Ack | Resolver | Poll only: {forbidden}"
        );
    }
}

#[test]
fn transport_policy_signal_surface_stays_minimal() {
    let transport_src = transport_source();
    let integration_src = integration_source();
    let readme_src = read("README.md");

    assert!(
        !transport_src.contains("pub struct TransportSnapshot"),
        "TransportSnapshot must stay internal"
    );
    assert!(
        !transport_src.contains("TransportSnapshotParts"),
        "transport snapshot option-bag constructor must not exist"
    );
    assert!(
        !transport_src.contains("from_parts(parts:"),
        "transport snapshot parts constructor must not exist"
    );
    assert!(
        !integration_src.contains("TransportSnapshotParts"),
        "integration::transport must not re-export TransportSnapshotParts"
    );
    assert!(
        !integration_src.contains("TransportSnapshot"),
        "integration::transport must not re-export TransportSnapshot"
    );
    assert!(
        !readme_src.contains("TransportSnapshot"),
        "README must not publish the removed TransportSnapshot surface"
    );
    assert!(
        !transport_src.contains("fn policy_attrs(&self)")
            && !transport_src.contains("pub trait TransportMetrics")
            && !transport_src.contains("type Metrics"),
        "Transport must not expose policy input, metrics, or telemetry compatibility hooks"
    );
    assert!(
        readme_src.contains("`integration::policy::replay::PolicyAttrs`")
            && readme_src.contains("ResolverRef::decision_state"),
        "README must describe replay attrs and resolver-state owned input"
    );
    for required in [
        "pub mod replay {",
        "pub use crate::transport::context::PolicyAttrs;",
    ] {
        assert!(
            integration_src.contains(required),
            "policy replay attrs must remain publicly reachable: {required}"
        );
    }
    for forbidden in [
        "ContextId",
        "ContextValue",
        "PolicyInput",
        "PolicySignals",
        "pub mod core {",
    ] {
        assert!(
            !integration_src.contains(forbidden),
            "policy signal extension namespace must not leak through integration: {forbidden}"
        );
    }
    for forbidden in [
        "pub const fn new(latency_us: Option<u64>, queue_depth: Option<u32>) -> Self",
        "pub const fn with_latency_us",
        "pub const fn with_queue_depth",
        "pub const fn with_congestion_marks",
        "pub const fn with_retransmissions",
        "pub const fn with_congestion_window",
        "pub const fn with_in_flight",
        "pub const fn with_algorithm",
    ] {
        assert!(
            !transport_src.contains(forbidden),
            "transport snapshot builder surface must stay removed: {forbidden}"
        );
    }
    for forbidden in [
        "TransportMetricsTapPayload,",
        "pub primary: (u32, u32)",
        "pub extension: Option<(u32, u32)>",
        "pub kind: TransportEventKind",
        "pub packet_number: u64",
        "pub payload_len: u32",
        "pub retransmissions: u32",
        "pub pn_space: u8",
        "pub cid_tag: u8",
        "pub struct TransportEventMeta",
        "pub packet_number: u64",
        "pub retransmissions: u32",
        "pub const fn new(\n        kind: TransportEventKind,\n        packet_number: u64",
        "pub const fn packet_number(",
        "pub const fn payload_len(",
        "pub const fn retry_count(",
        "pub const fn domain(",
        "pub const fn carrier_tag(",
        "pub const fn retransmissions(",
        "pub const fn packet_number_space(",
        "pub const fn connection_id_tag(",
        "pub const fn new_with_metadata",
        "pub const fn with_pn_space",
        "pub const fn with_cid_tag",
    ] {
        assert!(
            !transport_src.contains(forbidden) && !integration_src.contains(forbidden),
            "transport observation detail must stay accessor-only and non-literal: {forbidden}"
        );
    }
    assert!(
        !transport_src.contains("TransportEventKind")
            && !transport_src.contains("pub struct TransportEvent")
            && !integration_src.contains("TransportEventKind")
            && !integration_src.contains("TransportEvent"),
        "transport telemetry vocabulary must not be part of the protocol-neutral public surface"
    );
}

#[test]
fn core_repo_checks_do_not_assume_sibling_checkout_layout() {
    for path in [
        ".github/scripts/check_mgmt_boundary.sh",
        ".github/scripts/check_plane_boundaries.sh",
        ".github/scripts/check_surface_hygiene.sh",
        "tests/docs_surface.rs",
    ] {
        let src = read(path);
        for forbidden in [
            "../hibana-mgmt",
            "../hibana-epf",
            "hibana crate must live under the repository root",
        ] {
            assert!(
                !src.contains(forbidden),
                "core repo checks must not assume sibling checkout layout: {path}: {forbidden}"
            );
        }
    }
}
