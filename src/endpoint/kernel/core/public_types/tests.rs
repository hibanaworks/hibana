use super::ResidentActor;
use crate::{
    g::{self, Msg},
    runtime::{SessionKitStorage, ids::SessionId, program::project},
    test_support::transport::TestTransport,
};

#[test]
fn another_actor_in_the_same_rendezvous_cannot_use_the_mutation_lease() {
    let protocol = g::send::<0, 1, Msg<1, u32>>();
    let role0 = project::<0>(&protocol);
    let role1 = project::<1>(&protocol);
    let mut slab = [0; 16 * 1024];
    let mut storage = SessionKitStorage::<TestTransport>::uninit();
    let kit = storage.init();
    let rendezvous = kit
        .rendezvous(&mut slab, TestTransport::new())
        .expect("rendezvous");
    let sid = SessionId::new(904);
    let client = rendezvous.enter(sid, &role0).expect("client");
    let peer = rendezvous.enter(sid, &role1).expect("peer");
    // SAFETY: both affine owners remain live, and no operation has started.
    let actor = unsafe {
        client
            .owner
            .ptr
            .cast::<ResidentActor<'_, TestTransport>>()
            .as_ref()
    };
    // SAFETY: peer is the live owner of this distinct actor allocation.
    let other = unsafe {
        peer.owner
            .ptr
            .cast::<ResidentActor<'_, TestTransport>>()
            .as_ref()
    };
    let mut lease = actor.context.try_operation_lease().expect("operation");
    let rejected = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _kernel = other.borrow_kernel(&mut lease);
    }));
    assert!(
        rejected.is_err(),
        "foreign actor lease must be rejected before kernel borrowing"
    );
    drop(lease);
    drop(client);
    drop(peer);
}

#[test]
fn shared_frontier_workspace_outlives_retired_view_ports() {
    let protocol = g::route(
        g::send::<0, 1, Msg<1, u32>>(),
        g::send::<0, 1, Msg<2, u32>>(),
    );
    let role0 = project::<0>(&protocol);
    let role1 = project::<1>(&protocol);
    let mut slab = [0; 16 * 1024];
    let mut storage = SessionKitStorage::<TestTransport>::uninit();
    let kit = storage.init();
    let rendezvous = kit
        .rendezvous(&mut slab, TestTransport::new())
        .expect("rendezvous");
    let sid = SessionId::new(903);
    let client = rendezvous.enter(sid, &role0).expect("client");
    let peer = rendezvous.enter(sid, &role1).expect("peer");
    // SAFETY: client owns this pinned actor allocation. No public operation is
    // live, and the operation lease below is acquired before borrowing it.
    let actor = unsafe {
        client
            .owner
            .ptr
            .cast::<ResidentActor<'_, TestTransport>>()
            .as_ref()
    };
    let mut operation = actor.context.try_operation_lease().expect("operation");
    {
        let mut kernel = actor.borrow_kernel(&mut operation);
        let mut scratch = kernel
            .context
            .session
            .rendezvous()
            .try_scratch_lease()
            .expect("workspace");
        kernel.retire_transport_handles();
        assert!(kernel.io.ports.iter().all(Option::is_none));
        // This must borrow the common owner, even though this view no longer
        // has a transport port through which to reach its workspace.
        core::hint::black_box(kernel.frontier_scratch_workspace(&mut scratch));
    }
    drop(operation);
    drop(client);
    drop(peer);
}
