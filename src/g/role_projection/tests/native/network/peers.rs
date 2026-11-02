//! Direct peer TCP inboxes. DB's logical inbox stays at OWNER while WORKER
//! holds DB and reads its queued bytes across TCP. There is no central broker
//! or inherited data socket. This tests stable peer inboxes, not address migration.
use super::{
    Loan, Queued, Read, Value, g,
    inbox::{self, NetworkTransport, Peer, Process},
};
use crate::runtime::{SessionKitStorage, ids::SessionId};
use std::{
    io::{self, Write},
    net::SocketAddr,
    process::{Command, Stdio},
    time::Duration,
    vec,
};

type Global = g::Seq<
    g::Send<0, 1, Loan>,
    g::Seq<
        g::Send<2, 10, Queued>,
        g::Seq<
            g::Send<10, 2, Read>,
            g::Seq<
                g::Send<2, 10, Value>,
                g::Seq<g::Send<1, 0, Loan>, g::Seq<g::Send<10, 2, Read>, g::Send<2, 10, Value>>>,
            >,
        >,
    >,
>;
const SID: SessionId = SessionId::new(9024);
const WORKER: &str = "g::role_projection::tests::native::network::peers::worker_process";
const SERVICE: &str = "g::role_projection::tests::native::network::peers::service_process";

#[hibana_runtime::main]
#[test]
async fn old_peer_forwards_queued_bytes_to_the_remote_holder_without_a_broker()
-> core::result::Result<(), hibana::EndpointError> {
    let mut peer = Peer::bind(|_| Duration::ZERO);
    let mut children = vec![];
    let mut outputs = vec![];
    let mut addresses = [peer.address; 256];
    for (role, entrypoint) in [(1, WORKER), (2, SERVICE)] {
        let mut child = Process::spawn(
            Command::new(std::env::current_exe().unwrap())
                .args(["--ignored", "--exact", entrypoint, "--nocapture"])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit()),
        )
        .unwrap();
        let (input, mut output) = child.io().unwrap();
        let address = hibana_runtime::race(
            async {
                loop {
                    let line = inbox::line(&mut output).await?;
                    if let Some((_, address)) = line.split_once("inbox=") {
                        return address
                            .trim()
                            .parse::<SocketAddr>()
                            .map_err(io::Error::other);
                    }
                }
            },
            hibana_runtime::host::deadline(Duration::from_secs(5)),
        )
        .await
        .unwrap();
        addresses[role] = address;
        children.push((child, input));
        // Keep the output pipe open until the test harness has printed its
        // result and the child exits. It carries no choreography messages.
        outputs.push(output);
    }
    // These pipes carry deployment addresses only, never frames, bindings or
    // Endpoint values. Every protocol operation below uses a TCP transport.
    for (_, input) in &mut children {
        input
            .write_all(
                std::format!("{} {} {}\n", addresses[0], addresses[1], addresses[2]).as_bytes(),
            )
            .await
            .unwrap();
    }
    let address = peer.address;
    hibana_runtime::race(peer.serve(), async {
        let global = g::Program::<Global>::new();
        let owner_program = crate::runtime::program::project::<0>(&global);
        let db_program = crate::runtime::program::project::<10>(&global);
        // SAFETY: this process owns the sole initial DB operation boundary. All
        // remote holders obtain their rights from its projected Loan and return.
        let binding = unsafe { crate::runtime::transport::EndpointBinding::new() };
        let mut slab = vec![0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(
                &mut slab,
                NetworkTransport::new(addresses, Some((10, &binding))),
            )
            .unwrap();
        let mut owner = rv.enter(SID, &owner_program).unwrap();
        let db = rv.enter(SID, &db_program).unwrap();

        // Schedule the service's permitted send before moving DB. Its ordinary
        // frame is physically in the old holder's process, not in WORKER's queue.
        hibana_runtime::race(
            async {
                let _: () = (async {
                    loop {
                        let journal = inbox::journal(address).await;
                        if journal.iter().any(|(op, packet)| {
                            *op == 1 && packet[6] == 10 && packet[9..] == 7u32.to_be_bytes()
                        }) {
                            assert!(!journal.iter().any(|(op, _)| *op == 2));
                            break;
                        }
                        hibana_runtime::host::sleep(Duration::from_millis(1))
                            .await
                            .unwrap();
                    }
                })
                .await;
                Ok::<_, std::io::Error>(())
            },
            hibana_runtime::host::deadline(Duration::from_secs(5)),
        )
        .await
        .unwrap();
        owner.send::<Loan>(db).await.unwrap();
        assert!(children[0].0.wait().await.unwrap().success());
        // WORKER has exited. Its returned Endpoint resumes on this runtime and
        // consumes the same logical FIFO through the owner's still-live inbox.
        let mut db = owner.recv::<Loan>().await.unwrap();
        db.send::<Read>(&41).await.unwrap();
        assert_eq!(db.recv::<Value>().await.unwrap(), 42);
        assert!(children[1].0.wait().await.unwrap().success());
        let journal = inbox::journal(address).await;
        let sent: std::vec::Vec<_> = journal
            .iter()
            .filter(|(op, _)| *op == 1)
            .map(|(_, packet)| packet)
            .collect();
        let received: std::vec::Vec<_> = journal
            .iter()
            .filter(|(op, _)| *op == 2)
            .map(|(_, packet)| packet)
            .collect();
        assert_eq!(sent.len(), 4);
        assert_eq!(
            sent, received,
            "old and new holders consume the peer inbox exactly once in FIFO order"
        );
        Ok::<_, io::Error>(())
    })
    .await
    .unwrap();

    Ok(())
}

