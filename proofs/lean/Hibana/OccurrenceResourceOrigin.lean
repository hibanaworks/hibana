import Hibana.SourceOccurrenceTrace
import Hibana.SourceReceiptDependency

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

/-- Later independent acceptances do not replace an existing call's origin. -/
private theorem an_occurrence_call_survives_an_append
    (found : occurrenceCall? calls key = some occurrence) (pair : (TransportChannel × Nat) × Nat) :
    occurrenceCall? (calls ++ [pair]) key = some occurrence := by
  unfold occurrenceCall? at found ⊢
  cases present : calls.find? (fun entry => decide (entry.1 = key)) with
  | none => simp only [present, Option.map_none, reduceCtorEq] at found
  | some value =>
    simpa only [List.find?_append, present, Option.orElse_some] using found

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
      an_occurrence_call_survives_an_append callAt _⟩
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

end Hibana
