fn compact_ws(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut prev_space = false;
    for ch in input.chars() {
        if ch.is_whitespace() {
            if !prev_space {
                out.push(' ');
                prev_space = true;
            }
        } else {
            out.push(ch);
            prev_space = false;
        }
    }
    out
}

fn endpoint_kernel_source() -> String {
    [
        include_str!("../src/endpoint.rs"),
        include_str!("../src/endpoint/carrier.rs"),
        include_str!("../src/endpoint/kernel/mod.rs"),
        include_str!("../src/endpoint/kernel/authority.rs"),
        include_str!("../src/endpoint/kernel/control.rs"),
        include_str!("../src/endpoint/kernel/core.rs"),
        include_str!("../src/endpoint/kernel/endpoint_init.rs"),
        include_str!("../src/endpoint/kernel/frontier_observation.rs"),
        include_str!("../src/endpoint/kernel/frontier_select.rs"),
        include_str!("../src/endpoint/kernel/offer_refresh.rs"),
        include_str!("../src/endpoint/kernel/scope_evidence_logic.rs"),
        include_str!("../src/endpoint/kernel/decode.rs"),
        include_str!("../src/endpoint/kernel/evidence.rs"),
        include_str!("../src/endpoint/kernel/frontier.rs"),
        include_str!("../src/endpoint/kernel/inbox.rs"),
        include_str!("../src/endpoint/kernel/lane_port.rs"),
        include_str!("../src/endpoint/kernel/observe.rs"),
        include_str!("../src/endpoint/kernel/offer.rs"),
        include_str!("../src/endpoint/kernel/recv.rs"),
        include_str!("../src/endpoint/kernel/send.rs"),
    ]
    .join("\n")
}

fn strip_cfg_test_modules(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut cursor = 0usize;
    while let Some(rel_idx) = src[cursor..].find("#[cfg(test)]\nmod ") {
        let start = cursor + rel_idx;
        out.push_str(&src[cursor..start]);
        let mod_start = start + "#[cfg(test)]\n".len();
        let open_brace = src[mod_start..]
            .find('{')
            .map(|idx| mod_start + idx)
            .expect("cfg(test) module opening brace");
        let mut depth = 0usize;
        let mut end = None;
        for (offset, ch) in src[open_brace..].char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(open_brace + offset + 1);
                        break;
                    }
                }
                _ => {}
            }
        }
        cursor = end.expect("cfg(test) module closing brace");
    }
    out.push_str(&src[cursor..]);
    out
}

fn substrate_public_api_allowlist() -> &'static str {
    include_str!("../.github/allowlists/substrate-public-api.txt")
}

fn endpoint_public_api_allowlist() -> &'static str {
    include_str!("../.github/allowlists/endpoint-public-api.txt")
}

#[test]
fn sync_shim_does_not_return() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let sync_path = manifest_dir.join("src/sync.rs");
    assert!(
        !sync_path.exists(),
        "runtime fake-atomic shim must stay deleted: {}",
        sync_path.display()
    );

    let lib_src = include_str!("../src/lib.rs");
    let substrate_src = include_str!("../src/substrate.rs");
    let endpoint_src = include_str!("../src/endpoint.rs");
    let runtime_locality_src = [
        include_str!("../src/rendezvous/core.rs"),
        include_str!("../src/epf/host.rs"),
        include_str!("../src/observe/core.rs"),
    ]
    .join("\n");

    assert!(
        !lib_src.contains("mod sync;"),
        "crate root must not wire the deleted sync shim back in"
    );
    assert!(
        substrate_src.contains("LocalOnly")
            && endpoint_src.contains("LocalOnly")
            && runtime_locality_src.contains("Cell<"),
        "runtime owners must stay local-only and cell-backed"
    );
    for forbidden in [
        String::from("crate::sync"),
        ["Atom", "icBool"].concat(),
        ["Atom", "icU16"].concat(),
        ["Atom", "icU32"].concat(),
        ["Atom", "icUsize"].concat(),
        ["Atom", "icPtr"].concat(),
    ] {
        assert!(
            !runtime_locality_src.contains(&forbidden),
            "production runtime must not reintroduce fake sync or atomics: {forbidden}"
        );
    }
}

#[test]
fn ambient_stack_tuning_does_not_return() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let crate_cargo_config = manifest_dir.join(".cargo/config.toml");
    let workspace_cargo_config = manifest_dir
        .parent()
        .expect("workspace root")
        .join(".cargo/config.toml");
    assert!(
        !crate_cargo_config.exists(),
        "hibana must not rely on repo-local stack env defaults"
    );
    assert!(
        !workspace_cargo_config.exists(),
        "hibana must not rely on workspace-level stack env defaults"
    );

    let stack_sensitive_ws = compact_ws(
        &[
            include_str!("../src/control/lease/graph.rs"),
            include_str!("../src/control/lease/bundle.rs"),
            include_str!("../src/control/cluster/core.rs"),
            include_str!("../src/rendezvous/core.rs"),
            include_str!("../tests/route_dynamic_control.rs"),
        ]
        .join("\n"),
    );

    for forbidden in [
        concat!("RUST_MIN_", "STACK"),
        concat!("HIBANA_TEST_", "STACK"),
        concat!("GRAPH_MAX_", "NODES"),
        concat!("GRAPH_MAX_", "CHILDREN"),
        "Rendezvous::from_config(",
    ] {
        assert!(
            !stack_sensitive_ws.contains(forbidden),
            "ambient stack tuning and global LeaseGraph caps must not return: {forbidden}"
        );
    }
}

#[test]
fn endpoint_internal_carrier_stays_crate_private() {
    let endpoint_src = include_str!("../src/endpoint.rs");
    let flow_src = include_str!("../src/endpoint/flow.rs");
    let carrier_src = include_str!("../src/endpoint/carrier.rs");
    let runtime_mgmt_src = include_str!("../src/runtime/mgmt.rs");
    let substrate_src = include_str!("../src/substrate.rs");

    assert!(
        endpoint_src.contains("pub(crate) mod carrier;"),
        "endpoint root must keep the crate-private carrier owner module"
    );
    for required in [
        "carrier::EndpointCfg",
        "carrier::KernelCursorEndpoint",
        "carrier::KernelRouteBranch",
    ] {
        assert!(
            endpoint_src.contains(required) || flow_src.contains(required),
            "endpoint facade must route internal cursor aliases through carrier owner: {required}"
        );
    }
    assert!(
        substrate_src.contains("type KernelSessionCluster<'cfg, T, U, C, const MAX_RV: usize> =")
            || runtime_mgmt_src
                .contains("type KernelSessionCluster<'cfg, T, U, C, const MAX_RV: usize> ="),
        "runtime/substrate lower layer must keep the internal session-cluster owner alias"
    );
    assert!(
        !runtime_mgmt_src.contains("endpoint::carrier::PublicEndpoint")
            && !substrate_src.contains("endpoint::carrier::PublicEndpoint"),
        "runtime/substrate root surface must not regrow direct endpoint-carrier aliases"
    );
    for forbidden in [
        "pub struct SessionCfg",
        "pub struct EndpointCfg",
        "pub trait SessionCarrier",
        "pub trait EndpointCarrier",
    ] {
        assert!(
            !carrier_src.contains(forbidden),
            "carrier owner must stay crate-private: {forbidden}"
        );
    }

    let public_api_ws = [
        endpoint_public_api_allowlist(),
        substrate_public_api_allowlist(),
    ]
    .join("\n");
    for forbidden in [
        "SessionCfg",
        "EndpointCfg",
        "SessionCarrier",
        "EndpointCarrier",
    ] {
        assert!(
            !public_api_ws.contains(forbidden),
            "carrier internals must not leak into the public allowlists: {forbidden}"
        );
    }
}

#[test]
fn dead_forward_hub_does_not_return() {
    let transport_src = include_str!("../src/transport.rs");
    let rendezvous_core_src = include_str!("../src/rendezvous/core.rs");
    let frame_src = include_str!("../src/control/handle/frame.rs");
    let observe_core_src = include_str!("../src/observe/core.rs");
    let observe_ids_src = include_str!("../src/observe/ids.rs");
    let observe_events_src = include_str!("../src/observe/events.rs");
    let observe_check_src = include_str!("../src/observe/check.rs");
    let observe_normalise_src = include_str!("../src/observe/normalise.rs");

    assert!(
        !transport_src.contains("pub(crate) mod forward;"),
        "transport root must not keep the dead forward helper module"
    );
    assert!(
        !rendezvous_core_src.contains("pub fn with_forward<"),
        "Rendezvous must not keep the dead with_forward helper surface"
    );
    assert!(
        !rendezvous_core_src.contains("transport::forward::Forward"),
        "rendezvous core must not depend on the deleted forward helper owner"
    );
    assert!(
        !frame_src.contains("transport::forward"),
        "control-frame docs must not teach deleted transport::forward integration"
    );
    assert!(
        !observe_core_src.contains("Forward::relay()")
            && !observe_core_src.contains("Forward::splice()"),
        "observe docs must not reference deleted Forward helper entrypoints"
    );
    for forbidden in [
        "Forwarder boundary",
        "relay → splice transition",
        "forwarding switched to splice mode",
        "RELAY_FORWARD",
        "RelayForward",
        "ForwardEvent",
        "forward_trace(",
        "forward_equivalent",
    ] {
        assert!(
            !observe_ids_src.contains(forbidden)
                && !observe_events_src.contains(forbidden)
                && !observe_check_src.contains(forbidden)
                && !observe_normalise_src.contains(forbidden),
            "observe subtree must not keep dead relay/forward residue: {forbidden}"
        );
    }
}

#[test]
fn transport_trait_does_not_keep_default_fallback_hooks() {
    let transport_src = include_str!("../src/transport.rs");
    let transport_ws = compact_ws(transport_src);

    for required in [
        "fn requeue<'a>(&'a self, rx: &'a mut Self::Rx<'a>);",
        "fn drain_events(&self, emit: &mut dyn FnMut(TransportEvent));",
        "fn recv_label_hint<'a>(&'a self, rx: &'a Self::Rx<'a>) -> Option<u8>;",
        "fn metrics(&self) -> Self::Metrics;",
        "fn apply_pacing_update(&self, interval_us: u32, burst_bytes: u16);",
    ] {
        assert!(
            transport_ws.contains(required),
            "transport trait must require explicit owner implementations: {required}"
        );
    }

    assert!(
        transport_ws.contains("pub trait TransportMetrics {")
            && transport_ws.contains("fn snapshot(&self) -> TransportSnapshot;"),
        "transport metrics trait must collapse to an explicit snapshot owner"
    );
    assert!(
        transport_ws.contains("impl TransportMetrics for () {")
            && transport_ws.contains("TransportSnapshot::new(None, None)"),
        "unit should be the only zero-observation transport metrics owner"
    );

    for forbidden in [
        "debug_assert!(core::ptr::eq(rx, rx));",
        "Self::Metrics::default()",
        "pub struct NoopMetrics;",
        "pub trait TransportMetrics: Default",
        "fn latency_us(&self) -> Option<u64> {",
        "fn queue_depth(&self) -> Option<u32> {",
        "fn pacing_interval_us(&self) -> Option<u64> {",
        "fn congestion_marks(&self) -> Option<u32> {",
        "fn retransmissions(&self) -> Option<u32> {",
        "fn pto_count(&self) -> Option<u32> {",
        "fn srtt_us(&self) -> Option<u64> {",
        "fn latest_ack_pn(&self) -> Option<u64> {",
        "fn congestion_window(&self) -> Option<u64> {",
        "fn in_flight_bytes(&self) -> Option<u64> {",
        "fn algorithm(&self) -> Option<TransportAlgorithm> {",
        "fn drain_events(&self, _emit: &mut dyn FnMut(TransportEvent)) {}",
        "async fn quiesce_and_fence",
        "async fn resume_after",
    ] {
        assert!(
            !transport_src.contains(forbidden),
            "transport trait must not keep fallback hooks or dead splice seam: {forbidden}"
        );
    }
}

#[test]
fn policy_signals_provider_does_not_keep_zero_fallback_body() {
    let context_src = include_str!("../src/transport/context.rs");
    let context_ws = compact_ws(context_src);

    assert!(
        context_src.contains("use crate::substrate::policy::epf::Slot;"),
        "policy signals provider must import Slot from the canonical public owner"
    );
    assert!(
        context_ws.contains("pub trait PolicySignalsProvider {")
            && context_ws.contains("fn signals(&self, slot: Slot) -> PolicySignals;"),
        "policy signals provider must require an explicit owner implementation"
    );
    assert!(
        !context_ws.contains(
            "pub trait PolicySignalsProvider { fn signals(&self, _slot: Slot) -> PolicySignals { PolicySignals::ZERO } }"
        ),
        "policy signals provider must not keep a zero fallback body"
    );
    assert!(
        context_ws.contains("pub const FALSE: Self = Self(0);")
            && context_ws.contains("pub const TRUE: Self = Self(1);"),
        "ContextValue must expose explicit boolean constants instead of a bool-taking constructor"
    );
    assert!(
        !context_ws.contains("pub const fn from_bool(v: bool) -> Self"),
        "ContextValue must not keep a public bool-taking constructor"
    );
}

#[test]
fn control_kernel_uses_canonical_shot_marker_names() {
    let control_src = include_str!("../src/control.rs");
    let control_types_src = include_str!("../src/control/types.rs");
    let txn_src = include_str!("../src/control/automaton/txn.rs");

    for forbidden in [
        "trait OneShot",
        "impl OneShot for One",
        "S: OneShot",
        "**OneShot**",
    ] {
        assert!(
            !control_src.contains(forbidden)
                && !control_types_src.contains(forbidden)
                && !txn_src.contains(forbidden),
            "control kernel must not hide the canonical One marker behind an internal synonym: {forbidden}"
        );
    }

    assert!(
        txn_src.contains("impl<Inv: AtMostOnceCommit> InAcked<Inv, One> {"),
        "single-use transaction commit path must bind directly to the canonical One marker"
    );
}

#[test]
fn core_traits_do_not_keep_default_helper_bodies() {
    let cap_src = include_str!("../src/control/cap.rs");
    let cap_ws = compact_ws(cap_src);
    let mint_src = include_str!("../src/control/cap/mint.rs");
    let mint_ws = compact_ws(mint_src);
    let lease_graph_src = include_str!("../src/control/lease/graph.rs");
    let lease_graph_ws = compact_ws(lease_graph_src);
    let lease_planner_src = include_str!("../src/control/lease/planner.rs");
    let lease_planner_ws = compact_ws(lease_planner_src);
    let global_src = include_str!("../src/global.rs");
    let global_ws = compact_ws(global_src);

    assert!(
        cap_ws.contains("pub trait ControlHandle: Copy + Send + Sync + 'static {")
            && cap_ws
                .contains("fn visit_delegation_links(&self, f: &mut dyn FnMut(RendezvousId));"),
        "control handle trait must require explicit delegation-link ownership"
    );
    assert!(
        mint_ws.contains("pub trait MintConfigMarker: Copy {")
            && mint_ws.contains("fn as_config(&self) -> MintConfig<Self::Spec, Self::Policy>;"),
        "mint config marker must require an explicit owner conversion"
    );
    assert!(
        mint_ws.contains("pub trait SessionScopedKind: ResourceKind {")
            && mint_ws
                .contains("fn handle_for_session(sid: SessionId, lane: Lane) -> Self::Handle;")
            && mint_ws.contains("fn shot() -> CapShot;"),
        "session-scoped kind must require an explicit shot owner"
    );
    assert!(
        mint_ws.contains("pub trait ResourceKind {")
            && mint_ws.contains("const AUTO_MINT_EXTERNAL: bool;")
            && mint_ws.contains("fn caps_mask(handle: &Self::Handle) -> CapsMask;")
            && mint_ws.contains("fn scope_id(handle: &Self::Handle) -> Option<ScopeId>;"),
        "resource kind must require explicit owner-side mint/caps/scope hooks"
    );
    assert!(
        global_ws.contains("pub trait SendableLabel {")
            && global_ws.contains("const LABEL: u8;")
            && global_ws.contains("fn assert_sendable();"),
        "sendable label must require an explicit owner-side assertion hook"
    );
    assert!(
        lease_graph_ws.contains("pub(crate) trait LeaseFacet: Copy + Default {")
            && lease_graph_ws
                .contains("fn on_commit<'ctx>(&self, context: &mut Self::Context<'ctx>);")
            && lease_graph_ws
                .contains("fn on_rollback<'ctx>(&self, context: &mut Self::Context<'ctx>);"),
        "lease facet must require explicit owner-side commit/rollback hooks"
    );
    assert!(
        !lease_planner_ws.contains("trait LeaseSpecFacetNeeds"),
        "lease planner must not keep the removed LeaseSpecFacetNeeds compatibility seam"
    );

    for forbidden in [
        "pub trait ControlHandle: Copy + Send + Sync + 'static { fn visit_delegation_links(&self, _f: &mut dyn FnMut(RendezvousId)) {} }",
        "pub trait MintConfigMarker: Copy { type Spec: CapMintSpec; type Policy: CapMintPolicy; const INSTANCE: Self; fn as_config(&self) -> MintConfig<Self::Spec, Self::Policy> { MintConfig::<Self::Spec, Self::Policy>::new() } }",
        "pub trait SessionScopedKind: ResourceKind { fn handle_for_session(sid: SessionId, lane: Lane) -> Self::Handle; fn shot() -> CapShot { CapShot::One } }",
        "pub trait ResourceKind { type Handle: super::ControlHandle; const TAG: u8; const NAME: &'static str; const AUTO_MINT_EXTERNAL: bool = false;",
        "pub trait ResourceKind { type Handle: super::ControlHandle; const TAG: u8; const NAME: &'static str; const AUTO_MINT_EXTERNAL: bool; fn encode_handle(handle: &Self::Handle) -> [u8; CAP_HANDLE_LEN]; fn decode_handle(data: [u8; CAP_HANDLE_LEN]) -> Result<Self::Handle, CapError>; fn zeroize(handle: &mut Self::Handle); fn caps_mask(_handle: &Self::Handle) -> CapsMask { CapsMask::empty() }",
        "pub trait ResourceKind { type Handle: super::ControlHandle; const TAG: u8; const NAME: &'static str; const AUTO_MINT_EXTERNAL: bool; fn encode_handle(handle: &Self::Handle) -> [u8; CAP_HANDLE_LEN]; fn decode_handle(data: [u8; CAP_HANDLE_LEN]) -> Result<Self::Handle, CapError>; fn zeroize(handle: &mut Self::Handle); fn caps_mask(handle: &Self::Handle) -> CapsMask; fn scope_id(_handle: &Self::Handle) -> Option<ScopeId> { None }",
        "pub trait SendableLabel { const LABEL: u8; fn assert_sendable() { // Future work: enforce crash/no-send invariants here. } }",
        "pub(crate) trait LeaseFacet: Copy + Default { type Context<'ctx>; fn on_commit<'ctx>(&self, _context: &mut Self::Context<'ctx>) {}",
        "pub(crate) trait LeaseFacet: Copy + Default { type Context<'ctx>; fn on_commit<'ctx>(&self, context: &mut Self::Context<'ctx>); fn on_rollback<'ctx>(&self, _context: &mut Self::Context<'ctx>) {} }",
        "pub(crate) trait LeaseSpecFacetNeeds {",
    ] {
        assert!(
            !cap_ws.contains(forbidden)
                && !mint_ws.contains(forbidden)
                && !lease_graph_ws.contains(forbidden)
                && !lease_planner_ws.contains(forbidden)
                && !global_ws.contains(forbidden),
            "core traits must not keep default helper bodies: {forbidden}"
        );
    }
}

#[test]
fn wire_encode_trait_does_not_keep_optional_length_fallback() {
    let wire_src = include_str!("../src/transport/wire.rs");
    let wire_ws = compact_ws(wire_src);

    assert!(
        wire_ws.contains("pub trait WireEncode {")
            && wire_ws.contains("fn encoded_len(&self) -> Option<usize>;")
            && wire_ws
                .contains("fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError>;"),
        "wire encode trait must require an explicit encoded_len owner"
    );
    assert!(
        !wire_ws.contains("pub trait WireEncode { fn encoded_len(&self) -> Option<usize> { None }"),
        "wire encode trait must not keep a default None length fallback"
    );
}

#[test]
fn offer_kernel_stays_three_stage_and_fail_closed() {
    let kernel_src = endpoint_kernel_source();
    let offer_src = include_str!("../src/endpoint/kernel/offer.rs");
    let cursor_src = include_str!("../src/endpoint/kernel/core.rs");
    let decode_src = include_str!("../src/endpoint/kernel/decode.rs");
    let offer_body = impl_body(offer_src, "pub async fn offer(");
    let select_scope_body = impl_body(
        cursor_src,
        "fn select_scope(&mut self) -> RecvResult<OfferScopeSelection>",
    );
    let resolve_token_body = impl_body(cursor_src, "async fn resolve_token(");
    let materialize_branch_body = impl_body(cursor_src, "fn materialize_branch(");
    let preview_flow_meta_body = impl_body(
        cursor_src,
        "pub(super) fn preview_flow_meta<M>(&mut self) -> SendResult<crate::endpoint::kernel::SendPreview>",
    );
    let send_with_meta_body = impl_body(
        cursor_src,
        "async fn send_with_meta_and_cursor_in_place<M>(",
    );
    let prepare_send_control_body = impl_body(cursor_src, "fn prepare_send_control<M>(");
    let decode_branch_body = impl_body(decode_src, "pub async fn decode_branch<M>(");
    let apply_branch_recv_policy_body = impl_body(decode_src, "fn apply_branch_recv_policy(");

    let select_idx = offer_body
        .find("let selection = self_endpoint.select_scope()?;")
        .expect("offer must start by selecting a scope");
    let resolve_idx = offer_body
        .find(".resolve_token(")
        .expect("offer must resolve authority via resolve_token");
    let materialize_idx = offer_body
        .find("return self_endpoint.materialize_branch(")
        .expect("offer must materialize the chosen branch last");
    assert!(
        select_idx < resolve_idx && resolve_idx < materialize_idx,
        "offer kernel must stay ordered as select_scope -> resolve_token -> materialize_branch"
    );

    for forbidden in [
        "rebuild_pending_offers",
        "build_frontier_snapshot",
        "select_offer_entry",
        "lag correction",
        "passive takeover",
    ] {
        assert!(
            !kernel_src.contains(forbidden),
            "offer kernel must not regrow stale rescue helper: {forbidden}"
        );
    }

    for forbidden in [
        "poll_binding_for_offer(",
        "poll_binding_any_for_offer(",
        "take_scope_ack(",
        "peek_scope_ack(",
        "prepare_route_decision_from_resolver",
        "materialize_branch(",
        ".await",
    ] {
        assert!(
            !select_scope_body.contains(forbidden),
            "select_scope must stay non-consuming and authority-free: {forbidden}"
        );
    }

    for required in [
        "take_scope_ack(",
        "peek_scope_ack(",
        "RouteDecisionToken::from_resolver(",
        "RouteDecisionToken::from_poll(",
        "on_frontier_defer(",
    ] {
        assert!(
            resolve_token_body.contains(required),
            "resolve_token must remain the sole owner of route authority progression: {required}"
        );
    }
    for forbidden in ["materialize_branch(", "materialize_selected_arm_meta("] {
        assert!(
            !resolve_token_body.contains(forbidden),
            "resolve_token must not materialize branches: {forbidden}"
        );
    }

    assert!(
        materialize_branch_body.contains("self.preview_selected_arm_meta("),
        "materialize_branch must remain the owner of branch metadata materialization"
    );
    assert!(
        !preview_flow_meta_body.contains("evaluate_dynamic_policy("),
        "preview_flow_meta must stay policy-free and preview-only"
    );
    assert!(
        !send_with_meta_body.contains("evaluate_dynamic_policy("),
        "send orchestration must keep dynamic policy inside the consume-preparation helper"
    );
    assert!(
        prepare_send_control_body.contains("evaluate_dynamic_policy("),
        "send consume preparation must own dynamic policy evaluation"
    );
    for forbidden in [
        "take_scope_ack(",
        "peek_scope_ack(",
        "prepare_route_decision_from_resolver",
        "on_frontier_defer(",
        "eval_endpoint_policy(",
        "apply_recv_policy(",
    ] {
        assert!(
            !materialize_branch_body.contains(forbidden),
            "materialize_branch must not perform authority selection or defer logic: {forbidden}"
        );
    }
    assert!(
        decode_branch_body.contains("apply_branch_recv_policy("),
        "decode_branch must delegate recv policy consumption to the consume path helper",
    );
    for required in ["eval_endpoint_policy(", "apply_recv_policy("] {
        assert!(
            apply_branch_recv_policy_body.contains(required),
            "decode consume helper must own recv policy consumption: {required}"
        );
    }
}

fn impl_body<'a>(src: &'a str, anchor: &str) -> &'a str {
    let impl_anchor = src
        .find(anchor)
        .unwrap_or_else(|| panic!("missing impl anchor: {anchor}"));
    let open_brace = src[impl_anchor..]
        .find('{')
        .map(|idx| impl_anchor + idx)
        .expect("impl opening brace");
    let mut depth = 0usize;
    for (offset, ch) in src[open_brace..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    let end = open_brace + offset;
                    return &src[open_brace + 1..end];
                }
            }
            _ => {}
        }
    }
    panic!("impl closing brace");
}

#[test]
fn route_branch_does_not_expose_scope_coordinate_getters() {
    let src = include_str!("../src/endpoint/kernel/decode.rs");
    let impl_body = compact_ws(impl_body(
        src,
        "impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>",
    ));

    assert!(
        !impl_body.contains("pub fn scope_id("),
        "RouteBranch must not expose scope_id getter"
    );
    assert!(
        !impl_body.contains("pub fn scope_kind("),
        "RouteBranch must not expose scope_kind getter"
    );
    assert!(
        !impl_body.contains("pub fn scope_region("),
        "RouteBranch must not expose scope_region getter"
    );
}

#[test]
fn endpoint_app_surface_stays_on_canonical_localside_helpers() {
    let endpoint_src = include_str!("../src/endpoint.rs");
    let allowlist = endpoint_public_api_allowlist();

    for forbidden in ["pub async fn recv_direct", "pub async fn send_direct"] {
        assert!(
            !endpoint_src.contains(forbidden),
            "Endpoint facade must not regrow direct localside helpers: {forbidden}"
        );
        assert!(
            !allowlist.contains(forbidden),
            "endpoint public API allowlist must not keep deleted direct localside helpers: {forbidden}"
        );
    }

    for required in [
        "pub fn flow<'e, M>(",
        "pub async fn recv<M>(",
        "pub async fn offer<'e>(",
    ] {
        assert!(
            endpoint_src.contains(required),
            "Endpoint facade must stay on the canonical localside core API: {required}"
        );
    }
}

