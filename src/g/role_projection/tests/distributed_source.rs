//! Passive measurements of one execution, without changing its localside.
use crate::{endpoint::proof_snapshot::ActorSnapshot, test_support::transport::FrameEvidence};

pub(super) enum FrameBoundary {
    Sent(FrameEvidence),
    Received(FrameEvidence),
}

pub(super) struct OperationObservation {
    pub(super) view: u8,
    pub(super) label: u8,
    pub(super) before: ActorSnapshot,
    pub(super) after: ActorSnapshot,
    pub(super) frame: FrameBoundary,
}

impl OperationObservation {
    pub(super) fn frame(&self) -> &FrameEvidence {
        match &self.frame {
            FrameBoundary::Sent(frame) | FrameBoundary::Received(frame) => frame,
        }
    }
}

pub(super) fn check_observations(operations: &[OperationObservation]) {
    use std::collections::BTreeMap;
    let mut accepted = BTreeMap::new();
    let mut delivered = std::collections::BTreeSet::new();
    let mut actors = BTreeMap::new();
    for operation in operations {
        let before = &operation.before;
        let after = &operation.after;
        assert_eq!(before.actor, after.actor);
        if let Some(previous) = actors.insert(before.actor, after) {
            assert_eq!(previous, before, "missing actor boundary");
        }
        let frame = operation.frame();
        let event = before
            .steps
            .iter()
            .enumerate()
            .position(|(index, step)| {
                operation.after.steps[index].done
                    && step.view == operation.view
                    && match (&operation.frame, step.action) {
                        (FrameBoundary::Sent(_), super::LocalAction::Send { label, .. })
                        | (FrameBoundary::Received(_), super::LocalAction::Recv { label, .. }) => {
                            label == operation.label
                        }
                        _ => false,
                    }
            })
            .expect("source operation");
        assert!(after.steps[event].done);
        assert!(after.completion_mutations > before.completion_mutations);
        match &operation.frame {
            FrameBoundary::Sent(_) => {
                assert_eq!(frame.header.source_role(), operation.view);
                assert!(accepted.insert(frame.witness, operation).is_none());
            }
            FrameBoundary::Received(_) => {
                assert_eq!(frame.header.target_role(), operation.view);
                assert!(
                    delivered.insert(frame.witness),
                    "duplicate physical receive"
                );
                let source = accepted.get(&frame.witness).expect("actual acceptance");
                assert_eq!(source.frame().header, frame.header);
                assert_eq!(source.frame().payload(), frame.payload());
                assert_eq!(source.label, operation.label);
            }
        }
    }

    assert_eq!(delivered.len(), accepted.len());
    assert_eq!(operations.len(), accepted.len() * 2);
}

