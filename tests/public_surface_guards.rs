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

fn substrate_public_api_allowlist() -> &'static str {
    include_str!("../.github/allowlists/substrate-public-api.txt")
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
        lease_planner_ws.contains("pub(crate) trait LeaseSpecFacetNeeds {")
            && lease_planner_ws.contains("fn facet_needs() -> LeaseFacetNeeds;"),
        "lease-spec facet contract must require an explicit owner-side facet hook"
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
        "pub(crate) trait LeaseSpecFacetNeeds { fn facet_needs() -> LeaseFacetNeeds { Self::FACET_NEEDS } }",
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
    let cursor_src = include_str!("../src/endpoint/cursor.rs");
    let offer_body = impl_body(
        cursor_src,
        "pub async fn offer(self) -> RecvResult<RouteBranch<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>>",
    );
    let select_scope_body = impl_body(
        cursor_src,
        "fn select_scope(&mut self) -> RecvResult<OfferScopeSelection>",
    );
    let resolve_token_body = impl_body(cursor_src, "async fn resolve_token(");
    let materialize_branch_body = impl_body(cursor_src, "fn materialize_branch(");

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
            !cursor_src.contains(forbidden),
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
        materialize_branch_body.contains("self.materialize_selected_arm_meta("),
        "materialize_branch must remain the owner of branch metadata materialization"
    );
    for forbidden in [
        "take_scope_ack(",
        "peek_scope_ack(",
        "prepare_route_decision_from_resolver",
        "on_frontier_defer(",
    ] {
        assert!(
            !materialize_branch_body.contains(forbidden),
            "materialize_branch must not perform authority selection or defer logic: {forbidden}"
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
    let src = include_str!("../src/endpoint/cursor.rs");
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
        src.contains("pub(crate) const fn policies(&self) -> &[PolicyMarker]"),
        "EffList::policies should stay crate-private for internal policy metadata"
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
        ),
        "EffList::policy_with_scope must not remain public"
    );
    assert!(
        src.contains(
            "pub(crate) fn policy_with_scope(&self, offset: usize) -> Option<(PolicyMode, ScopeId)>"
        ),
        "EffList::policy_with_scope should stay crate-private"
    );
}

#[test]
fn eff_list_does_not_reintroduce_derived_lookup_tables() {
    let src = include_str!("../src/global/const_dsl.rs");
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
        "pub(crate) const fn first_dynamic_policy_in_range(",
        "pub(crate) const fn scope_id_for_offset(&self, offset: usize) -> Option<ScopeId> {",
        "pub const fn scope_has_linger(&self, scope: ScopeId) -> bool {",
    ] {
        assert!(
            src.contains(required),
            "EffList should keep direct lookup helpers while deriving them on demand: {required}"
        );
    }
}

#[test]
fn role_program_does_not_own_policy_marker_metadata_iterators() {
    let role_program_src = include_str!("../src/global/role_program.rs");
    let cluster_core_src = include_str!("../src/control/cluster/core.rs");

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
        cluster_core_src.contains("for marker in eff_list.policies() {"),
        "cluster owner must read raw policy markers directly from EffList"
    );
}

