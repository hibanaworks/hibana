#![cfg(feature = "std")]

#[path = "semantic_surface/common.rs"]
mod common;
#[path = "semantic_surface/descriptor_measurement.rs"]
mod descriptor_measurement;
#[path = "semantic_surface/endpoint_active_leases.rs"]
mod endpoint_active_leases;
#[path = "semantic_surface/endpoint_runtime.rs"]
mod endpoint_runtime;
#[path = "semantic_surface/lease_owner.rs"]
mod lease_owner;
#[path = "semantic_surface/public_docs.rs"]
mod public_docs;
#[path = "semantic_surface/send_commit.rs"]
mod send_commit;
#[path = "semantic_surface/send_commit_wire.rs"]
mod send_commit_wire;
#[path = "semantic_surface/source_residue.rs"]
mod source_residue;
#[path = "semantic_surface/source_residue_commit.rs"]
mod source_residue_commit;
#[path = "semantic_surface/source_residue_errors.rs"]
mod source_residue_errors;
#[path = "semantic_surface/source_residue_hygiene.rs"]
mod source_residue_hygiene;
#[path = "semantic_surface/source_residue_route_arm_lane.rs"]
mod source_residue_route_arm_lane;
#[path = "semantic_surface/source_residue_support.rs"]
mod source_residue_support;
#[path = "semantic_surface/source_residue_test_only.rs"]
mod source_residue_test_only;
#[path = "semantic_surface/transport_topology.rs"]
mod transport_topology;
