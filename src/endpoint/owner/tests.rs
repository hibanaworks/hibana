#[path = "../../../tests/common/mod.rs"]
mod common;

use super::EndpointOwner;
use crate::endpoint::carrier::EndpointHandle;
use crate::{
    Endpoint,
    g::{self, Msg},
    runtime::{
        SessionKitStorage,
        ids::SessionId,
        program::{RoleProgram, project},
    },
};
use common::TestTransport;
use core::{future::Future, pin::pin, task::Context};

static_assertions::assert_not_impl_any!(EndpointOwner<'static>: Clone, Copy, Send, Sync);
static_assertions::assert_not_impl_any!(Endpoint<'static, 0>: Clone, Copy, Send, Sync);

#[test]
fn foreign_role_or_generation_cannot_release_a_live_endpoint() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let protocol = g::send::<0, 1, Msg<1, u32>>();
        let client_program: RoleProgram<0> = project(&protocol);
        let server_program: RoleProgram<1> = project(&protocol);
        let mut slab = [0_u8; 16 * 1024];
        let mut storage = SessionKitStorage::<TestTransport>::uninit();
        let kit = storage.init();
        let rendezvous = kit
            .rendezvous(&mut slab, TestTransport::new())
            .expect("rendezvous");
        let session = SessionId::new(815);
        let client = rendezvous.enter(session, &client_program).expect("client");
        let mut server = rendezvous.enter(session, &server_program).expect("server");
        let original = client.owner.handle;
        let wrong_role = EndpointHandle::new(original.generation(), 1);
        let wrong_generation = EndpointHandle::new(
            original
                .generation()
                .checked_add(1)
                .expect("fresh generation"),
            0,
        );
        // SAFETY: the header belongs to the live client. Copy the immutable
        // callback before testing rejection so no header borrow spans a call.
        let release = unsafe { client.owner.ptr.as_ref().ops().drop_endpoint };
        for invalid in [wrong_role, wrong_generation] {
            // SAFETY: this is a live carrier pointer with no operation in
            // flight. An invalid role/generation must be rejected before any
            // mutation or destruction; the client remains the sole owner.
            unsafe { release(client.owner.ptr.cast(), invalid) };
        }
        let Endpoint { mut owner } = client;
        owner.handle = wrong_role;
        let Err(mut owner) = owner.into_endpoint::<0>() else {
            panic!("a mismatched handle cannot restore a typed endpoint");
        };
        assert!(rendezvous.enter(session, &client_program).is_err());
        owner.handle = original;
        let Ok(mut client) = owner.into_endpoint::<0>() else {
            panic!("the original owner must retain the same continuation");
        };
        client.send::<Msg<1, u32>>(&7).await.expect("request");
        assert_eq!(server.recv::<Msg<1, u32>>().await.expect("request"), 7);
    });
}

// These tests use the in-memory carrier and need task scheduling, not OS I/O.
// Keep the same Tokio runtime in native and Miri runs without opening kqueue.
#[test]
fn moving_owner_preserves_committed_cursor_and_live_lease() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let protocol = g::seq(
            g::send::<0, 1, Msg<1, u32>>(),
            g::seq(
                g::send::<1, 0, Msg<2, u32>>(),
                g::send::<0, 1, Msg<3, u32>>(),
            ),
        );
        let client_program: RoleProgram<0> = project(&protocol);
        let server_program: RoleProgram<1> = project(&protocol);
        let mut slab = [0_u8; 16 * 1024];
        let mut storage = SessionKitStorage::<TestTransport>::uninit();
        let kit = storage.init();
        let rendezvous = kit
            .rendezvous(&mut slab, TestTransport::new())
            .expect("rendezvous");
        let session = SessionId::new(810);
        let mut client = rendezvous.enter(session, &client_program).expect("client");
        let mut server = rendezvous.enter(session, &server_program).expect("server");

        client.send::<Msg<1, u32>>(&7).await.expect("request");
        assert_eq!(server.recv::<Msg<1, u32>>().await.expect("request"), 7);
        server.send::<Msg<2, u32>>(&8).await.expect("reply");

        let original_header = client.owner.ptr;
        let original_generation = client.owner.handle.generation();
        let Endpoint { owner } = client;
        assert!(
            rendezvous.enter(session, &client_program).is_err(),
            "moving the owner must retain the original attachment lease"
        );
        let Ok(mut client) = owner.into_endpoint::<0>() else {
            panic!("same logical role must retain its endpoint");
        };
        assert_eq!(client.owner.ptr, original_header);
        assert_eq!(client.owner.handle.generation(), original_generation);
        assert_eq!(client.recv::<Msg<2, u32>>().await.expect("reply"), 8);
        client.send::<Msg<3, u32>>(&9).await.expect("continuation");
        assert_eq!(server.recv::<Msg<3, u32>>().await.expect("continuation"), 9);
    });
}

