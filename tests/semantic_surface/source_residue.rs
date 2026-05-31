use super::common::*;

#[test]
fn production_sources_do_not_retain_test_only_effect_or_offer_helpers() {
    let production = read_production_rs_tree("src");
    for forbidden in [
        "for_test",
        "CpCommand",
        "PendingEffect",
        "EffectRunner",
        "DelegateOperands",
        "struct EffectEnvelope {",
        "enum EffectEnvelopeSource",
        "control_op_is_idempotent",
        "control_op_requires_gen_bump",
        "control_op_is_terminal",
        "control_op_modifies_history",
        "emit_policy_event_with_arg2",
        "run_effect_step",
        "after_local_effect",
        "PendingCapRelease::inert",
        "pub(crate) fn inert() -> Self",
        "pub(crate) fn disarm(&mut self)",
        "PolicyEventSpec",
        "PolicyEventKind",
        "TapEvents",
        "TEST_GLOBAL_TAP_RING",
        "TS_CHECKER",
        "install_ts_checker",
    ] {
        assert!(
            !production.contains(forbidden),
            "production sources must not retain repo-test effect runners or for-test escape hatches: {forbidden}"
        );
    }
}

#[test]
fn source_tree_does_not_retain_impossible_test_only_fixtures() {
    let source = read_all_rs_tree("src");
    for forbidden in [
        "CpCommand",
        "PendingEffect",
        "EffectRunner",
        "DelegateOperands",
        "run_effect_step",
        "after_local_effect",
        "dispatch_topology_ack_with_handle",
        "synthetic_for_test",
        "transport_for_test",
        "NonNull::dangling",
        "receipt: None",
    ] {
        assert!(
            !source.contains(forbidden),
            "source tests must not retain test-only effect runners or impossible transport fixtures: {forbidden}"
        );
    }
}

#[test]
fn package_artifact_does_not_ship_repo_integration_tests() {
    let cargo = read("Cargo.toml");
    let package_gate = read(".github/scripts/check_package_artifact.sh");

    assert!(
        !cargo.contains("autotests")
            && !cargo.contains("[[test]]")
            && !cargo.contains("\"/tests/**\"")
            && package_gate.contains("repo integration tests must not ship")
            && package_gate.contains("'^tests/'"),
        "repo integration tests must stay auto-discovered locally and absent from the production crate package"
    );
    assert!(
        package_gate
            .contains("run_package_allowing_omitted_repo_tests \"cargo package --no-verify\"")
            && package_gate.contains("package test build --features std")
            && package_gate.contains("cargo +\"${TOOLCHAIN}\" test --manifest-path"),
        "package artifact gate must whitelist only Cargo's omitted repo-test warnings and compile the packaged test target"
    );
}

#[test]
fn decode_failure_completion_is_terminal_without_branch_restore() {
    let endpoint = endpoint_facade_source();
    let decode = read("src/endpoint/kernel/decode.rs");

    assert!(
        !endpoint.contains("core::hint::black_box") && !decode.contains("core::hint::black_box"),
        "decode terminal cleanup must not rely on black_box to hide branch ownership"
    );
    assert!(
        !endpoint.contains("unsafe fn begin_public_decode_state(&mut self) -> RecvResult<()>"),
        "begin_public_decode_state must not expose a dead Result"
    );

    assert!(
        read("tests/no_policy_route_transport_hint.rs")
            .contains("completed decode future must fail fast on post-Ready poll"),
        "decode terminal paths must be guarded by behavior coverage, not private cleanup helper names"
    );
}

#[test]
fn offer_transport_payload_presence_is_not_length_sentinel() {
    let offer = offer_frontier_source();
    let offer_ingress = read("src/endpoint/kernel/offer/ingress.rs");
    let offer_materialization = read("src/endpoint/kernel/offer/materialization.rs");
    let offer_state = read("src/endpoint/kernel/offer/state.rs");
    let core = read("src/endpoint/kernel/core.rs");

    for forbidden in [
        "transport_payload_len",
        "transport_payload_lane",
        "binding_evidence: [Option<LaneIngressEvidence>; 2]",
        "transport_payload: [Option<",
    ] {
        assert!(
            !offer.contains(forbidden)
                && !offer_ingress.contains(forbidden)
                && !offer_materialization.contains(forbidden)
                && !offer_state.contains(forbidden),
            "offer preview staging must not resurrect stale sentinel or anonymous rollback storage: {forbidden}"
        );
    }
    assert!(
        !offer.contains("!payload.as_bytes().is_empty()")
            && !offer_ingress.contains("!payload.as_bytes().is_empty()")
            && !offer_materialization.contains("!payload.as_bytes().is_empty()"),
        "offer preview staging must keep zero-length transport payloads as real consumed frames"
    );
    assert!(
        !core.contains("for (len, lane, _payload) in rollback.transport_payload"),
        "offer rollback must not hide ingress ownership in tuple mini-vec iteration"
    );
}
