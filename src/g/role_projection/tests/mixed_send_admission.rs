//! Ordinary and owned messages share one FIFO through independent local reentry.
use super::distributed_source::{FrameBoundary, OperationObservation};
use super::*;
#[cfg(not(miri))]
use crate::endpoint::proof_snapshot::ActorSnapshot;
use crate::{
    endpoint::proof_snapshot::{actor_admissions, actor_snapshot},
    runtime::{SessionKitStorage, ids::SessionId},
    test_support::transport::TestTransport,
};
use std::cell::RefCell;

type Note = g::Msg<9, u32>;
type BorrowBody = g::Seq<g::Send<0, 1, Loan>, g::Seq<g::Send<0, 1, Note>, g::Send<1, 0, Loan>>>;
pub(super) type Steps =
    g::Seq<g::Roll<BorrowBody>, g::Seq<g::Send<10, 3, Read>, g::Send<3, 10, Value>>>;
#[cfg(not(miri))]
type Wire = (crate::transport::FrameHeader, std::vec::Vec<u8>);

#[test]
fn ordinary_and_owned_acceptances_keep_the_same_fifo_through_reentry() {
    let () = g::ProgramProjection::<Steps, 16>::VALIDATION;
    let owner_program = crate::runtime::program::project::<0>(&g::Program::<Steps>::new());
    let worker_program = crate::runtime::program::project::<1>(&g::Program::<Steps>::new());
    let db_program = crate::runtime::program::project::<10>(&g::Program::<Steps>::new());
    let service_program = crate::runtime::program::project::<3>(&g::Program::<Steps>::new());
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            let network = TestTransport::new();
            let mut owner_slab = [0; 32 * 1024];
            let mut worker_slab = [0; 32 * 1024];
            let mut service_slab = [0; 32 * 1024];
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
            let sid = SessionId::new(9062);
            let mut owner = owner_rv.enter(sid, &owner_program).unwrap();
            let mut db = owner_rv.enter(sid, &db_program).unwrap();
            let mut worker = worker_rv.enter(sid, &worker_program).unwrap();
            let mut service = service_rv.enter(sid, &service_program).unwrap();
            let operations = RefCell::new(std::vec::Vec::new());
            // SAFETY: all three pinned actors were attached with TestTransport.
            let initial = unsafe {
                [
                    actor_snapshot(&owner),
                    actor_snapshot(&worker),
                    actor_snapshot(&service),
                ]
            };
            let (sends, receipts, ()) =
                tokio::time::timeout(core::time::Duration::from_secs(30), async {
                    tokio::join!(
                        async {
                            let mut sends = std::vec::Vec::new();
                            for number in [7, 9] {
                                // SAFETY: this pinned actor was attached with TestTransport.
                                let (before_loan, loan_admission) =
                                    unsafe { (actor_snapshot(&owner), actor_admissions(&owner)) };
                                owner.send::<Loan>(db).await.unwrap();
                                operations.borrow_mut().push(OperationObservation {
                                    view: 0,
                                    label: 1,
                                    before: before_loan.clone(),
                                    after: unsafe { actor_snapshot(&owner) },
                                    frame: FrameBoundary::Sent(network.accepted_frame(1, 0)),
                                });
                                let loan_wire = network.queued_frame_at(1, 0);
                                let (before_note, note_admission) =
                                    unsafe { (actor_snapshot(&owner), actor_admissions(&owner)) };
                                owner.send::<Note>(&number).await.unwrap();
                                operations.borrow_mut().push(OperationObservation {
                                    view: 0,
                                    label: 9,
                                    before: before_note.clone(),
                                    after: unsafe { actor_snapshot(&owner) },
                                    frame: FrameBoundary::Sent(network.accepted_frame(1, 1)),
                                });
                                // Both frames are accepted before this executor polls WORKER.
                                // Inspection neither consumes them nor authorizes a receive.
                                let note_wire = network.queued_frame_at(1, 1);
                                assert_eq!(loan_wire.0.lane(), note_wire.0.lane());
                                sends.push((before_loan, loan_wire, loan_admission));
                                sends.push((before_note, note_wire, note_admission));
                                let before = unsafe { actor_snapshot(&owner) };
                                db = owner.recv::<Loan>().await.unwrap();
                                operations.borrow_mut().push(OperationObservation {
                                    view: 0,
                                    label: 1,
                                    before,
                                    after: unsafe { actor_snapshot(&owner) },
                                    frame: FrameBoundary::Received(
                                        network.received_frame(0, sid, 0),
                                    ),
                                });
                            }
                            let before = unsafe { actor_snapshot(&db) };
                            db.send::<Read>(&41).await.unwrap();
                            operations.borrow_mut().push(OperationObservation {
                                view: 10,
                                label: 3,
                                before,
                                after: unsafe { actor_snapshot(&db) },
                                frame: FrameBoundary::Sent(network.accepted_frame(3, 0)),
                            });
                            let before = unsafe { actor_snapshot(&db) };
                            assert_eq!(db.recv::<Value>().await.unwrap(), 42);
                            operations.borrow_mut().push(OperationObservation {
                                view: 10,
                                label: 4,
                                before,
                                after: unsafe { actor_snapshot(&db) },
                                frame: FrameBoundary::Received(network.received_frame(10, sid, 0)),
                            });
                            sends
                        },
                        async {
                            let mut receipts = std::vec::Vec::new();
                            for number in [7, 9] {
                                // SAFETY: WORKER has its own pinned TestTransport actor.
                                let before = unsafe { actor_snapshot(&worker) };
                                let db = worker.recv::<Loan>().await.unwrap();
                                let after = unsafe { actor_snapshot(&worker) };
                                operations.borrow_mut().push(OperationObservation {
                                    view: 1,
                                    label: 1,
                                    before: before.clone(),
                                    after: after.clone(),
                                    frame: FrameBoundary::Received(
                                        network.received_frame(1, sid, 0),
                                    ),
                                });
                                let note_before = unsafe { actor_snapshot(&worker) };
                                assert_eq!(worker.recv::<Note>().await.unwrap(), number);
                                operations.borrow_mut().push(OperationObservation {
                                    view: 1,
                                    label: 9,
                                    before: note_before,
                                    after: unsafe { actor_snapshot(&worker) },
                                    frame: FrameBoundary::Received(
                                        network.received_frame(1, sid, 0),
                                    ),
                                });
                                let return_before = unsafe { actor_snapshot(&worker) };
                                worker.send::<Loan>(db).await.unwrap();
                                operations.borrow_mut().push(OperationObservation {
                                    view: 1,
                                    label: 1,
                                    before: return_before,
                                    after: unsafe { actor_snapshot(&worker) },
                                    frame: FrameBoundary::Sent(network.accepted_frame(0, 0)),
                                });
                                receipts.push((before, after));
                            }
                            receipts
                        },
                        async {
                            let before = unsafe { actor_snapshot(&service) };
                            let number = service.recv::<Read>().await.unwrap();
                            operations.borrow_mut().push(OperationObservation {
                                view: 3,
                                label: 3,
                                before,
                                after: unsafe { actor_snapshot(&service) },
                                frame: FrameBoundary::Received(network.received_frame(3, sid, 0)),
                            });
                            let before = unsafe { actor_snapshot(&service) };
                            service.send::<Value>(&(number + 1)).await.unwrap();
                            operations.borrow_mut().push(OperationObservation {
                                view: 3,
                                label: 4,
                                before,
                                after: unsafe { actor_snapshot(&service) },
                                frame: FrameBoundary::Sent(network.accepted_frame(10, 0)),
                            });
                        },
                    )
                })
                .await
                .expect("mixed source FIFO stopped progressing");
            assert!(network.queue_is_empty());
            assert_eq!(sends.len(), 4);
            assert_eq!(receipts.len(), 2);
            assert_eq!(sends[0].1.1, sends[2].1.1);
            assert_ne!(receipts[0].1.views, receipts[1].1.views);
            let operations = operations.into_inner();
            super::distributed_source::check_observations(&operations);
            for actor in &initial {
                assert_eq!(
                    &operations
                        .iter()
                        .find(|op| op.before.actor == actor.actor)
                        .unwrap()
                        .before,
                    actor
                );
            }
            #[cfg(not(miri))]
            {
                write_observations(&sends, &receipts);
                let candidates = sends
                    .iter()
                    .map(|(before, _, admitted)| (before.clone(), admitted.clone()))
                    .collect::<std::vec::Vec<_>>();
                super::distributed_source::write_observations(&initial, &operations, &candidates);
            }
        });
}

