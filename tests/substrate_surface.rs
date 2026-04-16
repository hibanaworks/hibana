mod common;
#[path = "support/runtime.rs"]
mod runtime_support;
#[path = "support/tls_ref.rs"]
mod tls_ref_support;

use ::core::{cell::UnsafeCell, mem::MaybeUninit};
use std::fs;
use std::path::{Path, PathBuf};

use hibana::g;
use hibana::g::advanced::steps::{SendStep, SeqSteps, StepCons, StepNil};
use hibana::g::advanced::{RoleProgram, project};
use hibana::substrate::{
    SessionId, SessionKit,
    binding::NoBinding,
    cap::advanced::MintConfig,
    policy::{DynamicResolution, ResolverContext, ResolverError, ResolverRef},
    runtime::{Config, CounterClock, DefaultLabelUniverse},
};
use hibana::{Endpoint, RouteBranch};
use static_assertions::assert_not_impl_any;
use tls_ref_support::with_tls_ref;
const PROGRAM: g::Program<StepCons<SendStep<g::Role<0>, g::Role<1>, g::Msg<1, u8>, 0>, StepNil>> =
    g::send::<g::Role<0>, g::Role<1>, g::Msg<1, u8>, 0>();
type ConnectionSteps =
    SeqSteps<StepNil, StepCons<SendStep<g::Role<0>, g::Role<1>, g::Msg<1, u8>, 0>, StepNil>>;
const CONNECTION_SOURCE: g::Program<ConnectionSteps> = g::seq(StepNil::PROGRAM, PROGRAM);
static CLIENT_PROGRAM: RoleProgram<'static, 0> = project(&PROGRAM);
type StaticTestKit =
    SessionKit<'static, common::TestTransport, DefaultLabelUniverse, CounterClock, 2>;

std::thread_local! {
    static SESSION_SLOT: UnsafeCell<MaybeUninit<StaticTestKit>> = const {
        UnsafeCell::new(MaybeUninit::uninit())
    };
}

assert_not_impl_any!(StaticTestKit: Send, Sync);
assert_not_impl_any!(Endpoint<'static, 0, StaticTestKit, MintConfig>: Send, Sync);
assert_not_impl_any!(RouteBranch<'static, 'static, 0, StaticTestKit, MintConfig>: Send, Sync);

fn substrate_rs() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/substrate.rs");
    fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("read {} failed: {}", path.display(), err))
}

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

fn runtime_rs() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/runtime.rs");
    fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("read {} failed: {}", path.display(), err))
}

fn runtime_mgmt_rs() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/runtime/mgmt.rs");
    fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("read {} failed: {}", path.display(), err))
}

fn runtime_mgmt_payload_rs() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/runtime/mgmt/payload.rs");
    fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("read {} failed: {}", path.display(), err))
}

fn runtime_mgmt_request_reply_rs() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/runtime/mgmt/request_reply.rs");
    fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("read {} failed: {}", path.display(), err))
}

fn runtime_mgmt_observe_stream_rs() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/runtime/mgmt/observe_stream.rs");
    fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("read {} failed: {}", path.display(), err))
}

fn runtime_mgmt_test_support_rs() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/runtime/mgmt/test_support.rs");
    fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("read {} failed: {}", path.display(), err))
}

fn substrate_public_api_allowlist() -> &'static str {
    include_str!("../.github/allowlists/substrate-public-api.txt")
}

fn rendezvous_core_rs() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/rendezvous/core.rs");
    fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("read {} failed: {}", path.display(), err))
}

fn cluster_core_rs() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/control/cluster/core.rs");
    fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("read {} failed: {}", path.display(), err))
}

fn repo_boundary_gate_rs() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".github/scripts/check_boundary_contracts.sh");
    fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("read {} failed: {}", path.display(), err))
}

fn repo_mgmt_boundary_gate_rs() -> String {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".github/scripts/check_mgmt_boundary.sh");
    fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("read {} failed: {}", path.display(), err))
}

fn repo_plane_boundary_gate_rs() -> String {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".github/scripts/check_plane_boundaries.sh");
    fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("read {} failed: {}", path.display(), err))
}

fn repo_surface_hygiene_gate_rs() -> String {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".github/scripts/check_surface_hygiene.sh");
    fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("read {} failed: {}", path.display(), err))
}

fn repo_lowering_hygiene_gate_rs() -> String {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".github/scripts/check_lowering_hygiene.sh");
    fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("read {} failed: {}", path.display(), err))
}

fn direct_projection_binary_check_rs() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".github/scripts/check_direct_projection_binary.sh");
    fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("read {} failed: {}", path.display(), err))
}

fn quality_workflow_rs() -> String {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".github/workflows/quality-gates.yml");
    fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("read {} failed: {}", path.display(), err))
}

fn visit_rs_files(root: &Path, f: &mut impl FnMut(&Path)) {
    for entry in fs::read_dir(root)
        .unwrap_or_else(|err| panic!("read_dir {} failed: {}", root.display(), err))
    {
        let entry =
            entry.unwrap_or_else(|err| panic!("read_dir entry {} failed: {}", root.display(), err));
        let path = entry.path();
        if path.is_dir() {
            visit_rs_files(&path, f);
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            f(&path);
        }
    }
}

fn read_repo_test(path: &str) -> String {
    let full = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path);
    fs::read_to_string(&full)
        .unwrap_or_else(|err| panic!("read {} failed: {}", full.display(), err))
}

fn contains_ascii_word(haystack: &str, needle: &str) -> bool {
    let needle = needle.as_bytes();
    haystack
        .as_bytes()
        .windows(needle.len())
        .enumerate()
        .any(|(idx, window)| {
            window.eq_ignore_ascii_case(needle)
                && (idx == 0 || !haystack.as_bytes()[idx - 1].is_ascii_alphanumeric())
                && (idx + needle.len() == haystack.len()
                    || !haystack.as_bytes()[idx + needle.len()].is_ascii_alphanumeric())
        })
}

fn defer_resolver(_ctx: ResolverContext) -> Result<DynamicResolution, ResolverError> {
    Ok(DynamicResolution::Defer { retry_hint: 1 })
}

#[test]
fn substrate_internal_carrier_stays_private() {
    let substrate_src = substrate_rs();
    let runtime_mgmt_src = runtime_mgmt_rs();
    let allowlist = substrate_public_api_allowlist();

    assert!(
        substrate_src.contains("type KernelSessionCluster<'cfg, T, U, C, const MAX_RV: usize> =")
            || runtime_mgmt_src
                .contains("type KernelSessionCluster<'cfg, T, U, C, const MAX_RV: usize> ="),
        "substrate/runtime lower layer must keep the internal session-cluster owner alias"
    );
    assert!(
        !runtime_mgmt_src.contains("endpoint::carrier::PublicEndpoint")
            && !substrate_src.contains("endpoint::carrier::PublicEndpoint"),
        "mgmt root surface must not regrow direct endpoint-carrier aliases"
    );
    for forbidden in [
        "SessionCfg",
        "EndpointCfg",
        "SessionCarrier",
        "EndpointCarrier",
    ] {
        assert!(
            !allowlist.contains(forbidden),
            "carrier internals must not leak into substrate public surface allowlist: {forbidden}"
        );
    }
}

