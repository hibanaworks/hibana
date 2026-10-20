//! One logical FIFO continues across the two physical holders of its view.
use super::distributed_source::{FrameBoundary, OperationObservation};
use super::*;
use crate::{
    endpoint::proof_snapshot::actor_snapshot,
    runtime::{SessionKitStorage, ids::SessionId},
    test_support::transport::TestTransport,
};
use std::cell::RefCell;

type First = g::Msg<31, u32>;
type FirstValue = g::Msg<32, u32>;
type Second = g::Msg<33, u32>;
type SecondValue = g::Msg<34, u32>;
type Body = g::Seq<
    g::Send<10, 3, First>,
    g::Seq<
        g::Send<3, 10, FirstValue>,
        g::Seq<
            g::Send<0, 1, Loan>,
            g::Seq<
                g::Send<10, 3, Second>,
                g::Seq<g::Send<3, 10, SecondValue>, g::Send<1, 0, Loan>>,
            >,
        >,
    >,
>;
type Steps = g::Roll<Body>;

#[test]
fn a_logical_wire_word_keeps_its_order_when_the_sender_changes_holder() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            let () = g::ProgramProjection::<Steps, 16>::VALIDATION;
            let owner_program = crate::runtime::program::project::<0>(&g::Program::<Steps>::new());
            let worker_program = crate::runtime::program::project::<1>(&g::Program::<Steps>::new());
            let db_program = crate::runtime::program::project::<10>(&g::Program::<Steps>::new());
            let service_program = crate::runtime::program::project::<3>(&g::Program::<Steps>::new());
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
            let sid = SessionId::new(9064);
            let mut owner = owner_rv.enter(sid, &owner_program).unwrap();
            let mut db = owner_rv.enter(sid, &db_program).unwrap();
            let mut worker = worker_rv.enter(sid, &worker_program).unwrap();
            let mut service = service_rv.enter(sid, &service_program).unwrap();
            // SAFETY: these three pinned actors all use TestTransport. The copied
            // observations cannot issue an Endpoint or authorize its next operation.
            let initial = unsafe {
                [
                    actor_snapshot(&owner),
                    actor_snapshot(&worker),
                    actor_snapshot(&service),
                ]
            };
            let operations = RefCell::new(std::vec::Vec::new());
            tokio::time::timeout(core::time::Duration::from_secs(30), async {
                tokio::join!(
                    async {
                        for number in [10, 20] {
                            let before = unsafe { actor_snapshot(&db) };
                            db.send::<First>(&number).await.unwrap();
                            operations.borrow_mut().push(OperationObservation {
                                view: 10,
                                label: 31,
                                before,
                                after: unsafe { actor_snapshot(&db) },
                                frame: FrameBoundary::Sent(network.accepted_frame(3, 0)),
                            });
                            let before = unsafe { actor_snapshot(&db) };
                            assert_eq!(db.recv::<FirstValue>().await.unwrap(), number + 1);
                            operations.borrow_mut().push(OperationObservation {
                                view: 10,
                                label: 32,
                                before,
                                after: unsafe { actor_snapshot(&db) },
                                frame: FrameBoundary::Received(network.received_frame(10, sid, 0)),
                            });
                            let before = unsafe { actor_snapshot(&owner) };
                            owner.send::<Loan>(db).await.unwrap();
                            operations.borrow_mut().push(OperationObservation {
                                view: 0,
                                label: 1,
                                before,
                                after: unsafe { actor_snapshot(&owner) },
                                frame: FrameBoundary::Sent(network.accepted_frame(1, 0)),
                            });
                            let before = unsafe { actor_snapshot(&owner) };
                            db = owner.recv::<Loan>().await.unwrap();
                            operations.borrow_mut().push(OperationObservation {
                                view: 0,
                                label: 1,
                                before,
                                after: unsafe { actor_snapshot(&owner) },
                                frame: FrameBoundary::Received(network.received_frame(0, sid, 0)),
                            });
                        }
                    },
                    async {
                        for number in [110, 120] {
                            let before = unsafe { actor_snapshot(&worker) };
                            let mut db = worker.recv::<Loan>().await.unwrap();
                            operations.borrow_mut().push(OperationObservation {
                                view: 1,
                                label: 1,
                                before,
                                after: unsafe { actor_snapshot(&worker) },
                                frame: FrameBoundary::Received(network.received_frame(1, sid, 0)),
                            });
                            let before = unsafe { actor_snapshot(&db) };
                            db.send::<Second>(&number).await.unwrap();
                            operations.borrow_mut().push(OperationObservation {
                                view: 10,
                                label: 33,
                                before,
                                after: unsafe { actor_snapshot(&db) },
                                frame: FrameBoundary::Sent(network.accepted_frame(3, 0)),
                            });
                            let before = unsafe { actor_snapshot(&db) };
                            assert_eq!(db.recv::<SecondValue>().await.unwrap(), number + 1);
                            operations.borrow_mut().push(OperationObservation {
                                view: 10,
                                label: 34,
                                before,
                                after: unsafe { actor_snapshot(&db) },
                                frame: FrameBoundary::Received(network.received_frame(10, sid, 0)),
                            });
                            let before = unsafe { actor_snapshot(&worker) };
                            worker.send::<Loan>(db).await.unwrap();
                            operations.borrow_mut().push(OperationObservation {
                                view: 1,
                                label: 1,
                                before,
                                after: unsafe { actor_snapshot(&worker) },
                                frame: FrameBoundary::Sent(network.accepted_frame(0, 0)),
                            });
                        }
                    },
                    async {
                        for number in [10, 20] {
                            let before = unsafe { actor_snapshot(&service) };
                            assert_eq!(service.recv::<First>().await.unwrap(), number);
                            operations.borrow_mut().push(OperationObservation {
                                view: 3,
                                label: 31,
                                before,
                                after: unsafe { actor_snapshot(&service) },
                                frame: FrameBoundary::Received(network.received_frame(3, sid, 0)),
                            });
                            let before = unsafe { actor_snapshot(&service) };
                            service.send::<FirstValue>(&(number + 1)).await.unwrap();
                            operations.borrow_mut().push(OperationObservation {
                                view: 3,
                                label: 32,
                                before,
                                after: unsafe { actor_snapshot(&service) },
                                frame: FrameBoundary::Sent(network.accepted_frame(10, 0)),
                            });
                            let before = unsafe { actor_snapshot(&service) };
                            assert_eq!(service.recv::<Second>().await.unwrap(), number + 100);
                            operations.borrow_mut().push(OperationObservation {
                                view: 3,
                                label: 33,
                                before,
                                after: unsafe { actor_snapshot(&service) },
                                frame: FrameBoundary::Received(network.received_frame(3, sid, 0)),
                            });
                            let before = unsafe { actor_snapshot(&service) };
                            service.send::<SecondValue>(&(number + 101)).await.unwrap();
                            operations.borrow_mut().push(OperationObservation {
                                view: 3,
                                label: 34,
                                before,
                                after: unsafe { actor_snapshot(&service) },
                                frame: FrameBoundary::Sent(network.accepted_frame(10, 0)),
                            });
                        }
                    },
                );
            })
            .await
            .expect("a handoff stranded the logical FIFO");
            assert!(network.queue_is_empty());
            let operations = operations.into_inner();
            super::distributed_source::check_observations(&operations);
            assert_eq!(operations.len(), 24);
            let requests: std::vec::Vec<_> = operations
                .iter()
                .filter(|operation| {
                    operation.view == 10 && matches!(operation.frame, FrameBoundary::Sent(_))
                })
                .collect();
            assert_eq!(requests.len(), 4);
            assert_eq!(
                requests
                    .iter()
                    .map(|operation| operation.before.actor)
                    .collect::<std::vec::Vec<_>>(),
                [0, 1, 0, 1]
            );
            for request in &requests[1..] {
                assert_eq!(request.frame().header, requests[0].frame().header);
            }
            #[cfg(not(miri))]
            {
                use std::fmt::Write;
                let mut source = super::distributed_source::execution_source::<Steps, 16>(
                    &initial,
                    &operations,
                    &std::vec![(0, 0); operations.len()],
                );
                source.insert_str(0, "import Hibana.SourceReceiptWord\nimport Hibana.SourceActorWord\n");
                for actor in [0, 1, 3] {
                    writeln!(source, r#"
private def logicalActorGroup{actor} : SourceOrderGroup distributedImage{actor} distributedChoreo := {{
  rows := List.range distributedImage{actor}.eventCount
  distinct := by decide
  data := distributedWordData{actor}
  rolls := distributedWordRolls{actor}
  choices := []
  decoded := (Option.some_get _).symm
  decodedRolls := (Option.some_get _).symm
  nonempty := by decide
  paths := by decide
  choiceMembers := by decide
  slots := by decide
  active := by decide
  ordered := by decide
  uniformReset := by decide
  outsideChoices := by decide
}}

theorem logicalActor{actor}WholeSourceClock
    {{cut : DistributedSourceCut}} {{entries : List SourceAcceptance}}
    {{records : List (Nat × ActorCommitRecord)}}
    (history : DistributedSourceExecution distributedImages distributedChoreo 9064
      (distributedCut0, distributedEntries0) (cut, entries) records) :
    sourceFrameWord (List.range distributedImage{actor}.eventCount)
      (committedFrameClassWord (List.range distributedImage{actor}.eventCount) {actor} records) :=
  (a_measured_distributed_class_has_its_source_word_and_exact_prefix
    (actor := {actor}) (group := logicalActorGroup{actor}) history (by decide)).1

theorem logicalActor{actor}OutstandingSourceClock
    {{cut : DistributedSourceCut}} {{entries : List SourceAcceptance}}
    {{records : List (Nat × ActorCommitRecord)}} {{entry : SourceAcceptance}}
    (history : DistributedSourceExecution distributedImages distributedChoreo 9064
      (distributedCut0, distributedEntries0) (cut, entries) records)
    (present : entry ∈ entries) (stamp : entry.stamps {actor} (cut.actors {actor}).2) :
    sourceFrameWord (List.range distributedImage{actor}.eventCount)
      (committedFrameClassWord (List.range distributedImage{actor}.eventCount) {actor} records ++
        if entry.sample.event ∈ List.range distributedImage{actor}.eventCount then [entry.sample.event] else []) :=
  (a_pending_acceptance_extends_its_current_actor_source_word
    (actor := {actor}) (group := logicalActorGroup{actor}) history rfl (by decide) present stamp).1

theorem logicalActor{actor}AcceptanceInclusiveClock
    {{cut : DistributedSourceCut}} {{entries : List SourceAcceptance}}
    {{records : List (Nat × ActorCommitRecord)}}
    (history : DistributedSourceExecution distributedImages distributedChoreo 9064
      (distributedCut0, distributedEntries0) (cut, entries) records) :
    ∃ pending : List SourceAcceptance,
      pending.length ≤ 1 ∧
      (∀ entry ∈ pending, entry ∈ entries ∧ entry.stamps {actor} (cut.actors {actor}).2) ∧
      let word := committedFrameClassWord (List.range distributedImage{actor}.eventCount) {actor} records ++
        pending.filterMap (fun entry => if entry.sample.event ∈ List.range distributedImage{actor}.eventCount
          then some entry.sample.event else none)
      sourceFrameWord (List.range distributedImage{actor}.eventCount) word ∧
      (∀ event ∈ List.range distributedImage{actor}.eventCount,
        distributedImage{actor}.sourceRowSide? event = some .send →
        word.count event = ((actorAcceptedSends entries {actor}).map (fun entry => entry.sample.event)).count event) ∧
      (∀ event ∈ List.range distributedImage{actor}.eventCount,
        distributedImage{actor}.sourceRowSide? event = some .receive →
        word.count event = (committedFrameClassWord (List.range distributedImage{actor}.eventCount) {actor} records).count event) :=
  a_distributed_actor_has_its_acceptance_inclusive_source_word
    (actor := {actor}) (group := logicalActorGroup{actor}) history rfl (by decide) (by decide)
"#).unwrap();
                }
                source.push_str(r#"
theorem logicalReceiptPrefixesAtEveryBoundary
    {cut : DistributedSourceCut} {entries : List SourceAcceptance}
    {records : List (Nat × ActorCommitRecord)}
    (history : DistributedSourceExecution distributedImages distributedChoreo 9064
      (distributedCut0, distributedEntries0) (cut, entries) records) :
    ∃ receipts : List SourceAcceptance,
      receivedSourceWord distributedImages distributedChoreo records = receipts.map SourceAcceptance.event ∧
      ∀ channel, sourceChannelEntries receipts channel =
        (sourceChannelEntries entries channel).take (cut.carriers channel).delivered :=
  a_distributed_receipt_word_is_the_exact_accepted_fifo_prefix history

theorem logicalDeliveredRequestWordKeepsBothHolders :
    ∃ receipts : List SourceAcceptance,
      receivedSourceWord distributedImages distributedChoreo distributedRecords =
        receipts.map SourceAcceptance.event ∧
      (sourceChannelEntries receipts distributedChannel0).map SourceAcceptance.event = [0, 3, 0, 3] := by
  obtain ⟨receipts, word, prefixes⟩ :=
    a_distributed_receipt_word_is_the_exact_accepted_fifo_prefix distributedActualExecution
  refine ⟨receipts, word, ?_⟩
  rw [prefixes distributedChannel0]
  decide

theorem logicalRequestsHaveBothPhysicalSenders :
    ((sourceChannelEntries distributedEntries36 distributedChannel0).map
      (fun entry => entry.sample.image.role)) = [0, 1, 0, 1] ∧
    ((sourceChannelRecords distributedEntries36 distributedChannel0).map
      (fun entry => entry.1.effIndex?)) = [some 0, some 3, some 0, some 3] := by decide

theorem logicalClassIsLargerThanEitherPhysicalSenderClass :
    (distributedImage0.frameClassRows distributedFrameKey0_0).map distributedImage0.eventEffIndex? = [some 0] ∧
    (distributedImage1.frameClassRows distributedFrameKey1_1).map distributedImage1.eventEffIndex? = [some 3] ∧
    (distributedImage3.frameClassRows distributedFrameKey3_0).map distributedImage3.eventEffIndex? = [some 0, some 3] := by decide
"#);
                writeln!(source, "\nprivate theorem logicalBeforeSecondHolderReceipt : DistributedSourceExecution distributedImages distributedChoreo 9064\n  (distributedCut0, distributedEntries0) (distributedCut28, distributedEntries28) (distributedRecords.take 18) := by\n  exact {}\n",
                    (1..=28).fold(".refl".into(), |proof, boundary| std::format!("(.tail {proof} distributedStep{boundary})"))).unwrap();
                source.push_str(r#"
theorem logicalSecondHolderHeadGetsItsOrdinalBeforePublication
    {frame : TransportFrame} {next : TransportState}
    (received : (distributedCut28.carriers distributedChannel0).pollReceive = .delivered frame next) :
    ∃ receipts : List SourceAcceptance, ∃ history : ElasticAdmissionHistory, ∃ key : ElasticOccurrenceKey,
      receivedSourceWord distributedImages distributedChoreo (distributedRecords.take 18) =
        receipts.map SourceAcceptance.event ∧
      SendAdmissionTrace distributedChoreo 9064 (distributedCut28.carriers distributedChannel0)
        history (sourceChannelRecords distributedEntries28 distributedChannel0) ∧
      history.accepted[3]? = some key ∧ key.globalId = 3 ∧ key.iteration = 1 ∧
      distributedSend18.wire.identifies frame := by
  obtain ⟨receipts, history, sample, event, key, word, deliveredPrefix, trace, keyAt, recordAt,
    checked, sourceAt, identity, globalAt, ordinal⟩ :=
    a_prospective_fifo_head_has_its_actual_receipt_word_ordinal logicalBeforeSecondHolderReceipt received
  have actualAt : (sourceChannelRecords distributedEntries28 distributedChannel0)[
      (distributedCut28.carriers distributedChannel0).delivered]? = some (distributedSend18, 3) := by decide
  have same := Option.some.inj (recordAt.symm.trans actualAt)
  have sampleSame : sample = distributedSend18 := congrArg Prod.fst same
  have eventSame : event = 3 := congrArg Prod.snd same
  have pastCount : ((sourceChannelEntries receipts distributedChannel0).map SourceAcceptance.event).count event = 1 := by
    rw [deliveredPrefix, eventSame]
    decide
  refine ⟨receipts, history, key, word, trace, keyAt, ?_, ordinal.trans pastCount, ?_⟩
  · exact globalAt.trans eventSame
  · exact sampleSame ▸ identity
"#);
                source.insert_str(0, "import Hibana.SourceHandoffWord\nimport Hibana.SourcePortWord\nimport Hibana.SourceOwnedReceive\n");
                source.push_str(include_str!("logical_handoff_word.lean"));
                source.push_str(include_str!("logical_port_word.lean"));
                source.push_str(include_str!("logical_source_message.lean"));
                let directory =
                    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/lean-proof");
                std::fs::create_dir_all(&directory).unwrap();
                std::fs::write(directory.join("LogicalViewWordGenerated.lean"), source).unwrap();
            }
            #[cfg(miri)]
            let _ = initial;
        });
}