/// Serialize the already completed execution. No endpoint operation reads
/// these records, and no source identity is inferred from a frame label.
#[cfg(not(miri))]
pub(super) fn execution_source<Steps, const CAPACITY: usize>(
    initial: &[ActorSnapshot; 3],
    operations: &[OperationObservation],
    paths: &[(usize, usize)],
) -> std::string::String
where
    Steps: crate::g::ProgramShape + crate::test_support::lean_choreo::LeanChoreo,
{
    use super::{RoleDescriptorRef, RoleProjection};
    use crate::test_support::lean_descriptor::descriptor_source;
    use std::{collections::BTreeMap, fmt::Write};
    let session = operations[0].frame().header.session().raw();
    let mut source = std::format!(
        "import Hibana.DistributedSourceExecution\n\nset_option maxRecDepth 10000\n\
         set_option maxHeartbeats 0\n\nopen Hibana Hibana.EndpointDelegation\n\n\
         def distributedChoreo : Choreo := {}\n\n",
        Steps::lean_source()
    );
    for (actor, image) in [
        (0, &RoleProjection::<0, Steps, CAPACITY>::IMAGE_REF),
        (1, &RoleProjection::<1, Steps, CAPACITY>::IMAGE_REF),
        (3, &RoleProjection::<3, Steps, CAPACITY>::IMAGE_REF),
    ] {
        writeln!(
            source,
            "def distributedImage{actor} : RustDescriptorImage := {}\n",
            descriptor_source(RoleDescriptorRef::from_resident(image))
        )
        .unwrap();
    }
    source.push_str("def distributedImages (actor : Nat) : RustDescriptorImage :=\n  if actor = 1 then distributedImage1 else if actor = 3 then distributedImage3 else distributedImage0\n\n");
    for snapshot in initial {
        writeln!(
            source,
            "def distributedInitial{} : ActorProgressObservation := {}\n",
            snapshot.actor,
            super::commit::observation_source(snapshot)
        )
        .unwrap();
    }
    source.push_str("def distributedInitialActors (actor : Nat) : ActorProgressObservation × Nat :=\n  (if actor = 1 then distributedInitial1 else if actor = 3 then distributedInitial3 else distributedInitial0, 0)\n\n\
        def distributedCut0 : DistributedSourceCut := .initial distributedInitialActors\n\n\
        def distributedEntries0 : List SourceAcceptance := []\n\n");
    let mut accepted = BTreeMap::new();
    let mut stage = 0;
    let mut records = std::vec::Vec::new();
    for (index, operation) in operations.iter().enumerate() {
        let (previous_path, next_path) = paths[index];
        let before = &operation.before;
        let after = &operation.after;
        let actor = before.actor;
        let frame = operation.frame();
        let header = frame.header;
        let event = before
            .steps
            .iter()
            .enumerate()
            .position(|(index, step)| {
                operation.after.steps[index].done
                    && step.view == operation.view
                    && match (&operation.frame, step.action) {
                        (FrameBoundary::Sent(_), super::LocalAction::Send { label, .. })
                        | (FrameBoundary::Received(_), super::LocalAction::Recv { label, .. }) => {
                            label == operation.label
                        }
                        _ => false,
                    }
            })
            .unwrap();
        writeln!(source, "def distributedBefore{index} : ActorProgressObservation := {}\n\n\
            def distributedAfter{index} : ActorProgressObservation := {}\n\n\
            def distributedWire{index} : OwnedWireObservation := {{ session := {}, lane := {}, sender := {}, receiver := {}, frameLabel := {}, payload := {:?} }}\n\n\
            def distributedChannel{index} : TransportChannel := {{ session := {}, generation := 0, lane := {}, sender := {}, receiver := {} }}\n",
            super::commit::observation_source(before), super::commit::observation_source(after),
            header.session().raw(), header.lane(), header.source_role(), header.target_role(), header.label().raw(), frame.payload(),
            header.session().raw(), header.lane(), header.source_role(), header.target_role()).unwrap();
        let (base, commit, measurement) = if let Some(scope) = before.steps[event].reentry_scope {
            writeln!(source, "def distributedReentry{index} : ActorReentryCommitObservation := {{ previousPath := {previous_path}, nextPath := {next_path}, scope := {scope}, event := {event}, view := {}, before := distributedBefore{index}, after := distributedAfter{index} }}\n\n\
                private theorem distributedPrepared{index} : (distributedReentry{index}.prepared? distributedImage{actor} distributedChoreo).isSome = true := by decide\n\n\
                def distributedPreparedPair{index} := (distributedReentry{index}.prepared? distributedImage{actor} distributedChoreo).get distributedPrepared{index}\n",
                operation.view).unwrap();
            (
                std::format!("distributedPreparedPair{index}.2.before"),
                std::format!("distributedPreparedPair{index}.2"),
                std::format!(
                    ".fresh (sample := distributedReentry{index}) (by decide) (Option.some_get distributedPrepared{index}).symm"
                ),
            )
        } else {
            (
                std::format!("distributedBefore{index}"),
                std::format!(
                    "{{ pathIndex := {next_path}, event := {event}, view := {}, before := distributedBefore{index}, after := distributedAfter{index} }}",
                    operation.view
                ),
                std::format!(
                    ".ordinary (record := {{ commit := distributedCommit{index}, before := {}, after := {} }}) (by decide) (by decide)",
                    before.completion_mutations,
                    after.completion_mutations
                ),
            )
        };
        writeln!(source, "def distributedCommit{index} : ActorCommitRecord := {{ step := {commit}, wire := distributedWire{index} }}\n\n\
            private theorem distributedMeasurement{index} : ActorMeasuredCommit distributedImage{actor} distributedChoreo {session}\n  (distributedBefore{index}, {}) (distributedAfter{index}, {}) {} distributedCommit{index} :=\n  {measurement}\n",
            before.completion_mutations, after.completion_mutations,
            if before.steps[event].reentry_scope.is_some() {
                std::format!("((List.range distributedPreparedPair{index}.1.eventLength).map (distributedPreparedPair{index}.1.eventStart + ·))")
            } else { "[]".into() }).unwrap();
        if matches!(operation.frame, FrameBoundary::Sent(_)) {
            accepted.insert(frame.witness, index);
            writeln!(source, "def distributedSend{index} : SourceSendAdmission := {{ image := distributedImage{actor}, base := {base}, pathIndex := {next_path}, event := {event}, wire := distributedWire{index} }}\n\n\
                def distributedEntry{index} : SourceAcceptance := {{ sample := distributedSend{index}, channel := distributedChannel{index}, event := {}, mutation := {}, carrierWitness := {} }}\n\n\
                def distributedTransport{} : TransportState := ((distributedCut{stage}.carriers distributedChannel{index}).send ⟨{}, by decide⟩).get (by decide)\n\n\
                def distributedCut{} : DistributedSourceCut := distributedCut{stage}.carrierAt distributedChannel{index} distributedTransport{}\n\n\
                def distributedEntries{} := distributedEntries{stage} ++ [distributedEntry{index}]\n",
                match before.steps[event].action {
                    super::LocalAction::Send {eff_index, ..} => eff_index.dense_ordinal(),
                    super::LocalAction::Recv { .. } | super::LocalAction::Local { .. } | super::LocalAction::Terminate => unreachable!("observed send source"),
                },
                before.completion_mutations, frame.witness, stage+1, header.label().raw(), stage+1, stage+1, stage+1).unwrap();
            let preparation = if before.steps[event].reentry_scope.is_some() {
                std::format!(
                    ".fresh (rolls := (distributedChoreo.canonicalActorRollRows? distributedImage{actor}.role).get (by decide)) (roll := distributedPreparedPair{index}.1) (by decide) (by decide) (by decide)"
                )
            } else {
                ".current (by decide)".into()
            };
            writeln!(source, "private theorem distributedStep{} : DistributedSourceStep distributedImages distributedChoreo {session}\n  (distributedCut{stage}, distributedEntries{stage}) (distributedCut{}, distributedEntries{}) none := by\n  apply DistributedSourceStep.accept (sample := distributedSend{index}) (previousPath := {previous_path}) (witness := {}) (label := ⟨{frame_label}, by decide⟩)\n    (data := (distributedImage{actor}.decodeActorOrder? distributedChoreo).get (by decide))\n    (path := ((distributedImage{actor}.decodeActorOrder? distributedChoreo).get (by decide)).normalized.paths[{next_path}])\n    (row := ((distributedImage{actor}.decodeActorOrder? distributedChoreo).get (by decide)).rows[{event}])\n  · rfl\n  · decide\n  · exact (Option.some_get (x := distributedImage{actor}.decodeActorOrder? distributedChoreo) (by decide)).symm\n  · exact getElem?_pos _ _ (by decide)\n  · exact getElem?_pos _ _ (by decide)\n  · exact {preparation}\n  · decide\n  · decide\n  · decide\n  · decide\n  · unfold OwnedWireObservation.identifies; decide\n",
                stage+1,stage+1,stage+1,frame.witness, frame_label = header.label().raw()).unwrap();
            stage += 1;
            writeln!(source, "def distributedCut{} : DistributedSourceCut := distributedCut{stage}.actorAt {actor} (distributedAfter{index}, {})\n\n\
                def distributedEntries{} := distributedEntries{stage}\n\n\
                private theorem distributedStep{} : DistributedSourceStep distributedImages distributedChoreo {session}\n  (distributedCut{stage}, distributedEntries{stage}) (distributedCut{}, distributedEntries{}) (some ({actor}, distributedCommit{index})) := by\n  exact .publish (entry := distributedEntry{index}) (List.mem_append_right _ (List.mem_singleton_self _)) (by decide) (by decide) distributedMeasurement{index}\n",
                stage+1,after.completion_mutations,stage+1,stage+1,stage+1,stage+1).unwrap();
        } else {
            let sent_index = accepted[&frame.witness];
            writeln!(source, "def distributedTransport{} : TransportState := {{ distributedCut{stage}.carriers distributedChannel{index} with delivered := (distributedCut{stage}.carriers distributedChannel{index}).delivered + 1 }}\n\n\
                def distributedCut{} : DistributedSourceCut := (distributedCut{stage}.actorAt {actor} (distributedAfter{index}, {})).carrierAt distributedChannel{index} distributedTransport{}\n\n\
                def distributedEntries{} := distributedEntries{stage}\n\n\
                private theorem distributedStep{} : DistributedSourceStep distributedImages distributedChoreo {session}\n  (distributedCut{stage}, distributedEntries{stage}) (distributedCut{}, distributedEntries{}) (some ({actor}, distributedCommit{index})) := by\n  exact .receive (frame := ((distributedCut{stage}.carriers distributedChannel{index}).frames[(distributedCut{stage}.carriers distributedChannel{index}).delivered]?).get (by decide)) (entry := distributedEntry{sent_index}) (witness := {}) (by decide) (by decide) (by rfl) (by decide) (a_concrete_receive_pair_has_coherent_local_witnesses (by decide)) (by decide) distributedMeasurement{index}\n",
                stage+1,stage+1,after.completion_mutations,stage+1,stage+1,stage+1,stage+1,stage+1,frame.witness).unwrap();
        }
        stage += 1;
        records.push(std::format!("({actor}, distributedCommit{index})"));
    }
    writeln!(source, "def distributedRecords : List (Nat × ActorCommitRecord) := [{}]\n\n\
        theorem distributedActualExecution : DistributedSourceExecution distributedImages distributedChoreo {session}\n  (distributedCut0, distributedEntries0) (distributedCut{stage}, distributedEntries{stage}) distributedRecords := by\n  exact {}\n\n\
        theorem distributedActorHistories (actor : Nat) : ∃ cleared commits, ActorMeasuredHistory (distributedImages actor) distributedChoreo {session}\n  (distributedCut0.actors actor) (distributedCut{stage}.actors actor) cleared commits :=\n  a_distributed_source_execution_projects_each_actual_actor_history distributedActualExecution actor\n\n\
        theorem distributedFifoHistories (channel : TransportChannel) : ∃ history, SendAdmissionTrace distributedChoreo {session}\n  (distributedCut{stage}.carriers channel) history (sourceChannelRecords distributedEntries{stage} channel) :=\n  a_distributed_source_execution_projects_its_complete_fifo_histories distributedActualExecution channel\n",
        records.join(", "), (1..=stage).fold(".refl".into(), |proof, boundary| std::format!("(.tail {proof} distributedStep{boundary})"))).unwrap();
    write_frame_words(initial, &mut source);
    source
}

