private theorem asyncFirstAcceptance : SourceCallHistory asyncChoreo 9023
    [.accept asyncEntry0] [asyncEntry0] [] [asyncEntry0] :=
  .accept .nil (by decide) (by decide) (by decide)

private theorem asyncBothAcceptances : SourceCallHistory asyncChoreo 9023
    [.accept asyncEntry0, .accept asyncEntry1]
    [asyncEntry0, asyncEntry1] [] [asyncEntry0, asyncEntry1] :=
  .accept asyncFirstAcceptance (by decide) (by decide) (by decide)

private theorem asyncIndependentPublishes : SourceCallHistory asyncChoreo 9023
    [.accept asyncEntry0, .accept asyncEntry1, .publish asyncEntry1.callKey asyncCommit1]
    [asyncEntry0, asyncEntry1] [(asyncEntry1, asyncCommit1)] [asyncEntry0] :=
  .publish (left := [asyncEntry0]) (right := []) asyncBothAcceptances (by decide)

theorem asyncIndependentPublicationIsAnActualMeasuredCommit :
    ActorMeasuredCommit asyncImage asyncChoreo 9023 (asyncCut1, 0) (asyncCut2, 1) [] asyncCommit1 :=
  .ordinary (record := { commit := asyncCommit1, before := 0, after := 1 }) (by decide) (by decide)

theorem asyncLoanPublicationIsAnActualMeasuredCommit :
    ActorMeasuredCommit asyncImage asyncChoreo 9023 (asyncCut2, 1) (asyncCut3, 2) [] asyncCommit0 :=
  .ordinary (record := { commit := asyncCommit0, before := 1, after := 2 }) (by decide) (by decide)

theorem asyncSameActorPublishesInReverseAcceptanceOrder : SourceCallHistory asyncChoreo 9023
    [.accept asyncEntry0, .accept asyncEntry1,
      .publish asyncEntry1.callKey asyncCommit1, .publish asyncEntry0.callKey asyncCommit0]
    [asyncEntry0, asyncEntry1] [(asyncEntry1, asyncCommit1), (asyncEntry0, asyncCommit0)] [] :=
  .publish (left := []) (right := []) asyncIndependentPublishes (by decide)

theorem asyncOutstandingAcceptancesNeedNoActorWideStamp :
    asyncEntry0.sample.image.role = asyncEntry1.sample.image.role ∧
    asyncEntry0.mutation = asyncEntry1.mutation ∧
    asyncEntry0.sample.event ≠ asyncEntry1.sample.event := by decide

theorem asyncEarlierActorWideAcceptanceGuardRejectsTheLegalOverlap :
    [asyncEntry0].all (fun entry => !(entry.sample.image.role == asyncEntry1.sample.image.role &&
      entry.mutation == asyncEntry1.mutation)) = false := by decide

theorem asyncIndependentProgressChangesTheWholeActorBase :
    asyncEntry0.sample.base.progress ≠ asyncCommit0.step.before.progress ∧
    sourcePublicationPairCheck asyncEntry0.sample asyncCommit0 asyncImage asyncChoreo 9023 = false ∧
    retainedSendPublicationCheck asyncEntry0 asyncCommit0 asyncChoreo 9023 = true := by decide

theorem asyncAcceptancesAreConservedDespiteReorderedPublications :
    [asyncEntry0, asyncEntry1].Perm [asyncEntry1, asyncEntry0] := by
  simpa using a_source_call_history_partitions_its_acceptances
    asyncSameActorPublishesInReverseAcceptanceOrder

theorem asyncNoAcceptanceCanPublishTwice :
    ¬ ∃ word pending, SourceCallHistory asyncChoreo 9023 word [asyncEntry0, asyncEntry1]
      [(asyncEntry1, asyncCommit1), (asyncEntry0, asyncCommit0), (asyncEntry0, asyncCommit0)] pending := by
  rintro ⟨word, pending, history⟩
  have distinct := a_source_call_history_never_duplicates_a_publication history
  simp at distinct

theorem asyncOldOccurrenceKeyCannotBeAcceptedAgain :
    ¬ ∃ word published pending,
      SourceCallHistory asyncChoreo 9023 word [asyncEntry0, asyncEntry1, asyncEntry0] published pending := by
  rintro ⟨word, published, pending, history⟩
  have distinct := a_source_call_history_never_reuses_an_acceptance_key history
  simp at distinct