#[test]
fn eff_list_does_not_re_expose_policy_marker_slices() {
    let src = include_str!("../src/global/const_dsl.rs");

    for forbidden in [
        "pub enum StaticPlanKind",
        "pub(crate) enum StaticPlanKind",
        "pub enum PolicyMode",
        "pub struct PolicyMarker",
        "pub struct ControlSpecMarker",
        "pub const fn splice_local(dst_lane: u16) -> Self",
        "pub const fn reroute_local(dst_lane: u16, shard: u32) -> Self",
        "pub const fn with_scope_controller(self, scope: ScopeId, controller_role: u8) -> Self",
        "pub const fn with_scope_controller_role(self, scope: ScopeId, controller_role: u8) -> Self",
        "pub const fn with_scope_linger(self, scope: ScopeId, linger: bool) -> Self",
        "pub const fn with_control(self, scope_kind: ControlScopeKind, tap_id: u16) -> Self",
        "pub const fn static_kind(self) -> Option<StaticPlanKind>",
        "pub const fn push_policy(mut self, offset: usize, policy: PolicyMode) -> Self",
        "pub const fn push_control_spec(mut self, offset: usize, spec: ControlLabelSpec) -> Self",
        "pub fn push_control_spec_mut(&mut self, offset: usize, spec: ControlLabelSpec)",
        "pub(crate) fn push_control_spec_mut(&mut self, offset: usize, spec: ControlLabelSpec)",
        "pub fn push_policy_mut(&mut self, offset: usize, policy: PolicyMode)",
        "pub(crate) fn push_policy_mut(&mut self, offset: usize, policy: PolicyMode)",
    ] {
        assert!(
            !src.contains(forbidden),
            "policy-marker internals must not remain public: {forbidden}"
        );
    }
    for required in [
        "pub(crate) enum PolicyMode",
        "pub(crate) struct PolicyMarker",
        "pub(crate) struct ControlSpecMarker",
        "pub(crate) const fn with_scope_controller(self, scope: ScopeId, controller_role: u8) -> Self",
        "pub(crate) const fn with_scope_controller_role(",
        "pub(crate) const fn with_scope_linger(self, scope: ScopeId, linger: bool) -> Self",
        "pub(crate) const fn with_control(self, scope_kind: ControlScopeKind, tap_id: u16) -> Self",
        "pub(crate) const fn push_policy(mut self, offset: usize, policy: PolicyMode) -> Self",
        "pub(crate) const fn push_control_spec(mut self, offset: usize, spec: ControlLabelSpec) -> Self",
    ] {
        assert!(
            src.contains(required),
            "policy-marker internals should stay crate-private: {required}"
        );
    }

    assert!(
        !src.contains("pub const fn policies(&self) -> &[PolicyMarker]"),
        "EffList::policies must not remain public"
    );
    assert!(
        !src.contains("fn policies(&self) -> &[PolicyMarker]"),
        "EffList::policies should stay deleted so compiled lowering remains the sole policy owner"
    );
    assert!(
        !src.contains("pub const fn policy_at(&self, offset: usize) -> Option<PolicyMode>"),
        "EffList::policy_at must not remain public"
    );
    assert!(
        src.contains("pub(crate) const fn policy_at(&self, offset: usize) -> Option<PolicyMode>"),
        "EffList::policy_at should stay crate-private"
    );
    assert!(
        !src.contains(
            "pub fn policy_with_scope(&self, offset: usize) -> Option<(PolicyMode, ScopeId)>"
        ) && !src.contains(
            "pub const fn policy_with_scope(&self, offset: usize) -> Option<(PolicyMode, ScopeId)>"
        ),
        "EffList::policy_with_scope must not remain public"
    );
    assert!(
        src.contains(
            "pub(crate) const fn policy_with_scope(&self, offset: usize) -> Option<(PolicyMode, ScopeId)>"
        ),
        "EffList::policy_with_scope should stay crate-private"
    );
}

#[test]
fn eff_list_does_not_reintroduce_derived_lookup_tables() {
    let src = include_str!("../src/global/const_dsl.rs");
    let compiled_driver_src = include_str!("../src/global/compiled/driver.rs");
    let compact = compact_ws(src);

    assert!(
        compact.contains(
            "pub struct EffList { data: [EffStruct; MAX_CAPACITY], len: usize, scope_budget: u16, scope_markers: [ScopeMarker; MAX_CAPACITY], scope_marker_len: usize, control_markers: [ControlMarker; MAX_CAPACITY], control_marker_len: usize, policy_markers: [PolicyMarker; MAX_CAPACITY], policy_marker_len: usize, control_specs: [ControlSpecMarker; MAX_CAPACITY], control_spec_len: usize, }"
        ),
        "EffList should stay a compact owner without derived lookup-table baggage"
    );

    for forbidden in [
        "scope_by_offset:",
        "scope_linger_by_ordinal:",
        "scope_marker_head_by_ordinal:",
        "scope_marker_next:",
        "policy_by_offset:",
        "policy_index_by_offset:",
        "dynamic_policy_from_offset:",
        "control_spec_by_offset:",
        "control_spec_index_by_offset:",
        "const fn rebuild_scope_by_offset(",
        "const fn rebuild_dynamic_policy_index(",
        "struct DynamicPolicyInfo {",
    ] {
        assert!(
            !src.contains(forbidden),
            "EffList must not regrow derived lookup tables or rebuild shims: {forbidden}"
        );
    }

    for required in [
        "pub(crate) const fn scope_id_for_offset(&self, offset: usize) -> Option<ScopeId> {",
        "pub const fn scope_has_linger(&self, scope: ScopeId) -> bool {",
    ] {
        assert!(
            src.contains(required),
            "EffList should keep direct lookup helpers while deriving them on demand: {required}"
        );
    }
    assert!(
        !src.contains("pub(crate) const fn first_dynamic_policy_in_range("),
        "EffList must not own dynamic policy scans once lowering view is the single authority"
    );
    assert!(
        compiled_driver_src.contains("pub(crate) const fn first_dynamic_policy_in_range("),
        "LoweringView must remain the sole owner of dynamic policy scans"
    );
}

#[test]
fn role_program_does_not_own_policy_marker_metadata_iterators() {
    let role_program_src = include_str!("../src/global/role_program.rs");
    let cluster_core_src = include_str!("../src/control/cluster/core.rs");
    let compiled_program_src = include_str!("../src/global/compiled/program.rs");
    let compiled_driver_src = include_str!("../src/global/compiled/driver.rs");

    for forbidden in [
        "pub(crate) struct PolicyInfo",
        "pub(crate) struct PolicyIter",
        "pub(crate) fn policies(&self) -> PolicyIter",
        "program.policies()",
    ] {
        assert!(
            !role_program_src.contains(forbidden) && !cluster_core_src.contains(forbidden),
            "policy metadata must be reconstructed at the cluster owner instead of exposed via RoleProgram: {forbidden}"
        );
    }
    assert!(
        cluster_core_src
            .contains("self.with_transient_compiled_program(rv_id, program, |compiled|")
            && cluster_core_src.contains(".dynamic_policy_sites_for(POLICY)"),
        "cluster owner must consume transient compiled-program facts instead of rescanning policy markers"
    );
    assert!(
        !cluster_core_src.contains(".policies()") && !compiled_program_src.contains(".policies()"),
        "policy marker scans must not bypass the compiled lowering owner"
    );
    assert!(
        compiled_driver_src.contains("eff_list.policy_with_scope(")
            && !compiled_program_src.contains("eff_list.policy_with_scope("),
        "the unified lowering driver must own policy_with_scope reconstruction instead of CompiledProgram"
    );
}

#[test]
fn program_projection_validates_without_materializing_runtime_compiled_owners() {
    let program_src = include_str!("../src/global/program.rs");
    let role_program_src = include_str!("../src/global/role_program.rs");

    for required in [
        "let summary = LoweringSummary::scan_const(<Steps as BuildProgramSource>::SOURCE.eff_list());",
        "summary.validate_projection_program();",
        "validate_all_roles(&summary);",
        "summary.stamp()",
        "validated_program_stamp::<Steps>()",
        "RoleProgram::new(",
    ] {
        assert!(
            program_src.contains(required) || role_program_src.contains(required),
            "Program/project must stamp and validate without materializing runtime owners: {required}"
        );
    }
    for forbidden in [
        "CompiledProgram::compile(",
        "CompiledRole::compile(",
        "CompiledProgram::from_summary(",
        "CompiledRole::from_summary(",
        "CompiledProgram::validate_summary(",
        "CompiledRole::validate_summary(",
    ] {
        assert!(
            !program_src.contains(forbidden) && !role_program_src.contains(forbidden),
            "Program/project must not materialize runtime compiled owners during stamping: {forbidden}"
        );
    }
}

#[test]
fn runtime_compiled_materialization_stays_transient_and_cacheless() {
    let cluster_core_src = include_str!("../src/control/cluster/core.rs");
    let cluster_core_src = cluster_core_src
        .split("\nmod tests {")
        .next()
        .expect("cluster core runtime section");
    let cluster_core_ws = compact_ws(&cluster_core_src);

    for required in [
        "crate::global::compiled::with_lowering_lease(",
        "crate::global::compiled::LoweringLeaseMode::SummaryOnly",
        "crate::global::compiled::LoweringLeaseMode::SummaryAndRoleScratch",
    ] {
        assert!(
            cluster_core_ws.contains(required),
            "runtime transient materialization must route through the compiled lowering lease owner: {required}"
        );
    }
    for forbidden in [
        "struct TransientCompileScratch<'a> {",
        "fn transient_compile_scratch(",
        "program.ensure_program_image_from_summary(",
        "program.ensure_images_from_summary(",
        "scratch.init_summary(",
        "scratch.summary()",
        "scratch.role_compile_scratch()",
    ] {
        assert!(
            !cluster_core_src.contains(forbidden),
            "cluster owner must not rebuild lowering summaries directly once the compiled lowering lease owns image materialization: {forbidden}"
        );
    }
    let role_program_src = include_str!("../src/global/role_program.rs");
    let role_program_runtime_src = role_program_src
        .split("\n#[cfg(test)]\nmod tests {")
        .next()
        .expect("role program runtime section");
    assert!(
        !role_program_runtime_src.contains("CompiledProgram::init_from_summary(")
            && !role_program_runtime_src.contains("CompiledRole::init_from_summary::<ROLE>("),
        "RoleProgram must stay a thin witness and must not own compiled image storage or materialization"
    );

    for forbidden in [
        "MAX_COMPILED_PROGRAMS",
        "MAX_COMPILED_ROLES",
        "compiled_programs:",
        "compiled_roles:",
        "ProgramCacheEntry",
        "RoleCacheEntry",
        "CompiledCacheLease",
        "acquire_compiled_cache(",
        "with_pinned_compiled_program(",
        "with_pinned_compiled_role(",
        "release_compiled_cache_lease(",
    ] {
        assert!(
            !cluster_core_src.contains(forbidden),
            "runtime compiled cache must be gone from the cluster owner: {forbidden}"
        );
    }

    assert!(
        !cluster_core_ws.contains("transient_compiled:TransientCompileScratch")
            && !cluster_core_ws.contains("transient_compiled: TransientCompileScratch")
            && !cluster_core_ws.contains("endpoint_compiled:[PublicEndpointCompiledCell;MAX_RV]")
            && !cluster_core_ws.contains("endpoint_compiled: [PublicEndpointCompiledCell; MAX_RV]")
            && !cluster_core_src.contains("struct PublicEndpointCompiledCell")
            && !cluster_core_src.contains("transient_delegation_graph")
            && !cluster_core_src.contains("transient_splice_graph"),
        "ControlCore must keep transient lowering scratch only; compiled endpoint and graph caches must stay gone"
    );
}

#[test]
fn compiled_authority_completion_stays_summary_backed() {
    let runtime_mgmt_src = include_str!("../src/runtime/mgmt.rs");
    let runtime_mgmt_request_reply_src = include_str!("../src/runtime/mgmt/request_reply.rs");
    let runtime_mgmt_test_support_src = include_str!("../src/runtime/mgmt/test_support.rs");
    let role_program_src = include_str!("../src/global/role_program.rs");
    let cluster_effects_src = include_str!("../src/control/cluster/effects.rs");

    for forbidden in ["CompiledProgram::compile(", "CompiledRole::compile("] {
        assert!(
            !runtime_mgmt_src.contains(forbidden)
                && !runtime_mgmt_request_reply_src.contains(forbidden)
                && !runtime_mgmt_test_support_src.contains(forbidden)
                && !role_program_src.contains(forbidden)
                && !cluster_effects_src.contains(forbidden),
            "compiled authority completion must not keep raw EffList compiled-owner constructors in helper paths: {forbidden}"
        );
    }

    for required in [
        "with_management_compiled_programs_for_test",
        "crate::global::lowering_input(&CONTROLLER_PROGRAM)",
        "crate::global::lowering_input(&CLUSTER_PROGRAM)",
        "crate::global::compiled::with_compiled_programs(",
    ] {
        assert!(
            runtime_mgmt_request_reply_src.contains(required)
                || runtime_mgmt_test_support_src.contains(required),
            "management compiled helper must stay summary-backed: {required}"
        );
    }
    assert!(
        !runtime_mgmt_request_reply_src.contains(".eff_list_ref()")
            && !runtime_mgmt_test_support_src.contains(".eff_list_ref()"),
        "management compiled helper must not materialize compiled owners from raw eff_list_ref()"
    );
    assert!(
        !role_program_src.contains("pub(crate) fn compile_role(&self) -> CompiledRole {"),
        "RoleProgram must not keep a dead direct compiled-role helper"
    );
    for required in [
        "fn with_compiled_role_in_slot<const ROLE: u8, GlobalSteps, R>(",
        "crate::global::compiled::with_compiled_role_in_slot::<ROLE, _>(",
        "super::lowering_input(program)",
    ] {
        assert!(
            role_program_src.contains(required),
            "RoleProgram test helpers must delegate compiled-role materialization through the compiled owner: {required}"
        );
    }
}

#[test]
fn large_owner_types_do_not_regress_to_copy_semantics() {
    let effects_ws = compact_ws(include_str!("../src/control/cluster/effects.rs"));
    let role_program_ws = compact_ws(include_str!("../src/global/role_program.rs"));
    let typestate_ws = compact_ws(include_str!("../src/global/typestate/builder.rs"));

    for forbidden in [
        "#[derive(Debug, Clone, Copy)] pub(crate) struct EffectEnvelope {",
        "#[derive(Clone, Copy, Debug)] pub(crate) struct ProjectedRoleLayout {",
        "#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub struct RoleTypestate<const ROLE: u8> {",
    ] {
        assert!(
            !effects_ws.contains(forbidden)
                && !role_program_ws.contains(forbidden)
                && !typestate_ws.contains(forbidden),
            "large lowering/runtime owner types must not regain Copy semantics: {forbidden}"
        );
    }
}

#[test]
fn role_program_projection_metadata_stays_internal() {
    let role_program_src = include_str!("../src/global/role_program.rs");
    let compiled_lease_src = include_str!("../src/global/compiled/lease.rs");
    let role_program_ws = compact_ws(role_program_src);

    for forbidden in [
        "pub struct LaneSteps {",
        "pub struct PhaseRouteGuard {",
        "pub struct Phase {",
        "pub enum LocalDirection {",
        "pub struct LocalStep {",
        "pub fn active_lanes(&self) -> [bool; MAX_LANES] {",
        "pub struct ProjectedRoleLayout {",
        "pub struct ProjectedRoleData<const ROLE: u8> {",
        "pub fn layout(&self) -> ProjectedRoleLayout {",
        "fn layout(&self) -> ProjectedRoleLayout {",
        "pub fn projection(&self) -> ProjectedRoleData<ROLE> {",
        "pub(crate) fn projection(&self) -> ProjectedRoleData<ROLE> {",
        "pub(crate) fn active_lanes(&self) -> [bool; MAX_LANES] {",
        "pub struct LocalStepMeta {",
        "pub struct LocalMetaTable<'a> {",
        "pub const fn phase(&self, index: usize) -> &Phase {",
        "pub fn step_meta_for(&self, eff_index: EffIndex) -> Option<LocalStepMeta> {",
        "pub fn step_metas(&self) -> impl Iterator<Item = LocalStepMeta> + '_ {",
        "pub fn meta_table(&self) -> LocalMetaTable<'_> {",
        "pub const fn local_len(&self) -> usize {",
        "pub const fn step_meta(&'static self, idx: usize) -> LocalStep {",
        "pub const fn step_graph(&'static self) -> &'static RoleTypestate<ROLE> {",
        "pub(crate) struct LocalStepMeta {",
        "pub(crate) struct LocalMetaTable<'a> {",
        "pub(crate) const fn phase(&self, index: usize) -> &Phase {",
        "pub(crate) fn step_meta_for(&self, eff_index: EffIndex) -> Option<LocalStepMeta> {",
        "pub(crate) fn step_metas(&self) -> impl Iterator<Item = LocalStepMeta> + '_ {",
        "pub(crate) fn meta_table(&self) -> LocalMetaTable<'_> {",
        "pub(crate) const fn local_len(&self) -> usize {",
        "pub(crate) const fn step_meta(&'static self, idx: usize) -> LocalStep {",
        "pub(crate) const fn step_graph(&'static self) -> &'static RoleTypestate<ROLE> {",
        "impl<'prog, const ROLE: u8, GlobalSteps, Mint> core::ops::Deref",
        "impl<'prog, const ROLE: u8, GlobalSteps, Mint> AsRef<[LocalStep]>",
    ] {
        assert!(
            !role_program_src.contains(forbidden),
            "RoleProgram must not expose projection-inspection helpers publicly: {forbidden}"
        );
    }

    for required in [
        "pub(crate) struct LaneSteps {",
        "pub(crate) struct PhaseRouteGuard {",
        "pub(crate) struct Phase {",
        "pub(crate) enum LocalDirection {",
        "pub(crate) struct LocalStep {",
        "pub(crate) struct ProjectedRoleLayout {",
        "pub(crate) const fn stamp(&self) -> ProgramStamp {",
        "pub(crate) fn borrow_id(&self) -> usize {",
        "pub(crate) struct RoleLoweringInput<'prog> {",
        "pub(crate) const fn lowering_input<'prog, const ROLE: u8, GlobalSteps, Mint>(",
    ] {
        assert!(
            role_program_src.contains(required),
            "RoleProgram witness and lowering-input seams should stay crate-private: {required}"
        );
    }
    assert!(
        !role_program_src.contains("eff_list_ref"),
        "RoleProgram should stop exposing raw EffList inspection helpers once lowering input is erased"
    );
    assert!(
        !role_program_src.contains("pub(crate) fn compile_role(&self) -> CompiledRole {"),
        "RoleProgram must not keep a dead direct compiled-role convenience helper"
    );

    for forbidden in [
        "impl Clone for RoleProgram",
        "impl Copy for RoleProgram",
        "program_image_init: Cell<bool>",
        "program_image: UnsafeCell<MaybeUninit<CompiledProgram>>",
        "role_image_init: Cell<bool>",
        "role_image: UnsafeCell<MaybeUninit<CompiledRole>>",
        "impl<'prog, const ROLE: u8, GlobalSteps, Mint> Drop for RoleProgram",
    ] {
        assert!(
            !role_program_src.contains(forbidden),
            "RoleProgram must stay thin and avoid inline compiled-image ownership: {forbidden}"
        );
    }
    for forbidden in [
        "TransientRoleProgramScratch",
        "TransientLoweringScratch",
        "with_summary_from_storage",
        "with_lowering_scratch_from_storage",
    ] {
        assert!(
            !role_program_src.contains(forbidden),
            "RoleProgram must not keep transient lowering lease ownership after erasing to RoleLoweringInput: {forbidden}"
        );
    }
    for required in [
        "pub(crate) enum LoweringLeaseMode {",
        "pub(crate) struct LoweringLease<'a> {",
        "pub(crate) unsafe fn with_lowering_lease<R>(",
    ] {
        assert!(
            compiled_lease_src.contains(required),
            "compiled lowering layer must own the transient lowering lease seam: {required}"
        );
    }
    assert!(
        !role_program_ws.contains("LocalSteps = steps::StepNil"),
        "RoleProgram must not hide typed projection behind a StepNil default"
    );
    for forbidden in [
        "pub struct RoleProgram<'prog, const ROLE: u8, GlobalSteps, Mint = MintConfig> where Mint: MintConfigMarker, { eff_list: &'prog EffList, lease_budget: crate::control::lease::planner::LeaseGraphBudget, global_steps:",
        "pub struct RoleProgram<'prog, const ROLE: u8, GlobalSteps, Mint = MintConfig> where Mint: MintConfigMarker, { eff_list: &'prog EffList, lease_budget: crate::control::lease::planner::LeaseGraphBudget, mint: Mint, phases:",
        "pub struct RoleProgram<'prog, const ROLE: u8, GlobalSteps, Mint = MintConfig> where Mint: MintConfigMarker, { eff_list: &'prog EffList, lease_budget: crate::control::lease::planner::LeaseGraphBudget, mint: Mint, typestate:",
    ] {
        assert!(
            !role_program_ws.contains(forbidden),
            "RoleProgram must stay thin and avoid storing materialized projection metadata: {forbidden}"
        );
    }
}

#[test]
fn compiled_role_layout_and_typestate_registry_stay_compact_indexed() {
    let builder_src = include_str!("../src/global/typestate/builder.rs");
    let compiled_role_src = include_str!("../src/global/compiled/role.rs");
    let cursor_src = include_str!("../src/global/typestate/cursor.rs");
    let role_program_src = include_str!("../src/global/role_program.rs");
    let registry_src = include_str!("../src/global/typestate/registry.rs");

    for required in [
        "pub struct RoleTypestate<const ROLE: u8> {",
        "pub(super) len: u16,",
    ] {
        assert!(
            builder_src.contains(required),
            "role typestate owner should keep compact internal bounds: {required}"
        );
    }

    for required in [
        "pub(crate) struct LaneSteps {",
        "pub start: u16,",
        "pub len: u16,",
        "pub(crate) struct Phase {",
        "pub min_start: u16,",
        "pub(crate) struct ProjectedRoleLayout {",
        "local_steps: [LocalStep; MAX_STEPS],",
        "phases: [Phase; MAX_PHASES],",
    ] {
        assert!(
            role_program_src.contains(required),
            "projected role layout should keep compact internal bounds: {required}"
        );
    }

    for forbidden in ["local_len: u16,", "phase_len: u8,"] {
        assert!(
            !role_program_src.contains(forbidden),
            "projected role layout should derive sentinel-backed lengths: {forbidden}"
        );
    }

    for required in [
        "pub(crate) struct ScopeRecord {",
        "pub start: StateIndex,",
        "pub end: StateIndex,",
        "pub route_recv: [StateIndex; 2],",
        "pub(crate) const fn route_recv_count(&self) -> u8 {",
        "pub(super) struct ScopeRegistry {",
        "pub(super) records: *const ScopeRecord,",
        "pub(super) len: u16,",
        "pub(super) slots_by_scope: *const u16,",
    ] {
        assert!(
            registry_src.contains(required),
            "typestate registry should keep compact internal bounds: {required}"
        );
    }

    for forbidden in [
        "pub(super) ordinal_index: [u16; SCOPE_ORDINAL_INDEX_CAPACITY],",
        "ordinal_index: [SCOPE_ORDINAL_INDEX_EMPTY; SCOPE_ORDINAL_INDEX_CAPACITY],",
    ] {
        assert!(
            !registry_src.contains(forbidden),
            "typestate registry must not regress to whole-space ordinal indexing: {forbidden}"
        );
    }

    for required in [
        "self.typestate()\n            .first_recv_dispatch_target_for_label(scope_id, label)",
        "self.typestate()\n            .controller_arm_entry_for_label(scope_id, label)",
        "self.typestate().controller_arm_entry_by_arm(scope_id, arm)",
    ] {
        assert!(
            compiled_role_src.contains(required) || cursor_src.contains(required),
            "compiled role and phase cursor must keep controller/dispatch facts in compact shared facts: {required}"
        );
    }
}

#[test]
fn control_semantics_table_stays_fixed_and_stateless() {
    let compiled_program_src = include_str!("../src/global/compiled/program.rs");

    for required in [
        "pub(crate) struct ControlSemanticsTable {}",
        "LABEL_LOOP_CONTINUE => ControlSemanticKind::LoopContinue,",
        "LABEL_LOOP_BREAK => ControlSemanticKind::LoopBreak,",
        "Some(RouteDecisionKind::TAG) => ControlSemanticKind::RouteArm,",
    ] {
        assert!(
            compiled_program_src.contains(required),
            "compiled control semantics must stay fixed and exact: {required}"
        );
    }

    for forbidden in [
        "packed: [u64; 16],",
        "const fn label_slot(label: u8) -> (usize, usize) {",
        "by_label: [ControlSemanticKind; 256]",
        "by_resource_tag: [ControlSemanticKind; 256]",
        "with_resource_tag(",
    ] {
        assert!(
            !compiled_program_src.contains(forbidden),
            "compiled control semantics must not return to dense per-label/resource arrays or packed nibble tables: {forbidden}"
        );
    }
}

#[test]
fn role_typestate_scope_atlas_helpers_stay_deleted() {
    let typestate_src = include_str!("../src/global/typestate.rs");

    for forbidden in [
        "pub struct ScopeAtlasView {",
        "pub fn scope_atlas_view(&self) -> ScopeAtlasView {",
        "pub const fn from_static(program: &'static EffList) -> Self {",
        "fn resolve_scope(&self, scope_id: ScopeId) -> Option<ScopeId> {",
    ] {
        assert!(
            !typestate_src.contains(forbidden),
            "RoleTypestate must not regrow stale scope-atlas inspection helpers: {forbidden}"
        );
    }
}

#[test]
fn seq_is_the_only_public_composition_constructor() {
    let global_src = include_str!("../src/global.rs");
    let program_src = include_str!("../src/global/program.rs");
    let steps_src = include_str!("../src/global/steps.rs");

    assert!(
        !global_src.contains("pub mod compose {"),
        "g::advanced must not regrow a second composition namespace"
    );
    assert!(
        !global_src.contains("advanced::compose")
            && !global_src.contains("pub use super::super::program::seq;"),
        "public choreography composition must stay on g::seq only"
    );
    assert!(
        !global_src.contains("pub use super::super::program::{empty, seq};")
            && !global_src.contains("pub use super::super::program::empty;")
            && !global_src.contains("pub use super::super::program::seq;"),
        "g::advanced must not regrow a second zero-fragment or seq builder surface"
    );
    assert!(
        program_src.contains("pub const fn seq<LeftSteps, RightSteps>("),
        "program.rs must define the canonical composition constructor as seq"
    );
    assert!(
        steps_src.contains("pub const PROGRAM: Program<Self> = Program::<Self>::empty();"),
        "StepNil must own the canonical zero-fragment program witness"
    );
    assert!(
        !program_src.contains("pub const fn empty() -> Program<StepNil> {"),
        "program.rs must not expose a second public zero-fragment builder at module scope"
    );
    assert!(
        global_src.contains("program::route_binary(left, right)")
            && global_src.contains("program::par_binary(left, right)")
            && !global_src.contains("route_inferred")
            && !global_src.contains("par_inferred"),
        "public binary constructors must route through canonical binary helpers instead of inferred-builder vocabulary"
    );
    assert!(
        program_src.contains("pub(crate) const fn route_binary<LeftSteps, RightSteps>(")
            && program_src.contains("pub(crate) const fn par_binary<LeftSteps, RightSteps>(")
            && !program_src.contains("pub(crate) const fn route_inferred<")
            && !program_src.contains("pub(crate) const fn par_inferred<"),
        "program.rs must keep binary route/par owners canonical and delete inferred-builder helpers"
    );
}

