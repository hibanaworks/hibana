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
    parts
        .into_iter()
        .map(|part| {
            fs::read_to_string(&part)
                .unwrap_or_else(|err| panic!("read {} failed: {}", part.display(), err))
        })
        .collect::<Vec<_>>()
        .join("")
}

fn cluster_core_source() -> String {
    let mut source = read("src/session/cluster/core.rs");
    source.push_str(&read_dir_rs("src/session/cluster/core"));
    source
}

fn runtime_source() -> String {
    let mut source = read("src/runtime.rs");
    source.push_str(&read_dir_rs("src/runtime"));
    source
}

#[test]
fn sidecar_owner_single_authority() {
    let storage_layout = read("src/rendezvous/core/storage_layout.rs");
    assert!(
        storage_layout.contains("pub(crate) struct Sidecar<T>")
            && storage_layout.contains("ptr: *mut T")
            && storage_layout.contains("bytes: usize")
            && !storage_layout.contains("reclaim_delta"),
        "storage layout must keep the single sidecar owner as ptr/bytes only"
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
            "FreeRegion",
            "FREE_REGION_CAPACITY",
            "free_regions",
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
        .find("release(source_storage.cast())")
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
        .find(".init_replacement_storage(\n                    lease.ptr(),\n                    lane_base,\n                    target_lane_slots,\n                    target_assoc_slots,\n                );")
        .expect("assoc replacement must stage entries before release");
    let assoc_release_pos = capacity
        .find("self.release_sidecar(source_assoc);")
        .expect("assoc replacement must release the source sidecar");
    let assoc_commit_pos = capacity
        .find(".commit_storage(\n                    lease.ptr(),\n                    lane_base,\n                    target_lane_slots,\n                    target_assoc_slots,\n                );")
        .expect("assoc replacement must publish only after release");
    assert!(
        assoc_stage_pos < assoc_release_pos && assoc_release_pos < assoc_commit_pos,
        "assoc replacement must stage, release, then publish"
    );

    let route_stage_pos = capacity
        .find("self.routes.migrate_from_storage(")
        .expect("route replacement must stage entries before release");
    let route_release_pos = capacity
        .find("self.release_sidecar(source_route);")
        .expect("route replacement must release the source sidecar");
    let route_commit_pos = capacity
        .find("self.routes.rebind_from_storage(")
        .expect("route replacement must publish only after release");
    assert!(
        route_stage_pos < route_release_pos && route_release_pos < route_commit_pos,
        "route replacement must stage, release, then publish"
    );

    let assoc = read("src/rendezvous/association/storage.rs");
    assert!(
        assoc.contains("pub(in crate::rendezvous) unsafe fn init_replacement_storage")
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
    let runtime_allowlist = read(".github/allowlists/runtime-public-api.txt");
    assert!(
        !runtime_allowlist.contains("FrameHeader::new"),
        "public allowlist must not preserve FrameHeader::new"
    );
    assert!(
        !runtime_allowlist.contains("FrameHeader::from_raw")
            && !runtime_allowlist.contains("FrameHeader::raw"),
        "public allowlist must not teach FrameHeader u64 raw access"
    );
    let frame_label_source = read("src/transport/labels.rs");
    let frame_label_impl = frame_label_source
        .split("impl FrameLabel")
        .nth(1)
        .expect("FrameLabel impl must exist");
    assert!(
        frame_label_impl.contains("pub(crate) const fn new(raw: u8) -> Self")
            && frame_label_impl.contains("pub const fn raw(self) -> u8"),
        "FrameLabel must be a runtime-issued witness with public raw read only"
    );
    assert!(
        !runtime_allowlist.contains("FrameLabel::new"),
        "public allowlist must not preserve arbitrary FrameLabel construction"
    );
}

#[test]
fn localside_transport_seal() {
    let localside_sources = [
        "src/local.rs",
        "src/endpoint.rs",
        "src/endpoint/ops.rs",
        "src/endpoint/send.rs",
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
fn route_branch_public_methods_stay_minimal() {
    let allowlist = read(".github/allowlists/endpoint-public-api.txt");
    let methods = allowlist
        .lines()
        .filter_map(|line| line.strip_prefix("RouteBranch::"))
        .filter_map(|line| line.split_once(' ').map(|(name, _)| name))
        .collect::<Vec<_>>();
    assert_eq!(
        methods,
        ["label", "recv", "send"],
        "RouteBranch public methods must stay at label/recv/send only"
    );
    for forbidden in ["RouteBranch::decode", "pub fn decode<"] {
        assert!(
            !allowlist.contains(forbidden),
            "RouteBranch allowlist must not retain removed branch decode surface: {forbidden}"
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
    let owner_src = read("src/session/brand.rs");

    for forbidden in [
        "src/session/cap.rs",
        "src/session/cap",
        "src/session/cap/atomic_codecs.rs",
        "src/session/cap/resource_kinds.rs",
        "src/session/cap/mint/header.rs",
        "src/session/cap/mint/token.rs",
        "src/session/cap/mint/error.rs",
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
    let forbidden_control_token = "ControlToken";
    let forbidden_cap_header = "CapHeader";
    assert!(
        !owner_src.contains(forbidden_control_token)
            && !owner_src.contains(forbidden_cap_header)
            && !runtime_source().contains(forbidden_cap_header),
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
            && !runtime_src.contains("pub use crate::runtime_core::consts::TAP_EVENTS;")
            && !runtime_src
                .lines()
                .any(|line| line.trim() == "pub use crate::observe::core::TapEvent;")
            && !runtime_src.contains("CounterClock")
            && !runtime_src.contains("Clock")
            && !runtime_src.contains("RING_EVENTS")
            && !runtime_src.contains("TAP_EVENTS")
            && runtime_allowlist.contains("pub mod tap {")
            && runtime_allowlist
                .contains("pub use crate::observe::core::{Evidence, TapEvent, TapPort};")
            && !runtime_allowlist.contains("pub use crate::observe::core::TapEvent;")
            && !runtime_allowlist.contains("CounterClock")
            && !runtime_allowlist.contains("Clock")
            && !runtime_allowlist.contains("RING_EVENTS")
            && !runtime_allowlist.contains("TAP_EVENTS"),
        "runtime surface may expose tap diagnostics only under runtime::tap and must hide tap storage and clock resources"
    );

    for forbidden in [
        "TapBatch",
        "RawEvent",
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
fn runtime_entry_uses_direct_slab_without_public_config_wrapper() {
    let runtime = runtime_source();
    let runtime_buckets = read("src/runtime/buckets.rs");
    let session_kit = read("src/runtime/session_kit.rs");
    let runtime_allowlist = read(".github/allowlists/runtime-public-api.txt");
    let readme = read("README.md");
    let crate_docs = read("src/lib.rs");

    assert!(
        session_kit.contains("pub fn rendezvous(")
            && session_kit.contains("slab: &'cfg mut [u8],")
            && session_kit.contains("transport: T,")
            && runtime_allowlist.contains(
                "SessionKit::rendezvous pub fn rendezvous( &self, slab: &'cfg mut [u8], transport: T, ) -> Result<RendezvousKit<'_, 'cfg, T>, AttachError> {"
            ),
        "SessionKit::rendezvous must expose only direct slab + transport"
    );
    for forbidden in [
        "pub struct Config",
        "pub use crate::runtime_core::config::Config;",
        "Config::from_resources",
        "runtime::Config",
        "from_resources(",
    ] {
        assert!(
            !runtime.contains(forbidden)
                && !runtime_buckets.contains(forbidden)
                && !session_kit.contains(forbidden)
                && !runtime_allowlist.contains(forbidden)
                && !readme.contains(forbidden)
                && !crate_docs.contains(forbidden),
            "public runtime surface must not retain Config wrapper residue: {forbidden}"
        );
    }
}

#[test]
fn message_and_wire_codec_boundaries_stay_separated() {
    let message = read("src/global/message.rs");
    let g = read("src/g.rs");
    let wire = read("src/transport/wire.rs");
    let recv_kernel = [
        read("src/endpoint/futures.rs"),
        read("src/endpoint/kernel/core/runtime_types.rs"),
        read("src/endpoint/kernel/recv.rs"),
    ]
    .join("\n");
    let endpoint = [
        read("src/endpoint/send.rs"),
        read("src/endpoint/ops.rs"),
        read("src/endpoint/branch.rs"),
    ]
    .join("\n");

    assert!(
        message.contains("pub trait Message: seal::Sealed")
            && message.contains("const LOGICAL_LABEL: u8;")
            && message.contains("type Payload;")
            && message.contains(
                "impl<const LOGICAL_LABEL: u8, P> Message for crate::g::Msg<LOGICAL_LABEL, P>"
            )
            && g.contains(
                "pub struct Msg<const LOGICAL_LABEL: u8, Payload>(PhantomData<Payload>);"
            ),
        "Message must stay a sealed choreography descriptor with only label and payload"
    );
    for forbidden in "type Decoded|MessageRuntime|ENCODE_PAYLOAD|P: crate::transport::wire::WirePayload|WirePayload>::zero_payload|WirePayload>::decode_validated_payload".split('|') {
        assert!(
            !message.contains(forbidden),
            "Message/g::Msg must not regain codec obligations: {forbidden}"
        );
    }
    for forbidden in "fn encoded_len|zero_payload|pub trait WirePayload: WireEncode".split('|') {
        assert!(
            !wire.contains(forbidden),
            "wire public traits must not regain redundant encode/decode obligations: {forbidden}"
        );
    }
    for forbidden in [
        "ALLOWS_ZERO_LENGTH",
        "RecvPayloadMode",
        "RecvPayloadSource",
        "ZeroLength",
    ] {
        assert!(
            !wire.contains(forbidden) && !recv_kernel.contains(forbidden),
            "recv codec boundary must not regain zero-length bypass state: {forbidden}"
        );
    }
    assert!(
        endpoint.contains("M::Payload: WireEncode")
            && endpoint.contains("M::Payload: WirePayload")
            && endpoint.contains("<M::Payload as WirePayload>::Decoded<'e>")
            && !endpoint.contains("M::Decoded<'e>")
            && !endpoint.contains("MessageRuntime"),
        "endpoint operations must own codec bounds and decoded return types"
    );
}

#[test]
fn tap_surface_has_one_public_entry_and_internal_event_construction() {
    let session_kit = read("src/runtime/session_kit.rs");
    let event = read("src/observe/event.rs");
    let tap_impl = [
        read("src/observe/core.rs"),
        read("src/observe/ids.rs"),
        read("src/runtime_core/consts.rs"),
        read("src/rendezvous/core/access_port.rs"),
        read("src/session/cluster/effects.rs"),
    ]
    .join("\n");
    let runtime_buckets = read("src/runtime/buckets.rs");

    assert!(
        session_kit.contains("impl<'kit, 'cfg, T> RendezvousKit")
            && session_kit.contains("pub fn tap(&self) -> crate::runtime::tap::TapPort<'_>")
            && !session_kit.contains("SessionRendezvousKit")
            && !session_kit.contains("SessionRoleKit")
            && !session_kit.contains("RoleKit"),
        "tap must be entered through rendezvous-wide rv.tap() without fluent session/role witnesses"
    );
    for forbidden in "pub const fn new(|pub const fn zero(|pub const fn with_arg0|pub const fn with_arg1|pub const fn with_causal_key|pub const fn make_causal_key|pub const fn causal_role|pub const fn causal_seq|pub const fn input_word|impl WireEncode for TapEvent|impl WirePayload for TapEvent".split('|') {
        assert!(
            !event.contains(forbidden),
            "TapEvent generation must stay internal while public readers stay immutable: {forbidden}"
        );
    }
    let runtime_allowlist = read(".github/allowlists/runtime-public-api.txt");
    for required in [
        "pub struct SessionId(u32);",
        "SessionId::new pub const fn new(id: u32) -> Self",
        "pub struct TapEvent",
        "pub struct Evidence",
        "TapEvent::evidence pub const fn evidence(self) -> Evidence",
        "Evidence::input pub const fn input(self) -> [u32; 4]",
    ] {
        assert!(
            runtime_allowlist.contains(required),
            "runtime allowlist scanner must cover re-export owner item: {required}"
        );
    }
    for required in
        "LANE_ACQUIRE|LANE_RELEASE|ROUTE_ARM_SELECTION|RESOLVER_AUDIT|TRANSPORT_FAULT".split('|')
    {
        assert!(
            runtime_buckets.contains(required),
            "runtime::tap must expose canonical event identifiers: {required}"
        );
    }
    for forbidden in [
        "RING_BUFFER_SIZE",
        "USER_EVENT_RANGE_END",
        "lane_open_tap_event_id",
        "raw_event(self.now32(), 0x0100",
    ] {
        assert!(
            !tap_impl.contains(forbidden),
            "tap implementation must not regain split-ring or hidden event authority: {forbidden}"
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
            && resolver_src.contains("pub fn decide(self) -> Result<DecisionArm, ResolverError>")
            && resolver_src.contains("This is for typed resolver owners and resolver combinators")
            && resolver_src.contains("commit route/session progress")
            && !resolver_src.contains("pub fn resolve_decision")
            && !resolver_src.contains("pub fn evaluate")
            && !resolver_src.contains("erase_resolver_id"),
        "ResolverRef must carry resolver id and expose only the typed resolver-combinator decide seam without a public erasure shortcut"
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
    for src in [&cluster_src, &runtime_src, &readme_src] {
        assert!(
            !src.contains("DynamicResolution"),
            "dynamic resolver surface must use DecisionArm directly, not a generic DynamicResolution alias"
        );
        assert!(
            !src.contains("ResolverRef::from_fn")
                && !src.contains("ResolverRef::from_state")
                && !src.contains("ResolverRef::decision_fn"),
            "resolver constructor surface must stay on decision_state only"
        );
    }

    for required in [
        "pub enum DecisionArm",
        "pub fn decision_state",
        "Result<DecisionArm, ResolverError>",
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
        "pub fn decision_fn",
        "pub fn evaluate",
        "dispatch_decision_fn",
        "stateless:",
        "pub enum DecisionResolution",
        "DecisionResolution::Defer",
        "DecisionResolution::Arm",
        "pub fn decision_fn(resolver: fn(ResolverContext) -> DecisionResolutionOutcome)",
        "resolver: fn(&S, ResolverContext) -> DecisionResolutionOutcome,",
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
    assert!(
        !repo_path("src/resolver_audit.rs").exists(),
        "resolver audit replay owner must not return"
    );
    let production = [
        read("src/endpoint/kernel/core.rs"),
        read_dir_rs("src/endpoint/kernel/core"),
    ]
    .join("\n");
    for forbidden in "emit_endpoint_resolver_audit|endpoint_resolver_args|ResolverSlot::EndpointRx|ResolverSlot::EndpointTx|hash_tap_event|emit_resolver_audit_replay|EndpointRxAuditPlan|pub(crate) struct ResolverCtx|pub(crate) struct HostSlots|pub(crate) enum Action|pub(crate) struct AbortInfo|pub(crate) enum Trap".split('|') {
        assert!(
            !production.contains(forbidden),
            "hibana core must not keep resolver replay audit residue: {forbidden}"
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
        "Defer",
        "Deferred",
        "Source::Epf",
    ] {
        assert!(
            !authority.contains(forbidden),
            "route authority must stay Ack | Resolver | Poll only: {forbidden}"
        );
    }
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
