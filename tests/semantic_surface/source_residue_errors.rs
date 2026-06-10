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
fn control_errors_do_not_retain_unreachable_legacy_variants() {
    let source = read("src/control/cluster/error.rs")
        + &read("src/control/cluster/error/debug.rs")
        + &read("src/control.rs")
        + &read("src/rendezvous/error.rs")
        + &read("src/rendezvous/tables.rs")
        + &read("src/rendezvous/tables/generation.rs");
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
        "CpError::Abort",
        "CpError::StateSnapshot",
        "CpError::ResourceMismatch",
        "ResourceMismatch {",
        "InvalidLane",
        "AckTimeout",
        concat!("Delegation", "Error"),
        concat!("CpError::", "Delegation"),
        concat!("Cap", "Delegate"),
        "ResourceScope::SessionKit",
        "ResourceScope::RendezvousSlot",
        "ResourceScope::ProgramImage",
        "ResourceScope::RoleImage",
        "ResourceScope::EndpointStorageBudget",
        "ResourceScope::EndpointPin",
        "ResourceScope::ControlLaneStorage",
        "ResourceScope::Generic",
        "CpError::Generic",
        "Self::Generic",
        "Generic =>",
        "SessionKit =>",
        "RendezvousSlot =>",
        "ProgramImage =>",
        "RoleImage =>",
        "EndpointStorageBudget =>",
        "EndpointPin =>",
        "ControlLaneStorage =>",
    ] {
        assert!(
            !source.contains(forbidden),
            "control error surface must not retain unreachable legacy variants: {forbidden}"
        );
    }
}
