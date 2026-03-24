mod common;
#[path = "support/runtime.rs"]
mod runtime_support;

use std::fs;
use std::path::{Path, PathBuf};

use hibana::Endpoint;
use hibana::g;
use hibana::g::advanced::steps::{ProjectRole, SendStep, SeqSteps, StepCons, StepNil};
use hibana::g::advanced::{RoleProgram, project};
use hibana::substrate::{
    SessionCluster, SessionId,
    binding::NoBinding,
    cap::advanced::{EpochTbl, MintConfig},
    policy::{DynamicResolution, PolicyId, ResolverContext, ResolverError},
    runtime::{Config, CounterClock, DefaultLabelUniverse},
};

const PROGRAM: g::Program<StepCons<SendStep<g::Role<0>, g::Role<1>, g::Msg<1, u8>, 0>, StepNil>> =
    g::send::<g::Role<0>, g::Role<1>, g::Msg<1, u8>, 0>();
static CLIENT_PROGRAM: RoleProgram<
    'static,
    0,
    <StepCons<SendStep<g::Role<0>, g::Role<1>, g::Msg<1, u8>, 0>, StepNil> as ProjectRole<
        g::Role<0>,
    >>::Output,
> = project(&PROGRAM);

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

fn runtime_mgmt_kernel_rs() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/runtime/mgmt/kernel.rs");
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

fn direct_projection_binary_check_rs() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".github/scripts/check_direct_projection_binary.sh");
    fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("read {} failed: {}", path.display(), err))
}

fn block_body<'a>(src: &'a str, anchor: &str) -> &'a str {
    let anchor_idx = src
        .find(anchor)
        .unwrap_or_else(|| panic!("missing block anchor: {anchor}"));
    let mut cursor = anchor_idx;
    let mut open_brace = None;
    for line in src[anchor_idx..].split_inclusive('\n') {
        if line.trim() == "{" {
            open_brace = Some(
                cursor
                    + line
                        .find('{')
                        .expect("function body opening brace on brace line"),
            );
            break;
        }
        cursor += line.len();
    }
    let open_brace = open_brace.expect("block opening brace");
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
    panic!("block closing brace");
}

fn count_occurrences(haystack: &str, needle: &str) -> usize {
    haystack.match_indices(needle).count()
}

fn quality_workflow_rs() -> String {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".github/workflows/quality-gates.yml");
    fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("read {} failed: {}", path.display(), err))
}

fn collect_rs_files(root: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(root)
        .unwrap_or_else(|err| panic!("read_dir {} failed: {}", root.display(), err))
    {
        let entry =
            entry.unwrap_or_else(|err| panic!("read_dir entry {} failed: {}", root.display(), err));
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, files);
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            files.push(path);
        }
    }
}

fn hibana_src_files() -> Vec<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    collect_rs_files(&root, &mut files);
    files
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

fn defer_resolver(
    _cluster: &SessionCluster<
        'static,
        common::TestTransport,
        DefaultLabelUniverse,
        CounterClock,
        1,
    >,
    _ctx: ResolverContext,
) -> Result<DynamicResolution, ResolverError> {
    Ok(DynamicResolution::Defer { retry_hint: 1 })
}

#[test]
fn hibana_core_source_stays_protocol_neutral() {
    let hygiene_gate = repo_surface_hygiene_gate_rs();
    assert!(
        hygiene_gate.contains("protocol-specific vocabulary in hibana core"),
        "surface hygiene gate must reject protocol-specific vocabulary from hibana core"
    );

    for path in hibana_src_files() {
        let body = fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("read {} failed: {}", path.display(), err));
        for forbidden in ["quic", "h3", "hq"] {
            assert!(
                !contains_ascii_word(&body, forbidden),
                "hibana core must stay protocol-neutral: {} contains `{forbidden}`",
                path.display()
            );
        }
    }
}

