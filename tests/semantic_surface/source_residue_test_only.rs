use super::common::*;

#[test]
fn production_resident_images_do_not_retain_debug_identity_metadata() {
    let compiled_image = read("src/global/compiled/images/image.rs")
        + &read_production_rs_tree("src/global/compiled/images/image");
    let lowering = read("src/global/compiled/lowering/mod.rs")
        + &read_production_rs_tree("src/global/compiled/lowering");
    let event_program = read("src/global/event_program.rs");
    let cursor = read("src/global/typestate/cursor.rs");
    let role_projection = read("src/g/role_projection.rs");

    for (name, source) in [
        ("compiled image", compiled_image.as_str()),
        ("lowering", lowering.as_str()),
        ("event program", event_program.as_str()),
        ("cursor", cursor.as_str()),
        ("g projection", role_projection.as_str()),
    ] {
        for forbidden in [
            "ProgramImageBytes { stamp",
            "CompiledProgramRef { stamp",
            concat!("pub(super) stamp: ", "Program", "Stamp"),
            ".field(\"stamp\"",
            "impl PartialEq for CompiledProgramRef",
            "impl Eq for CompiledProgramRef",
            "impl PartialEq for RoleDescriptorRef",
            "impl Eq for RoleDescriptorRef",
            "impl PartialEq for LocalEventProgram",
            "impl Eq for LocalEventProgram",
            "cfg_attr(test",
            concat!("Program", "Stamp"),
            concat!("Role", "Debug", "Facts"),
            concat!("Role", "Debug", "Footprint"),
            concat!("Role", "Image", "Source"),
            "compiled_program_image(",
            "program_image(",
            "pub(crate) const fn compact_blob_len",
            "pub(crate) const fn largest_section_bytes",
            "pub(crate) const fn panic_repo_test",
            "pub(crate) fn write_lane_indices",
            "pub(crate) const fn segment_summary",
            "pub(super) const fn control_markers",
            "pub(crate) fn policy_at",
            "pub(crate) fn control_desc_at",
        ] {
            assert!(
                !source.contains(forbidden),
                "{name} must not retain production debug/equality metadata: {forbidden}"
            );
        }
    }

    assert!(
        !repo_file_exists(
            "src/global/compiled/images/image/role_descriptor_ref/tests/route_scope.rs"
        ),
        "RoleDescriptorRef must not keep a test-only lowering-scratch route-scope backpointer helper"
    );
}

