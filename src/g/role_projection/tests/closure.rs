//! Orderly carrier closure preserves an accepted transfer, then faults only
//! the actor that actually observes EOF. There is no shared runtime fault bus.
use super::*;
use crate::endpoint::proof_snapshot::owned_future::{Capture, OwnedFutureBoundary};
use crate::endpoint::proof_snapshot::{actor_snapshot, view_tag};
use crate::rendezvous::SessionFaultKind;
use crate::runtime::{SessionKitStorage, ids::SessionId};
use crate::test_support::transport::TestTransport;
use core::{
    future::Future,
    pin::pin,
    task::{Context, Poll, Waker},
};

mod abort;
mod ports;

type Steps =
    g::Seq<g::Roll<g::Seq<g::Send<0, 1, Loan>, g::Send<1, 0, Loan>>>, g::Send<10, 2, Read>>;

#[test]
fn an_accepted_loan_drains_before_eof_and_its_returned_value_remains_usable() {
    let () = g::ProgramProjection::<Steps, 8>::VALIDATION;
    let owner_program = crate::runtime::program::project::<0>(&g::Program::<Steps>::new());
    let worker_program = crate::runtime::program::project::<1>(&g::Program::<Steps>::new());
    let db_program = crate::runtime::program::project::<10>(&g::Program::<Steps>::new());
    let service_program = crate::runtime::program::project::<2>(&g::Program::<Steps>::new());
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async {
            let network = TestTransport::new();
            let mut owner_slab = [0u8; 32 * 1024];
            let mut worker_slab = [0u8; 32 * 1024];
            let mut service_slab = [0u8; 32 * 1024];
            let mut owner_storage = SessionKitStorage::uninit();
            let mut worker_storage = SessionKitStorage::uninit();
            let mut service_storage = SessionKitStorage::uninit();
            let owner_rv = owner_storage
                .init()
                .rendezvous(&mut owner_slab, network.clone())
                .unwrap();
            let worker_rv = worker_storage
                .init()
                .rendezvous(&mut worker_slab, network.clone())
                .unwrap();
            let service_rv = service_storage
                .init()
                .rendezvous(&mut service_slab, network.clone())
                .unwrap();
            let sid = SessionId::new(9060);
            let mut owner = owner_rv.enter(sid, &owner_program).unwrap();
            let mut worker = worker_rv.enter(sid, &worker_program).unwrap();
            let mut service = service_rv.enter(sid, &service_program).unwrap();
            let db = owner_rv.enter(sid, &db_program).unwrap();
            let owner_tag = view_tag(&owner);
            let worker_tag = view_tag(&worker);
            // SAFETY: each read is between polls, on the original pinned actor,
            // whose exact transport and slab remain alive throughout this test.
            let owner_before = unsafe { actor_snapshot(&owner) };
            let worker_before = unsafe { actor_snapshot(&worker) };
            let mut cx = Context::from_waker(Waker::noop());
            let capture = Capture::start();

            network.pause_send(0);
            let owner_pending = {
                let mut loan = pin!(owner.send::<Loan>(db));
                assert!(loan.as_mut().poll(&mut cx).is_pending());
                // SAFETY: the future retains the actor and no poll is in progress.
                let owner_pending = unsafe { owner_tag.snapshot::<TestTransport>() };
                network.resume_send();
                assert!(matches!(loan.as_mut().poll(&mut cx), Poll::Ready(Ok(()))));
                owner_pending
            };
            // SAFETY: same pinned allocation, between polls.
            let owner_sent = unsafe { owner_tag.snapshot::<TestTransport>() };
            let loan_wire = network.queued_frame(1);
            let loan_evidence = network.accepted_frame(1, 0);
            assert!(!network.queue_is_empty());
            network.close_port(sid, 1, loan_wire.0.lane());
            // Accepted bytes survive EOF; receiving them transfers the actual value.
            let db = worker.recv::<Loan>().await.unwrap();
            assert!(network.queue_is_empty());
            // SAFETY: no operation is active on this separate actor.
            let worker_received = unsafe { actor_snapshot(&worker) };

            network.pause_send(1);
            let worker_pending = {
                let mut give_back = pin!(worker.send::<Loan>(db));
                assert!(give_back.as_mut().poll(&mut cx).is_pending());
                // SAFETY: the future retains the actor and no poll is in progress.
                let worker_pending = unsafe { worker_tag.snapshot::<TestTransport>() };
                network.resume_send();
                assert!(matches!(
                    give_back.as_mut().poll(&mut cx),
                    Poll::Ready(Ok(()))
                ));
                worker_pending
            };
            let return_wire = network.queued_frame(0);
            let return_evidence = network.accepted_frame(0, 0);
            // SAFETY: same pinned allocation, between polls.
            let worker_sent = unsafe { worker_tag.snapshot::<TestTransport>() };
            let mut db = owner.recv::<Loan>().await.unwrap();
            // SAFETY: all three reads occur between operations.
            let owner_returned = unsafe { owner_tag.snapshot::<TestTransport>() };
            let worker_before_eof = unsafe { worker_tag.snapshot::<TestTransport>() };
            assert_eq!(worker_before_eof.fault, None);
            assert!(worker.recv::<Loan>().await.is_err());
            // SAFETY: the failed call has returned and the allocation remains live.
            let worker_after_eof = unsafe { worker_tag.snapshot::<TestTransport>() };
            let captured = capture.finish();
            assert!(matches!(
                captured.as_slice(),
                [
                    OwnedFutureBoundary::Seal { .. },
                    OwnedFutureBoundary::Poll {
                        result: Poll::Pending,
                        ..
                    },
                    OwnedFutureBoundary::Poll {
                        result: Poll::Ready(Ok(())),
                        ..
                    },
                    OwnedFutureBoundary::Drop { .. },
                    OwnedFutureBoundary::Seal { .. },
                    OwnedFutureBoundary::Poll {
                        result: Poll::Pending,
                        ..
                    },
                    OwnedFutureBoundary::Poll {
                        result: Poll::Ready(Ok(())),
                        ..
                    },
                    OwnedFutureBoundary::Drop { .. },
                ]
            ));
            assert_eq!(
                worker_after_eof.fault,
                Some(SessionFaultKind::TransportClosed)
            );
            assert_eq!(
                worker_after_eof.completion_mutations,
                worker_before_eof.completion_mutations
            );
            assert_eq!(worker_after_eof.steps, worker_before_eof.steps);
            assert_eq!(worker_after_eof.choices, worker_before_eof.choices);
            assert_eq!(
                worker_after_eof
                    .view_headers()
                    .collect::<std::vec::Vec<_>>(),
                worker_before_eof
                    .view_headers()
                    .collect::<std::vec::Vec<_>>()
            );
            assert_eq!(worker_after_eof.counter, worker_before_eof.counter);
            assert_eq!(worker_after_eof.live_views, worker_before_eof.live_views);
            // There is no cross-runtime fault propagation. The unique returned DB
            // value continues normally after the worker observes the closed lane.
            let (sent, received) = tokio::join!(db.send::<Read>(&42), service.recv::<Read>());
            sent.unwrap();
            assert_eq!(received.unwrap(), 42);
            assert!(network.queue_is_empty());
            // SAFETY: the owner is still alive and idle after the DB operation.
            assert_eq!(unsafe { owner_tag.snapshot::<TestTransport>() }.fault, None);
            #[cfg(not(miri))]
            write_observations(
                sid,
                &[
                    ("ownerBefore", &owner_before),
                    ("workerBefore", &worker_before),
                    ("ownerPending", &owner_pending),
                    ("ownerSent", &owner_sent),
                    ("workerReceived", &worker_received),
                    ("workerPending", &worker_pending),
                    ("workerSent", &worker_sent),
                    ("ownerReturned", &owner_returned),
                    ("workerBeforeEof", &worker_before_eof),
                    ("workerAfterEof", &worker_after_eof),
                ],
                &loan_wire,
                &return_wire,
                (&captured, [&loan_evidence, &return_evidence]),
            );
            #[cfg(miri)]
            let _ = (
                owner_before,
                worker_before,
                owner_pending,
                owner_sent,
                worker_received,
                worker_pending,
                worker_sent,
                owner_returned,
                return_wire,
                loan_evidence,
                return_evidence,
            );
        });
}

