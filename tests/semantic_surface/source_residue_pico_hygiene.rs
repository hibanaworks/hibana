use super::common::*;
use hibana::runtime::tap::TapEvent;
use std::{fs, path::PathBuf, process::Command};

fn named_struct_body<'a>(source: &'a str, name: &str) -> &'a str {
    let marker = format!("struct {name} {{");
    let tail = source
        .split(&marker)
        .nth(1)
        .unwrap_or_else(|| panic!("{name} struct must stay visible"));
    tail.split("\n}")
        .next()
        .unwrap_or_else(|| panic!("{name} struct body must stay visible"))
}

fn frame_header_impl(source: &str) -> &str {
    let impl_start = source
        .find("impl FrameHeader {")
        .expect("FrameHeader impl block");
    let tail = &source[impl_start..];
    let open = tail.find('{').expect("FrameHeader impl open");
    let mut depth = 0usize;
    for (idx, byte) in tail[open..].bytes().enumerate() {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return &tail[..open + idx + 1];
                }
            }
            _ => {}
        }
    }
    panic!("FrameHeader impl close");
}

fn g_project_body(source: &str) -> &str {
    let marker = "pub(crate) fn project<const ROLE: u8, Steps>";
    let start = source.find(marker).expect("g::project function");
    let tail = &source[start..];
    let open = tail.find('{').expect("g::project function open");
    let mut depth = 0usize;
    for (idx, byte) in tail[open..].bytes().enumerate() {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return &tail[..open + idx + 1];
                }
            }
            _ => {}
        }
    }
    panic!("g::project function close");
}

fn run_script(script: &str) {
    let root = PathBuf::from(option_env!("HIBANA_REPO_ROOT").unwrap_or(env!("CARGO_MANIFEST_DIR")));
    let output = Command::new("bash")
        .arg(root.join(script))
        .output()
        .unwrap_or_else(|err| panic!("run {script} failed: {err}"));
    assert!(
        output.status.success(),
        "{script} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn g_project_does_not_enumerate_role_projection_constructors() {
    let g = read("src/g.rs");
    let project = g_project_body(&g);
    assert!(
        project.contains("if ROLE >= ROLE_DOMAIN_SIZE")
            && project.contains("role_projection::role_projection_image_for::<ROLE, Steps>()"),
        "g::project must keep one const role-domain guard followed by direct projection"
    );

    for role in 0..16 {
        let forbidden = format!("{}{}{}", "role_projection_image_for::<", role, ", Steps>()");
        assert!(
            !project.contains(&forbidden),
            "g::project must not re-grow hand-written role dispatch: {forbidden}"
        );
    }
}

#[test]
fn production_sources_do_not_reintroduce_static_hygiene_residue() {
    let production = read_production_rs_tree("src");
    for forbidden in [
        "#[allow(dead_code)]",
        "legacy",
        "heuristic",
        "fallback",
        "infer",
        "absorbed",
        "keeps_waiting",
        "DiscardedAndPending",
        "mismatch must stay pending",
        "mismatch discard",
    ] {
        assert!(
            !production.contains(forbidden),
            "production source must not re-grow static hygiene residue: {forbidden}"
        );
    }

    let mut inline_in_attr_group = false;
    for line in production.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("#[") {
            if trimmed == "#[inline]" {
                assert!(
                    !inline_in_attr_group,
                    "production source must not duplicate #[inline] in one attribute group"
                );
                inline_in_attr_group = true;
            }
        } else if !trimmed.is_empty() {
            inline_in_attr_group = false;
        }
    }
}