#[test]
fn route_and_parallel_bounds_use_semantic_witnesses() {
    let global_src = include_str!("../src/global.rs");
    let program_src = include_str!("../src/global/program.rs");
    let global_ws = compact_ws(global_src);
    let program_ws = compact_ws(program_src);

    for required in [
        "pub trait RouteArmHead {",
        "pub trait SameRouteController<Other> {}",
        "pub trait DistinctRouteLabels<Other> {}",
        "pub trait NonEmptyParallelArm {",
        "trait LabelEq<Other> {",
        "trait RequireFalse {}",
        "mod label_eq;",
        "`g::route(left, right)` arms must begin with a controller self-send",
        "`g::route(left, right)` arms must start with the same controller self-send",
        "`g::route(left, right)` arms must use distinct labels",
        "`g::par(left, right)` arms must be non-empty protocol fragments",
    ] {
        assert!(
            global_src.contains(required) || program_src.contains(required),
            "route/par public bounds must use semantic witnesses instead of internal trait names: {required}"
        );
    }

    for required in [
        "LeftSteps: RouteArmHead + SameRouteController<RightSteps> + DistinctRouteLabels<RightSteps>",
        "RightSteps: RouteArmHead + TailLoopControl",
        "Left: BuildProgramSource + RouteArmHead + RouteArmLoopHead + SameRouteController<Right> + DistinctRouteLabels<Right>",
        "Right: BuildProgramSource + RouteArmHead + RouteArmLoopHead + TailLoopControl",
        "LeftSteps: NonEmptyParallelArm",
        "RightSteps: NonEmptyParallelArm + TailLoopControl",
        "let left_set = <Left as NonEmptyParallelArm>::ROLE_LANE_SET;",
        "let right_set = <Right as NonEmptyParallelArm>::ROLE_LANE_SET;",
        "type Controller = RouteController;",
    ] {
        assert!(
            global_ws.contains(required) || program_ws.contains(required),
            "route/par bounds must route through semantic witnesses directly: {required}"
        );
    }
    assert!(
        !global_src.contains("type Controller = Controller;"),
        "route semantic owners must not keep self-shadowing associated type shims"
    );

    for forbidden in [
        "LeftSteps: BinaryRoutePair<RightSteps>",
        "LeftSteps: steps::StepRoleSet + steps::StepNonEmpty + StepConcat<RightSteps>",
        "let controller = <LeftSteps as BinaryRoutePair<RightSteps>>::CONTROLLER;",
        "let left_set = <LeftSteps as StepRoleSet>::ROLE_LANE_SET;",
        "let right_set = <RightSteps as StepRoleSet>::ROLE_LANE_SET;",
        "pub trait RouteArms<Right>",
        "pub trait ParallelArms<Right>",
        "RouteControllerArm",
        "ParallelLaneShape",
        "duplicate route label",
    ] {
        assert!(
            !global_src.contains(forbidden) && !program_src.contains(forbidden),
            "route/par public bounds must not leak stale internal witness names: {forbidden}"
        );
    }
}

#[test]
fn docs_do_not_keep_project_turbofish_shims() {
    let readme_src = include_str!("../README.md");
    let app_path = readme_src
        .split("## Substrate Surface (protocol implementors only)")
        .next()
        .expect("README app-facing section");

    assert!(
        !readme_src.contains("project::<"),
        "README must not keep project::<...> turbofish shims"
    );
    for forbidden in [
        "The exact projected `LocalSteps` type is part of the contract.",
        "Do not erase `LocalSteps`.",
        "use hibana::g::advanced::steps::{ProjectRole, SendStep, StepCons, StepNil};",
    ] {
        assert!(
            !readme_src.contains(forbidden),
            "README must not keep the old projected-local walkthrough: {forbidden}"
        );
    }
    assert!(
        !app_path.contains("g::advanced"),
        "README app-facing path must not surface g::advanced before the SPI section"
    );
    assert!(
        readme_src.contains("App authors should stay on `g` and `Endpoint`."),
        "README must direct app authors to g + Endpoint only"
    );
    assert!(
        readme_src.contains("## Substrate Surface (protocol implementors only)")
            && readme_src.contains("Protocol implementors use the protocol-neutral SPI:"),
        "README must label projection examples as protocol-implementor SPI only"
    );
}

#[test]
fn hibana_tests_do_not_depend_on_workspace_files() {
    fn visit_rs_files(root: &std::path::Path, f: &mut impl FnMut(&std::path::Path)) {
        for entry in std::fs::read_dir(root)
            .unwrap_or_else(|err| panic!("read_dir {} failed: {}", root.display(), err))
        {
            let entry = entry
                .unwrap_or_else(|err| panic!("read_dir entry {} failed: {}", root.display(), err));
            let path = entry.path();
            if path.is_dir() {
                visit_rs_files(&path, f);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                f(&path);
            }
        }
    }

    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests");
    let forbidden = [
        ["..", "/..", "/api-sketch.md"].concat(),
        ["..", "/api-sketch.md"].concat(),
        ["..", "/AGENTS.md"].concat(),
        ["..", "/..", "/.github/workflows/macos-udp-tokio.yml"].concat(),
        ["include_str!(\"", "..", "/..", "/"].concat(),
        ["include_bytes!(\"", "..", "/..", "/"].concat(),
    ];

    visit_rs_files(&root, &mut |path| {
        let body = std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("read {} failed: {}", path.display(), err));
        for forbidden in &forbidden {
            assert!(
                !body.contains(forbidden.as_str()),
                "hibana tests must not depend on workspace files outside the repo root: {} contains `{forbidden}`",
                path.display()
            );
        }
    });
}

#[test]
fn regression_fixtures_do_not_hide_canonical_owners_behind_synonyms() {
    for (path, forbidden) in [
        ("nested_loop_route.rs", "type Steps = Decision;"),
        (
            "nested_loop_route.rs",
            "const PROGRAM: g::Program<Decision> = DECISION;",
        ),
        ("nested_loop_route.rs", "type TickSteps = StepCons<"),
        (
            "nested_loop_route.rs",
            "type CombinedSteps = <HandshakeSteps as StepConcat<Decision>>::Output;",
        ),
        (
            "nested_route_runtime.rs",
            "type InnerLeftControl = OuterLeftControl;",
        ),
        (
            "nested_route_runtime.rs",
            "type InnerRightControl = OuterRightControl;",
        ),
        (
            "nested_route_runtime.rs",
            "type InnerLeftControlStep = StepCons<",
        ),
        (
            "nested_route_runtime.rs",
            "type ProtocolSteps = <OuterLeftSteps as StepConcat<OuterRightSteps>>::Output;",
        ),
        (
            "route_dynamic_control.rs",
            "const OUTER_LOOP_BREAK_ARM: g::Program<LoopBrkSteps> = LOOP_BREAK_ARM;",
        ),
        ("route_dynamic_control.rs", "type LeftSteps = StepCons<"),
        (
            "route_dynamic_control.rs",
            "type NestedLoopSteps = <NestedLoopContinueSteps as StepConcat<LoopBrkSteps>>::Output;",
        ),
        (
            "ui/g-typelist-local-missing-step.rs",
            "type MissingLocal = StepNil;",
        ),
        ("cursor_send_recv.rs", "type Origin = Role<0>;"),
        ("cursor_send_recv.rs", "type PayloadMsg = Msg<1, u32>;"),
        ("cursor_send_recv.rs", "type GlobalSteps = StepCons<"),
        (
            "cursor_send_recv.rs",
            "type OriginLocal = <GlobalSteps as ProjectRole<Role<0>>>::Output;",
        ),
        ("route_dynamic_control.rs", "type Controller = Role<0>;"),
        ("route_dynamic_control.rs", "type RouteLeft = Msg<"),
        ("local_action.rs", "type LocalSteps = StepCons<"),
        (
            "local_action.rs",
            "type ActorLocal = <LocalSteps as ProjectRole<Role<0>>>::Output;",
        ),
        ("substrate_surface.rs", "type ProgramSteps = StepCons<"),
        (
            "substrate_surface.rs",
            "type ClientLocal = <ProgramSteps as ProjectRole<g::Role<0>>>::Output;",
        ),
        (
            "ui/g-route-policy-mismatch.rs",
            "type RouteMsgWithPolicy = Msg<",
        ),
        ("ui-pass/g-par-many.rs", "type R0 = g::Role<0>;"),
        ("ui-pass/g-par-many.rs", "type LaneA = StepCons<"),
        ("ui/g-efflist-deref.rs", "type Steps = StepCons<"),
        ("ui/g-empty_arms.rs", "type ArmSteps = StepCons<"),
        ("ui/g-par-role-conflict.rs", "type LaneA = StepCons<"),
        (
            "ui/g-route-policy-required.rs",
            "type LeftSteps = StepCons<",
        ),
        (
            "ui/g-route-controller-mismatch.rs",
            "type BadArm = StepCons<",
        ),
        (
            "ui/g-typed-route-duplicate-label.rs",
            "type ArmSteps = StepCons<",
        ),
        (
            "ui/g-typelist-local-label-mismatch.rs",
            "type GlobalSteps = StepCons<",
        ),
        (
            "ui/g-typelist-local-label-mismatch.rs",
            "type WrongLocal = StepCons<",
        ),
        (
            "ui/g-typelist-local-missing-step.rs",
            "type GlobalSteps = StepCons<",
        ),
        (
            "ui/g-typelist-local-payload-mismatch.rs",
            "type GlobalSteps = StepCons<",
        ),
        (
            "ui/g-typelist-local-payload-mismatch.rs",
            "type WrongClientLocal = StepCons<",
        ),
        ("ui/control-cancel-payload.rs", "type Steps = StepCons<"),
        ("ui/control-checkpoint-payload.rs", "type Steps = StepCons<"),
        (
            "ui-pass/g-route-static-control-basic.rs",
            "const LABEL: u8 = LABEL;",
        ),
        ("ui-pass/g-route-merged.rs", "const LABEL: u8 = LABEL;"),
        (
            "ui-pass/g-route-static-control-prefix-local.rs",
            "const LABEL: u8 = LABEL;",
        ),
        (
            "ui-pass/g-route-static-control-prefix-send.rs",
            "const LABEL: u8 = LABEL;",
        ),
        (
            "ui-pass/dynamic_route_defer_compiles.rs",
            "const LABEL: u8 = LABEL;",
        ),
        (
            "ui-pass/g-route-merged.rs",
            "type Arm0ControlStep = StepCons<",
        ),
        (
            "ui-pass/g-route-merged.rs",
            "type Steps = <Arm0Steps as StepConcat<Arm1Steps>>::Output;",
        ),
        (
            "ui-pass/g-route-static-control-basic.rs",
            "type Arm0ControlStep = StepCons<",
        ),
        (
            "ui-pass/g-route-static-control-basic.rs",
            "type Steps = <Arm0Steps as StepConcat<Arm1Steps>>::Output;",
        ),
        (
            "ui-pass/g-route-static-control-prefix-local.rs",
            "type Arm0ControlStep = StepCons<",
        ),
        (
            "ui-pass/g-route-static-control-prefix-local.rs",
            "type Steps = <Arm0Steps as StepConcat<Arm1Steps>>::Output;",
        ),
        (
            "ui-pass/g-route-static-control-prefix-send.rs",
            "type Arm0ControlStep = StepCons<",
        ),
        (
            "ui-pass/g-route-static-control-prefix-send.rs",
            "type Steps = <Arm0Steps as StepConcat<Arm1Steps>>::Output;",
        ),
        (
            "ui-pass/dynamic_route_defer_compiles.rs",
            "type Arm0ControlStep = StepCons<",
        ),
        (
            "ui-pass/dynamic_route_defer_compiles.rs",
            "type Steps = <Arm0Steps as StepConcat<Arm1Steps>>::Output;",
        ),
        ("ui/g-route-policy-mismatch.rs", "const LABEL: u8 = LABEL;"),
        ("ui/g-route-unprojectable.rs", "const LABEL: u8 = LABEL;"),
        (
            "cancel_rollback.rs",
            "type CancelProgram = g::Program<CancelSteps>;",
        ),
        (
            "cancel_rollback.rs",
            "type CheckpointProgram = g::Program<CheckpointSteps>;",
        ),
        (
            "offer_decode_binding_regression.rs",
            "type ControllerEndpoint = Endpoint<",
        ),
        (
            "offer_decode_binding_regression.rs",
            "type WorkerEndpoint = Endpoint<",
        ),
        (
            "cancel_rollback.rs",
            "type ControllerCancelLocal = <CancelSteps as ProjectRole<Role<0>>>::Output;",
        ),
        ("cancel_rollback.rs", "type CancelSteps = StepCons<"),
        (
            "cancel_rollback.rs",
            "type CheckpointSteps = SeqSteps<CheckpointStep, RollbackStep>;",
        ),
        (
            "cancel_rollback.rs",
            "type BootstrapSteps = StepCons<SendStep<Role<0>, Role<1>, Msg<1, u32>, 0>, StepNil>;",
        ),
        (
            "cancel_rollback.rs",
            "type ControllerCheckpointLocal = <CheckpointSteps as ProjectRole<Role<0>>>::Output;",
        ),
        (
            "cancel_rollback.rs",
            "type ControllerBootstrapLocal = <BootstrapSteps as ProjectRole<Role<0>>>::Output;",
        ),
        (
            "loop_lane_share.rs",
            "type TargetLocal = <ProtocolSteps as ProjectRole<Role<1>>>::Output;",
        ),
        ("loop_lane_share.rs", "type HandshakeSteps = StepCons<"),
        (
            "loop_lane_share.rs",
            "type LoopSeq = <LoopContSteps as StepConcat<LoopBrkSteps>>::Output;",
        ),
        (
            "nested_route_runtime.rs",
            "type WorkerLocal = <ProtocolSteps as ProjectRole<Role<1>>>::Output;",
        ),
        (
            "offer_decode_binding_regression.rs",
            "type WorkerLocal = <ProtocolSteps as ProjectRole<Role<1>>>::Output;",
        ),
        (
            "offer_decode_binding_regression.rs",
            "type LeftControlStep = StepCons<",
        ),
        (
            "offer_decode_binding_regression.rs",
            "type ProtocolSteps = SeqSteps<RouteSteps, TailSteps>;",
        ),
        (
            "route_dynamic_control.rs",
            "type ControllerRouteLocal = <RouteSteps as ProjectRole<Role<0>>>::Output;",
        ),
        (
            "route_dynamic_control.rs",
            "type WorkerRouteLocal = <RouteSteps as ProjectRole<Role<1>>>::Output;",
        ),
        (
            "route_dynamic_control.rs",
            "type LoopControllerLocal = <LoopDecision as ProjectRole<Role<0>>>::Output;",
        ),
        (
            "route_dynamic_control.rs",
            "type NestedLoopControllerLocal = <NestedLoopSteps as ProjectRole<Role<0>>>::Output;",
        ),
        (
            "route_with_internal_loops.rs",
            "type ClientLocal = <RouteSteps as ProjectRole<Role<0>>>::Output;",
        ),
        (
            "route_with_internal_loops.rs",
            "type ServerLocal = <RouteSteps as ProjectRole<Role<1>>>::Output;",
        ),
        (
            "route_with_internal_loops.rs",
            "type ArmAMarkerStep = StepCons<",
        ),
        (
            "route_with_internal_loops.rs",
            "type RouteSteps = <ArmASteps as StepConcat<ArmBSteps>>::Output;",
        ),
        (
            "ui-pass/g-route-merged.rs",
            "type PassiveLocal = <Steps as ProjectRole<g::Role<1>>>::Output;",
        ),
        (
            "ui-pass/g-route-static-control-basic.rs",
            "type PassiveLocal = <Steps as ProjectRole<g::Role<1>>>::Output;",
        ),
        (
            "ui-pass/g-route-static-control-prefix-local.rs",
            "type PassiveLocal = <Steps as ProjectRole<g::Role<1>>>::Output;",
        ),
        (
            "ui-pass/g-route-static-control-prefix-send.rs",
            "type PassiveLocal = <Steps as ProjectRole<g::Role<1>>>::Output;",
        ),
        (
            "ui-pass/dynamic_route_defer_compiles.rs",
            "type PassiveLocal = <Steps as ProjectRole<g::Role<1>>>::Output;",
        ),
        (
            "ui/g-route-unprojectable.rs",
            "type PassiveLocal = <Steps as ProjectRole<g::Role<1>>>::Output;",
        ),
        (
            "ui/g-route-policy-mismatch.rs",
            "type ControllerLocal = <RouteSteps as ProjectRole<Role<0>>>::Output;",
        ),
        (
            "ui/g-route-policy-mismatch.rs",
            "type WithPolicyKind = ArmWithPolicyKind;",
        ),
        (
            "ui/g-route-policy-mismatch.rs",
            "type WithPolicySteps = StepCons<",
        ),
        (
            "ui/g-route-policy-mismatch.rs",
            "type RouteSteps = <WithPolicySteps as StepConcat<WithoutPolicySteps>>::Output;",
        ),
        (
            "ui/g-route-unprojectable.rs",
            "type Arm0ControlStep = StepCons<",
        ),
        (
            "ui/g-route-unprojectable.rs",
            "type Steps = <Arm0Steps as StepConcat<Arm1Steps>>::Output;",
        ),
        (
            "mgmt_epf_integration.rs",
            "type Cluster = SessionKit<'static, TestTransport, DefaultLabelUniverse, CounterClock, 4>;",
        ),
    ] {
        let src = std::fs::read_to_string(format!("{}/tests/{}", env!("CARGO_MANIFEST_DIR"), path))
            .unwrap_or_else(|err| panic!("read fixture {path} failed: {err}"));
        assert!(
            !src.contains(forbidden),
            "fixture {path} must not hide canonical owners behind a pure synonym alias: {forbidden}"
        );
    }
}

#[test]
fn internal_source_test_modules_do_not_hide_canonical_owners_behind_synonyms() {
    for (path, forbidden) in [
        ("global/const_dsl.rs", "type Sender = g::Role<0>;"),
        (
            "global/typestate.rs",
            "type ControllerLoopLocal = <LoopSteps<",
        ),
        ("global/role_program.rs", "type Client = Role<0>;"),
        ("endpoint/kernel/core.rs", "type HintController = Role<0>;"),
        (
            "control/cluster/effects.rs",
            "type Actor = crate::g::Role<0>;",
        ),
    ] {
        let src = std::fs::read_to_string(format!("{}/src/{}", env!("CARGO_MANIFEST_DIR"), path))
            .unwrap_or_else(|err| panic!("read source fixture {path} failed: {err}"));
        assert!(
            !src.contains(forbidden),
            "internal source test module {path} must not hide canonical owners behind an alias: {forbidden}"
        );
    }
}

#[test]
fn route_projection_regression_fixtures_keep_canonical_inputs_live() {
    let route_policy_mismatch = include_str!("ui/g-route-policy-mismatch.rs");
    let route_unprojectable = include_str!("ui/g-route-unprojectable.rs");
    let empty_arms = include_str!("ui/g-empty_arms.rs");
    let par_empty_lane = include_str!("ui/g-par-empty-lane.rs");

    assert!(
        route_policy_mismatch.contains("control_kinds::UnitControl<")
            && route_policy_mismatch.contains("type ArmWithPolicyKind =")
            && route_policy_mismatch.contains("type ArmWithoutPolicyKind ="),
        "g-route-policy-mismatch must define route-control fixtures through the shared explicit trait-impl helper"
    );
    assert!(
        route_policy_mismatch.contains("const CONTROLLER: RoleProgram<")
            && route_policy_mismatch.contains("> = project(&ROUTE);"),
        "g-route-policy-mismatch must force controller projection through a typed const RoleProgram"
    );
    assert!(
        route_policy_mismatch.contains(
            "Msg<5, GenericCapToken<ArmWithPolicyKind>, CanonicalControl<ArmWithPolicyKind>>"
        ) && route_policy_mismatch.contains(
            "Msg<6, GenericCapToken<ArmWithoutPolicyKind>, CanonicalControl<ArmWithoutPolicyKind>>"
        ),
        "g-route-policy-mismatch must keep concrete control-kind aliases wired into canonical control messages"
    );
    assert!(
        route_unprojectable.contains("control_kinds::RouteControl<")
            && route_unprojectable.contains("type RouteArm100Kind =")
            && route_unprojectable.contains("type RouteArm101Kind ="),
        "g-route-unprojectable must define route-control fixtures through the shared explicit trait-impl helper"
    );
    assert!(
        route_unprojectable.contains("static PASSIVE_PROGRAM: RoleProgram<")
            && route_unprojectable.contains("> = project(&ROUTE);"),
        "g-route-unprojectable must force passive projection through a typed static RoleProgram"
    );
    assert!(
        route_unprojectable.contains(
            "Msg<100, GenericCapToken<RouteArm100Kind>, CanonicalControl<RouteArm100Kind>>"
        ) && route_unprojectable.contains(
            "Msg<101, GenericCapToken<RouteArm101Kind>, CanonicalControl<RouteArm101Kind>>"
        ),
        "g-route-unprojectable must keep concrete control-kind aliases wired into canonical control messages"
    );
    assert!(
        route_unprojectable.contains("let _ = &PASSIVE_PROGRAM;"),
        "g-route-unprojectable must keep a direct use-site for the projected program so compile-fail coverage cannot silently evaporate"
    );
    assert!(
        empty_arms.contains("use hibana::g::advanced::steps::StepNil;")
            && empty_arms.contains("let _ = g::route(arm, StepNil::PROGRAM);"),
        "g-empty_arms must exercise the canonical g::route call site while keeping the substrate-only empty witness explicit"
    );
    assert!(
        !empty_arms.contains("g::route::<"),
        "g-empty_arms must not bypass the canonical route constructor with an explicit turbofish witness"
    );
    assert!(
        par_empty_lane.contains("use hibana::g::advanced::steps::StepNil;")
            && par_empty_lane.contains("let _ = g::par(StepNil::PROGRAM, StepNil::PROGRAM);"),
        "g-par-empty-lane must exercise the canonical g::par call site while keeping the substrate-only empty witness explicit"
    );
    assert!(
        !par_empty_lane.contains("g::par::<"),
        "g-par-empty-lane must not bypass the canonical parallel constructor with an explicit turbofish witness"
    );
}

#[test]
fn route_control_fixtures_use_shared_explicit_impl_helper() {
    for (path, src, required) in [
        (
            "route_dynamic_control.rs",
            include_str!("route_dynamic_control.rs"),
            "RouteRightKind",
        ),
        (
            "nested_route_runtime.rs",
            include_str!("nested_route_runtime.rs"),
            "RouteRightKind",
        ),
        (
            "offer_decode_binding_regression.rs",
            include_str!("offer_decode_binding_regression.rs"),
            "RouteRightKind",
        ),
        (
            "ui-pass/g-route-merged.rs",
            include_str!("ui-pass/g-route-merged.rs"),
            "RouteArm100Kind",
        ),
        (
            "ui-pass/g-route-static-control-basic.rs",
            include_str!("ui-pass/g-route-static-control-basic.rs"),
            "RouteArm100Kind",
        ),
        (
            "ui-pass/g-route-static-control-prefix-local.rs",
            include_str!("ui-pass/g-route-static-control-prefix-local.rs"),
            "RouteArm100Kind",
        ),
        (
            "ui-pass/g-route-static-control-prefix-send.rs",
            include_str!("ui-pass/g-route-static-control-prefix-send.rs"),
            "RouteArm100Kind",
        ),
        (
            "ui-pass/dynamic_route_defer_compiles.rs",
            include_str!("ui-pass/dynamic_route_defer_compiles.rs"),
            "RouteArm100Kind",
        ),
        (
            "ui/g-route-unprojectable.rs",
            include_str!("ui/g-route-unprojectable.rs"),
            "RouteArm100Kind",
        ),
    ] {
        assert!(
            src.contains("control_kinds::RouteControl<") && src.contains(required),
            "{path} must define route-control tokens through the shared explicit trait-impl helper"
        );
        for forbidden in [
            "struct RouteRightKind;",
            "struct RouteArmKind<const LABEL: u8>;",
            "impl ResourceKind for RouteRightKind",
            "impl<const LABEL: u8> ResourceKind for RouteArmKind<LABEL>",
        ] {
            assert!(
                !src.contains(forbidden),
                "{path} must not keep manual route-control ResourceKind boilerplate: {forbidden}"
            );
        }
    }
}

#[test]
fn ui_diagnostics_stay_semantic() {
    let route_policy_required = include_str!("ui/g-route-policy-required.stderr");
    let route_policy_mismatch = include_str!("ui/g-route-policy-mismatch.stderr");
    let empty_arms = include_str!("ui/g-empty_arms.stderr");
    let route_controller_mismatch = include_str!("ui/g-route-controller-mismatch.stderr");
    let duplicate_route_label = include_str!("ui/g-typed-route-duplicate-label.stderr");
    let route_unprojectable = include_str!("ui/g-route-unprojectable.stderr");
    let par_empty_lane = include_str!("ui/g-par-empty-lane.stderr");

    for stderr in [
        route_policy_required,
        route_policy_mismatch,
        empty_arms,
        route_controller_mismatch,
        duplicate_route_label,
        route_unprojectable,
    ] {
        assert!(
            !stderr.contains("BinaryRoutePair"),
            "route diagnostics must not leak BinaryRoutePair"
        );
        assert!(
            !stderr.contains("RouteHead"),
            "route diagnostics must not leak RouteHead"
        );
    }
    assert!(
        route_policy_required
            .contains("`g::route(left, right)` arms must begin with a controller self-send"),
        "route head-shape diagnostics must lead with route-arm semantics"
    );
    assert!(
        route_policy_mismatch
            .contains("route scope recorded conflicting controller policy annotations"),
        "route-policy-mismatch diagnostics must speak in controller policy annotation terms"
    );
    assert!(
        !route_policy_mismatch.contains("PolicyMode"),
        "route-policy-mismatch diagnostics must not leak PolicyMode internals"
    );
    assert!(
        empty_arms.contains("`g::route(left, right)` arms must begin with a controller self-send"),
        "empty-arm route diagnostics must lead with controller self-send semantics"
    );
    assert!(
        route_controller_mismatch
            .contains("`g::route(left, right)` arms must start with the same controller self-send"),
        "controller-mismatch diagnostics must lead with controller semantics"
    );
    assert!(
        duplicate_route_label.contains("error[E0277]"),
        "duplicate-route-label rejection must be a type error instead of const panic"
    );
    assert!(
        duplicate_route_label.contains("`g::route(left, right)` arms must use distinct labels"),
        "duplicate-route-label diagnostics must lead with distinct-label semantics"
    );
    assert!(
        !duplicate_route_label.contains("error[E0080]")
            && !duplicate_route_label.contains("duplicate route label"),
        "duplicate-route-label diagnostics must not fall back to const panic wording"
    );
    assert!(
        route_unprojectable.contains(
            "Route unprojectable for this role: arms not mergeable, wire dispatch non-deterministic, and no dynamic policy annotation provided"
        ),
        "unprojectable-route diagnostics must describe the missing public policy annotation instead of internal plan jargon"
    );
    assert!(
        !route_unprojectable.contains("PolicyMode"),
        "unprojectable-route diagnostics must not leak policy internals"
    );
    assert!(
        par_empty_lane.contains("`g::par(left, right)` arms must be non-empty protocol fragments"),
        "parallel empty-arm diagnostics must use semantic non-empty wording"
    );
    assert!(
        par_empty_lane.contains("hibana::global::NonEmptyParallelArm"),
        "parallel empty-arm diagnostics must name the semantic non-empty witness"
    );
    for forbidden in [
        "BinaryRoutePair",
        "RouteHead",
        "ParallelFragment",
        "StepNonEmpty",
        "steps::StepRoleSet",
        "RoleLaneSet",
    ] {
        assert!(
            !par_empty_lane.contains(forbidden),
            "parallel diagnostics must not leak stale internal route/parallel names: {forbidden}"
        );
    }
}

