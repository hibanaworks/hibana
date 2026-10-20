//! Transport errors do not roll back accepted authority or broadcast a fault.
use super::*;
use crate::{
    endpoint::proof_snapshot::owned_future::{Capture, OwnedFutureBoundary},
    endpoint::proof_snapshot::{ActorSnapshot, view_tag},
    rendezvous::SessionFaultKind,
    runtime::{SessionKitStorage, ids::SessionId},
    test_support::transport::{FrameEvidence, TestTransport},
    transport::TransportError,
};
use core::pin::pin;
use std::vec::Vec;

mod io;
#[cfg(not(miri))]
mod observations;
use io::{At, Io};

type Tick = g::Msg<5, u32>;
type Steps = g::Par<g::Seq<g::Send<0, 1, Loan>, g::Send<10, 1, Read>>, g::Send<0, 1, Tick>>;
const SID: SessionId = SessionId::new(9077);
const ERRORS: [TransportError; 4] = [
    TransportError::Offline,
    TransportError::Deadline,
    TransportError::Capacity,
    TransportError::Failed,
];

struct Sample {
    fault: io::Fault,
    cuts: Vec<(&'static str, ActorSnapshot)>,
    frames: Vec<(&'static str, FrameEvidence)>,
    captured: Vec<OwnedFutureBoundary>,
}

#[test]
fn every_send_error_preserves_an_accepted_loan_and_the_remote_holder() {
    let () = g::ProgramProjection::<Steps, 8>::VALIDATION;
    let owner_program = crate::runtime::program::project::<0>(&g::Program::<Steps>::new());
    let worker_program = crate::runtime::program::project::<1>(&g::Program::<Steps>::new());
    let db_program = crate::runtime::program::project::<10>(&g::Program::<Steps>::new());
    let mut samples = Vec::new();
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async {
            for error in ERRORS {
                let network = TestTransport::new();
                let owner_io = Io::new(network.clone(), 0, Some((At::SendAfterAcceptance, error)));
                let fault_records = owner_io.observed.clone();
                let worker_io = Io::new(network.clone(), 1, None);
                let mut owner_slab = [0; 32 * 1024];
                let mut worker_slab = [0; 32 * 1024];
                let mut owner_storage = SessionKitStorage::uninit();
                let mut worker_storage = SessionKitStorage::uninit();
                let owner_rv = owner_storage
                    .init()
                    .rendezvous(&mut owner_slab, owner_io)
                    .unwrap();
                let worker_rv = worker_storage
                    .init()
                    .rendezvous(&mut worker_slab, worker_io)
                    .unwrap();
                let mut owner = owner_rv.enter(SID, &owner_program).unwrap();
                let db = owner_rv.enter(SID, &db_program).unwrap();
                let mut worker = worker_rv.enter(SID, &worker_program).unwrap();
                let owner_tag = view_tag(&owner);
                let worker_tag = view_tag(&worker);
                // SAFETY: these actors are pinned, idle, and use exactly Io.
                let worker_before = unsafe { worker_tag.snapshot::<Io>() };
                let [mut loans, mut ticker] = owner.par().unwrap();
                let [mut requests, _ticks] = worker.par().unwrap();
                let capture = Capture::start();
                let captured;
                {
                    let mut transfer = pin!(loans.send::<Loan>(db));
                    // SAFETY: the future retains its exact pinned actor between polls.
                    let owner_before = unsafe { owner_tag.snapshot::<Io>() };
                    (&mut transfer).await.unwrap_err();
                    // SAFETY: the failed poll returned; the future remains held.
                    let owner_after = unsafe { owner_tag.snapshot::<Io>() };
                    assert_eq!(owner_after.fault, Some(SessionFaultKind::TransportClosed));
                    assert_eq!(owner_before.steps, owner_after.steps);
                    assert_eq!(
                        owner_before.completion_mutations,
                        owner_after.completion_mutations
                    );
                    assert!(ticker.send::<Tick>(&99).await.is_err());
                    // SAFETY: the rejected independent operation has returned.
                    assert_eq!(unsafe { owner_tag.snapshot::<Io>() }, owner_after);
                    let loan = network.accepted_frame(1, 0);
                    assert!(!network.queue_is_empty());
                    let mut received = requests.recv::<Loan>().await.unwrap();
                    let loan_received = network.received_frame(1, SID, loan.header.lane());
                    assert_eq!(loan_received.witness, loan.witness);
                    // SAFETY: the same pinned worker is idle after receive.
                    let worker_received = unsafe { worker_tag.snapshot::<Io>() };
                    received.send::<Read>(&41).await.unwrap();
                    // SAFETY: the DB view belongs to the same pinned Io actor.
                    let worker_sent = unsafe { worker_tag.snapshot::<Io>() };
                    let read = network.accepted_frame(1, 0);
                    assert_eq!(requests.recv::<Read>().await.unwrap(), 41);
                    let read_received = network.received_frame(1, SID, read.header.lane());
                    assert_eq!(read_received.witness, read.witness);
                    // SAFETY: these copied observations are between completed calls.
                    let worker_done = unsafe { worker_tag.snapshot::<Io>() };
                    assert_eq!(worker_done.fault, None);
                    assert_eq!(
                        unsafe { owner_tag.snapshot::<Io>() }.fault,
                        owner_after.fault
                    );
                    assert!(network.queue_is_empty());
                    assert!(owner_rv.enter(SID, &db_program).is_err());
                    captured = capture.finish();
                    assert_eq!(fault_records.borrow().len(), 1);
                    samples.push(Sample {
                        fault: fault_records.borrow()[0],
                        cuts: std::vec![
                            ("ownerBefore", owner_before),
                            ("ownerAfter", owner_after),
                            ("workerBefore", worker_before),
                            ("workerReceived", worker_received),
                            ("workerSent", worker_sent),
                            ("workerDone", worker_done)
                        ],
                        frames: std::vec![
                            ("loan", loan),
                            ("loanReceived", loan_received),
                            ("read", read),
                            ("readReceived", read_received)
                        ],
                        captured,
                    });
                }
                assert!(owner_rv.enter(SID, &db_program).is_err());
                assert!(ticker.send::<Tick>(&99).await.is_err());
                assert_eq!(fault_records.borrow().len(), 1);
                // SAFETY: owner still retains this actor after the failed calls.
                assert_eq!(
                    unsafe { owner_tag.snapshot::<Io>() }.fault,
                    Some(SessionFaultKind::TransportClosed)
                );
            }
        });
    assert_eq!(
        samples
            .iter()
            .map(|sample| sample.fault.error)
            .collect::<Vec<_>>(),
        ERRORS
    );
    for sample in &samples {
        assert_eq!(sample.fault.at, At::SendAfterAcceptance);
        assert_eq!(sample.fault.session, SID);
        assert_eq!(sample.fault.view, 0);
        assert_eq!(sample.fault.lane, sample.frames[0].1.header.lane());
        assert_eq!(sample.cuts[0].1.actor, 0);
        assert!(matches!(
            sample.captured.as_slice(),
            [
                OwnedFutureBoundary::Seal { .. },
                OwnedFutureBoundary::Poll {
                    result: core::task::Poll::Ready(Err(_)),
                    ..
                }
            ]
        ));
    }
    #[cfg(not(miri))]
    observations::write("Send", &samples);
    #[cfg(miri)]
    let _ = samples;
}

#[test]
fn every_receive_error_keeps_the_frame_queued_without_issuing_its_resource() {
    let () = g::ProgramProjection::<Steps, 8>::VALIDATION;
    let owner_program = crate::runtime::program::project::<0>(&g::Program::<Steps>::new());
    let worker_program = crate::runtime::program::project::<1>(&g::Program::<Steps>::new());
    let db_program = crate::runtime::program::project::<10>(&g::Program::<Steps>::new());
    let mut samples = Vec::new();
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async {
            for error in ERRORS {
                let network = TestTransport::new();
                let owner_io = Io::new(network.clone(), 0, None);
                let worker_io =
                    Io::new(network.clone(), 1, Some((At::ReceiveBeforeDelivery, error)));
                let fault_records = worker_io.observed.clone();
                let mut owner_slab = [0; 32 * 1024];
                let mut worker_slab = [0; 32 * 1024];
                let mut owner_storage = SessionKitStorage::uninit();
                let mut worker_storage = SessionKitStorage::uninit();
                let owner_rv = owner_storage
                    .init()
                    .rendezvous(&mut owner_slab, owner_io)
                    .unwrap();
                let worker_rv = worker_storage
                    .init()
                    .rendezvous(&mut worker_slab, worker_io)
                    .unwrap();
                let mut owner = owner_rv.enter(SID, &owner_program).unwrap();
                let db = owner_rv.enter(SID, &db_program).unwrap();
                let mut worker = worker_rv.enter(SID, &worker_program).unwrap();
                let owner_tag = view_tag(&owner);
                let worker_tag = view_tag(&worker);
                let [mut loans, _ticker] = owner.par().unwrap();
                let [mut requests, _ticks] = worker.par().unwrap();
                let capture = Capture::start();
                {
                    let mut transfer = pin!(loans.send::<Loan>(db));
                    // SAFETY: the live future retains this pinned Io actor.
                    let owner_before = unsafe { owner_tag.snapshot::<Io>() };
                    (&mut transfer).await.unwrap();
                    // SAFETY: all snapshots are between public-operation polls.
                    let owner_after = unsafe { owner_tag.snapshot::<Io>() };
                    let worker_before = unsafe { worker_tag.snapshot::<Io>() };
                    let loan = network.accepted_frame(1, 0);
                    assert!(requests.recv::<Loan>().await.is_err());
                    let worker_after = unsafe { worker_tag.snapshot::<Io>() };
                    assert_eq!(worker_after.fault, Some(SessionFaultKind::TransportClosed));
                    assert_eq!(worker_before.steps, worker_after.steps);
                    assert_eq!(
                        worker_before.completion_mutations,
                        worker_after.completion_mutations
                    );
                    assert_eq!(worker_before.live_views, worker_after.live_views);
                    let remaining = network.accepted_frame(1, 0);
                    assert_eq!(remaining.witness, loan.witness);
                    assert_eq!(remaining.header, loan.header);
                    assert_eq!(remaining.payload(), loan.payload());
                    assert_eq!(unsafe { owner_tag.snapshot::<Io>() }.fault, None);
                    assert!(owner_rv.enter(SID, &db_program).is_err());
                    assert_eq!(fault_records.borrow().len(), 1);
                    let captured = capture.finish();
                    samples.push(Sample {
                        fault: fault_records.borrow()[0],
                        cuts: std::vec![
                            ("ownerBefore", owner_before),
                            ("ownerAfter", owner_after),
                            ("workerBefore", worker_before),
                            ("workerAfter", worker_after)
                        ],
                        frames: std::vec![("loan", loan)],
                        captured,
                    });
                }
            }
        });
    assert_eq!(
        samples
            .iter()
            .map(|sample| sample.fault.error)
            .collect::<Vec<_>>(),
        ERRORS
    );
    for sample in &samples {
        assert_eq!(sample.fault.at, At::ReceiveBeforeDelivery);
        assert_eq!(sample.fault.session, SID);
        assert_eq!(sample.fault.view, 1);
        assert_eq!(sample.fault.lane, sample.frames[0].1.header.lane());
        assert_eq!(sample.cuts[2].1.actor, 1);
        assert!(matches!(
            sample.captured.as_slice(),
            [
                OwnedFutureBoundary::Seal { .. },
                OwnedFutureBoundary::Poll {
                    result: core::task::Poll::Ready(Ok(())),
                    ..
                }
            ]
        ));
    }
    #[cfg(not(miri))]
    observations::write("Receive", &samples);
    #[cfg(miri)]
    let _ = samples;
}