#[test]
fn rendezvous_scratch_borrows_are_scoped_and_offer_progress_is_endpoint_resident() {
    let port = read("src/rendezvous/port.rs");
    let lane_port = read("src/endpoint/kernel/lane_port.rs");
    let public_ops = read("src/endpoint/kernel/public_ops.rs");
    let public_poll = read("src/endpoint/kernel/public_poll.rs");
    let layout = read("src/endpoint/kernel/layout.rs");
    let frontier_state = read("src/endpoint/kernel/frontier_state.rs");
    let transport = read("src/transport.rs");
    let production = read_production_rs_tree("src");
    let local_send = lane_port
        .find("pending.carrier == SendCarrier::Local")
        .expect("local send carrier branch");
    let payload_encode = lane_port
        .find("pending.payload.encode_into(scratch)")
        .expect("transport payload encoding");

    assert!(
        port.contains("pub(crate) struct ScratchLease<'r>")
            && port.contains("RendezvousAccessState::ScratchLease")
            && port.contains("pub(crate) fn try_scratch_lease(&self)")
            && public_poll.contains(".try_scratch_lease()")
            && public_ops.contains("fn retire_transport_handles(&mut self)")
            && public_ops.contains("retirement_port.require_access_barrier()")
            && port.contains("pub(crate) fn require_access_barrier(&self)")
            && port.contains("RendezvousAccessState::RegistryLease")
            && port.contains("RendezvousAccessState::ScratchLease")
            && port.contains("RendezvousAccessState::EndpointOperation")
            && port.contains("RendezvousAccessState::EndpointScratchLease"),
        "offer progress and external endpoint destructors must remain under a scoped rendezvous lease"
    );
    assert!(
        lane_port.contains("payload: RawSendPayload")
            && lane_port.contains("pending.payload.encode_into(scratch)")
            && lane_port.contains("pending.payload.encode_into(&mut unit_payload)")
            && local_send < payload_encode
            && !lane_port.contains("PendingSend<'r>")
            && !lane_port.contains("begin_send_outgoing")
            && transport.contains("must not retain the payload pointer")
            && transport.contains("whose address may differ"),
        "pending sends must retain the source payload, never a shared-scratch borrow"
    );
    assert!(
        layout.contains("frontier_visited_scopes: EndpointArenaSection")
            && frontier_state.contains("visited_scopes: *mut ScopeId")
            && !production.contains("OfferEntryTable")
            && !production.contains("global_frontier_scratch_state"),
        "poll-spanning offer state must be endpoint resident and derived from active lanes"
    );
}

#[test]
fn long_running_const_eval_allow_stays_capacity_regression_only() {
    let root = PathBuf::from(option_env!("HIBANA_REPO_ROOT").unwrap_or(env!("CARGO_MANIFEST_DIR")));
    let token = "allow(long_running_const_eval)";
    let allowed = root.join("tests/program_capacity_regression.rs");
    let mut offenders = Vec::new();

    fn collect_files(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
        if dir
            .file_name()
            .is_some_and(|name| name == "target" || name == ".git")
        {
            return;
        }
        for entry in
            fs::read_dir(dir).unwrap_or_else(|err| panic!("read {} failed: {err}", dir.display()))
        {
            let path = entry
                .unwrap_or_else(|err| panic!("read dir entry in {} failed: {err}", dir.display()))
                .path();
            if path.is_dir() {
                collect_files(&path, out);
            } else if path.extension().is_some_and(|ext| {
                matches!(
                    ext.to_str(),
                    Some("rs" | "sh" | "py" | "md" | "toml" | "yml" | "yaml")
                )
            }) {
                out.push(path);
            }
        }
    }

    let mut files = Vec::new();
    for dir in ["src", "tests", ".github"] {
        collect_files(&root.join(dir), &mut files);
    }
    files.sort();

    for path in files {
        if path
            .strip_prefix(&root)
            .map(|relative| {
                relative
                    == std::path::Path::new("tests/semantic_surface/source_residue_pico_hygiene.rs")
            })
            .unwrap_or(false)
        {
            continue;
        }
        let text = fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("read {} failed: {err}", path.display()));
        if !text.contains(token) {
            continue;
        }
        if path != allowed {
            offenders.push(path.display().to_string());
            continue;
        }
        assert!(
            text.lines()
                .take_while(|line| !line.contains(token))
                .any(|line| line
                    .contains("capacity regression intentionally pushes const evaluation")),
            "long_running_const_eval allow must carry the capacity-regression reason comment"
        );
    }

    assert!(
        offenders.is_empty(),
        "long_running_const_eval allow must stay limited to tests/program_capacity_regression.rs: {}",
        offenders.join(", ")
    );
}