#[test]
fn hibana_core_source_stays_protocol_neutral() {
    let hygiene_gate = repo_surface_hygiene_gate_rs();
    assert!(
        hygiene_gate.contains("protocol-specific vocabulary in hibana core"),
        "surface hygiene gate must reject protocol-specific vocabulary from hibana core"
    );

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    visit_rs_files(&root, &mut |path| {
        let body = fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("read {} failed: {}", path.display(), err));
        for forbidden in ["quic", "h3", "hq"] {
            assert!(
                !contains_ascii_word(&body, forbidden),
                "hibana core must stay protocol-neutral: {} contains `{forbidden}`",
                path.display()
            );
        }
    });
}

#[test]
fn substrate_facade_exposes_enter_and_policy_resolver_registration() {
    runtime_support::with_fixture(|clock, tap_buf, slab| {
        let transport = common::TestTransport::default();
        with_tls_ref(
            &SESSION_SLOT,
            |ptr| unsafe {
                ptr.write(StaticTestKit::new(clock));
            },
            |cluster| {
                let rv_id = cluster
                    .add_rendezvous_from_config(Config::new(tap_buf, slab), transport)
                    .expect("add rendezvous");

                cluster
                    .set_resolver::<7, 0, _>(
                        rv_id,
                        &CLIENT_PROGRAM,
                        ResolverRef::from_fn(defer_resolver),
                    )
                    .expect("install resolver");

                let endpoint = cluster
                    .enter(rv_id, SessionId::new(1), &CLIENT_PROGRAM, NoBinding)
                    .expect("enter endpoint");
                let _: &Endpoint<'_, 0, StaticTestKit, MintConfig> = &endpoint;
            },
        );
    });
}

#[test]
fn substrate_facade_drops_canonical_token_helpers() {
    let substrate_rs = substrate_rs();
    let allowlist = substrate_public_api_allowlist();

    for forbidden in [
        "pub fn canonical_session_token",
        "pub fn canonical_token_with_handle",
    ] {
        assert!(
            !substrate_rs.contains(forbidden),
            "substrate::SessionKit must not expose canonical token helper surface: {forbidden}"
        );
        assert!(
            !allowlist.contains(forbidden),
            "substrate public API allowlist must not keep deleted canonical token helpers: {forbidden}"
        );
    }
}

#[test]
fn substrate_facade_keeps_enter_as_the_only_public_attach_entry() {
    let substrate_src = substrate_rs();
    let allowlist = substrate_public_api_allowlist();

    for forbidden in ["pub unsafe fn init_in_place", "pub unsafe fn enter_into"] {
        assert!(
            !substrate_src.contains(forbidden),
            "substrate::SessionKit must not expose placement helper surface: {forbidden}"
        );
        assert!(
            !allowlist.contains(forbidden),
            "substrate public API allowlist must not keep deleted placement helpers: {forbidden}"
        );
    }

    assert!(
        substrate_src.contains("pub fn enter<'r, const ROLE: u8, Mint, B>("),
        "substrate::SessionKit must keep enter(...) as the canonical public attach entry"
    );
    assert!(
        substrate_src.contains("program: &crate::g::advanced::RoleProgram<'_, ROLE, Mint>,"),
        "substrate::SessionKit::enter must accept projected programs without extending the endpoint lifetime"
    );
    assert!(
        !substrate_src
            .contains("program: &'prog crate::g::advanced::RoleProgram<'prog, ROLE, Mint>,")
            && !substrate_src.contains("pub fn enter<'r, 'prog, const ROLE: u8, Mint, B>("),
        "substrate::SessionKit::enter must not tie endpoint lifetime to the projected RoleProgram borrow"
    );
}

#[test]
fn substrate_facade_projects_before_enter() {
    let connection = CONNECTION_SOURCE;
    let program: RoleProgram<'_, 0, MintConfig> = project(&connection);

    let _ = &program;
}

#[test]
fn direct_binary_projection_regression_has_a_dedicated_check_script() {
    let script = direct_projection_binary_check_rs();

    for required in [
        "--test substrate_surface",
        "substrate_facade_projects_before_enter",
        "--no-run",
        "--message-format=json",
        "\"name\":\"substrate_surface\"",
        "\"executable\":",
        "timeout 30s",
    ] {
        assert!(
            script.contains(required),
            "direct-binary projection regression check must pin the stack-overflow reproduction path: {required}"
        );
    }
}

#[test]
fn quality_workflow_runs_canonical_validation_suite() {
    let workflow = quality_workflow_rs();

    for required in [
        "sudo apt-get update",
        "sudo apt-get install -y ripgrep",
        "./.github/scripts/check_hibana_public_api.sh",
        "./.github/scripts/check_policy_surface_hygiene.sh",
        "./.github/scripts/check_mgmt_boundary.sh",
        "./.github/scripts/check_plane_boundaries.sh",
        "./.github/scripts/check_resolver_context_surface.sh",
        "./.github/scripts/check_lowering_hygiene.sh",
        "./.github/scripts/check_summary_authority_hygiene.sh",
        "./.github/scripts/check_exact_layout_hygiene.sh",
        "./.github/scripts/check_route_frontier_owner.sh",
        "./.github/scripts/check_surface_hygiene.sh",
        "./.github/scripts/check_warning_free.sh",
        "./.github/scripts/check_direct_projection_binary.sh",
        "./.github/scripts/check_huge_choreography_budget.sh",
        "./.github/scripts/check_subsystem_budget_gates.sh",
        "./.github/scripts/check_pico_size_matrix.sh",
        "cargo check --all-targets -p hibana",
        "cargo test -p hibana --features std",
        "cargo test -p hibana --test ui --features std",
        "cargo test -p hibana --test policy_replay --features std",
    ] {
        assert!(
            workflow.contains(required),
            "quality-gates workflow must run the canonical validation suite: {required}"
        );
    }
    assert!(
        !workflow.contains("./.github/scripts/check_policy_legacy_paths.sh"),
        "quality-gates workflow must not keep the stale policy-legacy gate name"
    );
    assert!(
        !workflow.contains("name: Boundary contracts gate")
            && !workflow.contains("run: ./.github/scripts/check_boundary_contracts.sh"),
        "quality-gates workflow must split boundary checks into UI-visible substeps instead of a single wrapper step"
    );
}

#[test]
fn substrate_facade_accepts_non_static_projected_programs() {
    fn enter_from_inner_scope<'a>(
        cluster: &'a StaticTestKit,
        rv_id: SessionId,
        rendezvous: hibana::substrate::RendezvousId,
    ) -> Endpoint<'a, 0, StaticTestKit, MintConfig> {
        let connection = CONNECTION_SOURCE;
        let program: RoleProgram<'_, 0, MintConfig> = project(&connection);
        cluster
            .enter(rendezvous, rv_id, &program, NoBinding)
            .expect("enter endpoint")
    }

    runtime_support::with_fixture(|clock, tap_buf, slab| {
        let transport = common::TestTransport::default();
        with_tls_ref(
            &SESSION_SLOT,
            |ptr| unsafe {
                ptr.write(StaticTestKit::new(clock));
            },
            |cluster| {
                let rv_id = cluster
                    .add_rendezvous_from_config(Config::new(tap_buf, slab), transport)
                    .expect("add rendezvous");
                let endpoint = enter_from_inner_scope(cluster, SessionId::new(2), rv_id);
                let _: &Endpoint<'_, 0, StaticTestKit, MintConfig> = &endpoint;
            },
        );
    });
}

