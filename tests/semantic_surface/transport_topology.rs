use super::common::*;

#[test]
fn transport_optional_default_is_explicit_and_documented() {
    let transport = transport_source();
    let readme = read("README.md");
    let hygiene = read(".github/scripts/check_surface_hygiene.sh");

    assert!(
        transport.contains("fn operational_deadline_ticks(&self) -> Option<u32>")
            && !transport.contains("apply_pacing_update"),
        "transport optional default must stay limited to the explicit wait-fuse hook"
    );
    for required in [
        "fn drain_events(&self, emit: &mut dyn FnMut(TransportEvent));",
        "fn recv_frame_hint<'a>(&'a self, rx: &'a Self::Rx<'a>) -> Option<FrameLabel>;",
        "fn metrics(&self) -> Self::Metrics;",
    ] {
        assert!(
            transport.contains(required),
            "non-optional transport evidence contracts must remain required: {required}"
        );
    }
    assert!(
        hygiene.contains("drain_events")
            && hygiene.contains("recv_frame_hint")
            && !hygiene.contains("fn[[:space:]]+apply_pacing_update"),
        "surface hygiene gate must continue rejecting semantic fallback hooks"
    );
    assert!(
        readme.contains("`drain_events(...)` and `metrics()` for observation and policy input")
            && readme.contains(
                "optional `operational_deadline_ticks()` for integration-owned wait fuses"
            )
            && !readme.contains("apply_pacing_update"),
        "README must document the wait-fuse default without teaching unused pacing fallback hooks"
    );
    assert!(
        transport.contains("Provide transport-level metrics for observation and policy input.")
            && transport.contains("Metrics are not route authority")
            && !transport.contains("metrics for routing decisions")
            && !transport.contains("adaptive route selection")
            && !transport.contains("protocol inference"),
        "Transport::metrics docs must keep metrics as observation/policy input, not route authority"
    );
}

#[test]
fn endpoint_resident_payload_unsafe_contracts_are_documented() {
    let lane_port = read("src/endpoint/kernel/runtime/lane_port.rs");

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

    let perform_effect = rendezvous_core
        .split_once("fn perform_effect(")
        .and_then(|(_, tail)| {
            tail.split_once("fn eval_effect(")
                .map(|(section, _)| section)
        })
        .expect("rendezvous core must keep perform_effect before eval_effect");

    for forbidden in [
        "ControlOp::TopologyBegin",
        "ControlOp::TopologyAck",
        "ControlOp::TopologyCommit",
    ] {
        assert!(
            !perform_effect.contains(forbidden),
            "topology operations must stay out of direct Rendezvous::perform_effect: {forbidden}"
        );
    }
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