#[test]
fn frame_header_has_no_u64_storage() {
    let transport = read("src/transport.rs");
    let header_impl = frame_header_impl(&transport);
    assert!(
        transport.contains("pub struct FrameHeader([u8; 8]);"),
        "FrameHeader must stay an eight-byte carrier observation"
    );
    assert!(
        !header_impl.contains("u64") && !header_impl.contains("1u64"),
        "FrameHeader impl must not re-grow u64 packing or extraction"
    );
}

#[test]
fn runtime_public_api_has_no_frame_header_u64_raw() {
    let transport = read("src/transport.rs");
    let allowlist = read(".github/allowlists/runtime-public-api.txt");
    let header_impl = frame_header_impl(&transport);
    assert!(
        header_impl.contains("pub const fn from_bytes(bytes: [u8; 8]) -> Self")
            && header_impl.contains("pub const fn bytes(self) -> [u8; 8]"),
        "FrameHeader public surface must stay byte-based"
    );
    for forbidden in ["pub const fn from_raw(", "pub const fn raw(", "raw_header"] {
        assert!(
            !header_impl.contains(forbidden),
            "runtime public API must not expose FrameHeader u64 raw surface: {forbidden}"
        );
    }
    for forbidden in ["FrameHeader::from_raw", "FrameHeader::raw", "raw_header"] {
        assert!(
            !allowlist.contains(forbidden),
            "runtime allowlist must not expose FrameHeader u64 raw surface: {forbidden}"
        );
    }
}

#[test]
fn tap_event_is_opaque_sixteen_byte_record() {
    let event = read("src/observe/event.rs");
    assert_eq!(
        core::mem::size_of::<TapEvent>(),
        16,
        "TapEvent must stay a 16-byte Pico-class diagnostic record"
    );
    assert!(
        event.contains("#[repr(transparent)]")
            && event.contains("pub struct TapEvent {\n    bytes: [u8; 16],\n}"),
        "TapEvent storage must stay an opaque [u8; 16] record"
    );
    for forbidden in [
        "pub ts:",
        "pub id:",
        "pub causal_key:",
        "pub arg0:",
        "pub arg1:",
        "arg2",
        "with_arg2",
        "pub const fn zero(",
        "pub const fn with_arg0",
        "pub const fn with_arg1",
        "pub const fn with_causal_key",
        "pub const fn make_causal_key",
        "impl WireEncode for TapEvent",
        "impl WirePayload for TapEvent",
        "encoded_len(",
        "require_exact_len(input.as_bytes().len(), 20)",
    ] {
        assert!(
            !event.contains(forbidden),
            "TapEvent must not re-grow the public-field 20-byte record shape: {forbidden}"
        );
    }
}

#[test]
fn tap_ring_bytes_stay_at_half_kib() {
    let consts = read("src/runtime_core/consts.rs");
    let observe = read("src/observe/core.rs");
    let runtime = read("src/runtime.rs");
    assert!(
        consts.contains("pub const TAP_EVENTS: usize = 32;"),
        "mandatory tap ring capacity must stay at 32 events"
    );
    assert!(
        core::mem::size_of::<[TapEvent; 32]>() == 512,
        "mandatory tap ring must stay exactly 512 bytes"
    );
    for forbidden in [
        "RING_BUFFER_SIZE",
        "USER_EVENT_RANGE_END",
        "TapBuffer",
        "tap_buffer",
        "TapCapacity",
        "tap_capacity",
        "TapConfig",
        "tap_config",
        "cfg(feature = \"std\")",
        "cfg(not(feature = \"std\"))",
        "cfg(target_arch",
    ] {
        assert!(
            !consts.contains(forbidden)
                && !observe.contains(forbidden)
                && !runtime.contains(forbidden),
            "tap must stay one fixed Pico-class evidence surface without public capacity/config or host split: {forbidden}"
        );
    }
}

