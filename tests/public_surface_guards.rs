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
    let mut source = read("src/session/cluster/core.rs");
    source.push_str(&read_dir_rs("src/session/cluster/core"));
    source
}

fn owner_witness_source() -> String {
    read("src/session/brand.rs")
}

fn runtime_source() -> String {
    let mut source = read("src/runtime.rs");
    source.push_str(&read_dir_rs("src/runtime"));
    source
}

fn transport_source() -> String {
    let mut source = read("src/transport.rs");
    source.push_str(&read_dir_rs("src/transport"));
    source
}

#[test]
fn sidecar_owner_single_authority() {
    let storage_layout = read("src/rendezvous/core/storage_layout.rs");
    assert!(
        storage_layout.contains("pub(crate) struct Sidecar<T>")
            && storage_layout.contains("ptr: *mut T")
            && storage_layout.contains("bytes: usize")
            && storage_layout.contains("reclaim_delta: usize"),
        "storage layout must keep the single sidecar owner for ptr/bytes/reclaim_delta"
    );

    for path in [
        "src/rendezvous/association.rs",
        "src/rendezvous/tables/route_table.rs",
        "src/rendezvous/tables/route_table/storage.rs",
        "src/session/cluster/core/dynamic_resolvers.rs",
        "src/rendezvous/core.rs",
        "src/rendezvous/core/storage_layout/capacity/endpoint_lease.rs",
    ] {
        let source = read(path);
        for forbidden in [
            "storage_reclaim_delta",
            "STORAGE_TAG_MASK",
            "encode_frames_ptr",
            "encode_entries_ptr",
            "endpoint_lease_reclaim_delta",
            "reclaim_delta:",
        ] {
            assert!(
                !source.contains(forbidden),
                "{path} must not independently retain sidecar reclaim authority: {forbidden}"
            );
        }
    }
}

#[test]
fn resolver_sidecar_replacement_publishes_after_release() {
    let resolver_bucket = read("src/session/cluster/core/dynamic_resolvers/bucket.rs");
    let source_pos = resolver_bucket
        .find("let source_storage = self.storage_sidecar();")
        .expect("resolver capacity growth must capture the source sidecar");
    let stage_pos = resolver_bucket
        .find("self.init_replacement_storage(storage.cast(), required);")
        .expect("resolver replacement must stage entries before release");
    let release_pos = resolver_bucket
        .find("free(source_storage.cast())")
        .expect("resolver replacement must release the source sidecar");
    let commit_pos = resolver_bucket
        .find("self.commit_storage(storage.cast(), required);")
        .expect("resolver replacement must publish the staged sidecar");
    assert!(
        source_pos < stage_pos && stage_pos < release_pos && release_pos < commit_pos,
        "resolver replacement must stage into the new sidecar, release the old sidecar, then publish"
    );

    assert!(
        resolver_bucket
            .contains("pub(in crate::session::cluster::core) unsafe fn init_replacement_storage")
            && resolver_bucket.contains("if let Some(entry) = *source_entries.add(source_idx)")
            && !resolver_bucket.contains("(*source_entries.add(source_idx)).take()"),
        "resolver staging must copy entries without mutating the published source bucket"
    );
}

#[test]
fn assoc_and_route_sidecar_replacement_stage_before_release() {
    let capacity = read("src/rendezvous/core/storage_layout/capacity.rs");
    let assoc_stage_pos = capacity
        .find(".init_replacement_storage(lease.ptr(), lane_base, target_slots);")
        .expect("assoc replacement must stage entries before release");
    let assoc_release_pos = capacity
        .find("self.release_sidecar(source_assoc, ResourceScope::LaneStorage)")
        .expect("assoc replacement must release the source sidecar");
    let assoc_commit_pos = capacity
        .find(".commit_storage(lease.ptr(), lane_base, target_slots);")
        .expect("assoc replacement must publish only after release");
    assert!(
        assoc_stage_pos < assoc_release_pos && assoc_release_pos < assoc_commit_pos,
        "assoc replacement must stage, release, then publish"
    );

    let route_stage_pos = capacity
        .find("self.routes.migrate_from_storage(")
        .expect("route replacement must stage entries before release");
    let route_release_pos = capacity
        .find("self.release_sidecar(source_route, ResourceScope::RouteTable)")
        .expect("route replacement must release the source sidecar");
    let route_commit_pos = capacity
        .find("self.routes.rebind_from_storage(")
        .expect("route replacement must publish only after release");
    assert!(
        route_stage_pos < route_release_pos && route_release_pos < route_commit_pos,
        "route replacement must stage, release, then publish"
    );

    let assoc = read("src/rendezvous/association.rs");
    assert!(
        assoc.contains("pub(super) unsafe fn init_replacement_storage")
            && assoc.contains("WaiterSlot::init_clone_from")
            && !assoc.contains("core::ptr::read(source_waiters.add(source_idx))"),
        "assoc staging must not move out of the published source waiter column"
    );

    let route = read("src/rendezvous/tables/route_table/storage.rs");
    assert!(
        route.contains("WaiterSlot::init_clone_from") && !route.contains("src_waiter.take()"),
        "route staging must not move out of the published source waiter column"
    );
}