#[test]
fn substrate_cluster_registration_avoids_rendezvous_stack_temporary() {
    let cluster_core_src = cluster_core_rs();
    let cluster_core_ws = compact_ws(&cluster_core_src);

    assert!(
        cluster_core_ws
            .contains("core .locals .register_local_from_config_auto(config, transport)"),
        "cluster rendezvous registration must construct directly inside the lease-core owner slot"
    );
    for forbidden in [
        "let rv = Rendezvous::from_config(config, transport);",
        "self.add_rendezvous(rv)",
    ] {
        assert!(
            !cluster_core_src.contains(forbidden),
            "cluster rendezvous registration must not materialize a large stack temporary: {forbidden}"
        );
    }
}

#[test]
fn runtime_support_uses_local_borrowed_fixture_owners() {
    let runtime_support_src = read_repo_test("tests/support/runtime.rs");

    assert!(
        runtime_support_src.contains("UnsafeCell<[TapEvent; RING_EVENTS]>")
            && runtime_support_src.contains("std::thread_local!"),
        "runtime support must keep tap/slab storage in thread-local borrowed fixture owners"
    );
    assert!(
        !runtime_support_src.contains("Box::")
            && !runtime_support_src.contains("vec![")
            && !runtime_support_src.contains("leak_"),
        "runtime support must not keep Box/Vec/leak-based fixture helpers"
    );
}

#[test]
fn repo_tests_use_erased_static_slots_for_handles() {
    let tests_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests");
    visit_rs_files(&tests_root, &mut |path| {
        if path.file_name().and_then(|name| name.to_str()) == Some("substrate_surface.rs") {
            return;
        }
        let src = fs::read_to_string(path)
            .unwrap_or_else(|err| panic!("read {} failed: {}", path.display(), err));
        for forbidden in ["StaticSlot<Endpoint<", "StaticSlot<RouteBranch<"] {
            assert!(
                !src.contains(forbidden),
                "tests must use erased static slots for handle storage: {} contains {forbidden}",
                path.display()
            );
        }
    });
}

#[test]
fn repo_tests_do_not_depend_on_stack_tuning_helpers() {
    let route_dynamic_control_src = read_repo_test("tests/route_dynamic_control.rs");
    let lease_bundle_src = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/control/lease/bundle.rs"),
    )
    .expect("read src/control/lease/bundle.rs");
    let rust_min_stack = concat!("RUST_MIN", "_STACK");
    let hibana_test_stack = concat!("HIBANA_TEST", "_STACK");
    let stack_size_call = concat!("stack_", "size(");
    let boxed_fixture_helper = "Box::leak(Box::new(SessionKit::new(";

    for forbidden in [rust_min_stack, hibana_test_stack] {
        assert!(
            !route_dynamic_control_src.contains(forbidden),
            "route_dynamic_control must not depend on stack-tuning residue: {forbidden}"
        );
    }
    assert!(
        !route_dynamic_control_src.contains(stack_size_call),
        "route_dynamic_control must run on the default test thread stack"
    );
    for deleted in [
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/support/large_stack_sync.rs"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/support/large_stack_async.rs"),
    ] {
        assert!(
            !deleted.exists(),
            "large-stack support module must be deleted once stack tuning is eradicated: {}",
            deleted.display()
        );
    }

    for forbidden in [stack_size_call] {
        assert!(
            !lease_bundle_src.contains(forbidden),
            "lease::bundle tests must not reintroduce stack-backed large fixture residue: {forbidden}"
        );
    }
    assert!(
        !route_dynamic_control_src.contains(boxed_fixture_helper),
        "route_dynamic_control must not hide SessionKit fixture ownership behind Box::leak(SessionKit::new(...))"
    );
}