#[test]
fn port_derives_endpoint_lease_count_from_the_live_root() {
    let port = read("src/rendezvous/port.rs");
    let rendezvous = read("src/rendezvous/core.rs");
    let resolver = read("src/session/cluster/core/dynamic_resolvers/bucket.rs");
    for forbidden in [
        "endpoint_leases: *const super::core::EndpointLeaseSlot",
        "endpoint_leases: *const EndpointLeaseSlot",
        "endpoint_leases: *const super::core::EndpointLeaseRecord",
        "endpoint_leases: *const EndpointLeaseRecord",
        "endpoint_lease_capacity: super::core::EndpointLeaseId,",
        "endpoint_lease_capacity: EndpointLeaseId,",
        "endpoint_lease_storage: *const Sidecar<EndpointLeaseSlot>",
        "endpoint_lease_capacity: *const EndpointLeaseId",
        "endpoint_lease_capacity: &'r Cell<EndpointLeaseId>",
        "endpoint_lease_capacity: &'tap Cell<EndpointLeaseId>",
    ] {
        assert!(
            !port.contains(forbidden),
            "Port must not snapshot endpoint lease table roots or capacity by value: {forbidden}"
        );
    }
    assert!(
        port.contains("endpoint_lease_storage: &'r Cell<Sidecar<EndpointLeaseRecord>>")
            && port.contains("fn endpoint_lease_owner_view(&self)")
            && port.contains("let storage = self.endpoint_lease_storage.get();")
            && port.contains("EndpointLeaseRecord::storage_slot_count(storage)"),
        "Port must reload the endpoint lease sidecar root and derive its slot count from exact bytes"
    );
    assert!(
        !rendezvous.contains("endpoint_lease_capacity:")
            && !named_struct_body(&resolver, "ResolverBucket<'cfg>").contains("capacity:"),
        "endpoint lease and resolver capacities must have one authority in Sidecar::bytes"
    );
}

#[test]
fn public_runtime_surface_does_not_accept_max_rv_budget_parameter() {
    let runtime = read("src/runtime/session_kit.rs");
    let allowlist = read(".github/allowlists/runtime-public-api.txt");
    for required in [
        "pub struct SessionKit<'cfg, T>",
        "pub struct SessionKitStorage<'cfg, T>",
        "Result<RendezvousKit<'_, 'cfg, T>, AttachError>",
    ] {
        assert!(
            runtime.contains(required) && allowlist.contains(required),
            "runtime surface must keep the single resident SessionKit shape: {required}"
        );
    }
    for forbidden in [
        "const MAX_RV",
        "MAX_RV>",
        "MAX_RV),",
        "MAX_RV>,",
        "SessionKitStorage::<T, N>",
        "caller-owned local rendezvous budget",
    ] {
        assert!(
            !runtime.contains(forbidden) && !allowlist.contains(forbidden),
            "public runtime surface must not re-grow caller-selected rendezvous capacity: {forbidden}"
        );
    }
}

#[test]
fn production_does_not_reintroduce_fixed_rendezvous_capacity() {
    let production = read_production_rs_tree("src");
    for forbidden in [
        "const MAX_RV",
        "MAX_RV == 4",
        "MAX_RV != 4",
        "MAX_RV > 4",
        "MAX_RV < 4",
        "RUNTIME_RENDEZVOUS_CAPACITY",
        "cluster_rendezvous_slot",
        "FREE_REGION_CAPACITY",
        "FreeRegion",
        "free_regions",
        "reclaim_delta",
        "MAX_FIRST_RECV_DISPATCH",
    ] {
        assert!(
            !production.contains(forbidden),
            "production code must not re-grow fixed rendezvous capacity: {forbidden}"
        );
    }
}

#[test]
fn packed_endpoint_handle_does_not_pack_rendezvous_or_slot() {
    let carrier = read("src/endpoint/carrier.rs");
    for required in [
        "pub(crate) struct PackedEndpointHandle(u32);",
        "pub(crate) const fn new(generation: u32) -> Self",
        "pub(crate) const fn generation(self) -> u32 {\n        self.0\n    }",
    ] {
        assert!(
            carrier.contains(required),
            "PackedEndpointHandle must stay a generation-only witness: {required}"
        );
    }
    for forbidden in [
        "pub(crate) struct PackedEndpointHandle(u64);",
        "RendezvousId",
        "EndpointLeaseId",
        "<< 32",
        "<< 16",
        ">> 32",
        "u16::from(slot)",
        "rv.raw()",
    ] {
        assert!(
            !carrier.contains(forbidden),
            "PackedEndpointHandle must not re-grow rv/slot/u64 packing: {forbidden}"
        );
    }
}

