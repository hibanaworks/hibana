use super::common::*;

#[test]
fn transport_contract_is_io_only_and_documented() {
    let transport = transport_source();
    let readme = read("README.md");
    let hygiene = read(".github/scripts/check_surface_hygiene.sh");

    assert!(
        !transport.contains("fn operational_deadline_ticks(&self)")
            && !transport.contains("fn policy_attrs(&self)")
            && !transport.contains("fn drain_events(&self)")
            && !transport.contains("TransportEvent")
            && !transport.contains("apply_pacing_update")
            && !transport.contains("LocalDirection")
            && !transport.contains("pub const fn is_local"),
        "transport must stay protocol-neutral I/O plus rollback/hint hooks"
    );
    for required in [
        "fn open<'a>(&'a self, port: PortOpen) -> (Self::Tx<'a>, Self::Rx<'a>);",
        "fn requeue<'a>(&self, rx: &mut Self::Rx<'a>);",
        "fn cancel_send<'a>(&self, tx: &'a mut Self::Tx<'a>);",
        "fn recv_frame_hint<'a>(&self, rx: &mut Self::Rx<'a>) -> Option<FrameLabel> {",
    ] {
        assert!(
            transport.contains(required),
            "transport surface must keep the minimal rollback/hint contract: {required}"
        );
    }
    assert!(
        !transport.contains("fn open<'a>(&self, port: PortOpen)"),
        "Transport::open must bind Tx/Rx handles to the transport borrow, not an unconstrained lifetime"
    );
    assert!(
        !transport.contains("type Metrics") && !transport.contains("fn metrics("),
        "transport surface must not keep a metrics associated type or compatibility hook"
    );
    assert!(
        hygiene.contains("recv_frame_hint")
            && !hygiene.contains("fn[[:space:]]+apply_pacing_update"),
        "surface hygiene gate must continue rejecting semantic fallback hooks"
    );
    assert!(
        readme.contains("The only optional transport hook is:")
            && readme.contains("`cancel_send(...)` for transport cleanup")
            && readme
                .contains("`recv_frame_hint(...)` as a non-blocking route-observation hint drain")
            && readme.contains(
                "Resolver input belongs to binding / integration policy state, not transport."
            )
            && !readme.contains("apply_pacing_update"),
        "README must document transport as I/O, rollback, and hint drain only"
    );
}

#[test]
fn endpoint_resident_payload_unsafe_contracts_are_documented() {
    let lane_port = read("src/endpoint/kernel/lane_port.rs");

    for function in ["endpoint_resident_payload", "recv_from_binding"] {
        let marker = format!("unsafe fn {function}");
        let start = lane_port
            .find(&marker)
            .unwrap_or_else(|| panic!("missing unsafe helper: {function}"));
        let prefix_start = start.saturating_sub(700);
        let prefix = &lane_port[prefix_start..start];
        assert!(
            prefix.contains("# Safety"),
            "{function} must carry its unsafe preconditions at the function boundary"
        );
    }
}

#[test]
fn type_level_choreography_stays_segmented_without_new_dsl() {
    let g = read("src/g.rs");
    let readme = read("README.md");
    let root_allowlist = read(".github/allowlists/g-public-api.txt");

    assert!(
        g.contains("pub use crate::global::program::Program;")
            && g.contains("pub use crate::global::{Msg, Role, par, route, send, seq};")
            && !g.contains("macro_rules!")
            && !g.contains("advanced")
            && !g.contains("loop_"),
        "app-facing choreography DSL must stay fixed to g::{{Role, Msg, Program, send, seq, route, par}}"
    );
    assert_eq!(
        lines(".github/allowlists/g-public-api.txt"),
        [
            "pub use Program;",
            "pub use Msg;",
            "pub use Role;",
            "pub use par;",
            "pub use route;",
            "pub use send;",
            "pub use seq;"
        ],
        "semantic surface must guard the app-facing DSL contract instead of pinning internal program-image storage"
    );
    for forbidden in ["advanced", "loop_", "fallback", "legacy", "compat"] {
        assert!(
            !root_allowlist.contains(forbidden) && !readme.contains(&format!("`g::{forbidden}`")),
            "public choreography docs must not grow extra DSL affordances: {forbidden}"
        );
    }
}

