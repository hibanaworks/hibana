use super::common::*;

fn named_struct_body<'a>(source: &'a str, name: &str) -> &'a str {
    let marker = format!("struct {name}");
    let visible_marker = format!("pub(crate) struct {name}");
    let tail = source
        .split(&marker)
        .nth(1)
        .or_else(|| source.split(&visible_marker).nth(1))
        .unwrap_or_else(|| panic!("{name} struct must stay visible"));
    tail.split_once('{')
        .unwrap_or_else(|| panic!("{name} struct body must start with an opening brace"))
        .1
        .split("\n}")
        .next()
        .unwrap_or_else(|| panic!("{name} struct body must stay visible"))
}

#[test]
fn production_and_gates_do_not_reintroduce_std_feature_branches() {
    let production = read_production_rs_tree("src");
    let readme = read("README.md");
    let gates = read_tree_except(
        ".github/scripts",
        &[".github/scripts/check_surface_hygiene.sh"],
    );
    let combined = [production.as_str(), readme.as_str(), gates.as_str()].join("\n");
    for forbidden in [
        "cfg(feature = \"std\")",
        "cfg(not(feature = \"std\"))",
        "features = [\"std\"]",
        "--features std",
        "std feature",
        "host diagnostics",
    ] {
        assert!(
            !combined.contains(forbidden),
            "production and gate surface must not reintroduce host cfg branching: {forbidden}"
        );
    }
    assert!(
        read("src/lib.rs").contains("#![no_std]")
            && !read("src/lib.rs").contains("cfg_attr(not(feature"),
        "crate root must be unconditionally no_std"
    );
    let surface_hygiene = read(".github/scripts/check_surface_hygiene.sh");
    assert!(
        surface_hygiene.contains("std feature")
            && surface_hygiene.contains("!.github/scripts/check_surface_hygiene.sh"),
        "surface hygiene must check host cfg wording with explicit self scope"
    );
}

#[test]
fn production_sources_do_not_reintroduce_transport_fragmentation_axis() {
    let production = read_production_rs_tree("src");
    for forbidden in ["FrameFlags", "flags: Frame", "FrameFlags::", "FrameFlags {"] {
        assert!(
            !production.contains(forbidden),
            "transport fragmentation vocabulary must not return to production source: {forbidden}"
        );
    }
    for line in production.lines() {
        for forbidden in ["FRAG", "IDX", "TOT"] {
            assert!(
                !line
                    .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
                    .any(|token| token == forbidden),
                "transport fragmentation token must not return to production source: {line}"
            );
        }
    }
    for forbidden in [
        "endpoint_resolver_args",
        "emit_endpoint_resolver_audit",
        "ResolverSlot::EndpointRx",
        "ResolverSlot::EndpointTx",
        "hash_tap_event",
        "emit_resolver_audit_replay",
        "EndpointRxAuditPlan",
        "publish_endpoint_rx_audit",
        "build_endpoint_rx_audit_plan",
    ] {
        assert!(
            !production.contains(forbidden),
            "endpoint resolver replay audit vocabulary must not return: {forbidden}"
        );
    }
}

#[test]
fn transport_surface_has_no_custom_error_axis() {
    let transport = read("src/transport.rs");
    let trait_body = transport
        .split("pub trait Transport")
        .nth(1)
        .expect("Transport trait must exist")
        .split("/// Observability helpers")
        .next()
        .expect("Transport trait must precede trace module");
    for forbidden in ["type Error", "Self::Error", "Into<TransportError>"] {
        assert!(
            !trait_body.contains(forbidden),
            "Transport trait must return compact TransportError directly: {forbidden}"
        );
    }

    let transport_boundary = [
        read("src/transport.rs"),
        read("src/endpoint/kernel/lane_port.rs"),
        read("src/rendezvous/port/recv_frame.rs"),
    ]
    .join("\n");
    for forbidden in ["Into<TransportError>", "map_err(Into::into)"] {
        assert!(
            !transport_boundary.contains(forbidden),
            "transport boundary must not keep custom-error erasure residue: {forbidden}"
        );
    }
}