#[test]
fn substrate_facade_exposes_enter_and_policy_resolver_registration() {
    let transport = common::TestTransport::default();
    let cluster: &mut SessionCluster<
        'static,
        common::TestTransport,
        DefaultLabelUniverse,
        CounterClock,
        1,
    > = Box::leak(Box::new(SessionCluster::new(runtime_support::leak_clock())));
    let rv_id = cluster
        .add_rendezvous_from_config(
            Config::new(
                runtime_support::leak_tap_storage(),
                runtime_support::leak_slab(1024),
            ),
            transport,
        )
        .expect("add rendezvous");

    cluster
        .set_resolver(rv_id, &CLIENT_PROGRAM, PolicyId::new(7), defer_resolver)
        .expect("install resolver");

    let endpoint = cluster
        .enter(rv_id, SessionId::new(1), &CLIENT_PROGRAM, NoBinding)
        .expect("enter endpoint");
    let _: &Endpoint<
        '_,
        0,
        common::TestTransport,
        DefaultLabelUniverse,
        CounterClock,
        EpochTbl,
        1,
        MintConfig,
        NoBinding,
    > = &endpoint;
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
            "substrate::SessionCluster must not expose canonical token helper surface: {forbidden}"
        );
        assert!(
            !allowlist.contains(forbidden),
            "substrate public API allowlist must not keep deleted canonical token helpers: {forbidden}"
        );
    }
}

#[test]
fn substrate_facade_registers_rendezvous_before_enter() {
    let transport = common::TestTransport::default();
    let cluster: &mut SessionCluster<
        'static,
        common::TestTransport,
        DefaultLabelUniverse,
        CounterClock,
        1,
    > = Box::leak(Box::new(SessionCluster::new(runtime_support::leak_clock())));

    let rv_id = cluster
        .add_rendezvous_from_config(
            Config::new(
                runtime_support::leak_tap_storage(),
                runtime_support::leak_slab(1024),
            ),
            transport,
        )
        .expect("add rendezvous");

    assert!(
        rv_id.raw() > 0,
        "rendezvous registration must allocate a concrete id"
    );
}

#[test]
fn substrate_facade_sets_resolver_before_enter() {
    let transport = common::TestTransport::default();
    let cluster: &mut SessionCluster<
        'static,
        common::TestTransport,
        DefaultLabelUniverse,
        CounterClock,
        1,
    > = Box::leak(Box::new(SessionCluster::new(runtime_support::leak_clock())));
    let rv_id = cluster
        .add_rendezvous_from_config(
            Config::new(
                runtime_support::leak_tap_storage(),
                runtime_support::leak_slab(1024),
            ),
            transport,
        )
        .expect("add rendezvous");

    cluster
        .set_resolver(rv_id, &CLIENT_PROGRAM, PolicyId::new(7), defer_resolver)
        .expect("install resolver");
}

#[test]
fn substrate_facade_projects_before_enter() {
    let app = g::send::<g::Role<0>, g::Role<1>, g::Msg<1, u8>, 0>();
    let connection = g::advanced::compose::seq(StepNil::PROGRAM, app);
    let program: RoleProgram<
        '_,
        0,
        <SeqSteps<
            StepNil,
            StepCons<SendStep<g::Role<0>, g::Role<1>, g::Msg<1, u8>, 0>, StepNil>,
        > as ProjectRole<g::Role<0>>>::Output,
    > = project(&connection);

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
        "./.github/scripts/check_hibana_public_api.sh",
        "./.github/scripts/check_policy_surface_hygiene.sh",
        "./.github/scripts/check_boundary_contracts.sh",
        "./.github/scripts/check_direct_projection_binary.sh",
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
}

#[test]
fn substrate_facade_accepts_non_static_projected_programs() {
    let transport = common::TestTransport::default();
    let cluster: &mut SessionCluster<
        'static,
        common::TestTransport,
        DefaultLabelUniverse,
        CounterClock,
        1,
    > = Box::leak(Box::new(SessionCluster::new(runtime_support::leak_clock())));
    let rv_id = cluster
        .add_rendezvous_from_config(
            Config::new(
                runtime_support::leak_tap_storage(),
                runtime_support::leak_slab(1024),
            ),
            transport,
        )
        .expect("add rendezvous");

    let app = g::send::<g::Role<0>, g::Role<1>, g::Msg<1, u8>, 0>();
    let connection = g::advanced::compose::seq(StepNil::PROGRAM, app);
    let program = project(&connection);

    let endpoint = cluster
        .enter(rv_id, SessionId::new(2), &program, NoBinding)
        .expect("enter endpoint");
    let _: &Endpoint<
        '_,
        0,
        common::TestTransport,
        DefaultLabelUniverse,
        CounterClock,
        EpochTbl,
        1,
        MintConfig,
        NoBinding,
    > = &endpoint;
}