/// Derive every remote row's complete wire class from the captured image.
/// No row/selector is chosen to make the source-word check pass. This only
/// serializes proof data after the Endpoint execution has completed.
#[cfg(not(miri))]
fn write_frame_words(initial: &[ActorSnapshot; 3], source: &mut std::string::String) {
    use std::fmt::Write;
    source.insert_str(0, "import Hibana.SourceFrameWord\n");
    for snapshot in initial {
        let actor = snapshot.actor;
        writeln!(source, "private def distributedWordData{actor} := (distributedImage{actor}.decodeActorOrder? distributedChoreo).get (by decide)\n\
            private def distributedWordRolls{actor} := distributedImage{actor}.decodeRollRows?.get (by decide)\n").unwrap();
        for (event, step) in snapshot.steps.iter().enumerate() {
            if matches!(step.action, super::LocalAction::Local { .. }) {
                continue;
            }
            let name = std::format!("distributedFrameClass{actor}_{event}");
            writeln!(source, "private def distributedFrameKey{actor}_{event} := (distributedImage{actor}.frameKey? {event}).get (by decide)\n\n\
              private def {name} : SourceOrderGroup distributedImage{actor} distributedChoreo := {{\n\
              rows := distributedImage{actor}.frameClassRows distributedFrameKey{actor}_{event}\n\
              distinct := by decide\n\
              data := distributedWordData{actor}\n\
              rolls := distributedWordRolls{actor}\n\
              choices := (distributedImage{actor}.frameClassChoices? distributedChoreo\n\
                distributedFrameKey{actor}_{event}).get (by decide)\n\
              decoded := (Option.some_get _).symm\n\
              decodedRolls := (Option.some_get _).symm\n\
              nonempty := by decide\n\
              paths := by decide\n\
              choiceMembers := by decide\n\
              slots := by decide\n\
              active := by decide\n\
              ordered := by decide\n\
              uniformReset := by decide\n\
              outsideChoices := by decide\n\
            }}\n").unwrap();
        }
        writeln!(source, r#"theorem distributedActor{actor}AllFrameClassesFollowTheirSourceWords
    (event : Nat) (bounded : event ∈ List.range distributedImage{actor}.eventCount)
    (key : SourceFrameKey) (keyAt : distributedImage{actor}.frameKey? event = some key) :
    sourceFrameWord (distributedImage{actor}.frameClassRows key)
      (committedFrameClassWord (distributedImage{actor}.frameClassRows key) {actor} distributedRecords) := by
  have count : distributedImage{actor}.eventCount = {} := by decide
  have bound := List.mem_range.mp bounded
  rw [count] at bound
  have alternatives : {} := by omega
  rcases alternatives with {}"#,
            snapshot.steps.len(),
            (0..snapshot.steps.len()).map(|event| std::format!("event = {event}")).collect::<std::vec::Vec<_>>().join(" ∨ "),
            (0..snapshot.steps.len()).map(|event| std::format!("at{event}")).collect::<std::vec::Vec<_>>().join(" | ")).unwrap();
        for (event, step) in snapshot.steps.iter().enumerate() {
            writeln!(source, "  · subst event").unwrap();
            if matches!(step.action, super::LocalAction::Local { .. }) {
                writeln!(source, "    have absent : distributedImage{actor}.frameKey? {event} = none := by decide\n    rw [absent] at keyAt\n    cases keyAt").unwrap();
            } else {
                writeln!(source, r#"    have actual : distributedImage{actor}.frameKey? {event} = some distributedFrameKey{actor}_{event} := by decide
    have same := Option.some.inj (keyAt.symm.trans actual)
    subst key
    exact (a_measured_distributed_class_has_its_source_word_and_exact_prefix
      (actor := {actor}) (group := distributedFrameClass{actor}_{event}) distributedActualExecution (by decide)).1"#).unwrap();
            }
        }
    }
}

#[cfg(not(miri))]
pub(super) fn write_observations(
    initial: &[ActorSnapshot; 3],
    operations: &[OperationObservation],
    candidates: &[(ActorSnapshot, std::vec::Vec<bool>)],
) {
    let mut source = execution_source::<super::mixed_send_admission::Steps, 16>(
        initial,
        operations,
        &std::vec![(0, 0); operations.len()],
    );
    source.insert_str(
        0,
        "import Hibana.FifoOccurrenceOrigin\nimport Hibana.SourcePublicationPrefix\nimport Hibana.SourceOwnedReceive\nimport Hibana.OccurrenceResourceOrigin\n",
    );
    use std::fmt::Write;
    writeln!(source, "private theorem distributedBeforeFreshReceiptExecution : DistributedSourceExecution distributedImages distributedChoreo 9062\n  (distributedCut0, distributedEntries0) (distributedCut13, distributedEntries13) (distributedRecords.take 8) := by\n  exact {}\n",
        (1..=13).fold(".refl".into(), |proof, boundary| std::format!("(.tail {proof} distributedStep{boundary})"))).unwrap();
    source.push_str(r#"
theorem distributedMixedCutKeepsIndependentActorVisits :
    (distributedCut13.actors 0).2 = 9 ∧
    (distributedCut13.actors 1).2 = 3 ∧
    (distributedCut13.carriers distributedChannel8).delivered = 2 ∧
    (distributedCut13.carriers distributedChannel8).frames.length = 4 := by decide

theorem distributedRepeatedLoanHasIdenticalBytes :
    distributedWire0 = distributedWire6 ∧ distributedWire6 = distributedWire8 := by decide

theorem distributedFreshReceiptAlsoFitsOldBytes :
    sourceReceivePairCheck distributedSend0 distributedCommit8 distributedImage1 distributedChoreo 9062 = true := by decide

theorem distributedFreshReceiptHasItsActualFifoHead :
    (sourceChannelEntries distributedEntries13 distributedChannel8)[
      (distributedCut13.carriers distributedChannel8).delivered]? = some distributedEntry6 := by decide

theorem distributedOldWitnessCannotIdentifyTheFreshFifoHead
    (entry : SourceAcceptance)
    (head : (sourceChannelEntries distributedEntries13 distributedChannel8)[
      (distributedCut13.carriers distributedChannel8).delivered]? = some entry) :
    entry.carrierWitness ≠ distributedEntry0.carrierWitness := by
  have identical := Option.some.inj (distributedFreshReceiptHasItsActualFifoHead.symm.trans head)
  subst entry
  decide

theorem distributedFreshReceiptResetsEvenWhenItsRowBitStaysTrue :
    distributedBefore8.doneState.done 0 = true ∧
    distributedAfter8.doneState.done 0 = true ∧
    distributedBefore8.progress ≠ distributedAfter8.progress ∧
    distributedPreparedPair8.2.before.doneState.done 0 = false := by decide

theorem distributedCurrentReceiptHasItsActualUnresetGlobalOrigin :
    ∃ before after prior later record marked untouched commits,
      DistributedSourceExecution distributedImages distributedChoreo 9062
        (distributedCut0, distributedEntries0) before prior ∧
      DistributedSourceStep distributedImages distributedChoreo 9062 before after (some (1, record)) ∧
      ActorMeasuredCommit distributedImage1 distributedChoreo 9062
        (before.1.actors 1) (after.1.actors 1) marked record ∧
      DistributedSourceExecution distributedImages distributedChoreo 9062
        after (distributedCut24, distributedEntries24) later ∧
      ActorMeasuredHistory distributedImage1 distributedChoreo 9062
        (after.1.actors 1) (distributedCut24.actors 1) untouched commits ∧
      0 ∉ untouched ∧ record.step.event = 0 :=
  a_current_distributed_row_has_its_unreset_actual_boundary distributedActualExecution (by decide) (by decide)

theorem distributedActualReceiveBoundaries
    (actor eff : Nat) (record : ActorCommitRecord) (binding : Binding)
    (member : (actor, record) ∈ distributedRecords)
    (receiveAt : sourceOperationIdentity? (distributedImages actor) distributedChoreo
      record.step.pathIndex record.step.event = some (eff, binding, .receive)) :
    ∃ before after prior later channel entry witness next frame,
      DistributedSourceExecution distributedImages distributedChoreo 9062
        (distributedCut0, distributedEntries0) before prior ∧
      DistributedSourceStep distributedImages distributedChoreo 9062 before after (some (actor, record)) ∧
      (before.1.carriers channel).pollReceive = .delivered frame next ∧
      (sourceChannelEntries before.2 channel)[(before.1.carriers channel).delivered]? = some entry ∧
      entry.carrierWitness = witness ∧
      SourceReceivePair entry.sample record (distributedImages actor) distributedChoreo 9062 ∧
      DistributedSourceExecution distributedImages distributedChoreo 9062
        after (distributedCut24, distributedEntries24) later :=
  a_logged_receive_has_its_exact_fifo_acceptance distributedActualExecution member receiveAt

def distributedFreshSentFacts := (distributedEntry6.sample.ownedFacts? distributedChoreo).get (by decide)
def distributedFreshReceivedFacts := (distributedCommit8.step.ownedFacts? distributedImage1 distributedChoreo).get (by decide)

theorem distributedProspectiveReceiptHasItsSourceInstanceBeforePairValidation :
    ∃ history key following,
      SendAdmissionTrace distributedChoreo 9062 (distributedCut13.carriers distributedChannel8) history
        (sourceChannelRecords distributedEntries13 distributedChannel8) ∧
      history.accepted[(distributedCut13.carriers distributedChannel8).delivered]? = some key ∧
      key.globalId = distributedEntry6.event ∧
      key.iteration = sourceOccurrenceOrdinal
        (sourceChannelRecords distributedEntries13 distributedChannel8)
        (distributedCut13.carriers distributedChannel8).delivered distributedEntry6.event ∧
      distributedEntry6.sample.effIndex? = some distributedEntry6.event ∧
      distributedEntry6.sample.wire.identifies
        (((distributedCut13.carriers distributedChannel8).frames[2]?).get (by decide)) ∧
      distributedFreshSentFacts.resource = distributedFreshReceivedFacts.resource ∧
      distributedFreshSentFacts.effIndex = distributedFreshReceivedFacts.effIndex ∧
      history.receive? = some (key, following) ∧
      SendAdmissionTrace distributedChoreo 9062 distributedTransport14 following
        (sourceChannelRecords distributedEntries13 distributedChannel8) :=
  a_prospective_owned_receipt_has_its_distributed_fifo_source_instance
    distributedBeforeFreshReceiptExecution (by decide) distributedFreshReceiptHasItsActualFifoHead
    ((Option.some_get (x := distributedEntry6.sample.ownedFacts? distributedChoreo) (by decide)).symm)
    (by decide)
    ((Option.some_get (x := distributedCommit8.step.ownedFacts? distributedImage1 distributedChoreo) (by decide)).symm)
    rfl

theorem distributedFirstAcceptedSendHasItsUnpublishedPrefix :
    ∃ pending,
      actorAcceptedSends distributedEntries1 0 = pending ∧ pending.length = 1 ∧
      ∀ entry ∈ pending, entry.stamps 0 (distributedCut1.actors 0).2 := by
  obtain ⟨pairs, pending, accepted, published, _, _, stamped⟩ :=
    a_distributed_actors_acceptances_lead_publications_by_at_most_one
      (actor := 0) (DistributedSourceExecution.tail .refl distributedStep1) rfl
  have lengths := congrArg List.length published
  have empty : pairs = [] := List.eq_nil_of_length_eq_zero (by
    simpa [actorPublishedSends] using lengths.symm)
  have equal : actorAcceptedSends distributedEntries1 0 = pending := by
    simpa only [empty, List.map_nil, List.nil_append] using accepted
  have count : (actorAcceptedSends distributedEntries1 0).length = 1 := by decide
  rw [equal] at count
  exact ⟨pending, equal, count, stamped⟩

private theorem distributedFreshPreparationExists :
    (prepareObservedActorBody? distributedImage1 distributedChoreo distributedBefore8 0 0 0 0).isSome = true := by decide

private def distributedFreshPreparation :=
  (prepareObservedActorBody? distributedImage1 distributedChoreo distributedBefore8 0 0 0 0).get distributedFreshPreparationExists

private theorem distributedFreshPreparedSourceIdentity :
    distributedEntry6.sample.effIndex? = distributedImage1.eventEffIndex? 0 := by
  have sender := (a_measured_distributed_class_has_its_source_word_and_exact_prefix
    (actor := 0) (group := distributedFrameClass0_0) distributedBeforeFreshReceiptExecution (by decide)).1
  have receiver := (a_prepared_wire_operation_extends_its_actors_measured_source_word
    (actor := 1) (group := distributedFrameClass1_0) (previousPath := 0) (pathIndex := 0) (event := 0)
    (base := distributedFreshPreparation.2) (data := distributedWordData1)
    (path := distributedWordData1.normalized.paths[0]) (row := distributedWordData1.rows[0])
    distributedBeforeFreshReceiptExecution (by decide)
    ((Option.some_get (x := distributedImage1.decodeActorOrder? distributedChoreo) (by decide)).symm)
    (getElem?_pos _ _ (by decide)) (getElem?_pos _ _ (by decide))
    (by decide) (by decide)
    (.fresh
      (rolls := (distributedChoreo.canonicalActorRollRows? distributedImage1.role).get (by decide))
      (roll := distributedFreshPreparation.1) (by decide) (by decide)
      (Option.some_get distributedFreshPreparationExists).symm)
    (by decide)).1
  exact source_frame_words_match_at_each_common_class_position
    (sentSource := distributedImage0.eventEffIndex?) (receivedSource := distributedImage1.eventEffIndex?)
    (index := 2) (sent := distributedEntry6.sample.event) (received := 0)
    (by decide) sender receiver (by decide) (by decide)

theorem distributedFreshMixedSourceIdentityBeforeItsNewReceivePair :
    distributedEntry6.sample.effIndex? = distributedImage1.eventEffIndex? distributedCommit8.step.event :=
  distributedFreshPreparedSourceIdentity
"#);
    source.push_str(include_str!("distributed_owned_receive.lean"));
    source.push_str("\nprivate def distributedCandidateData := (distributedImage0.decodeActorOrder? distributedChoreo).get (by decide)\n\n\
        private def distributedCandidatePath := (distributedCandidateData.normalized.paths[0]?).get (by decide)\n\n\
        private def distributedCandidateRolls := (distributedChoreo.canonicalActorRollRows? 0).get (by decide)\n\n\
        private def distributedCandidateRoll := (distributedCandidateRolls[0]?).get (by decide)\n\n");
    for (index, (before, admitted)) in candidates.iter().enumerate() {
        let label = if index % 2 == 0 { 1 } else { 9 };
        let event = before
            .steps
            .iter()
            .position(|step| matches!(step.action, super::LocalAction::Send { label: found, .. } if found == label))
            .unwrap();
        writeln!(source, "private def distributedCandidate{index} : ActorAdmissionObservation := {{ snapshot := {}, pathIndex := 0, admitted := {admitted:?}, retainedRolls := {:?} }}\n\n\
            private def distributedCandidateRow{index} := (distributedCandidateData.rows[{event}]?).get (by decide)\n",
            super::commit::observation_source(before), before.retained_rolls).unwrap();
        let base = if let Some(scope) = before.steps[event].reentry_scope {
            std::format!(
                "((prepareObservedActorBody? distributedImage0 distributedChoreo distributedCandidate{index}.snapshot 0 0 {scope} {event}).get (by decide)).2"
            )
        } else {
            std::format!("distributedCandidate{index}.snapshot")
        };
        writeln!(
            source,
            "private def distributedCandidateBase{index} : ActorProgressObservation := {base}\n"
        )
        .unwrap();
    }
    source.push_str(include_str!("distributed_occurrence.lean"));
    let directory = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/lean-proof");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("DistributedSourceGenerated.lean"), source).unwrap();
}