#[test]
fn endpoint_lease_slot_is_session_role_authority() {
    let lease_core = read("src/session/lease/core.rs");
    let rendezvous_core = read("src/rendezvous/core.rs");
    let endpoint_lease = read("src/rendezvous/core/endpoint_leases.rs");
    let registry_ops = read("src/session/lease/core/registry_ops.rs");
    let cluster_ops = read("src/session/cluster/core/session_cluster_ops.rs");
    let endpoint_attach = read("src/session/cluster/core/endpoint_attach.rs");
    let endpoint_core = read("src/endpoint/kernel/core.rs");
    let production_scope = [
        lease_core.as_str(),
        rendezvous_core.as_str(),
        endpoint_lease.as_str(),
        registry_ops.as_str(),
        cluster_ops.as_str(),
        endpoint_attach.as_str(),
        endpoint_core.as_str(),
    ]
    .join("\n");

    let lease_slot = named_struct_body(&rendezvous_core, "EndpointLeaseSlot");
    assert!(
        lease_slot.contains("sid: SessionId,") && lease_slot.contains("role: u8,"),
        "endpoint lease slot must be the live session-role identity owner"
    );
    assert!(
        registry_ops.contains("allocate_endpoint_lease_for_session_role")
            && registry_ops.contains("has_live_endpoint_session_role(sid, role)")
            && endpoint_lease.contains("pub(crate) fn has_live_endpoint_session_role"),
        "endpoint allocation must scan live endpoint leases before claiming a slot"
    );
    let claim = cluster_ops
        .find("core.locals.allocate_endpoint_lease_for_session_role")
        .expect("endpoint storage owner must claim endpoint lease");
    let resident = cluster_ops
        .find("rv.ensure_endpoint_resident_budget(resident_budget)")
        .expect("endpoint storage owner must ensure resident route/frontier budget");
    let assoc = cluster_ops
        .find("rv.ensure_core_lane_storage_for_assoc_entries")
        .expect("endpoint storage owner must ensure lane association capacity");
    assert!(
        cluster_ops.contains("core.locals.allocate_endpoint_lease_for_session_role")
            && cluster_ops.contains("sid,\n                ROLE,")
            && endpoint_attach.contains("allocate_public_endpoint_storage_for_rv::<ROLE>")
            && endpoint_attach.contains("PublicEndpointStorageRequest")
            && endpoint_attach.contains("required_bytes: storage_layout.total_bytes")
            && endpoint_attach.contains("required_align: storage_layout.total_align")
            && registry_ops.find("has_live_endpoint_session_role(sid, role)")
                < registry_ops.find("rendezvous.allocate_endpoint_lease")
            && !registry_ops.contains("ensure_endpoint_resident_budget")
            && claim < resident
            && resident < assoc
            && !endpoint_core.contains("release_session_role_claim"),
        "attach/drop must claim endpoint lease before sidecar capacity growth and release only that lease"
    );

    for forbidden in [
        "ROLE_CLAIM_SLOTS",
        "role_claims",
        "SessionRoleClaim",
        "SessionRoleClaimKey",
        "claim_session_role",
        "release_session_role_claim",
        "RoleClaimError",
        "bind_session_role",
        "unbind_session_role",
        "SessionRoleBinding",
        "role_bindings",
        "RoleBindingError::AlreadyBound",
        "RoleBindingError",
        "binding.refs",
        "refs += 1",
        "refs -= 1",
    ] {
        assert!(
            !production_scope.contains(forbidden),
            "session-role ownership must not reintroduce claim/refcount residue: {forbidden}"
        );
    }
}

