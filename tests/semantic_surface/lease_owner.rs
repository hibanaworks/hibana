use super::common::*;

#[test]
fn intrusive_rendezvous_registry_replaces_fixed_array_map() {
    let lease_core = read("src/session/lease/core.rs");
    let registry_ops = read("src/session/lease/core/registry_ops.rs");
    let rendezvous_core = read("src/rendezvous/core.rs");

    assert!(
        !repo_file_exists("src/session/lease/map.rs")
            && !lease_core.contains("ArrayMap")
            && !registry_ops.contains("ArrayMap"),
        "fixed ArrayMap rendezvous registry must stay deleted"
    );
    assert!(
        lease_core.contains("head: Cell<Option<NonNull<Rendezvous<'cfg, 'cfg, T>>>>")
            && rendezvous_core
                .contains("registry_next: Cell<Option<NonNull<Rendezvous<'rv, 'cfg, T>>>>")
            && rendezvous_core.contains(
                "resolver_bucket: UnsafeCell<crate::session::cluster::core::ResolverBucket<'cfg>>"
            )
            && registry_ops.contains("Rendezvous::init_in_slab_auto(id, resources, transport)")
            && registry_ops.contains("rendezvous_ref.link_registry_next(self.head.get());")
            && registry_ops.contains("self.head.set(Some(rendezvous_ptr));")
            && !lease_core.contains("RendezvousEntry")
            && !registry_ops.contains("allocate_external_persistent_sidecar_bytes"),
        "rendezvous header itself must remain the sole slab-resident registry node and resolver owner"
    );
    let init_pos = registry_ops
        .find("Rendezvous::init_in_slab_auto(id, resources, transport)")
        .expect("rendezvous initialization must stay explicit");
    let link_pos = registry_ops
        .find("rendezvous_ref.link_registry_next(self.head.get());")
        .expect("rendezvous must link its next owner before publication");
    let publish_pos = registry_ops
        .find("self.head.set(Some(rendezvous_ptr));")
        .expect("registry must publish the initialized rendezvous at its head");
    assert!(
        init_pos < link_pos && link_pos < publish_pos,
        "rendezvous must be fully initialized and linked before head publication"
    );
    assert!(
        registry_ops.contains("ptr::drop_in_place(rendezvous);")
            && registry_ops.contains(".next_available_rendezvous_id()")
            && registry_ops.contains(".ok_or(RegisterRendezvousError::CapacityExceeded)?")
            && !lease_core.contains("len: Cell<u16>")
            && !registry_ops.contains("RendezvousEntry"),
        "registry capacity must derive only from available ids and drop intrusive rendezvous headers directly"
    );
    assert!(
        !registry_ops.contains("self.len = self.len.wrapping_add")
            && !registry_ops.contains("self.len.set(")
            && !registry_ops.contains("self.head.set(Some(rendezvous_ptr));\n        unsafe"),
        "registry publication must not retain a second length authority or mutate after publication"
    );
}

#[test]
fn unpublished_endpoint_failures_abort_without_consuming_generation() {
    let endpoint_leases = read("src/rendezvous/core/endpoint_leases.rs");
    let session_ops = read("src/session/cluster/core/session_cluster_ops.rs");
    let endpoint_attach = read("src/session/cluster/core/endpoint_attach.rs");
    let allocation = session_ops
        .split("pub(in crate::session::cluster::core) fn allocate_public_endpoint_storage_for_rv")
        .nth(1)
        .and_then(|tail| {
            tail.split("pub(crate) fn public_endpoint_header_ptr")
                .next()
        })
        .expect("public endpoint allocation owner must stay visible");
    let attach = endpoint_attach
        .split("fn attach_public_endpoint_inner")
        .nth(1)
        .and_then(|tail| tail.split("pub(crate) fn enter").next())
        .expect("endpoint attach owner must stay visible");

    assert!(endpoint_leases.contains("fn abort_endpoint_lease_reservation"));
    assert!(endpoint_leases.contains("slot.state != EndpointLeaseState::Reserved"));
    assert!(endpoint_leases.contains("self.endpoint_lease_generation.get() != generation"));
    assert!(endpoint_leases.contains("generation.checked_sub(1)"));
    assert_eq!(
        allocation
            .matches("rv.abort_endpoint_lease_reservation(slot, generation);")
            .count(),
        2,
        "both resident-capacity failures must abort the reservation"
    );
    assert!(!allocation.contains("rv.release_endpoint_lease(slot, generation);"));
    assert!(attach.contains(".abort_endpoint_lease_reservation(slot, generation);"));
    assert!(!attach.contains(".release_endpoint_lease(slot, generation);"));
}