#[test]
fn transport_docs_do_not_teach_u64_header() {
    let docs = read("README.md");
    for forbidden in [
        "FrameHeader::from_raw",
        "raw_header",
        "u64 observation",
        "carrier-owned `u64`",
    ] {
        assert!(
            !docs.contains(forbidden),
            "transport docs must not teach u64 FrameHeader construction: {forbidden}"
        );
    }
    assert!(
        docs.contains("FrameHeader::from_bytes(header_bytes)"),
        "transport docs must teach byte-based FrameHeader construction"
    );
}

fn assert_frame_label_mask_byte_limb_storage(labels: &str) {
    let body = named_struct_body(labels, "FrameLabelMask");
    assert!(
        labels.contains("#[repr(transparent)]"),
        "FrameLabelMask must stay a transparent byte-limb wrapper"
    );
    assert_eq!(
        body.trim(),
        "limbs: [u8; 32],",
        "FrameLabelMask must stay a fixed [u8; 32] mask"
    );
    assert!(
        labels.contains("limbs[(frame_label >> 3) as usize]"),
        "FrameLabelMask indexing must stay byte-limb based"
    );
    assert!(
        labels.contains("1u8 << (frame_label & 7)"),
        "FrameLabelMask bit construction must stay u8 based"
    );
}

fn assert_frame_label_mask_ops_use_no_u64(labels: &str) {
    for forbidden in [
        "u64", "1u64", "[u64;", "word0", "word1", "word2", "word3", ">> 6", "<< 6", "* 64", "/ 64",
    ] {
        assert!(
            !labels.contains(forbidden),
            "FrameLabelMask must not re-grow wide integer storage or helpers: {forbidden}"
        );
    }
}

#[test]
fn frame_label_mask_has_no_u64_storage() {
    let labels = read("src/transport/labels.rs");
    assert_frame_label_mask_byte_limb_storage(&labels);
}

#[test]
fn frame_label_mask_ops_do_not_use_u64() {
    let labels = read("src/transport/labels.rs");
    assert_frame_label_mask_ops_use_no_u64(&labels);
}

#[test]
fn role_descriptor_rows_do_not_use_u64_hot_path_storage_or_helpers() {
    let dependency_facts = read("src/global/typestate/facts/dependency.rs");
    let image_types = read("src/global/role_program/image_types.rs");
    let lane_image = read("src/global/role_program/image_impl/lane_image.rs");
    let blob_image = read("src/global/role_program/image_impl/blob_image.rs");
    let program_columns = read("src/global/compiled/images/image/columns.rs");

    let dependency = named_struct_body(&dependency_facts, "PackedLocalDependency");
    let route_arm = named_struct_body(&image_types, "PackedRouteArmRow");
    let roll_scope = named_struct_body(&image_types, "PackedRollScopeRow");
    for required in [
        "start: u16",
        "end: u16",
        "dep_ordinal: u16",
        "conflict_route: u16",
    ] {
        assert!(
            dependency.contains(required),
            "dependency descriptor row must stay u16-limb based: {required}"
        );
    }
    for required in ["event_and_child: u32", "lane_step_row: PackedLaneRange"] {
        assert!(
            route_arm.contains(required),
            "route-arm descriptor row must stay u32/range based: {required}"
        );
    }
    let descriptor_hot_path = [
        dependency_facts.as_str(),
        image_types.as_str(),
        lane_image.as_str(),
        blob_image.as_str(),
        program_columns.as_str(),
    ]
    .join("\n");
    for required in [
        "ROLE_IMAGE_EVENT_STRIDE: usize = 10",
        "ROLE_IMAGE_ROUTE_SCOPE_STRIDE: usize = 2",
        "PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE: usize = 9",
    ] {
        assert!(
            descriptor_hot_path.contains(required),
            "scope descriptor rows must stay u16 ScopeId based: {required}"
        );
    }
    for required in ["scope: u16", "event_row: PackedLaneRange"] {
        assert!(
            roll_scope.contains(required),
            "roll-scope descriptor row must carry fixed-kind scope ordinal plus event range: {required}"
        );
    }
    for required in [
        "scope: scope.local_ordinal()",
        "ScopeId::roll_scope(self.scope)",
    ] {
        assert!(
            image_types.contains(required),
            "roll-scope descriptor row must preserve roll-scope identity without raw ScopeId storage: {required}"
        );
    }
    for forbidden in [
        "struct PackedLocalDependency(u64)",
        "struct PackedRouteArmRow(u64)",
        "struct PackedRollScopeRow(u64)",
        "const fn read_u64",
        "const fn write_u64",
        "const fn w64",
        "from_raw(raw: u64)",
        "raw(self) -> u64",
    ] {
        assert!(
            !descriptor_hot_path.contains(forbidden),
            "role descriptor hot path must not re-grow u64 storage/helpers: {forbidden}"
        );
    }
}