#[test]
fn scope_id_is_single_u16_identity_without_compact_shadow() {
    let scope = read("src/global/const_dsl/scope.rs");
    let const_dsl = read("src/global/const_dsl.rs");
    let program = read("src/global/compiled/images/program.rs");
    let route_resolvers = read("src/global/compiled/images/image/route_resolvers.rs");
    let blob_storage = read("src/global/compiled/images/image/blob_storage.rs");
    let commit = read("src/endpoint/kernel/core/runtime_types/commit.rs");
    let dynamic_resolvers = read("src/session/cluster/core/dynamic_resolvers.rs");
    let session_effects = read("src/session/cluster/core/session_effect_steps.rs");
    let route_table = read("src/rendezvous/tables/route_table.rs");
    let production_scope = [
        scope.as_str(),
        const_dsl.as_str(),
        program.as_str(),
        route_resolvers.as_str(),
        blob_storage.as_str(),
        commit.as_str(),
        dynamic_resolvers.as_str(),
        session_effects.as_str(),
        route_table.as_str(),
    ]
    .join("\n");

    let scope_body = named_struct_body(&scope, "ScopeId");
    assert!(
        scope_body.contains("raw: u16,")
            && scope.contains("const ABSENT_RAW: u16 = u16::MAX;")
            && scope.contains("const KIND_SHIFT: u16 = 13;")
            && scope.contains("const LOCAL_MASK: u16 = 0x1fff;")
            && scope.contains("pub(crate) const ORDINAL_CAPACITY: u16 = Self::LOCAL_MASK;"),
        "ScopeId must be a u16 sentinel with 3-bit kind and 13-bit local ordinal"
    );
    for required in [
        "scope: ScopeId,",
        "pub(crate) const fn scope(&self) -> ScopeId",
        "pub(crate) scope: crate::global::const_dsl::ScopeId",
        "scope: ScopeId,",
        "pub(crate) struct RouteFrame {\n    pub(crate) scope: ScopeId,",
    ] {
        assert!(
            production_scope.contains(required),
            "compiled/runtime scope owners must store ScopeId directly: {required}"
        );
    }
    for forbidden in [
        "struct ScopeId {\n    raw: u64",
        "CompactScopeId",
        "range_ordinal",
        "nest_ordinal",
        "canonical_raw",
        "KIND_SHIFT: u64",
        "scope.raw() as u32",
        "from_scope_id",
        "to_scope_id",
    ] {
        assert!(
            !production_scope.contains(forbidden),
            "scope identity must not reintroduce compact/u64/canonical residue: {forbidden}"
        );
    }
}

#[test]
fn route_resolver_authority_is_scope_keyed() {
    let const_dsl = read("src/global/const_dsl.rs");
    let eff_list = read("src/global/const_dsl/eff_list.rs");
    let source = read("src/g/source.rs");
    let seal = read("src/global/compiled/lowering/seal.rs");
    let blob_storage = read("src/global/compiled/images/image/blob_storage.rs");
    let program = read("src/global/compiled/images/program.rs");
    let program_ref = read("src/global/compiled/images/image/program_ref.rs");
    let dynamic_resolvers = read("src/session/cluster/core/dynamic_resolvers.rs");
    let dynamic_entry = named_struct_body(&dynamic_resolvers, "DynamicResolverEntry");
    let bucket = read("src/session/cluster/core/dynamic_resolvers/bucket.rs");
    let session_effects = read("src/session/cluster/core/session_effect_steps.rs");
    let production_scope = [
        const_dsl.as_str(),
        eff_list.as_str(),
        source.as_str(),
        seal.as_str(),
        blob_storage.as_str(),
        program.as_str(),
        program_ref.as_str(),
        dynamic_resolvers.as_str(),
        bucket.as_str(),
        session_effects.as_str(),
    ]
    .join("\n");

    for required in [
        "pub(crate) struct RouteResolverSite",
        "pub(crate) const fn new(scope: ScopeId, resolver_id: u16) -> Self",
        "pub(crate) const fn scope(&self) -> ScopeId",
        "pub(crate) struct DynamicResolverKey {\n    pub(crate) rv: RendezvousId,\n    pub(crate) scope: crate::global::const_dsl::ScopeId,",
        "pub(crate) const fn new(rv: RendezvousId, scope: crate::global::const_dsl::ScopeId) -> Self",
        "pub(crate) fn route_resolver_sites_for",
        "compiled.route_resolver_sites_for(RESOLVER)",
        "let site_scope = site.scope();",
        "DynamicResolverKey::new(",
        "site_scope)",
        "view.resolver_for_scope(route_scope)",
        "resolver_for_scope(&self, scope: ScopeId)",
    ] {
        assert!(
            production_scope.contains(required),
            "route resolver authority must stay scope-keyed: {required}"
        );
    }
    for forbidden in [
        "DynamicResolverSite",
        "dynamic_resolver_sites_for",
        "first_route_head_decision_resolver_id",
        "nested_non_resolver_enter",
        "resident_resolver_at(scope_start)",
        "ProjectionRouteResolverMismatch",
        "ProjectionRouteResolverAbsent",
        "RouteHead",
        "route_head",
        "RouteArmHead",
        "RouteDuplicateLabel",
        "pub(crate) controller_role",
        "marker.controller_role",
        "with_scope_controller",
        "with_scope_controller_role",
        "eff_index: EffIndex",
    ] {
        assert!(
            !production_scope.contains(forbidden),
            "route resolver authority must not regain arm-head or eff-index residue: {forbidden}"
        );
    }
    assert!(
        !dynamic_entry.contains("scope:"),
        "dynamic resolver entry must not duplicate the scope key owned by DynamicResolverKey/bucket"
    );
}

