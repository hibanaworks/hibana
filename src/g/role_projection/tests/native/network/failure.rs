//! Holder loss on independent TCP processes must not replenish issuance.
use super::super::{Loan, Read, failure::Global, g};
use super::{BROKER, Broker, NetworkTransport, Process};
use crate::{
    endpoint::proof_snapshot::view_tag,
    rendezvous::SessionFaultKind,
    runtime::{SessionKitStorage, ids::SessionId},
};
use std::{
    net::SocketAddr,
    process::Command,
    string::ToString,
    time::{Duration, Instant},
    vec,
};

const SID: SessionId = SessionId::new(9022);
const BEFORE: &str =
    "g::role_projection::tests::native::network::failure::worker_exits_before_receipt";
const AFTER: &str =
    "g::role_projection::tests::native::network::failure::worker_exits_after_receipt";

#[tokio::test(flavor = "current_thread")]
async fn a_tcp_holder_loss_ends_the_run_without_restoring_the_initial_authority() {
    let owner_program = crate::runtime::program::project::<0>(&g::Program::<Global>::new());
    let service_program = crate::runtime::program::project::<2>(&g::Program::<Global>::new());
    let db_program = crate::runtime::program::project::<10>(&g::Program::<Global>::new());
    // Failure injection chooses the separate child entrypoint, never a
    // runtime or choreography mode. Both cases execute the same localsides.
    for entrypoint in [BEFORE, AFTER] {
        // SAFETY: this test owns the isolated resource namespace and sole
        // issuer. Process guards and broker fence precede the binding's drop.
        let binding = unsafe { crate::runtime::transport::EndpointBinding::new() };
        let broker = Broker::start(BROKER).await;
        let mut worker = Process(
            Command::new(std::env::current_exe().unwrap())
                .args(["--ignored", "--exact", entrypoint, "--nocapture"])
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
                .rendezvous(
                    &mut slab,
                    NetworkTransport::new([broker.address; 256], Some((10, &binding))),
                )
                .unwrap();
            let mut owner = rv.enter(SID, &owner_program).unwrap();
            let mut service = rv.enter(SID, &service_program).unwrap();
            let db = rv.enter(SID, &db_program).unwrap();
            let began = Instant::now();
            let (returned, requested) = tokio::time::timeout(Duration::from_secs(8), async {
                tokio::join!(
                    async {
                        owner.send::<Loan>(db).await.unwrap();
                        owner.recv::<Loan>().await
                    },
                    service.recv::<Read>(),
                )
            })
            .await
            .expect("carrier deadlines must finish before the test watchdog");
            assert!(returned.is_err());
            assert!(requested.is_err());
            assert!(began.elapsed() >= Duration::from_secs(4));
            let tag = view_tag(&owner);
            // SAFETY: owner and its pinned NetworkTransport runtime remain
            // alive; no public operation is borrowed at this observation.
            let first = unsafe { tag.actor_call_cut::<NetworkTransport<'_>>() };
            assert_eq!(first.fault, Some(SessionFaultKind::TransportClosed));
            assert!(owner.recv::<Loan>().await.is_err());
            assert!(service.recv::<Read>().await.is_err());
            // SAFETY: the same pinned actor and concrete transport remain.
            let repeated = unsafe { tag.actor_call_cut::<NetworkTransport<'_>>() };
            assert_eq!(repeated, first);
            assert!(rv.enter(SID, &db_program).is_err());
            assert!(
                rv.enter(SessionId::new(SID.raw() + 1), &db_program)
                    .is_err()
            );
        }
        worker.wait_success().await;
        {
            let mut slab = vec![0; 64 * 1024];
            let mut storage = SessionKitStorage::uninit();
            let rv = storage
                .init()
                .rendezvous(
                    &mut slab,
                    NetworkTransport::new([broker.address; 256], Some((10, &binding))),
                )
                .unwrap();
            assert!(
                rv.enter(SessionId::new(SID.raw() + 2), &db_program)
                    .is_err(),
                "a fresh runtime cannot replenish the physical issuer"
            );
        }
        let journal = broker.journal().await;
        assert_eq!(
            journal
                .iter()
                .filter(|(operation, _)| *operation == 1)
                .count(),
            1
        );
        let received = journal
            .iter()
            .filter(|(operation, _)| *operation == 2)
            .count();
        match entrypoint {
            BEFORE => assert_eq!(received, 0),
            AFTER => assert_eq!(received, 1),
            _ => unreachable!(),
        }
        std::eprintln!(
            "TCP holder loss entry={entrypoint} accepted=1 received={received} first_fault=TransportClosed"
        );
        drop(worker);
        drop(broker);
    }
}

#[test]
#[ignore = "separate pre-receive exit exercised by the TCP failure fixture"]
fn worker_exits_before_receipt() {}

#[tokio::test(flavor = "current_thread")]
#[ignore = "separate post-receive exit exercised by the TCP failure fixture"]
async fn worker_exits_after_receipt() {
    let address: SocketAddr = std::env::var("HIBANA_NETWORK_ADDRESS")
        .unwrap()
        .parse()
        .unwrap();
    let program = crate::runtime::program::project::<1>(&g::Program::<Global>::new());
    let mut slab = vec![0; 64 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, NetworkTransport::new([address; 256], None))
        .unwrap();
    let mut worker = rv.enter(SID, &program).unwrap();
    drop(worker.recv::<Loan>().await.unwrap());
}