#[test]
fn dead_epoch_and_payload_helpers_do_not_regrow_through_public_surface() {
    let cap_src = include_str!("../src/control/cap.rs");
    let mint_src = include_str!("../src/control/cap/mint.rs");
    let substrate_src = include_str!("../src/substrate.rs");
    let ui_test_driver = include_str!("ui.rs");

    for required in ["pub struct E0;", "pub struct EpochTbl<"] {
        assert!(
            mint_src.contains(required),
            "mint owner must keep the live epoch witness owners needed by endpoint typestate: {required}"
        );
    }
    for forbidden in [
        "pub trait MaySend: EpochStep {}",
        "impl MaySend for E0 {}",
        "pub enum VmHandleError {",
    ] {
        assert!(
            !mint_src.contains(forbidden),
            "dead epoch/VM helper must stay deleted from the mint owner: {forbidden}"
        );
    }
    assert!(
        !cap_src.contains("pub(crate) mod payload;"),
        "cap root must not keep the dead payload helper module"
    );

    for forbidden in [
        "pub mod payload {",
        "pub mod token {",
        "ControlHandle;",
        "CAP_FIXED_HEADER_LEN,",
        "CAP_HEADER_LEN,",
        "CAP_NONCE_LEN,",
        "CAP_TAG_LEN,",
        "CAP_TOKEN_LEN,",
        "E0,",
        "HandleView,",
        "MaySend,",
        "ScopeEvent,",
        ", ScopeKind};",
        "VmHandleError,",
    ] {
        assert!(
            !substrate_src.contains(forbidden),
            "substrate::cap::advanced must not leak lower-layer epoch/token/payload helpers: {forbidden}"
        );
    }

    assert!(
        !ui_test_driver.contains("control-crash-next-non-send.rs"),
        "ui trybuild driver must not keep the stale crash-next non-send fixture once epoch witnesses are internal-only"
    );
}

#[test]
fn public_seq_preserves_segment_boundary() {
    let global_src = include_str!("../src/global.rs");
    let program_src = include_str!("../src/global/program.rs");

    assert!(
        global_src.contains(") -> Program<SeqSteps<LeftSteps, RightSteps>> {"),
        "public g::seq must preserve segment boundaries with SeqSteps"
    );
    assert!(
        program_src.contains(") -> Program<SeqSteps<LeftSteps, RightSteps>> {"),
        "program::seq must build preserved SeqSteps witnesses"
    );
    assert!(
        !global_src.contains("left.then(right)"),
        "public g::seq must not flatten composition through Program::then"
    );
    assert!(
        global_src.contains(
            ") -> Program<SeqSteps<LeftSteps, RightSteps>> {\n    program::seq(left, right)\n}"
        ),
        "public g::seq must delegate to the preserved program::seq constructor directly"
    );
}

#[test]
fn program_builder_helpers_are_not_public_surface() {
    let program_src = include_str!("../src/global/program.rs");

    for forbidden in [
        "pub const fn empty() -> Self",
        "pub const fn build() -> Self",
        "pub const fn eff_list(&self) -> &EffList",
        "pub const fn into_eff(self) -> EffList",
        "pub const fn scope_budget(&self) -> u16",
        "pub const fn then<NextSteps>(",
    ] {
        assert!(
            !program_src.contains(forbidden),
            "Program must not expose hidden builder/introspection helpers publicly: {forbidden}"
        );
    }

    for required in [
        "pub(crate) const fn empty() -> Self",
        "pub(crate) const fn build() -> Self",
        "pub(crate) const fn eff_list(&self) -> &EffList",
        "const fn into_eff(self) -> EffList",
        "const fn scope_budget(&self) -> u16",
        "pub(crate) const fn then<NextSteps>(",
        "pub const fn policy<const POLICY_ID: u16>(self) -> Program<PolicySteps<Steps, POLICY_ID>>",
    ] {
        assert!(
            program_src.contains(required),
            "Program helper visibility must stay hidden while policy remains public: {required}"
        );
    }
}

#[test]
fn binding_slot_keeps_lane_identity_canonical() {
    let binding_src = include_str!("../src/binding.rs");
    let binding_ws = compact_ws(binding_src);

    assert!(
        !binding_src.contains("fn map_lane("),
        "BindingSlot must not expose lane remapping"
    );
    assert!(
        !binding_src.contains("physical lane"),
        "BindingSlot docs must not teach logical/physical lane translation"
    );
    assert!(
        !binding_src.contains("pub enum ChannelStoreError"),
        "binding surface must not reintroduce a second channel-store error type"
    );
    assert!(
        binding_ws
            .contains("fn policy_signals_provider(&self) -> Option<&dyn PolicySignalsProvider>;"),
        "BindingSlot must require an explicit policy-signals owner hook"
    );
    assert!(
        !binding_ws.contains(
            "pub unsafe trait BindingSlot { fn on_send_with_meta(&mut self, meta: SendMetadata, payload: &[u8]) -> Result<SendDisposition, TransportOpsError>; fn poll_incoming_for_lane(&mut self, logical_lane: u8) -> Option<IncomingClassification>; fn on_recv(&mut self, channel: Channel, buf: &mut [u8]) -> Result<usize, TransportOpsError>; fn policy_signals_provider(&self) -> Option<&dyn PolicySignalsProvider> { None } }"
        ),
        "BindingSlot must not keep a default zero-signals fallback hook"
    );
}

#[test]
fn runtime_config_does_not_expose_kernel_liveness_knobs() {
    let config_src = include_str!("../src/runtime/config.rs");

    for forbidden in [
        "pub struct LivenessPolicy {",
        "pub fn with_liveness_policy(mut self, policy: LivenessPolicy) -> Self {",
        "pub fn liveness_policy(&self) -> LivenessPolicy {",
        "pub fn enable_global_tap(mut self) -> Self {",
        "pub(crate) fn with_liveness_policy(mut self, policy: LivenessPolicy) -> Self {",
        "pub(crate) fn liveness_policy(&self) -> LivenessPolicy {",
        "pub(crate) fn enable_global_tap(mut self) -> Self {",
        "global_tap: bool,",
    ] {
        assert!(
            !config_src.contains(forbidden),
            "runtime::Config must not expose internal kernel knobs publicly: {forbidden}"
        );
    }

    assert!(
        config_src.contains("pub(crate) struct LivenessPolicy {"),
        "runtime::Config should keep the canonical lower-layer liveness policy owner crate-private"
    );
}

#[test]
fn rendezvous_does_not_keep_dead_global_tap_registration() {
    let rendezvous_core_src = include_str!("../src/rendezvous/core.rs");

    for forbidden in [
        "tap_registered: bool,",
        "fn register_tap(&mut self) {",
        "fn unregister_tap(&mut self) {",
        "global_tap,",
        "rendezvous.register_tap();",
        "self.unregister_tap();",
    ] {
        assert!(
            !rendezvous_core_src.contains(forbidden),
            "Rendezvous must not keep the deleted global-tap registration path: {forbidden}"
        );
    }
}

#[test]
fn substrate_binding_and_epf_surface_use_canonical_names() {
    let substrate_src = include_str!("../src/substrate.rs");
    let substrate_compact = compact_ws(substrate_src);
    let allowlist = substrate_public_api_allowlist();
    let epf_block_start = allowlist
        .find("pub mod epf {")
        .expect("substrate public API allowlist must keep the epf bucket");
    let cap_block_start = allowlist
        .find("pub mod cap {")
        .expect("substrate public API allowlist must keep the cap bucket");
    let wire_block_start = allowlist
        .find("pub mod wire {")
        .expect("substrate public API allowlist must keep the wire bucket");
    let epf_block = &allowlist[epf_block_start..cap_block_start];
    let cap_block = &allowlist[cap_block_start..wire_block_start];

    for forbidden in [
        "pub use crate::control::types::{LaneId as Lane, RendezvousId, SessionId};",
        "BindingSlot as Binding",
        "IncomingClassification as Incoming",
        "NoBinding as NullBinding",
        "Outgoing as SendEnvelope",
        "pub mod advanced {\n        pub use crate::binding::ArrayChannelStore;",
        "pub use crate::control::cap::resource_kinds;",
        "pub use crate::epf::Slot;",
        "Slot as VmSlot",
        "Header as VmHeader",
        "pub const fn new(sid: u32, lane: u16, scope: ScopeId) -> Self",
    ] {
        assert!(
            !substrate_src.contains(forbidden),
            "substrate surface must not keep alias-only public names: {forbidden}"
        );
    }

    assert!(
        substrate_src.contains("pub use crate::control::types::{Lane, RendezvousId, SessionId};"),
        "substrate surface must expose the canonical Lane newtype directly"
    );
    assert!(
        substrate_src.contains("pub use crate::eff::EffIndex;"),
        "substrate surface must expose the canonical effect index owner used by ResolverContext and transport::SendMeta"
    );
    for forbidden in [
        "pub fn len(&self) -> usize {",
        "pub fn is_empty(&self) -> bool {",
    ] {
        assert!(
            !substrate_src.contains(forbidden),
            "SessionKit must not expose non-canonical collection helpers on the substrate surface: {forbidden}"
        );
    }
    assert!(
        epf_block.contains("pub use Slot;")
            && !allowlist[..epf_block_start].contains("pub use Slot;")
            && !allowlist[cap_block_start..].contains("pub use Slot;"),
        "substrate public API must keep Slot only inside the dedicated policy::epf bucket"
    );
    assert!(
        cap_block.contains("pub use {One, Many};")
            && !allowlist[..cap_block_start].contains("pub use {One, Many};")
            && !allowlist[wire_block_start..].contains("pub use {One, Many};"),
        "substrate public API must keep One/Many only inside the dedicated cap bucket"
    );
    for forbidden in [
        "pub use crate::epf::verifier::compute_hash;",
        "pub use crate::epf::{",
        "hash_policy_input,",
        "hash_tap_event,",
        "hash_transport_snapshot,",
        "policy_mode_tag,",
        "run_with,",
        "slot_tag,",
        "verdict_arm,",
        "verdict_reason,",
        "verdict_tag,",
    ] {
        assert!(
            !substrate_src.contains(forbidden),
            "substrate EPF audit surface must not re-export internal EPF helper functions: {forbidden}"
        );
    }
    for forbidden in [
        "pub fn with_mem( code: &'arena [u8], scratch: &'arena mut [u8], mem_len: usize, fuel_max: u16, ) -> Result<Self, HostError>",
        "pub fn run_with<F>(",
        "pub struct RunConfig {",
        "pub fn new(slot: super::Slot, event: &'a TapEvent, caps: CapsMask) -> Self",
        "pub fn set_policy_input(&mut self, input: [u32; 4])",
        "pub fn set_transport_snapshot(&mut self, snapshot: TransportSnapshot)",
        "pub struct MachineConfig<'arena> {",
        "pub struct HostSlots<'arena> {",
        "pub struct RunRequest<'a> {",
        "pub fn with_mem( code: &'arena [u8], config: MachineConfig<'arena>, ) -> Result<Self, HostError>",
        "pub fn new(event: &'a TapEvent) -> Self",
        "pub fn with_slot(mut self, slot: super::Slot) -> Self",
        "pub fn with_caps(mut self, caps: CapsMask) -> Self",
        "pub fn run(host_slots: &HostSlots<'_>, request: RunRequest<'_>) -> Action",
        "pub mod audit {",
    ] {
        assert!(
            !substrate_compact.contains(forbidden),
            "substrate::policy::epf must not keep stale replay helper wrappers: {forbidden}"
        );
    }
}

#[test]
fn resolver_context_surface_stays_accessor_only() {
    let cluster_core_src = include_str!("../src/control/cluster/core.rs");
    let cluster_core_compact = compact_ws(cluster_core_src);
    let resolver_impl = impl_body(cluster_core_src, "impl ResolverContext {");
    let substrate_src = include_str!("../src/substrate.rs");

    assert!(
        cluster_core_src.contains("pub struct ResolverContext {"),
        "ResolverContext must stay public as the substrate resolver callback input"
    );
    assert!(
        !cluster_core_compact.contains("pub fn new( rv_id: RendezvousId, session: Option<SessionId>, lane: Lane, eff_index: EffIndex, tag: u8, metrics: TransportSnapshot, scope_id: ScopeId, scope_trace: Option<ScopeTrace>, input: [u32; 4], attrs: crate::transport::context::PolicyAttrs, ) -> Self"),
        "ResolverContext must not expose a public constructor"
    );
    let mut public_methods = [""; 4];
    let mut public_method_len = 0usize;
    for line in resolver_impl.lines().map(str::trim_start) {
        if !(line.starts_with("pub fn ")
            || line.starts_with("pub const fn ")
            || line.starts_with("pub async fn ")
            || line.starts_with("pub unsafe fn "))
        {
            continue;
        }
        let method = line
            .split("fn ")
            .nth(1)
            .and_then(|rest| rest.split('(').next())
            .expect("ResolverContext public method name");
        assert!(
            public_method_len < public_methods.len(),
            "ResolverContext must keep a tiny public method surface"
        );
        public_methods[public_method_len] = method;
        public_method_len += 1;
    }
    assert_eq!(
        &public_methods[..public_method_len],
        ["attr", "input"],
        "ResolverContext must keep exactly the canonical attr()/input() public accessors"
    );
    for required in ["pub fn attr(", "pub fn input(&self, idx: u8) -> u32"] {
        assert!(
            cluster_core_src.contains(required),
            "ResolverContext must keep the minimal public accessor surface: {required}"
        );
    }
    assert!(
        cluster_core_src.contains("pub enum ResolverError {"),
        "substrate resolver surface must expose a semantic ResolverError owner"
    );
    assert!(
        substrate_src.contains("DynamicResolution, ResolverContext, ResolverError, ResolverRef"),
        "substrate policy surface must re-export ResolverRef next to the resolver callback contracts"
    );
    assert!(
        !substrate_src.contains("Result<crate::control::cluster::core::DynamicResolution, ()>"),
        "substrate set_resolver surface must not hide resolver failure behind unit error shorthand"
    );
}

#[test]
fn observe_root_does_not_expose_cap_event_id_helpers() {
    let observe_src = include_str!("../src/observe.rs");

    for forbidden in [
        "pub(crate) const fn cap_mint_id(",
        "pub(crate) const fn cap_claim_id(",
        "pub(crate) const fn cap_exhaust_id(",
        "pub const fn cap_mint<",
        "pub const fn cap_claim<",
        "pub const fn cap_exhaust<",
    ] {
        assert!(
            !observe_src.contains(forbidden),
            "observe root must not expose internal cap event-id helpers: {forbidden}"
        );
    }

    for required in [
        "pub(crate) const fn cap_mint<",
        "pub(crate) const fn cap_claim<",
        "pub(crate) const fn cap_exhaust<",
    ] {
        assert!(
            observe_src.contains(required),
            "cap event-id helpers should stay crate-private: {required}"
        );
    }
}