#[test]
fn role_program_projection_metadata_stays_internal() {
    let role_program_src = include_str!("../src/global/role_program.rs");
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
        "pub fn projection(&self) -> ProjectedRoleData<ROLE> {",
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
        "impl<'prog, const ROLE: u8, LocalSteps, Mint> core::ops::Deref",
        "impl<'prog, const ROLE: u8, LocalSteps, Mint> AsRef<[LocalStep]>",
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
        "pub(crate) struct ProjectedRoleData<const ROLE: u8> {",
        "fn layout(&self) -> ProjectedRoleLayout {",
        "pub(crate) fn projection(&self) -> ProjectedRoleData<ROLE> {",
        "pub(crate) fn active_lanes(&self) -> [bool; MAX_LANES] {",
        "let _ = super::typestate::RoleTypestate::<ROLE>::from_program(eff);",
    ] {
        assert!(
            role_program_src.contains(required),
            "RoleProgram projection-inspection helpers should stay crate-private: {required}"
        );
    }

    assert!(
        role_program_ws.contains(
            "pub struct RoleProgram<'prog, const ROLE: u8, LocalSteps, Mint = MintConfig> where Mint: MintConfigMarker, { eff_list: &'prog EffList, lease_budget: crate::control::lease::planner::LeaseGraphBudget, mint: Mint, _local_steps: core::marker::PhantomData<LocalSteps>, }"
        ),
        "RoleProgram must stay thin and store only eff list, lease budget, mint, and typed witness"
    );
    assert!(
        !role_program_ws.contains("LocalSteps = steps::StepNil"),
        "RoleProgram must not hide typed projection behind a StepNil default"
    );
    for forbidden in [
        "pub struct RoleProgram<'prog, const ROLE: u8, LocalSteps, Mint = MintConfig> where Mint: MintConfigMarker, { eff_list: &'prog EffList, lease_budget: crate::control::lease::planner::LeaseGraphBudget, local_steps:",
        "pub struct RoleProgram<'prog, const ROLE: u8, LocalSteps, Mint = MintConfig> where Mint: MintConfigMarker, { eff_list: &'prog EffList, lease_budget: crate::control::lease::planner::LeaseGraphBudget, mint: Mint, phases:",
        "pub struct RoleProgram<'prog, const ROLE: u8, LocalSteps, Mint = MintConfig> where Mint: MintConfigMarker, { eff_list: &'prog EffList, lease_budget: crate::control::lease::planner::LeaseGraphBudget, mint: Mint, typestate:",
    ] {
        assert!(
            !role_program_ws.contains(forbidden),
            "RoleProgram must stay thin and avoid storing materialized projection metadata: {forbidden}"
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
fn advanced_compose_seq_is_not_an_alias_shim() {
    let global_src = include_str!("../src/global.rs");
    let program_src = include_str!("../src/global/program.rs");
    let steps_src = include_str!("../src/global/steps.rs");

    assert!(
        global_src.contains("pub use super::super::program::seq;"),
        "g::advanced::compose must expose only the preserved composition constructor"
    );
    assert!(
        !global_src.contains("seq_preserving as seq"),
        "g::advanced::compose::seq must not be implemented as an alias shim"
    );
    assert!(
        !global_src.contains("pub use super::super::program::{empty, seq};")
            && !global_src.contains("pub use super::super::program::empty;"),
        "g::advanced::compose must not regrow a second zero-fragment builder surface"
    );
    assert!(
        program_src.contains("pub const fn seq<LeftSteps, RightSteps>("),
        "program.rs must define the canonical preserved composition constructor as seq"
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
        "impl_label_eq!(",
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
        "LeftSteps: StepConcat<RightSteps> + RouteArmHead + SameRouteController<RightSteps> + DistinctRouteLabels<RightSteps>",
        "RightSteps: RouteArmHead",
        "LeftSteps: StepConcat<RightSteps> + NonEmptyParallelArm",
        "RightSteps: NonEmptyParallelArm",
        "let controller = <<LeftSteps as RouteArmHead>::Controller as crate::global::RoleMarker>::INDEX;",
        "let left_set = <LeftSteps as NonEmptyParallelArm>::ROLE_LANE_SET;",
        "let right_set = <RightSteps as NonEmptyParallelArm>::ROLE_LANE_SET;",
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
    let api_sketch_src = include_str!("../../api-sketch.md");
    let app_path = readme_src
        .split("## Substrate Surface (protocol implementors only)")
        .next()
        .expect("README app-facing section");

    assert!(
        !readme_src.contains("project::<"),
        "README must not keep project::<...> turbofish shims"
    );
    assert!(
        !api_sketch_src.contains("project::<"),
        "api-sketch must not keep project::<...> turbofish shims"
    );
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
        ("ui-pass/g-efflist-deref.rs", "type Steps = StepCons<"),
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
            "type Cluster = SessionCluster<'static, TestTransport, DefaultLabelUniverse, CounterClock, 4>;",
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
        ("endpoint/cursor.rs", "type HintController = Role<0>;"),
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
        route_policy_mismatch.contains("hibana::impl_control_resource!(")
            && route_policy_mismatch.contains("ArmWithPolicyKind,")
            && route_policy_mismatch.contains("ArmWithoutPolicyKind,"),
        "g-route-policy-mismatch must define route-control fixtures through the canonical impl_control_resource! owner"
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
        ) && !route_policy_mismatch.contains("struct ArmKind<const LABEL: u8>;")
            && !route_policy_mismatch
                .contains("impl<const LABEL: u8> ResourceKind for ArmKind<LABEL>"),
        "g-route-policy-mismatch must use concrete canonical control kinds instead of test-local manual ResourceKind boilerplate"
    );
    assert!(
        route_unprojectable.contains("hibana::impl_control_resource!(")
            && route_unprojectable.contains("RouteArm100Kind,")
            && route_unprojectable.contains("RouteArm101Kind,"),
        "g-route-unprojectable must define route-control fixtures through the canonical impl_control_resource! owner"
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
        ) && !route_unprojectable.contains("struct RouteArmKind<const LABEL: u8>;")
            && !route_unprojectable
                .contains("impl<const LABEL: u8> ResourceKind for RouteArmKind<LABEL>"),
        "g-route-unprojectable must use concrete canonical control kinds instead of test-local manual ResourceKind boilerplate"
    );
    assert!(
        route_unprojectable.contains("let _ = PASSIVE_PROGRAM.eff_list();"),
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
fn route_control_fixtures_use_canonical_macro_owner() {
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
            src.contains("hibana::impl_control_resource!(") && src.contains(required),
            "{path} must define route-control tokens through the canonical impl_control_resource! owner"
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
        "pub(crate) const fn into_eff(self) -> EffList",
        "pub(crate) const fn scope_budget(&self) -> u16",
        "pub(crate) const fn then<NextSteps>(",
        "pub const fn policy<const POLICY_ID: u16>(self) -> Self",
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
        "SendMetadata as SendMeta",
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
        "substrate surface must expose the canonical effect index owner used by ResolverContext and SendMetadata"
    );
    for forbidden in [
        "pub fn len(&self) -> usize {",
        "pub fn is_empty(&self) -> bool {",
    ] {
        assert!(
            !substrate_src.contains(forbidden),
            "SessionCluster must not expose non-canonical collection helpers on the substrate surface: {forbidden}"
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
    let public_methods: Vec<_> = resolver_impl
        .lines()
        .map(str::trim_start)
        .filter(|line| {
            line.starts_with("pub fn ")
                || line.starts_with("pub const fn ")
                || line.starts_with("pub async fn ")
                || line.starts_with("pub unsafe fn ")
        })
        .map(|line| {
            line.split("fn ")
                .nth(1)
                .and_then(|rest| rest.split('(').next())
                .expect("ResolverContext public method name")
        })
        .collect();
    assert_eq!(
        public_methods,
        vec!["attr", "input"],
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
        substrate_src.contains("DynamicResolution, PolicyId, ResolverContext, ResolverError"),
        "substrate policy surface must re-export ResolverError next to the resolver callback contracts"
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
        !effects_src.contains("pub fn interpret_eff_list("),
        "private cluster effects owner must not expose interpret_eff_list publicly"
    );
    assert!(
        effects_src.contains("pub(crate) fn interpret_eff_list("),
        "interpret_eff_list should stay crate-private"
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
        "pub(crate) const fn new() -> Self {",
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
        "pub(crate) const fn empty() -> Self {",
        "pub(crate) fn cp_effects(&self) -> impl Iterator<Item = &CpEffect> {",
        "pub(crate) fn resources(&self) -> impl Iterator<Item = &ResourceDescriptor> {",
        "pub(crate) fn controls(&self) -> impl Iterator<Item = &ControlMarker> {",
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
        "pub(crate) struct TapBatch {",
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
    for forbidden in [
        "transmute::<usize, fn(u32)>",
        "static TS_CHECKER: AtomicUsize",
    ] {
        assert!(
            !observe_core_src.contains(forbidden),
            "observe timestamp checker must not hide function pointers behind integer transmute shims: {forbidden}"
        );
    }
    for required in [
        "static TS_CHECKER: Mutex<Option<fn(u32)>> = Mutex::new(None);",
        "*TS_CHECKER.lock().expect(\"timestamp checker mutex poisoned\")",
        "core::mem::replace(&mut *checker, new)",
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
        "pub(crate) enum LoaderError",
        "pub(crate) struct ImageLoader",
        "pub(crate) const fn new() -> Self",
        "pub(crate) fn begin(&mut self, header: Header) -> Result<(), LoaderError>",
        "pub(crate) fn write(&mut self, offset: u32, chunk: &[u8]) -> Result<(), LoaderError>",
        "pub(crate) fn commit_for_slot(&mut self, slot: Slot) -> Result<VerifiedImage<'_>, LoaderError>",
        "pub(crate) enum VerifyError",
        "pub(crate) struct VerifiedImage<'a> {",
        "pub(crate) const MAX_CODE_LEN: usize = 2048;",
        "pub(crate) fn new(bytes: &'a [u8]) -> Result<Self, VerifyError>",
        "pub(crate) fn new_for_slot(bytes: &'a [u8], slot: Slot) -> Result<Self, VerifyError>",
        "pub(crate) const fn policy_mode_tag(",
        "pub(crate) const fn verdict_tag(",
        "pub(crate) const fn verdict_arm(",
        "pub(crate) const fn verdict_reason(",
        "pub(crate) const fn slot_tag(",
        "pub(crate) fn hash_tap_event(",
        "pub(crate) fn hash_policy_input(",
        "pub(crate) fn hash_transport_snapshot(",
        "pub(crate) fn run_with<",
        "pub(crate) fn compute_hash(code: &[u8]) -> u32",
    ] {
        assert!(
            epf_src.contains(required)
                || loader_src.contains(required)
                || verifier_src.contains(required),
            "EPF helper should stay crate-private: {required}"
        );
    }
    for required in [
        "pub struct Header {",
        "pub const MAGIC: [u8; 4]",
        "pub const SIZE: usize",
        "pub const fn max_mem_len() -> usize",
    ] {
        assert!(
            verifier_src.contains(required),
            "substrate EPF surface still depends on the canonical public Header owner: {required}"
        );
    }
    for required in [
        "pub(crate) fn parse(bytes: &[u8]) -> Result<Self, VerifyError>",
        "pub(crate) fn encode_into(&self, buf: &mut [u8; Self::SIZE])",
    ] {
        assert!(
            verifier_src.contains(required),
            "Header parsing/encoding should stay verifier-internal even though Header itself is public: {required}"
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
        "pub(crate) const fn slot_allows_get_input(",
        "pub(crate) const fn slot_allows_mem_ops(",
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
        "pub struct ScopeEntry {",
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
        "pub(crate) struct ScopeEntry {",
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

    for forbidden in [
        "pub type EffIndex = u16;",
        "pub eff_index: u16,",
        "pub type LabelId = u8;",
        "pub type ResourceKindId = u8;",
        "pub type EffSlice = &'static [EffStruct];",
    ] {
        assert!(
            !eff_src.contains(forbidden) && !binding_src.contains(forbidden),
            "effect index must not regress to a raw integer alias or binding metadata field: {forbidden}"
        );
    }

    for required in [
        "pub struct EffIndex(u16);",
        "pub eff_index: EffIndex,",
        "pub struct EffSlice(&'static [EffStruct]);",
    ] {
        assert!(
            eff_src.contains(required) || binding_src.contains(required),
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
        typestate_src.contains("pub(crate) struct RouteRecvIndex(u16);"),
        "route recv list indices must keep their canonical newtype owner"
    );
    for forbidden in [
        "pub struct RouteRecvIndex(u16);",
        "pub const MAX_STATES: usize =",
        "pub enum JumpReason {",
        "pub struct JumpError {",
        "pub enum PassiveArmNavigation {",
        "pub enum LoopRole {",
        "pub struct LoopMetadata<const ROLE: u8> {",
        "pub struct PhaseCursor<const ROLE: u8> {",
        "pub fn phase_cursor(&'prog self) -> PhaseCursor<ROLE> {",
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
        "pub(crate) struct LoopMetadata<const ROLE: u8> {",
        "pub(crate) struct PhaseCursor<const ROLE: u8> {",
        "pub(crate) fn phase_cursor(&'prog self) -> PhaseCursor<ROLE> {",
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
    let mgmt_kernel_src = include_str!("../src/runtime/mgmt/kernel.rs");
    let trace_src = include_str!("../src/transport/trace.rs");
    let wire_src = include_str!("../src/transport/wire.rs");
    let endpoint_src = include_str!("../src/endpoint.rs");
    let affine_src = include_str!("../src/endpoint/affine.rs");
    let control_src = include_str!("../src/endpoint/control.rs");
    let cursor_src = include_str!("../src/endpoint/cursor.rs");
    let flow_src = include_str!("../src/endpoint/flow.rs");
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
                && !trace_src.contains(forbidden)
                && !wire_src.contains(forbidden)
                && !endpoint_src.contains(forbidden)
                && !affine_src.contains(forbidden)
                && !control_src.contains(forbidden)
                && !cursor_src.contains(forbidden)
                && !observe_core_src.contains(forbidden),
            "internal lower-layer owner must not stay public: {forbidden}"
        );
    }

    for required in [
        "pub(crate) fn into_await_begin(self) -> Manager<AwaitBegin, SLOTS> {",
        "pub(crate) fn stats(&self, slot: Slot) -> Result<StatsResp, MgmtError> {",
        "pub(crate) fn staged_version(&self, slot: Slot) -> Option<u32> {",
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
                || trace_src.contains(required)
                || wire_src.contains(required)
                || endpoint_src.contains(required)
                || affine_src.contains(required)
                || control_src.contains(required)
                || cursor_src.contains(required)
                || observe_core_src.contains(required),
            "internal lower-layer owner should stay crate-private: {required}"
        );
    }

    assert!(
        typestate_src.contains("pub(crate) fn assert_terminal(&self) {"),
        "global typestate cursor should keep assert_terminal crate-private"
    );
    assert!(
        typestate_src.contains("#[cfg(test)]\n    pub(crate) fn assert_terminal(&self) {"),
        "global typestate cursor should keep assert_terminal unit-test-only"
    );

    assert!(
        flow_src.contains("pub(crate) async fn send<'a, A>("),
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
        flow_src.matches("pub async fn send<'a, A>(").count() == 1,
        "public flow facade should keep exactly one public send entrypoint"
    );
    assert!(
        !cursor_src.contains(".then("),
        "endpoint cursor owners and their unit tests must not depend on hidden Program::then"
    );
    assert!(
        mgmt_kernel_src.matches(".then(").count() == 1
            && mgmt_kernel_src.contains(
                "STREAM_LOOP_BREAK_PREFIX.then(g::send::<\n    g::Role<1>,\n    g::Role<0>,\n    g::Msg<LABEL_OBSERVE_STREAM_END, ()>,\n    0,\n>())"
            ),
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
fn core_sources_do_not_keep_env_debug_escape_hatches() {
    let lib_src = include_str!("../src/lib.rs");
    let cursor_src = include_str!("../src/endpoint/cursor.rs");

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
    let cursor_src = include_str!("../src/endpoint/cursor.rs");
    let endpoint_src = include_str!("../src/endpoint.rs");

    assert!(
        cluster_core_src.contains(
            "pub(crate) fn attach_endpoint<'lease, 'prog, const ROLE: u8, LocalSteps, Mint, B>("
        ),
        "cluster lower layer must expose the canonical attach_endpoint helper"
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
    let mgmt_kernel_src = include_str!("../src/runtime/mgmt/kernel.rs");
    let transport_context_src = include_str!("../src/transport/context.rs");
    let tables_src = include_str!("../src/rendezvous/tables.rs");
    let typestate_src = include_str!("../src/global/typestate.rs");
    let typestate_ws = compact_ws(typestate_src);
    let steps_ws = compact_ws(steps_src);
    let global_src_full = include_str!("../src/global.rs");
    let role_program_src = include_str!("../src/global/role_program.rs");

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
        (endpoint_src, "pub(crate) mod cursor;", "pub mod cursor;"),
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
        (epf_src, "pub(crate) mod loader;", "pub mod loader;"),
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
        "pub struct FenceTable {",
        "pub struct AckTable {",
        "pub struct PolicyTable {",
        "pub struct VmCapsTable {",
        "pub struct CheckpointTable {",
        "pub struct SlotStorage {",
        "pub struct SlotArena {",
        "pub struct Rendezvous<",
        "pub struct SlotBundle<'rv, 'cfg: 'rv> {",
        "pub struct SlotBundleLease<'rv, 'cfg: 'rv> {",
        "pub struct LaneLease<'cfg, T, U, C, const MAX_RV: usize>",
        "pub struct CapsFacet<T, U, C, E>(PhantomData<(T, U, C, E)>)",
        "pub struct SpliceFacet<T, U, C, E>(PhantomData<(T, U, C, E)>)",
        "pub struct ObserveFacet<'tap, 'cfg> {",
        "pub struct SlotFacet<T, U, C, E>(PhantomData<(T, U, C, E)>)",
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
        "pub(crate) struct FenceTable {",
        "pub(crate) struct AckTable {",
        "pub(crate) struct PolicyTable {",
        "pub(crate) struct VmCapsTable {",
        "pub(crate) struct CheckpointTable {",
        "pub(crate) struct SlotStorage {",
        "pub(crate) struct SlotArena {",
        "pub(crate) struct Rendezvous<",
        "pub(crate) struct SlotBundle<'rv, 'cfg: 'rv> {",
        "pub(crate) struct SlotBundleLease<'rv, 'cfg: 'rv> {",
        "pub(crate) struct LaneLease<'cfg, T, U, C, const MAX_RV: usize>",
        "pub(crate) struct CapsFacet<T, U, C, E>(PhantomData<(T, U, C, E)>)",
        "pub(crate) struct SpliceFacet<T, U, C, E>(PhantomData<(T, U, C, E)>)",
        "pub(crate) struct ObserveFacet<'tap, 'cfg> {",
        "pub(crate) struct SlotFacet<T, U, C, E>(PhantomData<(T, U, C, E)>)",
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
        "pub fn initialise_control_marker(&self, lane: Lane, marker: &ControlMarker) {",
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
        "pub(crate) fn from_config(config: Config<'cfg, U, C>, transport: T) -> Self {",
        "pub(crate) fn initialise_control_marker(&self, lane: Lane, marker: &ControlMarker) {",
        "pub(crate) fn is_session_registered(&self, sid: SessionId) -> bool {",
        "pub(crate) fn release_lane(&self, lane: Lane) -> Option<SessionId> {",
        "pub(crate) fn into_port_guard(",
        "pub(crate) fn id(&self) -> RendezvousId {",
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
        "pub struct SlotSpec;",
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
        "pub trait LeaseSpecFacetNeeds {",
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
        "pub(crate) struct SlotSpec;",
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
        "pub(crate) trait LeaseSpec {",
        "pub(crate) struct FacetHandle<",
        "pub(crate) enum LeaseGraphError {",
        "pub(crate) struct LeaseGraph<",
        "pub(crate) struct ArrayMap<",
        "pub(crate) struct LeaseFacetNeeds {",
        "pub(crate) struct LeaseGraphBudget {",
        "pub(crate) trait LeaseSpecFacetNeeds {",
        "pub(crate) const fn lease_budget(&self) -> crate::control::lease::planner::LeaseGraphBudget {",
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
        !runtime_src.contains("pub(crate) use crate::control::cluster::core::SessionCluster;"),
        "runtime must not keep a crate-private SessionCluster alias shell"
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
    for forbidden in [
        "PolicyEvent,",
        "PolicyEventKind,",
        "TapBatch,",
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
        "type LoopSegmentSteps =",
        "type AfterLoop =",
        "type AfterCommit =",
        "type FullProgramSteps =",
        "type ControllerLocal =",
        "type ClusterLocal =",
        "type StreamLoopContinueSteps =",
        "type StreamLoopBreakSteps =",
        "type StreamLoopRouteSteps =",
        "type StreamProgramSteps =",
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
            !mgmt_kernel_src.contains(forbidden),
            "runtime management kernel must not hide canonical message/composition/projection owners behind local aliases: {forbidden}"
        );
    }
    for required in [
        "const LOAD_BEGIN: Program<",
        "const PROGRAM: Program<",
        "LoopDecisionSteps<",
        "LABEL_LOOP_CONTINUE,",
        "LABEL_LOOP_BREAK,",
        "LABEL_MGMT_LOAD_BEGIN,",
        "LABEL_MGMT_LOAD_CHUNK,",
        "LABEL_MGMT_ACTIVATE,",
        "LABEL_MGMT_REVERT,",
        "LABEL_MGMT_STATS,",
        "const STREAM_PROGRAM: Program<",
        "LABEL_OBSERVE_SUBSCRIBE,",
        "LABEL_OBSERVE_STREAM_END,",
        "LABEL_OBSERVE_BATCH,",
        "crate::g::advanced::compose::seq(STREAM_SUBSCRIBE, STREAM_LOOP_ROUTE);",
    ] {
        assert!(
            mgmt_kernel_src.contains(required),
            "runtime management kernel must keep preserved composition on direct canonical witnesses: {required}"
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

    for required in [
        "bash ./.github/scripts/check_hibana_public_api.sh",
        "bash ./.github/scripts/check_policy_surface_hygiene.sh",
        "bash ./.github/scripts/check_surface_hygiene.sh",
        "bash ./.github/scripts/check_boundary_contracts.sh",
        "bash ./.github/scripts/check_direct_projection_binary.sh",
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
        "./.github/scripts/check_hibana_public_api.sh",
        "./.github/scripts/check_policy_surface_hygiene.sh",
        "./.github/scripts/check_boundary_contracts.sh",
        "./.github/scripts/check_direct_projection_binary.sh",
        "./.github/scripts/check_no_std_build.sh",
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

    for required in [
        "check_mgmt_boundary.sh",
        "check_plane_boundaries.sh",
        "check_resolver_context_surface.sh",
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
}

#[test]
fn workspace_quality_workflow_keeps_canonical_hibana_flow() {
    let workspace_workflow = include_str!("../../.github/workflows/macos-udp-tokio.yml");

    for required in [
        "./.github/scripts/check_hibana_public_api.sh",
        "./.github/scripts/check_policy_surface_hygiene.sh",
        "./.github/scripts/check_boundary_contracts.sh",
        "./hibana/.github/scripts/check_no_std_build.sh",
        "./hibana/.github/scripts/check_direct_projection_binary.sh",
        "cargo check --all-targets -p hibana --manifest-path hibana/Cargo.toml",
        "cargo test -p hibana --features std --manifest-path hibana/Cargo.toml",
        "cargo test -p hibana --test ui --features std --manifest-path hibana/Cargo.toml",
        "cargo test -p hibana --test policy_replay --features std --manifest-path hibana/Cargo.toml",
    ] {
        assert!(
            workspace_workflow.contains(required),
            "workspace workflow must run the canonical hibana verification flow: {required}"
        );
    }
    assert!(
        !workspace_workflow.contains("./.github/scripts/check_policy_legacy_paths.sh"),
        "workspace workflow must not keep the stale policy-legacy gate name"
    );

    for forbidden in [
        "cargo test --test docs_surface --features std --manifest-path hibana/Cargo.toml",
        "cargo test --test root_surface --features std --manifest-path hibana/Cargo.toml",
        "cargo test --test public_surface_guards --features std --manifest-path hibana/Cargo.toml",
        "cargo test --test substrate_surface --features std --manifest-path hibana/Cargo.toml",
    ] {
        assert!(
            !workspace_workflow.contains(forbidden),
            "workspace workflow must not bypass the canonical hibana std test with cherry-picked stale test entries: {forbidden}"
        );
    }
}