#[test]
fn wrong_role_returns_the_same_unconsumed_owner() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let protocol = g::send::<0, 1, Msg<1, u32>>();
        let client_program: RoleProgram<0> = project(&protocol);
        let server_program: RoleProgram<1> = project(&protocol);
        let mut slab = [0_u8; 16 * 1024];
        let mut storage = SessionKitStorage::<TestTransport>::uninit();
        let kit = storage.init();
        let rendezvous = kit
            .rendezvous(&mut slab, TestTransport::new())
            .expect("rendezvous");
        let session = SessionId::new(811);
        let client = rendezvous.enter(session, &client_program).expect("client");
        let mut server = rendezvous.enter(session, &server_program).expect("server");

        let original_header = client.owner.ptr;
        let Endpoint { owner } = client;
        let Err(owner) = owner.into_endpoint::<1>() else {
            panic!("a physical holder change must not change the logical role");
        };
        assert_eq!(owner.ptr, original_header);
        assert!(rendezvous.enter(session, &client_program).is_err());
        let Ok(mut client) = owner.into_endpoint::<0>() else {
            panic!("rejected restoration must preserve ownership");
        };
        client.send::<Msg<1, u32>>(&37).await.expect("request");
        assert_eq!(server.recv::<Msg<1, u32>>().await.expect("request"), 37);
    });
}

#[test]
fn moving_owner_preserves_unconsumed_route_frame() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let protocol = g::route(
            g::seq(
                g::send::<0, 1, Msg<1, u32>>(),
                g::send::<1, 0, Msg<3, u32>>(),
            ),
            g::seq(
                g::send::<0, 1, Msg<2, u32>>(),
                g::send::<1, 0, Msg<4, u32>>(),
            ),
        );
        let client_program: RoleProgram<0> = project(&protocol);
        let server_program: RoleProgram<1> = project(&protocol);
        let mut slab = [0_u8; 16 * 1024];
        let mut storage = SessionKitStorage::<TestTransport>::uninit();
        let kit = storage.init();
        let rendezvous = kit
            .rendezvous(&mut slab, TestTransport::new())
            .expect("rendezvous");
        let session = SessionId::new(812);
        let mut client = rendezvous.enter(session, &client_program).expect("client");
        let mut server = rendezvous.enter(session, &server_program).expect("server");

        client.send::<Msg<2, u32>>(&41).await.expect("right branch");
        let preview = server.offer().await.expect("receive preview");
        assert_eq!(preview.label(), 2);
        drop(preview);

        let Endpoint { owner } = server;
        let Ok(mut server) = owner.into_endpoint::<1>() else {
            panic!("same logical role must restore");
        };
        let preview = server.offer().await.expect("same receive preview");
        assert_eq!(preview.label(), 2);
        assert_eq!(preview.recv::<Msg<2, u32>>().await.expect("same frame"), 41);
        server
            .send::<Msg<4, u32>>(&42)
            .await
            .expect("right continuation");
        assert_eq!(client.recv::<Msg<4, u32>>().await.expect("reply"), 42);
    });
}