#[test]
fn private_owners_do_not_keep_internal_helpers_public() {
    let effects_src = include_str!("../src/control/cluster/effects.rs");
    let scope_src = include_str!("../src/observe/scope.rs");
    let normalise_src = include_str!("../src/observe/normalise.rs");
    let observe_core_src = include_str!("../src/observe/core.rs");
    let check_src = include_str!("../src/observe/check.rs");
    let local_src = include_str!("../src/observe/local.rs");
    let epf_src = include_str!("../src/epf.rs");
    let loader_src = include_str!("../src/epf/loader.rs");
    let verifier_src = include_str!("../src/epf/verifier.rs");
    let slot_contract_src = include_str!("../src/epf/slot_contract.rs");
    let resource_kinds_src = include_str!("../src/control/cap/resource_kinds.rs");
    let capability_src = include_str!("../src/rendezvous/capability.rs");
    let dispatch_src = include_str!("../src/epf/dispatch.rs");
    let delegation_src = include_str!("../src/control/automaton/delegation.rs");
    let splice_src = include_str!("../src/control/automaton/splice.rs");
    let rendezvous_core_src = include_str!("../src/rendezvous/core.rs");
    let slots_src = include_str!("../src/rendezvous/slots.rs");
    let planner_src = include_str!("../src/control/lease/planner.rs");
    let typestate_src = include_str!("../src/global/typestate.rs");
    let const_dsl_src = include_str!("../src/global/const_dsl.rs");

    assert!(
        !effects_src.contains("interpret_eff_list("),
        "cluster effects owner must not keep the legacy interpret_eff_list shim outside lowering owners"
    );
    for forbidden in [
        "pub struct CapEntry {",
        "pub struct CapTable {",
        "pub const fn new() -> Self {",
    ] {
        assert!(
            !capability_src.contains(forbidden),
            "rendezvous capability owner must not stay public: {forbidden}"
        );
    }
    for required in [
        "pub(crate) struct CapEntry {",
        "pub(crate) struct CapTable {",
        "pub(crate) fn new() -> Self {",
    ] {
        assert!(
            capability_src.contains(required),
            "rendezvous capability owner should stay crate-private: {required}"
        );
    }
    for forbidden in [
        "pub struct EffectEnvelope {",
        "pub struct ResourceDescriptor {",
        "pub const MAX_CP_EFFECTS: usize =",
        "pub const MAX_TAP_EVENTS: usize =",
        "pub const MAX_RESOURCES: usize =",
        "pub const MAX_SCOPES: usize =",
        "pub const MAX_CONTROLS: usize =",
    ] {
        assert!(
            !effects_src.contains(forbidden),
            "private cluster effects owner must not keep control-envelope internals public: {forbidden}"
        );
    }
    for required in [
        "pub(crate) struct EffectEnvelope {",
        "pub(crate) struct ResourceDescriptor {",
        "pub(crate) fn cp_effects(&self) -> impl Iterator<Item = &CpEffect> {",
        "pub(crate) fn resources(&self) -> impl Iterator<Item = &ResourceDescriptor> {",
        "pub(crate) fn control_scopes(&self) -> impl Iterator<Item = ControlScopeKind> {",
    ] {
        assert!(
            effects_src.contains(required),
            "private cluster effects owner should stay crate-private: {required}"
        );
    }

    assert!(
        !scope_src.contains("pub fn tap_scope("),
        "private observe scope owner must not expose tap_scope publicly"
    );
    assert!(
        scope_src.contains("pub(crate) fn tap_scope("),
        "tap_scope should stay crate-private"
    );

    for forbidden in [
        "pub(crate) enum ForwardEvent",
        "pub(crate) enum DelegationEvent",
        "pub(crate) struct SloBreachEvent",
        "pub(crate) enum EndpointEvent",
        "pub(crate) enum TransportTapEventKind",
        "pub(crate) struct TransportTapEvent",
        "pub(crate) struct TransportMetricsTapEvent",
        "pub(crate) fn delegation_trace(",
        "pub(crate) fn slo_breach_trace(",
        "pub(crate) fn forward_trace(",
        "pub(crate) fn transport_trace(",
        "pub(crate) fn transport_metrics_trace(",
        "pub fn from_tap(event: TapEvent) -> Option<Self>",
        "pub fn from_tap_pair(main: TapEvent, extension: Option<TapEvent>) -> Option<Self>",
        "pub fn sid(&self) -> u32",
        "pub fn lane(&self) -> u8",
        "pub fn role(&self) -> u8",
        "pub fn label(&self) -> u8",
    ] {
        assert!(
            !normalise_src.contains(forbidden),
            "observe normalise helpers should stay file-local: {forbidden}"
        );
    }

    for forbidden in [
        "pub(crate) struct LocalActionFailure",
        "pub(crate) fn from_tap(event: TapEvent) -> Option<Self>",
        "pub(crate) fn feed(event: TapEvent)",
        "pub(crate) struct PolicyEventSpec",
        "pub(crate) fn policy_event_spec(id: u16) -> Option<PolicyEventSpec>",
        "pub(crate) fn sid_hint_from_tap(self, event: TapEvent) -> Option<u32>",
        "pub struct TapBatch {",
        "pub enum PolicyEventKind {",
        "pub struct PolicyEvent {",
        "pub struct TapRing<'a> {",
        "pub fn push(event: TapEvent)",
        "pub fn emit(ring: &TapRing<'_>, event: TapEvent)",
        "pub fn head() -> Option<usize>",
        "pub fn install_ts_checker(checker: Option<fn(u32)>) -> Option<fn(u32)>",
        "pub fn install_ring(ring: &'static TapRing<'static>) -> Option<&'static TapRing<'static>>",
        "pub fn uninstall_ring(ring: *const TapRing<'static>) -> bool",
        "pub fn uninstall_ring(ring: &'static TapRing<'static>) -> bool",
        "pub unsafe fn assume_static(&self) -> &'static TapRing<'static>",
        "pub unsafe fn as_static_ptr(&self) -> *const TapRing<'static>",
        "pub(crate) fn head() -> Option<usize>",
    ] {
        assert!(
            !observe_core_src.contains(forbidden)
                && !check_src.contains(forbidden)
                && !local_src.contains(forbidden),
            "observe subtree must not keep sibling-only helpers crate-visible: {forbidden}"
        );
    }
    for required in [
        "pub(super) struct LocalActionFailure",
        "pub(super) fn from_tap(event: TapEvent) -> Option<Self>",
        "pub(super) fn feed(event: TapEvent)",
        "pub(super) struct PolicyEventSpec",
        "pub(super) fn policy_event_spec(id: u16) -> Option<PolicyEventSpec>",
        "pub(super) fn sid_hint_from_tap(self, event: TapEvent) -> Option<u32>",
        "pub(crate) enum PolicyEventKind {",
        "pub(crate) fn push(event: TapEvent)",
        "pub(crate) fn emit(ring: &TapRing<'_>, event: TapEvent)",
        "pub(crate) fn install_ts_checker(checker: Option<fn(u32)>) -> Option<fn(u32)>",
        "pub(crate) struct TapRing<'a> {",
        "#[cfg(test)]\npub(crate) fn install_ring(ring: &'static TapRing<'static>) -> Option<&'static TapRing<'static>>",
        "#[cfg(test)]\npub(crate) fn uninstall_ring(ring: &'static TapRing<'static>) -> bool",
        "#[cfg(test)]\n    pub(crate) unsafe fn assume_static(&self) -> &'static TapRing<'static>",
        "#[cfg(test)]\nstruct TapEvents<'cursor, 'ring, T, F>",
        "#[cfg(test)]\n    fn events_since<'cursor, T, F>(",
        "#[cfg(test)]\n    pub(crate) fn events_since<'cursor, T, F>(",
    ] {
        assert!(
            observe_core_src.contains(required)
                || check_src.contains(required)
                || local_src.contains(required),
            "observe subtree helper should stay sibling-visible only: {required}"
        );
    }

    for forbidden in ["pub struct WakerSlot {", "pub struct RingBuffer<'a> {"] {
        assert!(
            !observe_core_src.contains(forbidden),
            "observe ring lower-layer helpers must not stay public: {forbidden}"
        );
    }
    let legacy_atomic_ts_checker = ["static TS_CHECKER: ", "Atom", "icUsize"].concat();
    for forbidden in ["transmute::<usize, fn(u32)>", &legacy_atomic_ts_checker] {
        assert!(
            !observe_core_src.contains(forbidden),
            "observe timestamp checker must not hide function pointers behind integer transmute shims: {forbidden}"
        );
    }
    for required in [
        "static TS_CHECKER: Cell<Option<fn(u32)>> = const { Cell::new(None) };",
        "TS_CHECKER.with(Cell::get)",
        "checker.set(new);",
    ] {
        assert!(
            observe_core_src.contains(required),
            "observe timestamp checker must use the direct function-pointer owner path: {required}"
        );
    }

    assert!(
        !resource_kinds_src.contains("pub fn caps_mask_from_tag("),
        "resource kind owner must not expose internal caps-mask decoding publicly"
    );
    assert!(
        resource_kinds_src.contains("pub(crate) fn caps_mask_from_tag("),
        "caps_mask_from_tag should stay crate-private"
    );
    assert!(
        !resource_kinds_src.contains("fn new(scope: ScopeId, arm: u8) -> Self"),
        "RouteDecisionHandle must not keep a redundant constructor when its public fields already define the value"
    );
    for forbidden in [
        "fn new(\n        src_rv: u16,\n        dst_rv: u16,\n        src_lane: u16,\n        dst_lane: u16,\n        old_gen: u16,\n        new_gen: u16,\n        seq_tx: u32,\n        seq_rx: u32,\n        flags: u16,\n    ) -> Self",
        "fn new(\n        src_rv: u16,\n        dst_rv: u16,\n        src_lane: u16,\n        dst_lane: u16,\n        seq_tx: u32,\n        seq_rx: u32,\n        shard: u32,\n        flags: u16,\n    ) -> Self",
        "SpliceHandle::new(",
        "RerouteHandle::new(",
    ] {
        assert!(
            !resource_kinds_src.contains(forbidden),
            "handle payload owners must not keep multi-argument convenience constructors or call sites: {forbidden}"
        );
    }

    for forbidden in [
        "pub enum LoaderError",
        "pub struct ImageLoader",
        "pub const fn new() -> Self",
        "pub fn begin(&mut self, header: Header) -> Result<(), LoaderError>",
        "pub fn write(&mut self, offset: u32, chunk: &[u8]) -> Result<(), LoaderError>",
        "pub fn commit_for_slot(&mut self, slot: Slot) -> Result<VerifiedImage<'_>, LoaderError>",
        "pub enum VerifyError",
        "pub struct VerifiedImage<'a> {",
        "pub const MAX_CODE_LEN: usize = 2048;",
        "pub fn new(bytes: &'a [u8]) -> Result<Self, VerifyError>",
        "pub fn new_for_slot(bytes: &'a [u8], slot: Slot) -> Result<Self, VerifyError>",
        "pub const fn policy_mode_tag(",
        "pub const fn verdict_tag(",
        "pub const fn verdict_arm(",
        "pub const fn verdict_reason(",
        "pub const fn slot_tag(",
        "pub fn hash_tap_event(",
        "pub fn hash_policy_input(",
        "pub fn hash_transport_snapshot(",
        "pub fn run_with<",
        "pub fn compute_hash(code: &[u8]) -> u32",
    ] {
        assert!(
            !epf_src.contains(forbidden)
                && !loader_src.contains(forbidden)
                && !verifier_src.contains(forbidden),
            "private EPF owner must not keep internal helpers public: {forbidden}"
        );
    }
    for required in [
        "pub(crate) struct VerifiedImage<'a> {",
        "pub(crate) const MAX_CODE_LEN: usize = 2048;",
        "pub(crate) const fn policy_mode_tag(",
        "pub(crate) const fn verdict_tag(",
        "pub(crate) const fn verdict_arm(",
        "pub(crate) const fn verdict_reason(",
        "pub(crate) const fn slot_tag(",
        "pub(crate) fn hash_tap_event(",
        "pub(crate) fn hash_policy_input(",
        "pub(crate) fn hash_transport_snapshot(",
        "pub(crate) fn run_with<",
    ] {
        assert!(
            epf_src.contains(required) || verifier_src.contains(required),
            "EPF helper should stay crate-private: {required}"
        );
    }
    assert!(
        epf_src.contains("#[cfg(test)]\npub(crate) mod loader;"),
        "EPF loader must stay cfg(test)-scoped after warning-free cleanup"
    );
    for required in [
        "#[cfg(test)]\n#[derive(Clone, Copy, Debug, PartialEq, Eq)]\npub(crate) enum VerifyError",
        "#[cfg(test)]\n    pub(crate) fn new(bytes: &'a [u8]) -> Result<Self, VerifyError>",
        "#[cfg(test)]\n    pub(crate) fn new_for_slot(bytes: &'a [u8], slot: Slot) -> Result<Self, VerifyError>",
        "#[cfg(test)]\npub(crate) fn compute_hash(code: &[u8]) -> u32",
        "#[cfg(test)]\n    pub(crate) fn parse(bytes: &[u8]) -> Result<Self, VerifyError>",
        "#[cfg(test)]\n    pub(crate) fn encode_into(&self, buf: &mut [u8; Self::SIZE])",
    ] {
        assert!(
            loader_src.contains(required) || verifier_src.contains(required),
            "EPF test-support helper must stay cfg(test)-scoped: {required}"
        );
    }
    for required in [
        "pub(crate) enum LoaderError",
        "pub(crate) struct ImageLoader",
        "pub(crate) const fn new() -> Self",
        "pub(crate) fn begin(&mut self, header: Header) -> Result<(), LoaderError>",
        "pub(crate) fn write(&mut self, offset: u32, chunk: &[u8]) -> Result<(), LoaderError>",
        "pub(crate) fn commit_for_slot(&mut self, slot: Slot) -> Result<VerifiedImage<'_>, LoaderError>",
    ] {
        assert!(
            loader_src.contains(required),
            "loader internals must stay crate-private under the cfg(test) loader module: {required}"
        );
    }
    for required in [
        "pub struct Header {",
        "pub const MAGIC: [u8; 4]",
        "pub const SIZE: usize",
    ] {
        assert!(
            verifier_src.contains(required),
            "substrate EPF surface still depends on the canonical public Header owner: {required}"
        );
    }
    for required in [
        "#[cfg(test)]\n    pub const fn max_mem_len() -> usize",
        "#[cfg(test)]\n    pub(crate) fn parse(bytes: &[u8]) -> Result<Self, VerifyError>",
        "#[cfg(test)]\n    pub(crate) fn encode_into(&self, buf: &mut [u8; Self::SIZE])",
    ] {
        assert!(
            verifier_src.contains(required),
            "Header parsing/encoding should stay verifier-internal and cfg(test)-scoped: {required}"
        );
    }

    for forbidden in [
        "pub struct SlotPolicyContract",
        "pub enum SlotPolicySource",
        "pub const fn slot_policy_contract(",
        "pub const fn slot_allows_get_input(",
        "pub const fn slot_allows_mem_ops(",
        "pub const fn slot_default_input(",
        "pub const fn facets_caps(",
        "pub const fn facets_slots(",
        "pub const fn facets_caps_splice(",
        "pub const fn facets_caps_delegation(",
        "pub const fn assert_budget_covers(",
        "pub const fn facet_needs(",
        "pub const fn assert_program_covers_facets(",
        "pub const fn include_atom(",
        "pub const fn state_index_to_usize(",
        "pub const fn const_send_typed<",
    ] {
        assert!(
            !slot_contract_src.contains(forbidden)
                && !planner_src.contains(forbidden)
                && !typestate_src.contains(forbidden)
                && !const_dsl_src.contains(forbidden),
            "private owner must not keep internal const helpers public: {forbidden}"
        );
    }
    for required in [
        "pub(crate) struct SlotPolicyContract",
        "pub(crate) enum SlotPolicySource",
        "pub(crate) const fn slot_policy_contract(",
        "pub(crate) const fn slot_default_input(",
        "pub(crate) const fn facets_caps(",
        "pub(crate) const fn facets_slots(",
        "pub(crate) const fn facets_caps_splice(",
        "pub(crate) const fn facets_caps_delegation(",
        "pub(crate) const fn assert_budget_covers(",
        "pub(crate) const fn facet_needs(",
        "pub(crate) const fn assert_program_covers_facets<",
        "pub(crate) const fn include_atom(",
        "pub(crate) const fn state_index_to_usize(",
        "pub(crate) const fn const_send_typed<",
    ] {
        assert!(
            slot_contract_src.contains(required)
                || planner_src.contains(required)
                || typestate_src.contains(required)
                || const_dsl_src.contains(required),
            "internal const helper should stay crate-private: {required}"
        );
    }
    for required in [
        "#[cfg(test)]\npub(crate) const fn slot_allows_get_input(",
        "#[cfg(test)]\npub(crate) const fn slot_allows_mem_ops(",
    ] {
        assert!(
            slot_contract_src.contains(required),
            "slot-contract verifier helpers must stay cfg(test)-scoped: {required}"
        );
    }

    for forbidden in [
        "pub fn decode_effect_call(",
        "pub fn ensure_allowed(",
        "pub const DELEGATION_LEASE_MAX_NODES",
        "pub const DELEGATION_LEASE_MAX_CHILDREN",
        "pub const SPLICE_LEASE_MAX_NODES",
        "pub const SPLICE_LEASE_MAX_CHILDREN",
        "pub const SLOT_COUNT",
        "pub const CODE_MAX",
        "pub const SCRATCH_MAX",
        "pub const fn slot_index(",
    ] {
        assert!(
            !dispatch_src.contains(forbidden)
                && !delegation_src.contains(forbidden)
                && !splice_src.contains(forbidden)
                && !slots_src.contains(forbidden),
            "private owner must not keep internal helpers public: {forbidden}"
        );
    }

    for required in [
        "pub(crate) fn decode_effect_call(",
        "pub(crate) fn ensure_allowed(",
        "pub(crate) const DELEGATION_LEASE_MAX_NODES",
        "pub(crate) const DELEGATION_LEASE_MAX_CHILDREN",
        "pub(crate) const SPLICE_LEASE_MAX_NODES",
        "pub(crate) const SPLICE_LEASE_MAX_CHILDREN",
        "pub(crate) const SLOT_COUNT",
        "pub(crate) const CODE_MAX",
        "pub(crate) const SCRATCH_MAX",
        "pub(crate) const fn slot_index(",
    ] {
        assert!(
            dispatch_src.contains(required)
                || delegation_src.contains(required)
                || splice_src.contains(required)
                || slots_src.contains(required),
            "internal helper should stay crate-private: {required}"
        );
    }

    for forbidden in [
        "pub enum LocalAction {",
        "pub struct LocalNode {",
        "pub struct ScopeRegion {",
        "pub struct ScopeRecord {",
        "pub struct RecvMeta {",
        "pub struct LocalMeta {",
        "pub const fn node(&self, index: usize) -> LocalNode {",
        "pub fn typestate_node(&self, index: usize) -> LocalNode {",
        "pub fn scope_region(&self) -> Option<ScopeRegion> {",
        "pub fn scope_region_by_id(&self, scope_id: ScopeId) -> Option<ScopeRegion> {",
        "pub struct ScopeRegionIter<'a> {",
        "pub fn nodes(&self) -> &[LocalNode] {",
        "pub fn scope_regions(&self) -> ScopeRegionIter<'_> {",
        "pub fn enclosing_scope_of_kind(&self, kind: ScopeKind) -> Option<ScopeRegion> {",
        "pub(crate) struct ScopeRegionIter<'a> {",
        "pub(crate) fn nodes(&self) -> &[LocalNode] {",
        "pub(crate) fn scope_regions(&self) -> ScopeRegionIter<'_> {",
        "pub(crate) fn enclosing_scope_of_kind(&self, kind: ScopeKind) -> Option<ScopeRegion> {",
        "pub fn route_scope_controller_policy(",
        "pub fn try_send_meta(&self) -> Option<SendMeta>",
        "pub fn try_recv_meta(&self) -> Option<RecvMeta>",
        "pub fn try_local_meta(&self) -> Option<LocalMeta>",
        "pub fn expect_send_meta(&self) -> SendMeta",
        "pub fn expect_recv_meta(&self) -> RecvMeta",
        "pub fn expect_local_meta(&self) -> LocalMeta",
        "pub(crate) fn expect_send_meta(&self) -> SendMeta",
        "pub(crate) fn expect_recv_meta(&self) -> RecvMeta",
        "pub(crate) fn expect_local_meta(&self) -> LocalMeta",
    ] {
        assert!(
            !typestate_src.contains(forbidden),
            "private typestate owner must not expose policy-bearing metadata publicly: {forbidden}"
        );
    }
    for required in [
        "pub(crate) enum LocalAction {",
        "pub(crate) struct LocalNode {",
        "pub(crate) struct ScopeRegion {",
        "pub(crate) struct ScopeRecord {",
        "pub struct SendMeta {",
        "pub(crate) struct RecvMeta {",
        "pub(crate) struct LocalMeta {",
        "pub(crate) const fn node(&self, index: usize) -> LocalNode {",
        "pub(crate) fn typestate_node(&self, index: usize) -> LocalNode {",
        "pub(crate) fn scope_region(&self) -> Option<ScopeRegion> {",
        "pub(crate) fn scope_region_by_id(&self, scope_id: ScopeId) -> Option<ScopeRegion> {",
        "pub(crate) fn route_scope_controller_policy(",
        "pub(crate) fn try_send_meta(&self) -> Option<SendMeta>",
        "pub(crate) fn try_recv_meta(&self) -> Option<RecvMeta>",
        "pub(crate) fn try_local_meta(&self) -> Option<LocalMeta>",
    ] {
        assert!(
            typestate_src.contains(required),
            "private typestate owner should keep policy-bearing metadata crate-private: {required}"
        );
    }

    let txn_src = include_str!("../src/control/automaton/txn.rs");
    let distributed_src = include_str!("../src/control/automaton/distributed.rs");

    for forbidden in [
        "pub trait Tap",
        "pub struct NoopTap",
        "pub struct Txn<",
        "pub struct InBegin<",
        "pub struct InAcked<",
        "pub struct Closed<",
        "pub fn begin(self, tap: &mut impl Tap) -> InBegin<Inv, S>",
        "pub fn ack(self, tap: &mut impl Tap) -> InAcked<Inv, S>",
        "pub fn lane(&self) -> Lane",
        "pub fn commit(self, tap: &mut impl Tap) -> Closed<Inv>",
        "pub fn abort(self, tap: &mut impl Tap) -> Closed<Inv>",
        "pub struct DistributedSpliceInv;",
        "pub struct SpliceIntent {",
        "pub struct SpliceAck {",
        "pub struct DistributedSplice;",
        "pub fn new(",
        "pub fn from_intent(intent: &SpliceIntent) -> Self",
        "pub fn begin(",
        "pub fn acknowledge(",
        "pub fn commit(",
    ] {
        assert!(
            !txn_src.contains(forbidden) && !distributed_src.contains(forbidden),
            "crate-private automaton owner must not keep internal transaction/splice types public: {forbidden}"
        );
    }
    for required in [
        "pub(crate) trait Tap",
        "pub(crate) struct NoopTap",
        "pub(crate) struct Txn<",
        "pub(crate) struct InBegin<",
        "pub(crate) struct InAcked<",
        "pub(crate) struct Closed<",
        "pub(crate) fn begin(self, tap: &mut impl Tap) -> InBegin<Inv, S>",
        "pub(crate) fn ack(self, tap: &mut impl Tap) -> InAcked<Inv, S>",
        "pub(crate) fn lane(&self) -> Lane",
        "pub(crate) fn commit(self, tap: &mut impl Tap) -> Closed<Inv>",
        "pub(crate) struct DistributedSpliceInv;",
        "pub(crate) struct SpliceIntent {",
        "pub(crate) struct SpliceAck {",
        "pub(crate) struct DistributedSplice;",
        "pub(crate) fn from_intent(intent: &SpliceIntent) -> Self",
        "pub(crate) fn acknowledge(",
        "pub(crate) fn commit(",
    ] {
        assert!(
            txn_src.contains(required) || distributed_src.contains(required),
            "crate-private automaton owner should keep internal transaction/splice types crate-visible only: {required}"
        );
    }

    for forbidden in [
        "pub struct DelegationLeaseSpec<",
        "pub struct DelegateMintSeed<",
        "pub struct DelegateMintAutomaton<",
        "pub struct SpliceGraphContext {",
        "pub struct SpliceLeaseSpec<",
        "pub struct SplicePrepareSeed {",
        "pub struct SplicePrepareAutomaton;",
        "pub struct SpliceBeginAutomaton;",
        "pub struct SpliceCommitAutomaton;",
        "pub fn new(last_intent: Option<SpliceIntent>) -> Self",
        "pub fn clear(&mut self)",
    ] {
        assert!(
            !delegation_src.contains(forbidden) && !splice_src.contains(forbidden),
            "crate-private automaton owner must not keep lease/splice orchestrators public: {forbidden}"
        );
    }
    for required in [
        "pub(crate) struct DelegationLeaseSpec<",
        "pub(crate) struct DelegateMintSeed<",
        "pub(crate) struct DelegateMintAutomaton<",
        "pub(crate) struct SpliceGraphContext {",
        "pub(crate) struct SpliceLeaseSpec<",
        "pub(crate) struct SplicePrepareSeed {",
        "pub(crate) struct SplicePrepareAutomaton;",
        "pub(crate) struct SpliceBeginAutomaton;",
        "pub(crate) struct SpliceCommitAutomaton;",
        "pub(crate) fn new(last_intent: Option<SpliceIntent>) -> Self",
        "pub(crate) fn clear(&mut self)",
    ] {
        assert!(
            delegation_src.contains(required) || splice_src.contains(required),
            "crate-private automaton owner should keep lease/splice orchestrators crate-visible only: {required}"
        );
    }

    for forbidden in [
        "pub fn begin_distributed_splice(",
        "pub fn take_cached_distributed_intent(",
        "pub fn process_splice_intent(",
        "pub fn commit_distributed_splice(",
        "pub(crate) fn commit_distributed_splice(",
    ] {
        assert!(
            !rendezvous_core_src.contains(forbidden),
            "rendezvous core must not keep distributed splice helpers public once splice messages are crate-private: {forbidden}"
        );
    }
    for required in [
        "pub(crate) fn begin_distributed_splice(",
        "pub(crate) fn take_cached_distributed_intent(",
        "pub(crate) fn process_splice_intent(",
    ] {
        assert!(
            rendezvous_core_src.contains(required),
            "rendezvous core should keep distributed splice helpers crate-private: {required}"
        );
    }

    let observe_events_src = include_str!("../src/observe/events.rs");
    for forbidden in [
        "pub(crate) struct SpliceBegin;",
        "pub(crate) struct SpliceCommit;",
    ] {
        assert!(
            !observe_events_src.contains(forbidden),
            "dead splice tap builders must stay deleted once runtime distributed commit helpers are gone: {forbidden}"
        );
    }
}

#[test]
fn control_type_source_names_stay_canonical() {
    let control_types_src = include_str!("../src/control/types.rs");
    let rendezvous_types_path =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/rendezvous/types.rs");

    for forbidden in [
        "pub struct LaneId(",
        "pub struct Gen(",
        "Gen::ZERO",
        "LaneId::as_wire",
        "Gen is already u16",
        "Gen as Generation",
        "LaneId as Lane",
    ] {
        assert!(
            !control_types_src.contains(forbidden),
            "old alias-backed control type naming must not remain: {forbidden}"
        );
    }

    assert!(
        !rendezvous_types_path.exists(),
        "rendezvous must not keep a separate type re-export shell file"
    );

    for required in [
        "pub struct Lane(pub u32);",
        "pub struct Generation(pub u16);",
    ] {
        assert!(
            control_types_src.contains(required),
            "canonical control type must exist directly: {required}"
        );
    }
}

#[test]
fn eff_index_and_binding_metadata_keep_canonical_owner_types() {
    let eff_src = include_str!("../src/eff.rs");
    let binding_src = include_str!("../src/binding.rs");
    let transport_src = include_str!("../src/transport.rs");

    for forbidden in [
        "pub type EffIndex = u16;",
        "pub eff_index: u16,",
        "pub type LabelId = u8;",
        "pub type ResourceKindId = u8;",
        "pub type EffSlice = &'static [EffStruct];",
    ] {
        assert!(
            !eff_src.contains(forbidden)
                && !binding_src.contains(forbidden)
                && !transport_src.contains(forbidden),
            "effect index must not regress to a raw integer alias or binding metadata field: {forbidden}"
        );
    }

    for forbidden in [
        "pub struct EffSlice(&'static [EffStruct]);",
        "Seq =",
        "Par =",
        "Alt =",
        "Recv,",
    ] {
        assert!(
            !eff_src.contains(forbidden),
            "flat raw effect metadata must not regrow ghost model residues: {forbidden}"
        );
    }

    for required in ["pub struct EffIndex(u16);", "pub eff_index: EffIndex,"] {
        assert!(
            eff_src.contains(required)
                || binding_src.contains(required)
                || transport_src.contains(required),
            "canonical effect index ownership must remain explicit: {required}"
        );
    }
}

#[test]
fn state_index_keeps_canonical_newtype_owner() {
    let typestate_src = include_str!("../src/global/typestate.rs");
    let role_program_src = include_str!("../src/global/role_program.rs");

    for forbidden in [
        "pub type StateIndex = u16;",
        "entry: u16,",
        "cursor_index: u16,",
        "next: u16,",
        "route_recv_head: u16,",
        "route_recv_tail: u16,",
        "route_recv_offset: u16,",
    ] {
        assert!(
            !typestate_src.contains(forbidden),
            "state index must not regress to a raw integer alias or raw field owner: {forbidden}"
        );
    }

    assert!(
        typestate_src.contains("pub struct StateIndex(u16);"),
        "canonical state index newtype must remain explicit"
    );
    assert!(
        !typestate_src.contains("pub(crate) struct RouteRecvIndex(u16);"),
        "route recv lookup must stay inline to RouteScopeRecord instead of reviving list indices"
    );
    for forbidden in [
        "pub struct RouteRecvIndex(u16);",
        "pub const MAX_STATES: usize =",
        "pub enum JumpReason {",
        "pub struct JumpError {",
        "pub enum PassiveArmNavigation {",
        "pub enum LoopRole {",
        "pub struct LoopMetadata {",
        "pub struct PhaseCursor {",
        "pub fn phase_cursor(&'prog self) -> PhaseCursor<ROLE> {",
        "pub(crate) fn phase_cursor(&'prog self) -> PhaseCursor<ROLE> {",
    ] {
        assert!(
            !typestate_src.contains(forbidden) && !role_program_src.contains(forbidden),
            "typestate/test cursor internals must not stay publicly reachable: {forbidden}"
        );
    }
    for required in [
        "pub(crate) const MAX_STATES: usize =",
        "pub(crate) enum JumpReason {",
        "pub(crate) struct JumpError {",
        "pub(crate) enum PassiveArmNavigation {",
        "pub(crate) enum LoopRole {",
        "pub(crate) struct LoopMetadata {",
        "pub(crate) struct PhaseCursor {",
    ] {
        assert!(
            typestate_src.contains(required) || role_program_src.contains(required),
            "typestate/test cursor internals must stay crate-private under their canonical owner: {required}"
        );
    }
}

#[test]
fn endpoint_transport_and_mgmt_lower_layers_stay_non_public() {
    let typestate_src = include_str!("../src/global/typestate.rs");
    let mgmt_src = include_str!("../src/runtime/mgmt.rs");
    let mgmt_payload_src = include_str!("../src/runtime/mgmt/payload.rs");
    let mgmt_request_reply_src = include_str!("../src/runtime/mgmt/request_reply.rs");
    let mgmt_observe_stream_src = include_str!("../src/runtime/mgmt/observe_stream.rs");
    let mgmt_test_support_src = include_str!("../src/runtime/mgmt/test_support.rs");
    let trace_src = include_str!("../src/transport/trace.rs");
    let wire_src = include_str!("../src/transport/wire.rs");
    let endpoint_src = include_str!("../src/endpoint.rs");
    let affine_src = include_str!("../src/endpoint/affine.rs");
    let control_src = include_str!("../src/endpoint/control.rs");
    let cursor_src = include_str!("../src/endpoint/kernel/core.rs");
    let kernel_mod_src = include_str!("../src/endpoint/kernel/mod.rs");
    let authority_src = include_str!("../src/endpoint/kernel/authority.rs");
    let offer_src = include_str!("../src/endpoint/kernel/offer.rs");
    let flow_src = include_str!("../src/endpoint/flow.rs");
    let cluster_core_src = include_str!("../src/control/cluster/core.rs");
    let rendezvous_core_src = include_str!("../src/rendezvous/core.rs");
    let observe_core_src = include_str!("../src/observe/core.rs");
    let observe_events_src = include_str!("../src/observe/events.rs");
    let observe_events_compact = compact_ws(observe_events_src);

    assert!(
        !typestate_src.contains("pub fn assert_terminal(&self) {"),
        "global typestate cursor must not keep assert_terminal public"
    );

    for forbidden in [
        "pub fn into_await_begin(self) -> Manager<AwaitBegin, SLOTS> {",
        "pub fn stats(&self, slot: Slot) -> Result<StatsResp, MgmtError> {",
        "pub fn staged_version(&self, slot: Slot) -> Option<u32> {",
        "const SLOT_COUNT: usize = RENDEZVOUS_SLOT_COUNT;",
        "pub struct TapFrameMeta {",
        "pub struct FrameFlags(u8);",
        "pub struct LocalFailureReason {",
        "pub struct LaneGuard<'cfg, T: Transport, U: LabelUniverse, C: Clock> {",
        "pub struct SessionControlCtx<'rv, T, U, C, E, const MAX_RV: usize = 8>",
        "pub enum LoopDecision {",
        "pub enum BranchKind {",
        "pub const TAP_BATCH_MAX_EVENTS: usize = 50;",
        "pub const fn id(self) -> u16 {",
    ] {
        assert!(
            !mgmt_src.contains(forbidden)
                && !mgmt_payload_src.contains(forbidden)
                && !mgmt_request_reply_src.contains(forbidden)
                && !mgmt_observe_stream_src.contains(forbidden)
                && !mgmt_test_support_src.contains(forbidden)
                && !trace_src.contains(forbidden)
                && !wire_src.contains(forbidden)
                && !endpoint_src.contains(forbidden)
                && !affine_src.contains(forbidden)
                && !control_src.contains(forbidden)
                && !cursor_src.contains(forbidden)
                && !kernel_mod_src.contains(forbidden)
                && !authority_src.contains(forbidden)
                && !offer_src.contains(forbidden)
                && !observe_core_src.contains(forbidden),
            "internal lower-layer owner must not stay public: {forbidden}"
        );
    }

    for required in [
        "pub(crate) fn into_await_begin(self) -> Manager<AwaitBegin, SLOTS> {",
        "pub(crate) struct TapFrameMeta {",
        "pub(crate) struct FrameFlags(u8);",
        "pub(crate) struct LocalFailureReason {",
        "pub(crate) struct LaneGuard<'cfg, T: Transport, U: LabelUniverse, C: Clock> {",
        "pub(crate) struct SessionControlCtx<'rv, T, U, C, E, const MAX_RV: usize = 8>",
        "pub(crate) enum LoopDecision {",
        "pub(crate) enum BranchKind {",
        "pub(crate) const TAP_BATCH_MAX_EVENTS: usize = 50;",
        "pub(super) const fn id(self) -> u16 {",
    ] {
        assert!(
            typestate_src.contains(required)
                || mgmt_src.contains(required)
                || mgmt_payload_src.contains(required)
                || mgmt_request_reply_src.contains(required)
                || mgmt_observe_stream_src.contains(required)
                || mgmt_test_support_src.contains(required)
                || trace_src.contains(required)
                || wire_src.contains(required)
                || endpoint_src.contains(required)
                || affine_src.contains(required)
                || control_src.contains(required)
                || cursor_src.contains(required)
                || kernel_mod_src.contains(required)
                || authority_src.contains(required)
                || offer_src.contains(required)
                || observe_core_src.contains(required),
            "internal lower-layer owner should stay crate-private: {required}"
        );
    }

    for forbidden in [
        "fn apply_seed",
        "mgmt_managers",
        "drive_mgmt(",
        "fn load_commit_with",
        "fn schedule_activate_with",
        "fn on_decision_boundary_for_slot_with",
        "fn revert_with",
    ] {
        assert!(
            !mgmt_src.contains(forbidden)
                && !mgmt_request_reply_src.contains(forbidden)
                && !mgmt_observe_stream_src.contains(forbidden)
                && !mgmt_test_support_src.contains(forbidden)
                && !cluster_core_src.contains(forbidden)
                && !rendezvous_core_src.contains(forbidden),
            "deleted mgmt lower-layer owner must not return: {forbidden}"
        );
    }
    assert!(
        !cluster_core_src.contains("fn on_decision_boundary(")
            && !offer_src.contains("cluster.on_decision_boundary("),
        "deleted mgmt decision-boundary hook must not return in cluster core or offer kernel"
    );

    assert!(
        typestate_src.contains("pub(crate) fn assert_terminal(&self) {"),
        "global typestate cursor should keep assert_terminal crate-private"
    );
    assert!(
        typestate_src.contains("#[cfg(test)]\n    pub(crate) fn assert_terminal(&self) {"),
        "global typestate cursor should keep assert_terminal unit-test-only"
    );

    assert!(
        flow_src.contains("pub(crate) fn send<'a, A>("),
        "CapFlow::send must stay crate-private"
    );
    assert!(
        !cursor_src.contains("type ControlResource<M> =")
            && !flow_src.contains("type ControlResource<M> ="),
        "endpoint lower layers must not hide control resource ownership behind local shorthand aliases"
    );
    assert!(
        cursor_src
            .contains("<<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind")
            && flow_src
                .contains("<<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind"),
        "endpoint lower layers must spell the canonical control resource owner directly"
    );
    assert!(
        flow_src.matches("pub fn send<'a, A>(").count() == 1,
        "public flow facade should keep exactly one public send entrypoint"
    );
    assert!(
        !cursor_src.contains(".then("),
        "endpoint cursor owners and their unit tests must not depend on hidden Program::then"
    );
    assert!(
        mgmt_observe_stream_src.matches(".then(").count() == 1
            && mgmt_observe_stream_src.contains("STREAM_LOOP_BREAK_PREFIX.then(g::send::<")
            && mgmt_observe_stream_src.contains("g::Msg<LABEL_OBSERVE_STREAM_END, ()>"),
        "the only remaining Program::then use must stay the mgmt stream break-arm tail join"
    );

    for forbidden in [
        "pub const fn pack_session_lane(sid: u32, lane: u16) -> u32 {",
        "pub const fn with_causal_and_scope(",
        "pub const fn with_digest(",
        "pub const fn new(ts: u32, id: u16, arg0: u32, arg1: u32) -> TapEvent {",
        "pub const fn with_causal(ts: u32, id: u16, causal: u16, arg0: u32, arg1: u32) -> TapEvent {",
    ] {
        assert!(
            !observe_events_src.contains(forbidden),
            "tap-event builders for crate-private owners must not stay public: {forbidden}"
        );
    }

    for required in [
        "pub(crate) const fn pack_session_lane(sid: u32, lane: u16) -> u32 {",
        "pub(crate) const fn with_causal_and_scope(",
        "pub(crate) const fn with_digest(",
        "pub const fn new(ts: u32, id: u16) -> TapEvent {",
    ] {
        assert!(
            observe_events_compact.contains(&compact_ws(required)),
            "tap-event builders for crate-private owners should stay crate-private: {required}"
        );
    }

    for required in [
        "pub const fn with_arg0(mut self, arg0: u32) -> Self {",
        "pub const fn with_arg1(mut self, arg1: u32) -> Self {",
        "pub const fn with_causal_key(mut self, causal_key: u16) -> Self {",
    ] {
        assert!(
            compact_ws(observe_core_src).contains(&compact_ws(required)),
            "tap-event builders for crate-private owners should stay crate-private: {required}"
        );
    }
}

#[test]
fn repo_does_not_reintroduce_stack_tuning_helpers() {
    let cluster_core_src = include_str!("../src/control/cluster/core.rs");
    let typestate_src = include_str!("../src/global/typestate.rs");
    let compiled_role_src = include_str!("../src/global/compiled/role.rs");
    let core_offer_tests_src = include_str!("../src/endpoint/kernel/core_offer_tests.rs");
    let stack_size_call = concat!("stack_size", "(");

    for (owner, body) in [
        ("control/cluster/core.rs", cluster_core_src),
        ("global/typestate.rs", typestate_src),
        ("global/compiled/role.rs", compiled_role_src),
        ("endpoint/kernel/core_offer_tests.rs", core_offer_tests_src),
    ] {
        assert!(
            !body.contains(stack_size_call),
            "{owner} must not reintroduce explicit stack tuning"
        );
    }

    for deleted in [
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/support/large_stack_sync.rs"),
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/support/large_stack_async.rs"),
    ] {
        assert!(
            !deleted.exists(),
            "deleted large-stack support module must stay absent: {}",
            deleted.display()
        );
    }
}