#[test]
fn repo_boundary_gates_track_current_mgmt_update_owners() {
    let boundary_gate = repo_boundary_gate_rs();
    let mgmt_gate = repo_mgmt_boundary_gate_rs();
    let plane_gate = repo_plane_boundary_gate_rs();
    let lowering_gate = repo_lowering_hygiene_gate_rs();
    let hygiene_gate = repo_surface_hygiene_gate_rs();

    assert!(
        boundary_gate.contains("check_mgmt_boundary.sh")
            && boundary_gate.contains("check_plane_boundaries.sh")
            && boundary_gate.contains("check_resolver_context_surface.sh")
            && boundary_gate.contains("check_lowering_hygiene.sh")
            && boundary_gate.contains("check_surface_hygiene.sh"),
        "hibana boundary gate must aggregate the canonical local boundary owners"
    );
    assert!(
        mgmt_gate.contains("pub mod session\\\\b")
            && mgmt_gate.contains("enter_controller|enter_cluster|enter_stream_controller|enter_stream_cluster|drive_controller|drive_cluster|drive_stream_cluster|drive_stream_controller")
            && mgmt_gate.contains("manager mutators must not be public"),
        "mgmt boundary gate must forbid the deleted session/helper surface and public manager mutators"
    );
    assert!(
        mgmt_gate.contains("src/runtime"),
        "mgmt boundary gate must stay semantic to the runtime mgmt subtree instead of a single file path"
    );
    assert!(
        plane_gate.contains("check_absent")
            && plane_gate.contains("fn apply_seed")
            && plane_gate.contains("drive_mgmt\\\\(")
            && plane_gate.contains("mgmt_managers")
            && plane_gate.contains("load_begin|load_chunk")
            && plane_gate.contains("load_commit_with|schedule_activate_with|on_decision_boundary_for_slot_with|revert_with")
            && plane_gate.contains("enter_controller|enter_cluster|enter_stream_controller|enter_stream_cluster|drive_controller|drive_cluster|drive_stream_cluster|drive_stream_controller"),
        "plane boundary gate must forbid the deleted mgmt runtime, cluster-hook, and slot-bundle owners"
    );
    assert!(
        !plane_gate.contains("boundary gate stale owner path:"),
        "plane boundary gate must stay semantic instead of depending on hard-coded owner-file existence"
    );
    assert!(
        lowering_gate.contains("interpret_eff_list\\\\(")
            && lowering_gate.contains("\\\\.policies\\\\(")
            && lowering_gate.contains("pub[[:space:]]+use[[:space:]].*EffList")
            && lowering_gate.contains("PhaseCursor::from_machine")
            && lowering_gate.contains("CompiledProgram::compile\\\\(")
            && lowering_gate.contains("CompiledRole::compile\\\\(")
            && lowering_gate.contains("controller_arm_wire_label")
            && lowering_gate.contains("src/endpoint/kernel")
            && lowering_gate.contains("legacy endpoint/cursor.rs owner still present")
            && lowering_gate.contains("macro_rules!"),
        "lowering hygiene gate must guard lowering shims, direct compiled-owner escape hatches, public EffList leakage, stale phase-cursor owners, deprecated endpoint semantic helpers, and new macro_rules owners"
    );
    assert!(
        hygiene_gate.contains("pure synonym type alias"),
        "surface hygiene gate must reject pure synonym type aliases in production source"
    );
    assert!(
        hygiene_gate.contains("role synonym alias shim"),
        "surface hygiene gate must reject production role-synonym aliases"
    );
    assert!(
        hygiene_gate.contains("legacy inferred binary builder vocabulary"),
        "surface hygiene gate must reject stale inferred-builder owner vocabulary"
    );
    assert!(
        hygiene_gate.contains("doc-hidden escape hatch"),
        "surface hygiene gate must reject doc-hidden escape hatches in production source"
    );
    assert!(
        hygiene_gate.contains("source import alias shim"),
        "surface hygiene gate must reject source-level import alias shims in production source"
    );
    assert!(
        hygiene_gate.contains("use[[:space:]][^;]*\\\\bas[[:space:]]+"),
        "surface hygiene gate must reject multiline source import aliases instead of single-line shims only"
    );
    assert!(
        hygiene_gate.contains("underscore inferred cast shim"),
        "surface hygiene gate must reject inferred `as _` casts in production source"
    );
    assert!(
        hygiene_gate.contains("self-shadowing associated const shim"),
        "surface hygiene gate must reject self-shadowing associated const shims in source and fixtures"
    );
    assert!(
        hygiene_gate.contains("route semantic self-shadowing associated type shim"),
        "surface hygiene gate must reject self-shadowing associated type shims in route semantic owners"
    );
    assert!(
        hygiene_gate.contains("stale allow shim"),
        "surface hygiene gate must reject stale allow shims in source and fixtures"
    );
    assert!(
        hygiene_gate.contains("underscore pointer cast shim"),
        "surface hygiene gate must reject inferred pointer casts in production source"
    );
    assert!(
        hygiene_gate.contains("cfg-gated no-op seam"),
        "surface hygiene gate must reject cfg-gated no-op escape seams in production source"
    );
    assert!(
        hygiene_gate.contains("transport trait fallback default shim"),
        "surface hygiene gate must reject fallback default bodies in the transport trait"
    );
    assert!(
        hygiene_gate.contains("transport metrics trait fallback default shim"),
        "surface hygiene gate must reject fallback default bodies in the transport metrics trait"
    );
    assert!(
        hygiene_gate.contains("policy signals provider fallback default shim"),
        "surface hygiene gate must reject fallback default bodies in the policy signals provider trait"
    );
    assert!(
        hygiene_gate.contains("core trait fallback default shim"),
        "surface hygiene gate must reject fallback default helper bodies in core traits"
    );
    assert!(
        hygiene_gate.contains("lease facet fallback default shim"),
        "surface hygiene gate must reject fallback default bodies in LeaseFacet"
    );
    assert!(
        !hygiene_gate.contains("lease spec facet fallback default shim"),
        "surface hygiene gate must not keep stale checks for the removed LeaseSpecFacetNeeds shim"
    );
    assert!(
        hygiene_gate.contains("resource kind fallback default shim"),
        "surface hygiene gate must reject fallback default bodies in ResourceKind"
    );
    assert!(
        hygiene_gate.contains("binding slot fallback default shim"),
        "surface hygiene gate must reject fallback default bodies in BindingSlot"
    );
    assert!(
        hygiene_gate.contains("wire encode trait fallback default shim"),
        "surface hygiene gate must reject fallback default bodies in the wire encode trait"
    );
    assert!(
        hygiene_gate.contains("transport splice fallback seam"),
        "surface hygiene gate must reject dead splice fallback seams in the transport trait"
    );
    assert!(
        hygiene_gate.contains("rendezvous stack temporary shim"),
        "surface hygiene gate must reject large rendezvous stack temporaries in cluster registration"
    );
    assert!(
        hygiene_gate.contains("stack-backed tap storage shim"),
        "surface hygiene gate must reject stack-backed tap storage helpers"
    );
    assert!(
        hygiene_gate.contains("public test-utils feature shim"),
        "surface hygiene gate must reject a public test-utils Cargo feature shim"
    );
    assert!(
        hygiene_gate.contains("project turbofish shim"),
        "surface hygiene gate must reject concrete project::<...> turbofish shims across docs/tests/source"
    );
    assert!(
        hygiene_gate.contains("app constructor turbofish shim"),
        "surface hygiene gate must reject g::route::<...> and g::par::<...> app-constructor shims across docs/tests/source"
    );
    assert!(
        hygiene_gate.contains("README pure role/message alias"),
        "surface hygiene gate must reject pure role/message aliases in README examples"
    );
    assert!(
        hygiene_gate.contains("README step/projection alias"),
        "surface hygiene gate must reject step/projection owner shorthands in README examples"
    );
    assert!(
        hygiene_gate.contains("example owner-hiding type alias"),
        "surface hygiene gate must reject owner-hiding type aliases in examples"
    );
    assert!(
        hygiene_gate.contains("example import alias shim"),
        "surface hygiene gate must reject import aliases in examples"
    );
    assert!(
        hygiene_gate.contains("example underscore cast shim"),
        "surface hygiene gate must reject underscore/pointer cast shims in examples"
    );
    assert!(
        hygiene_gate.contains("example escape hatch residue"),
        "surface hygiene gate must reject doc-hidden/dead-code/fallback residue in examples"
    );
    assert!(
        hygiene_gate.contains("internal source-test owner-hiding type alias"),
        "surface hygiene gate must reject owner-hiding type aliases inside internal source test modules"
    );
    assert!(
        hygiene_gate.contains("test fixture pure synonym type alias"),
        "surface hygiene gate must reject pure synonym type aliases in test fixtures"
    );
    assert!(
        hygiene_gate.contains("test fixture import alias shim"),
        "surface hygiene gate must reject import aliases in test fixtures"
    );
    assert!(
        hygiene_gate.contains("test fixture pure role alias"),
        "surface hygiene gate must reject pure role aliases in test fixtures"
    );
    assert!(
        hygiene_gate.contains("test fixture pure message alias"),
        "surface hygiene gate must reject pure message aliases in test fixtures"
    );
    assert!(
        hygiene_gate.contains("test fixture pure program type alias"),
        "surface hygiene gate must reject pure Program type aliases in test fixtures"
    );
    assert!(
        hygiene_gate.contains("test fixture pure role-program alias"),
        "surface hygiene gate must reject pure RoleProgram aliases in test fixtures"
    );
    assert!(
        hygiene_gate.contains("test fixture pure endpoint alias"),
        "surface hygiene gate must reject pure Endpoint aliases in test fixtures"
    );
    assert!(
        hygiene_gate.contains("test fixture pure cluster alias"),
        "surface hygiene gate must reject pure SessionKit aliases in test fixtures"
    );
    assert!(
        hygiene_gate.contains("test fixture project-role output alias"),
        "surface hygiene gate must reject ProjectRole output aliases in test fixtures"
    );
    assert!(
        hygiene_gate.contains("loop-lane-share step/composition alias shim"),
        "surface hygiene gate must reject step/composition aliases in loop_lane_share fixture"
    );
    assert!(
        hygiene_gate.contains("offer-decode-binding step/composition alias shim"),
        "surface hygiene gate must reject step/composition aliases in offer_decode_binding_regression fixture"
    );
    assert!(
        hygiene_gate.contains("nested-loop-route step/composition alias shim"),
        "surface hygiene gate must reject step/composition aliases in nested_loop_route fixture"
    );
    assert!(
        hygiene_gate.contains("nested-route-runtime step/composition alias shim"),
        "surface hygiene gate must reject step/composition aliases in nested_route_runtime fixture"
    );
    assert!(
        hygiene_gate.contains("route-dynamic-control step/composition alias shim"),
        "surface hygiene gate must reject step/composition aliases in route_dynamic_control fixture"
    );
    assert!(
        hygiene_gate.contains("route-with-internal-loops step/composition alias shim"),
        "surface hygiene gate must reject step/composition aliases in route_with_internal_loops fixture"
    );
    assert!(
        hygiene_gate.contains("cancel-rollback step/composition alias shim"),
        "surface hygiene gate must reject step/composition aliases in cancel_rollback fixture"
    );
    assert!(
        hygiene_gate.contains("ui route-policy-mismatch alias shim"),
        "surface hygiene gate must reject alias shims in g-route-policy-mismatch UI fixture"
    );
    assert!(
        hygiene_gate.contains("ui route-unprojectable alias shim"),
        "surface hygiene gate must reject alias shims in g-route-unprojectable UI fixture"
    );
    assert!(
        hygiene_gate.contains("manual route-control resource boilerplate"),
        "surface hygiene gate must reject test-local manual route-control ResourceKind boilerplate"
    );
    assert!(
        hygiene_gate.contains("test fixture pure program alias"),
        "surface hygiene gate must reject pure Program aliases in test fixtures"
    );
    assert!(
        hygiene_gate.contains("observe timestamp-checker transmute shim"),
        "surface hygiene gate must reject function-pointer transmute shims in observe core"
    );
    assert!(
        hygiene_gate.contains("underscore source escape hatch"),
        "surface hygiene gate must reject underscore-prefixed source escape hatches"
    );
    assert!(
        hygiene_gate.contains("offer-kernel rescue shim"),
        "surface hygiene gate must reject stale rescue helpers in the offer kernel"
    );
    assert!(
        hygiene_gate.contains("offer kernel stage order regression"),
        "surface hygiene gate must fail closed when offer() stops delegating through select_scope -> resolve_token -> materialize_branch"
    );
    assert!(
        hygiene_gate.contains("select_scope stage consuming/authority regression"),
        "surface hygiene gate must reject select_scope regressions that start polling or choosing arms"
    );
    assert!(
        hygiene_gate.contains("resolve_token stage materialization regression"),
        "surface hygiene gate must reject resolve_token regressions that materialize branches"
    );
    assert!(
        hygiene_gate.contains("materialize_branch stage authority regression"),
        "surface hygiene gate must reject materialize_branch regressions that perform authority selection"
    );
}

