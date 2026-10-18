//! Deliberate contract violation: FIFO alone cannot exclude a replay in roll.
//! This is a boundary counterexample, not a conforming transport execution or
//! an endorsement of accepting duplicate delegation frames in an integration.
use super::*;
use crate::endpoint::proof_snapshot::{actor_fault, view_tag};
use crate::runtime::{SessionKitStorage, ids::SessionId};
use crate::test_support::transport::TestTransport;
use crate::transport::{PortOpen, Transport};

#[test]
fn replaying_an_owned_frame_breaks_uniqueness_without_affine_delivery() {
    type Steps =
        g::Seq<g::Roll<g::Seq<g::Send<0, 1, Loan>, g::Send<1, 0, Loan>>>, g::Send<10, 2, Read>>;
    let () = g::ProgramProjection::<Steps, 8>::VALIDATION;
    let owner_program = crate::global::role_program::role_program_from_image::<0>(
        &RoleProjection::<0, Steps, 8>::IMAGE_REF,
    );
    let worker_program = crate::global::role_program::role_program_from_image::<1>(
        &RoleProjection::<1, Steps, 8>::IMAGE_REF,
    );
    let db_program = crate::global::role_program::role_program_from_image::<10>(
        &RoleProjection::<10, Steps, 8>::IMAGE_REF,
    );
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async {
            let network = TestTransport::new();
            let mut owner_slab = [0u8; 32 * 1024];
            let mut worker_slab = [0u8; 32 * 1024];
            let mut owner_storage = SessionKitStorage::uninit();
            let mut worker_storage = SessionKitStorage::uninit();
            let owner_rv = owner_storage
                .init()
                .rendezvous(&mut owner_slab, network.clone())
                .unwrap();
            let worker_rv = worker_storage
                .init()
                .rendezvous(&mut worker_slab, network.clone())
                .unwrap();
            let sid = SessionId::new(9055);
            let mut owner = owner_rv.enter(sid, &owner_program).unwrap();
            let mut worker = worker_rv.enter(sid, &worker_program).unwrap();
            let db = owner_rv.enter(sid, &db_program).unwrap();

            owner.send::<Loan>(db).await.unwrap();
            let (header, payload) = network.queued_frame(1);
            // Intentionally violate no-replay while preserving FIFO: append
            // the same already-accepted bytes, without a second Endpoint move.
            let (mut injected, _) = network.open(PortOpen::from_descriptor(
                header.source_role(),
                sid,
                crate::session::types::Lane::new(u32::from(header.lane())),
            ));
            network.stage_send(
                &mut injected,
                header.target_role(),
                header.lane(),
                header.label().raw(),
                &payload,
            );
            assert!(matches!(
                network.poll_send_staged(&mut injected),
                core::task::Poll::Ready(Ok(()))
            ));

            let db = worker.recv::<Loan>().await.unwrap();
            worker.send::<Loan>(db).await.unwrap();
            let db = owner.recv::<Loan>().await.unwrap();
            let replayed = worker.recv::<Loan>().await.unwrap();
            let owner_tag = view_tag(&db);
            let worker_tag = view_tag(&replayed);
            // SAFETY: both actual values and their separate pinned runtimes
            // remain alive. These tags only observe headers, never authorize
            // another resource operation or drive the localsides.
            unsafe {
                assert!(owner_tag.matches_header());
                assert!(worker_tag.matches_header());
                assert_eq!(actor_fault(&owner), None);
                assert_eq!(actor_fault(&worker), None);
            }
            assert!(network.queue_is_empty());

            owner.send::<Loan>(db).await.unwrap();
            assert_eq!(network.queued_frame(1), (header, payload));
            // The genuine next visit has exactly the old envelope. It is
            // still queued although the worker already accepted the replay.
            assert!(!network.queue_is_empty());
            drop(replayed);
        });
}