#[test]
fn endpoint_kernel_state_stays_no_alloc() {
    for (owner, body) in [
        (
            "endpoint/kernel/route_state.rs",
            include_str!("../src/endpoint/kernel/route_state.rs"),
        ),
        (
            "endpoint/kernel/frontier_state.rs",
            include_str!("../src/endpoint/kernel/frontier_state.rs"),
        ),
        (
            "endpoint/kernel/evidence_store.rs",
            include_str!("../src/endpoint/kernel/evidence_store.rs"),
        ),
    ] {
        assert!(
            !body.contains("std::boxed::Box") && !body.contains("Box::") && !body.contains("vec!["),
            "{owner} must stay no-alloc and must not reintroduce boxed state owners"
        );
    }
}

#[test]
fn endpoint_kernel_storage_stays_actual_scope_bounded() {
    let frontier_state_src = include_str!("../src/endpoint/kernel/frontier_state.rs");
    let frontier_src = include_str!("../src/endpoint/kernel/frontier.rs");
    let compiled_role_src = include_str!("../src/global/compiled/role.rs");
    let rendezvous_tables_src = include_str!("../src/rendezvous/tables.rs");

    for forbidden in [
        "root_frontier_slot_by_ordinal",
        "slot_by_entry: [u8; MAX_STATES]",
        "ScopeId::ORDINAL_CAPACITY as usize * 2",
        "ScopeId::ORDINAL_CAPACITY as usize * MAX_FIRST_RECV_DISPATCH",
        "slot_by_ordinal:",
        "ROUTE_SCOPE_ORDINAL_CAPACITY",
        "last_seen: [u16; ROLE_SLOTS]",
        "const LOOP_SLOTS: usize = eff::meta::MAX_EFF_NODES;",
        "buffered_label_lane_masks: [u8; 128]",
    ] {
        assert!(
            !frontier_state_src.contains(forbidden)
                && !frontier_src.contains(forbidden)
                && !compiled_role_src.contains(forbidden)
                && !include_str!("../src/endpoint/kernel/inbox.rs").contains(forbidden),
            "endpoint runtime storage must stay bounded by actual compiled facts, not ordinal-space reserves: {forbidden}"
        );
        assert!(
            !rendezvous_tables_src.contains(forbidden),
            "rendezvous route storage must stay bounded by active scopes, not ordinal-space reserves: {forbidden}"
        );
    }

    for forbidden in [
        "frontier_offer_entry_slots: step_len as u16",
        "frontier_observed_entry_slots: step_len as u16",
        "scope_evidence_slots: route_scope_count as u16",
    ] {
        assert!(
            !compiled_role_src.contains(forbidden),
            "compiled endpoint storage budget must stay aligned with sparse runtime owners: {forbidden}"
        );
    }
}

#[test]
fn typestate_nodes_stay_packed_and_sentinel_backed() {
    let facts_src = include_str!("../src/global/typestate/facts.rs");
    let facts_ws = compact_ws(facts_src);
    let cursor_src = include_str!("../src/global/typestate/cursor.rs");

    for required in [
        "scope: CompactScopeId,",
        "route_arm_raw: u8,",
        "flags: u8,",
        "const ROUTE_ARM_NONE: u8 = u8::MAX;",
        "const FLAG_CHOICE_DETERMINANT: u8 = 1 << 0;",
    ] {
        assert!(
            facts_src.contains(required),
            "typestate nodes should keep packed sentinel-backed resident fields: {required}"
        );
    }

    for forbidden in [
        "loop_scope: CompactScopeId,",
        "const fn encode_loop_scope(loop_scope: Option<ScopeId>) -> CompactScopeId {",
        "pub(crate) const fn loop_scope(&self) -> Option<ScopeId> {",
    ] {
        assert!(
            !facts_src.contains(forbidden),
            "typestate nodes should derive loop scope from ancestry instead of storing it: {forbidden}"
        );
    }

    for required in [
        "pub(crate) fn enclosing_loop_scope(&self, scope_id: ScopeId) -> Option<ScopeId> {",
        "pub(crate) fn node_loop_scope(&self, index: usize) -> Option<ScopeId> {",
    ] {
        assert!(
            cursor_src.contains(required),
            "cursor should recover loop scope from scope ancestry: {required}"
        );
    }

    assert!(
        !facts_ws.contains(
            "pub(crate) struct LocalNode { action: LocalAction, next: StateIndex, scope: ScopeId, route_arm: Option<u8>,"
        ),
        "typestate nodes must not regress to wide Option-backed resident fields"
    );
}

#[test]
fn public_endpoint_handles_route_through_rendezvous_endpoint_leases() {
    let cluster_core_src = include_str!("../src/control/cluster/core.rs");

    assert!(
        !cluster_core_src.contains("struct PublicEndpointCell {")
            && !cluster_core_src.contains("endpoint_cells:")
            && cluster_core_src.contains("slot: EndpointLeaseId,")
            && cluster_core_src.contains("release_public_endpoint_slot_owned("),
        "public endpoint ownership must stay on rendezvous endpoint leases"
    );
    for forbidden in [
        "MaybeUninit<ErasedPublicEndpointKernel",
        "storage: core::cell::UnsafeCell<MaybeUninit<ErasedPublicEndpointKernel",
    ] {
        assert!(
            !cluster_core_src.contains(forbidden),
            "rendezvous endpoint leases must not inline endpoint kernels again: {forbidden}"
        );
    }
}

#[test]
fn core_sources_do_not_keep_env_debug_escape_hatches() {
    let lib_src = include_str!("../src/lib.rs");
    let cursor_src = endpoint_kernel_source();

    for forbidden in [
        "HIBANA_DECODE_DEBUG",
        "HIBANA_OFFER_DEBUG",
        "HIBANA_CANCEL_CASES",
        "HIBANA_ROLLBACK_CASES",
        "std::env::var_os(",
        "eprintln!(",
    ] {
        assert!(
            !lib_src.contains(forbidden) && !cursor_src.contains(forbidden),
            "hibana core must not keep env/debug escape hatches: {forbidden}"
        );
    }
}

#[test]
fn eff_and_epf_engine_internals_stay_crate_private() {
    let eff_src = include_str!("../src/eff.rs");
    let epf_src = include_str!("../src/epf.rs");
    let vm_src = include_str!("../src/epf/vm.rs");

    for forbidden in [
        "pub const MAX_EFF_NODES: usize = 256;",
        "pub const ENGINE_FAIL_CLOSED: u16 = 0xFFFF;",
        "pub const ENGINE_LIVENESS_EXHAUSTED: u16 = 0xFFFE;",
        "pub const ANNOT_CAP: usize = 4;",
        "pub struct Annotation {",
        "pub struct Vm<'code> {",
        "pub fn annotations(&self) -> &[Annotation] {",
        "pub fn annot_count(&self) -> u8 {",
        "pub fn annot_dropped(&self) -> bool {",
        "pub fn new(code: &'code [u8], mem: &'code mut [u8], fuel: u16) -> Self {",
        "pub fn execute<'a>(&mut self, ctx: &mut VmCtx<'a>) -> VmAction {",
    ] {
        assert!(
            !eff_src.contains(forbidden)
                && !epf_src.contains(forbidden)
                && !vm_src.contains(forbidden),
            "engine internals must not stay public: {forbidden}"
        );
    }

    for required in [
        "pub(crate) const MAX_EFF_NODES: usize = 256;",
        "pub(crate) const ENGINE_FAIL_CLOSED: u16 = 0xFFFF;",
        "pub(crate) const ENGINE_LIVENESS_EXHAUSTED: u16 = 0xFFFE;",
        "pub(crate) const ANNOT_CAP: usize = 4;",
        "pub(crate) struct Annotation {",
        "pub(crate) struct Vm<'code> {",
        "pub(crate) fn annotations(&self) -> &[Annotation] {",
        "pub(crate) fn annot_count(&self) -> u8 {",
        "pub(crate) fn annot_dropped(&self) -> bool {",
        "pub(crate) fn new(code: &'code [u8], mem: &'code mut [u8], fuel: u16) -> Self {",
        "pub(crate) fn execute<'a>(&mut self, ctx: &mut VmCtx<'a>) -> VmAction {",
    ] {
        assert!(
            eff_src.contains(required) || epf_src.contains(required) || vm_src.contains(required),
            "engine internals should stay crate-private: {required}"
        );
    }
}

#[test]
fn cluster_hub_stays_explicit_and_dead_ffi_stays_deleted() {
    let cluster_src = include_str!("../src/control/cluster.rs");

    assert!(
        !cluster_src.contains("pub use core::*;"),
        "control::cluster root must not hide ownership behind a wildcard re-export"
    );
    for required in [
        "pub(crate) mod core;",
        "pub(crate) mod effects;",
        "pub(crate) mod error;",
    ] {
        assert!(
            cluster_src.contains(required),
            "control::cluster root must keep the curated crate-private surface explicit: {required}"
        );
    }
    for forbidden in ["pub use core::{", "pub use error::AttachError;"] {
        assert!(
            !cluster_src.contains(forbidden),
            "control::cluster root must not keep owner-concealing re-export hubs: {forbidden}"
        );
    }
    assert!(
        !cluster_src.contains("mod ffi;"),
        "dead cluster ffi boundary must be deleted instead of hidden"
    );
    assert!(
        !cluster_src.contains("DynamicResolverFn"),
        "cluster hub must not hide a plain resolver fn behind a type alias export"
    );
}

#[test]
fn hidden_attach_path_uses_canonical_attach_endpoint_name() {
    let cluster_core_src = include_str!("../src/control/cluster/core.rs");
    let cursor_src = endpoint_kernel_source();
    let endpoint_src = include_str!("../src/endpoint.rs");

    assert!(
        cluster_core_src.contains(
            "pub(crate) unsafe fn attach_endpoint_into<'r, const ROLE: u8, GlobalSteps, Mint, B>("
        ),
        "cluster lower layer must expose the canonical in-place attach_endpoint_into helper"
    );
    assert!(
        !cluster_core_src.contains("attach_cursor"),
        "cluster lower layer must not keep the stale attach_cursor name"
    );
    assert!(
        !endpoint_src.contains("mod delegate;"),
        "endpoint root must not keep the deleted delegated-claim helper module"
    );
    assert!(
        !cursor_src.contains(".attach_cursor::<"),
        "cursor tests must not keep using the stale attach_cursor helper name"
    );
}

#[test]
fn endpoint_kernel_split_and_lane_port_unsafe_boundary_hold() {
    let endpoint_src = include_str!("../src/endpoint.rs");
    let kernel_mod_src = include_str!("../src/endpoint/kernel/mod.rs");
    let kernel_core_src = include_str!("../src/endpoint/kernel/core.rs");
    let endpoint_init_src = include_str!("../src/endpoint/kernel/endpoint_init.rs");
    let lane_port_src = include_str!("../src/endpoint/kernel/lane_port.rs");
    let authority_src = include_str!("../src/endpoint/kernel/authority.rs");
    let control_src = include_str!("../src/endpoint/kernel/control.rs");
    let decode_src = include_str!("../src/endpoint/kernel/decode.rs");
    let frontier_src = include_str!("../src/endpoint/kernel/frontier.rs");
    let frontier_state_src = include_str!("../src/endpoint/kernel/frontier_state.rs");
    let inbox_src = include_str!("../src/endpoint/kernel/inbox.rs");
    let observe_src = include_str!("../src/endpoint/kernel/observe.rs");
    let offer_src = include_str!("../src/endpoint/kernel/offer.rs");
    let recv_src = include_str!("../src/endpoint/kernel/recv.rs");
    let route_state_src = include_str!("../src/endpoint/kernel/route_state.rs");
    let send_src = include_str!("../src/endpoint/kernel/send.rs");
    let evidence_store_src = include_str!("../src/endpoint/kernel/evidence_store.rs");

    assert!(
        endpoint_src.contains("pub(crate) mod kernel;"),
        "endpoint root must route internal kernel ownership through endpoint::kernel"
    );
    assert!(
        kernel_mod_src.contains("mod core;")
            && kernel_mod_src.contains("mod send;")
            && kernel_mod_src.contains("mod recv;")
            && kernel_mod_src.contains("mod offer;")
            && kernel_mod_src.contains("mod decode;")
            && kernel_mod_src.contains("mod authority;")
            && kernel_mod_src.contains("mod evidence;")
            && kernel_mod_src.contains("mod evidence_store;")
            && kernel_mod_src.contains("mod endpoint_init;")
            && kernel_mod_src.contains("mod frontier;")
            && kernel_mod_src.contains("mod frontier_state;")
            && kernel_mod_src.contains("mod inbox;")
            && kernel_mod_src.contains("mod control;")
            && kernel_mod_src.contains("mod observe;")
            && kernel_mod_src.contains("mod lane_port;"),
        "endpoint kernel must keep the split module boundary explicit"
    );
    assert!(
        std::fs::metadata(format!(
            "{}/src/endpoint/cursor.rs",
            env!("CARGO_MANIFEST_DIR")
        ))
        .is_err(),
        "legacy endpoint/cursor.rs owner must stay deleted"
    );

    let non_test_core = strip_cfg_test_modules(kernel_core_src);
    assert!(
        !non_test_core.contains("unsafe"),
        "production endpoint kernel core must keep unsafe concentrated out of core.rs"
    );
    for (path, src) in [
        ("endpoint_init.rs", endpoint_init_src),
        ("authority.rs", authority_src),
        ("control.rs", control_src),
        ("decode.rs", decode_src),
        ("frontier.rs", frontier_src),
        ("frontier_state.rs", frontier_state_src),
        ("inbox.rs", inbox_src),
        ("observe.rs", observe_src),
        ("offer.rs", offer_src),
        ("recv.rs", recv_src),
        ("route_state.rs", route_state_src),
        ("send.rs", send_src),
        ("evidence_store.rs", evidence_store_src),
    ] {
        if matches!(
            path,
            "endpoint_init.rs"
                | "frontier.rs"
                | "frontier_state.rs"
                | "inbox.rs"
                | "route_state.rs"
                | "evidence_store.rs"
        ) {
            assert!(
                src.contains("unsafe {") || src.contains("unsafe fn "),
                "endpoint kernel module {path} must remain an explicit in-place init unsafe owner"
            );
        } else {
            assert!(
                !src.contains("unsafe"),
                "endpoint kernel module {path} must not own production unsafe outside lane_port.rs and explicit init owners"
            );
        }
    }
    assert!(
        lane_port_src.contains("unsafe {"),
        "lane_port.rs must remain the concentrated owner for endpoint transport unsafe"
    );
}

#[test]
fn endpoint_kernel_owner_split_stays_explicit() {
    let kernel_core_src = include_str!("../src/endpoint/kernel/core.rs");
    let kernel_mod_src = include_str!("../src/endpoint/kernel/mod.rs");
    let kernel_core_offer_tests_src = include_str!("../src/endpoint/kernel/core_offer_tests.rs");
    let evidence_src = include_str!("../src/endpoint/kernel/evidence.rs");
    let evidence_store_src = include_str!("../src/endpoint/kernel/evidence_store.rs");
    let frontier_observation_src = include_str!("../src/endpoint/kernel/frontier_observation.rs");
    let frontier_select_src = include_str!("../src/endpoint/kernel/frontier_select.rs");
    let frontier_src = include_str!("../src/endpoint/kernel/frontier.rs");
    let frontier_state_src = include_str!("../src/endpoint/kernel/frontier_state.rs");
    let inbox_src = include_str!("../src/endpoint/kernel/inbox.rs");
    let layout_src = include_str!("../src/endpoint/kernel/layout.rs");
    let offer_src = include_str!("../src/endpoint/kernel/offer.rs");
    let offer_refresh_src = include_str!("../src/endpoint/kernel/offer_refresh.rs");
    let route_state_src = include_str!("../src/endpoint/kernel/route_state.rs");
    let scope_evidence_logic_src = include_str!("../src/endpoint/kernel/scope_evidence_logic.rs");
    let kernel_core_ws = compact_ws(kernel_core_src);

    for forbidden in [
        "struct ScopeEvidence {",
        "struct ScopeLoopMeta {",
        "struct ScopeLabelMeta {",
        "struct RouteArmState {",
        "struct ActiveEntrySet {",
        "struct ObservedEntrySet {",
        "struct FrontierSnapshot {",
        "struct OfferEntryState {",
        "struct OfferEntryObservedState {",
        "struct FrontierCandidate {",
        "struct BindingInbox {",
        "struct CachedRecvMeta {",
        "struct ScopeArmMaterializationMeta {",
        "struct CurrentScopeSelectionMeta {",
        "struct CurrentFrontierSelectionState {",
        "struct BranchMeta {",
        "enum BranchKind {",
        "lane_route_arms:",
        "root_frontier_state:",
        "offer_entry_state:",
        "scope_evidence:",
        "scope_evidence_generations:",
    ] {
        assert!(
            !kernel_core_src.contains(forbidden),
            "core.rs must stay an assembly owner instead of reabsorbing split state owners: {forbidden}"
        );
    }
    for forbidden in [
        ".global_active_entries.insert_entry(",
        ".global_active_entries.remove_entry(",
        ".offer_entry_state.get_mut(",
        "lane_route_arms[",
        "lane_linger_counts[",
    ] {
        assert!(
            !kernel_core_ws.contains(forbidden),
            "core.rs must delegate split state-table mutation to dedicated owners: {forbidden}"
        );
    }
    for forbidden in [
        "struct TestBinding {",
        "struct LaneAwareTestBinding {",
        "struct HintOnlyTransport {",
        "struct DeferredIngressBinding {",
    ] {
        assert!(
            !kernel_core_src.contains(forbidden),
            "core.rs must not reabsorb large embedded regression helpers: {forbidden}"
        );
    }
    assert!(
        kernel_core_src.contains("#[path = \"core_offer_tests.rs\"]"),
        "core.rs must keep large offer regression helpers in a child test module"
    );
    for required in [
        "#[path = \"scope_evidence_logic.rs\"]",
        "#[path = \"frontier_select.rs\"]",
        "#[path = \"frontier_observation.rs\"]",
        "#[path = \"offer_refresh.rs\"]",
    ] {
        assert!(
            kernel_core_src.contains(required),
            "core.rs must keep split logic owners as private child modules: {required}"
        );
    }
    assert!(
        kernel_mod_src.contains("mod endpoint_init;"),
        "kernel/mod.rs must keep endpoint_init.rs as the explicit in-place init owner"
    );
    for required in [
        "struct TestBinding {",
        "struct LaneAwareTestBinding {",
        "struct HintOnlyTransport {",
        "struct DeferredIngressBinding {",
    ] {
        assert!(
            kernel_core_offer_tests_src.contains(required),
            "core_offer_tests.rs must remain the canonical owner for large offer regression helpers: {required}"
        );
    }

    for required in [
        "pub(super) struct ScopeEvidence {",
        "pub(super) struct ScopeLoopMeta {",
        "pub(super) struct ScopeLabelMeta {",
        "pub(super) struct RouteArmState {",
    ] {
        assert!(
            evidence_src.contains(required),
            "evidence.rs must remain the canonical owner for scope evidence state: {required}"
        );
    }
    for required in [
        "pub(super) struct ScopeEvidenceSlot {",
        "pub(super) struct ScopeEvidenceTable {",
        "pub(super) fn generation(",
        "pub(super) fn record_ack(",
        "pub(super) fn record_hint(",
        "pub(super) fn mark_ready_arm(",
        "pub(super) fn clear(",
        "pub(super) fn conflicted(",
    ] {
        assert!(
            evidence_store_src.contains(required),
            "evidence_store.rs must remain the canonical mutable owner for scope evidence lifecycle: {required}"
        );
    }
    for forbidden in [
        "scope_evidence: [ScopeEvidence; crate::eff::meta::MAX_EFF_NODES]",
        "scope_evidence_generations: [u32; crate::eff::meta::MAX_EFF_NODES]",
        "scope_evidence_slots: [ScopeEvidenceSlot; crate::eff::meta::MAX_EFF_NODES]",
        "while idx < crate::eff::meta::MAX_EFF_NODES",
    ] {
        assert!(
            !evidence_store_src.contains(forbidden),
            "evidence store must not regress to dense effect-space reserve owners: {forbidden}"
        );
    }

    for required in [
        "pub(super) struct ActiveEntrySlot {",
        "pub(super) struct ActiveEntrySet {",
        "pub(super) struct ObservedEntrySet {",
        "pub(super) struct FrontierObservationMetaSlot {",
        "pub(super) struct FrontierObservationSlot {",
        "pub(super) slots: EntryBuffer<ActiveEntrySlot>,",
        "pub(super) slots: EntryBuffer<FrontierObservationSlot>,",
        "pub(crate) struct FrontierScratchLayout {",
        "pub(super) struct FrontierScratchView {",
        "pub(super) struct FrontierSnapshot {",
        "pub(super) struct OfferEntryState {",
        "pub(super) struct OfferEntryTable {",
        "slots: EntryBuffer<OfferEntrySlot>,",
        "pub(super) struct OfferEntryObservedState {",
        "pub(super) struct FrontierCandidate {",
        "pub(super) struct RootFrontierState {",
        "global_active_entry_slots: FrontierScratchSection,",
        "cached_observation_key_slots: FrontierScratchSection,",
        "#[cfg(test)]\n    pub(super) lane_idx: u8,",
        "#[cfg(test)]\n    pub(super) scope_id: ScopeId,",
        "#[cfg(test)]\n    pub(super) summary: OfferEntryStaticSummary,",
        "#[cfg(test)]\n    pub(super) offer_lane_mask: u8,",
        "#[cfg(test)]\n    pub(super) selection_meta: CurrentScopeSelectionMeta,",
        "#[cfg(test)]\n    pub(super) label_meta: ScopeLabelMeta,",
        "#[cfg(test)]\n    pub(super) materialization_meta: ScopeArmMaterializationMeta,",
        "#[cfg(test)]\n    pub(super) scope_id: ScopeId,",
        "#[cfg(test)]\n    pub(super) observed: OfferEntryObservedState,",
        "#[cfg(test)]\n    pub(super) frontier_mask: u8,",
    ] {
        assert!(
            frontier_src.contains(required),
            "frontier.rs must remain the canonical owner for frontier state: {required}"
        );
    }
    for forbidden in [
        "candidates: [FrontierCandidate; MAX_LANES]",
        "slots: [ScopeId; MAX_LANES]",
    ] {
        assert!(
            !frontier_src.contains(forbidden),
            "frontier snapshot/visit scratch owners must stay pointer-backed instead of fixed MAX_LANES arrays: {forbidden}"
        );
    }
    for forbidden in [
        "-> [u8; MAX_LANES]",
        "[0; MAX_LANES]",
        "[0u8; MAX_LANES]",
        "[ScopeId::none(); MAX_LANES]",
    ] {
        assert!(
            !frontier_observation_src.contains(forbidden),
            "frontier observation hot path must stay off stack-local fixed MAX_LANES scratch arrays: {forbidden}"
        );
        assert!(
            !offer_refresh_src.contains(forbidden),
            "offer refresh hot path must write into borrowed scratch instead of returning fixed MAX_LANES arrays: {forbidden}"
        );
    }
    for required in [
        "pub(super) struct FrontierState {",
        "fn root_frontier_slot(",
        "fn next_observation_epoch(",
        "fn store_frontier_observation(",
        "fn attach_lane_to_root_frontier(",
        "fn detach_lane_from_root_frontier(",
        "fn set_offer_entry_state(",
        "root_frontier_state:",
        "offer_entry_state: OfferEntryTable",
        "global_frontier_observed:",
        "global_frontier_scratch_initialized:",
    ] {
        assert!(
            frontier_state_src.contains(required),
            "frontier_state.rs must remain the canonical mutable owner for frontier tables: {required}"
        );
    }
    for forbidden in [
        "pub(super) global_active_entries:",
        "pub(super) global_frontier_observed_key:",
        "frontier_global_active_slots:",
        "frontier_global_observed_key_slots:",
    ] {
        assert!(
            !frontier_state_src.contains(forbidden) && !layout_src.contains(forbidden),
            "global frontier resident owners must stay out of endpoint arena and FrontierState header: {forbidden}"
        );
    }
    for forbidden in ["pending_branch_nonce", "pending_branch:"] {
        assert!(
            !frontier_state_src.contains(forbidden),
            "frontier_state.rs must not regress to resident pending-branch precommit state: {forbidden}"
        );
    }
    for forbidden in [
        "offer_entry_state: [OfferEntryState; crate::global::typestate::MAX_STATES]",
        "while entry_idx < crate::global::typestate::MAX_STATES",
        "slot_by_entry: [u8; MAX_STATES]",
        "pub(super) offer_lanes: [u8; MAX_LANES],",
        "pub(super) offer_lanes_len: u8,",
    ] {
        assert!(
            !frontier_state_src.contains(forbidden) && !frontier_src.contains(forbidden),
            "frontier owners must not regress to dense state-space reserve tables: {forbidden}"
        );
    }
    for required in [
        "pub(super) struct RouteState {",
        "fn set_route_arm(",
        "fn pop_route_arm(",
        "fn collect_lane_scopes<F>(",
        "fn clear_lane_offer_state(",
        "fn set_lane_offer_state(",
        "lane_route_arms:",
        "lane_offer_states:",
        "scope_evidence: ScopeEvidenceTable,",
        "lane_linger_counts:",
        "pub(super) unsafe fn init_empty(",
        "route_arm_storage: *mut RouteArmState,",
        "lane_offer_state_storage: *mut LaneOfferState,",
        "scope_evidence_slots: *mut ScopeEvidenceSlot,",
        "lane_dense_by_lane: &[u8; MAX_LANES],",
        "active_lane_count: usize,",
        "max_route_stack_depth: usize,",
        "route_scope_count: usize,",
    ] {
        assert!(
            route_state_src.contains(required),
            "route_state.rs must remain the canonical mutable owner for lane route bookkeeping: {required}"
        );
    }

    assert!(
        inbox_src.contains("pub(super) struct BindingInbox {"),
        "inbox.rs must remain the canonical owner for binding inbox buffering"
    );
    for required in [
        "pub(super) struct CachedRecvMeta {",
        "pub(super) struct ScopeArmMaterializationMeta {",
        "pub(super) struct CurrentScopeSelectionMeta {",
        "pub(super) struct CurrentFrontierSelectionState {",
        "pub(crate) struct BranchMeta {",
        "pub(crate) enum BranchKind {",
    ] {
        assert!(
            offer_src.contains(required),
            "offer.rs must remain the canonical owner for offer-path orchestration state: {required}"
        );
    }

    for forbidden in [
        "fn record_scope_ack(",
        "fn ingest_scope_evidence_for_offer(",
        "fn on_frontier_defer(",
        "fn align_cursor_to_selected_scope(",
        "fn frontier_observation_key(",
        "fn refresh_frontier_observation_cache(",
        "fn compose_frontier_observed_entries(",
        "fn refresh_offer_entry_state(",
        "fn sync_lane_offer_state(",
        "fn refresh_lane_offer_state(",
    ] {
        assert!(
            !kernel_core_src.contains(forbidden),
            "core.rs must stay a delegation owner instead of reabsorbing split logic bodies: {forbidden}"
        );
    }

    for required in [
        "fn record_scope_ack(",
        "fn ingest_scope_evidence_for_offer(",
        "fn recover_scope_evidence_conflict(",
    ] {
        assert!(
            scope_evidence_logic_src.contains(required),
            "scope_evidence_logic.rs must remain the canonical owner for scope-evidence ingestion: {required}"
        );
    }
    for required in [
        "fn on_frontier_defer(",
        "fn align_cursor_to_selected_scope(",
    ] {
        assert!(
            frontier_select_src.contains(required),
            "frontier_select.rs must remain the canonical owner for frontier selection logic: {required}"
        );
    }
    for required in [
        "fn frontier_observation_key(",
        "fn refresh_frontier_observation_cache(",
        "fn compose_frontier_observed_entries(",
        "fn refresh_frontier_observation_caches_for_entry(",
    ] {
        assert!(
            frontier_observation_src.contains(required),
            "frontier_observation.rs must remain the canonical owner for observation-cache logic: {required}"
        );
    }
    for required in [
        "fn refresh_offer_entry_state(",
        "fn sync_lane_offer_state(",
        "fn refresh_lane_offer_state(",
        "fn compute_lane_offer_state(",
    ] {
        assert!(
            offer_refresh_src.contains(required),
            "offer_refresh.rs must remain the canonical owner for offer-refresh bookkeeping: {required}"
        );
    }
}