#[test]
fn scope_frame_label_meta_does_not_own_frame_label_masks() {
    let evidence = read("src/endpoint/kernel/evidence.rs");
    let meta = named_struct_body(&evidence, "ScopeFrameLabelMeta");
    let masks = named_struct_body(&evidence, "ScopeFrameLabelMasks");
    let scratch = named_struct_body(&evidence, "ScopeFrameLabelScratch");
    let view = named_struct_body(&evidence, "ScopeFrameLabelView<'a>");

    for required in [
        "recv_frame_label: u8",
        "recv_arm: u8",
        "controller_frame_labels: [u8; 2]",
        "flags: u8",
    ] {
        assert!(
            meta.contains(required),
            "ScopeFrameLabelMeta must keep only scalar route facts: {required}"
        );
    }
    for forbidden in [
        "FrameLabelMask",
        "ScopeFrameLabelMasks",
        "limbs",
        "[u8; 32]",
    ] {
        assert!(
            !meta.contains(forbidden),
            "ScopeFrameLabelMeta must not own frame-label masks: {forbidden}"
        );
    }
    assert!(
        scratch.contains("masks: ScopeFrameLabelMasks")
            && view.contains("masks: &'a ScopeFrameLabelMasks"),
        "mask ownership must stay confined to scratch and borrowed through ScopeFrameLabelView"
    );
    assert_eq!(
        masks.trim(),
        "pub(super) arm_frame_label_masks: [FrameLabelMask; 2],",
        "ScopeFrameLabelMasks must not re-grow duplicate per-arm mask sets"
    );
}

#[test]
fn scope_frame_label_scratch_is_not_copy() {
    let evidence = read("src/endpoint/kernel/evidence.rs");
    let scratch_prefix = evidence
        .split("pub(super) struct ScopeFrameLabelScratch")
        .next()
        .expect("evidence source");
    let scratch_attrs = scratch_prefix
        .rsplit("\n\n")
        .next()
        .expect("ScopeFrameLabelScratch attrs");
    assert!(
        !scratch_attrs.contains("Copy") && !scratch_attrs.contains("Clone"),
        "ScopeFrameLabelScratch must not be Clone/Copy"
    );
}

#[test]
fn scope_frame_label_masks_is_not_copy() {
    let evidence = read("src/endpoint/kernel/evidence.rs");
    let masks_prefix = evidence
        .split("pub(super) struct ScopeFrameLabelMasks")
        .next()
        .expect("evidence source");
    let masks_attrs = masks_prefix
        .rsplit("\n\n")
        .next()
        .expect("ScopeFrameLabelMasks attrs");
    assert!(
        !masks_attrs.contains("Copy") && !masks_attrs.contains("Clone"),
        "ScopeFrameLabelMasks must not be Clone/Copy"
    );
}