#[test]
fn role_lane_mask_stays_lane_indexed_projection_only() {
    let steps = read("src/global/steps.rs");
    assert!(
        steps.contains("struct RoleLaneMask {\n    lanes: [u16; ROLE_LANE_COUNT],\n}"),
        "RoleLaneMask must stay lane-indexed u16 storage"
    );
    for required in [
        "fn union(self, other: Self, active_span: u16) -> Self",
        "fn intersects(&self, other: &Self, active_span: u16) -> bool",
        "fn shift_lanes(self, offset: u16, active_span: u16) -> Self",
    ] {
        assert!(
            steps.contains(required),
            "RoleLaneMask ops must be bounded by active lane span: {required}"
        );
    }
    for forbidden in [
        "ROLE_LANE_WORDS",
        "words: [u64",
        "bits: [u64",
        "1u64 <<",
        "0..=u8::MAX",
        "while lane <= u8::MAX",
    ] {
        assert!(
            !steps.contains(forbidden),
            "RoleLaneMask must not re-grow flattened u64 or fixed full-lane scans: {forbidden}"
        );
    }

    let source = read("src/g/source.rs");
    for required in [
        "shift_lanes(self.lane_span, right.lane_span)",
        "intersects(&right_role_lane_mask, combined_lane_span)",
        "union(right_role_lane_mask, combined_lane_span)",
    ] {
        assert!(
            source.contains(required),
            "ProgramSourceData::par must pass the right/computed lane span: {required}"
        );
    }

    for (path, contents) in [
        ("src/runtime", read_production_rs_tree("src/runtime")),
        (
            "src/runtime_core",
            read_production_rs_tree("src/runtime_core"),
        ),
        ("src/endpoint", read_production_rs_tree("src/endpoint")),
        ("src/transport", read("src/transport.rs")),
        ("src/rendezvous", read_production_rs_tree("src/rendezvous")),
        ("src/session", read_production_rs_tree("src/session")),
    ] {
        assert!(
            !contents.contains("RoleLaneMask"),
            "RoleLaneMask must stay confined to projection layers, found in {path}"
        );
    }
}

#[test]
fn public_surface_scanner_covers_trait_associated_items_and_type_shape() {
    let g_allowlist = read(".github/allowlists/g-public-api.txt");
    let runtime_allowlist = read(".github/allowlists/runtime-public-api.txt");
    let scanner = read(".github/scripts/check_public_api_allowlists.py");

    for required in [
        "Message::LOGICAL_LABEL const LOGICAL_LABEL: u8;",
        "Message::Payload type Payload;",
    ] {
        assert!(
            g_allowlist.contains(required),
            "g public allowlist scanner must cover Message associated item: {required}"
        );
    }
    for required in [
        "Transport::Tx type Tx<'a>: 'a where Self: 'a;",
        "Transport::Rx type Rx<'a>: 'a where Self: 'a;",
        "Transport::open fn open<'a>(&'a self, port: PortOpen) -> (Self::Tx<'a>, Self::Rx<'a>);",
        "Transport::poll_send fn poll_send<'a, 'f>( &self, tx: &'a mut Self::Tx<'a>, outgoing: Outgoing<'f>, cx: &mut Context<'_>, ) -> Poll<Result<(), TransportError>> where 'a: 'f;",
        "Transport::cancel_send fn cancel_send<'a>(&self, tx: &'a mut Self::Tx<'a>);",
        "Transport::poll_recv fn poll_recv<'a>( &'a self, rx: &'a mut Self::Rx<'a>, cx: &mut Context<'_>, ) -> Poll<Result<ReceivedFrame<'a>, TransportError>>;",
        "Transport::requeue fn requeue<'a>(&self, rx: &mut Self::Rx<'a>) -> Result<(), TransportError>;",
        "WireEncode::encode_into fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError>;",
        "WirePayload::Decoded type Decoded<'a>;",
        "WirePayload::validate_payload fn validate_payload(input: Payload<'_>) -> Result<(), CodecError>;",
        "WirePayload::decode_validated_payload fn decode_validated_payload<'a>(input: Payload<'a>) -> Self::Decoded<'a>;",
        "WirePayload::decode_payload fn decode_payload<'a>(input: Payload<'a>) -> Result<Self::Decoded<'a>, CodecError> {",
    ] {
        assert!(
            runtime_allowlist.contains(required),
            "runtime public allowlist scanner must cover trait associated item: {required}"
        );
    }
    for required in [
        "DecisionArm::Left variant Left",
        "DecisionArm::Right variant Right",
        "TransportError::Offline variant Offline",
        "TransportError::Deadline variant Deadline",
        "TransportError::Capacity variant Capacity",
        "TransportError::Failed variant Failed",
        "CodecError::Truncated variant Truncated",
        "CodecError::Malformed variant Malformed",
    ] {
        assert!(
            runtime_allowlist.contains(required),
            "runtime public allowlist scanner must cover enum variant item: {required}"
        );
    }
    for forbidden in [
        "Message::Decoded",
        "WireEncode::encoded_len",
        "Transport::poll_flush",
        "WirePayload::zero_payload",
    ] {
        assert!(
            !g_allowlist.contains(forbidden) && !runtime_allowlist.contains(forbidden),
            "public allowlists must fail closed for removed trait item: {forbidden}"
        );
    }
    assert!(
        scanner.contains("trait_owner_at")
            && scanner.contains("is_trait_item_start")
            && scanner.contains("trait_item_name")
            && scanner.contains("collect_public_enum_shape")
            && scanner.contains("collect_public_struct_shape")
            && scanner.contains("def run_self_test()")
            && scanner.contains("FixtureEnum::Added variant Added")
            && scanner.contains("FixtureStruct::exposed pub field exposed: u8")
            && scanner.contains("FixtureTuple::0 pub field 0: u8")
            && scanner.contains("FixtureVariantFields::Struct.named field named: u8")
            && scanner.contains("parse_tuple_fields")
            && scanner.contains("parse_named_fields")
            && scanner.contains("src/global/message.rs")
            && scanner.contains("src/observe/event.rs")
            && scanner.contains("src/session/types.rs")
            && scanner.contains("src/transport/wire.rs"),
        "stable source scanner must include public trait items, enum variants, public fields, and re-export owner files"
    );
}