// Read only the address configuration. A child creates its own listener,
// runtime, slab and transport; no parent socket or memory is inherited.
fn addresses(peer: &Peer, role: usize) -> [SocketAddr; 256] {
    std::println!("inbox={}", peer.address);
    io::stdout().flush().unwrap();
    let mut line = std::string::String::new();
    io::stdin().read_line(&mut line).unwrap();
    let mut entries = line.split_whitespace();
    let mut addresses = [entries.next().unwrap().parse().unwrap(); 256];
    addresses[1] = entries.next().unwrap().parse().unwrap();
    addresses[2] = entries.next().unwrap().parse().unwrap();
    assert!(entries.next().is_none());
    assert_eq!(addresses[role], peer.address);
    addresses
}

#[hibana_runtime::main]
#[test]
#[ignore = "separate peer exercised by the parent TCP fixture"]
async fn worker_process() -> core::result::Result<(), hibana::EndpointError> {
    let mut peer = Peer::bind(|_| Duration::ZERO);
    let addresses = addresses(&peer, 1);
    hibana_runtime::race(peer.serve(), async {
        let worker_program = crate::runtime::program::project::<1>(&g::Program::<Global>::new());
        let mut slab = vec![0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, NetworkTransport::new(addresses, None))
            .unwrap();
        let mut worker = rv.enter(SID, &worker_program).unwrap();
        let mut db = worker.recv::<Loan>().await.unwrap();
        assert_eq!(db.recv::<Queued>().await.unwrap(), 7);
        db.send::<Read>(&7).await.unwrap();
        assert_eq!(db.recv::<Value>().await.unwrap(), 8);
        worker.send::<Loan>(db).await.unwrap();
        Ok::<_, io::Error>(())
    })
    .await
    .unwrap();

    Ok(())
}

#[hibana_runtime::main]
#[test]
#[ignore = "separate physical service exercised by the parent TCP fixture"]
async fn service_process() -> core::result::Result<(), hibana::EndpointError> {
    let mut peer = Peer::bind(|_| Duration::ZERO);
    let addresses = addresses(&peer, 2);
    hibana_runtime::race(peer.serve(), async {
        let service_program = crate::runtime::program::project::<2>(&g::Program::<Global>::new());
        let mut slab = vec![0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, NetworkTransport::new(addresses, None))
            .unwrap();
        let mut service = rv.enter(SID, &service_program).unwrap();
        service.send::<Queued>(&7).await.unwrap();
        let value = service.recv::<Read>().await.unwrap() + 1;
        service.send::<Value>(&value).await.unwrap();
        let value = service.recv::<Read>().await.unwrap() + 1;
        service.send::<Value>(&value).await.unwrap();
        Ok::<_, io::Error>(())
    })
    .await
    .unwrap();

    Ok(())
}