#[test]
fn substrate_cluster_registration_avoids_rendezvous_stack_temporary() {
    let cluster_core_src = cluster_core_rs();
    let cluster_core_ws = compact_ws(&cluster_core_src);

    assert!(
        cluster_core_ws.contains("core.locals.register_local_from_config(config, transport)"),
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
fn runtime_support_avoids_stack_backed_tap_storage() {
    let runtime_support_src = read_repo_test("tests/support/runtime.rs");

    assert!(
        runtime_support_src.contains("vec![TapEvent::default(); RING_EVENTS].into_boxed_slice()"),
        "runtime support must allocate tap storage on the heap without first materializing a large stack array"
    );
    assert!(
        !runtime_support_src.contains("Box::new([TapEvent::default(); RING_EVENTS])"),
        "runtime support must not keep stack-backed tap storage boxing"
    );
}

#[test]
fn repo_boundary_gates_track_current_mgmt_update_owners() {
    let boundary_gate = repo_boundary_gate_rs();
    let mgmt_gate = repo_mgmt_boundary_gate_rs();
    let plane_gate = repo_plane_boundary_gate_rs();
    let hygiene_gate = repo_surface_hygiene_gate_rs();

    assert!(
        boundary_gate.contains("check_mgmt_boundary.sh")
            && boundary_gate.contains("check_plane_boundaries.sh")
            && boundary_gate.contains("check_resolver_context_surface.sh")
            && boundary_gate.contains("check_surface_hygiene.sh"),
        "hibana boundary gate must aggregate the canonical local boundary owners"
    );
    assert!(
        mgmt_gate.contains("schedule_activate") && mgmt_gate.contains("on_decision_boundary"),
        "mgmt boundary gate must track the full live manager mutator surface"
    );
    assert!(
        mgmt_gate.contains("src/runtime"),
        "mgmt boundary gate must stay semantic to the runtime mgmt subtree instead of a single file path"
    );
    assert!(
        plane_gate.contains("check_required_multiline")
            && plane_gate.contains("fn apply_seed")
            && plane_gate.contains("drive_mgmt\\\\(")
            && plane_gate.contains("async fn drive_load_branch")
            && plane_gate.contains("load_begin\\\\(")
            && plane_gate.contains("load_chunk\\\\(")
            && plane_gate.contains("fn load_commit_with")
            && plane_gate.contains("fn schedule_activate_with")
            && plane_gate.contains("fn on_decision_boundary_for_slot_with")
            && plane_gate.contains("fn revert_with"),
        "plane boundary gate must anchor direct mutators to their canonical semantic owners"
    );
    assert!(
        !plane_gate.contains("boundary gate stale owner path:"),
        "plane boundary gate must stay semantic instead of depending on hard-coded owner-file existence"
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
        hygiene_gate.contains("lease spec facet fallback default shim"),
        "surface hygiene gate must reject fallback default bodies in LeaseSpecFacetNeeds"
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
        "surface hygiene gate must reject pure SessionCluster aliases in test fixtures"
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
fn runtime_mgmt_direct_mutators_stay_on_canonical_owners() {
    let runtime_mgmt_src = runtime_mgmt_rs();
    let runtime_mgmt_kernel_src = runtime_mgmt_kernel_rs();
    let tests_anchor = runtime_mgmt_src
        .find("#[cfg(test)]\nmod tests {")
        .expect("runtime mgmt tests module anchor");
    let production_runtime_mgmt = &runtime_mgmt_src[..tests_anchor];
    let apply_seed_body = compact_ws(block_body(&runtime_mgmt_src, "fn apply_seed"));
    let drive_load_branch_body = compact_ws(block_body(
        &runtime_mgmt_kernel_src,
        "async fn drive_load_branch",
    ));

    assert!(
        apply_seed_body.contains("cluster.drive_mgmt(rv_id, sid, seed)"),
        "apply_seed must stay the canonical direct drive_mgmt owner"
    );
    assert_eq!(
        count_occurrences(&runtime_mgmt_src, ".drive_mgmt("),
        1,
        "runtime mgmt must keep exactly one direct drive_mgmt call"
    );
    for forbidden in [
        "manager.load_begin(",
        "manager.load_chunk(",
        "manager.load_commit(",
        "manager.schedule_activate(",
        "manager.on_decision_boundary(",
        "manager.revert(",
    ] {
        assert!(
            !production_runtime_mgmt.contains(forbidden),
            "production runtime::mgmt must not keep direct manager mutators outside the canonical helpers: {forbidden}"
        );
    }

    assert!(
        drive_load_branch_body.contains("manager.load_begin(")
            && drive_load_branch_body.contains("manager.load_chunk("),
        "drive_load_branch must own the direct load_begin/load_chunk mutators"
    );
    assert_eq!(
        count_occurrences(&runtime_mgmt_kernel_src, "manager.load_begin("),
        1,
        "mgmt kernel must keep a single direct load_begin owner"
    );
    assert_eq!(
        count_occurrences(&runtime_mgmt_kernel_src, "manager.load_chunk("),
        2,
        "mgmt kernel must keep the canonical two load_chunk branches only"
    );
}

#[test]
fn rendezvous_slot_bundle_wrappers_keep_policy_mutators_scoped() {
    let rendezvous_core_src = rendezvous_core_rs();
    let load_commit_with_body = compact_ws(block_body(&rendezvous_core_src, "fn load_commit_with"));
    let schedule_activate_with_body = compact_ws(block_body(
        &rendezvous_core_src,
        "fn schedule_activate_with",
    ));
    let on_decision_boundary_for_slot_with_body = compact_ws(block_body(
        &rendezvous_core_src,
        "fn on_decision_boundary_for_slot_with",
    ));
    let revert_with_body = compact_ws(block_body(&rendezvous_core_src, "fn revert_with"));

    assert!(
        load_commit_with_body.contains(".load_commit(slot, self.storage_mut(slot))"),
        "load_commit_with must stay the canonical load_commit owner"
    );
    assert!(
        schedule_activate_with_body.contains("manager.schedule_activate(slot)"),
        "schedule_activate_with must stay the canonical schedule_activate owner"
    );
    assert!(
        on_decision_boundary_for_slot_with_body.contains(
            "unsafe { manager.on_decision_boundary(slot, &mut *storage_ptr, &mut *host_ptr) }"
        ),
        "on_decision_boundary_for_slot_with must stay the canonical decision-boundary owner"
    );
    assert!(
        revert_with_body
            .contains("unsafe { manager.revert(slot, &mut *storage_ptr, &mut *host_ptr) }"),
        "revert_with must stay the canonical revert owner"
    );
    assert_eq!(
        count_occurrences(&rendezvous_core_src, ".load_commit("),
        1,
        "rendezvous slot bundle must keep a single direct load_commit owner"
    );
    assert_eq!(
        count_occurrences(&rendezvous_core_src, "manager.schedule_activate("),
        1,
        "rendezvous slot bundle must keep a single direct schedule_activate owner"
    );
    assert_eq!(
        count_occurrences(&rendezvous_core_src, "manager.on_decision_boundary("),
        1,
        "rendezvous slot bundle must keep a single direct on_decision_boundary owner"
    );
    assert_eq!(
        count_occurrences(&rendezvous_core_src, "manager.revert("),
        1,
        "rendezvous slot bundle must keep a single direct revert owner"
    );
}

#[test]
fn hibana_core_and_surface_hygiene_gate_stay_protocol_neutral() {
    let gate = repo_surface_hygiene_gate_rs();

    assert!(
        gate.contains("(?i)\\\\b(quic|h3|hq|qpack|alpn)\\\\b|http/3"),
        "surface hygiene gate must reject protocol-specific vocabulary in hibana/src"
    );

    for path in hibana_src_files() {
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
    }
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
        "substrate::SessionCluster::set_resolver must not use function-pointer transmute"
    );
    assert!(
        !substrate_rs.contains("DynamicResolution, DynamicResolverFn, PolicyId, ResolverContext"),
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
        "crate::control::cluster::core::PolicyId",
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
        !runtime_rs.contains("pub type SessionCluster<'cfg, T, U, C, const MAX_RV: usize> ="),
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
    let runtime_mgmt_kernel_rs = runtime_mgmt_kernel_rs();
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
            .find("    pub mod session {")
            .expect("substrate::mgmt session block must exist");
        &rest[..end]
    };

    assert!(
        !substrate_rs.contains("pub use crate::runtime::mgmt::session::*;"),
        "substrate::mgmt::session must not glob re-export runtime session internals"
    );
    assert!(
        !substrate_rs.contains("crate::runtime::mgmt::session::"),
        "substrate::mgmt::session wrappers must not route through a stale runtime::mgmt::session owner path"
    );
    assert!(
        !substrate_rs.contains("crate::endpoint::cursor::CursorEndpoint<"),
        "substrate::mgmt::session must not leak CursorEndpoint in public signatures"
    );
    assert!(
        !substrate_rs.contains("pub struct CodeSessionRequest<'a> {"),
        "substrate::mgmt::session must not regrow a local request wrapper owner"
    );
    assert!(
        !substrate_rs
            .contains("pub fn run_code_session<'cfg, 'request, T, U, C, const MAX_RV: usize>("),
        "substrate::mgmt::session must not regrow local code-session sugar"
    );
    assert!(
        !runtime_mgmt_rs.contains("pub mod session;")
            && !runtime_mgmt_rs.contains("session::management_eff_lists()"),
        "internal runtime management kernel must not keep the stale session module owner"
    );
    assert!(
        runtime_mgmt_rs.contains("mod kernel;"),
        "internal runtime management choreography must live behind a non-surface kernel module"
    );
    assert!(
        runtime_mgmt_rs.contains("endpoint: crate::Endpoint<")
            && !runtime_mgmt_rs.contains("endpoint: crate::endpoint::cursor::CursorEndpoint<"),
        "runtime mgmt wrappers must stay on the public Endpoint facade"
    );
    let runtime_mgmt_ws = compact_ws(&runtime_mgmt_rs);
    assert!(
        runtime_mgmt_ws
            .contains("pub struct LoadRequest<'a> { pub slot: crate::substrate::policy::epf::Slot, pub code: &'a [u8], pub fuel_max: u16, pub mem_len: u16, }")
            && runtime_mgmt_ws.contains(
                "pub struct SlotRequest { pub slot: crate::substrate::policy::epf::Slot, }"
            )
            && runtime_mgmt_ws.contains(
                "pub struct LoadBegin { pub slot: crate::substrate::policy::epf::Slot, pub code_len: u32, pub fuel_max: u16, pub mem_len: u16, pub hash: u32, }"
            ),
        "runtime mgmt public payloads must spell the canonical public Slot owner directly"
    );
    assert!(
        !substrate_rs.contains(
            "        pub use crate::runtime::mgmt::session::{drive_stream_cluster, drive_stream_controller};"
        ),
        "substrate::mgmt::session must not re-export runtime stream drivers directly"
    );
    assert!(
        !substrate_rs.contains("        pub fn set_resolver<'cfg, T, U, C, const MAX_RV: usize>("),
        "substrate::mgmt::session must not keep a second public resolver registration entry"
    );
    assert!(
        !substrate_rs.contains("crate::runtime::mgmt::set_resolver("),
        "substrate::mgmt::session must not route through a stale runtime resolver wrapper"
    );
    assert!(
        !runtime_mgmt_rs
            .contains("pub(crate) fn set_resolver<'cfg, T, U, C, const MAX_RV: usize>(")
            && !runtime_mgmt_kernel_rs
                .contains("pub(crate) fn set_resolver<'cfg, T, U, C, const MAX_RV: usize>("),
        "runtime management owners must not keep the deleted duplicate resolver wrapper"
    );
    for required in [
        "        pub fn enter_controller<'lease, 'cfg, T, U, C, B, const MAX_RV: usize>(",
        "        pub fn enter_cluster<'lease, 'cfg, T, U, C, B, const MAX_RV: usize>(",
        "        pub fn enter_stream_controller<'lease, 'cfg, T, U, C, B, const MAX_RV: usize>(",
        "        pub fn enter_stream_cluster<'lease, 'cfg, T, U, C, B, const MAX_RV: usize>(",
        "        pub async fn drive_cluster<'lease, 'cfg, T, U, C, Mint, B, const MAX_RV: usize>(",
        "        pub async fn drive_stream_cluster<'lease, T, U, C, Mint, F, B, const MAX_RV: usize>(",
        "        pub async fn drive_stream_controller<'lease, T, U, C, Mint, F, B, const MAX_RV: usize>(",
        "            T: crate::substrate::Transport + 'lease,",
        "            T: crate::substrate::Transport + 'cfg,",
        "            U: crate::substrate::runtime::LabelUniverse,",
        "            C: crate::substrate::runtime::Clock,",
        "            Mint: crate::substrate::cap::advanced::MintConfigMarker,",
        "            Mint::Policy: crate::substrate::cap::advanced::AllowsCanonical,",
        "            F: FnMut() -> bool,",
        "            F: FnMut(crate::substrate::mgmt::session::tap::TapEvent) -> bool,",
        "            B: crate::substrate::binding::BindingSlot,",
        "            subscribe: crate::substrate::mgmt::SubscribeReq,",
    ] {
        assert!(
            substrate_rs.contains(required),
            "substrate::mgmt::session must provide curated management helpers: {required}"
        );
    }
    for forbidden in [
        "crate::runtime::mgmt::session::CONTROLLER_PROGRAM",
        "crate::runtime::mgmt::session::STREAM_CONTROLLER_PROGRAM",
        "crate::runtime::mgmt::session::STREAM_CLUSTER_PROGRAM",
        "pub use crate::runtime::mgmt::session::{CONTROLLER_PROGRAM",
        "pub use crate::runtime::mgmt::session::{STREAM_CLUSTER_PROGRAM",
        "pub use crate::runtime::mgmt::session::{STREAM_CONTROLLER_PROGRAM",
        "pub use crate::runtime::mgmt::session::{StreamControl",
    ] {
        assert!(
            !substrate_rs.contains(forbidden),
            "substrate::mgmt::session must not re-export choreography artifacts or bespoke traits: {forbidden}"
        );
    }
    assert!(
        !substrate_rs.contains("pub trait StreamControl"),
        "substrate::mgmt::session must not define a bespoke stream control trait"
    );
    assert!(substrate_rs.contains("        pub mod tap {"));
    assert!(
        substrate_rs.contains("            pub use crate::observe::core::TapEvent;"),
        "substrate::mgmt::session::tap must stay on the minimal TapEvent facade"
    );
    for forbidden in ["            pub mod events {", "            pub mod ids {"] {
        assert!(
            !substrate_rs.contains(forbidden),
            "substrate::mgmt::session::tap must not expose lower-layer observe helper buckets: {forbidden}"
        );
    }
    for forbidden in [
        "AssociationSnapshot",
        "FenceCounters",
        "PolicyEvent,",
        "PolicyEventKind,",
        "TAP_BATCH_MAX_EVENTS",
        "TapBatch,",
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
            "substrate::mgmt::session::tap must not leak extra observe helpers: {forbidden}"
        );
    }
    assert!(!substrate_rs.contains("pub mod observe {"));
    assert!(
        !substrate_rs
            .contains("pub fn run_code_session<'cfg, 'request, T, U, C, const MAX_RV: usize>("),
        "substrate::mgmt::session must not keep a local wrapper entry for management code sessions"
    );
    assert!(
        substrate_rs.contains("pub use crate::runtime::mgmt::{LoadRequest, Request, SlotRequest};"),
        "substrate::mgmt::session must re-export the canonical request sum type and payload owners"
    );
    assert!(
        substrate_rs.contains("impl<'request> Request<'request> {")
            && substrate_rs.contains(
                "pub async fn drive_controller<'lease, T, U, C, Mint, B, const MAX_RV: usize>("
            ),
        "substrate::mgmt::session must expose the canonical controller-role driver on Request itself"
    );
    for forbidden in [
        "Result<super::Reply, super::MgmtError>",
        "subscribe: super::SubscribeReq,",
        "F: FnMut(tap::TapEvent) -> bool,",
        "B: crate::binding::BindingSlot,",
        "            T: crate::transport::Transport + 'cfg,",
        "            U: crate::runtime::consts::LabelUniverse + 'cfg,",
        "            C: crate::runtime::config::Clock + 'cfg,",
    ] {
        assert!(
            !substrate_rs.contains(forbidden),
            "substrate::mgmt::session must use canonical substrate owner paths in public contracts: {forbidden}"
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
            "pub use crate::binding::{\n        BindingSlot, Channel, ChannelDirection, ChannelKey, ChannelStore, IncomingClassification,\n        NoBinding, SendDisposition, SendMetadata, TransportOpsError,\n    };"
        ),
        "substrate::binding root must stay on the minimal channel/binding surface"
    );
    assert!(
        !substrate_rs.contains("ChannelStoreError"),
        "substrate::binding must not leak a second channel-store error surface"
    );
    assert!(
        !runtime_mgmt_kernel_rs.contains("type LoopRouteSteps =")
            && !runtime_mgmt_kernel_rs.contains("type StreamLoopRouteSteps =")
            && !runtime_mgmt_kernel_rs.contains("type LoopContinueMsg =")
            && !runtime_mgmt_kernel_rs.contains("type LoopBreakMsg =")
            && !runtime_mgmt_kernel_rs.contains("type StreamContinueMsg =")
            && !runtime_mgmt_kernel_rs.contains("type StreamBreakMsg =")
            && !runtime_mgmt_kernel_rs.contains("type TapBatchMsg =")
            && runtime_mgmt_kernel_rs.contains("const LOOP_SEGMENT: Program<")
            && runtime_mgmt_kernel_rs.contains("LoopDecisionSteps<")
            && runtime_mgmt_kernel_rs.contains("LABEL_LOOP_CONTINUE,")
            && runtime_mgmt_kernel_rs.contains("LABEL_LOOP_BREAK,")
            && runtime_mgmt_kernel_rs.contains("const STREAM_LOOP_ROUTE: Program<")
            && runtime_mgmt_kernel_rs.contains("g::Role<1>,")
            && runtime_mgmt_kernel_rs.contains(
                "steps::SendStep<g::Role<1>, g::Role<0>, g::Msg<LABEL_OBSERVE_STREAM_END, ()>>"
            )
            && runtime_mgmt_kernel_rs.contains(
                "steps::SendStep<g::Role<1>, g::Role<0>, g::Msg<LABEL_OBSERVE_BATCH, TapBatch>>"
            ),
        "runtime mgmt kernel must use direct canonical loop witnesses instead of local loop aliases"
    );
    assert!(
        !runtime_mgmt_kernel_rs.contains("type CommandRouteSteps = CommandStep;"),
        "runtime mgmt kernel must not hide the command step behind a pure synonym alias"
    );
    assert!(
        !runtime_mgmt_kernel_rs.contains("type Controller = g::Role<0>;")
            && !runtime_mgmt_kernel_rs.contains("type Cluster = g::Role<1>;"),
        "runtime mgmt kernel must not keep pure role synonym aliases"
    );
    assert!(
        !runtime_mgmt_kernel_rs.contains("STREAM_LOOP_CONTINUE_PREFIX.then(")
            && runtime_mgmt_kernel_rs.contains("const STREAM_LOOP_CONTINUE_ARM: Program<")
            && runtime_mgmt_kernel_rs.contains("STREAM_LOOP_CONTINUE_PREFIX,")
            && runtime_mgmt_kernel_rs.contains(
                "g::send::<g::Role<1>, g::Role<0>, g::Msg<LABEL_OBSERVE_BATCH, TapBatch>, 0>(),"
            ),
        "runtime mgmt kernel must preserve the continue-arm segment via g::seq"
    );
    for forbidden in [
        "BindingSlot as Binding",
        "IncomingClassification as Incoming",
        "NoBinding as NullBinding",
        "SendMetadata as SendMeta",
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