#[test]
fn moving_owner_preserves_independent_repetition_cursors() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let protocol = g::par(
            g::seq(
                g::send::<0, 1, Msg<1, u32>>(),
                g::send::<1, 0, Msg<2, u32>>(),
            )
            .roll(),
            g::seq(
                g::send::<0, 2, Msg<3, u32>>(),
                g::send::<2, 0, Msg<4, u32>>(),
            )
            .roll(),
        );
        let client_program: RoleProgram<0> = project(&protocol);
        let left_program: RoleProgram<1> = project(&protocol);
        let right_program: RoleProgram<2> = project(&protocol);
        let mut slab = [0_u8; 16 * 1024];
        let mut storage = SessionKitStorage::<TestTransport>::uninit();
        let kit = storage.init();
        let rendezvous = kit
            .rendezvous(&mut slab, TestTransport::new())
            .expect("rendezvous");
        let session = SessionId::new(814);
        let mut client = rendezvous.enter(session, &client_program).expect("client");
        let mut left = rendezvous.enter(session, &left_program).expect("left");
        let mut right = rendezvous.enter(session, &right_program).expect("right");

        client.send::<Msg<1, u32>>(&7).await.expect("left request");
        client
            .send::<Msg<3, u32>>(&11)
            .await
            .expect("right request");
        assert_eq!(left.recv::<Msg<1, u32>>().await.expect("left request"), 7);
        left.send::<Msg<2, u32>>(&8).await.expect("left reply");
        assert_eq!(client.recv::<Msg<2, u32>>().await.expect("left reply"), 8);

        let Endpoint { owner } = client;
        let Ok(mut client) = owner.into_endpoint::<0>() else {
            panic!("same logical role must restore");
        };
        client
            .send::<Msg<1, u32>>(&9)
            .await
            .expect("next left visit");
        assert_eq!(
            left.recv::<Msg<1, u32>>().await.expect("next left visit"),
            9
        );
        assert_eq!(
            right.recv::<Msg<3, u32>>().await.expect("right request"),
            11
        );
        right.send::<Msg<4, u32>>(&12).await.expect("right reply");
        assert_eq!(client.recv::<Msg<4, u32>>().await.expect("right reply"), 12);
        left.send::<Msg<2, u32>>(&10)
            .await
            .expect("next left reply");
        assert_eq!(
            client.recv::<Msg<2, u32>>().await.expect("next left reply"),
            10
        );
    });
}

#[test]
fn dropping_owner_keeps_peer_cancellation_observable() {
    let protocol = g::send::<0, 1, Msg<1, u32>>();
    let client_program: RoleProgram<0> = project(&protocol);
    let server_program: RoleProgram<1> = project(&protocol);
    let mut slab = [0_u8; 16 * 1024];
    let mut storage = SessionKitStorage::<TestTransport>::uninit();
    let kit = storage.init();
    let rendezvous = kit
        .rendezvous(&mut slab, TestTransport::new())
        .expect("rendezvous");
    let session = SessionId::new(813);
    let client = rendezvous.enter(session, &client_program).expect("client");
    let mut server = rendezvous.enter(session, &server_program).expect("server");
    let mut receive = pin!(server.recv::<Msg<1, u32>>());
    let mut context = Context::from_waker(core::task::Waker::noop());
    assert!(receive.as_mut().poll(&mut context).is_pending());

    let Endpoint { owner } = client;
    assert!(receive.as_mut().poll(&mut context).is_pending());
    drop(owner);
    let core::task::Poll::Ready(Err(error)) = receive.as_mut().poll(&mut context) else {
        panic!("owner drop must remain observable to the peer");
    };
    assert!(std::format!("{error:?}").contains("EndpointDropped"));
}

#[test]
fn erased_owner_uses_the_existing_endpoint_budget() {
    assert_eq!(
        core::mem::size_of::<EndpointOwner<'static>>(),
        core::mem::size_of::<Endpoint<'static, 0>>(),
    );
    assert!(core::mem::size_of::<EndpointOwner<'static>>() <= 2 * core::mem::size_of::<usize>(),);
}
