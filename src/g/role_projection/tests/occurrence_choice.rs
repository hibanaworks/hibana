//! One actual FIFO receipt carries a branch choice through changing rolled visits.
use super::distributed_source::{FrameBoundary, OperationObservation};
use super::*;
#[cfg(not(miri))]
use crate::endpoint::proof_snapshot::ActorSnapshot;
use crate::{
    endpoint::proof_snapshot::actor_snapshot,
    runtime::{
        SessionKitStorage,
        ids::SessionId,
        resolver::{DecisionArm, ResolverRef},
    },
    test_support::transport::TestTransport,
};
use core::cell::{Cell, RefCell};

type LeftNotice = g::Msg<8, ()>;
type RightNotice = g::Msg<9, ()>;
type LeftLoan = g::Msg<1, Endpoint<'static, 10>>;
type RightLoan = g::Msg<2, Endpoint<'static, 10>>;
type Return = g::Msg<5, Endpoint<'static, 10>>;
type Borrow = g::Roll<
    g::Resolve<
        g::Route<
            g::Seq<g::Send<0, 1, LeftLoan>, g::Send<1, 0, Return>>,
            g::Seq<g::Send<0, 1, RightLoan>, g::Send<1, 0, Return>>,
        >,
        972,
    >,
>;
type Steps = g::Seq<
    g::Resolve<
        g::Route<
            g::Seq<g::Send<0, 1, LeftNotice>, Borrow>,
            g::Seq<g::Send<0, 1, RightNotice>, Borrow>,
        >,
        971,
    >,
    g::Seq<g::Send<10, 3, Read>, g::Send<3, 10, Value>>,
>;
#[cfg(not(miri))]
type Wire = (crate::transport::FrameHeader, std::vec::Vec<u8>);

