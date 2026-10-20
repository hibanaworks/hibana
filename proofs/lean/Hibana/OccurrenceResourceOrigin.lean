import Hibana.SourceOccurrenceTrace
import Hibana.SourceReceiptDependency
import Hibana.SourceOwnedReceive

namespace Hibana
open EndpointDelegation

private theorem unfolding_keeps_resource_membership
    (unfolding : SourceUnfolding source expanded) (key : EndpointKey) :
    key ∈ expanded.resources ↔ key ∈ source.resources := by
  induction unfolding with
  | send => rfl
  | seq first second left right | par first second left right
  | route resolver first second left right =>
    simpa only [Source.resources, List.mem_eraseDups, List.mem_append] using or_congr left right
  | rollOnce body ih => exact ih
  | rollMore earlier next left right =>
    simp only [Source.resources, List.mem_eraseDups, List.mem_append, left, right, or_self]
  | resumeAfterContinuation earlier next left right =>
    simp only [Source.resources, List.mem_eraseDups, List.mem_append] at left ⊢
    rw [left, right]
    constructor
    · intro observed
      exact observed.elim id Or.inl
    · exact Or.inl
  | resumeAlongsideContinuation earlier next tail first second third =>
    simp only [Source.resources, List.mem_eraseDups, List.mem_append] at first second third ⊢
    rw [first, second, third]
    constructor
    · intro observed
      rcases observed with first | second | tail
      · exact Or.inl first
      · exact Or.inl second
      · exact Or.inr tail
    · intro observed
      exact observed.elim Or.inl (fun tail => Or.inr (Or.inr tail))

/-- Finite expansion does not create another initial owner, even when visits
overlap a continuation or independent parallel work. -/
theorem source_unfolding_preserves_each_initial_owner
    (unfolding : SourceUnfolding source expanded) (key : EndpointKey) :
    initialOwners expanded.resources key = initialOwners source.resources key := by
  have membership := unfolding_keeps_resource_membership unfolding key
  by_cases present : key ∈ source.resources
  · have actual := membership.mpr present
    simp [initialOwners, present, actual]
  · have actual : key ∉ expanded.resources := fun member => present (membership.mp member)
    simp [initialOwners, present, actual]

private theorem an_accepted_occurrence_keeps_its_call_and_source
    (trace : SourceOccurrenceTrace images choreo plan word calls points) :
    ∀ entry, DistributedCallBoundary.accept entry ∈ word →
      ∃ occurrence, occurrenceCall? calls entry.callKey = some occurrence ∧
        plan.matchesAction entry.sample.image choreo entry.sample.image.role
          entry.sample.pathIndex entry.sample.event occurrence .send = true := by
  induction trace with
  | nil => simp
  | @accept word calls points entry occurrence prior fresh origin matched ready ih =>
    intro actual member
    rcases List.mem_append.mp member with earlier | current
    · obtain ⟨previous, resolved, sourceAt⟩ := ih actual earlier
      exact ⟨previous, a_resolved_occurrence_call_survives_a_prefix
        (List.prefix_append _ _) resolved, sourceAt⟩
    · have same := List.mem_singleton.mp current
      cases same
      have absent : calls.find? (fun pair => decide (pair.1 = entry.callKey)) = none := by
        apply List.find?_eq_none.mpr
        intro pair present
        have different := of_decide_eq_true (List.all_eq_true.mp fresh pair present)
        exact fun equal => different (of_decide_eq_true equal)
      refine ⟨occurrence, ?_, matched⟩
      simp [occurrenceCall?, List.find?_append, absent]
  | quiet prior ih | retireView prior ih | close prior ih | abort prior ih
  | observeClosure prior ih | transportFault prior ih
  | publish prior callAt matched ready ih | receive prior callAt matched ready ih
  | localCommit prior matched ready ih =>
    intro entry member
    exact ih entry (by simpa using member)