#[test]
fn runtime_mgmt_deleted_helper_family_stays_absent() {
    let runtime_mgmt_src = runtime_mgmt_rs();
    let runtime_mgmt_request_reply_src = runtime_mgmt_request_reply_rs();
    let runtime_mgmt_observe_stream_src = runtime_mgmt_observe_stream_rs();
    let runtime_mgmt_test_support_src = runtime_mgmt_test_support_rs();
    let cluster_core_src = cluster_core_rs();

    for forbidden in [
        "fn apply_seed",
        "MgmtCluster",
        "LoadMode",
        "RequestAction",
        "MgmtAutomaton",
        ".drive_mgmt(",
    ] {
        assert!(
            !runtime_mgmt_src.contains(forbidden),
            "runtime::mgmt root must not keep deleted mgmt runtime owners: {forbidden}"
        );
    }
    for forbidden in [
        "fn enter_controller",
        "fn enter_cluster",
        "fn enter_stream_controller",
        "fn enter_stream_cluster",
        "fn drive_controller",
        "fn drive_cluster",
        "fn drive_stream_cluster",
        "fn drive_stream_controller",
        "fn drive_load_branch",
        "STREAM_CONTROLLER_PROGRAM",
        "STREAM_CLUSTER_PROGRAM",
    ] {
        assert!(
            !runtime_mgmt_request_reply_src.contains(forbidden)
                && !runtime_mgmt_observe_stream_src.contains(forbidden)
                && !runtime_mgmt_test_support_src.contains(forbidden),
            "runtime::mgmt lower layers must not regrow deleted helper or mutator owners: {forbidden}"
        );
    }
    for forbidden in ["manager.load_begin(", "manager.load_chunk("] {
        assert!(
            !runtime_mgmt_request_reply_src.contains(forbidden)
                && !runtime_mgmt_observe_stream_src.contains(forbidden),
            "runtime::mgmt production owners must not regrow deleted load mutator owners: {forbidden}"
        );
    }
    for forbidden in ["mgmt_managers", "drive_mgmt(", "on_decision_boundary("] {
        assert!(
            !cluster_core_src.contains(forbidden),
            "cluster core must not keep deleted mgmt execution hooks: {forbidden}"
        );
    }
}

#[test]
fn rendezvous_slot_bundle_wrappers_stay_deleted() {
    let rendezvous_core_src = rendezvous_core_rs();
    for forbidden in [
        "fn load_commit_with",
        "fn schedule_activate_with",
        "fn on_decision_boundary_for_slot_with",
        "fn revert_with",
        "manager.load_commit(",
        "manager.schedule_activate(",
        "manager.on_decision_boundary(",
        "manager.revert(",
    ] {
        assert!(
            !rendezvous_core_src.contains(forbidden),
            "rendezvous slot bundle must not regrow deleted mgmt wrapper owners: {forbidden}"
        );
    }
}

#[test]
fn hibana_core_and_surface_hygiene_gate_stay_protocol_neutral() {
    let gate = repo_surface_hygiene_gate_rs();

    assert!(
        gate.contains("(?i)\\\\b(quic|h3|hq|qpack|alpn)\\\\b|http/3"),
        "surface hygiene gate must reject protocol-specific vocabulary in hibana/src"
    );

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    visit_rs_files(&root, &mut |path| {
        let src = fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("read {} failed: {}", path.display(), err));
        let lower = src.to_ascii_lowercase();

        assert!(
            !lower.contains("http/3"),
            "hibana core must stay protocol-neutral; found HTTP/3 token in {}",
            path.display()
        );

        for token in lower.split(|ch: char| !ch.is_ascii_alphanumeric()) {
            if matches!(token, "quic" | "h3" | "hq" | "qpack" | "alpn") {
                panic!(
                    "hibana core must stay protocol-neutral; found `{}` in {}",
                    token,
                    path.display()
                );
            }
        }
    });
}

#[test]
fn canonical_std_validation_tests_are_not_hidden_behind_test_utils() {
    for path in [
        "tests/mgmt_epf_integration.rs",
        "tests/mgmt_graph.rs",
        "tests/lease_observe.rs",
        "tests/policy_normalise.rs",
        "tests/policy_replay.rs",
        "tests/route_dynamic_control.rs",
    ] {
        let src = read_repo_test(path);
        assert!(
            src.starts_with("#![cfg(feature = \"std\")]"),
            "canonical std validation test must run under --features std: {path}"
        );
        assert!(
            !src.contains("feature = \"test-utils\""),
            "canonical std validation test must not be hidden behind test-utils: {path}"
        );
    }
}

