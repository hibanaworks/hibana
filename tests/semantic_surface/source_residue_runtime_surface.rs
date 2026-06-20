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

fn inline_always_function_span(source: &str, attr_start: usize) -> Option<usize> {
    let tail = &source[attr_start..];
    let brace_rel = tail.find('{')?;
    let body_start = attr_start + brace_rel;
    let mut depth = 0usize;
    let mut started = false;
    for (idx, ch) in source[body_start..].char_indices() {
        match ch {
            '{' => {
                depth += 1;
                started = true;
            }
            '}' => {
                depth = depth.checked_sub(1)?;
                if started && depth == 0 {
                    return Some(source[attr_start..=body_start + idx].lines().count());
                }
            }
            _ => {}
        }
    }
    None
}

#[test]
fn production_large_functions_do_not_force_inline_always() {
    const MAX_FORCED_INLINE_SPAN: usize = 24;
    for path in production_rs_files("src") {
        let source = read(&path);
        let mut offset = 0usize;
        while let Some(rel) = source[offset..].find("#[inline(always)]") {
            let attr_start = offset + rel;
            let span = inline_always_function_span(&source, attr_start)
                .unwrap_or_else(|| panic!("inline(always) function must parse in {path}"));
            assert!(
                span <= MAX_FORCED_INLINE_SPAN,
                "large production functions must not force inline(always): {path}:{line} spans {span} lines",
                line = source[..attr_start].lines().count() + 1
            );
            offset = attr_start + "#[inline(always)]".len();
        }
    }
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

    assert!(
        scope.contains("pub(crate) struct ScopeId(u16);")
            && scope.contains("const ABSENT_RAW: u16 = u16::MAX;")
            && scope.contains("const RESERVED_BIT: u16 = 0x8000;")
            && scope.contains("const KIND_SHIFT: u16 = 13;")
            && scope.contains("const LOCAL_MASK: u16 = 0x1fff;")
            && scope
                .contains("pub(crate) const LOCAL_CAPACITY: u16 = Self::MAX_LOCAL_ORDINAL + 1;"),
        "ScopeId must be a u16 sentinel with reserved/kind/local packed identity"
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
        "struct ScopeId {\n    raw: u32",
        "struct ScopeId {\n    raw: u64",
        "CompactScopeId",
        "canonical_raw",
        "new_with_parts",
        "range_ordinal",
        "nest_ordinal",
        "pub(crate) const fn ordinal(",
        "ScopeKind::Plain",
        "scope_kind:",
        "KIND_SHIFT: u64",
        "scope.raw() as u32",
        "from_raw(raw: u32)",
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
    let route_dsl = read("src/global/const_dsl/route.rs");
    let eff_list = read("src/global/const_dsl/eff_list.rs");
    let source = read("src/g/source.rs");
    let seal = read("src/global/compiled/lowering/seal.rs");
    let columns = read("src/global/compiled/images/image/columns.rs");
    let blob_storage = read("src/global/compiled/images/image/blob_storage.rs");
    let program = read("src/global/compiled/images/program.rs");
    let program_ref = read("src/global/compiled/images/image/program_ref.rs");
    let route_resolvers = read("src/global/compiled/images/image/route_resolvers.rs");
    let dynamic_resolvers = read("src/session/cluster/core/dynamic_resolvers.rs");
    let dynamic_entry = named_struct_body(&dynamic_resolvers, "DynamicResolverEntry");
    let route_site = named_struct_body(&program, "RouteResolverSite");
    let bucket = read("src/session/cluster/core/dynamic_resolvers/bucket.rs");
    let session_effects = read("src/session/cluster/core/session_effect_steps.rs");
    let production_scope = [
        const_dsl.as_str(),
        route_dsl.as_str(),
        eff_list.as_str(),
        source.as_str(),
        seal.as_str(),
        columns.as_str(),
        blob_storage.as_str(),
        program.as_str(),
        program_ref.as_str(),
        route_resolvers.as_str(),
        dynamic_resolvers.as_str(),
        bucket.as_str(),
        session_effects.as_str(),
    ]
    .join("\n");

    for required in [
        "pub(crate) struct RouteResolverSite",
        "pub(crate) const fn new(scope: ScopeId, resolver_id: u16) -> Self",
        "pub(crate) const fn scope(&self) -> ScopeId",
        "PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE: usize = 6",
        "self.write_u16(out, scope.raw());",
        "self.write_u16(out + 2, resolver_id);",
        "self.write_u8(out + 4,",
        "self.write_u8(out + 5, decision_tag);",
        "if ScopeId::from_raw(self.read_u16_at(offset)) == scope_id",
        "pub(crate) struct RouteResolverMarker",
        "pub(crate) scope: ScopeId,\n    pub(crate) resolver_id: u16,",
        "pub(crate) struct DynamicResolverKey {\n    pub(crate) rv: RendezvousId,\n    pub(crate) scope: crate::global::const_dsl::ScopeId,",
        "pub(crate) const fn new(rv: RendezvousId, scope: crate::global::const_dsl::ScopeId) -> Self",
        "pub(crate) fn route_resolver_sites_for",
        "compiled.route_resolver_sites_for(RESOLVER)",
        "let site_scope = site.scope();",
        "DynamicResolverKey::new(",
        "site_scope)",
        "eff_list.resolver_for_scope(route_scope)",
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
        "pub(crate) struct ResolverMarker",
        "offset: usize,\n    pub(crate) scope_id: ScopeId,\n    pub(crate) resolver: RouteResolver",
        "pub(crate) const fn resolver_at(",
        "pub(crate) const fn resolver_with_scope(",
        "PROGRAM_IMAGE_RESOLVER_STRIDE",
        "ProgramResolverRow",
        "resident_resolver_at",
        "first_visible_frontier",
        "collect_first_visible_frontier",
        "seen_lane_words",
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
        "PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE: usize = 8",
        "PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE: usize = 12",
        "read_u32_at",
        "write_u32",
        "pub(crate) struct RouteFrontierSummary",
        "push_route_frontier(route_summary)",
        "route_frontier_summaries(&self)",
        "view.route_frontier_summary(route_scope)",
    ] {
        assert!(
            !production_scope.contains(forbidden),
            "route resolver authority must not regain arm-head or eff-index residue: {forbidden}"
        );
    }
    assert_eq!(
        route_site.trim(),
        "scope: ScopeId,\n    resolver_id: u16,",
        "RouteResolverSite must be exactly scope + resolver_id"
    );
    assert!(
        !dynamic_entry.contains("scope:"),
        "dynamic resolver entry must not duplicate the scope key owned by DynamicResolverKey/bucket"
    );
}

#[test]
fn endpoint_selector_validation_stays_private_seal_scan_without_stored_summaries() {
    let g_core = read("src/g.rs");
    let source = read("src/g/source.rs");
    let const_dsl = read("src/global/const_dsl.rs");
    let endpoint_selectors = read("src/global/const_dsl/endpoint_selectors.rs");
    let frame_labels = read("src/global/frame_labels.rs");
    let scope_ranges = read("src/global/const_dsl/scope_ranges.rs");
    let eff_list = read("src/global/const_dsl/eff_list.rs");
    let route = read("src/global/const_dsl/route.rs");
    let seal = read("src/global/compiled/lowering/seal.rs");
    let lowering_driver = read("src/global/compiled/lowering/driver.rs");
    let lowering_image = read("src/global/compiled/lowering/driver/impls/image.rs");
    let role_image_impl = read("src/global/role_program/image_impl.rs");
    let role_event_rows = read("src/global/role_program/image_impl/event_rows.rs");
    let role_roll_rows = read("src/global/role_program/image_impl/roll_rows.rs");
    let role_projection = read("src/g/role_projection.rs");
    let combined = [
        g_core.as_str(),
        source.as_str(),
        const_dsl.as_str(),
        endpoint_selectors.as_str(),
        frame_labels.as_str(),
        scope_ranges.as_str(),
        eff_list.as_str(),
        route.as_str(),
        seal.as_str(),
        lowering_driver.as_str(),
        lowering_image.as_str(),
        role_image_impl.as_str(),
        role_event_rows.as_str(),
        role_roll_rows.as_str(),
        role_projection.as_str(),
    ]
    .join("\n");

    for required in [
        "ScopeEvent::Split",
        "let left_len = eff.len();",
        "push_parallel_scope_split(parallel_scope, left_len)",
        "pub(crate) const fn validate_parallel_endpoint_selectors(eff_list: &EffList) -> bool",
        "pub(crate) const fn validate_roll_reentry_endpoint_selectors(eff_list: &EffList) -> bool",
        "const fn parallel_endpoint_selector_conflicts(",
        "struct EndpointSelector(u32);",
        "EndpointSelector::inbound_evidence(",
        "const fn inbound_selector_at(",
        "atom_idx as u32",
        "pub(crate) const fn first_visible_endpoint_selector_conflicts_from_markers(",
        "pub(crate) const fn local_route_observer_paths_mergeable",
        "ProgramSourceError::ParallelAmbiguousEndpointSelector",
        "ProgramSourceError::ReentryAmbiguousEndpointSelector",
        "if !validate_parallel_endpoint_selectors(eff_list)",
        "if !validate_roll_reentry_endpoint_selectors(eff_list)",
        "if first_visible_endpoint_selector_conflicts_from_markers(",
        "if local_route_observer_paths_mergeable(",
        "while role < crate::g::ROLE_DOMAIN_SIZE",
        "validate_compiled_layout(role, eff_list)",
        "pub(crate) const fn parallel_arm_ranges_from_enter(",
        "const ROUTE_SCOPE_ORDINAL_BYTES: usize = MAX_COMPILED_IMAGE_NODES.div_ceil(8);",
        "let mut route_scope_ordinals = [0u8; ROUTE_SCOPE_ORDINAL_BYTES];",
        "let byte = ordinal >> 3;",
        "let bit = ordinal & 7;",
        "let mask = 1u8 << bit;",
        "const SOURCE: ProgramSourceData = <Steps as ProgramTerm>::PROGRAM_SOURCE;",
        "const SOURCE_EFF_LIST: &'static crate::global::const_dsl::EffList =",
        "let source = Self::SOURCE_EFF_LIST;",
    ] {
        assert!(
            combined.contains(required),
            "endpoint selector validation must keep public operation authority and u8 scope masks: {required}"
        );
    }
    for forbidden in [
        "validate_compiled_layout::<0>",
        "validate_compiled_layout::<1>",
        "validate_compiled_layout::<15>",
        "local_route_observer_paths_mergeable::<ROLE>",
        "const fn validate_compiled_layout<const ROLE: u8>",
        "RoleLaneScratch::from_program::<ROLE>",
        "local_step_range_for_eff_range::<ROLE>",
        "fill_dependency_rows::<ROLE>",
        "push_resident_rows::<ROLE>",
        "push_route_arm_lane_rows::<ROLE>",
        "push_roll_scope_rows::<ROLE>",
        "local_event_row_for_eff::<ROLE>",
        "role_lowering_counts::<ROLE>",
        "exact_resident_row_count_for_role",
        "validate_resident_row_capacity",
        "let source_data = <Steps as ProgramTerm>::PROGRAM_SOURCE;",
        "recv_frame_label_at(eff_list, atom_idx, atom)",
        "const fn recv_frame_label_at(",
        "frame_label_from_prior_count(count)",
    ] {
        assert!(
            !combined.contains(forbidden),
            "projection validation must not reintroduce const-generic all-role expansion: {forbidden}"
        );
    }

    let resolver_branch = seal
        .find("let has_dynamic_resolver = scope_has_dynamic_resolver")
        .expect("resolver branch must stay present");
    let intrinsic_branch = seal
        .find("if !has_dynamic_resolver")
        .expect("intrinsic route branch must stay present");
    let overlap_reject = seal
        .find("if first_visible_endpoint_selector_conflicts_from_markers(")
        .expect("intrinsic branch endpoint selector overlap rejection must stay present");
    assert!(
        resolver_branch < intrinsic_branch && intrinsic_branch < overlap_reject,
        "cross-branch endpoint selector overlap must be rejected only under intrinsic route authority"
    );

    for forbidden in [
        "struct LabelMask(",
        "duplicate_label",
        "ParallelDuplicateLabel",
        "has_duplicate_label",
        "branch_label_overlap",
        "has_branch_label_overlap",
        "label_words",
        "[u64; 4]",
        "route_scope_ordinals = [0u64",
        "1u64 <<",
        ">> 6",
        "<< 6",
        "/ 64",
        "% 64",
        "ops: [EndpointOpKey; eff::meta::MAX_EFF_NODES]",
        "[EndpointOpKey::EMPTY; eff::meta::MAX_EFF_NODES]",
        "struct EndpointOpFrontier",
        "EndpointOpKey",
        "OutboundOpMask",
        "ProjectedInboundKey",
        "LocalSig",
        "[LocalSig",
        "collect_local_sigs",
        "local_sequences_equal",
        "frontier: EndpointOpFrontier",
        "src/g/source/frontier.rs",
        "src/global/const_dsl/endpoint_ops.rs",
        "pub(crate) struct RouteFrontierSummary",
        "route_frontier_summaries",
        "push_route_frontier",
        "route_frontier_summary(",
        "has_ambiguous_endpoint_op",
        "has_intrinsic_branch_op_overlap",
        "struct EndpointOp(",
        "first_visible_endpoint_op_conflicts_from_markers",
        "nth_local_endpoint_op",
        "local_endpoint_op_count",
        "validate_parallel_endpoint_ops",
        "ParallelAmbiguousEndpointOp",
        "ReentryAmbiguousEndpointOp",
        "PublicEndpointSelector",
        "pub(crate) const fn nth_local_endpoint_selector",
        "pub(crate) const fn local_endpoint_selector_count",
        "previous.to == atom.to && previous.lane == atom.lane",
        "frame_key_targets",
        "frame_key_lanes",
        "frame_key_counts",
        "#[derive(Clone, Copy)]\npub(crate) struct ProgramSourceData",
        "#[derive(Clone, Copy)]\npub(crate) struct EffList",
        "#[derive(Clone, Copy)]\npub(crate) struct RoleLaneScratch",
        "#[derive(Clone, Copy)]\npub(crate) struct RoleImageBytes",
        "#[derive(Clone, Copy)]\npub(crate) struct ProgramImageBytes",
        "#[derive(Clone, Copy)]\npub(crate) struct FrameLabelAssigner",
        "pub(crate) struct FrameLabelScratch",
    ] {
        assert!(
            !combined.contains(forbidden),
            "projection selector validation must not re-grow stored frontier summaries: {forbidden}"
        );
    }
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
        "shift_lanes(left.lane_span, right.lane_span)",
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
