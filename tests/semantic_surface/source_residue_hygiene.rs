use super::common::*;

#[test]
fn role_projection_does_not_hide_exact_count_dispatch() {
    let production = read_production_rs_tree("src");
    let role_projection = read("src/g/role_projection.rs");

    assert!(
        role_projection.len() <= 20 * 1024,
        "role_projection.rs must stay a small value-level projection boundary, not a generated dispatch table"
    );
    assert!(
        !repo_file_exists("src/g/role_projection/role_image_dispatch.rs"),
        "role_image_dispatch.rs must not return as generated exact-count dispatch"
    );
    for forbidden in [
        "role_image_dispatch",
        "dispatch_role_",
        "RoleProjectionColumns<",
        "local_step_events_exact::<",
        "local_step_lanes_exact::<",
        "route_arm_rows_exact::<",
    ] {
        assert!(
            !production.contains(forbidden),
            "production source must not encode role image row counts as type dispatch: {forbidden}"
        );
    }

    for line in production.lines() {
        assert!(
            !(line.contains("macro_rules!")
                && line.contains("role")
                && line.contains("projection")),
            "role projection must not be hidden behind a macro-generated dispatch table: {line}"
        );
        assert!(
            !(line.contains("include!") && line.contains("role") && line.contains("projection")),
            "role projection must not include a generated dispatch table: {line}"
        );
    }

    if repo_file_exists("build.rs") {
        let build_rs = read("build.rs");
        assert!(
            !(build_rs.contains("role_projection")
                || build_rs.contains("role projection")
                || build_rs.contains("dispatch_role_")),
            "build.rs must not generate role projection dispatch"
        );
    }
}

#[test]
fn production_sources_do_not_retain_test_only_effect_or_offer_helpers() {
    let production = read_production_rs_tree("src");
    for forbidden in [
        "for_test",
        "CpCommand",
        "PendingEffect",
        "EffectRunner",
        "DelegateOperands",
        "pub(crate) mod delegation",
        "DELEGATION_LEASE",
        "delegation_children",
        "DelegationLeaseSpec",
        "struct EffectEnvelope {",
        "enum EffectEnvelopeSource",
        concat!("control", "_op_is_idempotent"),
        concat!("control", "_op_requires_gen_bump"),
        concat!("control", "_op_is_terminal"),
        concat!("control", "_op_modifies_history"),
        "emit_resolver_event_with_arg2",
        "run_effect_step",
        "after_local_effect",
        "PendingCapRelease::inert",
        "pub(crate) fn inert() -> Self",
        "pub(crate) fn disarm(&mut self)",
        "ResolverEventSpec",
        "ResolverEventKind",
        "TapEvents",
        "#[cfg(all(test, hibana_repo_tests))]\npub const",
        "pub const ROUTE_PICK",
        "pub const RESOLVER_ABORT",
        "pub const RESOLVER_ANNOT",
        "pub const RESOLVER_TRAP",
        "pub const RESOLVER_EFFECT",
        "pub const RESOLVER_STATE_RESTORE",
        "TEST_GLOBAL_TAP_RING",
        "TS_CHECKER",
        "install_ts_checker",
        "global_tap_ring_ptr",
        "check_event_timestamp",
        "_ => ScopeKind::Generic",
        "inferred item nodes",
        "inferred item generated",
        "JumpReason",
        "JumpError",
        "try_follow_jumps_from_index",
        "try_next_index_past_jumps_from",
        "flow_follow_jumps_from",
        "jump_reason_at",
        "jump_target_at",
        "is_jump_at",
    ] {
        assert!(
            !production.contains(forbidden),
            "production sources must not retain repo-test effect runners or for-test bypasses: {forbidden}"
        );
    }
}

#[test]
fn source_tree_does_not_retain_impossible_test_only_fixtures() {
    let source = read_all_rs_tree("src");
    let forbidden_route_ack_dispatch = concat!("dispatch_", "topo", "logy", "_ack_with_handle");
    for forbidden in [
        "CpCommand",
        "PendingEffect",
        "EffectRunner",
        "DelegateOperands",
        "delegate_resolver",
        "endpoint_delegate",
        "invalid delegate token",
        "run_effect_step",
        "after_local_effect",
        forbidden_route_ack_dispatch,
        concat!("syn", "thetic", "_for_test"),
        "transport_for_test",
        "add_rendezvous_auto",
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
fn package_artifact_ships_repo_tests_without_publish_warning_filter() {
    let cargo = read("Cargo.toml");
    let package_gate = read(".github/scripts/check_package_artifact.sh");

    assert!(
        !cargo.contains("autotests")
            && !cargo.contains("[[test]]")
            && cargo.contains("\"/tests/**\"")
            && !package_gate.contains("repo repository tests must not ship")
            && !package_gate.contains("run_package_clean_with_omitted_repo_tests")
            && !package_gate.contains("ignoring test `"),
        "repo repository tests must stay Cargo-auto-discovered and ship with the crate so publish is warning-free"
    );
    assert!(
        package_gate.contains("run_package_clean \"cargo package --no-verify\"")
            && package_gate.contains("shipped repository tests must include their module tree")
            && package_gate.contains("package representative test build --features std")
            && package_gate.contains("--test semantic_surface --no-run")
            && package_gate.contains("cargo +\"${TOOLCHAIN}\" test --manifest-path"),
        "package artifact gate must reject package warnings and compile a representative packaged repository target"
    );
}

#[test]
fn cached_recv_meta_index_overflow_fails_closed() {
    fn impl_fn_body<'a>(source: &'a str, name: &str) -> &'a str {
        let marker = format!("fn {name}(");
        let tail = source
            .split(&marker)
            .nth(1)
            .unwrap_or_else(|| panic!("{name} must stay visible"));
        let next = tail
            .find("\n    #[inline]\n    fn ")
            .or_else(|| tail.find("\n    fn "))
            .unwrap_or(tail.len());
        &tail[..next]
    }

    let source = read("src/endpoint/kernel/core/decision_resolver/impls/select.rs");
    for name in [
        "cached_recv_meta_from_recv",
        "cached_recv_meta_from_send",
        "cached_recv_meta_from_local",
        "route_arm_cached_recv_meta",
    ] {
        let body = impl_fn_body(&source, name);
        assert!(
            body.contains("checked_state_index("),
            "{name} must keep StateIndex bounds explicit"
        );
        assert!(
            body.contains("crate::invariant()"),
            "{name} must fail closed when descriptor/cursor indices cannot fit StateIndex"
        );
        assert!(
            !body.contains("return CachedRecvMeta::EMPTY;"),
            "{name} must not hide index overflow as missing receive evidence"
        );
    }
}
