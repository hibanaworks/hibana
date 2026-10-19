//! A peer can finish while the original send still awaits its TCP receipt.
use super::super::{Loan, Read, RoleProjection, Value, g, preemption};
use super::{Broker, NetworkTransport, Process, inbox};
use crate::{
    endpoint::proof_snapshot::view_tag,
    runtime::{SessionKitStorage, ids::SessionId},
};
use core::pin::pin;
use std::{net::SocketAddr, process::Command, string::ToString, time::Duration, vec};

const SID: SessionId = SessionId::new(9023);
const BROKER: &str = "g::role_projection::tests::native::network::preemption::broker_process";
const WORKER: &str = "g::role_projection::tests::native::network::preemption::worker_process";
const PEER: &str =
    "g::role_projection::tests::native::network::preemption::independent_peer_process";
type Independent = g::Msg<93, u32>;
type Global = g::Par<preemption::Global, g::Send<0, 2, Independent>>;

mod publication;

#[tokio::test(flavor = "current_thread")]
async fn a_tcp_receiver_returns_the_resource_while_the_send_is_pending_between_polls() {
    let () = g::ProgramProjection::<Global, 16>::VALIDATION;
    let owner_program = crate::global::role_program::role_program_from_image::<0>(
        &RoleProjection::<0, Global, 16>::IMAGE_REF,
    );
    let db_program = crate::global::role_program::role_program_from_image::<10>(
        &RoleProjection::<10, Global, 16>::IMAGE_REF,
    );
    // SAFETY: this namespace has one issuer. Its children and inbox are fenced
    // before the binding, including while unwinding.
    let binding = unsafe { crate::runtime::transport::EndpointBinding::new() };
    let broker = Broker::start(BROKER).await;
    let mut child = Process(
        Command::new(std::env::current_exe().unwrap())
            .args(["--ignored", "--exact", WORKER, "--nocapture"])
            .env("HIBANA_NETWORK_ADDRESS", broker.address.to_string())
            .spawn()
            .unwrap(),
    );
    let mut peer = Process(
        Command::new(std::env::current_exe().unwrap())
            .args(["--ignored", "--exact", PEER, "--nocapture"])
            .env("HIBANA_NETWORK_ADDRESS", broker.address.to_string())
            .spawn()
            .unwrap(),
    );
    let observations;
    let operation_observations;
    let pending_journal;
    {
        let mut slab = vec![0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(
                &mut slab,
                NetworkTransport::new(broker.address, Some((10, &binding))),
            )
            .unwrap();
        let mut owner = rv.enter(SID, &owner_program).unwrap();
        let db = rv.enter(SID, &db_program).unwrap();
        let tag = view_tag(&db);
        let owner_tag = view_tag(&owner);
        // SAFETY: this tag retains the pinned NetworkTransport actor. The
        // snapshots only observe calls; they never decide an operation.
        let before = unsafe { owner_tag.snapshot::<NetworkTransport<'_>>() };
        let [mut resources, mut independent] = owner.par().unwrap();
        let independent_tag = independent.proof_tag();
        {
            let mut transfer = pin!(resources.send::<Loan>(db));
            tokio::select! {
                result = &mut transfer => panic!("delayed acknowledgement returned before the peer finished: {result:?}"),
                () = child.wait_success() => {}
            }
            // The peer has used and returned DB, yet the original send future
            // has not returned. This is outside its synchronous kernel lease.
            // SAFETY: storage retains this exact pinned NetworkTransport actor.
            assert!(unsafe { tag.matches_header() });
            // SAFETY: the same pinned allocation uses NetworkTransport.
            let cut = unsafe { tag.actor_call_cut::<NetworkTransport<'_>>() };
            assert!(cut.lease_available);
            assert_eq!(cut.fault, None);
            // SAFETY: the same actor is alive, with no borrowed kernel poll.
            assert!(unsafe { tag.is_send_payload::<NetworkTransport<'_>>() });

            // A different fragment of this very actor commits while the
            // accepted owned send still retains its payload between polls.
            // SAFETY: the same pinned actor is idle between the future polls.
            let pending_cut = unsafe { independent_tag.operation_cut::<NetworkTransport<'_>>() };
            let prepared = pending_cut
                .operations
                .iter()
                .find_map(|slot| (slot.view == 0).then_some(slot.prepared_send).flatten())
                .expect("the actual pending owned future retains its prepared source event");
            assert_eq!(prepared.1, 1);
            assert!(pending_cut.actor.steps.iter().any(|step| step.view == 0
                && matches!(step.action, crate::global::typestate::LocalAction::Send {
                    eff_index, label: 1, lane, ..
                } if eff_index.dense_ordinal() == prepared.0 && lane == prepared.2)));
            let pending = pending_cut.actor.clone();
            independent.send::<Independent>(&77).await.unwrap();
            peer.wait_success().await;
            // SAFETY: both observations use the same pinned sender actor.
            assert!(unsafe { tag.matches_header() });
            assert!(unsafe { tag.is_send_payload::<NetworkTransport<'_>>() });
            // SAFETY: independent's await completed; transfer is not polling.
            let independent_cut =
                unsafe { independent_tag.operation_cut::<NetworkTransport<'_>>() };
            assert!(
                independent_cut
                    .operations
                    .iter()
                    .any(|slot| slot.view == 0 && slot.prepared_send == Some(prepared))
            );
            let independent_done = independent_cut.actor.clone();
            operation_observations = [pending_cut, independent_cut];
            pending_journal = broker.journal().await;
            transfer.await.unwrap();
            // SAFETY: the live owner view and its original actor remain pinned.
            let published = unsafe { owner_tag.snapshot::<NetworkTransport<'_>>() };
            observations = [before, pending, independent_done, published];
        }
        // SAFETY: the resident header remains allocated, now retired.
        assert!(!unsafe { tag.matches_header() });
        let mut db = resources.recv::<Loan>().await.unwrap();
        tokio::join!(
            async {
                db.send::<Read>(&42).await.unwrap();
                assert_eq!(db.recv::<Value>().await.unwrap(), 43);
            },
            async {
                assert_eq!(resources.recv::<Read>().await.unwrap(), 42);
                resources.send::<Value>(&43).await.unwrap();
            },
        );
    }
    publication::write_observations(&observations, &pending_journal, &operation_observations);
    let journal = broker.journal().await;
    assert_eq!(
        journal
            .iter()
            .filter(|(operation, _)| *operation == 1)
            .count(),
        7
    );
    assert_eq!(
        journal
            .iter()
            .filter(|(operation, _)| *operation == 2)
            .count(),
        7
    );
    drop(child);
    drop(peer);
    drop(broker);
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "delayed TCP acknowledgement server exercised by the parent fixture"]
async fn broker_process() {
    inbox::serve(|header| {
        if header.bytes()[5..7] == [0, 1] {
            Duration::from_secs(1)
        } else {
            Duration::ZERO
        }
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "separate peer exercised by the pending-send network fixture"]
async fn worker_process() {
    let address: SocketAddr = std::env::var("HIBANA_NETWORK_ADDRESS")
        .unwrap()
        .parse()
        .unwrap();
    let () = g::ProgramProjection::<Global, 16>::VALIDATION;
    let program = crate::global::role_program::role_program_from_image::<1>(
        &RoleProjection::<1, Global, 16>::IMAGE_REF,
    );
    let mut slab = vec![0; 64 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, NetworkTransport::new(address, None))
        .unwrap();
    let mut worker = rv.enter(SID, &program).unwrap();
    let mut db = worker.recv::<Loan>().await.unwrap();
    tokio::join!(
        async {
            db.send::<Read>(&41).await.unwrap();
            assert_eq!(db.recv::<Value>().await.unwrap(), 42);
        },
        async {
            assert_eq!(worker.recv::<Read>().await.unwrap(), 41);
            worker.send::<Value>(&42).await.unwrap();
        },
    );
    worker.send::<Loan>(db).await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "separate peer exercised by the parallel pending-send fixture"]
async fn independent_peer_process() {
    let address: SocketAddr = std::env::var("HIBANA_NETWORK_ADDRESS")
        .unwrap()
        .parse()
        .unwrap();
    let () = g::ProgramProjection::<Global, 16>::VALIDATION;
    let program = crate::global::role_program::role_program_from_image::<2>(
        &RoleProjection::<2, Global, 16>::IMAGE_REF,
    );
    let mut slab = vec![0; 64 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, NetworkTransport::new(address, None))
        .unwrap();
    let mut peer = rv.enter(SID, &program).unwrap();
    assert_eq!(peer.recv::<Independent>().await.unwrap(), 77);
}
