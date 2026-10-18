//! One actual FIFO receipt carries a branch choice through changing rolled visits.
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
use core::cell::Cell;

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
    let owner_program = crate::global::role_program::role_program_from_image::<0>(
        &RoleProjection::<0, Steps, 32>::IMAGE_REF,
    );
    let worker_program = crate::global::role_program::role_program_from_image::<1>(
        &RoleProjection::<1, Steps, 32>::IMAGE_REF,
    );
    let db_program = crate::global::role_program::role_program_from_image::<10>(
        &RoleProjection::<10, Steps, 32>::IMAGE_REF,
    );
    let service_program = crate::global::role_program::role_program_from_image::<3>(
        &RoleProjection::<3, Steps, 32>::IMAGE_REF,
    );
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
            let (sends, receipts, ()) =
                tokio::time::timeout(core::time::Duration::from_secs(30), async {
                    tokio::join!(
                        async {
                            // SAFETY: this pinned runtime uses TestTransport throughout.
                            let before = unsafe { actor_snapshot(&owner) };
                            let branch = owner.offer().await.unwrap();
                            assert_eq!(branch.label(), 8);
                            branch.send::<LeftNotice>(&()).await.unwrap();
                            let mut sends = std::vec![(before, network.queued_frame(1))];
                            for (visit, value) in [7, 8, 9].into_iter().enumerate() {
                                number.set(value);
                                let before = unsafe { actor_snapshot(&owner) };
                                let branch = owner.offer().await.unwrap();
                                match branch.label() {
                                    1 => branch.send::<LeftLoan>(db).await.unwrap(),
                                    2 => branch.send::<RightLoan>(db).await.unwrap(),
                                    label => panic!("unexpected loan label {label}"),
                                }
                                // The ordinary notice precedes the first loan in this
                                // actual FIFO; observing it does not consume either frame.
                                sends.push((
                                    before,
                                    network.queued_frame_at(1, usize::from(visit == 0)),
                                ));
                                db = owner.recv::<Return>().await.unwrap();
                            }
                            db.send::<Read>(&41).await.unwrap();
                            assert_eq!(db.recv::<Value>().await.unwrap(), 42);
                            sends
                        },
                        async {
                            let branch = worker.offer().await.unwrap();
                            assert_eq!(branch.label(), 8);
                            branch.recv::<LeftNotice>().await.unwrap();
                            let mut receipts = std::vec::Vec::new();
                            for expected in [1, 2, 1] {
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
                                receipts.push((before, after));
                                worker.send::<Return>(db).await.unwrap();
                            }
                            receipts
                        },
                        async {
                            let number = service.recv::<Read>().await.unwrap();
                            service.send::<Value>(&(number + 1)).await.unwrap();
                        },
                    )
                })
                .await
                .expect("alternating owned choices stopped progressing");
            assert!(network.queue_is_empty());
            assert_eq!(sends.len(), 4);
            assert_eq!(receipts.len(), 3);
            assert!(
                sends
                    .iter()
                    .all(|(_, wire)| wire.0.lane() == sends[0].1.0.lane())
            );
            assert_eq!(sends[1].1.1, sends[3].1.1);
            #[cfg(not(miri))]
            write_observations(&sends, &receipts);
        });
}

#[cfg(not(miri))]
fn write_observations(
    sends: &[(ActorSnapshot, Wire)],
    receipts: &[(ActorSnapshot, ActorSnapshot)],
) {
    use crate::test_support::{lean_choreo::LeanChoreo, lean_descriptor::descriptor_source};
    let mut source = std::format!(
        "import Hibana.OccurrenceChoiceKnowledge\n\nset_option maxRecDepth 10000\n\n\
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
        let label = [8, 1, 2, 1][index];
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
    for (visit, (before, after)) in receipts.iter().enumerate() {
        let index = visit + 1;
        let label = [1, 2, 1][visit];
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
        source.push_str(&std::format!(
            "def choiceReceipt{visit} : Hibana.ActorCommitObservation := {{ pathIndex := {next_path}, \
             event := {event}, view := 1, before := {base}, after := choiceReceiverAfter{visit} }}\n\n\
             theorem choiceReceipt{visit}Checked : choiceReceipt{visit}.ownedCheck occurrenceWorkerImage \
             occurrenceChoiceChoreo choiceWire{index} 9063 = true := by decide\n\n"
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
    for index in 0..4 {
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
    let directory = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/lean-proof");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("OccurrenceChoiceGenerated.lean"), source).unwrap();
}