#[test]
fn route_site_tap_evidence_uses_local_ordinal_site() {
    let events = read("src/observe/events.rs");
    let select = read("src/endpoint/kernel/core/decision_resolver/impls/select.rs");
    let resolver = read("src/endpoint/kernel/core/decision_resolver/impls.rs");

    assert!(
        events.contains("const fn route_site(scope_id: ScopeId) -> u16")
            && events.contains("scope_id.local_ordinal()")
            && events.contains("((route_site(scope_id) as u32) << 16) | (arm as u32)")
            && events.contains("((route_site(scope_id) as u32) << 16) | (resolver_id as u32)")
            && events.contains("TapEvent::make_causal_key(lane, result)"),
        "tap event owner must pack route-site evidence from ScopeId::local_ordinal"
    );
    for forbidden in [
        "scope_id.raw() as u32",
        "((resolver_id as u32) << 16) | result",
        "ids::RESOLVER_AUDIT",
    ] {
        assert!(
            !select.contains(forbidden) && !resolver.contains(forbidden),
            "route-site tap evidence must not regain old packing: {forbidden}"
        );
    }
}

#[test]
fn tap_reader_surface_stays_minimal() {
    let event = read("src/observe/event.rs");
    let allowlist = read(".github/allowlists/runtime-public-api.txt");
    let tap_event_attrs = event
        .split("pub struct TapEvent")
        .next()
        .expect("TapEvent declaration must exist")
        .rsplit("#[derive")
        .next()
        .expect("TapEvent derive attributes must be visible");
    assert!(
        !tap_event_attrs.contains("Debug"),
        "TapEvent must not derive raw storage Debug"
    );
    assert!(
        event.contains("impl core::fmt::Debug for TapEvent"),
        "TapEvent Debug must stay semantic instead of exposing raw bytes"
    );
    for required in [
        "TapEvent::ts",
        "TapEvent::id",
        "TapEvent::causal_key",
        "TapEvent::arg0",
        "TapEvent::arg1",
        "TapEvent::evidence",
        "Evidence::kind",
        "Evidence::reason",
        "Evidence::input",
    ] {
        assert!(
            allowlist.contains(required),
            "runtime allowlist must include canonical tap reader: {required}"
        );
    }
    for forbidden in [
        "pub const fn causal_role",
        "pub const fn causal_seq",
        "pub const fn input_word",
        "TapEvent::causal_role",
        "TapEvent::causal_seq",
        "Evidence::input_word",
    ] {
        assert!(
            !event.contains(forbidden) && !allowlist.contains(forbidden),
            "tap derived convenience helper must not be public: {forbidden}"
        );
    }
}
