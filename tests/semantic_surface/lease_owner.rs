use super::common::*;

#[test]
fn intrusive_rendezvous_registry_replaces_fixed_array_map() {
    let lease_core = read("src/session/lease/core.rs");
    let registry_ops = read("src/session/lease/core/registry_ops.rs");

    assert!(
        !repo_file_exists("src/session/lease/map.rs")
            && !lease_core.contains("ArrayMap")
            && !registry_ops.contains("ArrayMap"),
        "fixed ArrayMap rendezvous registry must stay deleted"
    );
    assert!(
        lease_core.contains("head: Option<NonNull<RendezvousEntry<'cfg, T>>>")
            && lease_core.contains("next: Option<NonNull<RendezvousEntry<'cfg, T>>>")
            && registry_ops.contains("Rendezvous::init_in_slab_auto(id, resources, transport)")
            && registry_ops.contains("allocate_external_persistent_sidecar_bytes")
            && registry_ops.contains("RendezvousEntry::init_from_parts("),
        "rendezvous registry must stay intrusive and slab-resident"
    );
    let init_pos = registry_ops
        .find("RendezvousEntry::init_from_parts(")
        .expect("registry entry initialization must stay explicit");
    let link_pos = registry_ops
        .find("self.head = NonNull::new(entry_ptr);")
        .expect("registry entry must publish by linking at the head");
    assert!(
        init_pos < link_pos,
        "registry entry must be fully initialized before it is linked"
    );
    assert!(
        registry_ops.contains("ptr::drop_in_place(rendezvous);")
            && registry_ops.contains("return Err(RegisterRendezvousError::StorageExhausted);"),
        "entry allocation failure must drop the unlinked rendezvous before returning"
    );
    assert!(
        !registry_ops.contains("self.head = NonNull::new(entry_ptr);\n        unsafe")
            && !registry_ops.contains("self.len = self.len.wrapping_add"),
        "registry publication must not link before initialization or use wrapping length growth"
    );
}