private theorem a_matched_occurrence_has_its_exact_operation
    {plan : SourceOccurrenceCut source}
    (located : plan.path.bindings.find? (fun entry => entry.1 == occurrence) =
      some (occurrence, binding))
    (matched : plan.matchesAction image choreo actor pathIndex row occurrence side = true) :
    ∃ eff, sourceOperationIdentity? image choreo pathIndex row = some (eff, binding, side) := by
  unfold SourceOccurrenceCut.matchesAction at matched
  rw [located] at matched
  cases originAt : (plan.unfolding.occurrences 0)[occurrence]? with
  | none => simp only [originAt, Bool.false_eq_true] at matched
  | some origin =>
    cases identityAt : sourceOperationIdentity? image choreo pathIndex row with
    | none => simp only [originAt, identityAt, Bool.false_eq_true] at matched
    | some actual =>
      obtain ⟨eff, actualBinding, actualSide⟩ := actual
      cases actionAt : action? plan.path.bindings ⟨occurrence, side⟩ with
      | none => simp only [originAt, identityAt, actionAt, Bool.false_eq_true] at matched
      | some action =>
        simp only [originAt, identityAt, actionAt, decide_eq_true_eq] at matched
        exact ⟨eff, by simp only [matched.2.2.1, matched.2.2.2.1]⟩

private theorem a_physical_acceptance_keeps_its_source_check
    (execution : DistributedPhysicalExecution images choreo session first last word entries published pending) :
    ∀ entry, DistributedCallBoundary.accept entry ∈ word → entry.sample.check choreo session = true := by
  induction execution with
  | refl => simp
  | accept history imageAt checked decoded pathAt rowAt prepared slotFree freshWitness sourceAt accepted identity ih =>
    intro entry member
    rcases List.mem_append.mp member with earlier | current
    · exact ih entry earlier
    · have same := List.mem_singleton.mp current
      cases same
      exact checked
  | transportFault history checked beforeAt ih
  | close history ih
  | observeClosure history checked beforeAt bound closed ih
  | abort history ih
  | retireView history checked beforeAt ih
  | quiet history checked headers ih
  | publish history actorAt sameWire committed ih
  | localCommit history actorRole localAt committed ih
  | receive history actorRole received entryAt identity sameWire committed ih =>
    intro entry member
    exact ih entry (by simpa using member)

/-- Retain the actual carrier call as well as the static source match. The
same source row and bytes can occur in several visits; only the call lookup
identifies the accepted occurrence in this chronological execution. -/
theorem an_observed_occurrence_receipt_has_its_same_word_boundary
    (trace : SourceOccurrenceTrace images choreo plan word calls points)
    (observed : SourcePoint.committed ⟨occurrence, .receive⟩ ∈ points) :
    ∃ key actor record, .receive key actor record ∈ word ∧
      plan.matchesAction (images actor) choreo actor record.step.pathIndex
        record.step.event occurrence .receive = true ∧
      occurrenceCall? calls key = some occurrence := by
  induction trace with
  | nil => simp at observed
  | quiet prior ih | retireView prior ih | close prior ih | abort prior ih
  | observeClosure prior ih | transportFault prior ih =>
    obtain ⟨key, actor, record, present, matched, callAt⟩ := ih observed
    exact ⟨key, actor, record, List.mem_append_left _ present, matched, callAt⟩
  | accept prior fresh origin matched ready ih =>
    obtain ⟨key, actor, record, present, actual, callAt⟩ := ih (by simpa using observed)
    exact ⟨key, actor, record, List.mem_append_left _ present, actual,
      a_resolved_occurrence_call_survives_a_prefix (List.prefix_append _ _) callAt⟩
  | publish prior callAt matched ready ih
  | localCommit prior matched ready ih =>
    obtain ⟨key, actor, record, present, actual, resolved⟩ := ih (by simpa using observed)
    exact ⟨key, actor, record, List.mem_append_left _ present, actual, resolved⟩
  | @receive word calls points key actor actual record prior callAt matched ready ih =>
    rcases List.mem_append.mp observed with old | last
    · obtain ⟨earlierKey, earlierActor, earlierRecord, present, actualMatch, resolved⟩ := ih old
      exact ⟨earlierKey, earlierActor, earlierRecord, List.mem_append_left _ present, actualMatch, resolved⟩
    · have same : occurrence = actual := by simpa using List.mem_singleton.mp last
      subst occurrence
      exact ⟨key, actor, record, List.mem_append_right _ List.mem_cons_self, matched, callAt⟩

private theorem a_matched_occurrence_has_its_source_actor
    {plan : SourceOccurrenceCut source}
    (matched : plan.matchesAction image choreo actor pathIndex row occurrence side = true) :
    (action? plan.path.bindings ⟨occurrence, side⟩).map Action.actor = some actor := by
  unfold SourceOccurrenceCut.matchesAction at matched
  split at matched <;>
    simp_all only [Bool.false_eq_true, decide_eq_true_eq, Option.map_some]