#[test]
fn production_sources_do_not_retain_test_only_control_or_trace_helpers() {
    let lease_core = read("src/control/lease/core.rs");
    let lease_mod = read("src/control/lease.rs");
    let rendezvous_core = read("src/rendezvous/core.rs")
        + &read("src/rendezvous/core/local_topology.rs")
        + &read("src/rendezvous/core/lane_lifecycle.rs");
    let route_hints = read("src/rendezvous/port/route_hints.rs");
    let route_table_storage = read("src/rendezvous/tables/route_table/storage.rs");
    let capability = read("src/rendezvous/capability.rs");
    let transport_labels = read("src/transport/labels.rs");
    let observe_scope = read("src/observe/scope.rs");
    let const_dsl = read("src/global/const_dsl.rs");
    let control_txn =
        read("src/control/automaton/txn.rs") + &read("src/control/automaton/distributed.rs");
    let cluster_core = read("src/control/cluster/core/session_cluster_ops.rs");
    let cluster_topology = read("src/control/cluster/core/cluster_storage.rs")
        + &read("src/control/cluster/core/session_effect_steps.rs")
        + &read("src/control/cluster/core/topology_state.rs")
        + &read("src/control/cluster/core/descriptor_controls/prepared_send.rs");
    let frontier_state = read("src/endpoint/kernel/frontier_state.rs");
    let frontier_select = read("src/endpoint/kernel/core/frontier_select.rs");
    let frontier_observation = read("src/endpoint/kernel/core/frontier_observation.rs")
        + &read("src/endpoint/kernel/core/frontier_observation/cache_slots.rs");
    let offer_refresh = read("src/endpoint/kernel/core/offer_refresh.rs");
    let commit_delta = read("src/endpoint/kernel/core/commit_delta.rs")
        + &read("src/endpoint/kernel/core.rs")
        + &read("src/endpoint/kernel/decision_state/tests.rs");

    for (name, source, forbidden) in [
        (
            "lease core",
            lease_core.as_str(),
            "pub(crate) struct TopologySpec",
        ),
        (
            "lease core",
            lease_core.as_str(),
            "pub(crate) trait ControlAutomaton",
        ),
        (
            "lease core",
            lease_core.as_str(),
            "pub(crate) enum ControlStep",
        ),
        ("lease core", lease_core.as_str(), "is_registered("),
        ("lease module", lease_mod.as_str(), "mod bundle"),
        ("rendezvous core", rendezvous_core.as_str(), "TopologyFacet"),
        (
            "rendezvous core",
            rendezvous_core.as_str(),
            "topology_facet(",
        ),
        (
            "rendezvous core",
            rendezvous_core.as_str(),
            "fn topology_begin_from_intent(",
        ),
        (
            "rendezvous core",
            rendezvous_core.as_str(),
            "pub(crate) fn topology_begin(",
        ),
        (
            "rendezvous core",
            rendezvous_core.as_str(),
            "pub(crate) fn is_session_registered",
        ),
        (
            "rendezvous core",
            rendezvous_core.as_str(),
            "pub(crate) fn session_lane",
        ),
        (
            "rendezvous core",
            rendezvous_core.as_str(),
            "pub(crate) fn advance_lane_generation_to",
        ),
        (
            "route hints",
            route_hints.as_str(),
            "pub(super) const fn new()",
        ),
        ("route hints", route_hints.as_str(), "pub(super) fn push("),
        (
            "route hints",
            route_hints.as_str(),
            "pub(super) fn has_matching",
        ),
        (
            "route table",
            route_table_storage.as_str(),
            "build_test_table",
        ),
        (
            "route table",
            route_table_storage.as_str(),
            "allocate_test_storage",
        ),
        (
            "capability table",
            capability.as_str(),
            "pub(crate) fn insert_entry(&self, entry: CapEntry)",
        ),
        (
            "transport labels",
            transport_labels.as_str(),
            "has_matching_in_word",
        ),
        (
            "transport labels",
            transport_labels.as_str(),
            "pub(crate) fn has_matching",
        ),
        (
            "observe scope",
            observe_scope.as_str(),
            "ScopeTrace::decode",
        ),
        ("observe scope", observe_scope.as_str(), "fn tap_scope("),
        (
            "const dsl",
            const_dsl.as_str(),
            "pub(crate) const fn eff_len",
        ),
        (
            "const dsl",
            const_dsl.as_str(),
            "pub(crate) const fn scope_marker_len",
        ),
        (
            "const dsl",
            const_dsl.as_str(),
            "pub(crate) const fn route_scope_enter_len",
        ),
        (
            "const dsl",
            const_dsl.as_str(),
            "pub(crate) const fn control_marker_len",
        ),
        (
            "const dsl",
            const_dsl.as_str(),
            "pub(crate) const fn policy_marker_len",
        ),
        (
            "const dsl",
            const_dsl.as_str(),
            "pub(crate) const fn control_spec_len",
        ),
        (
            "cluster core",
            cluster_core.as_str(),
            "resident_test_role_image",
        ),
        (
            "frontier state",
            frontier_state.as_str(),
            "frontier_observation_epoch",
        ),
        (
            "frontier state",
            frontier_state.as_str(),
            "global_frontier_observed",
        ),
        (
            "frontier state",
            frontier_state.as_str(),
            "fn next_observation_epoch",
        ),
        (
            "frontier state",
            frontier_state.as_str(),
            "fn cached_frontier_observed_entries",
        ),
        (
            "frontier state",
            frontier_state.as_str(),
            "fn frontier_observation_cache",
        ),
        (
            "frontier state",
            frontier_state.as_str(),
            "fn store_frontier_observation",
        ),
        ("frontier select", frontier_select.as_str(), "#[cfg(test)]"),
        (
            "frontier select",
            frontier_select.as_str(),
            "#[cfg(not(test))]",
        ),
        (
            "frontier observation",
            frontier_observation.as_str(),
            "#[cfg(test)]",
        ),
        (
            "frontier observation",
            frontier_observation.as_str(),
            "#[cfg(not(test))]",
        ),
        ("offer refresh", offer_refresh.as_str(), "#[cfg(test)]"),
        (
            "commit delta",
            commit_delta.as_str(),
            "test_commit_delta_apply_permit",
        ),
        (
            "cluster topology",
            cluster_topology.as_str(),
            "cached_operands",
        ),
        (
            "cluster topology",
            cluster_topology.as_str(),
            "CachedTopologyBucket",
        ),
        (
            "cluster topology",
            cluster_topology.as_str(),
            "cache_topology_operands",
        ),
        (
            "cluster topology",
            cluster_topology.as_str(),
            "distributed_topology_operands",
        ),
        ("control txn", control_txn.as_str(), "NoopTap"),
        ("control txn", control_txn.as_str(), "trait Tap"),
        ("control txn", control_txn.as_str(), "impl Tap"),
        ("control txn", control_txn.as_str(), ".begin(&mut"),
        ("control txn", control_txn.as_str(), ".ack(&mut"),
        ("control txn", control_txn.as_str(), ".commit(&mut"),
    ] {
        assert!(
            !source.contains(forbidden),
            "{name} must not retain production test/debug-only helper residue: {forbidden}"
        );
    }

    for path in [
        "src/control/automaton/topology.rs",
        "src/control/lease/graph.rs",
        "src/control/lease/graph/tests.rs",
    ] {
        assert!(
            !repo_file_exists(path),
            "{path} must not remain as a test-only control authority"
        );
    }

    assert!(
        !repo_file_exists("src/control/lease/bundle.rs")
            && !repo_file_exists("src/control/lease/bundle/tests.rs"),
        "lease bundle was only a test/measurement context and must stay deleted"
    );
    assert!(
        !repo_file_exists("src/control/cluster/core/topology_state/cache.rs")
            && !repo_file_exists("src/control/cluster/core/topology_state/tests.rs"),
        "cached topology operands were only a test-side authority and must stay deleted"
    );
}