#[test]
fn typestate_builder_stays_a_facade_and_emit_owns_lowering_walk() {
    let builder_src = include_str!("../src/global/typestate/builder.rs");
    let emit_src = include_str!("../src/global/typestate/emit.rs");
    let emit_walk_src = include_str!("../src/global/typestate/emit_walk.rs");
    let emit_scope_src = include_str!("../src/global/typestate/emit_scope.rs");
    let emit_route_src = include_str!("../src/global/typestate/emit_route.rs");
    let registry_src = include_str!("../src/global/typestate/registry.rs");
    let route_facts_src = include_str!("../src/global/typestate/route_facts.rs");

    for forbidden in [
        "struct ScopeEntry {",
        "struct ScopeRecord {",
        "struct ScopeRegistry {",
        "struct RouteRecvNode {",
        "struct PrefixAction {",
        "const MAX_LOOP_TRACKED: usize =",
        "pub(super) const fn build_internal(",
    ] {
        assert!(
            !builder_src.contains(forbidden),
            "builder.rs must stay a facade/owner shell instead of reabsorbing lowering details: {forbidden}"
        );
    }

    for required in [
        "pub struct RoleTypestate<const ROLE: u8> {",
        "pub(crate) struct RoleTypestateValue {",
        "pub(super) nodes: *const LocalNode,",
    ] {
        assert!(
            builder_src.contains(required),
            "builder.rs must remain the canonical owner for the typestate value shell: {required}"
        );
    }

    for forbidden in [
        "const MAX_LOOP_TRACKED: usize =",
        "pub(super) const fn build_internal(",
        "jump_backpatch_indices",
        "route_recv_nodes",
        "route_passive_arm_start",
    ] {
        assert!(
            !emit_src.contains(forbidden),
            "emit.rs must stay a lowering facade instead of reabsorbing walk internals: {forbidden}"
        );
    }
    for required in [
        "pub(crate) struct RoleCompileScratch {",
        "pub(crate) unsafe fn init_value_from_summary_for_role(",
        "super::emit_walk::init_role_typestate_value(",
        "fn passive_arm_scope_by_arm_for_scope_registry(",
    ] {
        assert!(
            emit_src.contains(required),
            "emit.rs must remain the canonical facade for typestate lowering: {required}"
        );
    }

    for required in [
        "pub(super) unsafe fn init_role_typestate_value<P: TypestateProgramView>(",
        "role: u8,",
        "const MAX_LINGER_ARM_TRACK: usize =",
        "jump_backpatch_indices",
        "route_entry.route_recv[arm as usize] = current_state;",
        "route_passive_arm_start",
    ] {
        assert!(
            emit_walk_src.contains(required),
            "emit_walk.rs must remain the canonical owner for lowering walk state: {required}"
        );
    }

    for required in [
        "pub(super) const fn alloc_scope_record(",
        "pub(super) unsafe fn init_scope_registry(",
    ] {
        assert!(
            emit_scope_src.contains(required),
            "emit_scope.rs must remain the canonical owner for scope-entry lowering helpers: {required}"
        );
    }

    for required in [
        "pub(super) const MAX_LOOP_TRACKED: usize =",
        "pub(super) const fn find_loop_entry_state(",
        "pub(super) const fn store_loop_entry_if_absent(",
        "pub(super) const fn parallel_phase_eff_range(",
        "pub(super) const fn phase_route_entry_for_arm(",
        "pub(super) const fn phase_route_arm_for_record(",
    ] {
        assert!(
            emit_route_src.contains(required),
            "emit_route.rs must remain the canonical owner for route/phase lowering helpers: {required}"
        );
    }

    for required in [
        "pub(crate) struct ScopeRecord {",
        "pub(super) struct ScopeRegistry {",
    ] {
        assert!(
            registry_src.contains(required),
            "registry.rs must remain the canonical owner for scope-registry facts: {required}"
        );
    }

    for forbidden in ["pub passive_arm_scope: [u16; 2],"] {
        assert!(
            !registry_src.contains(forbidden),
            "ScopeRecord must derive passive nested route scopes from arm_entry instead of storing dedicated links: {forbidden}"
        );
    }

    for required in [
        "pub(super) struct PrefixAction {",
        "pub(super) const fn route_policy_differs(",
    ] {
        assert!(
            route_facts_src.contains(required),
            "route_facts.rs must remain the canonical owner for route-fact lowering helpers: {required}"
        );
    }
}

#[test]
fn phase_cursor_owns_private_machine_facts_without_cache_lease_backpointers() {
    let cursor_src = include_str!("../src/global/typestate/cursor.rs");
    let layout_src = include_str!("../src/endpoint/kernel/layout.rs");
    let endpoint_init_src = include_str!("../src/endpoint/kernel/endpoint_init.rs");

    assert!(
        cursor_src.contains("struct PhaseCursorMachine {")
            && cursor_src.contains("pub(crate) struct PhaseCursorState {")
            && cursor_src.contains("pub(crate) struct PhaseCursor {")
            && cursor_src.contains("machine: PhaseCursorMachine,")
            && cursor_src.contains("state: *mut PhaseCursorState,")
            && cursor_src.contains("compiled_role: *const CompiledRoleImage,")
            && cursor_src.contains("control_semantics: *const ControlSemanticsTable,")
            && cursor_src.contains("idx: u16,")
            && cursor_src.contains("phase_index: u8,")
            && cursor_src.contains("lane_cursors: [u16; MAX_LANES],")
            && cursor_src.contains("current_step_labels: [u8; MAX_LANES],")
            && cursor_src.contains("labeled_lane_mask: u8,")
            && layout_src.contains("phase_cursor_state: EndpointArenaSection,")
            && endpoint_init_src
                .contains("section_ptr::<crate::global::typestate::PhaseCursorState>(")
            && endpoint_init_src.contains("arena_layout.phase_cursor_state(),"),
        "PhaseCursor must keep shared machine facts in the header and mutable cursor state in the leased arena"
    );
    for forbidden in [
        "Arc<PhaseCursorMachine>",
        "Arc::<PhaseCursorMachine>",
        "from_pinned_role_ptr(",
        "CompiledCacheLease",
        "layout: ProjectedRoleLayout,",
        "pub(crate) struct PhaseCursorImage {",
        "struct PhaseCursorLayout {",
        "image: *const PhaseCursorImage,",
        "label_lane_mask: [u8; 256],",
        "struct PhaseCursorMachine {\n    role: u8,",
        "struct PhaseCursorMachine {\n    role:",
        "struct PhaseCursorMachine {\n    typestate: RoleTypestateValue,",
    ] {
        assert!(
            !cursor_src.contains(forbidden),
            "PhaseCursor must not re-inline compiled machine facts after attach: {forbidden}"
        );
    }
}

#[test]
fn endpoint_kernel_reads_control_semantics_from_shared_compiled_facts() {
    let cursor_src = include_str!("../src/global/typestate/cursor.rs");
    let endpoint_core_src = include_str!("../src/endpoint/kernel/core.rs");
    let endpoint_init_src = include_str!("../src/endpoint/kernel/endpoint_init.rs");

    assert!(
        cursor_src.contains("control_semantics: *const ControlSemanticsTable,"),
        "PhaseCursorMachine must keep control semantics behind shared compiled facts"
    );
    for forbidden in [
        "pub(super) control_semantics: ControlSemanticsTable,",
        "::core::ptr::addr_of_mut!((*dst).control_semantics).write(",
    ] {
        assert!(
            !endpoint_core_src.contains(forbidden) && !endpoint_init_src.contains(forbidden),
            "CursorEndpoint must not re-inline control semantics after attach: {forbidden}"
        );
    }
}