#[test]
fn substrate_policy_root_stays_minimal() {
    let substrate_rs = substrate_rs();
    let runtime_rs = runtime_rs();

    assert!(
        substrate_rs.starts_with("//! Protocol-neutral substrate surface for protocol implementors.\n\npub use crate::control::cluster::error::{AttachError, CpError};"),
        "substrate root must expose AttachError and CpError from the cluster error owner at the substrate root"
    );
    assert!(
        !substrate_rs.contains("transmute(resolver)"),
        "substrate::SessionKit::set_resolver must not use function-pointer transmute"
    );
    assert!(
        !substrate_rs.contains("DynamicResolverFn"),
        "substrate::policy root must not re-export DynamicResolverFn"
    );
    assert!(
        !substrate_rs
            .contains("pub mod policy {\n    pub use crate::control::cluster::error::CpError;"),
        "substrate::policy root must not re-export CpError"
    );
    assert!(
        !substrate_rs.contains("pub mod internal {"),
        "substrate::policy::internal must be deleted from the public substrate surface"
    );
    assert!(
        !substrate_rs.contains("pub use crate::control::cluster::DynamicResolverFn"),
        "DynamicResolverFn must not be re-exported through substrate.rs"
    );
    assert!(
        !substrate_rs.contains("pub use crate::transport::context::core::*;"),
        "substrate::policy::core must not hide context keys behind wildcard re-exports"
    );
    for forbidden in [
        "crate::control::cap::mint::EpochTbl",
        "crate::control::cap::mint::MintConfig",
        "crate::control::cap::mint::MintConfigMarker",
        "crate::control::cluster::error::AttachError",
        "crate::control::cluster::core::ResolverContext",
        "crate::control::cluster::core::DynamicResolution",
        "crate::control::cluster::core::ResolverError",
        "crate::runtime::config::Config<'cfg, U, C>",
    ] {
        assert!(
            !substrate_rs.contains(forbidden),
            "substrate public signatures must not leak lower-layer owner paths: {forbidden}"
        );
    }
    assert!(
        !runtime_rs.contains("pub type SessionKit<'cfg, T, U, C, const MAX_RV: usize> ="),
        "runtime.rs must not hide the kernel cluster behind a type alias"
    );
    assert!(
        !runtime_rs.contains("pub use crate::control::cluster::core::AttachError;"),
        "runtime.rs must not pretend to own AttachError"
    );
    assert!(
        substrate_rs.contains("pub use crate::control::cluster::error::{AttachError, CpError};"),
        "substrate surface must re-export AttachError from the cluster error owner"
    );
    assert!(
        !substrate_rs.contains("pub use crate::global::const_dsl::DynamicMeta;"),
        "DynamicMeta must not remain in the substrate surface"
    );
    assert!(
        !substrate_rs.contains("with_control_plan"),
        "with_control_plan must be deleted from the substrate surface"
    );
    for forbidden in ["pub mod lease {", "pub mod txn {", "pub mod cluster {"] {
        assert!(
            !substrate_rs.contains(forbidden),
            "legacy substrate helper surface must be deleted: {forbidden}"
        );
    }
}

#[test]
fn endpoint_and_route_branch_handles_stay_small() {
    let endpoint_size = size_of::<Endpoint<'static, 0, StaticTestKit, MintConfig>>();
    let branch_size = size_of::<RouteBranch<'static, 'static, 0, StaticTestKit, MintConfig>>();
    let word = size_of::<usize>();

    assert!(
        endpoint_size <= 8 * word,
        "Endpoint handle must stay choreography-independent and bounded: size={endpoint_size} bytes"
    );
    assert!(
        branch_size <= 8 * word,
        "RouteBranch handle must stay choreography-independent and bounded: size={branch_size} bytes"
    );
}

#[test]
fn substrate_cap_root_stays_minimal() {
    let substrate_rs = substrate_rs();
    let substrate_ws = compact_ws(&substrate_rs);
    let allowlist = substrate_public_api_allowlist();
    let cap_block_start = allowlist
        .find("pub mod cap {")
        .expect("substrate public API allowlist must keep the cap bucket");
    let wire_block_start = allowlist
        .find("pub mod wire {")
        .expect("substrate public API allowlist must keep the wire bucket");
    let cap_block = &allowlist[cap_block_start..wire_block_start];

    assert!(
        substrate_ws.contains(
            "pub use crate::control::cap::mint::{ CapShot, ControlResourceKind, GenericCapToken, ResourceKind, };"
        ),
        "substrate::cap root must only expose the minimal capability surface"
    );
    assert!(
        cap_block.contains("pub use {One, Many};"),
        "substrate::cap must keep the canonical One/Many owner inside the cap bucket"
    );
    assert!(
        !allowlist[..cap_block_start].contains("pub use {One, Many};")
            && !allowlist[wire_block_start..].contains("pub use {One, Many};"),
        "substrate public API must not regrow a competing root-level One/Many alias"
    );
    assert!(
        !substrate_rs.contains("    pub use crate::control::cap::resource_kinds;"),
        "substrate::cap root must not keep the standard resource-kind catalogue at the root surface"
    );
    assert!(
        !substrate_rs.contains("pub use crate::control::cap::{\n        CAP_FIXED_HEADER_LEN"),
        "substrate::cap root must not re-export mint/epoch/detail constants"
    );
    assert!(
        substrate_rs.contains("    pub mod advanced {"),
        "substrate::cap advanced bucket must exist for lower-level mint/epoch/detail helpers"
    );
    for forbidden in [
        "pub use crate::control::cap::payload::*;",
        "pub use crate::control::cap::typed_tokens::*;",
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
            !substrate_rs.contains(forbidden),
            "substrate::cap::advanced must use explicit curated exports: {forbidden}"
        );
    }
    for required in [
        "AllowsCanonical,",
        "pub use crate::control::cap::resource_kinds::{",
        "LoopContinueKind,",
        "RouteDecisionKind,",
        "LoadBeginKind,",
        "LoopDecisionHandle,",
    ] {
        assert!(
            substrate_rs.contains(required),
            "substrate::cap::advanced must own the explicit standard control-kind catalogue: {required}"
        );
    }
}

#[test]
fn substrate_runtime_root_stays_minimal() {
    let substrate_rs = substrate_rs();

    assert!(
        substrate_rs.contains(
            "pub use crate::runtime::config::{Clock, Config, CounterClock};\n    pub use crate::runtime::consts::{DefaultLabelUniverse, LabelUniverse};"
        ),
        "substrate::runtime root must only expose clock/config + label-universe types"
    );
    assert!(
        !substrate_rs
            .contains("pub mod runtime {\n    pub use crate::control::cluster::error::CpError;"),
        "substrate::runtime root must not re-export CpError"
    );
    assert!(
        !substrate_rs.contains("pub mod consts {")
            && !substrate_rs.contains("pub use crate::runtime::consts::*;"),
        "substrate::runtime must not grow a public consts bucket"
    );
}