#[test]
fn no_by_value_scope_frame_label_meta_in_hot_paths() {
    let hot_path = [
        read("src/endpoint/kernel/core/frontier_helpers.rs"),
        read("src/endpoint/kernel/core/scope_evidence_logic.rs"),
        read("src/endpoint/kernel/offer.rs"),
        read("src/endpoint/kernel/offer/facts.rs"),
        read("src/endpoint/kernel/offer/passive.rs"),
        read("src/endpoint/kernel/offer/select.rs"),
    ]
    .join("\n");
    for forbidden in [
        "ScopeFrameLabelMasks::EMPTY",
        "frame_label_masks",
        "frame_label_meta: &ScopeFrameLabelMeta",
        ") -> ScopeFrameLabelMeta",
        ".frame_hint_mask(&",
        "fn selection_frame_label_meta(",
        "fn offer_scope_frame_label_meta(",
        "fn scope_frame_label_meta(",
        "fn scope_frame_label_meta_at(",
    ] {
        assert!(
            !hot_path.contains(forbidden),
            "scope frame-label hot path must use ScopeFrameLabelScratch/View, not by-value mask plumbing: {forbidden}"
        );
    }
    assert!(
        hot_path.contains("ScopeFrameLabelScratch::EMPTY")
            && hot_path.contains("ScopeFrameLabelView<'_>"),
        "offer hot paths must build scratch locally and pass ScopeFrameLabelView"
    );
}

#[test]
fn no_by_value_scope_frame_label_scratch_in_hot_paths() {
    let hot_path = [
        read("src/endpoint/kernel/core/frontier_helpers.rs"),
        read("src/endpoint/kernel/core/scope_evidence_logic.rs"),
        read("src/endpoint/kernel/offer.rs"),
        read("src/endpoint/kernel/offer/facts.rs"),
        read("src/endpoint/kernel/offer/passive.rs"),
        read("src/endpoint/kernel/offer/select.rs"),
    ]
    .join("\n");
    for forbidden in [
        ") -> ScopeFrameLabelScratch",
        "frame_label_scratch: ScopeFrameLabelScratch",
        "scratch: ScopeFrameLabelScratch",
        "ScopeFrameLabelMasks,",
    ] {
        assert!(
            !hot_path.contains(forbidden),
            "scope frame-label scratch must not move by value through hot path signatures: {forbidden}"
        );
    }
    assert!(
        hot_path.contains("&mut ScopeFrameLabelScratch")
            && hot_path.contains("ScopeFrameLabelView<'_>"),
        "hot paths must use mutable scratch and borrowed frame-label views"
    );
}

#[test]
fn production_evidence_source_under_file_budget() {
    assert!(
        read("src/endpoint/kernel/evidence.rs").lines().count() <= 300,
        "evidence.rs production owner must stay under 300 lines"
    );
}

#[test]
fn source_residue_forbidden_literals_are_checked_without_split_hiding() {
    run_script(".github/scripts/check_no_split_guard_literals.sh");
    let resolver_gate = read(".github/scripts/check_resolver_surface_hygiene.sh");
    assert!(
        resolver_gate.contains("--glob")
            && resolver_gate.contains("!tests/semantic_surface/source_residue_pico_hygiene.rs"),
        "resolver hygiene must use an explicit guard-file scope instead of split forbidden literals"
    );
    assert!(
        resolver_gate.contains("resolver_authority_deny_self_test")
            && resolver_gate.contains("resolver authority deny self-test missed token"),
        "resolver hygiene must fixture-test each forbidden authority token"
    );
}

#[test]
fn resolver_surface_hygiene_uses_explicit_guard_scope() {
    run_script(".github/scripts/check_resolver_surface_hygiene.sh");
}

#[test]
fn kernel_quarantine_extracts_poll_public_recv_by_braces() {
    let script = read(".github/scripts/check_kernel_monomorphization_quarantine.sh");
    assert!(
        script.contains("def extract_rust_function(")
            && script.contains("extract_rust_function(public_runtime, \"poll_public_recv\")"),
        "kernel quarantine must use brace-matched poll_public_recv extraction"
    );
}

#[test]
fn final_form_static_gates_run_without_budget_or_regex_false_failure() {
    run_script(".github/scripts/check_maintainability_budgets.sh");
    run_script(".github/scripts/check_resolver_surface_hygiene.sh");
    run_script(".github/scripts/check_kernel_monomorphization_quarantine.sh");
}