#[test]
fn advanced_steps_and_internal_hubs_stay_canonical() {
    let global_src = include_str!("../src/global.rs");
    let const_dsl_src = include_str!("../src/global/const_dsl.rs");
    let const_dsl_ws = compact_ws(const_dsl_src);
    let steps_src = include_str!("../src/global/steps.rs");
    let rendezvous_src = include_str!("../src/rendezvous.rs");
    let rendezvous_error_src = include_str!("../src/rendezvous/error.rs");
    let rendezvous_port_src = include_str!("../src/rendezvous/port.rs");
    let lease_src = include_str!("../src/control/lease.rs");
    let lease_bundle_src = include_str!("../src/control/lease/bundle.rs");
    let lease_core_src = include_str!("../src/control/lease/core.rs");
    let lease_map_src = include_str!("../src/control/lease/map.rs");
    let lease_planner_src = include_str!("../src/control/lease/planner.rs");
    let lease_graph_src = include_str!("../src/control/lease/graph.rs");
    let substrate_src = include_str!("../src/substrate.rs");
    let substrate_ws = compact_ws(substrate_src);
    let cap_src = include_str!("../src/control/cap.rs");
    let resource_kinds_src = include_str!("../src/control/cap/resource_kinds.rs");
    let control_src = include_str!("../src/control.rs");
    let runtime_src = include_str!("../src/runtime.rs");
    let endpoint_src = include_str!("../src/endpoint.rs");
    let automaton_src = include_str!("../src/control/automaton.rs");
    let handle_src = include_str!("../src/control/handle.rs");
    let observe_src = include_str!("../src/observe.rs");
    let eff_src = include_str!("../src/eff.rs");
    let epf_src = include_str!("../src/epf.rs");
    let mgmt_request_reply_src = include_str!("../src/runtime/mgmt/request_reply.rs");
    let mgmt_observe_stream_src = include_str!("../src/runtime/mgmt/observe_stream.rs");
    let mgmt_test_support_src = include_str!("../src/runtime/mgmt/test_support.rs");
    let transport_context_src = include_str!("../src/transport/context.rs");
    let tables_src = include_str!("../src/rendezvous/tables.rs");
    let typestate_src = include_str!("../src/global/typestate.rs");
    let typestate_ws = compact_ws(typestate_src);
    let steps_ws = compact_ws(steps_src);
    let global_src_full = include_str!("../src/global.rs");
    let role_program_src = include_str!("../src/global/role_program.rs");
    let compiled_driver_src = include_str!("../src/global/compiled/driver.rs");
    let compiled_program_src = include_str!("../src/global/compiled/program.rs");

    for required in [
        "pub type LoopContinueSteps<",
        "pub type LoopBreakSteps<",
        "pub type LoopContinueStepsL<",
        "pub type LoopBreakStepsL<",
        "pub type LoopDecisionSteps<",
        "pub type LoopDecisionStepsL<",
        "pub type LoopSteps<",
        "pub type LoopStepsL<",
    ] {
        assert!(
            steps_src.contains(required),
            "g::advanced::steps must own canonical loop-step combinators: {required}"
        );
    }
    for required in [
        "LoopBreakSteps,",
        "LoopBreakStepsL,",
        "LoopContinueSteps,",
        "LoopContinueStepsL,",
        "LoopDecisionSteps,",
        "LoopDecisionStepsL,",
        "LoopSteps,",
        "LoopStepsL,",
    ] {
        assert!(
            global_src.contains(required),
            "g::advanced::steps must re-export canonical loop-step combinators: {required}"
        );
    }
    for forbidden in [
        "pub const fn with_role(mut self, role_index: u8, lane: u8) -> Self",
        "pub fn first_recv_target(",
    ] {
        assert!(
            !steps_ws.contains(forbidden) && !typestate_ws.contains(forbidden),
            "global lower-layer helpers must not stay public: {forbidden}"
        );
    }
    for required in [
        "pub(crate) const fn with_role(mut self, role_index: u8, lane: u8) -> Self",
        "pub(crate) fn first_recv_target(",
    ] {
        assert!(
            steps_ws.contains(required) || typestate_ws.contains(required),
            "global lower-layer helpers should stay crate-private: {required}"
        );
    }
    for forbidden in [
        "pub use types::*;",
        "pub use error::*;",
        "pub use capability::*;",
        "pub use core::*;",
        "pub use splice::*;",
        "pub use slots::*;",
    ] {
        assert!(
            !rendezvous_src.contains(forbidden),
            "rendezvous hub must not hide ownership behind wildcard re-exports: {forbidden}"
        );
    }
    for forbidden in ["pub use core::*;", "pub use core::{"] {
        assert!(
            !lease_src.contains(forbidden),
            "lease hub must not hide ownership behind root re-export hubs: {forbidden}"
        );
    }
    for (src, required, forbidden) in [
        (
            automaton_src,
            "pub(crate) mod delegation;",
            "pub mod delegation;",
        ),
        (
            automaton_src,
            "pub(crate) mod distributed;",
            "pub mod distributed;",
        ),
        (automaton_src, "pub(crate) mod splice;", "pub mod splice;"),
        (automaton_src, "pub(crate) mod txn;", "pub mod txn;"),
        (lease_src, "pub(crate) mod bundle;", "pub mod bundle;"),
        (lease_src, "pub(crate) mod core;", "pub mod core;"),
        (lease_src, "pub(crate) mod graph;", "pub mod graph;"),
        (lease_src, "pub(crate) mod map;", "pub mod map;"),
        (lease_src, "pub(crate) mod planner;", "pub mod planner;"),
        (handle_src, "pub(crate) mod bag;", "pub mod bag;"),
        (handle_src, "pub(crate) mod frame;", "pub mod frame;"),
        (handle_src, "pub(crate) mod spec;", "pub mod spec;"),
        (runtime_src, "pub(crate) mod config;", "pub mod config;"),
        (runtime_src, "pub(crate) mod consts;", "pub mod consts;"),
        (runtime_src, "pub(crate) mod mgmt;", "pub mod mgmt;"),
        (endpoint_src, "pub(crate) mod affine;", "pub mod affine;"),
        (endpoint_src, "pub(crate) mod control;", "pub mod control;"),
        (endpoint_src, "pub(crate) mod kernel;", "pub mod kernel;"),
        (endpoint_src, "pub(crate) mod flow;", "pub mod flow;"),
        (cap_src, "pub(crate) mod mint;", "pub mod mint;"),
        (
            cap_src,
            "pub(crate) mod typed_tokens;",
            "pub mod typed_tokens;",
        ),
        (observe_src, "pub(crate) mod core;", "pub mod core;"),
        (observe_src, "pub(crate) mod events;", "pub mod events;"),
        (observe_src, "pub(crate) mod scope;", "pub mod scope;"),
        (eff_src, "pub(crate) mod meta {", "pub mod meta {"),
        (epf_src, "pub(crate) mod dispatch;", "pub mod dispatch;"),
        (epf_src, "pub(crate) mod host;", "pub mod host;"),
        (
            epf_src,
            "#[cfg(test)]\npub(crate) mod loader;",
            "pub mod loader;",
        ),
        (epf_src, "pub(crate) mod ops;", "pub mod ops;"),
        (
            epf_src,
            "pub(crate) mod slot_contract;",
            "pub mod slot_contract;",
        ),
        (epf_src, "pub(crate) mod verifier;", "pub mod verifier;"),
        (epf_src, "pub(crate) mod vm;", "pub mod vm;"),
        (
            transport_context_src,
            "pub(crate) mod core {",
            "pub mod core {",
        ),
        (
            resource_kinds_src,
            "pub(crate) mod splice_flags {",
            "pub mod splice_flags {",
        ),
        (
            resource_kinds_src,
            "pub(crate) mod reroute_flags {",
            "pub mod reroute_flags {",
        ),
    ] {
        assert!(
            src.contains(required) && !src.contains(forbidden),
            "private roots must keep internal submodules crate-private: {required}"
        );
    }
    for forbidden in ["pub fn hash32(&self) -> u32 {"] {
        assert!(
            !transport_context_src.contains(forbidden),
            "policy context helpers must not expose internal cache helpers publicly: {forbidden}"
        );
    }
    for required in ["pub(crate) fn hash32(&self) -> u32 {"] {
        assert!(
            transport_context_src.contains(required),
            "policy context cache helpers should stay crate-private under the transport context owner: {required}"
        );
    }
    assert!(
        cap_src.contains("pub mod resource_kinds;"),
        "control::cap root must keep resource_kinds as the sole public submodule"
    );
    assert!(
        !observe_src.contains("pub mod ids;"),
        "observe root must not expose tap identifier constants as a public hub"
    );
    assert!(
        observe_src.contains("pub(crate) mod ids;"),
        "observe ids should stay crate-private under the canonical observe owner"
    );
    let slots_src = include_str!("../src/rendezvous/slots.rs");
    let association_src = include_str!("../src/rendezvous/association.rs");
    let rendezvous_core_src = include_str!("../src/rendezvous/core.rs");
    let rendezvous_splice_src = include_str!("../src/rendezvous/splice.rs");
    for forbidden in [
        "pub struct AssocTable {",
        "pub struct GenTable {",
        "pub struct LoopTable {",
        "pub struct RouteTable {",
        "pub struct PolicyTable {",
        "pub struct VmCapsTable {",
        "pub struct CheckpointTable {",
        "pub struct SlotStorage {",
        "pub struct SlotArena {",
        "pub struct Rendezvous<",
        "pub struct SlotBundle<'rv> {",
        "pub struct LaneLease<'cfg, T, U, C, const MAX_RV: usize>",
        "pub struct CapsFacet<T, U, C, E>(PhantomData<(T, U, C, E)>)",
        "pub struct SpliceFacet<T, U, C, E>(PhantomData<(T, U, C, E)>)",
        "pub struct ObserveFacet<'tap, 'cfg> {",
        "pub struct PendingSplice {",
        "pub struct SpliceStateTable {",
        "pub struct DistributedSpliceTable {",
    ] {
        assert!(
            !association_src.contains(forbidden)
                && !tables_src.contains(forbidden)
                && !slots_src.contains(forbidden)
                && !rendezvous_core_src.contains(forbidden)
                && !rendezvous_splice_src.contains(forbidden),
            "rendezvous lower-layer owners must not stay public: {forbidden}"
        );
    }
    for required in [
        "pub(super) struct AssocTable {",
        "pub(crate) struct GenTable {",
        "pub(crate) struct LoopTable {",
        "pub(crate) struct RouteTable {",
        "pub(crate) struct PolicyTable {",
        "pub(crate) struct VmCapsTable {",
        "pub(crate) struct CheckpointTable {",
        "pub(crate) struct SlotStorage {",
        "pub(crate) struct SlotArena {",
        "pub(crate) struct Rendezvous<",
        "pub(crate) struct SlotBundle<'rv> {",
        "pub(crate) struct LaneLease<'cfg, T, U, C, const MAX_RV: usize>",
        "pub(crate) struct CapsFacet<T, U, C, E>(PhantomData<(T, U, C, E)>)",
        "pub(crate) struct SpliceFacet<T, U, C, E>(PhantomData<(T, U, C, E)>)",
        "pub(crate) struct ObserveFacet<'tap, 'cfg> {",
        "pub(super) struct PendingSplice {",
        "pub(super) struct SpliceStateTable {",
        "pub(super) struct DistributedSpliceTable {",
    ] {
        assert!(
            association_src.contains(required)
                || tables_src.contains(required)
                || slots_src.contains(required)
                || rendezvous_core_src.contains(required)
                || rendezvous_splice_src.contains(required),
            "rendezvous lower-layer owners should stay non-public: {required}"
        );
    }
    for forbidden in [
        "pub fn from_config(config: Config<'cfg, U, C>, transport: T) -> Self {",
        "pub fn with_test_rendezvous_from_config<'cfg, T, U, C, R>(",
        "pub fn initialise_control_scope(&self, lane: Lane, scope_kind: ControlScopeKind) {",
        "pub fn is_session_registered(&self, sid: SessionId) -> bool {",
        "pub fn release_lane(&self, lane: Lane) -> Option<SessionId> {",
        "pub fn into_port_guard(",
        "pub fn id(&self) -> RendezvousId {",
        "pub fn tap(&self) -> &TapRing<'cfg> {",
        "pub fn liveness_policy(&self) -> crate::runtime::config::LivenessPolicy {",
        "pub fn now32(&self) -> u32 {",
        "pub fn cancel_begin(&self, sid: SessionId) -> Result<(), CancelError> {",
        "pub fn cancel_ack(&self, sid: SessionId, r#gen: Generation) -> Result<(), CancelError> {",
        "pub fn checkpoint(&self, sid: SessionId) -> Result<Generation, CheckpointError> {",
        "pub fn rollback(&self, sid: SessionId, epoch: Generation) -> Result<(), RollbackError> {",
        "pub const fn new() -> Self {",
        "pub fn mint_cap<K: crate::control::cap::mint::ResourceKind>(",
        "pub fn next_nonce_seed(",
        "pub fn begin(",
        "pub fn commit(",
        "pub fn tap(&self) -> &'tap crate::observe::core::TapRing<'cfg> {",
        "pub fn new(\n        sid: SessionId,",
        "pub fn into_parts(",
        "pub fn insert(&self, intent: SpliceIntent) -> Result<(), SpliceError> {",
    ] {
        assert!(
            !rendezvous_core_src.contains(forbidden) && !rendezvous_splice_src.contains(forbidden),
            "rendezvous lower-layer methods must not stay public: {forbidden}"
        );
    }
    for required in [
        "pub(crate) fn initialise_control_scope(&self, lane: Lane, scope_kind: ControlScopeKind) {",
        "pub(crate) fn is_session_registered(&self, sid: SessionId) -> bool {",
        "pub(crate) fn release_lane(&self, lane: Lane) -> Option<SessionId> {",
        "pub(crate) fn into_port_guard(",
        "pub(crate) fn tap(&self) -> &TapRing<'cfg> {",
        "pub(crate) fn liveness_policy(&self) -> crate::runtime::config::LivenessPolicy {",
        "pub(crate) fn now32(&self) -> u32 {",
        "pub(crate) fn cancel_begin(&self, sid: SessionId) -> Result<(), CancelError> {",
        "pub(crate) fn cancel_ack(&self, sid: SessionId, r#gen: Generation) -> Result<(), CancelError> {",
        "pub(crate) fn checkpoint(&self, sid: SessionId) -> Result<Generation, CheckpointError> {",
        "pub(crate) fn rollback(&self, sid: SessionId, epoch: Generation) -> Result<(), RollbackError> {",
        "pub(crate) const fn new() -> Self {",
        "pub(crate) fn mint_cap<K: crate::control::cap::mint::ResourceKind>(",
        "pub(crate) fn next_nonce_seed(",
        "pub(crate) fn begin(",
        "pub(crate) fn commit(",
        "pub(crate) fn tap(&self) -> &'tap crate::observe::core::TapRing<'cfg> {",
        "pub(super) fn new(\n        sid: SessionId,",
        "pub(super) fn into_parts(",
        "pub(super) fn insert(&self, intent: SpliceIntent) -> Result<(), SpliceError> {",
    ] {
        assert!(
            rendezvous_core_src.contains(required) || rendezvous_splice_src.contains(required),
            "rendezvous lower-layer methods should stay non-public: {required}"
        );
    }
    for forbidden in [
        "CommitError as ",
        "GenError as ",
        "GenerationRecord as ",
        "SpliceError as ",
        "PendingSplice as ",
        "Guard as BrandGuard",
        "events::{self as tap_events",
        "TransportEvent as ",
        "TransportMetrics as ",
        "brand_guard: Guard<'static>,",
        "mem::transmute::<Guard<'_>, Guard<'static>>",
        "mem::transmute::<Guard<'static>, Guard<'rv>>",
        "core::mem::transmute::<_, Port<'cfg, T, crate::control::cap::mint::EpochTbl>>",
    ] {
        assert!(
            !rendezvous_core_src.contains(forbidden),
            "rendezvous core must not hide canonical owners behind import aliases: {forbidden}"
        );
    }
    for required in [
        "pub(crate) fn brand(&self) -> Guard<'rv> {",
        "Guard::new()",
        "PendingSplice::new(",
        "crate::control::cluster::error::SpliceError::InvalidLane",
        "crate::observe::events::TransportEvent::new(",
        "fn flush_transport_events(&self) -> Option<crate::transport::TransportEvent> {",
    ] {
        assert!(
            rendezvous_core_src.contains(required),
            "rendezvous core must spell canonical owners directly after alias removal: {required}"
        );
    }
    for forbidden in [
        "pub mod consts {",
        "pub use crate::runtime::consts::*;",
        "pub use crate::transport::context::core::*;",
        "pub use crate::control::cap::payload::*;",
        "pub use crate::control::cap::typed_tokens::*;",
        "pub use flow::Flow;",
        "pub use program::Program;",
    ] {
        assert!(
            !substrate_src.contains(forbidden)
                && !endpoint_src.contains(forbidden)
                && !global_src_full.contains(forbidden),
            "internal hubs must not hide owner modules behind re-export shims: {forbidden}"
        );
    }
    for forbidden in ["pub use mint::*;", "pub use resource_kinds::*;"] {
        assert!(
            !cap_src.contains(forbidden),
            "control::cap root must not hide mint/resource-kind ownership behind wildcard re-exports: {forbidden}"
        );
    }
    assert!(
        !cap_src.contains("pub use mint::{"),
        "control::cap root must not act as a mint re-export hub"
    );
    assert!(
        substrate_src.contains("AllowsCanonical,"),
        "substrate::cap::advanced must own the canonical mint-policy witness when a public signature depends on it"
    );
    for forbidden in [
        "AllowsCanonical,",
        "BumpAt,",
        "CAP_FIXED_HEADER_LEN,",
        "CAP_HANDLE_LEN,",
        "CAP_HEADER_LEN,",
        "CAP_NONCE_LEN,",
        "CAP_TAG_LEN,",
        "CAP_TOKEN_LEN,",
        "Ckpt,",
        "Committed,",
        "ControlMint,",
        "E0,",
        "EndpointEpoch,",
        "EpochTable,",
        "HandleView,",
        "LaneKey,",
        "LaneToken,",
        "MaySend,",
        "NoControlKind,",
        "NonceSeed,",
        "Owner,",
        "RolledBack,",
        "SessionScopedKind,",
        "Stop,",
        "VerifiedCap,",
        "VmHandleError,",
    ] {
        assert!(
            !cap_src.contains(forbidden),
            "control::cap root must not re-export mint-only lower-layer names: {forbidden}"
        );
    }
    assert!(
        substrate_ws.contains(
            "pub use crate::control::cap::mint::{ CapShot, ControlResourceKind, GenericCapToken, ResourceKind, };"
        ),
        "substrate public cap surface must source canonical cap types directly from mint owner"
    );
    for forbidden in [
        "pub(crate) use crate::control::cap::typed_tokens::{CapFlowToken, CapRegisteredToken};",
        "pub(crate) use crate::control::cluster::effects::CpEffect;",
        "pub(crate) use crate::control::handle::frame::ControlFrame;",
        "pub(crate) use crate::control::lease::planner::LeaseGraphBudget;",
        "pub(crate) use crate::control::types::RendezvousId;",
    ] {
        assert!(
            !control_src.contains(forbidden),
            "control root must not hide canonical internal owners behind crate-private re-export hubs: {forbidden}"
        );
    }
    for forbidden in [
        "pub(crate) use const_dsl::EffList;",
        "pub(crate) use role_program::RoleProgram;",
        "pub(crate) use steps::{SendStep, StepConcat, StepCons, StepNil};",
        "Alias for last_gen (for compatibility).",
        "pub fn last_ack_gen(&self, lane: Lane) -> Option<Generation> {",
        "backward compat",
        "pub(crate) use mint::CapToken;",
        "pub(crate) type CapToken = GenericCapToken<EndpointResource>;",
        "pub type LeaseFacetFlags = u8;",
        "pub struct FacetSet<const CAPS: bool, const SLOTS: bool, const SPLICE: bool, const DELEGATION: bool>;",
        "pub type FacetCaps = FacetSet<true, false, false, false>;",
        "pub type FacetSlots = FacetSet<false, true, false, false>;",
        "pub type FacetCapsSplice = FacetSet<true, false, true, false>;",
        "pub type FacetCapsDelegation = FacetSet<true, false, false, true>;",
        "pub type FacetContext<'graph, S> =",
        "pub struct ControlCore<",
        "pub enum RegisterRendezvousError {",
        "pub enum LeaseError {",
        "pub struct RendezvousLease<",
        "pub trait RendezvousSpec<",
        "pub struct FullSpec;",
        "pub struct SpliceSpec;",
        "pub struct DelegationSpec;",
        "pub struct LeaseObserve<",
        "pub trait ControlAutomaton<",
        "pub enum ControlStep<",
        "pub enum DelegationDriveError<",
        "pub enum LeaseBundleError {",
        "pub struct CapsBundleHandle<",
        "pub struct SlotBundleHandle<'ctx, 'cfg> {",
        "pub struct LeaseBundleFacet<",
        "pub struct LeaseBundleContext<",
        "pub trait LeaseGraphBundleExt<",
        "pub trait LeaseFacet: Copy + Default {",
        "pub trait LeaseSpec {",
        "pub struct FacetHandle<",
        "pub enum LeaseGraphError {",
        "pub struct LeaseGraph<",
        "pub struct ArrayMap<",
        "pub struct LeaseFacetNeeds {",
        "pub struct LeaseGraphBudget {",
        "pub const fn lease_budget(&self) -> crate::control::lease::planner::LeaseGraphBudget {",
        "pub enum PolicyMode {",
        "pub const fn dynamic(policy_id: u16) -> Self {",
        "pub enum CapError {",
        "pub struct GenerationRecord {",
        "pub enum GenError {",
        "pub enum CancelError {",
        "pub enum CheckpointError {",
        "pub enum RollbackError {",
        "pub enum CommitError {",
        "pub struct Port<",
        "pub fn new<'tap>(",
        "pub fn transport(&self) -> &'r T {",
        "pub fn now32(&self) -> u32 {",
        "pub fn caps_mask(&self) -> CapsMask {",
        "pub fn flush_transport_events(&self) -> Option<TransportEvent> {",
        "pub fn lane(&self) -> Lane {",
        "pub fn rv_id(&self) -> RendezvousId {",
        "pub fn attach_verified(",
        "pub fn with_attach_verified(",
        "pub fn accept_verified(",
        "pub fn with_accept_verified(",
        "pub fn adopt_port(",
        "pub fn checkpoint_token(",
        "pub fn commit_token(",
        "pub fn rollback_token(",
        "pub fn cancel_token(",
        "type LoopContinueMsg =",
        "type LoopBreakMsg =",
    ] {
        assert!(
            !global_src_full.contains(forbidden)
                && !tables_src.contains(forbidden)
                && !typestate_src.contains(forbidden)
                && !lease_src.contains(forbidden)
                && !lease_bundle_src.contains(forbidden)
                && !lease_core_src.contains(forbidden)
                && !lease_map_src.contains(forbidden)
                && !lease_planner_src.contains(forbidden)
                && !lease_graph_src.contains(forbidden)
                && !const_dsl_src.contains(forbidden)
                && !role_program_src.contains(forbidden)
                && !rendezvous_core_src.contains(forbidden)
                && !rendezvous_error_src.contains(forbidden)
                && !rendezvous_port_src.contains(forbidden),
            "internal core must not keep alias/compat shims for canonical owners: {forbidden}"
        );
    }
    assert!(
        !rendezvous_src.contains("mod types;"),
        "rendezvous must not keep a pure type re-export shell module"
    );
    for required in ["pub(crate) mod capability;", "pub(crate) mod core;"] {
        assert!(
            rendezvous_src.contains(required),
            "rendezvous root must expose internal owner modules directly instead of hiding them behind a hub: {required}"
        );
    }
    for required in [
        "pub(crate) struct ControlCore<",
        "pub(crate) enum RegisterRendezvousError {",
        "pub(crate) enum LeaseError {",
        "pub(crate) struct RendezvousLease<",
        "pub(crate) trait RendezvousSpec<",
        "pub(crate) struct FullSpec;",
        "pub(crate) struct SpliceSpec;",
        "pub(crate) struct DelegationSpec;",
        "pub(crate) struct LeaseObserve<",
        "pub(crate) trait ControlAutomaton<",
        "pub(crate) enum ControlStep<",
        "pub(crate) enum DelegationDriveError<",
        "pub(crate) enum LeaseBundleError {",
        "pub(crate) struct CapsBundleHandle<",
        "pub(crate) struct SlotBundleHandle<'ctx, 'cfg> {",
        "pub(crate) struct LeaseBundleFacet<",
        "pub(crate) struct LeaseBundleContext<",
        "pub(crate) trait LeaseGraphBundleExt<",
        "pub(crate) trait LeaseFacet: Copy + Default {",
        "pub(crate) trait LeaseChildStorage<Id: Copy>: Copy {",
        "pub(crate) struct InlineLeaseChildStorage<Id: Copy + Default, const CAPACITY: usize> {",
        "pub(crate) trait LeaseSpec: Sized {",
        "pub(crate) trait LeaseNodeStorage<'graph, S: LeaseSpec> {",
        "pub(crate) struct InlineLeaseNodeStorage<'graph, S: LeaseSpec, const CAPACITY: usize> {",
        "pub(crate) struct FacetHandle<",
        "pub(crate) enum LeaseGraphError {",
        "pub(crate) struct LeaseGraph<",
        "pub(crate) struct ArrayMap<",
        "pub(crate) struct LeaseFacetNeeds {",
        "pub(crate) struct LeaseGraphBudget {",
        "pub(crate) const fn lease_budget(&self) -> LeaseGraphBudget {",
        "pub(crate) enum PolicyMode {",
        "pub(crate) const fn dynamic(policy_id: u16) -> Self {",
        "pub(crate) enum CapError {",
        "pub(crate) struct GenerationRecord {",
        "pub(crate) enum GenError {",
        "pub(crate) enum CancelError {",
        "pub(crate) enum CheckpointError {",
        "pub(crate) enum RollbackError {",
        "pub(crate) enum CommitError {",
        "pub(crate) struct Port<",
        "pub(crate) fn new<'tap>(",
        "pub(crate) fn transport(&self) -> &'r T {",
        "pub(crate) fn now32(&self) -> u32 {",
        "pub(crate) fn caps_mask(&self) -> CapsMask {",
        "pub(crate) fn flush_transport_events(&self) -> Option<TransportEvent> {",
        "pub(crate) fn lane(&self) -> Lane {",
        "pub(crate) fn rv_id(&self) -> RendezvousId {",
    ] {
        assert!(
            lease_core_src.contains(required)
                || lease_bundle_src.contains(required)
                || lease_graph_src.contains(required)
                || lease_map_src.contains(required)
                || lease_planner_src.contains(required)
                || const_dsl_src.contains(required)
                || global_src_full.contains(required)
                || role_program_src.contains(required)
                || compiled_driver_src.contains(required)
                || compiled_program_src.contains(required)
                || rendezvous_error_src.contains(required)
                || rendezvous_port_src.contains(required),
            "lease lower layer must stay crate-private under its canonical owner: {required}"
        );
    }
    assert!(
        !const_dsl_ws
            .contains("impl PolicyMode { pub const fn with_scope(self, scope: ScopeId) -> Self {"),
        "PolicyMode must not leak a public with_scope helper under the const DSL owner"
    );
    assert!(
        const_dsl_ws.contains("pub(crate) const fn with_scope(self, scope: ScopeId) -> Self {"),
        "PolicyMode helpers must stay crate-private under their canonical const DSL owner"
    );
    assert!(
        !const_dsl_ws.contains(
            "pub const fn compose(kind: ScopeKind, local: u16, range: u16, nest: u16) -> Self {"
        ),
        "ScopeId must not expose a raw multi-argument compose constructor on the public substrate surface"
    );
    assert!(
        !const_dsl_ws.contains("pub const fn new(kind: ScopeKind, local: u16) -> Self {"),
        "ScopeId must not expose a generic kind-based constructor when named constructors cover the public surface"
    );
    assert!(
        const_dsl_ws.contains("pub(crate) const fn compose(kind: ScopeKind, local: u16, range: u16, nest: u16) -> Self {"),
        "ScopeId::compose should stay crate-private under the canonical const DSL owner"
    );
    assert!(
        const_dsl_ws.contains("pub(crate) const fn new(kind: ScopeKind, local: u16) -> Self {"),
        "ScopeId::new should stay crate-private under the canonical const DSL owner"
    );
    for forbidden in [
        "pub use crate::control::types::{Generation, Lane, RendezvousId, SessionId};",
        "pub use capability::{",
        "pub use core::{",
        "pub use error::{",
        "pub use slots::{",
    ] {
        assert!(
            !rendezvous_src.contains(forbidden),
            "rendezvous root must not keep owner-concealing re-export hubs: {forbidden}"
        );
    }
    for forbidden in [
        "pub(crate) use error::CapError;",
        "pub(crate) use port::Port;",
        "pub(crate) use slots::slot_index;",
        "pub(crate) use tables::LoopDisposition;",
    ] {
        assert!(
            !rendezvous_src.contains(forbidden),
            "rendezvous root must not keep crate-private owner-hiding re-export hubs: {forbidden}"
        );
    }
    assert!(
        !runtime_src.contains("pub use crate::control::cluster::core::AttachError;"),
        "runtime must not masquerade as the public owner of AttachError"
    );
    assert!(
        !runtime_src.contains("pub(crate) use crate::control::cluster::core::SessionKit;"),
        "runtime must not keep a crate-private SessionKit alias shell"
    );
    for forbidden in [
        "pub(crate) use cursor::CursorEndpoint;",
        "pub(crate) use cursor::RouteBranch as CursorRouteBranch;",
    ] {
        assert!(
            !endpoint_src.contains(forbidden),
            "endpoint facade must not keep crate-private cursor alias shims: {forbidden}"
        );
    }
    for forbidden in ["pub use core::{", "pub use scope::ScopeTrace;"] {
        assert!(
            !observe_src.contains(forbidden),
            "observe root must not hide core/scope ownership behind root re-exports: {forbidden}"
        );
    }
    assert!(
        !include_str!("../src/observe/scope.rs").contains("pub struct ScopeTrace"),
        "ScopeTrace must not be public outside the observe subtree"
    );
    assert!(
        include_str!("../src/observe/scope.rs").contains("pub(crate) struct ScopeTrace"),
        "ScopeTrace should stay crate-private as an observe lower-layer helper"
    );
    assert!(
        !substrate_src.contains("pub mod events {"),
        "substrate tap surface must not expose a RawEvent helper bucket"
    );
    assert!(
        !substrate_src.contains("pub mod session {")
            && !substrate_src.contains("pub fn enter_controller<'cfg, T, U, C, B, const MAX_RV: usize>(")
            && !substrate_src.contains("pub fn enter_cluster<'cfg, T, U, C, B, const MAX_RV: usize>(")
            && !substrate_src.contains("pub async fn drive_cluster<'lease, 'cfg, T, U, C, Mint, B, const MAX_RV: usize>(")
            && !substrate_src.contains("pub async fn drive_stream_cluster<'lease, T, U, C, Mint, F, B, const MAX_RV: usize>(")
            && !substrate_src.contains("pub async fn drive_stream_controller<'lease, T, U, C, Mint, F, B, const MAX_RV: usize>(")
            && !substrate_src.contains("pub async fn drive_controller<'lease, T, U, C, Mint, B, const MAX_RV: usize>("),
        "substrate mgmt must not regrow the deleted public helper family"
    );
    assert!(
        substrate_src.contains("pub mod request_reply {")
            && substrate_src.contains("pub mod observe_stream {")
            && substrate_src.contains("pub const PREFIX: crate::g::Program<PrefixSteps> =")
            && substrate_src.contains("ROLE_CLUSTER,")
            && substrate_src.contains("ROLE_CONTROLLER,"),
        "substrate mgmt must stay on the canonical payload-plus-prefix surface"
    );
    let runtime_mgmt_src = include_str!("../src/runtime/mgmt.rs");
    assert!(
        !runtime_mgmt_src
            .contains("pub(crate) fn enter_controller<'cfg, T, U, C, B, const MAX_RV: usize>(")
            && !runtime_mgmt_src
                .contains("pub(crate) fn enter_cluster<'cfg, T, U, C, B, const MAX_RV: usize>(")
            && !runtime_mgmt_src.contains(
                "pub(crate) fn enter_stream_controller<'cfg, T, U, C, B, const MAX_RV: usize>("
            )
            && !runtime_mgmt_src.contains(
                "pub(crate) fn enter_stream_cluster<'cfg, T, U, C, B, const MAX_RV: usize>("
            )
            && !runtime_mgmt_src.contains("pub(crate) async fn drive_controller<")
            && !runtime_mgmt_src.contains("pub(crate) async fn drive_cluster<")
            && !runtime_mgmt_src.contains("pub(crate) async fn drive_stream_cluster<")
            && !runtime_mgmt_src.contains("pub(crate) async fn drive_stream_controller<"),
        "runtime mgmt root must not regrow the deleted helper-wrapper family"
    );
    assert!(
        !substrate_src.contains("pub mod ids {"),
        "substrate tap surface must not expose observe id constants"
    );
    assert!(
        !substrate_src.contains("FORWARD_CONTROL"),
        "substrate tap surface must not keep dead forward-control tap identifiers"
    );
    for forbidden in [
        "pub use crate::observe::ids;",
        "pub use crate::observe::events;",
        "pub use crate::observe::events::RawEvent;",
        "pub use crate::observe::ids::{",
    ] {
        assert!(
            !substrate_src.contains(forbidden),
            "substrate tap surface must not re-export lower-layer observe owners: {forbidden}"
        );
    }
    assert!(
        !substrate_src.contains("TapBatch"),
        "substrate tap surface must stay on TapEvent only and keep batching private"
    );
    for forbidden in [
        "PolicyEvent,",
        "PolicyEventKind,",
        "TapRing,",
        "install_ring,",
        "uninstall_ring,",
        "RawEvent",
        "SLO_BREACH",
        "LOCAL_ACTION_FAIL",
    ] {
        assert!(
            !substrate_src.contains(forbidden),
            "substrate tap surface must stay on TapEvent only: {forbidden}"
        );
    }
    for forbidden in [
        "pub const DELEG_ABORT: u16",
        "pub const SLO_BREACH: u16",
        "pub(crate) struct SloBreach;",
        "pub(crate) struct SloBreachEvent",
        "pub(crate) fn slo_breach_trace(",
    ] {
        assert!(
            !include_str!("../src/observe/ids.rs").contains(forbidden)
                && !include_str!("../src/observe/events.rs").contains(forbidden)
                && !include_str!("../src/observe/normalise.rs").contains(forbidden),
            "dead observe delegation/slo helpers must stay deleted: {forbidden}"
        );
    }
    for forbidden in [
        "pub use crate::observe::TapEvent;",
        "pub use slot_contract::slot_default_input;",
        "pub use vm::{Slot, Trap, VmAction};",
    ] {
        assert!(
            !epf_src.contains(forbidden),
            "epf root must not keep owner-concealing re-export hubs: {forbidden}"
        );
    }
    for forbidden in [
        "type LoadBeginSteps =",
        "type LoopContinueArmSteps =",
        "type LoopRouteSteps =",
        "type AfterLoop =",
        "type AfterCommit =",
        "type FullProgramSteps =",
        "type ControllerLocal =",
        "type ClusterLocal =",
        "type StreamLoopContinueSteps =",
        "type StreamLoopBreakSteps =",
        "type StreamLoopRouteSteps =",
        "type StreamControllerLocal =",
        "type StreamClusterLocal =",
        "type LoadCommitSteps =",
        "type LoadBeginTokenStep =",
        "type LoadBeginMsgStep =",
        "type LoopChunkStep =",
        "type LoopBreakStep =",
        "type CommandStep =",
        "type StreamSubscribeStep =",
        "type StreamBatchStep =",
        "type StreamEndStep =",
        "type StreamLoopContinueStep =",
        "type StreamLoopBreakStep =",
        "type LoadBeginTokenMsg =",
        "type LoadBeginMsg =",
        "type LoadChunkMsg =",
        "type LoadCommitTokenMsg =",
        "type LoopContinueMsg =",
        "type LoopBreakMsg =",
        "type CommandMsg =",
        "type SubscribeMsg =",
        "type TapBatchMsg =",
        "type StreamContinueMsg =",
        "type StreamBreakMsg =",
        "type StreamEndMsg =",
    ] {
        assert!(
            !mgmt_request_reply_src.contains(forbidden)
                && !mgmt_observe_stream_src.contains(forbidden)
                && !mgmt_test_support_src.contains(forbidden),
            "runtime management owners must not hide canonical message/composition/projection owners behind local aliases: {forbidden}"
        );
    }
    for required in [
        "const LOAD_BEGIN: Program<",
        "pub const PROGRAM: Program<ProgramSteps> =",
        "type LoopSegmentSteps = RouteSteps<",
        "LABEL_LOOP_CONTINUE,",
        "LABEL_LOOP_BREAK,",
        "LABEL_MGMT_LOAD_BEGIN,",
        "LABEL_MGMT_LOAD_CHUNK,",
        "LABEL_MGMT_ACTIVATE,",
        "LABEL_MGMT_REVERT,",
        "LABEL_MGMT_STATS,",
    ] {
        assert!(
            mgmt_request_reply_src.contains(required),
            "request-reply management owner must keep preserved composition on direct canonical witnesses: {required}"
        );
    }
    for required in [
        "pub struct TapBatch {",
        "pub const PROGRAM: Program<ProgramSteps> =",
        "const STREAM_SUBSCRIBE: Program<",
        "const STREAM_LOOP_ROUTE: Program<",
        "LABEL_OBSERVE_SUBSCRIBE,",
        "LABEL_OBSERVE_STREAM_END,",
        "LABEL_OBSERVE_BATCH,",
        "crate::g::seq(STREAM_SUBSCRIBE, STREAM_LOOP_ROUTE);",
    ] {
        assert!(
            mgmt_observe_stream_src.contains(required),
            "observe-stream management owner must keep direct canonical g::seq witnesses: {required}"
        );
    }
}

#[test]
fn crate_manifest_does_not_reintroduce_public_test_utils_feature() {
    let cargo_toml = include_str!("../Cargo.toml");

    assert!(
        !cargo_toml.contains("test-utils = []"),
        "crate manifest must not keep a public test-utils feature"
    );
}

#[test]
fn quality_gates_and_docs_keep_canonical_repo_owned_checks() {
    let readme = include_str!("../README.md");
    let quality_workflow = include_str!("../.github/workflows/quality-gates.yml");
    let boundary_gate = include_str!("../.github/scripts/check_boundary_contracts.sh");
    let direct_projection_gate =
        include_str!("../.github/scripts/check_direct_projection_binary.sh");
    let cluster_core = include_str!("../src/control/cluster/core.rs");
    let pico_smoke_gate = include_str!("../.github/scripts/check_pico_smoke.sh");
    let pico_size_matrix_gate = include_str!("../.github/scripts/check_pico_size_matrix.sh");
    let huge_budget_gate = include_str!("../.github/scripts/check_huge_choreography_budget.sh");

    for required in [
        "bash ./.github/scripts/check_hibana_public_api.sh",
        "bash ./.github/scripts/check_policy_surface_hygiene.sh",
        "bash ./.github/scripts/check_surface_hygiene.sh",
        "bash ./.github/scripts/check_lowering_hygiene.sh",
        "bash ./.github/scripts/check_boundary_contracts.sh",
        "bash ./.github/scripts/check_warning_free.sh",
        "bash ./.github/scripts/check_direct_projection_binary.sh",
        "bash ./.github/scripts/check_huge_choreography_budget.sh",
        "bash ./.github/scripts/check_pico_size_matrix.sh",
        "cargo check --all-targets -p hibana",
        "cargo test -p hibana --features std",
        "cargo test -p hibana --test ui --features std",
        "cargo test -p hibana --test policy_replay --features std",
    ] {
        assert!(
            readme.contains(required),
            "README must document the canonical hibana validation flow: {required}"
        );
    }

    for required in [
        "sudo apt-get update",
        "sudo apt-get install -y ripgrep",
        "./.github/scripts/check_hibana_public_api.sh",
        "./.github/scripts/check_policy_surface_hygiene.sh",
        "./.github/scripts/check_mgmt_boundary.sh",
        "./.github/scripts/check_plane_boundaries.sh",
        "./.github/scripts/check_resolver_context_surface.sh",
        "./.github/scripts/check_lowering_hygiene.sh",
        "./.github/scripts/check_surface_hygiene.sh",
        "./.github/scripts/check_warning_free.sh",
        "./.github/scripts/check_direct_projection_binary.sh",
        "./.github/scripts/check_huge_choreography_budget.sh",
        "./.github/scripts/check_no_std_build.sh",
        "./.github/scripts/check_pico_smoke.sh",
        "./.github/scripts/check_pico_size_matrix.sh",
        "cargo check --all-targets -p hibana",
        "cargo test -p hibana --features std",
        "cargo test -p hibana --test ui --features std",
        "cargo test -p hibana --test policy_replay --features std",
    ] {
        assert!(
            quality_workflow.contains(required),
            "quality-gates workflow must run the canonical hibana gate/test set: {required}"
        );
    }
    assert!(
        !quality_workflow.contains("./.github/scripts/check_policy_legacy_paths.sh"),
        "quality-gates workflow must not keep the stale policy-legacy gate name"
    );
    assert!(
        !quality_workflow.contains("name: Boundary contracts gate")
            && !quality_workflow.contains("run: ./.github/scripts/check_boundary_contracts.sh"),
        "quality-gates workflow must expose boundary failures as per-check steps instead of a single wrapper step"
    );

    for required in [
        "check_mgmt_boundary.sh",
        "check_plane_boundaries.sh",
        "check_resolver_context_surface.sh",
        "check_lowering_hygiene.sh",
        "check_surface_hygiene.sh",
    ] {
        assert!(
            boundary_gate.contains(required),
            "boundary contracts gate must aggregate the canonical boundary owner: {required}"
        );
    }

    for required in [
        "--test substrate_surface",
        "substrate_facade_projects_before_enter",
        "--test public_surface_guards",
        "route_projection_regression_fixtures_keep_canonical_inputs_live",
        "ui_diagnostics_stay_semantic",
    ] {
        assert!(
            direct_projection_gate.contains(required),
            "direct projection binary gate must keep the canonical typed-projection regression owner: {required}"
        );
    }

    for required in [
        "SMOKE_TARGET_DIR=\"$ROOT/target/pico_smoke\"",
        "SRAM_BUDGET=$((168 * 1024))",
        "PRACTICAL_FLASH_BUDGET=$((768 * 1024))",
        "PRACTICAL_STATIC_SRAM_BUDGET=$((48 * 1024))",
        "PRACTICAL_KERNEL_STACK_BUDGET=$((24 * 1024))",
        "PRACTICAL_PEAK_SRAM_BUDGET=$((96 * 1024))",
        "pico smoke practical contract exceeded for",
        "pico smoke kernel stack reserve bytes:",
        "pico smoke peak stack upper-bound bytes:",
        "pico smoke peak sram upper-bound bytes:",
        "pico smoke measured sidecar/scratch high-water bytes:",
        "pico smoke measured live slab bytes:",
        "pico smoke measured peak stack bytes:",
        "pico smoke measured peak sram bytes:",
    ] {
        assert!(
            pico_smoke_gate.contains(required),
            "pico smoke gate must keep the SRAM-first canonical settings: {required}"
        );
    }

    for forbidden in [
        "internal/pico_smoke/target",
        "HIBANA_PICO_ENFORCE_PRACTICAL_CONTRACT",
        "report-only",
    ] {
        assert!(
            !pico_smoke_gate.contains(forbidden),
            "pico smoke gate must not write nested target artefacts into the repo: {forbidden}"
        );
    }

    for required in [
        "run_shape route_heavy",
        "run_shape linear_heavy \"linear-heavy\"",
        "run_shape fanout_heavy \"fanout-heavy\"",
        "== resident size matrix ==",
        "huge_shape_matrix_resident_bytes_stay_measured_and_local",
    ] {
        assert!(
            pico_size_matrix_gate.contains(required),
            "pico size matrix gate must keep the fixed huge choreography shape matrix: {required}"
        );
    }

    for required in [
        "timeout_seconds = 180",
        "huge_choreography_compile",
        "huge_choreography_runtime",
        "huge_choreography_resident",
    ] {
        assert!(
            huge_budget_gate.contains(required),
            "huge choreography budget gate must keep the canonical timeout-bounded compile checks: {required}"
        );
    }

    for required in [
        "resident-shape name={name}",
        "compiled_program_header_bytes: size_of::<CompiledProgramImage>()",
        "compiled_role_header_bytes: size_of::<CompiledRoleImage>()",
        "RouteTable::storage_bytes(",
        "LoopTable::storage_bytes(",
        "let endpoint_layout = compiled_role.endpoint_arena_layout_for_binding(false);",
        "public_endpoint_storage_requirement(compiled_role, false);",
        "endpoint_bytes: endpoint_layout.total_bytes(),",
        "huge_shape_matrix_resident_bytes_stay_measured_and_local",
    ] {
        assert!(
            cluster_core.contains(required),
            "cluster core must keep the local resident measurement owner: {required}"
        );
    }
}

#[test]
fn huge_runtime_storage_story_stays_concrete() {
    let pico_smoke_src = include_str!("../internal/pico_smoke/src/main.rs");
    let huge_runtime_src = include_str!("../tests/huge_choreography_runtime.rs");

    for source in [pico_smoke_src, huge_runtime_src] {
        assert!(
            !source.contains("unsafe impl<T> Sync"),
            "huge choreography support must not reintroduce blanket generic Sync shims"
        );
        assert!(
            !source.contains("struct StaticCell"),
            "huge choreography support must not reintroduce the generic StaticCell pattern"
        );
    }

    for forbidden in [
        "init_empty(",
        "MaybeUninit<HugeKit>",
        "with_runtime_kit_ref(",
    ] {
        assert!(
            !huge_runtime_src.contains(forbidden),
            "huge choreography runtime must stay on the public canonical path: {forbidden}"
        );
    }

    for required in [
        "let kit = HugeKit::new(clock);",
        "project(&ROUTE_HEAVY_PROGRAM);",
        ".add_rendezvous_from_config(",
        ".enter(rv_id, sid, controller_program, NoBinding)",
        ".enter(rv_id, sid, worker_program, NoBinding)",
    ] {
        assert!(
            huge_runtime_src.contains(required),
            "huge choreography runtime must keep the public canonical path visible: {required}"
        );
    }
}
