use super::EndpointHandle;
use crate::test_support::transport::TestTransport;
use core::{future::Future, pin::pin, task::Context};

use crate::{
    g::{self, Msg},
    runtime::{SessionKit, SessionKitStorage, ids::SessionId, program::project},
};

#[test]
fn reentrant_carrier_entry_is_rejected_before_mutable_actor_borrow() {
    let protocol = g::send::<0, 1, Msg<1, u32>>();
    let role0 = project::<0>(&protocol);
    let mut slab = [0; 16 * 1024];
    let mut storage = SessionKitStorage::<TestTransport>::uninit();
    let kit = storage.init();
    let rendezvous = kit
        .rendezvous(&mut slab, TestTransport::new())
        .expect("rendezvous");
    let client = rendezvous
        .enter(SessionId::new(901), &role0)
        .expect("client");
    let ptr = client.owner.ptr.cast();
    let handle = client.owner.handle;
    let tag = crate::endpoint::proof_snapshot::view_tag(&client);
    let mut nested_entered = false;
    // SAFETY: the live client is the only public owner. This deliberately
    // reenters the raw carrier to test its aliasing boundary, while leaving
    // the outer cursor borrow live across the rejected nested invocation.
    let outer_entered = unsafe {
        SessionKit::<TestTransport>::with_public_endpoint_mut(ptr, handle, false, |kernel| {
            // SAFETY: this test created the live pinned actor with TestTransport.
            let cut = tag.actor_call_cut::<TestTransport>();
            assert!(!cut.lease_available);
            assert_eq!(cut.fault, None);
            let cursor = &mut kernel.progress.cursor;
            let before = cursor.index();
            let nested_result =
                SessionKit::<TestTransport>::with_public_endpoint_mut(ptr, handle, false, |_| {
                    nested_entered = true;
                    true
                });
            assert!(!nested_result);
            assert_eq!(cursor.index(), before);
            true
        })
    };
    assert!(outer_entered);
    assert!(!nested_entered);
    // SAFETY: the same actor remains pinned and the client is still live.
    let cut = unsafe { tag.actor_call_cut::<TestTransport>() };
    assert!(cut.lease_available);
    assert_eq!(cut.fault, None);
    // The rejected entry did not hold a lease or retire the live generation.
    let header = unsafe {
        // SAFETY: the affine client still owns this pinned immutable header.
        client.owner.ptr.as_ref()
    };
    assert!(
        EndpointHandle::new(handle.generation(), handle.view_generation()).matches_header(header)
    );
    // SAFETY: the header belongs to the client's live pinned actor.
    let actor = unsafe {
        client
            .owner
            .ptr
            .as_ref()
            .actor()
            .cast::<crate::endpoint::kernel::ResidentActor<'_, TestTransport>>()
            .as_ref()
    };
    let lease = actor
        .context
        .try_operation_lease()
        .expect("outer lease released");
    drop(lease);
    drop(client);
}

#[test]
fn pending_receive_keeps_its_io_but_releases_the_actor_borrow() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let protocol = g::send::<0, 1, Msg<1, u32>>();
        let role0 = project::<0>(&protocol);
        let role1 = project::<1>(&protocol);
        let mut slab = [0; 16 * 1024];
        let mut storage = SessionKitStorage::<TestTransport>::uninit();
        let kit = storage.init();
        let rendezvous = kit
            .rendezvous(&mut slab, TestTransport::new())
            .expect("rendezvous");
        let sid = SessionId::new(902);
        let mut client = rendezvous.enter(sid, &role0).expect("client");
        let mut server = rendezvous.enter(sid, &role1).expect("server");
        let server_header = server.owner.ptr;
        let mut receive = pin!(server.recv::<Msg<1, u32>>());
        let mut cx = Context::from_waker(futures::task::noop_waker_ref());
        assert!(receive.as_mut().poll(&mut cx).is_pending());
        let lease = unsafe {
            // SAFETY: the public receive future retains its unique server
            // owner. Only immutable header metadata is read across Pending.
            server_header
                .as_ref()
                .actor()
                .cast::<crate::endpoint::kernel::ResidentActor<'_, TestTransport>>()
                .as_ref()
        }
        .context
        .try_operation_lease()
        .expect("Pending released the shared mutation lease");
        drop(lease);
        client.send::<Msg<1, u32>>(&37).await.expect("send");
        assert_eq!(receive.await.expect("receive"), 37);
    });
}