#[test]
fn substrate_mgmt_and_binding_roots_stay_minimal() {
    let substrate_rs = substrate_rs();
    let runtime_mgmt_rs = runtime_mgmt_rs();
    let runtime_mgmt_payload_rs = runtime_mgmt_payload_rs();
    let runtime_mgmt_request_reply_rs = runtime_mgmt_request_reply_rs();
    let runtime_mgmt_observe_stream_rs = runtime_mgmt_observe_stream_rs();
    let runtime_mgmt_observe_stream_ws = compact_ws(&runtime_mgmt_observe_stream_rs);
    let runtime_mgmt_test_support_rs = runtime_mgmt_test_support_rs();
    let binding_body = {
        let start = substrate_rs
            .find("pub mod binding {")
            .expect("substrate::binding block must exist");
        let rest = &substrate_rs[start..];
        let end = rest
            .find("pub mod policy {")
            .expect("substrate::policy block must follow binding");
        &rest[..end]
    };
    let mgmt_head = {
        let start = substrate_rs
            .find("pub mod mgmt {")
            .expect("substrate::mgmt block must exist");
        let rest = &substrate_rs[start..];
        let end = rest
            .find("pub mod binding {")
            .expect("substrate::mgmt block must end before substrate::binding");
        &rest[..end]
    };

    assert!(
        !substrate_rs.contains("pub mod session {"),
        "substrate::mgmt must not keep the deleted session helper module"
    );
    assert!(
        !substrate_rs.contains("crate::runtime::mgmt::session::"),
        "substrate::mgmt must not route through a stale runtime::mgmt::session owner path"
    );
    assert!(
        !substrate_rs.contains("crate::endpoint::cursor::CursorEndpoint<")
            && !substrate_rs.contains("crate::endpoint::kernel::CursorEndpoint<"),
        "substrate::mgmt must not leak CursorEndpoint in public signatures"
    );
    assert!(
        !substrate_rs.contains("pub struct CodeSessionRequest<'a> {"),
        "substrate::mgmt must not regrow a local request wrapper owner"
    );
    assert!(
        !substrate_rs
            .contains("pub fn run_code_session<'cfg, 'request, T, U, C, const MAX_RV: usize>("),
        "substrate::mgmt must not regrow local code-session sugar"
    );
    assert!(
        !runtime_mgmt_rs.contains("pub mod session;")
            && !runtime_mgmt_rs.contains("session::management_compiled_programs()"),
        "internal runtime management kernel must not keep the stale session module owner"
    );
    assert!(
        runtime_mgmt_rs.contains("mod payload;")
            && runtime_mgmt_rs.contains("mod request_reply;")
            && runtime_mgmt_rs.contains("mod observe_stream;")
            && runtime_mgmt_rs.contains("#[cfg(test)]\nmod test_support;"),
        "runtime mgmt root must wire payload/request-reply/observe-stream/test-support owners explicitly"
    );
    assert!(
        runtime_mgmt_rs.contains("PROGRAM as REQUEST_REPLY_PREFIX")
            && runtime_mgmt_rs.contains("PROGRAM as OBSERVE_STREAM_PREFIX"),
        "runtime mgmt must expose the canonical public prefix owners"
    );
    assert!(
        !runtime_mgmt_rs
            .contains("pub(crate) fn enter_controller<'cfg, T, U, C, B, const MAX_RV: usize>(")
            && !runtime_mgmt_rs
                .contains("pub(crate) fn enter_cluster<'cfg, T, U, C, B, const MAX_RV: usize>(")
            && !runtime_mgmt_rs.contains(
                "pub(crate) fn enter_stream_controller<'cfg, T, U, C, B, const MAX_RV: usize>("
            )
            && !runtime_mgmt_rs.contains(
                "pub(crate) fn enter_stream_cluster<'cfg, T, U, C, B, const MAX_RV: usize>("
            )
            && !runtime_mgmt_rs.contains("pub(crate) async fn drive_controller<")
            && !runtime_mgmt_rs.contains("pub(crate) async fn drive_cluster<")
            && !runtime_mgmt_rs.contains("pub(crate) async fn drive_stream_cluster<")
            && !runtime_mgmt_rs.contains("pub(crate) async fn drive_stream_controller<"),
        "runtime mgmt root must stay on payload/prefix ownership instead of regrowing helper wrappers"
    );
    let runtime_mgmt_payload_ws = compact_ws(&runtime_mgmt_payload_rs);
    assert!(
        runtime_mgmt_payload_ws
            .contains("pub struct LoadRequest<'a> { pub slot: crate::substrate::policy::epf::Slot, pub code: &'a [u8], pub fuel_max: u16, pub mem_len: u16, }")
            && runtime_mgmt_payload_ws.contains(
                "pub struct SlotRequest { pub slot: crate::substrate::policy::epf::Slot, }"
            )
            && runtime_mgmt_payload_ws.contains(
                "pub struct LoadBegin { pub slot: crate::substrate::policy::epf::Slot, pub code_len: u32, pub fuel_max: u16, pub mem_len: u16, pub hash: u32, }"
            ),
        "runtime mgmt payload owner must spell the canonical public Slot owner directly"
    );
    assert!(
        !runtime_mgmt_rs.contains("pub struct LoadRequest<'a> {")
            && !runtime_mgmt_rs.contains("pub struct TapBatch {"),
        "runtime mgmt root must stay a facade instead of keeping payload/batching bodies"
    );
    assert!(
        !substrate_rs.contains("crate::runtime::mgmt::enter_controller(")
            && !substrate_rs.contains("crate::runtime::mgmt::enter_cluster(")
            && !substrate_rs.contains("crate::runtime::mgmt::enter_stream_controller(")
            && !substrate_rs.contains("crate::runtime::mgmt::enter_stream_cluster(")
            && !substrate_rs.contains("crate::runtime::mgmt::drive_cluster(")
            && !substrate_rs.contains("crate::runtime::mgmt::drive_stream_cluster(")
            && !substrate_rs.contains("crate::runtime::mgmt::drive_stream_controller("),
        "substrate::mgmt must not route public surface through deleted management helper wrappers"
    );
    assert!(
        !substrate_rs.contains("        pub fn set_resolver<'cfg, T, U, C, const MAX_RV: usize>("),
        "substrate::mgmt must not keep a second public resolver registration entry"
    );
    assert!(
        !substrate_rs.contains("crate::runtime::mgmt::set_resolver("),
        "substrate::mgmt must not route through a stale runtime resolver wrapper"
    );
    assert!(
        !runtime_mgmt_rs
            .contains("pub(crate) fn set_resolver<'cfg, T, U, C, const MAX_RV: usize>(")
            && !runtime_mgmt_request_reply_rs
                .contains("pub(crate) fn set_resolver<'cfg, T, U, C, const MAX_RV: usize>("),
        "runtime management owners must not keep the deleted duplicate resolver wrapper"
    );
    for required in [
        "pub use crate::runtime::mgmt::{",
        "ROLE_CLUSTER,",
        "ROLE_CONTROLLER,",
        "LoadRequest,",
        "Request,",
        "SlotRequest,",
        "pub mod request_reply {",
        "pub mod observe_stream {",
        "pub use crate::runtime::mgmt::RequestReplyPrefixSteps as PrefixSteps;",
        "pub use crate::runtime::mgmt::ObserveStreamPrefixSteps as PrefixSteps;",
        "pub const PREFIX: crate::g::Program<PrefixSteps> =",
    ] {
        assert!(
            substrate_rs.contains(required),
            "substrate::mgmt must expose the canonical prefix surface: {required}"
        );
    }
    for forbidden in [
        "pub fn enter_controller<'cfg, T, U, C, B, const MAX_RV: usize>(",
        "pub fn enter_cluster<'cfg, T, U, C, B, const MAX_RV: usize>(",
        "pub fn enter_stream_controller<'cfg, T, U, C, B, const MAX_RV: usize>(",
        "pub fn enter_stream_cluster<'cfg, T, U, C, B, const MAX_RV: usize>(",
        "pub async fn drive_cluster<'lease, 'cfg, T, U, C, Mint, B, const MAX_RV: usize>(",
        "pub async fn drive_stream_cluster<'lease, T, U, C, Mint, F, B, const MAX_RV: usize>(",
        "pub async fn drive_stream_controller<'lease, T, U, C, Mint, F, B, const MAX_RV: usize>(",
        "impl<'request> Request<'request> {",
        "pub async fn drive_controller<'lease, T, U, C, Mint, B, const MAX_RV: usize>(",
    ] {
        assert!(
            !substrate_rs.contains(forbidden),
            "substrate::mgmt must not keep the deleted management helper family: {forbidden}"
        );
    }
    assert!(substrate_rs.contains("pub mod tap {"));
    assert!(
        substrate_rs.contains("pub use crate::observe::core::TapEvent;")
            && !substrate_rs.contains("TapBatch"),
        "substrate::mgmt::tap must stay on TapEvent only"
    );
    for forbidden in ["            pub mod events {", "            pub mod ids {"] {
        assert!(
            !substrate_rs.contains(forbidden),
            "substrate::mgmt::tap must not expose lower-layer observe helper buckets: {forbidden}"
        );
    }
    for forbidden in [
        "AssociationSnapshot",
        "FenceCounters",
        "PolicyEvent,",
        "PolicyEventKind,",
        "TAP_BATCH_MAX_EVENTS",
        "TapRing,",
        "emit,",
        "for_each_since,",
        "head,",
        "install_ring,",
        "push,",
        "RawEvent",
        "uninstall_ring,",
    ] {
        assert!(
            !substrate_rs.contains(forbidden),
            "substrate::mgmt::tap must not leak extra observe helpers: {forbidden}"
        );
    }
    assert!(!substrate_rs.contains("pub mod observe {"));
    assert!(
        !substrate_rs
            .contains("pub fn run_code_session<'cfg, 'request, T, U, C, const MAX_RV: usize>("),
        "substrate::mgmt must not keep a local wrapper entry for management code sessions"
    );
    assert!(
        substrate_rs.contains("LoadRequest,")
            && substrate_rs.contains("Request,")
            && substrate_rs.contains("SlotRequest,"),
        "substrate::mgmt must re-export the canonical request sum type and payload owners"
    );
    for forbidden in [
        "Result<super::Reply, super::MgmtError>",
        "subscribe: super::SubscribeReq,",
        "F: FnMut(tap::TapEvent) -> bool,",
    ] {
        assert!(
            !substrate_rs.contains(forbidden),
            "substrate::mgmt must use canonical substrate owner paths in public contracts: {forbidden}"
        );
    }
    assert!(
        !substrate_rs.contains("Command, LOAD_CHUNK_MAX, LoadBegin, LoadChunk, MgmtError"),
        "substrate::mgmt root must not expose load-size/facet/stat helpers"
    );
    assert!(
        !substrate_rs.contains("pub use crate::runtime::mgmt::{\n        Command,"),
        "substrate::mgmt root must not regrow the deleted Command surface"
    );
    assert!(
        !mgmt_head.contains("pub mod advanced {"),
        "substrate::mgmt must not keep a hidden advanced surface"
    );
    assert!(
        substrate_rs.contains(
            "pub use crate::binding::{\n        BindingSlot, Channel, ChannelDirection, ChannelKey, ChannelStore, IncomingClassification,\n        NoBinding, TransportOpsError,\n    };"
        ),
        "substrate::binding root must stay on the minimal channel/binding surface"
    );
    assert!(
        !substrate_rs.contains("ChannelStoreError"),
        "substrate::binding must not leak a second channel-store error surface"
    );
    assert!(
        !runtime_mgmt_request_reply_rs.contains("type CommandRouteSteps = CommandStep;")
            && !runtime_mgmt_request_reply_rs.contains("type Controller = g::Role<0>;")
            && !runtime_mgmt_request_reply_rs.contains("type Cluster = g::Role<1>;")
            && runtime_mgmt_request_reply_rs.contains("const LOOP_SEGMENT: Program<")
            && runtime_mgmt_request_reply_rs.contains("pub type ProgramSteps = SeqSteps<")
            && runtime_mgmt_request_reply_rs.contains("pub const PROGRAM: Program<ProgramSteps> ="),
        "request-reply owner must hold the canonical request/reply choreography directly"
    );
    assert!(
        !runtime_mgmt_observe_stream_rs.contains("type StreamLoopRouteSteps =")
            && !runtime_mgmt_observe_stream_rs.contains("type StreamContinueMsg =")
            && !runtime_mgmt_observe_stream_rs.contains("type StreamBreakMsg =")
            && !runtime_mgmt_observe_stream_rs.contains("type TapBatchMsg =")
            && runtime_mgmt_observe_stream_rs.contains("pub struct TapBatch {")
            && runtime_mgmt_observe_stream_rs.contains("const STREAM_LOOP_ROUTE: Program<")
            && runtime_mgmt_observe_stream_ws.contains("g::Msg<LABEL_OBSERVE_STREAM_END, ()>")
            && runtime_mgmt_observe_stream_ws.contains("g::Msg<LABEL_OBSERVE_BATCH, TapBatch>"),
        "observe-stream owner must keep batching local while preserving the canonical loop witnesses"
    );
    assert!(
        runtime_mgmt_test_support_rs
            .contains("pub(crate) fn with_management_compiled_programs_for_test")
            && runtime_mgmt_test_support_rs.contains("Manager<Cold, SLOTS>")
            && runtime_mgmt_test_support_rs.contains("PromotionGateThresholds"),
        "test-support owner must hold the staging manager and compiled-program helper"
    );
    for forbidden in [
        "BindingSlot as Binding",
        "IncomingClassification as Incoming",
        "NoBinding as NullBinding",
        "Outgoing as SendEnvelope",
    ] {
        assert!(
            !binding_body.contains(forbidden),
            "substrate::binding must not reintroduce alias-only public names: {forbidden}"
        );
    }
    assert!(
        !binding_body.contains("    pub mod advanced {"),
        "substrate::binding must not regrow a lower-layer advanced bucket"
    );
}