#[cfg(not(miri))]
fn write_observations(
    sid: SessionId,
    snapshots: &[(&str, &crate::endpoint::proof_snapshot::ActorSnapshot)],
    loan: &(crate::transport::FrameHeader, std::vec::Vec<u8>),
    returned: &(crate::transport::FrameHeader, std::vec::Vec<u8>),
    captured: (
        &[OwnedFutureBoundary],
        [&crate::test_support::transport::FrameEvidence; 2],
    ),
) {
    use crate::test_support::{lean_choreo::LeanChoreo, lean_descriptor::descriptor_source};
    let mut source = std::format!(
        "import Hibana.SourceOccurrenceTrace\nimport Hibana.OwnedCallHistory\n\n\
         def closureChoreo : Hibana.Choreo := {}\n\n\
         def ownerImage : Hibana.RustDescriptorImage := {}\n\n\
         def workerImage : Hibana.RustDescriptorImage := {}\n\n",
        Steps::lean_source(),
        descriptor_source(RoleDescriptorRef::from_resident(
            &RoleProjection::<0, Steps, 8>::IMAGE_REF
        )),
        descriptor_source(RoleDescriptorRef::from_resident(
            &RoleProjection::<1, Steps, 8>::IMAGE_REF
        )),
    );
    for (name, snapshot) in snapshots {
        source.push_str(&std::format!(
            "def {name} : Hibana.ActorProgressObservation := {}\n\n",
            super::commit::observation_source(snapshot),
        ));
        source.push_str(&std::format!(
            "def {name}Mutations : Nat := {}\n\n",
            snapshot.completion_mutations,
        ));
    }
    for (name, (header, payload)) in [("loanWire", loan), ("returnWire", returned)] {
        source.push_str(&std::format!(
            "def {name} : Hibana.OwnedWireObservation := {{ session := {}, lane := {}, \
             sender := {}, receiver := {}, frameLabel := {}, payload := {:?} }}\n\n",
            sid.raw(),
            header.lane(),
            header.source_role(),
            header.target_role(),
            header.label().raw(),
            payload,
        ));
    }
    let before = snapshots
        .iter()
        .find(|row| row.0 == "workerBeforeEof")
        .unwrap()
        .1;
    let event = before
        .steps
        .iter()
        .position(|step| matches!(step.action, LocalAction::Recv { .. }) && step.view == 1)
        .unwrap();
    source.push_str(&std::format!(
        "def loanChannel : Hibana.TransportChannel := {{ session := {}, generation := 0, \
         lane := {}, sender := 0, receiver := 1 }}\n\n\
         def actualEof : Hibana.ActorClosureObservation := {{ channel := loanChannel, event := {event}, \
         before := workerBeforeEof, after := workerAfterEof }}\n\n\
         theorem actualClosureCheck : actualEof.check workerImage closureChoreo {} = true := by decide\n",
        sid.raw(), loan.0.lane(), sid.raw(),
    ));
    let snapshot = |name: &str| snapshots.iter().find(|row| row.0 == name).unwrap().1;
    for (name, before, after, view, wire) in [
        ("loanSend", "ownerPending", "ownerSent", 0, "loanWire"),
        ("loanRecv", "workerBefore", "workerReceived", 1, "loanWire"),
        ("returnSend", "workerPending", "workerSent", 1, "returnWire"),
        ("returnRecv", "ownerSent", "ownerReturned", 0, "returnWire"),
    ] {
        let changed: std::vec::Vec<_> = snapshot(before)
            .steps
            .iter()
            .zip(&snapshot(after).steps)
            .enumerate()
            .filter_map(|(index, (old, new))| (old.done != new.done).then_some(index))
            .collect();
        assert_eq!(changed.len(), 1);
        let event = changed[0];
        source.push_str(&std::format!(
            "def {name} : Hibana.ActorCommitRecord := {{ wire := {wire}, step := {{ pathIndex := 0, \
             event := {event}, view := {view}, before := {before}, after := {after} }} }}\n\n",
        ));
    }
    for (index, name, observed, wire) in [
        (0, "loanRequest", "ownerBefore", loan),
        (4, "returnRequest", "workerReceived", returned),
    ] {
        let OwnedFutureBoundary::Seal {
            carrier,
            payload,
            label,
            region,
        } = captured.0[index]
        else {
            panic!("actual constructor boundary");
        };
        assert_eq!(carrier.actor_address, payload.actor_address);
        let actor = snapshot(observed);
        for identity in [carrier, payload] {
            let view = actor
                .views
                .iter()
                .find(|view| view.role == identity.role)
                .unwrap();
            assert_eq!((identity.run, identity.nonce), (view.run, view.nonce));
        }
        let bounds = region.bounds();
        let evidence = captured.1[index / 4];
        assert_eq!(evidence.header, wire.0);
        assert_eq!(evidence.payload(), wire.1);
        source.push_str(&std::format!(
            "def {name} : Hibana.OwnedCallHistory.Request := {{ carrier := ({actor}, {}, {}, {}), \
             payload := ({actor}, {}, {}, {}), label := {label}, borrow := ({}, {}) }}\n\n\
             def {name}Witness : Nat := {}\n\n",
            carrier.role,
            carrier.run,
            carrier.nonce,
            payload.role,
            payload.run,
            payload.nonce,
            bounds.start,
            bounds.end,
            evidence.witness,
            actor = actor.actor,
        ));
    }
    source.push_str(include_str!("closure.lean"));
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/lean-proof");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("OwnedClosureGenerated.lean"), source).unwrap();
}