/-- Derive the exact source origin of a required resource from the projected
query and the SAME occurrence word. The original issuance is not supplied by
the caller. In the other case, the last symbolic handoff before this target
has a real receive record for that occurrence and actor. The complete source
prefix witness remains available for excluding intervening handoffs.

This supplies an origin, not the claim that its Rust value is still held now.
Current custody and later accepted transfers must be composed separately. -/
theorem a_ready_occurrence_resource_has_its_initial_owner_or_same_word_receipt
    {plan : SourceOccurrenceCut source} {target : ActionId} {binding : Binding}
    {action : Action} {resource : EndpointKey}
    (trace : SourceOccurrenceTrace images choreo plan word calls points)
    (located : plan.path.bindings.find? (fun entry => entry.1 == target.event) =
      some (target.event, binding))
    (decoded : action? plan.path.bindings target = some action)
    (required : match target.side with
      | .receive => binding.message.receiver = resource
      | .send => binding.message.sender = resource ∨ binding.message.payload = .endpoint resource
      | .local => binding.message.sender = resource)
    (localReady : occurrenceLocalReady plan action.actor
      (occurrenceLocalHistory plan action.actor points) target.entryPoint = true)
    (wire : occurrenceWireReady points target.entryPoint = true) :
    ∃ run index symbolic,
      elaborate plan.expanded.resources (initialOwners plan.expanded.resources)
        plan.path.selections plan.expanded 0 = some run ∧
      run.bindings[index]? = some (target.event, symbolic) ∧
      instantiateBinding plan.normalized.issuance symbolic = some binding ∧
      run.equations.all (equationAccepted plan.normalized.issuance) = true ∧
      (run.bindings.mapM fun (event, item) => do
        let value ← instantiateBinding plan.normalized.issuance item
        some (event, value)) = some plan.path.bindings ∧
      ((instantiate plan.original.issuance (initialOwners source.resources resource) = some action.actor ∧
          ∀ entry ∈ run.bindings.take index, entry.2.message.payload ≠ .endpoint resource) ∨
        ∃ previousEvent previousBinding previousAction earlier previous later key record,
          run.bindings.take index = earlier ++ (previousEvent, previous) :: later ∧
          previous.message.payload = .endpoint resource ∧
          (∀ entry ∈ later, entry.2.message.payload ≠ .endpoint resource) ∧
          instantiateBinding plan.normalized.issuance previous = some previousBinding ∧
          plan.path.bindings.find? (fun entry => entry.1 == previousEvent) = some (previousEvent, previousBinding) ∧
          previousBinding.message.payload = .endpoint resource ∧
          action? plan.path.bindings ⟨previousEvent, .receive⟩ = some previousAction ∧
          previousAction.actor = action.actor ∧
          SourcePointEdge plan.expanded plan.path.bindings
            (.committed ⟨previousEvent, .receive⟩) target.entryPoint ∧
          points.contains (.committed ⟨previousEvent, .receive⟩) = true ∧
          .receive key action.actor record ∈ word ∧
          plan.matchesAction (images action.actor) choreo action.actor record.step.pathIndex
            record.step.event previousEvent .receive = true ∧
          occurrenceCall? calls key = some previousEvent) := by
  obtain ⟨run, elaborated, solved, mapped⟩ :=
    accepted_normalized_path_has_its_exact_issuance_derivation plan.accepted plan.selected
  obtain ⟨⟨event, symbolic⟩, sourceMember, lowered⟩ := successful_option_mapM_has_input
    mapped (target.event, binding) (List.mem_of_find?_eq_some located)
  cases bindingAt : instantiateBinding plan.normalized.issuance symbolic with
  | none => simp [bindingAt] at lowered
  | some actual =>
    have same : (event, actual) = (target.event, binding) := by simpa [bindingAt] using lowered
    cases same
    obtain ⟨index, bound, atIndex⟩ := List.mem_iff_getElem.mp sourceMember
    have present : run.bindings[index]? = some (target.event, symbolic) := by
      rw [List.getElem?_eq_getElem bound, atIndex]
    have messageAt := (instantiated_binding_retains_message_and_actor_references bindingAt).1
    have sourceRequired : match target.side with
        | .receive => symbolic.message.receiver = resource
        | .send => symbolic.message.sender = resource ∨ symbolic.message.payload = .endpoint resource
        | .local => symbolic.message.sender = resource := by simpa only [messageAt] using required
    have globallyReady := (projected_occurrence_readiness_is_exact plan points target.entryPoint).mpr
      ⟨action.actor, localReady, wire⟩
    obtain ⟨next, step⟩ := a_checked_source_occurrence_has_its_exact_transition
      (occurrenceCut plan points) target.entryPoint globallyReady
    have dependencies : ∀ before, SourcePointEdge plan.expanded plan.path.bindings before target.entryPoint →
        points.contains before = true := by
      cases step with
      | advance present pending ready => exact ready
    refine ⟨run, index, symbolic, elaborated, present, bindingAt, solved, mapped, ?_⟩
    rcases accepted_resource_action_has_initial_authority_or_a_receipt_dependency
        elaborated present bindingAt solved mapped sourceRequired decoded with
      ⟨initialActor, initialAt, actorAt, untouched⟩ |
      ⟨previousEvent, previousBinding, previousAction, previousAt, payload,
        previousDecoded, sameActor, bound, before, edge, earlier, previous, later,
        shape, transferred, untouched, instantiated⟩
    · have issuer : instantiate plan.normalized.issuance
          (initialOwners plan.expanded.resources resource) = some action.actor :=
        initialAt.trans (congrArg some actorAt.symm)
      rw [plan.sameIssuance, source_unfolding_preserves_each_initial_owner plan.unfolding] at issuer
      exact .inl ⟨issuer, untouched⟩
    · have received := dependencies _ edge
      obtain ⟨key, actor, record, inWord, matched, callAt⟩ :=
        an_observed_occurrence_receipt_has_its_same_word_boundary trace
          (List.contains_iff_mem.mp received)
      have atActor := a_matched_occurrence_has_its_source_actor matched
      rw [previousDecoded, Option.map_some, Option.some.injEq] at atActor
      have actualActor : actor = action.actor := atActor.symm.trans sameActor
      exact .inr ⟨previousEvent, previousBinding, previousAction, earlier, previous, later,
        key, record, shape, transferred, untouched, instantiated, previousAt, payload,
        previousDecoded, sameActor, edge, received,
        by simpa only [actualActor] using inWord, by simpa only [actualActor] using matched, callAt⟩

