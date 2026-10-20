use super::ResidentActor;
use crate::{
    g::{self, Msg},
    runtime::{SessionKitStorage, ids::SessionId, program::project},
    test_support::transport::TestTransport,
};

#[test]
fn failed_initial_attachment_does_not_consume_the_physical_resource_binding() {
    type Loan<'r> = g::Msg<1, crate::Endpoint<'r, 10>>;
    let global = g::seq(g::send::<0, 1, Loan<'_>>(), g::send::<1, 0, Loan<'_>>());
    let owner_program = project::<0>(&global);
    let resource_program = project::<10>(&global);
    let mut slab = [0u8; 32 * 1024];
    let mut storage = SessionKitStorage::<TestTransport>::uninit();
    let kit = storage.init();
    let rv = kit.rendezvous(&mut slab, TestTransport::new()).unwrap();
    let owner = rv.enter(SessionId::new(843), &owner_program).unwrap();
    // SAFETY: this live owner pins the TestTransport actor. The explicit
    // operation lease protects the short progress borrow used to force an
    // unpublished issuance failure, without any transport operation.
    let actor = unsafe {
        owner
            .owner
            .ptr
            .as_ref()
            .actor()
            .cast::<ResidentActor<'_, TestTransport>>()
            .as_ref()
    };
    let lease = actor.context.try_operation_lease().unwrap();
    let progress = actor.progress.borrow_mut();
    assert!(rv.enter(SessionId::new(843), &resource_program).is_err());
    drop(progress);
    drop(lease);
    // The actor exists, but failure after reservation did not publish a view.
    // The RAII reservation must return the same initial authority.
    let db = rv.enter(SessionId::new(843), &resource_program).unwrap();
    drop(db);
}

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
            .as_ref()
            .actor()
            .cast::<ResidentActor<'_, TestTransport>>()
            .as_ref()
    };
    // SAFETY: peer is the live owner of this distinct actor allocation.
    let other = unsafe {
        peer.owner
            .ptr
            .as_ref()
            .actor()
            .cast::<ResidentActor<'_, TestTransport>>()
            .as_ref()
    };
    let mut lease = actor.context.try_operation_lease().expect("operation");
    let rejected = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        other.with_kernel(
            &mut lease,
            other.views.get(1).unwrap(),
            crate::endpoint::carrier::OperationRegion::WHOLE,
            |_| (),
        );
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
            .as_ref()
            .actor()
            .cast::<ResidentActor<'_, TestTransport>>()
            .as_ref()
    };
    let mut operation = actor.context.try_operation_lease().expect("operation");
    actor.with_kernel(
        &mut operation,
        actor.views.get(0).unwrap(),
        crate::endpoint::carrier::OperationRegion::WHOLE,
        |kernel| {
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
        },
    );
    drop(operation);
    drop(client);
    drop(peer);
}