#[test]
fn transport_contract_documents_lane_and_hint_drain() {
    let readme = read("README.md");
    let transport = transport_source();
    let transport_tests = read("src/transport/tests.rs");
    let test_transport = read("tests/common/mod.rs");

    for (path, source) in [
        ("README.md", readme.as_str()),
        ("src/transport.rs", transport.as_str()),
    ] {
        assert!(
            source.contains("open(port)") || source.contains("PortOpen"),
            "{path} must document Transport::open as a descriptor-derived port witness"
        );
        assert!(
            source.contains("hint-drain"),
            "{path} must document recv_frame_hint as a route-observation drain"
        );
        assert!(
            source.contains("must not consume payload bytes")
                || source.contains("must not yield the same observation again"),
            "{path} must separate route-observation draining from payload receive"
        );
    }

    assert!(
        !readme.contains("open(local_role")
            && !readme.contains("open(local_role, session_id, lane)"),
        "README must not keep the old raw Transport::open contract"
    );
    assert!(
        readme.contains("`requeue(...)` as the required rollback path")
            && transport.contains("A no-op requeue violates the")
            && transport.contains("endpoint rollback contract"),
        "Transport::requeue must be documented as a required rollback contract, not an optional best-effort hook"
    );
    assert!(
        !transport_tests.contains("let _ = rx;") && !test_transport.contains("_lane: u8"),
        "test transports must not silently ignore rollback or opened logical lanes"
    );
}

#[test]
fn resolver_reject_error_captures_public_callsite() {
    let reject_line = line!() + 1;
    let error = hibana::integration::policy::ResolverError::reject();

    assert_eq!(error.operation(), "reject");
    assert!(
        error
            .file()
            .ends_with("tests/semantic_surface/transport_topology.rs")
    );
    assert_eq!(error.line(), reject_line);
}

#[test]
fn topology_validation_has_no_test_only_semantic_owner() {
    let topology = read("src/control/automaton/topology.rs");
    let distributed = read("src/control/automaton/distributed.rs");
    let rendezvous_topology = read("src/rendezvous/topology.rs");
    let rendezvous_core = rendezvous_core_source();

    for forbidden in [
        "TopologyCommitAutomaton",
        "pub(crate) fn process_intent",
        "DistributedTopology::process_intent",
        "pub(super) fn topology_commit",
        ".topology.topology_commit(",
    ] {
        assert!(
            !topology.contains(forbidden)
                && !distributed.contains(forbidden)
                && !rendezvous_topology.contains(forbidden)
                && !rendezvous_core.contains(forbidden),
            "topology validation must use production cluster/rendezvous paths, not test-only owner: {forbidden}"
        );
    }

    assert!(
        !rendezvous_core.contains("fn perform_effect("),
        "test-only effect replay must live under src/**/tests/**, not in production rendezvous core modules"
    );
}

#[test]
fn stable_public_api_gate_has_no_nightly_or_rustdoc_json_owner() {
    let script = read(".github/scripts/check_hibana_public_api.sh");
    let final_gate = read(".github/scripts/run_final_form_gates.sh");
    let workflow = read(".github/workflows/quality-gates.yml");
    let combined = format!("{script}\n{final_gate}\n{workflow}");

    for required in [
        "export TOOLCHAIN=\"${TOOLCHAIN:-1.95.0}\"",
        "bash ./.github/scripts/run_final_form_gates.sh",
        "bash ./.github/scripts/check_hibana_public_api.sh",
        "stable public API check passed",
    ] {
        assert!(
            combined.contains(required),
            "Rust 1.95 public API gate missing required owner: {required}"
        );
    }

    for forbidden in [
        "dtolnay/rust-toolchain@nightly",
        "rustup which cargo --toolchain nightly",
        "rustup which rustc --toolchain nightly",
        "rustup which rustdoc --toolchain nightly",
        "target/doc/hibana.json",
        "HIBANA_RUSTDOC_JSON",
        "-Z unstable-options",
        "--output-format json",
    ] {
        assert!(
            !combined.contains(forbidden),
            "stable public API gate must not depend on nightly rustdoc JSON: {forbidden}"
        );
    }
}