open OwnedCallHistory

private theorem an_owned_event_has_its_same_history_boundary
    (history : OwnedCallHistory.History images resource choreo initialValue events cut) :
    ∀ event ∈ events, ∃ earlier later before after,
      events = earlier ++ event :: later ∧
      OwnedCallHistory.History images resource choreo initialValue earlier before ∧
      OwnedCallHistory.Boundary images resource choreo earlier event before after := by
  induction history with
  | nil => simp
  | @tail events before next after prior boundary ih =>
    intro event member
    rcases List.mem_append.mp member with old | current
    · obtain ⟨earlier, later, first, last, shape, prefixHistory, actual⟩ := ih event old
      exact ⟨earlier, later ++ [next], first, last,
        by simp only [shape, List.append_assoc, List.cons_append], prefixHistory, actual⟩
    · have same := List.mem_singleton.mp current
      cases same
      exact ⟨events, [], before, after, by simp, prior, boundary⟩

private theorem a_prior_acceptance_survives_its_suffix
    (member : entry ∈ acceptedEntries earlier) (later : List OwnedCallEvent) :
    entry ∈ acceptedEntries (earlier ++ later) := by
  simp only [acceptedEntries, sourceWord, List.filterMap_append] at member ⊢
  exact List.mem_append_left _ member

private theorem an_acceptance_is_in_its_source_word
    {word : List DistributedCallBoundary}
    (member : entry ∈ word.filterMap DistributedCallBoundary.acceptance?) :
    DistributedCallBoundary.accept entry ∈ word := by
  obtain ⟨boundary, present, projected⟩ := List.mem_filterMap.mp member
  cases boundary <;> simp only [DistributedCallBoundary.acceptance?, reduceCtorEq] at projected
  case accept actual =>
    cases Option.some.inj projected
    exact present