#[test]
fn public_header_surface_minimal() {
    let transport = read("src/transport.rs");
    let impl_start = transport
        .find("impl FrameHeader {")
        .expect("FrameHeader impl block");
    let frame_header_source = &transport[impl_start..];
    let impl_open = frame_header_source
        .find('{')
        .expect("FrameHeader impl open");
    let mut depth = 0usize;
    let mut impl_end = None;
    for (idx, byte) in frame_header_source[impl_open..].bytes().enumerate() {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    impl_end = Some(impl_open + idx + 1);
                    break;
                }
            }
            _ => {}
        }
    }
    let frame_header_impl = &frame_header_source[..impl_end.expect("FrameHeader impl close")];
    assert!(
        frame_header_impl.contains("pub const fn from_bytes(bytes: [u8; 8]) -> Self")
            && frame_header_impl.contains("pub const fn bytes(self) -> [u8; 8]"),
        "FrameHeader public surface must expose byte evidence roundtrip"
    );
    for forbidden in [
        "pub const fn new(",
        "pub const fn from_raw(",
        "pub const fn raw(",
        "pub const fn session(",
        "pub const fn lane(",
        "pub const fn source_role(",
        "pub const fn target_role(",
        "pub const fn label(",
    ] {
        assert!(
            !frame_header_impl.contains(forbidden),
            "FrameHeader must not expose public field packing or unpacking: {forbidden}"
        );
    }
    assert!(
        !read(".github/allowlists/runtime-public-api.txt").contains("FrameHeader::new"),
        "public allowlist must not preserve FrameHeader::new"
    );
    assert!(
        !read(".github/allowlists/runtime-public-api.txt").contains("FrameHeader::from_raw")
            && !read(".github/allowlists/runtime-public-api.txt").contains("FrameHeader::raw"),
        "public allowlist must not teach FrameHeader u64 raw access"
    );
}

#[test]
fn localside_transport_seal() {
    let localside_sources = [
        "src/local.rs",
        "src/endpoint.rs",
        "src/endpoint/ops.rs",
        "src/endpoint/flow.rs",
        "src/endpoint/branch.rs",
    ]
    .map(read)
    .join("\n");

    for forbidden in [
        "FrameHeader::from_parts",
        "FrameHeader::new",
        "pack_frame_header",
        "ReceivedFrame::framed",
        "Transport::poll_recv",
        "Transport::poll_send",
    ] {
        assert!(
            !localside_sources.contains(forbidden),
            "localside surface must not expose or direct-call transport substrate: {forbidden}"
        );
    }
}

#[test]
fn core_source_tree_no_longer_keeps_mgmt_or_epf_owners() {
    for forbidden in [
        repo_path("src/runtime/mgmt.rs"),
        repo_path("src/runtime/mgmt"),
        repo_path("src/epf.rs"),
        repo_path("src/epf"),
    ] {
        assert!(
            !forbidden.exists(),
            "core tree must remove the forbidden owner path: {}",
            forbidden.display()
        );
    }
}

#[test]
fn transport_context_owner_stays_forbidden() {
    assert!(
        !repo_path("src/transport/context.rs").exists(),
        "transport context owner must stay forbidden; resolver input is owned by explicit resolver state"
    );
}

#[test]
fn core_ingress_binding_surface_stays_forbidden() {
    let readme_src = read("README.md");
    let observe_src = read("src/observe/core.rs");

    assert!(
        !repo_path("src/ingress.rs").exists() && !repo_path("src/binding.rs").exists(),
        "core ingress binding files must stay forbidden; transport owns receive demux state"
    );

    for forbidden in [
        "resolver_signals",
        "ResolverSignals",
        "route_resolver_signals",
        "Route-resolver input",
        "Route-resolver staging",
        "route_input(",
        "route_attrs(",
    ] {
        assert!(
            !readme_src.contains(forbidden) && !observe_src.contains(forbidden),
            "public binding surface must not expose resolver signal vocabulary: {forbidden}"
        );
    }
}