#[cfg(not(miri))]
fn write_observations(
    sends: &[(ActorSnapshot, Wire, std::vec::Vec<bool>)],
    receipts: &[(ActorSnapshot, ActorSnapshot)],
) {
    use crate::test_support::{lean_choreo::LeanChoreo, lean_descriptor::descriptor_source};
    let mut source = std::format!(
        "import Hibana.FifoOccurrenceOrigin\nimport Hibana.ActorOccurrenceHistory\n\nset_option maxRecDepth 10000\n\n\
         def mixedSendChoreo : Hibana.Choreo := {}\n\n",
        Steps::lean_source(),
    );
    for (name, image) in [
        (
            "mixedOwnerImage",
            &RoleProjection::<0, Steps, 16>::IMAGE_REF,
        ),
        (
            "mixedWorkerImage",
            &RoleProjection::<1, Steps, 16>::IMAGE_REF,
        ),
    ] {
        source.push_str(&std::format!(
            "def {name} : Hibana.RustDescriptorImage := {}\n\n",
            descriptor_source(RoleDescriptorRef::from_resident(image))
        ));
    }
    for (index, (before, (header, payload), _admitted)) in sends.iter().enumerate() {
        let label = if index % 2 == 0 { 1 } else { 9 };
        let event = before
            .steps
            .iter()
            .position(|step| {
                matches!(step.action,
            LocalAction::Send { label: found, .. } if found == label)
            })
            .unwrap();
        source.push_str(&std::format!(
            "def mixedBefore{index} : Hibana.ActorProgressObservation := {}\n\n\
             def mixedWire{index} : Hibana.OwnedWireObservation := {{ session := {}, lane := {}, sender := {}, \
             receiver := {}, frameLabel := {}, payload := {payload:?} }}\n\n",
            super::commit::observation_source(before), header.session().raw(), header.lane(),
            header.source_role(), header.target_role(), header.label().raw(),
        ));
        let base = if let Some(scope) = before.steps[event].reentry_scope {
            std::format!(
                "((Hibana.prepareObservedActorBody? mixedOwnerImage mixedSendChoreo \
                mixedBefore{index} 0 0 {scope} {event}).get (by decide)).2"
            )
        } else {
            std::format!("mixedBefore{index}")
        };
        source.push_str(&std::format!(
            "def mixedSend{index} : Hibana.SourceSendAdmission := {{ image := mixedOwnerImage, \
             base := {base}, pathIndex := 0, event := {event}, wire := mixedWire{index} }}\n\n\
             theorem mixedSend{index}IsSourceChecked : mixedSend{index}.check mixedSendChoreo 9062 = true := by decide\n\n\
             def mixedEvent{index} := mixedSend{index}.effIndex?.get (by decide)\n\n",
        ));
    }
    for (visit, (before, after)) in receipts.iter().enumerate() {
        let event = before
            .steps
            .iter()
            .position(|step| matches!(step.action, LocalAction::Recv { label: 1, .. }))
            .unwrap();
        source.push_str(&std::format!(
            "def mixedReceiverBefore{visit} : Hibana.ActorProgressObservation := {}\n\n\
             def mixedReceiverAfter{visit} : Hibana.ActorProgressObservation := {}\n\n",
            super::commit::observation_source(before),
            super::commit::observation_source(after),
        ));
        let base = if let Some(scope) = before.steps[event].reentry_scope {
            std::format!(
                "((Hibana.prepareObservedActorBody? mixedWorkerImage mixedSendChoreo \
                mixedReceiverBefore{visit} 0 0 {scope} {event}).get (by decide)).2"
            )
        } else {
            std::format!("mixedReceiverBefore{visit}")
        };
        source.push_str(&std::format!(
            "def mixedReceipt{visit} : Hibana.ActorCommitObservation := {{ pathIndex := 0, event := {event}, \
             view := 1, before := {base}, after := mixedReceiverAfter{visit} }}\n\n\
             theorem mixedReceipt{visit}IsSourceChecked : mixedReceipt{visit}.ownedCheck mixedWorkerImage mixedSendChoreo \
             mixedWire{} 9062 = true := by decide\n\n",
            visit * 2,
        ));
    }
    source.push_str(r#"

def mixedChannel : Hibana.TransportChannel := {
  session := 9062, generation := 0, lane := mixedWire0.lane,
  sender := mixedWire0.sender, receiver := mixedWire0.receiver }
def mixedCarrier0 := Hibana.TransportState.initial mixedChannel
def mixedGhost0 := Hibana.ElasticAdmissionHistory.initial mixedChannel
def mixedRecords0 : List (Hibana.SourceSendAdmission × Nat) := []
theorem mixedPrefix0 : Hibana.SendAdmissionTrace mixedSendChoreo 9062 mixedCarrier0 mixedGhost0 mixedRecords0 :=
  .initial mixedChannel

def mixedCarrier1 := (mixedCarrier0.send ⟨mixedWire0.frameLabel, by decide⟩).get (by decide)
def mixedGhost1 := mixedGhost0.appendOccurrence mixedEvent0
def mixedRecords1 := mixedRecords0 ++ [(mixedSend0, mixedEvent0)]
theorem mixedPrefix1 : Hibana.SendAdmissionTrace mixedSendChoreo 9062 mixedCarrier1 mixedGhost1 mixedRecords1 :=
  .accept (transport := mixedCarrier0) (sent := mixedCarrier1) (history := mixedGhost0) (records := mixedRecords0)
    (sample := mixedSend0) (eventId := mixedEvent0) (label := ⟨mixedWire0.frameLabel, by decide⟩)
    mixedPrefix0 mixedSend0IsSourceChecked (by decide) (by decide) ⟨rfl, rfl, rfl, rfl, rfl⟩

def mixedCarrier2 := (mixedCarrier1.send ⟨mixedWire1.frameLabel, by decide⟩).get (by decide)
def mixedGhost2 := mixedGhost1.appendOccurrence mixedEvent1
def mixedRecords2 := mixedRecords1 ++ [(mixedSend1, mixedEvent1)]
theorem mixedPrefix2 : Hibana.SendAdmissionTrace mixedSendChoreo 9062 mixedCarrier2 mixedGhost2 mixedRecords2 :=
  .accept (transport := mixedCarrier1) (sent := mixedCarrier2) (history := mixedGhost1) (records := mixedRecords1)
    (sample := mixedSend1) (eventId := mixedEvent1) (label := ⟨mixedWire1.frameLabel, by decide⟩)
    mixedPrefix1 mixedSend1IsSourceChecked (by decide) (by decide) ⟨rfl, rfl, rfl, rfl, rfl⟩

def mixedCarrierFirstLoanReceived := { mixedCarrier2 with delivered := 1 }
def mixedGhostFirstLoanReceived := { mixedGhost2 with delivered := 1 }
theorem mixedFirstLoanWasDelivered :
    Hibana.SendAdmissionTrace mixedSendChoreo 9062 mixedCarrierFirstLoanReceived mixedGhostFirstLoanReceived mixedRecords2 :=
  .receive (transport := mixedCarrier2) (history := mixedGhost2)
    (frame := (mixedCarrier2.frames[0]?).get (by decide)) (key := (mixedGhost2.accepted[0]?).get (by decide))
    mixedPrefix2 (by decide) rfl

def mixedCarrierFirstComplete := { mixedCarrier2 with delivered := 2 }
def mixedGhostFirstComplete := { mixedGhost2 with delivered := 2 }
theorem mixedFirstVisitWasDelivered :
    Hibana.SendAdmissionTrace mixedSendChoreo 9062 mixedCarrierFirstComplete mixedGhostFirstComplete mixedRecords2 :=
  .receive (transport := mixedCarrierFirstLoanReceived) (history := mixedGhostFirstLoanReceived)
    (frame := (mixedCarrier2.frames[1]?).get (by decide)) (key := (mixedGhost2.accepted[1]?).get (by decide))
    mixedFirstLoanWasDelivered (by decide) rfl

def mixedCarrier3 := (mixedCarrierFirstComplete.send ⟨mixedWire2.frameLabel, by decide⟩).get (by decide)
def mixedGhost3 := mixedGhostFirstComplete.appendOccurrence mixedEvent2
def mixedRecords3 := mixedRecords2 ++ [(mixedSend2, mixedEvent2)]
theorem mixedPrefix3 : Hibana.SendAdmissionTrace mixedSendChoreo 9062 mixedCarrier3 mixedGhost3 mixedRecords3 :=
  .accept (transport := mixedCarrierFirstComplete) (sent := mixedCarrier3) (history := mixedGhostFirstComplete) (records := mixedRecords2)
    (sample := mixedSend2) (eventId := mixedEvent2) (label := ⟨mixedWire2.frameLabel, by decide⟩)
    mixedFirstVisitWasDelivered mixedSend2IsSourceChecked (by decide) (by decide) ⟨rfl, rfl, rfl, rfl, rfl⟩

def mixedCarrier4 := (mixedCarrier3.send ⟨mixedWire3.frameLabel, by decide⟩).get (by decide)
def mixedGhost4 := mixedGhost3.appendOccurrence mixedEvent3
def mixedRecords4 := mixedRecords3 ++ [(mixedSend3, mixedEvent3)]
theorem mixedPrefix4 : Hibana.SendAdmissionTrace mixedSendChoreo 9062 mixedCarrier4 mixedGhost4 mixedRecords4 :=
  .accept (transport := mixedCarrier3) (sent := mixedCarrier4) (history := mixedGhost3) (records := mixedRecords3)
    (sample := mixedSend3) (eventId := mixedEvent3) (label := ⟨mixedWire3.frameLabel, by decide⟩)
    mixedPrefix3 mixedSend3IsSourceChecked (by decide) (by decide) ⟨rfl, rfl, rfl, rfl, rfl⟩

theorem mixedTraceCoversAllFourActualAcceptedFrames :
    mixedRecords4.length = mixedCarrier4.frames.length :=
  Hibana.a_checked_source_admission_trace_records_every_frame mixedPrefix4

theorem mixedOrdinaryFrameCannotBeOmitted :
    ¬Hibana.SendAdmissionTrace mixedSendChoreo 9062 mixedCarrier4 mixedGhost4
      [(mixedSend0, mixedEvent0), (mixedSend2, mixedEvent2), (mixedSend3, mixedEvent3)] := by
  intro fake
  have size := Hibana.a_checked_source_admission_trace_records_every_frame fake
  change 3 = 4 at size
  cases size

theorem mixedSourceInstancesIncludeTheOrdinaryFrames :
    mixedGhost4.accepted = [⟨mixedEvent0, 0⟩, ⟨mixedEvent1, 0⟩, ⟨mixedEvent0, 1⟩, ⟨mixedEvent1, 1⟩] ∧
      mixedGhost4.delivered = 2 := by decide

theorem mixedOrdinaryFramesHaveNoOwnedResource :
    mixedSend1.ownedFacts? mixedSendChoreo = none ∧ mixedSend3.ownedFacts? mixedSendChoreo = none := by
  constructor <;> rfl

def mixedFreshSource := (mixedSend2.ownedFacts? mixedSendChoreo).get (by decide)
def mixedFreshReceiptFacts := (mixedReceipt1.ownedFacts? mixedWorkerImage mixedSendChoreo).get (by decide)
theorem mixedFreshBytesBindTheActualReceiverWithoutSenderPublication :
    mixedFreshSource.resource = mixedFreshReceiptFacts.resource ∧
      mixedFreshSource.effIndex = mixedFreshReceiptFacts.effIndex :=
  Hibana.accepted_owned_source_bytes_identify_the_checked_receiver
    mixedSend2IsSourceChecked
    ((Option.some_get (x := mixedSend2.ownedFacts? mixedSendChoreo) (by decide)).symm)
    mixedReceipt1IsSourceChecked
    ((Option.some_get (x := mixedReceipt1.ownedFacts? mixedWorkerImage mixedSendChoreo) (by decide)).symm)
    rfl

theorem mixedActualFreshReceiveHasItsExactAcceptedOccurrence :
    ∃ sample eventId key following,
      mixedRecords4[mixedCarrier4.delivered]? = some (sample, eventId) ∧
      mixedGhost4.accepted[mixedCarrier4.delivered]? = some key ∧
      sample.check mixedSendChoreo 9062 = true ∧ sample.effIndex? = some eventId ∧
      sample.wire.identifies ((mixedCarrier4.frames[2]?).get (by decide)) ∧ key.globalId = eventId ∧
      ((mixedCarrier4.frames[2]?).get (by decide)).sequence = mixedGhost4.delivered ∧
      key.iteration < mixedGhost4.nextIteration key.globalId ∧
      mixedGhost4.receive? = some (key, following) ∧
      Hibana.SendAdmissionTrace mixedSendChoreo 9062 ({ mixedCarrier4 with delivered := 3 }) following mixedRecords4 :=
  Hibana.a_fifo_receive_has_its_actual_accepted_source_occurrence mixedPrefix4 (by decide)

theorem mixedEveryOccurrenceHasItsPhysicalPrefixOrdinal :
    ∀ index record key,
      mixedRecords4[index]? = some record → mixedGhost4.accepted[index]? = some key →
      key.globalId = record.2 ∧
        key.iteration = Hibana.sourceOccurrenceOrdinal mixedRecords4 index record.2 :=
  Hibana.an_accepted_source_occurrence_has_its_exact_fifo_prefix_ordinal mixedPrefix4

theorem mixedCheckedFreshReceiptBindsItsActualSourceInstance :
    ∃ key following,
      mixedGhost4.accepted[mixedCarrier4.delivered]? = some key ∧
      key.globalId = mixedEvent2 ∧
      key.iteration = Hibana.sourceOccurrenceOrdinal mixedRecords4 mixedCarrier4.delivered mixedEvent2 ∧
      mixedSend2.effIndex? = some mixedEvent2 ∧
      mixedSend2.wire.identifies ((mixedCarrier4.frames[2]?).get (by decide)) ∧
      mixedFreshSource.resource = mixedFreshReceiptFacts.resource ∧
      mixedFreshSource.effIndex = mixedFreshReceiptFacts.effIndex ∧
      mixedGhost4.receive? = some (key, following) ∧
      Hibana.SendAdmissionTrace mixedSendChoreo 9062
        ({ mixedCarrier4 with delivered := 3 }) following mixedRecords4 :=
  Hibana.a_checked_owned_fifo_receipt_retains_its_actual_source_instance
    mixedPrefix4 (by decide) (by decide)
    ((Option.some_get (x := mixedSend2.ownedFacts? mixedSendChoreo) (by decide)).symm)
    mixedReceipt1IsSourceChecked
    ((Option.some_get (x := mixedReceipt1.ownedFacts? mixedWorkerImage mixedSendChoreo) (by decide)).symm)
    rfl

theorem mixedIdenticalLoanBytesDoNotIdentifyTheFreshReceiptAsTheFirstVisit :
    mixedWire0.payload = mixedWire2.payload ∧
      ¬∃ key, mixedGhost4.accepted[mixedCarrier4.delivered]? = some key ∧ key.iteration = 0 := by
  constructor
  · rfl
  · rintro ⟨key, keyAt, old⟩
    have ordinal := mixedEveryOccurrenceHasItsPhysicalPrefixOrdinal
      mixedCarrier4.delivered (mixedSend2, mixedEvent2) key (by decide) keyAt
    have exactOrdinal : key.iteration = 1 := ordinal.2
    omega
"#);
    let directory = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/lean-proof");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("MixedSendAdmissionGenerated.lean"), source).unwrap();
}
