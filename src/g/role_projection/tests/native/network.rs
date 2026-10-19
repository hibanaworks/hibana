//! The existing owned global over TCP, with independent owner, worker,
//! physical service and byte-inbox processes. No file, queue descriptor,
//! shared Rust memory or EndpointBinding crosses a process boundary. This is
//! a loopback network-inbox fixture, not arbitrary transport qualification.
use super::{Global, Loan, Queued, Read, RoleProjection, Value, g};
use crate::runtime::{SessionKitStorage, ids::SessionId};
use std::{net::SocketAddr, process::Command, string::ToString, time::Duration, vec};

#[path = "../../../../../tests/support/network_inbox.rs"]
mod inbox;
use inbox::{Broker, NetworkTransport, Process};

mod composed;

const SID: SessionId = SessionId::new(9020);
const WORKER: &str = "g::role_projection::tests::native::network::worker_process";
const SERVICE: &str = "g::role_projection::tests::native::network::service_process";
const BROKER: &str = "g::role_projection::tests::native::network::broker_process";

#[tokio::test(flavor = "current_thread")]
async fn tcp_handoff_preserves_an_unread_frame_after_the_sender_process_exits() {
    let () = g::ProgramProjection::<Global, 16>::VALIDATION;
    let owner_program = crate::global::role_program::role_program_from_image::<0>(
        &RoleProjection::<0, Global, 16>::IMAGE_REF,
    );
    let db_program = crate::global::role_program::role_program_from_image::<10>(
        &RoleProjection::<10, Global, 16>::IMAGE_REF,
    );
    let broker = Broker::start(BROKER).await;
    let mut service_process = Process(
        Command::new(std::env::current_exe().unwrap())
            .args(["--ignored", "--exact", SERVICE, "--nocapture"])
            .env("HIBANA_NETWORK_ADDRESS", broker.address.to_string())
            .env("HIBANA_NETWORK_PARENT", std::process::id().to_string())
            .spawn()
            .unwrap(),
    );
    let mut worker_process = Process(
        Command::new(std::env::current_exe().unwrap())
            .args(["--ignored", "--exact", WORKER, "--nocapture"])
            .env("HIBANA_NETWORK_ADDRESS", broker.address.to_string())
            .env("HIBANA_NETWORK_PARENT", std::process::id().to_string())
            .spawn()
            .unwrap(),
    );
    {
        let mut slab = vec![0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, NetworkTransport::new(broker.address, None))
            .unwrap();
        assert!(rv.enter(SID, &db_program).is_err());
        let mut owner = rv.enter(SID, &owner_program).unwrap();
        let db = owner.recv::<Loan>().await.unwrap();
        owner.send::<Loan>(db).await.unwrap();

        // Test scheduling only: the returning process must be gone and the
        // ordinary frame must remain unread in the actual TCP inbox before
        // this receiver commits the return. Observations confer no authority.
        worker_process.wait_success().await;
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let journal = broker.journal().await;
                let queued = journal.iter().find(|(operation, packet)| {
                    *operation == 1 && packet[6] == 10 && packet[8..] == 123u32.to_be_bytes()
                });
                if let Some((_, queued)) = queued {
                    assert!(
                        !journal
                            .iter()
                            .any(|(operation, packet)| *operation == 2 && packet == queued)
                    );
                    break;
                }
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .unwrap();

        let mut db = owner.recv::<Loan>().await.unwrap();
        assert_eq!(db.recv::<Queued>().await.unwrap(), 123);
        db.send::<Read>(&1).await.unwrap();
        assert_eq!(db.recv::<Value>().await.unwrap(), 45);
        owner.send::<Loan>(db).await.unwrap();
    }
    service_process.wait_success().await;
    let journal = broker.journal().await;
    let accepted: std::vec::Vec<_> = journal
        .iter()
        .filter(|(operation, _)| *operation == 1)
        .map(|(_, packet)| packet)
        .collect();
    let received: std::vec::Vec<_> = journal
        .iter()
        .filter(|(operation, _)| *operation == 2)
        .map(|(_, packet)| packet)
        .collect();
    assert_eq!(
        accepted.len(),
        9,
        "every ordinary and owned frame is recorded"
    );
    assert_eq!(
        received.len(),
        accepted.len(),
        "no accepted frame remains unread"
    );
    for packet in accepted {
        assert_eq!(
            received
                .iter()
                .filter(|candidate| **candidate == packet)
                .count(),
            1
        );
    }
    drop(worker_process);
    drop(service_process);
    drop(broker);
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "separate initial issuer and physical service for the network fixture"]
async fn service_process() {
    let parent: u32 = std::env::var("HIBANA_NETWORK_PARENT")
        .unwrap()
        .parse()
        .unwrap();
    assert_ne!(parent, std::process::id());
    let address: SocketAddr = std::env::var("HIBANA_NETWORK_ADDRESS")
        .unwrap()
        .parse()
        .unwrap();
    let () = g::ProgramProjection::<Global, 16>::VALIDATION;
    let service_program = crate::global::role_program::role_program_from_image::<2>(
        &RoleProjection::<2, Global, 16>::IMAGE_REF,
    );
    let db_program = crate::global::role_program::role_program_from_image::<10>(
        &RoleProjection::<10, Global, 16>::IMAGE_REF,
    );
    let mut value = 41u32;
    // SAFETY: this process exclusively owns value and its initial issuer.
    // The final return is received and dropped before the binding ends.
    let binding = unsafe { crate::runtime::transport::EndpointBinding::new() };
    let mut slab = vec![0; 64 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(
            &mut slab,
            NetworkTransport::new(address, Some((10, &binding))),
        )
        .unwrap();
    let mut service = rv.enter(SID, &service_program).unwrap();
    let db = rv.enter(SID, &db_program).unwrap();
    service.send::<Loan>(db).await.unwrap();
    value += service.recv::<Read>().await.unwrap();
    service.send::<Value>(&value).await.unwrap();
    service.send::<Queued>(&123).await.unwrap();
    value += service.recv::<Read>().await.unwrap();
    service.send::<Value>(&value).await.unwrap();
    drop(service.recv::<Loan>().await.unwrap());
    assert_eq!(value, 45);
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "separate process entrypoint exercised by the network parent fixture"]
async fn worker_process() {
    let parent: u32 = std::env::var("HIBANA_NETWORK_PARENT")
        .unwrap()
        .parse()
        .unwrap();
    assert_ne!(parent, std::process::id());
    let address: SocketAddr = std::env::var("HIBANA_NETWORK_ADDRESS")
        .unwrap()
        .parse()
        .unwrap();
    let () = g::ProgramProjection::<Global, 16>::VALIDATION;
    let worker_program = crate::global::role_program::role_program_from_image::<1>(
        &RoleProjection::<1, Global, 16>::IMAGE_REF,
    );
    let db_program = crate::global::role_program::role_program_from_image::<10>(
        &RoleProjection::<10, Global, 16>::IMAGE_REF,
    );
    let mut slab = vec![0; 64 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, NetworkTransport::new(address, None))
        .unwrap();
    assert!(rv.enter(SID, &db_program).is_err());
    let mut worker = rv.enter(SID, &worker_program).unwrap();
    let mut db = worker.recv::<Loan>().await.unwrap();
    db.send::<Read>(&3).await.unwrap();
    assert_eq!(db.recv::<Value>().await.unwrap(), 44);
    worker.send::<Loan>(db).await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "separate byte-inbox server exercised by the network parent fixture"]
async fn broker_process() {
    assert_eq!(std::env::var("HIBANA_NETWORK_BROKER").unwrap(), "1");
    inbox::serve().await;
}