#[test]
fn core_resource_kind_catalogue_keeps_mgmt_and_resolver_lifecycle_internal_only() {
    let owner_src = owner_witness_source();

    for forbidden in [
        concat!("src/session/", "cap.rs"),
        concat!("src/session/", "cap"),
        concat!("src/session/", "cap/atomic_codecs.rs"),
        concat!("src/session/", "cap/resource_kinds.rs"),
        concat!("src/session/", "cap", "/mi", "nt/header.rs"),
        concat!("src/session/", "cap", "/mi", "nt/token.rs"),
        concat!("src/session/", "cap", "/mi", "nt/error.rs"),
    ] {
        assert!(
            !repo_path(forbidden).exists(),
            "forbidden session codec substrate must stay forbidden: {forbidden}"
        );
    }

    for forbidden_name in [
        "ResolverLoadKind",
        "ResolverActivateKind",
        "ResolverRevertKind",
        "ResolverAnnotateKind",
        "LoadBeginKind",
        "LoadCommitKind",
    ] {
        let forbidden = format!("pub struct {forbidden_name};");
        assert!(
            !owner_src.contains(&forbidden),
            "core must not remain the public owner of mgmt/resolver lifecycle kinds: {forbidden}"
        );
    }
    let forbidden_control_token = ["Control", "Token"].concat();
    let forbidden_cap_header = ["Cap", "Header"].concat();
    assert!(
        !owner_src.contains(&forbidden_control_token)
            && !owner_src.contains(&forbidden_cap_header)
            && !runtime_source().contains(&forbidden_cap_header),
        "brand owner witness must not retain raw token/header session substrate"
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
            !owner_src.contains(forbidden),
            "forbidden token substrate must keep low-level token/header accessors out of this owner: {forbidden}"
        );
    }
}

#[test]
fn runtime_surface_hides_tap_storage_resource() {
    let mut runtime_src = read("src/runtime.rs");
    runtime_src.push_str(&read("src/runtime/buckets.rs"));
    let runtime_allowlist = read(".github/allowlists/runtime-public-api.txt");

    assert!(
        !runtime_src.contains("pub mod runtime {")
            && runtime_src.contains("pub mod tap {")
            && runtime_src.contains("pub use crate::observe::core::{Evidence, TapEvent, TapPort};")
            && !runtime_src.contains("pub use crate::runtime_core::consts::RING_EVENTS;")
            && !runtime_src
                .lines()
                .any(|line| line.trim() == "pub use crate::observe::core::TapEvent;")
            && !runtime_src.contains("CounterClock")
            && !runtime_src.contains("Clock")
            && !runtime_src.contains("RING_EVENTS")
            && runtime_allowlist.contains("pub mod tap {")
            && runtime_allowlist
                .contains("pub use crate::observe::core::{Evidence, TapEvent, TapPort};")
            && !runtime_allowlist.contains("pub use crate::observe::core::TapEvent;")
            && !runtime_allowlist.contains("CounterClock")
            && !runtime_allowlist.contains("Clock")
            && !runtime_allowlist.contains("RING_EVENTS"),
        "runtime surface may expose tap diagnostics only under runtime::tap and must hide tap storage and clock resources"
    );

    for forbidden in [
        "TapBatch",
        concat!("Raw", "Event"),
        "for_each_since",
        "install_ring",
        "push(",
    ] {
        assert!(
            !runtime_src.contains(forbidden),
            "runtime tap surface must stay minimal: {forbidden}"
        );
    }
}

#[test]
fn runtime_resolver_surface_is_decision_input_owner() {
    let runtime_src = runtime_source();
    let resolver_src = read("src/session/cluster/core/dynamic_resolvers.rs");

    assert!(
        runtime_src.contains("ResolverRef")
            && !runtime_src.contains("pub use crate::transport::context::ResolverAttrs;")
            && !runtime_src.contains("pub mod replay {"),
        "runtime must keep resolver state as the only public resolver input owner"
    );
    let resolver_root = runtime_src
        .split("pub mod resolver {")
        .nth(1)
        .and_then(|tail| tail.split("/// Wire payload codec surface.").next())
        .expect("runtime resolver surface must be followed by wire surface");
    {
        let required = "ResolverRef";
        assert!(
            resolver_root.contains(required),
            "runtime::resolver must keep the resolver root: {required}"
        );
    }
    assert!(
        resolver_src.contains("pub struct ResolverRef<'cfg, const RESOLVER_ID: u16")
            && resolver_src
                .contains("pub fn evaluate(self) -> Result<DecisionResolution, ResolverError>")
            && resolver_src.contains("This is for typed resolver owners")
            && resolver_src.contains("not commit route/session progress")
            && !resolver_src.contains("pub fn resolve_decision")
            && !resolver_src.contains("erase_resolver_id"),
        "ResolverRef must carry resolver id and expose only the typed resolver-combinator evaluate seam without a public erasure shortcut"
    );
    for forbidden in [
        "ResolverContext",
        "ContextId",
        "ContextValue",
        "ResolverInput",
        "ResolverSignals,",
        "ResolverSlot",
        "pub mod core",
        "pub mod replay",
        "ResolverAttrs",
    ] {
        assert!(
            !resolver_root.contains(forbidden),
            "runtime::resolver root must not expose lower-level replay metadata: {forbidden}"
        );
    }
    for forbidden in [
        "pub mod advanced {",
        "pub mod epf {",
        "crate::epf::",
        "resolver::epf",
    ] {
        assert!(
            !resolver_root.contains(forbidden),
            "runtime::resolver must not regrow forbidden or extra buckets: {forbidden}"
        );
    }
}

