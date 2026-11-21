//! The controller publishes its decision before the holder process starts.
//! The inbox journal is observed by the controller process only.
use super::super::super::send_wait::WakeCount;
use super::{BROKER, Broker, NetworkTransport, Process, g};
use crate::{
    endpoint::proof_snapshot::view_tag,
    runtime::{SessionKitStorage, ids::SessionId, program::project, transport::EndpointBinding},
};
use core::{
    future::Future,
    pin::pin,
    task::{Context, Waker},
};
use std::{
    net::SocketAddr,
    process::Command,
    string::ToString,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    vec,
};

type Loan = g::Msg<1, crate::Endpoint<'static, 2>>;
type Work = g::Msg<2, ()>;
type Skip = g::Msg<3, ()>;
type Optional = g::Msg<4, ()>;
type Value = g::Msg<5, u8>;
type Global = g::Seq<
    g::Send<0, 1, Loan>,
    g::Seq<
        g::Route<g::Seq<g::Send<0, 1, Work>, g::Send<2, 0, Optional>>, g::Send<0, 1, Skip>>,
        g::Send<2, 0, Value>,
    >,
>;
const SID: SessionId = SessionId::new(9117);
const HOLDER: &str = "g::role_projection::tests::native::network::open_choice::holder_process";

#[hibana_runtime::main]
#[test]
async fn a_remote_choice_enables_the_same_parked_send_only_after_local_receipt()
-> Result<(), crate::EndpointError> {
    let global = g::Program::<Global>::new();
    let owner_program = project::<0>(&global);
    let resource_program = project::<2>(&global);
    let broker = Broker::start(BROKER).await;
    // SAFETY: this process is the sole initial issuer of the resource. The
    // remote holder is reaped and its final frame drained before binding drop.
    let binding = unsafe { EndpointBinding::new() };
    let mut slab = vec![0; 32 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(
            &mut slab,
            NetworkTransport::new([broker.address; 256], Some((2, &binding))),
        )
        .unwrap();
    let mut owner = rv.enter(SID, &owner_program).unwrap();
    owner
        .send::<Loan>(rv.enter(SID, &resource_program).unwrap())
        .await?;
    owner.send::<Skip>(&()).await?;
    let queued = broker.journal().await;
    let [(1, loan), (1, skip)] = queued.as_slice() else {
        panic!("both sends must be accepted before the holder starts: {queued:?}");
    };
    assert_eq!(&loan[5..7], &[0, 1]);
    assert_eq!(&skip[5..7], &[0, 1]);
    assert_ne!(loan, skip);
    let mut child = Process::spawn(
        Command::new(std::env::current_exe().unwrap())
            .args(["--ignored", "--exact", HOLDER, "--nocapture"])
            .env("HIBANA_NETWORK_ADDRESS", broker.address.to_string())
            .env("HIBANA_NETWORK_PARENT", std::process::id().to_string()),
    )
    .unwrap();
    assert!(child.wait().await.unwrap().success());
    assert_eq!(owner.recv::<Value>().await?, 17);
    let journal = broker.journal().await;
    let [
        (1, original),
        (1, decision),
        (2, received_loan),
        (2, received_skip),
        (1, value),
        (2, received_value),
    ] = journal.as_slice()
    else {
        panic!("unexpected TCP admission or receipt order: {journal:?}");
    };
    assert_eq!(original, loan);
    assert_eq!(decision, skip);
    assert_eq!(received_loan, loan);
    assert_eq!(received_skip, skip);
    assert_eq!(&value[5..7], &[2, 0]);
    assert_eq!(value, received_value);
    Ok(())
}

#[hibana_runtime::main]
#[test]
#[ignore = "private holder process exercised by the TCP choice test"]
async fn holder_process() -> Result<(), crate::EndpointError> {
    let parent: u32 = std::env::var("HIBANA_NETWORK_PARENT")
        .unwrap()
        .parse()
        .unwrap();
    assert_ne!(parent, std::process::id());
    let address: SocketAddr = std::env::var("HIBANA_NETWORK_ADDRESS")
        .unwrap()
        .parse()
        .unwrap();
    let program = project::<1>(&g::Program::<Global>::new());
    let mut slab = vec![0; 32 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, NetworkTransport::new([address; 256], None))
        .unwrap();
    let mut holder = rv.enter(SID, &program).unwrap();
    let mut resource = holder.recv::<Loan>().await?;
    let tag = view_tag(&resource);
    let mut send = pin!(resource.send::<Value>(&17));
    let wakes = Arc::new(WakeCount(AtomicUsize::new(0)));
    let waker = Waker::from(wakes.clone());
    assert!(
        send.as_mut()
            .poll(&mut Context::from_waker(&waker))
            .is_pending()
    );
    // SAFETY: this actor is pinned, uses NetworkTransport and is between polls.
    let parked = unsafe { tag.operation_cut::<NetworkTransport<'_>>() };
    assert_eq!(parked.actor.choices.len(), 1);
    assert!(parked.actor.choices[0].1.is_none());
    let slot = parked
        .operations
        .iter()
        .find(|slot| (slot.view, slot.lane) == (parked.target.view, parked.target.lane))
        .unwrap();
    assert_eq!(slot.preparing.map(|(label, _)| label), Some(5));
    assert!(slot.prepared_send.is_none());
    let before_receipt = wakes.0.load(Ordering::Relaxed);
    holder.recv::<Skip>().await?;
    assert!(wakes.0.load(Ordering::Relaxed) > before_receipt);
    send.await?;
    Ok(())
}