#[test]
fn capability_mint_does_not_retain_test_only_endpoint_identity_api() {
    let source = capability_token_source();
    for forbidden in [
        "pub(crate) use crate::global::const_dsl::ControlScopeKind",
        "EndpointHandle",
        "EndpointResource",
        "endpoint_identity(",
        "endpoint_header(",
        "raw_header(",
        "fn handle(&self)",
        "const fn handle(&self)",
    ] {
        assert!(
            !source.contains(forbidden),
            "production capability minting must not retain test-only endpoint identity API: {forbidden}"
        );
    }
}

#[test]
fn production_cap_codecs_do_not_retain_test_only_encode_fixtures() {
    let source =
        read("src/control/cap/resource_kinds.rs") + &read("src/control/cap/atomic_codecs.rs");
    for forbidden in [
        "fn bytes_are_zero",
        "LoopDecisionHandle::decode",
        "pub(crate) fn encode_session_lane_handle",
        "pub(crate) const fn mint_session_lane_handle",
        "TAG_STATE_SNAPSHOT_CONTROL",
        "TAG_TOPOLOGY_BEGIN_CONTROL",
    ] {
        assert!(
            !source.contains(forbidden),
            "production control capability codecs must not retain test-only encode fixtures: {forbidden}"
        );
    }
    let atomic_codecs = read("src/control/cap/atomic_codecs.rs");
    assert!(
        !atomic_codecs.contains("pub(crate) fn encode(self) -> [u8; CAP_HANDLE_LEN]"),
        "TopologyHandle must not retain its test-only reverse encoder in production atomic codecs"
    );
}
