use super::common::*;

#[test]
fn rendezvous_resident_state_does_not_keep_measurement_only_frontier_words() {
    let source = read("src/rendezvous/core.rs")
        + &read("src/rendezvous/core/storage_layout.rs")
        + &read("src/rendezvous/core/lane_lifecycle.rs");
    for forbidden in [
        "runtime_frontier: u32",
        "update_runtime_frontier",
        "runtime_sidecar_high_water_bytes",
        "runtime_image_frontier_bytes",
        "runtime_frontier_workspace_bytes",
        "runtime_storage_snapshot",
        "live_endpoint_storage_bytes",
        "direct_init_lane_range",
        "init_from_config(",
        "cleanup_failed_public_init",
    ] {
        assert!(
            !source.contains(forbidden),
            "production Rendezvous resident state must not retain measurement-only frontier metadata: {forbidden}"
        );
    }
}

#[test]
fn session_errors_do_not_retain_forbidden_variants() {
    let source = read("src/session/cluster/error.rs")
        + &read("src/session/cluster/error/debug.rs")
        + &read("src/session.rs")
        + &read("src/rendezvous/error.rs");
    assert!(
        !repo_file_exists("src/rendezvous/tables/generation.rs"),
        "generation table substrate must stay forbidden"
    );
    for forbidden in [
        "GenerationRecord",
        "GenError",
        "check_and_update(",
        "restore_to(",
        "pub enum AbortError",
        "Abort(AbortError)",
        "impl From<AbortError>",
        "impl fmt::Debug for AbortError",
        "pub enum StateSnapshotError",
        "StateSnapshot(StateSnapshotError)",
        "impl From<StateSnapshotError>",
        "impl fmt::Debug for StateSnapshotError",
        "ClusterError::Abort",
        "ClusterError::StateSnapshot",
        "ClusterError::ResourceMismatch",
        "UnsupportedEffect",
        "ResolverSiteStatic",
        "ResourceMismatch {",
        "InvalidLane",
        "AckTimeout",
        "DelegationError",
        "ClusterError::Delegation",
        "CapDelegate",
        "ResourceScope::SessionKit",
        "ResourceScope::RendezvousSlot",
        "ResourceScope::ProgramImage",
        "ResourceScope::RoleImage",
        "ResourceScope::EndpointStorageBudget",
        "ResourceScope::EndpointPin",
        "ResourceScope::SessionLaneStorage",
        "ResourceScope::Generic",
        "    EndpointBounds,",
        "    EndpointMark,",
        "    EndpointHeader,",
        "ClusterError::Generic",
        "Self::Generic",
        "Generic =>",
        "SessionKit =>",
        "RendezvousSlot =>",
        "ProgramImage =>",
        "RoleImage =>",
        "EndpointStorageBudget =>",
        "EndpointPin =>",
        "SessionLaneStorage =>",
    ] {
        assert!(
            !source.contains(forbidden),
            "session error surface must not retain forbidden variants: {forbidden}"
        );
    }
}

#[test]
fn dynamic_resolver_resolution_does_not_encode_authority_as_reject_reason() {
    let source = read("src/session/cluster/core/session_effect_steps.rs")
        + &read("src/endpoint/kernel/offer/resolve.rs")
        + &read("src/endpoint/kernel/core/decision_resolver/impls.rs")
        + &read("src/endpoint/kernel/core/decision_resolver/impls/select.rs");
    for forbidden in [
        "ResolverReject { resolver_id: 0 }",
        "ResolverReject { resolver_id: 6 }",
        "reason != 0",
        "Abort(0)",
        ".unwrap_or(cause)",
        "DynamicResolverResolution::ResolverUnbound",
        "RouteResolveStep::ResolverUnbound",
        "RouteResolveOutcome::ResolverUnbound",
        "ResolverUnbound",
    ] {
        assert!(
            !source.contains(forbidden),
            "dynamic resolver authority must not keep unregistered-resolver alternate paths: {forbidden}"
        );
    }
    assert!(
        source.contains("return Err(ClusterError::DynamicResolverInvariant { resolver_id });"),
        "unregistered dynamic resolver sites must fail closed as projection/runtime invariant"
    );
}

#[test]
fn endpoint_session_context_does_not_keep_optional_cluster_or_resolver_layer() {
    let source = read("src/endpoint/session.rs")
        + &read("src/endpoint/kernel/public_ops.rs")
        + &read("src/session/cluster/core/session_cluster_ops.rs");
    let optional_resolver_layer = "_resolver: Option<()";
    for forbidden in [
        "cluster: Option<",
        optional_resolver_layer,
        ".map(|cluster| cluster.poison_session",
        ".unwrap_or(cause)",
        "if let Some(cluster) = self.session.cluster()",
    ] {
        assert!(
            !source.contains(forbidden),
            "endpoint session context must not retain optional-cluster extra residue: {forbidden}"
        );
    }
}