#[test]
fn dynamic_resolver_surface_uses_one_decision_resolver() {
    let cluster_src = cluster_core_source();
    let runtime_src = runtime_source();
    let readme_src = read("README.md");
    let decision_resolver_src = read("src/endpoint/kernel/core/decision_resolver/impls.rs");
    let collapsed_resolution = concat!("Dynamic", "Resolution");
    let generic_stateless_ctor = concat!("ResolverRef::", "from_fn");
    let generic_state_ctor = concat!("ResolverRef::", "from_state");

    for src in [&cluster_src, &runtime_src, &readme_src] {
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
            "dynamic resolver public SPI must keep the route decision item: {required}"
        );
    }

    for forbidden in [
        "pub enum LoopResolution",
        "pub enum RollResolution",
        "pub fn loop_fn",
        "pub fn loop_state",
        "pub fn roll_fn",
        "pub fn roll_state",
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
        !decision_resolver_src.contains("if meta.peer == ROLE"),
        "dynamic route decision must not omit resolver validation for local sends"
    );
}

#[test]
fn core_resolver_audit_has_no_in_crate_resolver_owner() {
    let resolver_audit = read("src/resolver_audit.rs");
    for forbidden in [
        "pub(crate) struct ResolverCtx",
        "pub(crate) struct HostSlots",
        "pub(crate) enum Action",
        "pub(crate) struct AbortInfo",
        "pub(crate) enum Trap",
    ] {
        assert!(
            !resolver_audit.contains(forbidden),
            "hibana core must not keep an in-crate resolver owner: {forbidden}"
        );
    }

    for path in [
        "src/rendezvous/port.rs",
        "src/rendezvous/core.rs",
        "src/endpoint/kernel/core.rs",
    ] {
        let src = read(path);
        for forbidden in ["run_resolver(", "resolver_mode_tag("] {
            assert!(
                !src.contains(forbidden),
                "hibana core must record resolver audit inputs without route authority: {path}: {forbidden}"
            );
        }
    }

    let authority = read("src/endpoint/kernel/authority.rs");
    for forbidden in [
        "RouteResolverDecision",
        "route_resolver_decision_from_action",
        concat!("Defer", "Source::Epf"),
    ] {
        assert!(
            !authority.contains(forbidden),
            "route authority must stay Ack | Resolver | Poll only: {forbidden}"
        );
    }
}

#[test]
fn transport_resolver_signal_surface_stays_minimal() {
    let transport_src = transport_source();
    let runtime_src = runtime_source();
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
        !runtime_src.contains("TransportSnapshotParts"),
        "runtime::transport must not re-export TransportSnapshotParts"
    );
    assert!(
        !runtime_src.contains("TransportSnapshot"),
        "runtime::transport must not re-export TransportSnapshot"
    );
    assert!(
        !readme_src.contains("TransportSnapshot"),
        "README must not publish the forbidden TransportSnapshot surface"
    );
    assert!(
        !transport_src.contains("fn resolver_attrs(&self)")
            && !transport_src.contains("pub trait TransportMetrics")
            && !transport_src.contains("type Metrics"),
        "Transport must not expose resolver input, metrics, or telemetry extra hooks"
    );
    assert!(
        !readme_src.contains("ResolverAttrs") && readme_src.contains("ResolverRef::decision_state"),
        "README must keep replay attrs out of the canonical path and describe resolver-state owned input"
    );
    for forbidden in [
        "ContextId",
        "ContextValue",
        "ResolverInput",
        "ResolverSignals",
        "ResolverAttrs",
        "pub mod core {",
        "pub mod replay {",
        "advanced::resolver",
    ] {
        assert!(
            !runtime_src.contains(forbidden),
            "resolver signal extension namespace must not leak through runtime: {forbidden}"
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
            "transport snapshot builder surface must stay forbidden: {forbidden}"
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
            !transport_src.contains(forbidden) && !runtime_src.contains(forbidden),
            "transport observation detail must stay accessor-only and non-literal: {forbidden}"
        );
    }
    assert!(
        !transport_src.contains("TransportEventKind")
            && !transport_src.contains("pub struct TransportEvent")
            && !runtime_src.contains("TransportEventKind")
            && !runtime_src.contains("TransportEvent"),
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