#[test]
fn substrate_epf_root_stays_minimal() {
    let substrate_rs = substrate_rs();
    let allowlist = substrate_public_api_allowlist();
    let epf_block_start = allowlist
        .find("pub mod epf {")
        .expect("substrate public API allowlist must keep the epf bucket");
    let cap_block_start = allowlist
        .find("pub mod cap {")
        .expect("substrate public API allowlist must keep the cap bucket");
    let epf_block = &allowlist[epf_block_start..cap_block_start];

    assert!(
        substrate_rs.contains("    pub mod epf {"),
        "substrate::policy::epf bucket must exist for protocol-facing slot/header helpers"
    );
    assert!(
        epf_block.contains("pub use Header;") && epf_block.contains("pub use Slot;"),
        "substrate::policy::epf must keep the canonical Slot/Header owner inside the epf bucket"
    );
    assert!(
        !allowlist[..epf_block_start].contains("pub use Slot;")
            && !allowlist[cap_block_start..].contains("pub use Slot;"),
        "substrate public API must not regrow a competing root-level Slot alias"
    );
    for forbidden in [
        "pub use crate::epf::Slot;",
        "pub use crate::epf::Slot as VmSlot;",
        "pub use Header as VmHeader;",
        "pub use crate::epf::Action;",
        "pub use crate::epf::PolicyMode;",
        "pub use crate::epf::TapEvent;",
        "pub use crate::epf::Trap;",
        "pub use crate::epf::VmAction;",
        "pub use crate::epf::host::{HostSlots, Machine};",
        "pub use crate::epf::ops;",
        "pub use crate::epf::run_with;",
        "pub use crate::epf::verifier::compute_hash;",
        "pub mod audit {",
        "MachineConfig<'arena>",
        "RunRequest<'a>",
    ] {
        assert!(
            !substrate_rs.contains(forbidden),
            "substrate::policy::epf must not leak helper surface: {forbidden}"
        );
    }
}
