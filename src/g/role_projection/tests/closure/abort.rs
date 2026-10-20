//! An accepted transfer can be isolated by transport failure. It grants no
//! receiver value and cannot be replenished by attaching the resource again.
use super::*;

#[test]
fn an_aborted_accepted_loan_is_fenced_without_a_receipt_or_reissued_authority() {
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
            let sid = SessionId::new(9061);
            let mut owner = owner_rv.enter(sid, &owner_program).unwrap();
            let mut worker = worker_rv.enter(sid, &worker_program).unwrap();
            let db = owner_rv.enter(sid, &db_program).unwrap();
            let owner_tag = view_tag(&owner);
            // SAFETY: the exact pinned actor/transport stay alive, between polls.
            let owner_before = unsafe { actor_snapshot(&owner) };
            let worker_before = unsafe { actor_snapshot(&worker) };
            let capture = Capture::start();
            network.pause_send(0);
            let owner_pending = {
                let mut future = pin!(owner.send::<Loan>(db));
                let mut cx = Context::from_waker(Waker::noop());
                assert!(future.as_mut().poll(&mut cx).is_pending());
                // SAFETY: the live future retains the pinned actor, between polls.
                let observed = unsafe { owner_tag.snapshot::<TestTransport>() };
                network.resume_send();
                assert!(matches!(future.as_mut().poll(&mut cx), Poll::Ready(Ok(()))));
                observed
            };
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
                ]
            ));
            // SAFETY: the sender has returned and its future has dropped normally.
            let owner_sent = unsafe { actor_snapshot(&owner) };
            let evidence = network.accepted_frame(1, 0);
            assert!(!network.queue_is_empty());
            network.abort_port(sid, 1, evidence.header.lane());
            assert!(network.queue_is_empty());
            assert!(worker.recv::<Loan>().await.is_err());
            // SAFETY: the failed call returned without creating a DB Endpoint.
            let worker_faulted = unsafe { actor_snapshot(&worker) };
            assert_eq!(
                worker_faulted.fault,
                Some(SessionFaultKind::TransportClosed)
            );
            assert_eq!(worker_before.steps, worker_faulted.steps);
            assert_eq!(
                worker_before.completion_mutations,
                worker_faulted.completion_mutations
            );
            assert_eq!(worker_before.counter, worker_faulted.counter);
            assert_eq!(worker_before.live_views, worker_faulted.live_views);
            assert_eq!(
                worker_before.view_headers().collect::<std::vec::Vec<_>>(),
                worker_faulted.view_headers().collect::<std::vec::Vec<_>>()
            );
            // Acceptance consumed the sender's actual Rust value. Neither closure
            // nor a failed attachment can create a replacement for it in this run.
            assert!(owner_rv.enter(sid, &db_program).is_err());
            // SAFETY: both original actors remain live and idle here.
            let owner_after = unsafe { actor_snapshot(&owner) };
            assert_eq!(owner_sent, owner_after);
            assert_eq!(owner_after.fault, None);
            #[cfg(not(miri))]
            write(
                sid,
                &[
                    ("ownerBefore", &owner_before),
                    ("ownerPending", &owner_pending),
                    ("ownerSent", &owner_sent),
                    ("workerBefore", &worker_before),
                    ("workerFaulted", &worker_faulted),
                ],
                (&captured, &evidence),
            );
            #[cfg(miri)]
            let _ = (owner_before, owner_pending, captured, evidence);
        });
}

#[cfg(not(miri))]
fn write(
    sid: SessionId,
    snapshots: &[(&str, &crate::endpoint::proof_snapshot::ActorSnapshot)],
    captured: (
        &[OwnedFutureBoundary],
        &crate::test_support::transport::FrameEvidence,
    ),
) {
    use crate::test_support::{lean_choreo::LeanChoreo, lean_descriptor::descriptor_source};
    let mut source = std::format!(
        "import Hibana.SourceOccurrenceTrace\nimport Hibana.OwnedCallHistory\n\n\
         def abortChoreo : Hibana.Choreo := {}\n\n\
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
            "def {name} : Hibana.ActorProgressObservation := {}\n\n\
             def {name}Mutations : Nat := {}\n\n",
            super::super::commit::observation_source(snapshot),
            snapshot.completion_mutations,
        ));
    }
    let wire = captured.1;
    source.push_str(&std::format!(
        "def loanWire : Hibana.OwnedWireObservation := {{ session := {}, lane := {}, \
         sender := {}, receiver := {}, frameLabel := {}, payload := {:?} }}\n\n\
         def loanWitness : Nat := {}\n\n",
        sid.raw(),
        wire.header.lane(),
        wire.header.source_role(),
        wire.header.target_role(),
        wire.header.label().raw(),
        wire.payload(),
        wire.witness,
    ));
    let snapshot = |name: &str| snapshots.iter().find(|row| row.0 == name).unwrap().1;
    let before = snapshot("ownerBefore");
    let OwnedFutureBoundary::Seal {
        carrier,
        payload,
        label,
        region,
    } = captured.0[0]
    else {
        panic!("actual loan constructor");
    };
    assert_eq!(carrier.actor_address, payload.actor_address);
    for identity in [carrier, payload] {
        let view = before
            .views
            .iter()
            .find(|view| view.role == identity.role)
            .unwrap();
        assert_eq!((identity.run, identity.nonce), (view.run, view.nonce));
    }
    let bounds = region.bounds();
    let changed: std::vec::Vec<_> = snapshot("ownerPending")
        .steps
        .iter()
        .zip(&snapshot("ownerSent").steps)
        .enumerate()
        .filter_map(|(index, (old, new))| (old.done != new.done).then_some(index))
        .collect();
    assert_eq!(changed.len(), 1);
    source.push_str(&std::format!(
        "def loanRequest : Hibana.OwnedCallHistory.Request := {{ carrier := (0, {}, {}, {}), \
         payload := (0, {}, {}, {}), label := {label}, borrow := ({}, {}) }}\n\n\
         def loanSend : Hibana.ActorCommitRecord := {{ wire := loanWire, step := {{ pathIndex := 0, \
         event := {}, view := 0, before := ownerPending, after := ownerSent }} }}\n\n",
        carrier.role, carrier.run, carrier.nonce, payload.role, payload.run, payload.nonce,
        bounds.start, bounds.end, changed[0],
    ));
    source.push_str(include_str!("abort.lean"));
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/lean-proof");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("OwnedAbortGenerated.lean"), source).unwrap();
}