#[test]
fn changing_rolled_loans_teach_only_the_received_occurrences_choices() {
    let () = g::ProgramProjection::<Steps, 32>::VALIDATION;
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
            let number = Cell::new(7u32);
            let mut owner_slab = [0; 64 * 1024];
            let mut worker_slab = [0; 64 * 1024];
            let mut service_slab = [0; 64 * 1024];
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
            owner_rv
                .set_resolver(
                    &owner_program,
                    ResolverRef::<971>::decision_state(&number, |_| Ok(DecisionArm::Left)),
                )
                .unwrap();
            owner_rv
                .set_resolver(
                    &owner_program,
                    ResolverRef::<972>::decision_state(&number, |number| {
                        Ok(if number.get() % 2 == 1 {
                            DecisionArm::Left
                        } else {
                            DecisionArm::Right
                        })
                    }),
                )
                .unwrap();
            let session = SessionId::new(9063);
            let mut owner = owner_rv.enter(session, &owner_program).unwrap();
            let mut db = owner_rv.enter(session, &db_program).unwrap();
            let mut worker = worker_rv.enter(session, &worker_program).unwrap();
            let mut service = service_rv.enter(session, &service_program).unwrap();
            let initial = unsafe {
                [
                    actor_snapshot(&owner),
                    actor_snapshot(&worker),
                    actor_snapshot(&service),
                ]
            };
            let operations = RefCell::new(std::vec::Vec::new());
            let (sends, (_notice_before, _notice_after, receipts), ()) =
                tokio::time::timeout(core::time::Duration::from_secs(30), async {
                    tokio::join!(
                        async {
                            // SAFETY: this pinned runtime uses TestTransport throughout.
                            let before = unsafe { actor_snapshot(&owner) };
                            let branch = owner.offer().await.unwrap();
                            assert_eq!(branch.label(), 8);
                            branch.send::<LeftNotice>(&()).await.unwrap();
                            operations.borrow_mut().push(OperationObservation {
                                view: 0,
                                label: 8,
                                before: before.clone(),
                                after: unsafe { actor_snapshot(&owner) },
                                frame: FrameBoundary::Sent(network.accepted_frame(1, 0)),
                            });
                            let mut sends = std::vec![(before, network.queued_frame(1))];
                            for (visit, value) in [7, 8, 9, 9].into_iter().enumerate() {
                                number.set(value);
                                let before = unsafe { actor_snapshot(&owner) };
                                let branch = owner.offer().await.unwrap();
                                let label = branch.label();
                                match label {
                                    1 => branch.send::<LeftLoan>(db).await.unwrap(),
                                    2 => branch.send::<RightLoan>(db).await.unwrap(),
                                    label => panic!("unexpected loan label {label}"),
                                }
                                operations.borrow_mut().push(OperationObservation {
                                    view: 0,
                                    label,
                                    before: before.clone(),
                                    after: unsafe { actor_snapshot(&owner) },
                                    frame: FrameBoundary::Sent(
                                        network.accepted_frame(1, usize::from(visit == 0)),
                                    ),
                                });
                                // The ordinary notice precedes the first loan in this
                                // actual FIFO; observing it does not consume either frame.
                                sends.push((
                                    before,
                                    network.queued_frame_at(1, usize::from(visit == 0)),
                                ));
                                let before = unsafe { actor_snapshot(&owner) };
                                db = owner.recv::<Return>().await.unwrap();
                                operations.borrow_mut().push(OperationObservation {
                                    view: 0,
                                    label: 5,
                                    before,
                                    after: unsafe { actor_snapshot(&owner) },
                                    frame: FrameBoundary::Received(
                                        network.received_frame(0, session, 0),
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
                                frame: FrameBoundary::Received(
                                    network.received_frame(10, session, 0),
                                ),
                            });
                            sends
                        },
                        async {
                            let notice_before = unsafe { actor_snapshot(&worker) };
                            let branch = worker.offer().await.unwrap();
                            assert_eq!(branch.label(), 8);
                            branch.recv::<LeftNotice>().await.unwrap();
                            let notice_after = unsafe { actor_snapshot(&worker) };
                            operations.borrow_mut().push(OperationObservation {
                                view: 1,
                                label: 8,
                                before: notice_before.clone(),
                                after: notice_after.clone(),
                                frame: FrameBoundary::Received(
                                    network.received_frame(1, session, 0),
                                ),
                            });
                            let mut receipts = std::vec::Vec::new();
                            for expected in [1, 2, 1, 1] {
                                // SAFETY: this receiver has its own pinned TestTransport actor.
                                let before = unsafe { actor_snapshot(&worker) };
                                let branch = worker.offer().await.unwrap();
                                assert_eq!(branch.label(), expected);
                                let db = match branch.label() {
                                    1 => branch.recv::<LeftLoan>().await.unwrap(),
                                    2 => branch.recv::<RightLoan>().await.unwrap(),
                                    label => panic!("unexpected received loan label {label}"),
                                };
                                let after = unsafe { actor_snapshot(&worker) };
                                operations.borrow_mut().push(OperationObservation {
                                    view: 1,
                                    label: expected,
                                    before: before.clone(),
                                    after: after.clone(),
                                    frame: FrameBoundary::Received(
                                        network.received_frame(1, session, 0),
                                    ),
                                });
                                worker.send::<Return>(db).await.unwrap();
                                let return_wire = network.queued_frame(0);
                                let returned = unsafe { actor_snapshot(&worker) };
                                operations.borrow_mut().push(OperationObservation {
                                    view: 1,
                                    label: 5,
                                    before: after.clone(),
                                    after: returned.clone(),
                                    frame: FrameBoundary::Sent(network.accepted_frame(0, 0)),
                                });
                                receipts.push((before, after, returned, return_wire));
                            }
                            (notice_before, notice_after, receipts)
                        },
                        async {
                            let before = unsafe { actor_snapshot(&service) };
                            let number = service.recv::<Read>().await.unwrap();
                            operations.borrow_mut().push(OperationObservation {
                                view: 3,
                                label: 3,
                                before,
                                after: unsafe { actor_snapshot(&service) },
                                frame: FrameBoundary::Received(
                                    network.received_frame(3, session, 0),
                                ),
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
                .expect("alternating owned choices stopped progressing");
            assert!(network.queue_is_empty());
            assert_eq!(sends.len(), 5);
            assert_eq!(receipts.len(), 4);
            assert!(
                sends
                    .iter()
                    .all(|(_, wire)| wire.0.lane() == sends[0].1.0.lane())
            );
            assert_eq!(sends[1].1.1, sends[3].1.1);
            assert_eq!(sends[3].1.1, sends[4].1.1);
            assert_eq!(receipts[3].0.choices, receipts[3].1.choices);
            assert!(receipts[3].0.completion_mutations < receipts[3].1.completion_mutations);
            let operations = operations.into_inner();
            assert_eq!(operations.len(), 22);
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
                write_observations(&sends, &receipts, (&_notice_before, &_notice_after));
                write_distributed(&initial, &operations);
            }
        });
}

/// Choose compatible complete witnesses from the copied local knowledge.
/// Role 1 deliberately chooses the opposite unentered sibling from role 0.
/// This runs only after execution and cannot decide a route or an operation.
#[cfg(not(miri))]
fn write_distributed(initial: &[ActorSnapshot; 3], operations: &[OperationObservation]) {
    let path = |snapshot: &ActorSnapshot| {
        let selected = snapshot
            .choices
            .iter()
            .find(|(scope, _)| *scope == 2)
            .and_then(|(_, arm)| *arm);
        usize::from(selected == Some(1)) * 2 + usize::from(snapshot.actor == 1)
    };
    let paths = operations
        .iter()
        .map(|operation| (path(&operation.before), path(&operation.after)))
        .collect::<std::vec::Vec<_>>();
    let mut source =
        super::distributed_source::execution_source::<Steps, 32>(initial, operations, &paths);
    source.insert_str(0, "import Hibana.DistributedChoiceHistory\n");
    source.push_str(r#"
theorem distributedCurrentChoice2HasItsActualGlobalOrigin :
  ∃ before after prior later record cleared,
    DistributedSourceExecution distributedImages distributedChoreo 9063 (distributedCut0, distributedEntries0) before prior ∧
    DistributedSourceStep distributedImages distributedChoreo 9063 before after (some (1, record)) ∧
    ActorMeasuredCommit distributedImage1 distributedChoreo 9063 (before.1.actors 1) (after.1.actors 1) cleared record ∧
    record.IntroducesChoice distributedImage1 distributedChoreo 2 .left ∧
    DistributedSourceExecution distributedImages distributedChoreo 9063 after (distributedCut33, distributedEntries33) later ∧
    ActorChoiceUnresetHistory distributedImage1 distributedChoreo 9063 2 (after.1.actors 1) (distributedCut33.actors 1) :=
  a_current_distributed_choice_has_its_unreset_actual_boundary distributedActualExecution (by decide) (by decide)

theorem distributedCurrentChoice0HasItsActualGlobalOrigin :
  ∃ before after prior later record cleared,
    DistributedSourceExecution distributedImages distributedChoreo 9063 (distributedCut0, distributedEntries0) before prior ∧
    DistributedSourceStep distributedImages distributedChoreo 9063 before after (some (1, record)) ∧
    ActorMeasuredCommit distributedImage1 distributedChoreo 9063 (before.1.actors 1) (after.1.actors 1) cleared record ∧
    record.IntroducesChoice distributedImage1 distributedChoreo 0 .left ∧
    DistributedSourceExecution distributedImages distributedChoreo 9063 after (distributedCut33, distributedEntries33) later ∧
    ActorChoiceUnresetHistory distributedImage1 distributedChoreo 9063 0 (after.1.actors 1) (distributedCut33.actors 1) :=
  a_current_distributed_choice_has_its_unreset_actual_boundary distributedActualExecution (by decide) (by decide)

theorem distributedPartialWitnessesAreIndependent :
    distributedSend14.pathIndex = 0 ∧ distributedCommit15.step.pathIndex = 1 ∧
    distributedSend14.pathIndex ≠ distributedCommit15.step.pathIndex := by decide

theorem distributedUnenteredSiblingRemainsUnknown :
    (distributedCut33.actors 0).1.doneState.selected 4 = none ∧
    (distributedCut33.actors 1).1.doneState.selected 4 = none := by decide

theorem distributedSameArmStillHasAFreshIntroducingReceive :
distributedBefore15.choices = distributedAfter15.choices ∧
    distributedCommit15.IntroducesChoice distributedImage1 distributedChoreo 2 .left := by
  constructor
  · decide
  · exact ⟨by decide, by decide, 1, [(2, 0), (0, 0)], by decide, by decide, by decide⟩

theorem distributedCurrentChoiceReceiptHasItsActualHead :
    distributedWire1 = distributedWire14 ∧ distributedWire14 = distributedWire15 ∧
    (sourceChannelEntries distributedEntries23 distributedChannel15)[
      (distributedCut23.carriers distributedChannel15).delivered]? = some distributedEntry14 ∧
    distributedEntry14.carrierWitness ≠ distributedEntry1.carrierWitness :=
  ⟨by decide, by decide, by rfl, by decide⟩

theorem distributedChoiceAfterReturnDoesNotAssertValueCustody :
    distributedAfter16.doneState.selected 2 = some .left ∧
    distributedAfter16.viewIssued 10 = false := by decide

def distributedCurrentSource := (distributedSend14.ownedFacts? distributedChoreo).get (by decide)
def distributedCurrentReceipt :=
  (distributedCommit15.step.ownedFacts? distributedImage1 distributedChoreo).get (by decide)
def distributedCurrentMemberships :=
  (canonicalEventRoutePath? (canonicalProgramSource distributedChoreo) distributedCurrentSource.effIndex).get (by decide)

private theorem distributedActualPostReceiptChoiceSuffix :
    ActorChoiceUnresetHistory distributedImage1 distributedChoreo 9063 2
      (distributedCommit15.step.after, (distributedCut24.actors 1).2) (distributedCut33.actors 1) :=
  .commit (record := { commit := distributedCommit16, before := (distributedCut24.actors 1).2, after := (distributedCut26.actors 1).2 })
    .refl (by decide) (by decide)

theorem distributedCurrentChoiceComesFromItsActualAcceptedFrame :
    distributedCommit15.step.after.actor = distributedCurrentReceipt.binding.receiverActor ∧
    (distributedCut33.actors 1).1.doneState.selected 2 = some .left :=
  a_received_choice_stays_tied_to_its_accepted_source_occurrence
    (sender := distributedSend14) (receivedFacts := distributedCurrentReceipt)
    (sentFacts := distributedCurrentSource) (memberships := distributedCurrentMemberships)
    (receiver := distributedCommit15.step) (wire := distributedWire15)
    (receiverImage := distributedImage1) (choreo := distributedChoreo) (session := 9063)
    (scope := 2) (arm := .left) (previous := none)
    (count := (distributedCut24.actors 1).2) (final := distributedCut33.actors 1)
    (by decide) (Option.some_get (by decide)).symm
    (by decide) (Option.some_get (by decide)).symm (by decide) (by decide)
    (Option.some_get (by decide)).symm (by decide) (by decide) distributedActualPostReceiptChoiceSuffix
"#);
    let directory = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/lean-proof");
    std::fs::create_dir_all(&directory).unwrap();
    // Generated equal-data checks can use reflexivity without synthesizing a
    // Decidable instance. Arithmetic obligations still use ordinary decide;
    // both paths produce proofs checked by the same Lean kernel.
    std::fs::write(
        directory.join("DistributedChoiceGenerated.lean"),
        source.replace("by decide", "by first | rfl | decide"),
    )
    .unwrap();
}

#[cfg(not(miri))]
fn write_observations(
    sends: &[(ActorSnapshot, Wire)],
    receipts: &[(ActorSnapshot, ActorSnapshot, ActorSnapshot, Wire)],
    notice: (&ActorSnapshot, &ActorSnapshot),
) {
    use crate::test_support::{lean_choreo::LeanChoreo, lean_descriptor::descriptor_source};
    let mut source = std::format!(
        "import Hibana.OccurrenceChoiceKnowledge\nimport Hibana.ActorChoiceOrigin\nimport Hibana.SourceOwnedReceive\n\nset_option maxRecDepth 10000\n\n\
         def occurrenceChoiceChoreo : Hibana.Choreo := {}\n\n",
        Steps::lean_source(),
    );
    for (name, image) in [
        (
            "occurrenceOwnerImage",
            &RoleProjection::<0, Steps, 32>::IMAGE_REF,
        ),
        (
            "occurrenceWorkerImage",
            &RoleProjection::<1, Steps, 32>::IMAGE_REF,
        ),
    ] {
        source.push_str(&std::format!(
            "def {name} : Hibana.RustDescriptorImage := {}\n\n",
            descriptor_source(RoleDescriptorRef::from_resident(image))
        ));
    }
    for (index, (before, (header, payload))) in sends.iter().enumerate() {
        let label = [8, 1, 2, 1, 1][index];
        let event = before
            .steps
            .iter()
            .position(|step| {
                matches!(step.action,
            LocalAction::Send { label: found, .. } if found == label)
            })
            .unwrap();
        let next_path = if label == 2 { 2 } else { 0 };
        let previous_path = if index == 3 { 2 } else { 0 };
        source.push_str(&std::format!(
            "def choiceBefore{index} : Hibana.ActorProgressObservation := {}\n\n\
             def choiceWire{index} : Hibana.OwnedWireObservation := {{ session := {}, lane := {}, \
             sender := {}, receiver := {}, frameLabel := {}, payload := {payload:?} }}\n\n",
            super::commit::observation_source(before),
            header.session().raw(),
            header.lane(),
            header.source_role(),
            header.target_role(),
            header.label().raw(),
        ));
        let base = if let Some(scope) = before.steps[event].reentry_scope {
            std::format!(
                "((Hibana.prepareObservedActorBody? occurrenceOwnerImage occurrenceChoiceChoreo \
                choiceBefore{index} {previous_path} {next_path} {scope} {event}).get (by decide)).2"
            )
        } else {
            std::format!("choiceBefore{index}")
        };
        source.push_str(&std::format!(
            "def choiceSend{index} : Hibana.SourceSendAdmission := {{ image := occurrenceOwnerImage, \
             base := {base}, pathIndex := {next_path}, event := {event}, wire := choiceWire{index} }}\n\n\
             theorem choiceSend{index}Checked : choiceSend{index}.check occurrenceChoiceChoreo 9063 = true := by decide\n\n"
        ));
    }
    source.push_str(&std::format!(
        "def choicePrefixBefore : Hibana.ActorProgressObservation := {}\n\n\
         def choicePrefixAfter : Hibana.ActorProgressObservation := {}\n\n\
         def choicePrefixCommit : Hibana.ActorCommitRecord := {{ step := {{ pathIndex := 1, event := 0, view := 1, \
         before := choicePrefixBefore, after := choicePrefixAfter }}, wire := choiceWire0 }}\n\n\
         private theorem choicePrefixChecked : choicePrefixCommit.check occurrenceWorkerImage \
         occurrenceChoiceChoreo 9063 = true := by decide\n\n\
         private theorem choicePrefixHistory : Hibana.ActorMeasuredHistory occurrenceWorkerImage \
         occurrenceChoiceChoreo 9063 (choicePrefixBefore, {}) (choicePrefixAfter, {}) [] 1 :=\n  \
         .tail .refl (.commit (record := {{ commit := choicePrefixCommit, before := {}, after := {} }}) \
         choicePrefixChecked (by decide))\n\n",
        super::commit::observation_source(notice.0), super::commit::observation_source(notice.1),
        notice.0.completion_mutations, notice.1.completion_mutations,
        notice.0.completion_mutations, notice.1.completion_mutations
    ));
    for (visit, (before, after, returned, (return_header, return_payload))) in
        receipts.iter().enumerate()
    {
        let index = visit + 1;
        let label = [1, 2, 1, 1][visit];
        let event = before
            .steps
            .iter()
            .position(|step| {
                matches!(step.action,
            LocalAction::Recv { label: found, .. } if found == label)
            })
            .unwrap();
        // The never-entered sibling route is unknown. Sender and receiver
        // intentionally use different complete source witnesses for that slot.
        let next_path = if label == 2 { 3 } else { 1 };
        let previous_path = if visit == 2 { 3 } else { 1 };
        source.push_str(&std::format!(
            "def choiceReceiverBefore{visit} : Hibana.ActorProgressObservation := {}\n\n\
             def choiceReceiverAfter{visit} : Hibana.ActorProgressObservation := {}\n\n",
            super::commit::observation_source(before),
            super::commit::observation_source(after),
        ));
        let base = if let Some(scope) = before.steps[event].reentry_scope {
            std::format!(
                "((Hibana.prepareObservedActorBody? occurrenceWorkerImage occurrenceChoiceChoreo \
                choiceReceiverBefore{visit} {previous_path} {next_path} {scope} {event}).get (by decide)).2"
            )
        } else {
            std::format!("choiceReceiverBefore{visit}")
        };
        if visit == 0 {
            source.push_str(&std::format!(
                "def choiceReceipt{visit} : Hibana.ActorCommitObservation := {{ pathIndex := {next_path}, \
                 event := {event}, view := 1, before := {base}, after := choiceReceiverAfter{visit} }}\n\n"
            ));
        } else {
            let scope = before.steps[event].reentry_scope.unwrap();
            source.push_str(&std::format!(
                "def choiceFresh{visit} : Hibana.ActorReentryCommitObservation := {{ previousPath := {previous_path}, \
                 nextPath := {next_path}, scope := {scope}, event := {event}, view := 1, \
                 before := choiceReceiverBefore{visit}, after := choiceReceiverAfter{visit} }}\n\n\
                 private theorem choicePreparedPresent{visit} : \
                 (choiceFresh{visit}.prepared? occurrenceWorkerImage occurrenceChoiceChoreo).isSome = true := by decide\n\n\
                 def choicePrepared{visit} := (choiceFresh{visit}.prepared? occurrenceWorkerImage \
                 occurrenceChoiceChoreo).get choicePreparedPresent{visit}\n\n\
                 def choiceReceipt{visit} := choicePrepared{visit}.2\n\n\
                 private theorem choicePreparedAt{visit} : choiceFresh{visit}.prepared? occurrenceWorkerImage \
                 occurrenceChoiceChoreo = some (choicePrepared{visit}.1, choiceReceipt{visit}) := \
                 (Option.some_get choicePreparedPresent{visit}).symm\n\n"
            ));
        }
        source.push_str(&std::format!(
            "theorem choiceReceipt{visit}Checked : choiceReceipt{visit}.ownedCheck occurrenceWorkerImage \
             occurrenceChoiceChoreo choiceWire{index} 9063 = true := by decide\n\n\
             private theorem choiceReceiptRecordChecked{visit} : \
             ({{ step := choiceReceipt{visit}, wire := choiceWire{index} }} : Hibana.ActorCommitRecord).check \
             occurrenceWorkerImage occurrenceChoiceChoreo 9063 = true := by decide\n\n"
        ));
        let arm = if label == 2 { "right" } else { "left" };
        source.push_str(&std::format!(r#"private theorem choiceSourcePresent{visit} : (choiceSend{index}.ownedFacts? occurrenceChoiceChoreo).isSome = true := by decide
def choiceSourceFacts{visit} := (choiceSend{index}.ownedFacts? occurrenceChoiceChoreo).get choiceSourcePresent{visit}
private theorem choiceSourceAt{visit} : choiceSend{index}.ownedFacts? occurrenceChoiceChoreo = some choiceSourceFacts{visit} :=
  (Option.some_get choiceSourcePresent{visit}).symm

private theorem choiceReceiptPresent{visit} : (choiceReceipt{visit}.ownedFacts? occurrenceWorkerImage occurrenceChoiceChoreo).isSome = true := by decide
def choiceReceiptFacts{visit} := (choiceReceipt{visit}.ownedFacts? occurrenceWorkerImage occurrenceChoiceChoreo).get choiceReceiptPresent{visit}
private theorem choiceReceiptAt{visit} : choiceReceipt{visit}.ownedFacts? occurrenceWorkerImage occurrenceChoiceChoreo = some choiceReceiptFacts{visit} :=
  (Option.some_get choiceReceiptPresent{visit}).symm
private theorem choiceReceiverSide{visit} : choiceReceiptFacts{visit}.side = .receive := by decide

private theorem choiceMembershipsPresent{visit} : (Hibana.canonicalEventRoutePath? (Hibana.canonicalProgramSource occurrenceChoiceChoreo) choiceSourceFacts{visit}.effIndex).isSome = true := by decide
def choiceMemberships{visit} := (Hibana.canonicalEventRoutePath? (Hibana.canonicalProgramSource occurrenceChoiceChoreo) choiceSourceFacts{visit}.effIndex).get choiceMembershipsPresent{visit}
private theorem choiceMembershipsAt{visit} : Hibana.canonicalEventRoutePath? (Hibana.canonicalProgramSource occurrenceChoiceChoreo) choiceSourceFacts{visit}.effIndex = some choiceMemberships{visit} :=
  (Option.some_get choiceMembershipsPresent{visit}).symm
private theorem choiceInnerMember{visit} : choiceMemberships{visit}.find? (fun row => row.1 == 2) = some (2, Hibana.encodeRouteArm .{arm}) := by decide
private theorem choiceInnerUnknown{visit} : choiceReceipt{visit}.before.choices.find? (fun row => row.1 == 2) = some (2, none) := by decide
private theorem choiceSiblingUnrelated{visit} : choiceMemberships{visit}.find? (fun row => row.1 == 4) = none := by decide
private theorem choiceSiblingUnknown{visit} : choiceReceipt{visit}.before.choices.find? (fun row => row.1 == 4) = some (4, none) := by decide

theorem choiceReceipt{visit}LearnsFromItsAcceptedFrame :
    choiceReceipt{visit}.after.actor = choiceReceiptFacts{visit}.binding.receiverActor ∧
      choiceReceipt{visit}.after.doneState.selected 2 = some .{arm} :=
  Hibana.a_checked_owned_receipt_learns_its_accepted_source_choice
    (sentFacts := choiceSourceFacts{visit}) (receivedFacts := choiceReceiptFacts{visit})
    (memberships := choiceMemberships{visit}) (previous := none)
    choiceSend{index}Checked choiceSourceAt{visit} choiceReceipt{visit}Checked choiceReceiptAt{visit}
    choiceReceiverSide{visit} rfl choiceMembershipsAt{visit} choiceInnerMember{visit} choiceInnerUnknown{visit}

theorem choiceReceipt{visit}PreservesUnknownSibling :
    choiceReceipt{visit}.after.doneState.selected 4 = none :=
  Hibana.a_checked_owned_frame_preserves_an_unrelated_local_choice
    (sentFacts := choiceSourceFacts{visit}) (receivedFacts := choiceReceiptFacts{visit})
    (memberships := choiceMemberships{visit}) (scope := 4) (previous := none)
    choiceSend{index}Checked choiceSourceAt{visit} choiceReceipt{visit}Checked choiceReceiptAt{visit}
    rfl choiceMembershipsAt{visit} choiceSiblingUnrelated{visit} choiceSiblingUnknown{visit}

theorem choiceReceipt{visit}NeedsNoCommonWholePath : choiceSend{index}.pathIndex ≠ choiceReceipt{visit}.pathIndex := by decide

"#));
        let return_event = after
            .steps
            .iter()
            .zip(&returned.steps)
            .position(|(before, after)| !before.done && after.done)
            .unwrap();
        source.push_str(&std::format!(
            "def choiceReturned{visit} : Hibana.ActorProgressObservation := {}\n\n\
             def choiceReturnWire{visit} : Hibana.OwnedWireObservation := {{ session := {}, lane := {}, \
             sender := {}, receiver := {}, frameLabel := {}, payload := {return_payload:?} }}\n\n\
             def choiceReturn{visit} : Hibana.ActorCommitRecord := {{ step := {{ pathIndex := {next_path}, \
             event := {return_event}, view := 1, before := choiceReceiverAfter{visit}, after := choiceReturned{visit} }}, \
             wire := choiceReturnWire{visit} }}\n\n\
             private theorem choiceReturnChecked{visit} : choiceReturn{visit}.check occurrenceWorkerImage \
             occurrenceChoiceChoreo 9063 = true := by decide\n\n",
            super::commit::observation_source(returned), return_header.session().raw(), return_header.lane(),
            return_header.source_role(), return_header.target_role(), return_header.label().raw()
        ));
        if visit == 0 {
            source.push_str(&std::format!(
                "private theorem choiceMeasuredHistory{visit} : ∃ cleared commits, Hibana.ActorMeasuredHistory \
                 occurrenceWorkerImage occurrenceChoiceChoreo 9063 \
                 (choicePrefixBefore, {}) (choiceReceiverAfter{visit}, {}) cleared commits :=\n  \
                 ⟨[], 2, .tail (.tail choicePrefixHistory (.quiet (by decide))) \
                 (.commit (record := {{ commit := {{ step := choiceReceipt0, wire := choiceWire1 }}, \
                 before := {}, after := {} }}) choiceReceiptRecordChecked0 (by decide))⟩\n\n",
                notice.0.completion_mutations, after.completion_mutations,
                before.completion_mutations, after.completion_mutations
            ));
        } else {
            let previous = visit - 1;
            let previous_count = receipts[previous].2.completion_mutations;
            source.push_str(&std::format!(
                "private theorem choiceFreshChecked{visit} : Hibana.actorReentryMutationCheck choiceFresh{visit} \
                 choiceWire{index} occurrenceWorkerImage occurrenceChoiceChoreo 9063 {} {} = true := by decide\n\n\
                 private theorem choiceMeasuredHistory{visit} : ∃ cleared commits, Hibana.ActorMeasuredHistory \
                 occurrenceWorkerImage occurrenceChoiceChoreo 9063 \
                 (choicePrefixBefore, {}) (choiceReceiverAfter{visit}, {}) cleared commits := by\n  \
                 obtain ⟨cleared, commits, history⟩ := choiceReturnedHistory{previous}\n  \
                 have gap : Hibana.ActorMeasuredStep occurrenceWorkerImage occurrenceChoiceChoreo 9063 \
                 (choiceReturned{previous}, {previous_count}) (choiceReceiverBefore{visit}, {previous_count}) [] 0 := .quiet (by decide)\n  \
                 have fresh := Hibana.ActorMeasuredStep.reentry \
                 (roll := choicePrepared{visit}.1) \
                 (step := choiceReceipt{visit}) choiceFreshChecked{visit} choicePreparedAt{visit}\n  \
                 exact ⟨_, _, .tail (.tail history gap) fresh⟩\n\n",
                before.completion_mutations, after.completion_mutations,
                notice.0.completion_mutations, after.completion_mutations
            ));
        }
        source.push_str(&std::format!(
            "private theorem choiceReturnedHistory{visit} : ∃ cleared commits, Hibana.ActorMeasuredHistory \
             occurrenceWorkerImage occurrenceChoiceChoreo 9063 \
             (choicePrefixBefore, {}) (choiceReturned{visit}, {}) cleared commits := by\n  \
             obtain ⟨cleared, commits, history⟩ := choiceMeasuredHistory{visit}\n  \
             exact ⟨_, _, .tail history (.commit (record := {{ commit := choiceReturn{visit}, \
             before := {}, after := {} }}) choiceReturnChecked{visit} (by decide))⟩\n\n\
             theorem choiceCurrentArm{visit}HasItsActualIntroducingCommit :\n  \
             ∃ before after prior marked priorCommits record, \
             Hibana.ActorMeasuredHistory occurrenceWorkerImage occurrenceChoiceChoreo 9063 \
             (choicePrefixBefore, {}) before prior priorCommits ∧ \
             Hibana.ActorMeasuredCommit occurrenceWorkerImage occurrenceChoiceChoreo 9063 before after marked record ∧ \
             record.IntroducesChoice occurrenceWorkerImage occurrenceChoiceChoreo 2 .{arm} ∧ \
             Hibana.ActorChoiceUnresetHistory occurrenceWorkerImage occurrenceChoiceChoreo 9063 2 \
             after (choiceReceiverAfter{visit}, {}) ∧ \
             record.check occurrenceWorkerImage occurrenceChoiceChoreo 9063 = true ∧ \
             record.step.SourceOrdered occurrenceWorkerImage occurrenceChoiceChoreo := by\n  \
             obtain ⟨cleared, commits, history⟩ := choiceMeasuredHistory{visit}\n  \
             exact Hibana.a_current_choice_has_its_latest_actual_source_origin history (by decide) (by decide)\n\n",
            notice.0.completion_mutations, returned.completion_mutations,
            after.completion_mutations, returned.completion_mutations,
            notice.0.completion_mutations, after.completion_mutations
        ));
        source.push_str(&std::format!(
            "theorem choiceOuterArm{visit}KeepsItsActualOriginAcrossInnerResets :\n  \
             ∃ before after prior marked priorCommits record, \
             Hibana.ActorMeasuredHistory occurrenceWorkerImage occurrenceChoiceChoreo 9063 \
             (choicePrefixBefore, {}) before prior priorCommits ∧ \
             Hibana.ActorMeasuredCommit occurrenceWorkerImage occurrenceChoiceChoreo 9063 before after marked record ∧ \
             record.IntroducesChoice occurrenceWorkerImage occurrenceChoiceChoreo 0 .left ∧ \
             Hibana.ActorChoiceUnresetHistory occurrenceWorkerImage occurrenceChoiceChoreo 9063 0 \
             after (choiceReceiverAfter{visit}, {}) ∧ \
             record.check occurrenceWorkerImage occurrenceChoiceChoreo 9063 = true ∧ \
             record.step.SourceOrdered occurrenceWorkerImage occurrenceChoiceChoreo := by\n  \
             obtain ⟨cleared, commits, history⟩ := choiceMeasuredHistory{visit}\n  \
             exact Hibana.a_current_choice_has_its_latest_actual_source_origin history (by decide) (by decide)\n\n",
            notice.0.completion_mutations, after.completion_mutations
        ));
    }
    source.push_str(
        "def choiceChannel : Hibana.TransportChannel := { session := 9063, generation := 0, \
         lane := choiceWire0.lane, sender := choiceWire0.sender, receiver := choiceWire0.receiver }\n\n\
         def choiceCarrier0 := Hibana.TransportState.initial choiceChannel\n\n\
         def choiceGhost0 := Hibana.ElasticAdmissionHistory.initial choiceChannel\n\n\
         def choiceRecords0 : List (Hibana.SourceSendAdmission × Nat) := []\n\n\
         theorem choiceTrace0 : Hibana.SendAdmissionTrace occurrenceChoiceChoreo 9063 choiceCarrier0 choiceGhost0 choiceRecords0 := .initial choiceChannel\n\n",
    );
    let mut prior_carrier = std::string::String::from("choiceCarrier0");
    let mut prior_ghost = std::string::String::from("choiceGhost0");
    let mut prior_trace = std::string::String::from("choiceTrace0");
    for index in 0..sends.len() {
        let next = index + 1;
        source.push_str(&std::format!(
            "def choiceEvent{index} := choiceSend{index}.effIndex?.get (by decide)\n\n\
             def choiceCarrier{next} := ({prior_carrier}.send ⟨choiceWire{index}.frameLabel, by decide⟩).get (by decide)\n\n\
             def choiceGhost{next} := {prior_ghost}.appendOccurrence choiceEvent{index}\n\n\
             def choiceRecords{next} := choiceRecords{index} ++ [(choiceSend{index}, choiceEvent{index})]\n\n\
             theorem choiceTrace{next} : Hibana.SendAdmissionTrace occurrenceChoiceChoreo 9063 choiceCarrier{next} choiceGhost{next} choiceRecords{next} :=\n  \
             .accept (transport := {prior_carrier}) (sent := choiceCarrier{next}) (history := {prior_ghost}) \
             (records := choiceRecords{index}) (sample := choiceSend{index}) (eventId := choiceEvent{index}) \
             (label := ⟨choiceWire{index}.frameLabel, by decide⟩)\n    \
             {prior_trace} choiceSend{index}Checked (by decide) (by decide) ⟨rfl, rfl, rfl, rfl, rfl⟩\n\n"
        ));
        prior_carrier = std::format!("choiceCarrier{next}");
        prior_ghost = std::format!("choiceGhost{next}");
        prior_trace = std::format!("choiceTrace{next}");
        let deliveries: &[usize] = match index {
            1 => &[0, 1],
            2 => &[2],
            3 => &[3],
            _ => &[],
        };
        for &delivered in deliveries {
            let consumed = delivered + 1;
            source.push_str(&std::format!(
                "def choiceCarrierConsumed{consumed} := {{ {prior_carrier} with delivered := {consumed} }}\n\n\
                 def choiceGhostConsumed{consumed} := {{ {prior_ghost} with delivered := {consumed} }}\n\n\
                 theorem choiceConsumed{consumed} : Hibana.SendAdmissionTrace occurrenceChoiceChoreo 9063 \
                 choiceCarrierConsumed{consumed} choiceGhostConsumed{consumed} choiceRecords{next} :=\n  \
                 .receive (transport := {prior_carrier}) (history := {prior_ghost})\n    \
                 (frame := ({prior_carrier}.frames[{delivered}]?).get (by decide))\n    \
                 (key := ({prior_ghost}.accepted[{delivered}]?).get (by decide)) {prior_trace} (by decide) rfl\n\n"
            ));
            prior_carrier = std::format!("choiceCarrierConsumed{consumed}");
            prior_ghost = std::format!("choiceGhostConsumed{consumed}");
            prior_trace = std::format!("choiceConsumed{consumed}");
        }
    }
    source.push_str(r#"
theorem choiceTraceIncludesTheOrdinaryPrefixAndAllThreeLoans :
    choiceRecords4.length = choiceCarrier4.frames.length :=
  Hibana.a_checked_source_admission_trace_records_every_frame choiceTrace4

theorem choiceActualOccurrenceKeysFollowTheFIFO :
    choiceGhost4.accepted = [⟨choiceEvent0, 0⟩, ⟨choiceEvent1, 0⟩, ⟨choiceEvent2, 0⟩, ⟨choiceEvent1, 1⟩] ∧
      choiceGhost4.delivered = 3 := by decide

def choiceFinalFrame := (choiceCarrier4.frames[3]?).get (by decide)
theorem choiceFreshReceiveHasItsActualAcceptedSource :
    ∃ sample eventId key,
      choiceRecords4[choiceCarrier4.delivered]? = some (sample, eventId) ∧
      choiceGhost4.accepted[choiceCarrier4.delivered]? = some key ∧
      sample.check occurrenceChoiceChoreo 9063 = true ∧ sample.effIndex? = some eventId ∧
      key.globalId = eventId ∧ sample.wire.identifies choiceFinalFrame := by
  obtain ⟨sample, eventId, key, following, atRecord, atKey, checked, sourceAt, identity, sameSource, _⟩ :=
    Hibana.a_fifo_receive_has_its_actual_accepted_source_occurrence choiceTrace4
      (next := { choiceCarrier4 with delivered := 4 }) (frame := choiceFinalFrame) (by decide)
  exact ⟨sample, eventId, key, atRecord, atKey, checked, sourceAt, sameSource, identity⟩

theorem choiceActualFreshSourceIsTheOneTeachingTheNewArm :
    (choiceRecords4[choiceCarrier4.delivered]?).map (fun record => record.1) = some choiceSend3 ∧
      choiceReceipt2.after.doneState.selected 2 = some .left := by
  exact ⟨by rfl, choiceReceipt2LearnsFromItsAcceptedFrame.2⟩

theorem choiceEqualVisibleArmStillComesFromAFreshPreparedBase :
    choiceReceiverBefore3.choices = choiceReceiverAfter3.choices ∧
      choiceReceipt3.before.choices.find? (fun row => row.1 == 2) = some (2, none) ∧
      choiceReceiverAfter3.choices.find? (fun row => row.1 == 2) = some (2, some Hibana.RouteArm.left) := by
  exact ⟨by rfl, choiceInnerUnknown3, by decide⟩
"#);
    let received_count = receipts[3].1.completion_mutations;
    let returned_count = receipts[3].2.completion_mutations;
    source.push_str(&std::format!(r#"
private theorem choiceLastReturnSuffix :
    Hibana.ActorChoiceUnresetHistory occurrenceWorkerImage occurrenceChoiceChoreo 9063 2
      (choiceReceipt3.after, {received_count}) (choiceReturned3, {returned_count}) :=
  .commit (record := {{ commit := choiceReturn3, before := {received_count}, after := {returned_count} }})
    .refl choiceReturnChecked3 (by decide)
"#));
    source.push_str(r#"
theorem choiceEqualArmCurrentKnowledgeHasItsActualFifthFrame :
    (choiceRecords5[choiceCarrier5.delivered]?).map (fun record => record.1) = some choiceSend4 ∧
      choiceGhost5.accepted[choiceCarrier5.delivered]? = some ⟨choiceEvent4, 2⟩ ∧
      choiceReceipt3.after.actor = choiceReceiptFacts3.binding.receiverActor ∧
      choiceReturned3.doneState.selected 2 = some .left := by
  refine ⟨rfl, by rfl, ?_⟩
  exact Hibana.a_received_choice_stays_tied_to_its_accepted_source_occurrence
    (sentFacts := choiceSourceFacts3) (receivedFacts := choiceReceiptFacts3)
    (memberships := choiceMemberships3) (previous := none)
    choiceSend4Checked choiceSourceAt3 choiceReceipt3Checked choiceReceiptAt3
    choiceReceiverSide3 rfl choiceMembershipsAt3 choiceInnerMember3 choiceInnerUnknown3 choiceLastReturnSuffix

def choiceInventedSibling : Hibana.ActorProgressObservation :=
  { choiceReceiverBefore0 with choices := choiceReceiverBefore0.choices.map fun (scope, arm) =>
      (scope, if scope = 4 then some .right else arm) }

theorem choiceZeroWriteGapCannotInventSiblingKnowledge (count : Nat) :
    Hibana.actorCommitMutationHistoryCheck occurrenceWorkerImage occurrenceChoiceChoreo 9063
      choiceReceiverBefore0 count choiceInventedSibling count [] = false := by
  apply Bool.eq_false_iff.mpr
  intro checked
  have same := Hibana.a_zero_write_measured_gap_preserves_committed_choices checked
  have atSibling := congrArg (fun choices => choices.find? (fun row => row.1 == 4)) same
  change some (4, some Hibana.RouteArm.right) = some (4, none) at atSibling
  cases atSibling

def choiceForgottenOuter : Hibana.ActorProgressObservation :=
  { choiceReceiverBefore0 with choices := choiceReceiverBefore0.choices.map fun (scope, arm) =>
      (scope, if scope = 0 then none else arm) }

theorem choiceZeroWriteGapCannotForgetOuterKnowledge (count : Nat) :
    Hibana.actorCommitMutationHistoryCheck occurrenceWorkerImage occurrenceChoiceChoreo 9063
      choiceReceiverBefore0 count choiceForgottenOuter count [] = false := by
  apply Bool.eq_false_iff.mpr
  intro checked
  have same := Hibana.a_zero_write_measured_gap_preserves_committed_choices checked
  have atOuter := congrArg (fun choices => choices.find? (fun row => row.1 == 0)) same
  change some (0, none) = some (0, some Hibana.RouteArm.left) at atOuter
  cases atOuter
"#);
    source.push_str(
        "\nprivate def choiceSharedSourceAtoms := (occurrenceOwnerImage.decodeAtoms?).get (by decide)\n\
         private theorem choiceWorkerSharesSourceAtoms : occurrenceWorkerImage.decodeAtoms? = \
         some choiceSharedSourceAtoms := by decide\n",
    );
    for (visit, (before, _, _, _)) in receipts.iter().enumerate() {
        let index = visit + 1;
        let accepted = index + 1;
        let label = [1, 2, 1, 1][visit];
        let event = before
            .steps
            .iter()
            .position(|step| {
                matches!(step.action,
                    LocalAction::Recv { label: found, .. } if found == label)
            })
            .unwrap();
        let path = if label == 2 { 3 } else { 1 };
        source.push_str(&std::format!(
            r#"
private def choiceBeforeReceiveFacts{visit} :=
  (occurrenceWorkerImage.ownedSourceFacts? occurrenceChoiceChoreo {path} {event}).get (by decide)

private theorem choiceBeforeReceiveMetadata{visit} :
    choiceBeforeReceiveFacts{visit}.side = .receive ∧
      choiceBeforeReceiveFacts{visit}.binding.receiverActor = occurrenceWorkerImage.role ∧
      choiceSend{index}.effIndex? = some choiceBeforeReceiveFacts{visit}.effIndex := by decide

private def choiceAcceptedBeforeReceiveFrame{visit} :=
  (choiceCarrier{accepted}.frames[{index}]?).get (by decide)

private theorem choiceAcceptedBeforeReceiveIdentity{visit} :
    choiceWire{index}.identifies choiceAcceptedBeforeReceiveFrame{visit} := by
  unfold Hibana.OwnedWireObservation.identifies
  decide

private theorem choiceBeforeReceiveWireColumns{visit} :
    occurrenceWorkerImage.eventLane? {event} = some choiceWire{index}.lane ∧
      occurrenceWorkerImage.eventFrameLabel? {event} = some choiceWire{index}.frameLabel := by decide

/-- Only immutable source rows and the accepted frame are used here. The
receiver's after-snapshot, issued tag and current receive-pair are absent.
The sender's owned kind is derived from the source. The four calls cover
changing arms and equal-arm reentry with unknown siblings. -/
theorem choicePrepared{visit}ChecksAcceptedBytesBeforeReceipt (bytes : List Nat) :
    choiceBeforeReceiveFacts{visit}.wireMatches 1
        {{ choiceWire{index} with payload := bytes }} 9063 = true ↔
      bytes = choiceWire{index}.payload := by
  have receiverAt := (Option.some_get
    (x := occurrenceWorkerImage.ownedSourceFacts? occurrenceChoiceChoreo {path} {event}) (by decide)).symm
  have atomsAt := (Option.some_get (x := occurrenceOwnerImage.decodeAtoms?) (by decide)).symm
  obtain ⟨_, _, _, _, checked⟩ := Hibana.a_matched_owned_receive_recovers_its_accepted_sender
    (sender := choiceSend{index}) (receiverImage := occurrenceWorkerImage)
    (choreo := occurrenceChoiceChoreo) (session := 9063) (pathIndex := {path}) (event := {event})
    (receivedFacts := choiceBeforeReceiveFacts{visit})
    (atoms := choiceSharedSourceAtoms) (wire := {{ choiceWire{index} with payload := bytes }})
    (frame := choiceAcceptedBeforeReceiveFrame{visit})
    choiceSend{index}Checked receiverAt atomsAt
    choiceWorkerSharesSourceAtoms choiceBeforeReceiveMetadata{visit}.2.2
    choiceBeforeReceiveMetadata{visit}.1 choiceBeforeReceiveMetadata{visit}.2.1
    choiceAcceptedBeforeReceiveIdentity{visit} choiceAcceptedBeforeReceiveIdentity{visit}
    choiceBeforeReceiveWireColumns{visit}.1 choiceBeforeReceiveWireColumns{visit}.2
  exact checked
"#,
        ));
    }
    let directory = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/lean-proof");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("OccurrenceChoiceGenerated.lean"), source).unwrap();
}
