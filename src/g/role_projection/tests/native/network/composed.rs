//! The same resolved, rolled parallel global used by the in-memory and Unix
//! fixtures, over separate TCP inbox and worker processes.
use super::super::super::{
    Loan, Read, Value,
    composed::{AfterRead, AfterValue, Finished, Global, Subtract},
};
use super::{BROKER, Broker, NetworkTransport, Process};
use crate::{
    g,
    runtime::{
        SessionKitStorage,
        ids::SessionId,
        resolver::{DecisionArm, ResolverRef},
    },
};
use core::cell::Cell;
use std::{net::SocketAddr, process::Command, string::ToString, vec};

const SID: SessionId = SessionId::new(9021);
const WORKER: &str = "g::role_projection::tests::native::network::composed::worker_process";

#[tokio::test(flavor = "current_thread")]
async fn tcp_inboxes_preserve_resolved_choices_and_repeated_loans_alongside_parallel_work() {
    let owner_program = crate::runtime::program::project::<0>(&g::Program::<Global>::new());
    let service_program = crate::runtime::program::project::<2>(&g::Program::<Global>::new());
    let db_program = crate::runtime::program::project::<10>(&g::Program::<Global>::new());
    let left_program = crate::runtime::program::project::<3>(&g::Program::<Global>::new());
    let right_program = crate::runtime::program::project::<4>(&g::Program::<Global>::new());
    let mut value = 41u32;
    // SAFETY: only this service owns value and its one initial issuer.
    // During unwinding, process guards fence remote rights before the binding.
    let binding = unsafe { crate::runtime::transport::EndpointBinding::new() };
    let broker = Broker::start(BROKER).await;
    let mut child = Process(
        Command::new(std::env::current_exe().unwrap())
            .args(["--ignored", "--exact", WORKER, "--nocapture"])
            .env("HIBANA_NETWORK_ADDRESS", broker.address.to_string())
            .env("HIBANA_NETWORK_PARENT", std::process::id().to_string())
            .spawn()
            .unwrap(),
    );
    {
        let mut owner_slab = vec![0; 64 * 1024];
        let mut service_slab = vec![0; 64 * 1024];
        let mut side_slab = vec![0; 64 * 1024];
        let mut owner_storage = SessionKitStorage::uninit();
        let mut service_storage = SessionKitStorage::uninit();
        let mut side_storage = SessionKitStorage::uninit();
        let owner_rv = owner_storage
            .init()
            .rendezvous(&mut owner_slab, NetworkTransport::new(broker.address, None))
            .unwrap();
        let service_rv = service_storage
            .init()
            .rendezvous(
                &mut service_slab,
                NetworkTransport::new(broker.address, Some((10, &binding))),
            )
            .unwrap();
        let side_rv = side_storage
            .init()
            .rendezvous(&mut side_slab, NetworkTransport::new(broker.address, None))
            .unwrap();
        assert!(owner_rv.enter(SID, &db_program).is_err());
        let mut owner = owner_rv.enter(SID, &owner_program).unwrap();
        let mut service = service_rv.enter(SID, &service_program).unwrap();
        let db = service_rv.enter(SID, &db_program).unwrap();
        let mut left = side_rv.enter(SID, &left_program).unwrap();
        let mut right = side_rv.enter(SID, &right_program).unwrap();
        tokio::join!(
            async {
                service.send::<Loan>(db).await.unwrap();
                for expected in [44, 40] {
                    let branch = service.offer().await.unwrap();
                    value = match branch.label() {
                        3 => value + branch.recv::<Read>().await.unwrap(),
                        6 => value - branch.recv::<Subtract>().await.unwrap(),
                        label => panic!("unexpected branch {label}"),
                    };
                    assert_eq!(value, expected);
                    service.send::<Value>(&value).await.unwrap();
                }
                value += service.recv::<AfterRead>().await.unwrap();
                service.send::<AfterValue>(&value).await.unwrap();
                drop(service.recv::<Finished>().await.unwrap());
            },
            async {
                let mut db = owner.recv::<Loan>().await.unwrap();
                for _ in 0..2 {
                    owner.send::<Loan>(db).await.unwrap();
                    db = owner.recv::<Loan>().await.unwrap();
                }
                db.send::<AfterRead>(&5).await.unwrap();
                assert_eq!(db.recv::<AfterValue>().await.unwrap(), 45);
                owner.send::<Finished>(db).await.unwrap();
            },
            async {
                for number in [10, 20] {
                    left.send::<Read>(&number).await.unwrap();
                    assert_eq!(left.recv::<Value>().await.unwrap(), number + 1);
                }
            },
            async {
                for number in [10, 20] {
                    assert_eq!(right.recv::<Read>().await.unwrap(), number);
                    right.send::<Value>(&(number + 1)).await.unwrap();
                }
            }
        );
    }
    child.wait_success().await;
    assert_eq!(value, 45);
    let journal = broker.journal().await;
    let mut accepted = std::collections::BTreeMap::<_, std::vec::Vec<_>>::new();
    let mut received = std::collections::BTreeMap::<_, std::vec::Vec<_>>::new();
    for (operation, packet) in &journal {
        let direction: [u8; 7] = packet[..7].try_into().unwrap();
        match operation {
            1 => accepted.entry(direction).or_default().push(packet),
            2 => received.entry(direction).or_default().push(packet),
            _ => unreachable!(),
        }
    }
    assert_eq!(accepted.values().map(std::vec::Vec::len).sum::<usize>(), 16);
    assert_eq!(
        accepted, received,
        "whole logical directions retain FIFO across TCP connections and holders"
    );
    drop(child);
    drop(broker);
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "separate process entrypoint exercised by the composed network fixture"]
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
    let worker_program = crate::runtime::program::project::<1>(&g::Program::<Global>::new());
    let db_program = crate::runtime::program::project::<10>(&g::Program::<Global>::new());
    let delta = Cell::new(3u32);
    let mut slab = vec![0; 64 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, NetworkTransport::new(address, None))
        .unwrap();
    rv.set_resolver(
        &db_program,
        ResolverRef::<702>::decision_state(&delta, |number| {
            Ok(if number.get() % 2 == 1 {
                DecisionArm::Left
            } else {
                DecisionArm::Right
            })
        }),
    )
    .unwrap();
    assert!(rv.enter(SID, &db_program).is_err());
    let mut worker = rv.enter(SID, &worker_program).unwrap();
    for (number, expected) in [(3, 44), (4, 40)] {
        delta.set(number);
        let mut db = worker.recv::<Loan>().await.unwrap();
        let branch = db.offer().await.unwrap();
        match branch.label() {
            3 => branch.send::<Read>(&number).await.unwrap(),
            6 => branch.send::<Subtract>(&number).await.unwrap(),
            label => panic!("unexpected resolver choice {label}"),
        }
        assert_eq!(db.recv::<Value>().await.unwrap(), expected);
        worker.send::<Loan>(db).await.unwrap();
    }
}