private theorem a_tracked_receive_has_its_actual_issued_view
    (history : OwnedCallHistory.History images resource choreo initialValue events cut)
    (received : DistributedCallBoundary.receive key actor record ∈ sourceWord events)
    (tracked : ∀ entry ∈ acceptedEntries events, entry.callKey = key → tracks resource choreo entry = true) :
    ∃ frame facts,
      OwnedCallEvent.receive key actor record frame (actor, facts.afterView.header) ∈ events ∧
      record.step.ownedFacts? (images actor) choreo = some facts ∧ facts.resource = resource ∧
      facts.side = .receive ∧ record.step.after.actor = actor ∧ record.wire.identifies frame := by
  obtain ⟨event, present, projected⟩ := List.mem_filterMap.mp received
  obtain ⟨earlier, later, before, after, shape, prior, boundary⟩ :=
    an_owned_event_has_its_same_history_boundary history event present
  cases boundary <;> simp only [OwnedCallEvent.source?, Option.some.injEq, reduceCtorEq] at projected
  case source observed untracked =>
    subst observed
    obtain ⟨entry, member, keyAt, untrackedAt⟩ := untracked
    have inAll := a_prior_acceptance_survives_its_suffix member
      (OwnedCallEvent.source (.receive key actor record) :: later)
    rw [← shape] at inAll
    have actual := tracked entry inAll keyAt
    rw [actual] at untrackedAt
    contradiction
  case receive state rest requests actualKey actualActor actualRecord frame facts factsAt resourceAt receiving actorAt wire =>
    cases projected
    exact ⟨frame, facts, present, factsAt, resourceAt, receiving, actorAt, wire⟩

/-- The causal occurrence receipt is the actual owned receive in this SAME
custody/physical word, not an ordinary stutter or a postulated live value.
Its resource classification comes from the accepted source message. The
issued tag is then retained now or has a concrete subsequent release event,
including its source boundary for the remaining causal-interval proof.

No sender-publication order, common actor visit, available Rust value, or
successful final admission is a premise. Authentication, complete observation
and application/executor progress remain the declared integration contracts. -/
theorem an_owned_occurrence_receipt_has_its_actual_value_or_subsequent_release
    {plan : SourceOccurrenceCut source} {binding : Binding}
    (custody : OwnedCallHistory.History images resource choreo initialValue events cut)
    (physical : DistributedPhysicalExecution images choreo session first last
      (sourceWord events) entries published pending)
    (trace : SourceOccurrenceTrace images choreo plan (sourceWord events) calls points)
    (located : plan.path.bindings.find? (fun entry => entry.1 == occurrence) =
      some (occurrence, binding))
    (payload : binding.message.payload = .endpoint resource)
    (received : DistributedCallBoundary.receive key actor record ∈ sourceWord events)
    (callAt : occurrenceCall? calls key = some occurrence) :
    ∃ frame facts,
      OwnedCallEvent.receive key actor record frame (actor, facts.afterView.header) ∈ events ∧
      record.step.ownedFacts? (images actor) choreo = some facts ∧ facts.resource = resource ∧
      facts.side = .receive ∧ record.step.after.actor = actor ∧ record.wire.identifies frame ∧
      (TransferValueLocated (actor, facts.afterView.header) cut.custody ∨
        ∃ earlier later event before after,
          events = earlier ++ event :: later ∧
          OwnedCallEvent.receive key actor record frame (actor, facts.afterView.header) ∈ earlier ∧
          OwnedCallHistory.History images resource choreo initialValue earlier before ∧
          OwnedCallHistory.Boundary images resource choreo earlier event before after ∧
          TransferValueLocated (actor, facts.afterView.header) before.custody ∧
          event.releasedValue? = some (actor, facts.afterView.header)) := by
  have tracked : ∀ entry ∈ acceptedEntries events,
      entry.callKey = key → tracks resource choreo entry = true := by
    intro entry member keyAt
    have acceptedAt := an_acceptance_is_in_its_source_word member
    obtain ⟨acceptedOccurrence, actualCall, sourceAt⟩ :=
      an_accepted_occurrence_keeps_its_call_and_source trace entry acceptedAt
    rw [keyAt] at actualCall
    have same := Option.some.inj (actualCall.symm.trans callAt)
    subst acceptedOccurrence
    obtain ⟨eff, identity⟩ := a_matched_occurrence_has_its_exact_operation located sourceAt
    obtain ⟨facts, factsAt, resourceAt⟩ := a_checked_sender_with_owned_message_has_its_source_facts
      (a_physical_acceptance_keeps_its_source_check physical entry acceptedAt) identity payload
    simp only [tracks, factsAt, resourceAt, decide_true]
  obtain ⟨frame, facts, present, factsAt, resourceAt, receiving, actorAt, wire⟩ :=
    a_tracked_receive_has_its_actual_issued_view custody received tracked
  exact ⟨frame, facts, present, factsAt,
    resourceAt, receiving, actorAt, wire,
    a_joint_received_value_has_its_current_location_or_actual_release custody present⟩

end Hibana
